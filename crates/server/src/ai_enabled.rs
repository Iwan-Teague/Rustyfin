use std::convert::Infallible;
use std::time::Instant;

use async_stream::stream;
use axum::extract::{Path, State};
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
use crate::ai_assistant::types::{
    AssistantActivityTraceItem, AssistantFollowUpContext, AssistantGroundingSource, AssistantPhase,
    AssistantPhaseEvent, AssistantStatusEvent, AssistantStatusKind, AssistantToolActivityEvent,
    AssistantToolActivityState, AssistantToolContextBlock, AssistantToolInput, AssistantTurnStats,
};
use crate::ai_assistant::{
    AssistantChatRequest, build_assistant_messages, immediate_response_for_message,
    plan_tool_calls_with_model_assist, status_label_for_tool_call,
    unsupported_write_response_for_message,
};
use crate::ai_audit::{AiAssistantAuditResponseKind, persist_chat_audit_event};
use crate::ai_conversations::ConversationMessageRequest;
use crate::ai_storage::{
    AiModelSummary, current_model_dir, list_models_with_storage_status, model_file_path,
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

#[derive(Clone)]
struct ConversationPersistence {
    conversation_id: String,
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/chat", post(chat))
        .route(
            "/conversations",
            get(crate::ai_conversations::list_conversations)
                .post(crate::ai_conversations::create_conversation),
        )
        .route(
            "/conversations/{id}",
            get(crate::ai_conversations::get_conversation)
                .patch(crate::ai_conversations::update_conversation)
                .delete(crate::ai_conversations::delete_conversation),
        )
        .route(
            "/conversations/{id}/messages/stream",
            post(stream_conversation_message),
        )
}

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<AiModelSummary>,
    inference_available: bool,
    model_storage_available: bool,
    model_storage_error: Option<String>,
}

async fn list_models(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ModelsResponse>, AppError> {
    let (models, model_storage_available, model_storage_error) =
        list_models_with_storage_status(&state).await;

    Ok(Json(ModelsResponse {
        models,
        inference_available: true,
        model_storage_available,
        model_storage_error,
    }))
}

async fn chat(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<AssistantChatRequest>,
) -> Response {
    stream_chat_response(state, user, req, None)
}

async fn stream_conversation_message(
    user: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<ConversationMessageRequest>,
) -> Result<Response, AppError> {
    let (_, _, history) = crate::ai_conversations::load_conversation_request_context(
        &state,
        &user.user_id,
        &conversation_id,
    )
    .await?;

    crate::ai_conversations::persist_user_turn(
        &state,
        &user.user_id,
        &conversation_id,
        &req.message,
    )
    .await?;

    Ok(stream_chat_response(
        state,
        user,
        AssistantChatRequest {
            model: req.model,
            message: req.message,
            history,
        },
        Some(ConversationPersistence { conversation_id }),
    ))
}

fn stream_chat_response(
    state: AppState,
    user: AuthUser,
    req: AssistantChatRequest,
    persistence: Option<ConversationPersistence>,
) -> Response {
    let model_name = req.model.clone();
    let history_len = req.history.len();
    let sse_stream = stream! {
        let trace_id = uuid::Uuid::new_v4().to_string();
        let turn_started = Instant::now();
        let chat_metrics = state.runtime_metrics.start_ai_chat_request();
        let mut audit_written = false;
        let mut assistant_content = String::new();
        let mut activity_trace = Vec::<AssistantActivityTraceItem>::new();
        let mut grounding_blocks = Vec::<AssistantToolContextBlock>::new();
        let mut grounding_sources = Vec::<AssistantGroundingSource>::new();
        let mut follow_up_contexts = Vec::<AssistantFollowUpContext>::new();
        let mut stats: Option<AssistantTurnStats> = None;
        let mut tool_duration_ms = 0_u64;

        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            username = %user.username,
            model = %model_name,
            history_len,
            "ai chat request received"
        );

        let planning_started_ts_ms = now_ts_ms();
        start_phase(
            &mut activity_trace,
            AssistantPhase::Planning,
            "Thinking...",
            planning_started_ts_ms,
        );
        yield Ok::<Event, Infallible>(sse_json_event(
            "phase",
            &AssistantPhaseEvent {
                phase: AssistantPhase::Planning,
                label: "Thinking...".to_string(),
                started_ts_ms: planning_started_ts_ms,
                finished_ts_ms: None,
            },
        ));

        if let Some(message) = unsupported_write_response_for_message(&req.message) {
            finish_phase(
                &mut activity_trace,
                AssistantPhase::Planning,
                now_ts_ms(),
            );
            yield Ok::<Event, Infallible>(sse_json_event(
                "phase",
                &AssistantPhaseEvent {
                    phase: AssistantPhase::Planning,
                    label: "Thinking...".to_string(),
                    started_ts_ms: planning_started_ts_ms,
                    finished_ts_ms: Some(now_ts_ms()),
                },
            ));
            assistant_content = message;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                0.0,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::UnsupportedWriteRefusal,
                &[],
                &[],
                &[],
                None,
            )
            .await;
            if let Some(persistence) = &persistence {
                let _ = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &[],
                    &activity_trace,
                    stats.as_ref(),
                    Some(&trace_id),
                )
                .await;
            }
            chat_metrics.mark_success();
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        }

        if let Some(message) = immediate_response_for_message(&req.message) {
            finish_phase(
                &mut activity_trace,
                AssistantPhase::Planning,
                now_ts_ms(),
            );
            yield Ok::<Event, Infallible>(sse_json_event(
                "phase",
                &AssistantPhaseEvent {
                    phase: AssistantPhase::Planning,
                    label: "Thinking...".to_string(),
                    started_ts_ms: planning_started_ts_ms,
                    finished_ts_ms: Some(now_ts_ms()),
                },
            ));
            assistant_content = message;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                0.0,
            ));
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
            if let Some(persistence) = &persistence {
                let _ = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &[],
                    &activity_trace,
                    stats.as_ref(),
                    Some(&trace_id),
                )
                .await;
            }
            chat_metrics.mark_success();
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
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

        let (engine, queue_duration_ms, model_load_duration_ms) =
            match load_engine_for_chat(&state, &model_name, &gguf_path).await {
            Ok((engine, queue_ms, load_ms)) => (engine, queue_ms, load_ms),
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

        let planner_started = Instant::now();
        let planned_tool_set =
            plan_tool_calls_with_model_assist(&engine, &user, &req.message, &req.history).await;
        let planner_duration_ms = planner_started.elapsed().as_millis() as u64;
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

        let planning_finished_ts_ms = now_ts_ms();
        finish_phase(
            &mut activity_trace,
            AssistantPhase::Planning,
            planning_finished_ts_ms,
        );
        yield Ok::<Event, Infallible>(sse_json_event(
            "phase",
            &AssistantPhaseEvent {
                phase: AssistantPhase::Planning,
                label: "Thinking...".to_string(),
                started_ts_ms: planning_started_ts_ms,
                finished_ts_ms: Some(planning_finished_ts_ms),
            },
        ));

        let context = AssistantContext::new(&user, trace_id.clone());
        if !planned_tools.is_empty() {
            let scheduled_tools = planned_tools
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, call)| {
                    let label = status_label_for_tool_call(&call);
                    let tool_id = format!("tool-{}", index + 1);
                    let started_ts_ms = now_ts_ms();
                    (call, tool_id, label, started_ts_ms)
                })
                .collect::<Vec<_>>();

            for (call, tool_id, label, started_ts_ms) in &scheduled_tools {
                let tool_event = AssistantToolActivityEvent {
                    id: tool_id.clone(),
                    tool: call.tool.as_str().to_string(),
                    label: label.clone(),
                    state: AssistantToolActivityState::Running,
                    started_ts_ms: *started_ts_ms,
                    finished_ts_ms: None,
                };
                start_tool(&mut activity_trace, &tool_event);
                yield Ok::<Event, Infallible>(sse_json_event("tool", &tool_event));
                yield Ok::<Event, Infallible>(sse_json_event(
                    "status",
                    &AssistantStatusEvent {
                        tool: call.tool.as_str(),
                        label: label.clone(),
                        kind: AssistantStatusKind::Checking,
                    },
                ));
            }

            let tool_phase_started = Instant::now();
            let tool_results = join_all(scheduled_tools.into_iter().map(|(call, tool_id, label, started_ts_ms)| {
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
                    (call, block, source, tool_id, label, started_ts_ms)
                }
            })).await;
            tool_duration_ms = tool_phase_started.elapsed().as_millis() as u64;

            for (call, block, source, tool_id, _label, started_ts_ms) in tool_results {
                let state_name = if block.status == "error" {
                    AssistantToolActivityState::Error
                } else {
                    AssistantToolActivityState::Complete
                };
                let finished_ts_ms = now_ts_ms();
                let tool_event = AssistantToolActivityEvent {
                    id: tool_id.clone(),
                    tool: call.tool.as_str().to_string(),
                    label: block.label.clone(),
                    state: state_name,
                    started_ts_ms,
                    finished_ts_ms: Some(finished_ts_ms),
                };
                finish_tool(&mut activity_trace, &tool_event);
                yield Ok::<Event, Infallible>(sse_json_event("tool", &tool_event));
                yield Ok::<Event, Infallible>(sse_json_event(
                    "status",
                    &AssistantStatusEvent {
                        tool: call.tool.as_str(),
                        label: block.label.clone(),
                        kind: match state_name {
                            AssistantToolActivityState::Running => AssistantStatusKind::Checking,
                            AssistantToolActivityState::Complete => AssistantStatusKind::Complete,
                            AssistantToolActivityState::Error => AssistantStatusKind::Error,
                        },
                    },
                ));
                follow_up_contexts.push(build_follow_up_context(&call, &block));
                grounding_blocks.push(block);
                grounding_sources.push(source);
            }

            if !grounding_sources.is_empty() {
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .event("grounding")
                        .data(json!({
                            "sources": &grounding_sources,
                            "follow_up_contexts": &follow_up_contexts,
                        }).to_string()),
                );
            }
        }

        let generation_phase_started_ms = now_ts_ms();
        let generation_started = Instant::now();
        start_phase(
            &mut activity_trace,
            AssistantPhase::Generating,
            "Thinking...",
            generation_phase_started_ms,
        );
        yield Ok::<Event, Infallible>(sse_json_event(
            "phase",
            &AssistantPhaseEvent {
                phase: AssistantPhase::Generating,
                label: "Thinking...".to_string(),
                started_ts_ms: generation_phase_started_ms,
                finished_ts_ms: None,
            },
        ));

        let messages = build_assistant_messages(req.clone(), &grounding_blocks);
        let raw_stream = engine.chat_stream(messages, rustfin_ai_agent::SamplingParams::default());
        futures::pin_mut!(raw_stream);

        while let Some(chunk) = raw_stream.next().await {
            match chunk {
                Ok(rustfin_ai_agent::ChatChunk::Token(text)) => {
                    assistant_content.push_str(&text);
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("token")
                            .data(json!({ "text": text }).to_string()),
                    );
                }
                Ok(rustfin_ai_agent::ChatChunk::Stats {
                    prompt_tokens,
                    completion_tokens,
                    total_duration_ms,
                    tokens_per_second,
                }) => {
                    let generating_finished_ts_ms = now_ts_ms();
                    finish_phase(
                        &mut activity_trace,
                        AssistantPhase::Generating,
                        generating_finished_ts_ms,
                    );
                    yield Ok::<Event, Infallible>(sse_json_event(
                        "phase",
                        &AssistantPhaseEvent {
                            phase: AssistantPhase::Generating,
                            label: "Thinking...".to_string(),
                            started_ts_ms: generation_phase_started_ms,
                            finished_ts_ms: Some(generating_finished_ts_ms),
                        },
                    ));
                    stats = Some(build_turn_stats(
                        prompt_tokens,
                        completion_tokens,
                        total_duration_ms,
                        planner_duration_ms,
                        tool_duration_ms,
                        turn_started.elapsed().as_millis() as u64,
                        queue_duration_ms,
                        model_load_duration_ms,
                        tokens_per_second,
                    ));
                    yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
                }
                Ok(rustfin_ai_agent::ChatChunk::Done) => {
                    if stats.is_none() {
                        let generation_duration_ms =
                            generation_started.elapsed().as_millis() as u64;
                        let generating_finished_ts_ms = now_ts_ms();
                        finish_phase(
                            &mut activity_trace,
                            AssistantPhase::Generating,
                            generating_finished_ts_ms,
                        );
                        yield Ok::<Event, Infallible>(sse_json_event(
                            "phase",
                            &AssistantPhaseEvent {
                                phase: AssistantPhase::Generating,
                                label: "Thinking...".to_string(),
                                started_ts_ms: generation_phase_started_ms,
                                finished_ts_ms: Some(generating_finished_ts_ms),
                            },
                        ));
                        stats = Some(build_turn_stats(
                            0,
                            0,
                            generation_duration_ms,
                            planner_duration_ms,
                            tool_duration_ms,
                            turn_started.elapsed().as_millis() as u64,
                            queue_duration_ms,
                            model_load_duration_ms,
                            0.0,
                        ));
                        yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
                    }

                    if let Some(persistence) = &persistence {
                        let grounding_tools = planned_tools
                            .iter()
                            .map(|call| call.tool.as_str().to_string())
                            .collect::<Vec<_>>();
                        let _ = crate::ai_conversations::persist_assistant_turn(
                            &state,
                            &user.user_id,
                            &persistence.conversation_id,
                            &assistant_content,
                            &model_name,
                            &grounding_tools,
                            &follow_up_contexts,
                            &grounding_sources,
                            &activity_trace,
                            stats.as_ref(),
                            Some(&trace_id),
                        )
                        .await;
                    }

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
                    yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
                }
                Err(error) => {
                    let error_message = error.to_string();
                    let finished_ts_ms = now_ts_ms();
                    finish_phase(
                        &mut activity_trace,
                        AssistantPhase::Generating,
                        finished_ts_ms,
                    );
                    yield Ok::<Event, Infallible>(sse_json_event(
                        "phase",
                        &AssistantPhaseEvent {
                            phase: AssistantPhase::Generating,
                            label: "Thinking...".to_string(),
                            started_ts_ms: generation_phase_started_ms,
                            finished_ts_ms: Some(finished_ts_ms),
                        },
                    ));
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
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("error")
                            .data(json!({ "message": error_message }).to_string()),
                    );
                }
            }
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
) -> Result<(rustfin_ai_agent::LlamaEngine, u64, u64), String> {
    let queue_started = Instant::now();
    let mut guard = state.engine.lock().await;
    let queue_duration_ms = queue_started.elapsed().as_millis() as u64;
    let needs_reload = guard.engine.is_none() || guard.loaded_model.as_deref() != Some(model_name);

    let load_started = Instant::now();
    if needs_reload {
        let engine = rustfin_ai_agent::LlamaEngine::load(
            gguf_path,
            rustfin_ai_agent::LlamaEngineParams::default(),
        )
        .map_err(|error| format!("failed to load model {}: {error}", gguf_path.display()))?;
        guard.loaded_model = Some(model_name.to_string());
        guard.engine = Some(engine);
    }
    let model_load_duration_ms = load_started.elapsed().as_millis() as u64;

    let engine = guard
        .engine
        .clone()
        .ok_or_else(|| "no inference engine loaded".to_string())?;
    Ok((engine, queue_duration_ms, model_load_duration_ms))
}

fn sse_json_event<T: Serialize>(event_type: &str, payload: &T) -> Event {
    Event::default()
        .event(event_type)
        .data(serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string()))
}

fn now_ts_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn start_phase(
    activity_trace: &mut Vec<AssistantActivityTraceItem>,
    phase: AssistantPhase,
    label: &str,
    started_ts_ms: i64,
) {
    activity_trace.push(AssistantActivityTraceItem::Phase {
        phase,
        label: label.to_string(),
        started_ts_ms,
        finished_ts_ms: None,
    });
}

fn finish_phase(
    activity_trace: &mut Vec<AssistantActivityTraceItem>,
    phase: AssistantPhase,
    finished_ts_ms: i64,
) {
    if let Some(AssistantActivityTraceItem::Phase { finished_ts_ms: finished, .. }) = activity_trace
        .iter_mut()
        .rev()
        .find(|item| matches!(item, AssistantActivityTraceItem::Phase { phase: item_phase, finished_ts_ms: None, .. } if *item_phase == phase))
    {
        *finished = Some(finished_ts_ms);
    }
}

fn start_tool(
    activity_trace: &mut Vec<AssistantActivityTraceItem>,
    event: &AssistantToolActivityEvent,
) {
    activity_trace.push(AssistantActivityTraceItem::Tool {
        id: event.id.clone(),
        tool: event.tool.clone(),
        label: event.label.clone(),
        state: event.state,
        started_ts_ms: event.started_ts_ms,
        finished_ts_ms: event.finished_ts_ms,
    });
}

fn finish_tool(
    activity_trace: &mut Vec<AssistantActivityTraceItem>,
    event: &AssistantToolActivityEvent,
) {
    if let Some(AssistantActivityTraceItem::Tool {
        label,
        state,
        finished_ts_ms,
        ..
    }) = activity_trace
        .iter_mut()
        .find(|item| matches!(item, AssistantActivityTraceItem::Tool { id, .. } if id == &event.id))
    {
        *label = event.label.clone();
        *state = event.state;
        *finished_ts_ms = event.finished_ts_ms;
    }
}

fn build_turn_stats(
    prompt_tokens: u64,
    completion_tokens: u64,
    generation_duration_ms: u64,
    planner_duration_ms: u64,
    tool_duration_ms: u64,
    end_to_end_duration_ms: u64,
    queue_duration_ms: u64,
    model_load_duration_ms: u64,
    tokens_per_second: f64,
) -> AssistantTurnStats {
    AssistantTurnStats {
        prompt_tokens: clamp_token_count(prompt_tokens),
        completion_tokens: clamp_token_count(completion_tokens),
        total_duration_ms: generation_duration_ms,
        generation_duration_ms,
        planner_duration_ms,
        tool_duration_ms,
        end_to_end_duration_ms,
        queue_duration_ms,
        model_load_duration_ms,
        tokens_per_second,
    }
}

fn clamp_token_count(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn tool_input_summary(input: &AssistantToolInput) -> String {
    match input {
        AssistantToolInput::None => "none".to_string(),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
            query,
        } => format!(
            "calendar:{label}:{from_date}->{to_date}:query={}",
            query.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::ChannelsFilter { query } => {
            format!("channels:query={}", query.as_deref().unwrap_or("*"))
        }
        AssistantToolInput::DownloadsFilter {
            query,
            availability,
        } => format!(
            "downloads:query={}:availability={}",
            query.as_deref().unwrap_or("*"),
            availability.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::LibrarySearch { query } => format!("library_query:{query}"),
        AssistantToolInput::LibraryRecent { query } => {
            format!("library_recent:query={}", query.as_deref().unwrap_or("*"))
        }
        AssistantToolInput::Weather {
            location,
            forecast_days,
        } => format!(
            "weather:location={}:days={}",
            location,
            forecast_days
                .map(|days| days.to_string())
                .unwrap_or_else(|| "current".to_string())
        ),
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
