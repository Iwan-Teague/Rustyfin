use std::convert::Infallible;

use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{StreamExt, future::join_all};
use serde::Serialize;
use serde_json::json;
use tracing::{info, warn};

use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::tools::{build_follow_up_context, execute_tool, source_from_block};
use crate::ai_assistant::types::{AssistantToolContextBlock, AssistantToolInput};
use crate::ai_assistant::{
    AssistantChatRequest, AssistantStatusEvent, AssistantStatusKind, build_assistant_messages,
    immediate_response_for_message, plan_tool_calls_with_model_assist, status_label_for_tool_call,
};
use crate::ai_audit::{AiAssistantAuditResponseKind, persist_chat_audit_event};
use crate::ai_storage::{
    AiModelSummary, current_model_dir, list_models_from_state, model_file_path,
};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
        }
    }
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/chat", post(chat))
}

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<AiModelSummary>,
    inference_available: bool,
}

async fn list_models(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models = list_models_from_state(&state).await?;

    Ok(Json(ModelsResponse {
        models,
        inference_available: true,
    }))
}

async fn chat(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<AssistantChatRequest>,
) -> Response {
    let model_name = req.model.clone();
    let sse_stream = stream! {
        let trace_id = uuid::Uuid::new_v4().to_string();
        let chat_metrics = state.runtime_metrics.start_ai_chat_request();
        let mut audit_written = false;
        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            username = %user.username,
            model = %model_name,
            history_len = req.history.len(),
            "ai chat request received"
        );

        if let Some(message) = immediate_response_for_message(&req.message) {
            info!(
                trace_id = %trace_id,
                user_id = %user.user_id,
                response_kind = "clarification",
                "ai chat short-circuited to clarification"
            );
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Clarification,
                &[],
                &[],
                &[],
                None,
            )
            .await;
            chat_metrics.mark_success();
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": message }).to_string()),
            );
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        }

        let model_dir = current_model_dir(&state).await;
        let gguf_path = match model_file_path(&model_dir, &model_name) {
            Ok(path) => path,
            Err(error) => {
                let error_message = error.to_string();
                persist_chat_audit_event(
                    &state,
                    &user,
                    &req,
                    &trace_id,
                    AiAssistantAuditResponseKind::ModelPathError,
                    &[],
                    &[],
                    &[],
                    Some(&error_message),
                )
                .await;
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .event("error")
                        .data(json!({ "message": error_message }).to_string()),
                );
                return;
            }
        };

        let engine = match load_engine_for_chat(&state, &model_name, &gguf_path).await {
            Ok(engine) => engine,
            Err(error_message) => {
                persist_chat_audit_event(
                    &state,
                    &user,
                    &req,
                    &trace_id,
                    AiAssistantAuditResponseKind::ModelLoadError,
                    &[],
                    &[],
                    &[],
                    Some(&error_message),
                )
                .await;
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .event("error")
                        .data(json!({ "message": error_message }).to_string()),
                );
                warn!(
                    trace_id = %trace_id,
                    user_id = %user.user_id,
                    model = %model_name,
                    error = %error_message,
                    "ai chat failed to load model"
                );
                return;
            }
        };

        let planned_tool_set =
            plan_tool_calls_with_model_assist(&engine, &user, &req.message, &req.history).await;
        let planned_tools = planned_tool_set.calls;
        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            planner_mode = planned_tool_set.mode.as_str(),
            planned_tool_count = planned_tools.len(),
            planned_tools = %planned_tools
                .iter()
                .map(|call| call.tool.as_str())
                .collect::<Vec<_>>()
                .join(","),
            "ai chat planned grounded tools"
        );
        for call in &planned_tools {
            let event = AssistantStatusEvent {
                tool: call.tool.as_str(),
                label: status_label_for_tool_call(call),
                kind: AssistantStatusKind::Checking,
            };
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("status")
                    .data(serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())),
            );
        }

        let context = AssistantContext::new(&user, trace_id.clone());
        let tool_results = join_all(planned_tools.iter().cloned().map(|call| {
            let context = context.clone();
            let state = state.clone();
            async move {
                let tool_metrics = state.runtime_metrics.start_ai_tool_call();
                let block = execute_tool(&state, &context, &call).await;
                if block.status == "ok" {
                    tool_metrics.mark_success();
                    info!(
                        trace_id = %context.trace_id,
                        user_id = %context.user_id,
                        tool = call.tool.as_str(),
                        input = %tool_input_summary(&call.input),
                        result = %tool_result_summary(&block),
                        "ai grounded tool completed"
                    );
                } else {
                    warn!(
                        trace_id = %context.trace_id,
                        user_id = %context.user_id,
                        tool = call.tool.as_str(),
                        input = %tool_input_summary(&call.input),
                        result = %tool_result_summary(&block),
                        "ai grounded tool failed"
                    );
                }
                let source = source_from_block(call.tool, &block);
                (call, block, source)
            }
        })).await;

        let mut grounding_blocks = Vec::new();
        let mut grounding_sources = Vec::new();
        let mut follow_up_contexts = Vec::new();
        for (call, block, source) in tool_results {
            let kind = if block.status == "error" {
                AssistantStatusKind::Error
            } else {
                AssistantStatusKind::Complete
            };
            let status_event = AssistantStatusEvent {
                tool: call.tool.as_str(),
                label: block.label.clone(),
                kind,
            };
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("status")
                    .data(serde_json::to_string(&status_event).unwrap_or_else(|_| "{}".to_string())),
            );
            follow_up_contexts.push(build_follow_up_context(&call, &block));
            grounding_blocks.push(block);
            grounding_sources.push(source);
        }

        if !grounding_sources.is_empty() {
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("grounding")
                    .data(json!({
                        "sources": grounding_sources,
                        "follow_up_contexts": follow_up_contexts,
                    }).to_string()),
            );
        }

        let messages = build_assistant_messages(req.clone(), &grounding_blocks);
        let raw_stream = engine.chat_stream(
            messages,
            rustfin_ai_agent::SamplingParams::default(),
        );
        futures::pin_mut!(raw_stream);

        while let Some(chunk) = raw_stream.next().await {
            let event = match chunk {
                Ok(rustfin_ai_agent::ChatChunk::Token(text)) => Event::default()
                    .event("token")
                    .data(json!({ "text": text }).to_string()),
                Ok(rustfin_ai_agent::ChatChunk::Stats {
                    prompt_tokens,
                    completion_tokens,
                    total_duration_ms,
                    tokens_per_second,
                }) => Event::default().event("stats").data(
                    json!({
                        "prompt_tokens": prompt_tokens,
                        "completion_tokens": completion_tokens,
                        "total_duration_ms": total_duration_ms,
                        "tokens_per_second": tokens_per_second,
                    })
                    .to_string(),
                ),
                Ok(rustfin_ai_agent::ChatChunk::Done) => {
                    persist_chat_audit_event(
                        &state,
                        &user,
                        &req,
                        &trace_id,
                        AiAssistantAuditResponseKind::Completed,
                        &planned_tools,
                        &grounding_blocks,
                        &grounding_sources,
                        None,
                    )
                    .await;
                    chat_metrics.mark_success();
                    info!(
                        trace_id = %trace_id,
                        user_id = %user.user_id,
                        model = %model_name,
                        grounded_tool_count = grounding_blocks.len(),
                        "ai chat completed"
                    );
                    Event::default().event("done").data("{}")
                }
                Err(error) => {
                    let error_message = error.to_string();
                    if !audit_written {
                        persist_chat_audit_event(
                            &state,
                            &user,
                            &req,
                            &trace_id,
                            AiAssistantAuditResponseKind::StreamError,
                            &planned_tools,
                            &grounding_blocks,
                            &grounding_sources,
                            Some(&error_message),
                        )
                        .await;
                        audit_written = true;
                    }
                    warn!(
                        trace_id = %trace_id,
                        user_id = %user.user_id,
                        model = %model_name,
                        error = %error_message,
                        "ai chat stream failed"
                    );
                    Event::default()
                        .event("error")
                        .data(json!({ "message": error_message }).to_string())
                }
            };
            yield Ok::<Event, Infallible>(event);
        }
    };

    Sse::new(Box::pin(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn load_engine_for_chat(
    state: &AppState,
    model_name: &str,
    gguf_path: &std::path::Path,
) -> Result<rustfin_ai_agent::LlamaEngine, String> {
    let mut guard = state.engine.lock().await;
    let needs_reload = guard.engine.is_none() || guard.loaded_model.as_deref() != Some(model_name);

    if needs_reload {
        let engine = rustfin_ai_agent::LlamaEngine::load(
            gguf_path,
            rustfin_ai_agent::LlamaEngineParams::default(),
        )
        .map_err(|error| format!("failed to load model {}: {error}", gguf_path.display()))?;
        guard.loaded_model = Some(model_name.to_string());
        guard.engine = Some(engine);
    }

    guard
        .engine
        .clone()
        .ok_or_else(|| "no inference engine loaded".to_string())
}

fn tool_input_summary(input: &AssistantToolInput) -> String {
    match input {
        AssistantToolInput::None => "none".to_string(),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
        } => format!("calendar:{label}:{from_date}->{to_date}"),
        AssistantToolInput::DownloadsFilter {
            query,
            availability,
        } => format!(
            "downloads:query={}:availability={}",
            query.as_deref().unwrap_or("*"),
            availability.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::LibrarySearch { query } => format!("library_query:{query}"),
        AssistantToolInput::WebSearch { query } => format!("web_search:{query}"),
        AssistantToolInput::WebFetch { url } => format!("web_fetch:{url}"),
        AssistantToolInput::RoomsFilter { room_mode, query } => format!(
            "rooms:mode={}:query={}",
            room_mode.as_deref().unwrap_or("*"),
            query.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::ServerFilter {
            query,
            availability,
        } => format!(
            "servers:query={}:availability={}",
            query.as_deref().unwrap_or("*"),
            availability.as_deref().unwrap_or("*")
        ),
    }
}

fn tool_result_summary(block: &AssistantToolContextBlock) -> String {
    let count = block
        .data
        .get("total_count")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            block
                .data
                .get("match_count")
                .and_then(serde_json::Value::as_u64)
        })
        .or_else(|| {
            ["events", "birthdays", "rooms", "servers", "matches"]
                .iter()
                .find_map(|key| {
                    block
                        .data
                        .get(key)
                        .and_then(serde_json::Value::as_array)
                        .map(|items: &Vec<serde_json::Value>| items.len() as u64)
                })
        });

    match count {
        Some(count) => format!(
            "status={}, label={}, count={count}",
            block.status, block.label
        ),
        None => format!("status={}, label={}", block.status, block.label),
    }
}
