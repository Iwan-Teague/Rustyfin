use std::convert::Infallible;
use std::path::Path as StdPath;
use std::time::Instant;

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{StreamExt, future::join_all};
use serde::Serialize;
use serde_json::json;
use tracing::{info, warn};

use crate::ai_assistant::confirmation::{
    CONFIRMATION_TOKEN_TTL_SECS, pending_action_request_for_message_with_state,
};
use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::tools::{build_follow_up_context, execute_tool, source_from_block};
use crate::ai_assistant::types::{
    AssistantActivityTraceItem, AssistantConfirmationPayload, AssistantConfirmationRequiredEvent,
    AssistantFollowUpContext, AssistantGroundingSource, AssistantPendingAction,
    AssistantPendingActionStatus, AssistantPhase, AssistantPhaseEvent, AssistantResponseMode,
    AssistantRuntimePhase, AssistantStatusEvent, AssistantStatusKind, AssistantToolActivityEvent,
    AssistantToolActivityState, AssistantToolContextBlock, AssistantToolInput, AssistantTurnStats,
};
use crate::ai_assistant::weather::deterministic_weather_reply;
use crate::ai_assistant::{
    AssistantChatRequest, build_assistant_messages, deterministic_calendar_reply,
    deterministic_current_datetime_reply, deterministic_network_reply, deterministic_rooms_reply,
    immediate_response_for_message, plan_tool_calls_with_model_assist, status_label_for_tool_call,
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
    pub active_phase: AssistantRuntimePhase,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
            active_phase: AssistantRuntimePhase::Idle,
        }
    }
}

#[derive(Clone)]
struct ConversationPersistence {
    conversation_id: String,
}

const MIN_CONTEXT_WINDOW_TOKENS: u32 = 1024;
const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 4096;
const MAX_CONTEXT_WINDOW_TOKENS: u32 = 32768;
const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 192;
const COMPACTION_MAX_RECENT_HISTORY_MESSAGES: usize = 8;
const COMPACTION_MIN_RECENT_HISTORY_MESSAGES: usize = 0;
const COMPACTION_SUMMARY_TARGET_TOKENS: u32 = 320;
const COMPACTION_SUMMARY_FALLBACK_TARGET_TOKENS: [u32; 3] = [320, 192, 128];
const COMPACTION_SUMMARY_MAX_DURATION_MS: u64 = 15_000;
const COMPACTION_MESSAGE_CHAR_LIMIT: usize = 1200;

#[derive(Debug, Clone)]
struct PreparedGenerationMessages {
    messages: Vec<rustfin_ai_agent::ChatMessage>,
    compacted: bool,
    original_prompt_tokens: u32,
    final_prompt_tokens: u32,
    retained_history_messages: usize,
    summarized_history_messages: usize,
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/runtime", get(crate::ai_runtime::get_ai_runtime))
        .route(
            "/transcribe",
            post(crate::ai_transcribe::transcribe_audio).layer(DefaultBodyLimit::max(
                crate::ai_transcribe::MAX_AI_TRANSCRIBE_BYTES,
            )),
        )
        .route("/chat", post(chat))
        .route(
            "/artifacts/{id}/download",
            get(crate::ai_generated_artifacts::download_generated_artifact),
        )
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
            "/conversations/{id}/move",
            post(crate::ai_conversations::move_conversation),
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
    AxumPath(conversation_id): AxumPath<String>,
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
            response_mode: req.response_mode,
            confirmation_token: req.confirmation_token,
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
        set_engine_phase(&state, AssistantRuntimePhase::Planning).await;
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

        if let Some(confirmation_token) = req.confirmation_token.as_deref() {
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

            let payload = match load_confirmation_payload(&state, &user, confirmation_token).await {
                Ok(payload) => payload,
                Err(message) => {
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
                            None,
                            Some(&trace_id),
                        )
                        .await;
                    }
                    chat_metrics.mark_success();
                    set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("token")
                            .data(json!({ "text": assistant_content }).to_string()),
                    );
                    yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
                    yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
                    return;
                }
            };

            let confirmed_context = AssistantContext::new(&user, trace_id.clone())
                .with_conversation_id(payload.conversation_id.as_deref())
                .with_confirmed_write_tool(payload.call.tool.as_str());
            let planned_tools = vec![payload.call.clone()];
            set_engine_phase(&state, AssistantRuntimePhase::Grounding).await;
            let tool_id = "tool-1".to_string();
            let running_label = status_label_for_tool_call(&payload.call);
            let tool_started_ts_ms = now_ts_ms();
            let running_event = AssistantToolActivityEvent {
                id: tool_id.clone(),
                tool: payload.call.tool.as_str().to_string(),
                label: running_label.clone(),
                state: AssistantToolActivityState::Running,
                started_ts_ms: tool_started_ts_ms,
                finished_ts_ms: None,
            };
            start_tool(&mut activity_trace, &running_event);
            yield Ok::<Event, Infallible>(sse_json_event("tool", &running_event));
            yield Ok::<Event, Infallible>(sse_json_event(
                "status",
                &AssistantStatusEvent {
                    tool: payload.call.tool.as_str(),
                    label: running_label,
                    kind: AssistantStatusKind::Checking,
                },
            ));

            let tool_started = Instant::now();
            let block = execute_tool(&state, &confirmed_context, &payload.call).await;
            tool_duration_ms = tool_started.elapsed().as_millis() as u64;
            let source = source_from_block(payload.call.tool, &block);
            let follow_up_context = build_follow_up_context(&payload.call, &block);
            grounding_blocks.push(block.clone());
            grounding_sources.push(source.clone());
            follow_up_contexts.push(follow_up_context);

            let tool_state = if block.status == "error" {
                AssistantToolActivityState::Error
            } else {
                AssistantToolActivityState::Complete
            };
            let finished_ts_ms = now_ts_ms();
            let finished_event = AssistantToolActivityEvent {
                id: tool_id,
                tool: payload.call.tool.as_str().to_string(),
                label: block.label.clone(),
                state: tool_state,
                started_ts_ms: tool_started_ts_ms,
                finished_ts_ms: Some(finished_ts_ms),
            };
            finish_tool(&mut activity_trace, &finished_event);
            yield Ok::<Event, Infallible>(sse_json_event("tool", &finished_event));
            yield Ok::<Event, Infallible>(sse_json_event(
                "status",
                &AssistantStatusEvent {
                    tool: payload.call.tool.as_str(),
                    label: block.label.clone(),
                    kind: match tool_state {
                        AssistantToolActivityState::Running => AssistantStatusKind::Checking,
                        AssistantToolActivityState::Complete => AssistantStatusKind::Complete,
                        AssistantToolActivityState::Error => AssistantStatusKind::Error,
                    },
                },
            ));
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("grounding")
                    .data(json!({
                        "sources": &grounding_sources,
                        "follow_up_contexts": &follow_up_contexts,
                    }).to_string()),
            );

            if block.status == "ok" {
                if let Err(message) =
                    consume_confirmation_payload(&state, &user, confirmation_token, &payload).await
                {
                    assistant_content = message;
                } else {
                    assistant_content = assistant_text_for_confirmed_action(&block);
                }
            } else {
                assistant_content = assistant_text_for_confirmed_action(&block);
            }

            stats = Some(build_turn_stats(
                0,
                0,
                0,
                0,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                0.0,
            ));

            if let Some(persistence) = &persistence {
                let grounding_tool_names = planned_tools
                    .iter()
                    .map(|call| call.tool.as_str().to_string())
                    .collect::<Vec<_>>();
                let _ = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tool_names,
                    &follow_up_contexts,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        }

        if let Some(result) = pending_action_request_for_message_with_state(
            &state,
            &user,
            &req.message,
            persistence.as_ref().map(|value| value.conversation_id.as_str()),
            &req.model,
        )
        .await
        {
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

            let mut pending_action: Option<AssistantPendingAction> = None;
            assistant_content = match result {
                Ok(parsed) => {
                    let expires_ts = chrono::Utc::now().timestamp() + CONFIRMATION_TOKEN_TTL_SECS;
                    match store_confirmation_payload(&state, &user, &parsed.payload, expires_ts).await {
                        Ok(token) => {
                            let event = AssistantConfirmationRequiredEvent {
                                token: token.id.clone(),
                                action_kind: parsed.payload.action_kind,
                                summary: parsed.payload.summary.clone(),
                                expires_ts: token.expires_ts,
                            };
                            pending_action = Some(AssistantPendingAction {
                                token: token.id.clone(),
                                action_kind: parsed.payload.action_kind,
                                summary: parsed.payload.summary.clone(),
                                expires_ts: token.expires_ts,
                                status: AssistantPendingActionStatus::Pending,
                            });
                            yield Ok::<Event, Infallible>(sse_json_event(
                                "confirmation_required",
                                &event,
                            ));
                            format!("{} Reply with \"Confirm\" to continue.", parsed.payload.summary)
                        }
                        Err(message) => message,
                    }
                }
                Err(message) => message,
            };
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
                    pending_action.as_ref(),
                    Some(&trace_id),
                )
                .await;
            }
            chat_metrics.mark_success();
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        }

        if let Some(message) = unsupported_write_response_for_message(&req.message) {
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
                    None,
                    Some(&trace_id),
                )
                .await;
            }
            chat_metrics.mark_success();
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
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
                    None,
                    Some(&trace_id),
                )
                .await;
            }
            chat_metrics.mark_success();
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
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
                set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
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
                set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
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

        let context = AssistantContext::new(&user, trace_id.clone()).with_conversation_id(
            persistence.as_ref().map(|value| value.conversation_id.as_str()),
        );
        if !planned_tools.is_empty() {
            set_engine_phase(&state, AssistantRuntimePhase::Grounding).await;
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

        if let Some(datetime_reply) =
            deterministic_current_datetime_reply(&req.message, &req.history, &grounding_blocks)
        {
            assistant_content = datetime_reply;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                0.0,
            ));
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
            if let Some(persistence) = &persistence {
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
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        } else if let Some(calendar_reply) =
            deterministic_calendar_reply(&req.message, &grounding_blocks)
        {
            assistant_content = calendar_reply;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                0.0,
            ));
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
            if let Some(persistence) = &persistence {
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
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        } else if let Some(network_reply) =
            deterministic_network_reply(&req.message, &grounding_blocks)
        {
            assistant_content = network_reply;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                0.0,
            ));
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
            if let Some(persistence) = &persistence {
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
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        } else if let Some(rooms_reply) =
            deterministic_rooms_reply(&req.message, &grounding_blocks)
        {
            assistant_content = rooms_reply;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                0.0,
            ));
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
            if let Some(persistence) = &persistence {
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
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        } else if let Some(weather_reply) =
            deterministic_weather_reply(&req.message, &grounding_blocks)
        {
            assistant_content = weather_reply;
            stats = Some(build_turn_stats(
                0,
                0,
                0,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                0.0,
            ));
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
            if let Some(persistence) = &persistence {
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
                    None,
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
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": assistant_content }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            return;
        }

        let generation_phase_started_ms = now_ts_ms();
        let generation_started = Instant::now();
        set_engine_phase(&state, AssistantRuntimePhase::Generating).await;
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

        let prepared_messages = match prepare_generation_messages(&engine, &req, &grounding_blocks).await {
            Ok(prepared) => prepared,
            Err(error_message) => {
                persist_chat_audit_event(
                    &state,
                    &user,
                    &req,
                    &trace_id,
                    AiAssistantAuditResponseKind::ModelLoadError,
                    &planned_tools,
                    &grounding_blocks,
                    &grounding_sources,
                    Some(&error_message),
                )
                .await;
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .event("error")
                        .data(json!({ "message": error_message }).to_string()),
                );
                set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
                warn!(
                    trace_id = %trace_id,
                    user_id = %user.user_id,
                    model = %model_name,
                    error = %error_message,
                    "ai chat failed while preparing compacted context"
                );
                return;
            }
        };
        if prepared_messages.compacted {
            info!(
                trace_id = %trace_id,
                user_id = %user.user_id,
                model = %model_name,
                original_prompt_tokens = prepared_messages.original_prompt_tokens,
                final_prompt_tokens = prepared_messages.final_prompt_tokens,
                retained_history_messages = prepared_messages.retained_history_messages,
                summarized_history_messages = prepared_messages.summarized_history_messages,
                context_window_tokens = engine.params().n_ctx,
                "ai chat compacted conversation history to fit context window"
            );
        }

        let raw_stream = engine.chat_stream(
            prepared_messages.messages,
            sampling_params_for_response_mode(req.response_mode),
        );
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
                            None,
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
                    set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
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
                    set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
                }
            }
        }
    };

    Sse::new(Box::pin(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}

pub(crate) async fn load_engine_for_chat(
    state: &AppState,
    model_name: &str,
    gguf_path: &std::path::Path,
) -> Result<(rustfin_ai_agent::LlamaEngine, u64, u64), String> {
    let queue_started = Instant::now();
    let mut guard = state.engine.lock().await;
    let queue_duration_ms = queue_started.elapsed().as_millis() as u64;
    let needs_reload = guard.engine.is_none() || guard.loaded_model.as_deref() != Some(model_name);
    guard.active_phase = AssistantRuntimePhase::LoadingModel;

    let load_started = Instant::now();
    if needs_reload {
        let engine =
            rustfin_ai_agent::LlamaEngine::load(gguf_path, engine_params_for_model(gguf_path))
                .map_err(|error| {
                    format!("failed to load model {}: {error}", gguf_path.display())
                })?;
        guard.loaded_model = Some(model_name.to_string());
        guard.engine = Some(engine);
    }
    let model_load_duration_ms = load_started.elapsed().as_millis() as u64;

    let engine = guard
        .engine
        .clone()
        .ok_or_else(|| "no inference engine loaded".to_string())?;
    guard.active_phase = AssistantRuntimePhase::Idle;
    Ok((engine, queue_duration_ms, model_load_duration_ms))
}

async fn set_engine_phase(state: &AppState, phase: AssistantRuntimePhase) {
    let mut guard = state.engine.lock().await;
    guard.active_phase = phase;
}

fn engine_params_from_env() -> rustfin_ai_agent::LlamaEngineParams {
    let mut params = rustfin_ai_agent::LlamaEngineParams::default();
    params.split_mode = parse_gpu_split_mode_from_env();
    params.main_gpu = parse_i32_env("RUSTFIN_AI_GPU_MAIN_DEVICE");
    params.device_indices = parse_device_indices_env("RUSTFIN_AI_GPU_DEVICES");
    params
}

fn engine_params_for_model(gguf_path: &StdPath) -> rustfin_ai_agent::LlamaEngineParams {
    let mut params = engine_params_from_env();
    let requested_context = parse_u32_env("RUSTFIN_AI_CONTEXT_LENGTH");
    let model_context = match rustfin_ai_agent::ModelStore::inspect_file(gguf_path) {
        Ok(info) => info.context_length,
        Err(error) => {
            warn!(
                model = %gguf_path.display(),
                %error,
                "failed to inspect model context length; falling back to host heuristics"
            );
            None
        }
    };
    let host_memory = detect_host_total_memory_bytes();
    params.n_ctx = resolve_effective_context_window(requested_context, model_context, host_memory);
    params
}

fn parse_gpu_split_mode_from_env() -> rustfin_ai_agent::engine::LlamaGpuSplitMode {
    parse_gpu_split_mode_override(std::env::var("RUSTFIN_AI_GPU_SPLIT_MODE").ok().as_deref())
}

fn parse_gpu_split_mode_override(
    value: Option<&str>,
) -> rustfin_ai_agent::engine::LlamaGpuSplitMode {
    match value
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("none") => rustfin_ai_agent::engine::LlamaGpuSplitMode::None,
        Some("row") => rustfin_ai_agent::engine::LlamaGpuSplitMode::Row,
        Some("layer") | None | Some("") => rustfin_ai_agent::engine::LlamaGpuSplitMode::Layer,
        Some(other) => {
            warn!(
                value = %other,
                "ignoring unsupported RUSTFIN_AI_GPU_SPLIT_MODE; expected one of none|layer|row"
            );
            rustfin_ai_agent::engine::LlamaGpuSplitMode::Layer
        }
    }
}

fn parse_i32_env(name: &str) -> Option<i32> {
    parse_i32_override(name, std::env::var(name).ok().as_deref())
}

fn parse_u32_env(name: &str) -> Option<u32> {
    parse_u32_override(name, std::env::var(name).ok().as_deref())
}

fn parse_i32_override(name: &str, value: Option<&str>) -> Option<i32> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.parse::<i32>() {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            warn!(env = name, value = trimmed, %error, "ignoring invalid integer env override");
            None
        }
    }
}

fn parse_u32_override(name: &str, value: Option<&str>) -> Option<u32> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.parse::<u32>() {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            warn!(env = name, value = trimmed, %error, "ignoring invalid unsigned integer env override");
            None
        }
    }
}

fn detect_host_total_memory_bytes() -> Option<u64> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total = system.total_memory();
    (total > 0).then_some(total)
}

fn suggested_context_window_from_host_memory(total_memory_bytes: Option<u64>) -> Option<u32> {
    let gib = total_memory_bytes? / (1024 * 1024 * 1024);
    Some(match gib {
        0..=15 => 4096,
        16..=31 => 8192,
        32..=63 => 16384,
        _ => 32768,
    })
}

fn resolve_effective_context_window(
    requested_context: Option<u32>,
    model_context: Option<u32>,
    host_total_memory_bytes: Option<u64>,
) -> u32 {
    let hardware_cap = suggested_context_window_from_host_memory(host_total_memory_bytes);
    let mut resolved = requested_context
        .or(hardware_cap)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS)
        .clamp(MIN_CONTEXT_WINDOW_TOKENS, MAX_CONTEXT_WINDOW_TOKENS);

    if let Some(hardware_cap) = hardware_cap {
        resolved =
            resolved.min(hardware_cap.clamp(MIN_CONTEXT_WINDOW_TOKENS, MAX_CONTEXT_WINDOW_TOKENS));
    }
    if let Some(model_context) = model_context {
        resolved = resolved.min(model_context.max(MIN_CONTEXT_WINDOW_TOKENS));
    }

    resolved.clamp(MIN_CONTEXT_WINDOW_TOKENS, MAX_CONTEXT_WINDOW_TOKENS)
}

fn parse_device_indices_env(name: &str) -> Vec<usize> {
    parse_device_indices_override(name, std::env::var(name).ok().as_deref())
}

fn parse_device_indices_override(name: &str, value: Option<&str>) -> Vec<usize> {
    let Some(trimmed) = value.map(str::trim) else {
        return Vec::new();
    };
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
        return Vec::new();
    }

    trimmed
        .split(',')
        .filter_map(|segment| {
            let item = segment.trim();
            if item.is_empty() {
                return None;
            }
            match item.parse::<usize>() {
                Ok(index) => Some(index),
                Err(error) => {
                    warn!(
                        env = name,
                        value = item,
                        %error,
                        "ignoring invalid AI GPU device index override"
                    );
                    None
                }
            }
        })
        .collect()
}

fn response_mode_completion_reserve_tokens(
    response_mode: AssistantResponseMode,
    context_window_tokens: u32,
) -> u32 {
    let dynamic = match response_mode {
        AssistantResponseMode::Instant => context_window_tokens / 6,
        AssistantResponseMode::Thinking => context_window_tokens / 4,
        AssistantResponseMode::Extended => context_window_tokens / 3,
    };

    match response_mode {
        AssistantResponseMode::Instant => dynamic.clamp(384, 640),
        AssistantResponseMode::Thinking => dynamic.clamp(768, 1536),
        AssistantResponseMode::Extended => dynamic.clamp(1024, 4096),
    }
}

fn prompt_budget_tokens(response_mode: AssistantResponseMode, context_window_tokens: u32) -> u32 {
    context_window_tokens.saturating_sub(
        response_mode_completion_reserve_tokens(response_mode, context_window_tokens)
            + CONTEXT_SAFETY_MARGIN_TOKENS,
    )
}

fn build_prompt_messages(
    request: &AssistantChatRequest,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Vec<rustfin_ai_agent::ChatMessage> {
    build_assistant_messages(request.clone(), grounding_blocks)
}

async fn prepare_generation_messages(
    engine: &rustfin_ai_agent::LlamaEngine,
    request: &AssistantChatRequest,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Result<PreparedGenerationMessages, String> {
    let messages = build_prompt_messages(request, grounding_blocks);
    let original_prompt_tokens = engine
        .count_chat_tokens(&messages)
        .map_err(|error| format!("failed to count prompt tokens: {error}"))?;
    let budget = prompt_budget_tokens(request.response_mode, engine.params().n_ctx);

    if original_prompt_tokens <= budget || request.history.is_empty() {
        return Ok(PreparedGenerationMessages {
            messages,
            compacted: false,
            original_prompt_tokens,
            final_prompt_tokens: original_prompt_tokens,
            retained_history_messages: request.history.len(),
            summarized_history_messages: 0,
        });
    }

    let mut recent_keep = request
        .history
        .len()
        .min(COMPACTION_MAX_RECENT_HISTORY_MESSAGES);
    let minimum_recent_keep = request
        .history
        .len()
        .min(COMPACTION_MIN_RECENT_HISTORY_MESSAGES);

    let mut best_candidate: Option<PreparedGenerationMessages> = None;

    loop {
        let summarized_history_messages = request.history.len().saturating_sub(recent_keep);
        let summary_targets = if summarized_history_messages == 0 {
            vec![COMPACTION_SUMMARY_TARGET_TOKENS]
        } else {
            COMPACTION_SUMMARY_FALLBACK_TARGET_TOKENS.to_vec()
        };

        for summary_target_tokens in summary_targets {
            let summary = if summarized_history_messages == 0 {
                None
            } else {
                Some(
                    summarize_history_for_compaction(
                        engine,
                        &request.history[..summarized_history_messages],
                        summary_target_tokens,
                    )
                    .await?,
                )
            };
            let compacted_request =
                build_compacted_request(request, summary.as_deref(), recent_keep);
            let compacted_messages = build_prompt_messages(&compacted_request, grounding_blocks);
            let final_prompt_tokens = engine
                .count_chat_tokens(&compacted_messages)
                .map_err(|error| format!("failed to count compacted prompt tokens: {error}"))?;

            let candidate = PreparedGenerationMessages {
                messages: compacted_messages,
                compacted: true,
                original_prompt_tokens,
                final_prompt_tokens,
                retained_history_messages: recent_keep,
                summarized_history_messages,
            };

            if candidate.final_prompt_tokens <= budget {
                return Ok(candidate);
            }

            let replace_best = best_candidate
                .as_ref()
                .map(|best| candidate.final_prompt_tokens < best.final_prompt_tokens)
                .unwrap_or(true);
            if replace_best {
                best_candidate = Some(candidate);
            }
        }

        if recent_keep == minimum_recent_keep {
            break;
        }

        recent_keep = recent_keep.saturating_sub(1).max(minimum_recent_keep);
    }

    let best_candidate = best_candidate.ok_or_else(|| {
        "failed to build any compacted prompt candidate for this conversation".to_string()
    })?;
    Err(format!(
        "the conversation needs {} prompt tokens even after compaction, but only {} fit into the current {} token context window",
        best_candidate.final_prompt_tokens,
        budget,
        engine.params().n_ctx
    ))
}

fn build_compacted_request(
    request: &AssistantChatRequest,
    summary: Option<&str>,
    recent_keep: usize,
) -> AssistantChatRequest {
    let retained_start = request.history.len().saturating_sub(recent_keep);
    let mut history = Vec::new();

    if let Some(summary) = summary.map(str::trim).filter(|summary| !summary.is_empty()) {
        history.push(crate::ai_assistant::types::AssistantHistoryMessage {
            role: "system".to_string(),
            content: format!(
                "Compacted Rustyfin conversation memory for earlier turns:\n{}",
                summary
            ),
            grounding_tools: Vec::new(),
            follow_up_contexts: Vec::new(),
        });
    }

    history.extend_from_slice(&request.history[retained_start..]);

    AssistantChatRequest {
        model: request.model.clone(),
        message: request.message.clone(),
        response_mode: request.response_mode,
        confirmation_token: request.confirmation_token.clone(),
        history,
    }
}

async fn summarize_history_for_compaction(
    engine: &rustfin_ai_agent::LlamaEngine,
    history: &[crate::ai_assistant::types::AssistantHistoryMessage],
    target_tokens: u32,
) -> Result<String, String> {
    if history.is_empty() {
        return Ok(String::new());
    }

    let context_window = engine.params().n_ctx;
    let segment_budget = context_window
        .saturating_sub(target_tokens + CONTEXT_SAFETY_MARGIN_TOKENS)
        .max(1024);
    let segments = split_history_for_summary(engine, history, segment_budget)?;

    let mut summaries = Vec::new();
    for segment in segments {
        let rendered = render_history_segment(&segment);
        let summary = run_compaction_summary(engine, &rendered, target_tokens)
            .await
            .unwrap_or_else(|_| fallback_history_summary(&segment));
        if !summary.trim().is_empty() {
            summaries.push(summary.trim().to_string());
        }
    }

    if summaries.is_empty() {
        return Ok(fallback_history_summary(history));
    }
    if summaries.len() == 1 {
        return Ok(summaries[0].clone());
    }

    let combined = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| format!("Chunk {} summary:\n{}", index + 1, summary))
        .collect::<Vec<_>>()
        .join("\n\n");

    run_compaction_summary(engine, &combined, target_tokens)
        .await
        .or_else(|_| Ok(shorten_text(&combined, 1800)))
}

fn split_history_for_summary(
    engine: &rustfin_ai_agent::LlamaEngine,
    history: &[crate::ai_assistant::types::AssistantHistoryMessage],
    prompt_budget: u32,
) -> Result<Vec<Vec<crate::ai_assistant::types::AssistantHistoryMessage>>, String> {
    let mut segments = Vec::new();
    let mut current = Vec::new();

    for message in history.iter().cloned() {
        current.push(message.clone());
        let rendered = render_history_segment(&current);
        let prompt = build_compaction_summary_messages(&rendered, COMPACTION_SUMMARY_TARGET_TOKENS);
        let prompt_tokens = engine
            .count_chat_tokens(&prompt)
            .map_err(|error| format!("failed to count summary prompt tokens: {error}"))?;

        if prompt_tokens > prompt_budget && current.len() > 1 {
            current.pop();
            segments.push(current);
            current = vec![message];
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    Ok(segments)
}

fn render_history_segment(
    history: &[crate::ai_assistant::types::AssistantHistoryMessage],
) -> String {
    history
        .iter()
        .map(|message| {
            let role = message.role.to_ascii_uppercase();
            let content = shorten_text(
                &message
                    .content
                    .replace('\n', " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                COMPACTION_MESSAGE_CHAR_LIMIT,
            );
            if message.grounding_tools.is_empty() {
                format!("{role}: {content}")
            } else {
                format!(
                    "{role}: {content}\nGrounded tools: {}",
                    message.grounding_tools.join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_compaction_summary_messages(
    rendered_history: &str,
    target_tokens: u32,
) -> Vec<rustfin_ai_agent::ChatMessage> {
    vec![
        rustfin_ai_agent::ChatMessage {
            role: "system".to_string(),
            content: format!(
                "You are compacting earlier Rustyfin chat history into a durable memory note for the next assistant turn. \
Preserve user goals, decisions, constraints, names, dates, URLs, identifiers, tool findings, and open tasks. \
Do not invent details. Use concise bullet lines. Keep the summary within about {target_tokens} tokens."
            ),
        },
        rustfin_ai_agent::ChatMessage {
            role: "user".to_string(),
            content: format!("Earlier conversation to compact:\n{rendered_history}"),
        },
    ]
}

async fn run_compaction_summary(
    engine: &rustfin_ai_agent::LlamaEngine,
    rendered_history: &str,
    target_tokens: u32,
) -> Result<String, String> {
    let messages = build_compaction_summary_messages(rendered_history, target_tokens);
    let stream = engine.chat_stream(
        messages,
        rustfin_ai_agent::SamplingParams {
            temperature: 0.2,
            top_p: 0.9,
            top_k: 24,
            repeat_penalty: 1.05,
            max_tokens: target_tokens,
            max_duration_ms: Some(COMPACTION_SUMMARY_MAX_DURATION_MS),
        },
    );
    futures::pin_mut!(stream);

    let mut summary = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk.map_err(|error| error.to_string())? {
            rustfin_ai_agent::ChatChunk::Token(text) => summary.push_str(&text),
            rustfin_ai_agent::ChatChunk::Done => break,
            rustfin_ai_agent::ChatChunk::Stats { .. } => {}
        }
    }

    let summary = summary.trim().to_string();
    if summary.is_empty() {
        Err("history compaction produced an empty summary".to_string())
    } else {
        Ok(summary)
    }
}

fn fallback_history_summary(
    history: &[crate::ai_assistant::types::AssistantHistoryMessage],
) -> String {
    history
        .iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            format!(
                "- {}: {}",
                message.role,
                shorten_text(
                    &message
                        .content
                        .replace('\n', " ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                    180
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn shorten_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn sampling_params_for_response_mode(
    response_mode: AssistantResponseMode,
) -> rustfin_ai_agent::SamplingParams {
    match response_mode {
        AssistantResponseMode::Instant => rustfin_ai_agent::SamplingParams {
            temperature: 0.45,
            top_p: 0.85,
            top_k: 24,
            repeat_penalty: 1.05,
            max_tokens: 640,
            max_duration_ms: None,
        },
        AssistantResponseMode::Thinking => rustfin_ai_agent::SamplingParams {
            temperature: 0.72,
            top_p: 0.94,
            top_k: 48,
            repeat_penalty: 1.08,
            max_tokens: 1536,
            max_duration_ms: None,
        },
        AssistantResponseMode::Extended => rustfin_ai_agent::SamplingParams {
            temperature: 0.58,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            max_tokens: u32::MAX,
            max_duration_ms: Some(30 * 60 * 1000),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_device_indices_override, parse_gpu_split_mode_override, parse_i32_override,
        prompt_budget_tokens, resolve_effective_context_window,
        response_mode_completion_reserve_tokens, sampling_params_for_response_mode,
        suggested_context_window_from_host_memory,
    };
    use crate::ai_assistant::types::AssistantResponseMode;
    use rustfin_ai_agent::engine::LlamaGpuSplitMode;

    #[test]
    fn parse_gpu_split_mode_override_defaults_to_layer() {
        assert_eq!(
            parse_gpu_split_mode_override(None),
            LlamaGpuSplitMode::Layer
        );
        assert_eq!(
            parse_gpu_split_mode_override(Some("layer")),
            LlamaGpuSplitMode::Layer
        );
        assert_eq!(
            parse_gpu_split_mode_override(Some("unsupported")),
            LlamaGpuSplitMode::Layer
        );
    }

    #[test]
    fn parse_gpu_split_mode_override_supports_none_and_row() {
        assert_eq!(
            parse_gpu_split_mode_override(Some("none")),
            LlamaGpuSplitMode::None
        );
        assert_eq!(
            parse_gpu_split_mode_override(Some("row")),
            LlamaGpuSplitMode::Row
        );
    }

    #[test]
    fn parse_i32_override_ignores_empty_and_invalid_values() {
        assert_eq!(parse_i32_override("TEST_VALUE", None), None);
        assert_eq!(parse_i32_override("TEST_VALUE", Some("   ")), None);
        assert_eq!(parse_i32_override("TEST_VALUE", Some("abc")), None);
        assert_eq!(parse_i32_override("TEST_VALUE", Some("7")), Some(7));
    }

    #[test]
    fn parse_device_indices_override_supports_all_and_filters_invalid_items() {
        assert_eq!(
            parse_device_indices_override("TEST_VALUE", None),
            Vec::<usize>::new()
        );
        assert_eq!(
            parse_device_indices_override("TEST_VALUE", Some("all")),
            Vec::<usize>::new()
        );
        assert_eq!(
            parse_device_indices_override("TEST_VALUE", Some("0, 2, bad, 2")),
            vec![0, 2, 2]
        );
    }

    #[test]
    fn instant_mode_uses_tighter_sampling_profile() {
        let instant = sampling_params_for_response_mode(AssistantResponseMode::Instant);
        let thinking = sampling_params_for_response_mode(AssistantResponseMode::Thinking);
        let extended = sampling_params_for_response_mode(AssistantResponseMode::Extended);
        assert!(instant.max_tokens < thinking.max_tokens);
        assert!(instant.temperature < thinking.temperature);
        assert!(instant.top_k < thinking.top_k);
        assert!(thinking.max_tokens < extended.max_tokens);
        assert_eq!(extended.max_duration_ms, Some(30 * 60 * 1000));
    }

    #[test]
    fn suggested_context_window_scales_with_host_memory() {
        let gib = 1024_u64 * 1024 * 1024;
        assert_eq!(
            suggested_context_window_from_host_memory(Some(8 * gib)),
            Some(4096)
        );
        assert_eq!(
            suggested_context_window_from_host_memory(Some(24 * gib)),
            Some(8192)
        );
        assert_eq!(
            suggested_context_window_from_host_memory(Some(48 * gib)),
            Some(16384)
        );
        assert_eq!(
            suggested_context_window_from_host_memory(Some(96 * gib)),
            Some(32768)
        );
    }

    #[test]
    fn context_window_resolution_respects_model_and_hardware_caps() {
        let gib = 1024_u64 * 1024 * 1024;
        assert_eq!(
            resolve_effective_context_window(Some(32768), Some(8192), Some(24 * gib)),
            8192
        );
        assert_eq!(
            resolve_effective_context_window(Some(32768), Some(65536), Some(24 * gib)),
            8192
        );
        assert_eq!(
            resolve_effective_context_window(None, Some(65536), Some(48 * gib)),
            16384
        );
        assert_eq!(
            resolve_effective_context_window(Some(2048), Some(65536), Some(96 * gib)),
            2048
        );
    }

    #[test]
    fn prompt_budget_reserves_more_space_for_slower_modes() {
        let context_window = 8192;
        let instant =
            response_mode_completion_reserve_tokens(AssistantResponseMode::Instant, context_window);
        let thinking = response_mode_completion_reserve_tokens(
            AssistantResponseMode::Thinking,
            context_window,
        );
        let extended = response_mode_completion_reserve_tokens(
            AssistantResponseMode::Extended,
            context_window,
        );

        assert!(instant < thinking);
        assert!(thinking < extended);
        assert_eq!(
            prompt_budget_tokens(AssistantResponseMode::Instant, context_window),
            context_window - instant - 192
        );
        assert_eq!(
            prompt_budget_tokens(AssistantResponseMode::Thinking, context_window),
            context_window - thinking - 192
        );
        assert_eq!(
            prompt_budget_tokens(AssistantResponseMode::Extended, context_window),
            context_window - extended - 192
        );
    }
}

async fn store_confirmation_payload(
    state: &AppState,
    user: &AuthUser,
    payload: &AssistantConfirmationPayload,
    expires_ts: i64,
) -> Result<rustfin_db::repo::ai_assistant_confirmation::AiAssistantConfirmationTokenRow, String> {
    let payload_json = serde_json::to_string(payload)
        .map_err(|e| format!("failed to serialize confirmation payload: {e}"))?;
    rustfin_db::repo::ai_assistant_confirmation::create_confirmation_token(
        &state.db,
        rustfin_db::repo::ai_assistant_confirmation::CreateAiAssistantConfirmationTokenParams {
            user_id: &user.user_id,
            action_kind: payload.action_kind.as_str(),
            payload_json: &payload_json,
            expires_ts,
        },
    )
    .await
    .map_err(|e| format!("failed to store confirmation token: {e}"))
}

async fn load_confirmation_payload(
    state: &AppState,
    user: &AuthUser,
    token: &str,
) -> Result<AssistantConfirmationPayload, String> {
    let row = rustfin_db::repo::ai_assistant_confirmation::get_confirmation_token_for_user(
        &state.db,
        token,
        &user.user_id,
    )
    .await
    .map_err(|e| format!("failed to load confirmation token: {e}"))?
    .ok_or_else(|| "That confirmation token was not found for this account.".to_string())?;

    if row.consumed_ts.is_some() {
        return Err("That confirmation token was already used.".to_string());
    }
    if row.expires_ts < chrono::Utc::now().timestamp() {
        return Err("That confirmation token has expired. Ask Rustyfin AI to prepare the calendar action again.".to_string());
    }

    serde_json::from_str::<AssistantConfirmationPayload>(&row.payload_json)
        .map_err(|e| format!("failed to decode confirmation payload: {e}"))
}

async fn consume_confirmation_payload(
    state: &AppState,
    user: &AuthUser,
    token: &str,
    payload: &AssistantConfirmationPayload,
) -> Result<(), String> {
    let row = rustfin_db::repo::ai_assistant_confirmation::get_confirmation_token_for_user(
        &state.db,
        token,
        &user.user_id,
    )
    .await
    .map_err(|e| format!("failed to reload confirmation token: {e}"))?
    .ok_or_else(|| "That confirmation token was not found for this account.".to_string())?;

    if row.consumed_ts.is_some() {
        return Err("That confirmation token was already used.".to_string());
    }
    if row.expires_ts < chrono::Utc::now().timestamp() {
        return Err("That confirmation token has expired. Ask Rustyfin AI to prepare the calendar action again.".to_string());
    }

    let consumed = rustfin_db::repo::ai_assistant_confirmation::consume_confirmation_token(
        &state.db,
        token,
        &user.user_id,
        chrono::Utc::now().timestamp(),
    )
    .await
    .map_err(|e| format!("failed to consume confirmation token: {e}"))?;
    if !consumed {
        return Err("That confirmation token was already used.".to_string());
    }

    if let Some(conversation_id) = payload.conversation_id.as_deref() {
        let pending_action = AssistantPendingAction {
            token: token.to_string(),
            action_kind: payload.action_kind,
            summary: payload.summary.clone(),
            expires_ts: row.expires_ts,
            status: AssistantPendingActionStatus::Confirmed,
        };
        let pending_action_json = serde_json::to_string(&pending_action)
            .map_err(|e| format!("failed to serialize confirmed pending action: {e}"))?;
        let _ = rustfin_db::repo::ai_conversations::update_pending_action_json_for_token(
            &state.db,
            conversation_id,
            &user.user_id,
            token,
            &pending_action_json,
        )
        .await;
    }

    Ok(())
}

fn assistant_text_for_confirmed_action(block: &AssistantToolContextBlock) -> String {
    if block.status != "ok" {
        return block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Rustyfin AI could not complete that action.")
            .to_string();
    }

    if block.tool == "document_create_download" {
        let artifact = block.data.get("artifact");
        let file_name = artifact
            .and_then(|value| value.get("file_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&block.label);
        let media_type = artifact
            .and_then(|value| value.get("media_type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text/plain; charset=utf-8");
        let format_label = if media_type.starts_with("text/markdown") {
            "markdown"
        } else {
            "plain-text"
        };
        return format!(
            "I created the downloadable {format_label} document \"{file_name}\". Use the download link below."
        );
    }

    let event = block.data.get("event");
    let title = event
        .and_then(|value| value.get("title"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&block.label);
    let event_date = event
        .and_then(|value| value.get("event_date"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let scope = event
        .and_then(|value| value.get("scope"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("personal");
    let event_type = event
        .and_then(|value| value.get("event_type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("event");

    let human_date = chrono::NaiveDate::parse_from_str(event_date, "%Y-%m-%d")
        .map(|date| date.format("%B %-d, %Y").to_string())
        .unwrap_or_else(|_| event_date.to_string());
    let scope_label = if scope == "global" {
        "the shared calendar"
    } else {
        "your personal calendar"
    };

    if block.tool == "calendar_delete_event" {
        if event_type == "birthday" {
            return format!(
                "I deleted and verified the recurring birthday \"{title}\" from {scope_label}."
            );
        }
        return format!(
            "I deleted and verified the calendar event \"{title}\" on {human_date} from {scope_label}."
        );
    }

    if event_type == "birthday" {
        return format!(
            "I created and verified the recurring birthday \"{title}\" on {human_date} in {scope_label}."
        );
    }

    format!(
        "I created and verified the calendar event \"{title}\" on {human_date} in {scope_label}."
    )
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
        AssistantToolInput::CurrentDateTime { location } => format!(
            "current_datetime:location={}",
            location.as_deref().unwrap_or("host")
        ),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
            query,
        } => format!(
            "calendar:{label}:{from_date}->{to_date}:query={}",
            query.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::CalendarCreateEvent {
            scope,
            title,
            event_date,
            ..
        } => format!("calendar_create_event:scope={scope}:title={title}:date={event_date}"),
        AssistantToolInput::CalendarCreateBirthday {
            scope,
            title,
            event_date,
            birthday_year,
            ..
        } => format!(
            "calendar_create_birthday:scope={scope}:title={title}:date={event_date}:birth_year={birthday_year}"
        ),
        AssistantToolInput::CalendarDeleteEvent {
            event_id,
            title,
            event_date,
            scope,
            event_type,
            ..
        } => format!(
            "calendar_delete_event:id={event_id}:scope={scope}:title={title}:date={event_date}:type={event_type}"
        ),
        AssistantToolInput::DocumentCreateDownload {
            file_name,
            format,
            model_name,
            ..
        } => format!(
            "document_create_download:file_name={file_name}:format={format}:model={model_name}"
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
        AssistantToolInput::WeatherHistory {
            location,
            start_date,
            end_date,
            label,
        } => format!(
            "weather_history:location={location}:label={label}:range={start_date}->{end_date}"
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
