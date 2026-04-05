use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use async_stream::stream;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use serde::Serialize;
use serde_json::json;
use tracing::{info, warn};

use crate::ai_assistant::confirmation::{
    CONFIRMATION_TOKEN_TTL_SECS, pending_action_request_for_message_with_state,
};
use crate::ai_assistant::context::AssistantContext;
use crate::ai_assistant::executor::{AssistantGroundedExecutor, ExecutorPostStep};
use crate::ai_assistant::memory::{
    augment_history_with_entity_graph, build_grounding_chunks_for_turn, persist_grounding_artifacts,
};
use crate::ai_assistant::scheduler::{TurnPriority, TurnScheduler};
use crate::ai_assistant::tools::{
    build_follow_up_context, execute_tool, source_from_block, tool_result_to_outcome,
};
use crate::ai_assistant::types::{
    AssistantActivityTraceItem, AssistantConfirmationPayload, AssistantConfirmationRequiredEvent,
    AssistantExecutionTrace, AssistantFollowUpContext, AssistantGroundingChunk,
    AssistantGroundingSource, AssistantPendingAction, AssistantPendingActionStatus, AssistantPhase,
    AssistantPhaseEvent, AssistantPlannerDebug, AssistantPlannerMode, AssistantRuntimePhase,
    AssistantStatusEvent, AssistantStatusKind, AssistantToolActivityEvent,
    AssistantToolActivityState, AssistantToolContextBlock, AssistantToolInput, AssistantTurnStats,
    ConversationPromptDebug, PlannedToolSet,
};
use crate::ai_assistant::weather::deterministic_weather_reply;
use crate::ai_assistant::{
    AssistantChatRequest, build_assistant_messages, build_assistant_messages_with_budget,
    deterministic_ai_runtime_reply, deterministic_calendar_reply,
    deterministic_current_datetime_reply, deterministic_library_reply,
    deterministic_multi_step_reply, deterministic_network_reply,
    deterministic_tool_inventory_reply, immediate_response_for_message, plan_execution_candidates,
    plan_tool_calls_with_model_assist, status_label_for_tool_call,
    unsafe_action_response_for_message, unsupported_write_response_for_message,
};
use crate::ai_audit::{
    AiAssistantAuditResponseKind, persist_chat_audit_event, persist_chat_audit_event_with_planner,
};
use crate::ai_conversations::ConversationMessageRequest;
use crate::ai_model_routing::RoleRoutingDecision;
use crate::ai_storage::{
    AiModelSummary, current_model_dir, list_models_with_storage_status, model_file_path,
};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

#[cfg(test)]
use crate::ai_assistant::types::{AssistantResponseMode, PlannedToolCall};

pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
    pub role_models: HashMap<rustfin_ai_agent::ModelRole, LoadedRoleModel>,
    pub role_routing: Vec<RoleRoutingDecision>,
    pub last_prompt_debug: Option<ConversationPromptDebug>,
    pub last_execution_trace: Option<AssistantExecutionTrace>,
    pub active_phase: AssistantRuntimePhase,
    pub scheduler: std::sync::Arc<TurnScheduler>,
}

#[derive(Clone)]
pub struct LoadedRoleModel {
    pub model_name: String,
    pub backend_id: String,
    pub backend_kind: rustfin_ai_agent::BackendKind,
    pub backend: Arc<dyn rustfin_ai_agent::InferenceBackend>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
            role_models: HashMap::new(),
            role_routing: Vec::new(),
            last_prompt_debug: None,
            last_execution_trace: None,
            active_phase: AssistantRuntimePhase::Idle,
            scheduler: std::sync::Arc::new(TurnScheduler::new()),
        }
    }
}

#[derive(Clone)]
struct ConversationPersistence {
    conversation_id: String,
}

#[derive(Serialize)]
struct AssistantToolAttemptEvent {
    step_index: u32,
    tool: String,
    outcome_kind: String,
    recovery_depth: u8,
    is_alternate: bool,
    latency_ms: u64,
}

#[derive(Serialize)]
struct AssistantClarificationSseEvent {
    message: String,
}

#[derive(Serialize)]
struct AssistantStopReasonSseEvent {
    reason: String,
    final_answer_path: String,
    tool_step_count: u32,
    alternate_tool_count: u32,
    recovery_step_count: u32,
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/runtime", get(crate::ai_runtime::get_ai_runtime))
        .merge(crate::ai_tasks::router())
        .route(
            "/transcribe",
            post(crate::ai_transcribe::transcribe_audio).layer(DefaultBodyLimit::max(
                crate::ai_transcribe::MAX_AI_TRANSCRIBE_BYTES,
            )),
        )
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
            response_mode: crate::ai_assistant::types::AssistantResponseMode::Thinking,
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
    let requested_model_name = req.model.clone();
    let history_len = req.history.len();
    let sse_stream = stream! {
        let mut model_name = requested_model_name.clone();
        let trace_id = uuid::Uuid::new_v4().to_string();
        let assistant_context = AssistantContext::new(&user, trace_id.clone());
        let turn_started = Instant::now();
        let chat_metrics = state.runtime_metrics.start_ai_chat_request();
        let mut audit_written = false;
        let mut assistant_content = String::new();
        let mut activity_trace = Vec::<AssistantActivityTraceItem>::new();
        let mut grounding_blocks = Vec::<AssistantToolContextBlock>::new();
        let mut grounding_sources = Vec::<AssistantGroundingSource>::new();
        let mut grounding_chunks = Vec::<AssistantGroundingChunk>::new();
        let mut follow_up_contexts = Vec::<AssistantFollowUpContext>::new();
        let mut stats: Option<AssistantTurnStats> = None;
        let mut tool_duration_ms = 0_u64;

        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            username = %user.username,
            model = %requested_model_name,
            history_len,
            "ai chat request received"
        );

        {
            let mut guard = state.engine.lock().await;
            guard.role_models.clear();
            guard.role_routing.clear();
            guard.loaded_model = None;
            guard.engine = None;
        }

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
                    stats = Some(build_server_authored_turn_stats(
                        &req,
                        &assistant_content,
                        &grounding_chunks,
                        0,
                        0,
                        turn_started.elapsed().as_millis() as u64,
                        0,
                        0,
                        None,
                        None,
                    ));
                    persist_chat_audit_event(
                        &state,
                        &user,
                        &req,
                        &trace_id,
                        AiAssistantAuditResponseKind::Clarification,
                        &[],
                        &[],
                        &grounding_chunks,
                        &[],
                        None,
                    )
                    .await;
                    if let Some(persistence) = &persistence {
                        let turn_result = crate::ai_conversations::persist_assistant_turn(
                            &state,
                            &user.user_id,
                            &persistence.conversation_id,
                            &assistant_content,
                            &model_name,
                            &[],
                            &[],
                            &grounding_chunks,
                            &grounding_sources,
                            &activity_trace,
                            stats.as_ref(),
                            None,
                            Some(&trace_id),
                        )
                        .await;
                        persist_turn_grounding_artifacts(
                            &state,
                            &assistant_context,
                            &persistence.conversation_id,
                            turn_result,
                            &grounding_chunks,
                            &follow_up_contexts,
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
                        "chunks": &grounding_chunks,
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

            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));

            if let Some(persistence) = &persistence {
                let grounding_tool_names = planned_tools
                    .iter()
                    .map(|call| call.tool.as_str().to_string())
                    .collect::<Vec<_>>();
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tool_names,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
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
                &grounding_chunks,
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
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Clarification,
                &[],
                &[],
                &grounding_chunks,
                &[],
                None,
            )
            .await;
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    pending_action.as_ref(),
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
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

        if let Some(message) = unsafe_action_response_for_message(&req.message) {
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
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::UnsupportedWriteRefusal,
                &[],
                &[],
                &grounding_chunks,
                &[],
                None,
            )
            .await;
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
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
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::UnsupportedWriteRefusal,
                &[],
                &[],
                &grounding_chunks,
                &[],
                None,
            )
            .await;
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
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

        if let Some(message) = deterministic_tool_inventory_reply(&user, &req.message) {
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
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &[],
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
                    &grounding_sources,
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
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                None,
                None,
            ));
            persist_chat_audit_event(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Clarification,
                &[],
                &[],
                &grounding_chunks,
                &[],
                None,
            )
            .await;
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &[],
                    &[],
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
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

        let augmented_history = augment_history_with_entity_graph(
            &state,
            &assistant_context,
            &req.history,
            &req.message,
        )
        .await;

        let model_dir = current_model_dir(&state).await;
        let available_models = crate::ai_storage::list_models_from_state(&state)
            .await
            .unwrap_or_default();
        let available_model_names = available_models
            .iter()
            .map(|model| model.name.clone())
            .collect::<Vec<_>>();
        let scheduler = {
            let guard = state.engine.lock().await;
            guard.scheduler.clone()
        };
        let remote_backend = match crate::ai_admin::load_remote_backend_config(&state).await {
            Ok(Some(remote_config)) => {
                scheduler.set_remote_backend(Some(remote_config.clone()));
                Some(remote_config)
            }
            Ok(None) => {
                scheduler.set_remote_backend(None);
                None
            }
            Err(error) => {
                warn!(
                    trace_id = %trace_id,
                    user_id = %user.user_id,
                    error = ?error,
                    "failed to load persisted remote AI backend config; continuing with local role routing"
                );
                None
            }
        };
        let profiles = load_model_profiles_for_host(&state).await;
        let mut role_routing = crate::ai_model_routing::resolve_role_routing_plan(
            Some(&requested_model_name),
            &available_model_names,
            &profiles,
            remote_backend.as_ref(),
            chrono::Utc::now().timestamp(),
        );
        let answer_route = role_route(&role_routing, rustfin_ai_agent::ModelRole::Answer)
            .cloned()
            .unwrap_or_else(|| crate::ai_model_routing::ResolvedRoleRouting {
                selection: rustfin_ai_agent::RoleModelSelection {
                    model_name: requested_model_name.clone(),
                    source: rustfin_ai_agent::ModelSelectionSource::ExplicitRequest,
                },
                decision: crate::ai_model_routing::RoleRoutingDecision {
                    role: rustfin_ai_agent::ModelRole::Answer,
                    model_name: requested_model_name.clone(),
                    backend_id: "local_llama".to_string(),
                    backend_kind: rustfin_ai_agent::BackendKind::Local,
                    selection_source: rustfin_ai_agent::ModelSelectionSource::ExplicitRequest,
                    recommendation_status: crate::ai_benchmark_recommendations::BenchmarkRecommendationStatus::Missing,
                    recommendation_note: Some("no answer-role routing decision was produced".to_string()),
                    recommendation_model_name: None,
                    recommendation_updated_ts: None,
                },
                tuning_profile: None,
            });
        let answer_context_length_tokens = answer_route
            .tuning_profile
            .as_ref()
            .and_then(|profile| u32::try_from(profile.context_window.max(1)).ok())
            .unwrap_or(4096);
        model_name = answer_route.selection.model_name.clone();
        {
            let mut guard = state.engine.lock().await;
            guard.role_routing = role_routing.iter().map(|route| route.decision.clone()).collect();
        }
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
                    &grounding_chunks,
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

        let model_size_bytes = tokio::fs::metadata(&gguf_path)
            .await
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        let turn_priority = TurnPriority::Interactive;

        set_engine_phase(&state, AssistantRuntimePhase::LoadingModel).await;
        let (_turn_lease, engine, model_load_duration_ms, queue_duration_ms, scheduler_decision) =
            match scheduler
                .acquire_model(
                    turn_priority,
                    &model_name,
                    gguf_path.clone(),
                    engine_params_for_profile(answer_route.tuning_profile.as_ref()),
                    estimated_model_bytes(model_size_bytes, answer_route.tuning_profile.as_ref()),
                )
                .await
            {
                Ok(result) => result,
                Err(error_message) => {
                    persist_chat_audit_event(
                        &state,
                        &user,
                        &req,
                        &trace_id,
                        AiAssistantAuditResponseKind::ModelLoadError,
                        &[],
                        &[],
                        &grounding_chunks,
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

        let answer_backend: Arc<dyn rustfin_ai_agent::InferenceBackend> =
            Arc::new(rustfin_ai_agent::LocalLlamaBackend::new(
                model_name.clone(),
                engine.clone(),
            ));
        let mut loaded_role_models = HashMap::new();
        loaded_role_models.insert(
            rustfin_ai_agent::ModelRole::Answer,
            LoadedRoleModel {
                model_name: model_name.clone(),
                backend_id: "local_llama".to_string(),
                backend_kind: rustfin_ai_agent::BackendKind::Local,
                backend: answer_backend.clone(),
            },
        );

        if let Some(planner_route) = role_route(&role_routing, rustfin_ai_agent::ModelRole::Planner)
            .cloned()
        {
            let planner_backend = if let Some(loaded) =
                find_loaded_backend_for_route(&loaded_role_models, &planner_route)
            {
                loaded.backend
            } else {
                match planner_route.decision.backend_kind {
                    rustfin_ai_agent::BackendKind::Remote => {
                        match remote_backend.clone() {
                            Some(remote_config) => match rustfin_ai_agent::RemotePromptBackend::new(
                                rustfin_ai_agent::RemoteBackendConfig::from(&remote_config),
                            ) {
                                Ok(remote_prompt_backend) => {
                                    Arc::new(remote_prompt_backend)
                                        as Arc<dyn rustfin_ai_agent::InferenceBackend>
                                }
                                Err(error) => {
                                    warn!(
                                        trace_id = %trace_id,
                                        user_id = %user.user_id,
                                        error = %error,
                                        "failed to initialize remote planner backend; falling back to answer backend"
                                    );
                                    if let Some(route) = role_routing
                                        .iter_mut()
                                        .find(|route| route.decision.role == rustfin_ai_agent::ModelRole::Planner)
                                    {
                                        route.selection.model_name = model_name.clone();
                                        route.selection.source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                        route.decision.model_name = model_name.clone();
                                        route.decision.backend_id = "local_llama".to_string();
                                        route.decision.backend_kind = rustfin_ai_agent::BackendKind::Local;
                                        route.decision.selection_source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                        route.decision.recommendation_note = Some(format!(
                                            "remote planner backend initialization failed: {error}"
                                        ));
                                    }
                                    answer_backend.clone()
                                }
                            },
                            None => answer_backend.clone(),
                        }
                    }
                    rustfin_ai_agent::BackendKind::Local => {
                        let planner_path =
                            model_file_path(&model_dir, &planner_route.selection.model_name).ok();
                        let planner_size_bytes = if let Some(path) = planner_path.as_ref() {
                            tokio::fs::metadata(path)
                                .await
                                .map(|metadata| metadata.len())
                                .unwrap_or_default()
                        } else {
                            0
                        };
                        if let Some(path) = planner_path {
                            match scheduler
                                .acquire_aux_model(
                                    &planner_route.selection.model_name,
                                    path,
                                    engine_params_for_profile(planner_route.tuning_profile.as_ref()),
                                    estimated_model_bytes(
                                        planner_size_bytes,
                                        planner_route.tuning_profile.as_ref(),
                                    ),
                                )
                                .await
                            {
                                Ok((planner_engine, _planner_load_duration_ms)) => {
                                    Arc::new(rustfin_ai_agent::LocalLlamaBackend::new(
                                        planner_route.selection.model_name.clone(),
                                        planner_engine,
                                    )) as Arc<dyn rustfin_ai_agent::InferenceBackend>
                                }
                                Err(error) => {
                                    warn!(
                                        trace_id = %trace_id,
                                        user_id = %user.user_id,
                                        role = "planner",
                                        model = %planner_route.selection.model_name,
                                        error = %error,
                                        "failed to load planner role model; falling back to answer backend"
                                    );
                                    if let Some(route) = role_routing
                                        .iter_mut()
                                        .find(|route| route.decision.role == rustfin_ai_agent::ModelRole::Planner)
                                    {
                                        route.selection.model_name = model_name.clone();
                                        route.selection.source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                        route.decision.model_name = model_name.clone();
                                        route.decision.backend_id = "local_llama".to_string();
                                        route.decision.backend_kind = rustfin_ai_agent::BackendKind::Local;
                                        route.decision.selection_source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                        route.decision.recommendation_note = Some(format!(
                                            "planner role model load failed: {error}"
                                        ));
                                    }
                                    answer_backend.clone()
                                }
                            }
                        } else {
                            if let Some(route) = role_routing
                                .iter_mut()
                                .find(|route| route.decision.role == rustfin_ai_agent::ModelRole::Planner)
                            {
                                route.selection.model_name = model_name.clone();
                                route.selection.source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                route.decision.model_name = model_name.clone();
                                route.decision.backend_id = "local_llama".to_string();
                                route.decision.backend_kind = rustfin_ai_agent::BackendKind::Local;
                                route.decision.selection_source = rustfin_ai_agent::ModelSelectionSource::Fallback;
                                route.decision.recommendation_note = Some(format!(
                                    "planner role model `{}` was not found locally",
                                    planner_route.selection.model_name
                                ));
                            }
                            answer_backend.clone()
                        }
                    }
                }
            };

            let effective_planner_route =
                role_route(&role_routing, rustfin_ai_agent::ModelRole::Planner)
                    .cloned()
                    .unwrap_or(planner_route);
            loaded_role_models.insert(
                rustfin_ai_agent::ModelRole::Planner,
                LoadedRoleModel {
                    model_name: effective_planner_route.selection.model_name,
                    backend_id: effective_planner_route.decision.backend_id,
                    backend_kind: effective_planner_route.decision.backend_kind,
                    backend: planner_backend,
                },
            );
        }

        {
            let mut guard = state.engine.lock().await;
            guard.loaded_model = Some(model_name.clone());
            guard.engine = Some(engine.clone());
            guard.role_models = loaded_role_models;
            guard.role_routing = role_routing.iter().map(|route| route.decision.clone()).collect();
            guard.active_phase = AssistantRuntimePhase::Planning;
        }

        let planner_started = Instant::now();
        let planned_tool_set = if scheduler_decision.prefer_deterministic_planner {
            let calls = crate::ai_assistant::orchestrator::plan_tool_calls_with_history(
                &req.message,
                &augmented_history,
            );
            PlannedToolSet {
                mode: AssistantPlannerMode::DeterministicFallback,
                debug: AssistantPlannerDebug {
                    schema_version: 2,
                    planner_mode: Some(
                        AssistantPlannerMode::DeterministicFallback
                            .as_str()
                            .to_string(),
                    ),
                    validated_call_count: calls.len() as u32,
                    final_selected_tools: calls
                        .iter()
                        .map(|call| call.tool.as_str().to_string())
                        .collect(),
                    ..AssistantPlannerDebug::default()
                },
                calls,
            }
        } else {
            let planner_loaded = {
                let guard = state.engine.lock().await;
                guard
                    .role_models
                    .get(&rustfin_ai_agent::ModelRole::Planner)
                    .cloned()
            };
            match planner_loaded {
                Some(planner_loaded) => {
                    let planner_backend = rustfin_ai_agent::RoleBoundPromptBackend::new(
                        planner_loaded.backend,
                        rustfin_ai_agent::ModelRole::Planner,
                    );
                    plan_tool_calls_with_model_assist(
                        &planner_backend,
                        &user,
                        &req.message,
                        &augmented_history,
                    )
                    .await
                }
                None => PlannedToolSet {
                    mode: AssistantPlannerMode::DeterministicFallback,
                    calls: crate::ai_assistant::orchestrator::plan_tool_calls_with_history(
                        &req.message,
                        &augmented_history,
                    ),
                    debug: AssistantPlannerDebug::default(),
                },
            }
        };
        let planner_duration_ms = planner_started.elapsed().as_millis() as u64;
        let mut planner_debug = planned_tool_set.debug.clone();
        let execution_candidates = plan_execution_candidates(req.response_mode, &planned_tool_set);
        let initial_planned_tools = planned_tool_set.calls;
        let mut planned_tools = initial_planned_tools.clone();
        let mut attempted_tool_calls = initial_planned_tools.clone();
        let mut grounded_executor: Option<AssistantGroundedExecutor> = None;
        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            planner_mode = planned_tool_set.mode.as_str(),
            planned_tool_count = initial_planned_tools.len(),
            planned_tools = %initial_planned_tools
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
        if let Some(candidate) = execution_candidates.first() {
            set_engine_phase(&state, AssistantRuntimePhase::Grounding).await;
            let initial_calls = candidate
                .candidate_steps
                .iter()
                .map(|step| step.call.clone())
                .collect::<Vec<_>>();
            let used_role_backends = role_routing
                .iter()
                .map(|route| {
                    format!(
                        "{:?}:{}:{}",
                        route.decision.role,
                        route.decision.model_name,
                        route.decision.backend_kind.as_str()
                    )
                })
                .collect::<Vec<_>>();
            let mut executor = AssistantGroundedExecutor::new(
                &req.message,
                req.response_mode,
                Some(planned_tool_set.mode),
                &initial_calls,
                used_role_backends,
                crate::ai_assistant::provider::ToolExecutionProfile::full_access(),
            );

            while let Some(step) = executor.next_step() {
                let call = step.call.clone();
                let tool_id = format!("tool-{}", step.step_index);
                let label = status_label_for_tool_call(&call);
                let started_ts_ms = now_ts_ms();
                let tool_event = AssistantToolActivityEvent {
                    id: tool_id.clone(),
                    tool: call.tool.as_str().to_string(),
                    label: label.clone(),
                    state: AssistantToolActivityState::Running,
                    started_ts_ms,
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
                if step.recovery_depth > 0 {
                    yield Ok::<Event, Infallible>(
                        Event::default().event("assistant_recovery_attempt").data(
                            json!({
                                "step_index": step.step_index,
                                "tool": call.tool.as_str(),
                                "edge": step.edge_label,
                                "recovery_depth": step.recovery_depth,
                            })
                            .to_string(),
                        ),
                    );
                }

                let tool_started = Instant::now();
                let tool_metrics = state.runtime_metrics.start_ai_tool_call();
                let block = execute_tool(&state, &context, &call).await;
                let latency_ms = tool_started.elapsed().as_millis() as u64;
                tool_duration_ms = tool_duration_ms.saturating_add(latency_ms);
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
                let outcome = tool_result_to_outcome(&req.message, &call, block.clone());
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
                yield Ok::<Event, Infallible>(sse_json_event(
                    "assistant_tool_attempt",
                    &AssistantToolAttemptEvent {
                        step_index: step.step_index,
                        tool: call.tool.as_str().to_string(),
                        outcome_kind: outcome.kind.as_str().to_string(),
                        recovery_depth: step.recovery_depth,
                        is_alternate: step.is_alternate,
                        latency_ms,
                    },
                ));

                match executor.record_step(step, outcome, source, latency_ms) {
                    ExecutorPostStep::Continue => {}
                    ExecutorPostStep::Stop => break,
                    ExecutorPostStep::AskClarification => {
                        if let Some(request) = executor.clarification() {
                            yield Ok::<Event, Infallible>(sse_json_event(
                                "assistant_clarification_required",
                                &AssistantClarificationSseEvent {
                                    message: request.message.clone(),
                                },
                            ));
                        }
                        break;
                    }
                }
            }

            attempted_tool_calls = executor
                .all_records()
                .iter()
                .map(|record| record.step.call.clone())
                .collect();
            let retained_records = executor.retained_records();
            planned_tools = retained_records
                .iter()
                .map(|record| record.step.call.clone())
                .collect();
            follow_up_contexts = retained_records
                .iter()
                .map(|record| build_follow_up_context(&record.step.call, &record.outcome.block))
                .collect();
            grounding_blocks = retained_records
                .iter()
                .map(|record| record.outcome.block.clone())
                .collect();
            grounding_sources = retained_records
                .iter()
                .map(|record| record.source.clone())
                .collect();
            grounded_executor = Some(executor);
        }

        grounding_chunks = build_grounding_chunks_for_turn(
            &state,
            &assistant_context,
            &req,
            &planned_tools,
            &grounding_blocks,
            &grounding_sources,
            &augmented_history,
        )
        .await;

        if !grounding_sources.is_empty() || !grounding_chunks.is_empty() {
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("grounding")
                    .data(json!({
                        "sources": &grounding_sources,
                        "follow_up_contexts": &follow_up_contexts,
                        "chunks": &grounding_chunks,
                    }).to_string()),
            );
        }

        let grounding_tools = planned_tools
            .iter()
            .map(|call| call.tool.as_str().to_string())
            .collect::<Vec<_>>();
        let mut execution_trace = grounded_executor.as_ref().map(|executor| executor.trace().clone());
        planner_debug.execution_trace = execution_trace.clone();

        if let Some(executor) = grounded_executor.as_mut()
            && let Some(request) = executor.clarification().cloned()
        {
            assistant_content = request.message;
            executor.finalize_bounded_failure();
            execution_trace = Some(executor.trace().clone());
            planner_debug.execution_trace = execution_trace.clone();
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Clarification,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "clarification_required".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "clarification".to_string()),
                    tool_step_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.tool_step_count)
                        .unwrap_or(0),
                    alternate_tool_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.alternate_tool_count)
                        .unwrap_or(0),
                    recovery_step_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.recovery_step_count)
                        .unwrap_or(0),
                },
            ));
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

        if let Some(datetime_reply) =
            deterministic_current_datetime_reply(&req.message, &req.history, &grounding_blocks)
        {
            assistant_content = datetime_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.tool_step_count)
                        .unwrap_or(0),
                    alternate_tool_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.alternate_tool_count)
                        .unwrap_or(0),
                    recovery_step_count: execution_trace
                        .as_ref()
                        .map(|trace| trace.recovery_step_count)
                        .unwrap_or(0),
                },
            ));
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

        if let Some(ai_runtime_reply) =
            deterministic_ai_runtime_reply(&req.message, &grounding_blocks)
        {
            assistant_content = ai_runtime_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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

        if let Some(calendar_reply) =
            deterministic_calendar_reply(&req.message, &grounding_blocks)
        {
            assistant_content = calendar_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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

        if let Some(network_reply) =
            deterministic_network_reply(&req.message, &grounding_blocks)
        {
            assistant_content = network_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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

        if let Some(library_reply) =
            deterministic_library_reply(&req.message, &grounding_blocks)
        {
            assistant_content = library_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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

        if let Some(weather_reply) = deterministic_weather_reply(&req.message, &grounding_blocks) {
            assistant_content = weather_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_deterministic_reply();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "deterministic_reply".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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

        if let Some(multi_step_reply) =
            deterministic_multi_step_reply(&req.message, execution_trace.as_ref(), &grounding_blocks)
        {
            assistant_content = multi_step_reply;
            if let Some(executor) = grounded_executor.as_mut() {
                executor.finalize_bounded_failure();
                execution_trace = Some(executor.trace().clone());
                planner_debug.execution_trace = execution_trace.clone();
            }
            stats = Some(build_server_authored_turn_stats(
                &req,
                &assistant_content,
                &grounding_chunks,
                planner_duration_ms,
                tool_duration_ms,
                turn_started.elapsed().as_millis() as u64,
                queue_duration_ms,
                model_load_duration_ms,
                Some(&planner_debug),
                execution_trace.as_ref(),
            ));
            if let Some(persistence) = &persistence {
                let turn_result = crate::ai_conversations::persist_assistant_turn(
                    &state,
                    &user.user_id,
                    &persistence.conversation_id,
                    &assistant_content,
                    &model_name,
                    &grounding_tools,
                    &follow_up_contexts,
                    &grounding_chunks,
                    &grounding_sources,
                    &activity_trace,
                    stats.as_ref(),
                    None,
                    Some(&trace_id),
                )
                .await;
                persist_turn_grounding_artifacts(
                    &state,
                    &assistant_context,
                    &persistence.conversation_id,
                    turn_result,
                    &grounding_chunks,
                    &follow_up_contexts,
                )
                .await;
            }
            persist_chat_audit_event_with_planner(
                &state,
                &user,
                &req,
                &trace_id,
                AiAssistantAuditResponseKind::Completed,
                &attempted_tool_calls,
                &grounding_blocks,
                &grounding_chunks,
                &grounding_sources,
                Some(&planner_debug),
                None,
            )
            .await;
            {
                let mut guard = state.engine.lock().await;
                guard.last_execution_trace = execution_trace.clone();
            }
            yield Ok::<Event, Infallible>(sse_json_event(
                "assistant_stop_reason",
                &AssistantStopReasonSseEvent {
                    reason: execution_trace
                        .as_ref()
                        .map(|trace| trace.stop_reason.as_str().to_string())
                        .unwrap_or_else(|| "bounded_failure".to_string()),
                    final_answer_path: execution_trace
                        .as_ref()
                        .map(|trace| trace.final_answer_path.as_str().to_string())
                        .unwrap_or_else(|| "bounded_failure".to_string()),
                    tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                    alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                    recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                },
            ));
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
        if let Some(executor) = grounded_executor.as_mut() {
            executor.finalize_model_answer();
            execution_trace = Some(executor.trace().clone());
            planner_debug.execution_trace = execution_trace.clone();
        }
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

        let prompt_request = crate::ai_assistant::AssistantChatRequest {
            model: model_name.clone(),
            message: req.message.clone(),
            response_mode: req.response_mode,
            confirmation_token: req.confirmation_token.clone(),
            history: augmented_history.clone(),
        };
        let sampling = rustfin_ai_agent::SamplingParams {
            max_tokens: scheduler_decision.max_generation_tokens,
            ..rustfin_ai_agent::SamplingParams::default()
        };
        let mut emergency_prompt_compaction = false;
        'answer_generation: loop {
            let (messages, current_prompt_debug) = build_assistant_messages_with_budget(
                prompt_request.clone(),
                &grounding_chunks,
                answer_context_length_tokens,
                emergency_prompt_compaction,
            );
            {
                let mut guard = state.engine.lock().await;
                guard.last_prompt_debug = Some(current_prompt_debug.clone());
            }

            let raw_stream = answer_backend.chat_stream_boxed(
                rustfin_ai_agent::ModelRole::Answer,
                messages,
                sampling.clone(),
                None,
            );
            futures::pin_mut!(raw_stream);
            let mut retry_after_context_overflow = false;

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
                        prefill_duration_ms: _prefill_duration_ms,
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
                        let mut built_stats = build_turn_stats_with_planner(
                            prompt_tokens,
                            completion_tokens,
                            total_duration_ms,
                            planner_duration_ms,
                            tool_duration_ms,
                            turn_started.elapsed().as_millis() as u64,
                            queue_duration_ms,
                            model_load_duration_ms,
                            tokens_per_second,
                            Some(&planner_debug),
                            execution_trace.as_ref(),
                        );
                        apply_prompt_debug_to_stats(&mut built_stats, &current_prompt_debug);
                        stats = Some(built_stats);
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
                            stats = Some(build_estimated_model_turn_stats(
                                &req,
                                &assistant_content,
                                &grounding_chunks,
                                generation_duration_ms,
                                planner_duration_ms,
                                tool_duration_ms,
                                turn_started.elapsed().as_millis() as u64,
                                queue_duration_ms,
                                model_load_duration_ms,
                                Some(&planner_debug),
                                execution_trace.as_ref(),
                                Some(&current_prompt_debug),
                            ));
                            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
                        }

                        if let Some(persistence) = &persistence {
                            let grounding_tools = planned_tools
                                .iter()
                                .map(|call| call.tool.as_str().to_string())
                                .collect::<Vec<_>>();
                            let turn_result = crate::ai_conversations::persist_assistant_turn(
                                &state,
                                &user.user_id,
                                &persistence.conversation_id,
                                &assistant_content,
                                &model_name,
                                &grounding_tools,
                                &follow_up_contexts,
                                &grounding_chunks,
                                &grounding_sources,
                                &activity_trace,
                                stats.as_ref(),
                                None,
                                Some(&trace_id),
                            )
                            .await;
                            persist_turn_grounding_artifacts(
                                &state,
                                &assistant_context,
                                &persistence.conversation_id,
                                turn_result,
                                &grounding_chunks,
                                &follow_up_contexts,
                            )
                            .await;
                        }

                        persist_chat_audit_event_with_planner(
                            &state,
                            &user,
                            &req,
                            &trace_id,
                            AiAssistantAuditResponseKind::Completed,
                            &attempted_tool_calls,
                            &grounding_blocks,
                            &grounding_chunks,
                            &grounding_sources,
                            Some(&planner_debug),
                            None,
                        )
                        .await;
                        {
                            let mut guard = state.engine.lock().await;
                            guard.last_execution_trace = execution_trace.clone();
                        }
                        yield Ok::<Event, Infallible>(sse_json_event(
                            "assistant_stop_reason",
                            &AssistantStopReasonSseEvent {
                                reason: execution_trace
                                    .as_ref()
                                    .map(|trace| trace.stop_reason.as_str().to_string())
                                    .unwrap_or_else(|| "model_answer_completed".to_string()),
                                final_answer_path: execution_trace
                                    .as_ref()
                                    .map(|trace| trace.final_answer_path.as_str().to_string())
                                    .unwrap_or_else(|| "model_answer".to_string()),
                                tool_step_count: execution_trace.as_ref().map(|trace| trace.tool_step_count).unwrap_or(0),
                                alternate_tool_count: execution_trace.as_ref().map(|trace| trace.alternate_tool_count).unwrap_or(0),
                                recovery_step_count: execution_trace.as_ref().map(|trace| trace.recovery_step_count).unwrap_or(0),
                            },
                        ));
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
                        let mut error_message = error.to_string();
                        if is_context_overflow_error(&error_message) && !emergency_prompt_compaction {
                            warn!(
                                trace_id = %trace_id,
                                user_id = %user.user_id,
                                model = %model_name,
                                prompt_budget_tokens = current_prompt_debug.prompt_budget_tokens,
                                context_length_tokens = answer_context_length_tokens,
                                error = %error_message,
                                "ai chat prompt exceeded context; retrying with emergency history compaction"
                            );
                            assistant_content.clear();
                            stats = None;
                            emergency_prompt_compaction = true;
                            retry_after_context_overflow = true;
                            break;
                        }
                        if is_context_overflow_error(&error_message) {
                            error_message = "This conversation is too large to fit the current AI model context, even after compaction. Start a new chat or remove the very large earlier turn.".to_string();
                        }
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
                            persist_chat_audit_event_with_planner(
                                &state,
                                &user,
                                &req,
                                &trace_id,
                                AiAssistantAuditResponseKind::StreamError,
                                &planned_tools,
                                &grounding_blocks,
                                &grounding_chunks,
                                &grounding_sources,
                                Some(&planner_debug),
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

            if retry_after_context_overflow {
                continue 'answer_generation;
            }

            break 'answer_generation;
        }
    };

    Sse::new(Box::pin(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn set_engine_phase(state: &AppState, phase: AssistantRuntimePhase) {
    let mut guard = state.engine.lock().await;
    guard.active_phase = phase;
}

async fn persist_turn_grounding_artifacts(
    state: &AppState,
    context: &AssistantContext,
    conversation_id: &str,
    turn_result: Result<String, AppError>,
    grounding_chunks: &[AssistantGroundingChunk],
    follow_up_contexts: &[AssistantFollowUpContext],
) {
    if let Ok(turn_id) = turn_result {
        persist_grounding_artifacts(
            state,
            context,
            conversation_id,
            &turn_id,
            grounding_chunks,
            follow_up_contexts,
        )
        .await;
    }
}

pub(crate) fn engine_params_from_env() -> rustfin_ai_agent::LlamaEngineParams {
    rustfin_ai_agent::LlamaEngineParams {
        split_mode: parse_gpu_split_mode_from_env(),
        main_gpu: parse_i32_env("RUSTFIN_AI_GPU_MAIN_DEVICE"),
        device_indices: parse_device_indices_env("RUSTFIN_AI_GPU_DEVICES"),
        ..rustfin_ai_agent::LlamaEngineParams::default()
    }
}

pub(crate) fn engine_params_for_profile(
    profile: Option<&rustfin_db::repo::ai_models::AiModelProfileRow>,
) -> rustfin_ai_agent::LlamaEngineParams {
    let mut params = profile
        .map(|profile| rustfin_ai_agent::LlamaEngineParams {
            n_gpu_layers: profile.recommended_n_gpu_layers,
            tensor_split: Vec::new(),
            split_mode: parse_gpu_split_mode_override(Some(&profile.recommended_split_mode)),
            main_gpu: profile.recommended_main_gpu,
            device_indices: serde_json::from_str(&profile.recommended_device_indices_json)
                .unwrap_or_default(),
            n_ctx: u32::try_from(profile.context_window.max(1)).unwrap_or(4096),
            n_threads: u32::try_from(profile.recommended_n_threads.max(1)).unwrap_or(8),
        })
        .unwrap_or_default();

    if std::env::var("RUSTFIN_AI_GPU_SPLIT_MODE").is_ok() {
        params.split_mode = parse_gpu_split_mode_from_env();
    }
    if let Some(main_gpu) = parse_i32_env("RUSTFIN_AI_GPU_MAIN_DEVICE") {
        params.main_gpu = Some(main_gpu);
    }
    let device_indices = parse_device_indices_env("RUSTFIN_AI_GPU_DEVICES");
    if !device_indices.is_empty() {
        params.device_indices = device_indices;
    }

    params
}

fn estimated_model_bytes(
    model_size_bytes: u64,
    profile: Option<&rustfin_db::repo::ai_models::AiModelProfileRow>,
) -> u64 {
    profile
        .and_then(|profile| u64::try_from(profile.estimated_model_bytes).ok())
        .filter(|bytes| *bytes > 0)
        .unwrap_or_else(|| model_size_bytes.saturating_mul(3).saturating_div(2))
}

fn role_route(
    routes: &[crate::ai_model_routing::ResolvedRoleRouting],
    role: rustfin_ai_agent::ModelRole,
) -> Option<&crate::ai_model_routing::ResolvedRoleRouting> {
    routes.iter().find(|route| route.decision.role == role)
}

fn find_loaded_backend_for_route(
    loaded_role_models: &HashMap<rustfin_ai_agent::ModelRole, LoadedRoleModel>,
    route: &crate::ai_model_routing::ResolvedRoleRouting,
) -> Option<LoadedRoleModel> {
    loaded_role_models
        .values()
        .find(|loaded| {
            loaded.model_name == route.selection.model_name
                && loaded.backend_id == route.decision.backend_id
                && loaded.backend_kind == route.decision.backend_kind
        })
        .cloned()
}

async fn load_model_profiles_for_host(
    state: &AppState,
) -> Vec<rustfin_db::repo::ai_models::AiModelProfileRow> {
    let host_fingerprint = crate::ai_admin::host_fingerprint();
    rustfin_db::repo::ai_models::list_model_profiles_for_host(&state.db, &host_fingerprint, 200)
        .await
        .unwrap_or_default()
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        LoadedRoleModel, assistant_text_for_confirmed_action, build_server_authored_turn_stats,
        build_turn_stats_with_planner, find_loaded_backend_for_route, grounding_recovery_plan,
        is_context_overflow_error, parse_device_indices_override, parse_gpu_split_mode_override,
        parse_i32_override,
    };
    use crate::ai_assistant::build_assistant_messages_with_budget;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{
        AssistantChatRequest, AssistantGroundingChunk, AssistantGroundingVisibility,
        AssistantHistoryMessage, AssistantPlannerDebug, AssistantResponseMode,
        AssistantToolContextBlock, AssistantToolInput, PlannedToolCall, PlannerExecutionStats,
        PlannerFallbackReason,
    };
    use futures::stream::BoxStream;
    use rustfin_ai_agent::engine::LlamaGpuSplitMode;
    use rustfin_ai_agent::{
        BackendCapabilities, BackendKind, ChatChunk, ChatMessage, InferenceBackend, ModelRole,
        PromptCacheHint, SamplingParams,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[derive(Clone)]
    struct DummyBackend;

    impl InferenceBackend for DummyBackend {
        fn backend_id(&self) -> &'static str {
            "local_llama"
        }

        fn backend_kind(&self) -> BackendKind {
            BackendKind::Local
        }

        fn model_name(&self) -> Option<&str> {
            Some("planner.gguf")
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                kind: BackendKind::Local,
                supports_streaming: true,
                supports_prompt_cache: false,
                supports_structured_output: true,
                can_degrade: true,
                max_parallel_requests: 1,
            }
        }

        fn chat_stream_boxed(
            &self,
            _role: ModelRole,
            _messages: Vec<ChatMessage>,
            _sampling: SamplingParams,
            _prompt_cache: Option<PromptCacheHint>,
        ) -> BoxStream<'static, Result<ChatChunk, rustfin_ai_agent::AiError>> {
            Box::pin(futures::stream::empty())
        }
    }

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
    fn build_turn_stats_with_planner_carries_planner_counters() {
        let planner_debug = AssistantPlannerDebug {
            validation_errors: vec!["bad arg".to_string(), "bad enum".to_string()],
            repair_attempt_count: 1,
            execution: PlannerExecutionStats {
                parse_attempts: 2,
                validation_failures: 1,
                repair_attempts: 1,
                repair_successes: 1,
                fallback_reason: Some(PlannerFallbackReason::ArgumentInvalid),
            },
            ..AssistantPlannerDebug::default()
        };

        let stats = build_turn_stats_with_planner(
            10,
            5,
            20,
            7,
            3,
            40,
            2,
            1,
            12.0,
            Some(&planner_debug),
            None,
        );

        assert_eq!(stats.planner_validation_error_count, 2);
        assert_eq!(stats.planner_repair_count, 1);
        assert_eq!(stats.planner_parse_attempts, 2);
        assert_eq!(stats.planner_validation_failures, 1);
        assert_eq!(stats.planner_repair_attempts, 1);
        assert_eq!(stats.planner_repair_successes, 1);
        assert_eq!(
            stats.planner_fallback_reason.as_deref(),
            Some("argument_invalid")
        );
    }

    #[test]
    fn build_server_authored_turn_stats_estimates_prompt_and_completion_tokens() {
        let stats = build_server_authored_turn_stats(
            &AssistantChatRequest {
                model: "test.gguf".to_string(),
                message: "What is the weather in Campile?".to_string(),
                response_mode: AssistantResponseMode::Thinking,
                confirmation_token: None,
                history: vec![AssistantHistoryMessage {
                    role: "user".to_string(),
                    content: "What was it like yesterday in Campile?".to_string(),
                    grounding_tools: Vec::new(),
                    follow_up_contexts: Vec::new(),
                    grounding_chunks: Vec::new(),
                }],
            },
            "Forecast for Campile, County Wexford, Leinster, Ireland over the next 7 days.",
            &[AssistantGroundingChunk {
                id: "grounding:test".to_string(),
                source_kind: "weather_get_forecast".to_string(),
                title: "Weather grounding".to_string(),
                excerpt: "Campile, County Wexford, Leinster, Ireland".to_string(),
                score: 1.0,
                visibility: AssistantGroundingVisibility::User,
                topic_key: None,
                owner_user_id: None,
                source_id: None,
                source_sub_id: None,
                citation: None,
            }],
            5_000,
            250,
            5_600,
            100,
            50,
            None,
            None,
        );

        assert!(stats.prompt_tokens > 0);
        assert!(stats.completion_tokens > 0);
        assert_eq!(stats.generation_duration_ms, 0);
        assert!(stats.tokens_per_second > 0.0);
    }

    #[test]
    fn build_assistant_messages_with_budget_compacts_oversized_history() {
        let request = AssistantChatRequest {
            model: "test.gguf".to_string(),
            message: "hello?".to_string(),
            response_mode: AssistantResponseMode::Thinking,
            confirmation_token: None,
            history: (0..12)
                .flat_map(|index| {
                    [
                        AssistantHistoryMessage {
                            role: "user".to_string(),
                            content: format!(
                                "user message {index}: {}",
                                "books and basements ".repeat(80)
                            ),
                            grounding_tools: Vec::new(),
                            follow_up_contexts: Vec::new(),
                            grounding_chunks: Vec::new(),
                        },
                        AssistantHistoryMessage {
                            role: "assistant".to_string(),
                            content: format!(
                                "assistant reply {index}: {}",
                                "3.1415926535 ".repeat(160)
                            ),
                            grounding_tools: Vec::new(),
                            follow_up_contexts: Vec::new(),
                            grounding_chunks: Vec::new(),
                        },
                    ]
                })
                .collect(),
        };

        let (messages, prompt_debug) =
            build_assistant_messages_with_budget(request, &[], 4096, false);
        let estimated_tokens = messages.iter().fold(0_u64, |total, message| {
            let role_cost = (message.role.len() / 4).saturating_add(4) as u64;
            let content_cost = (message.content.len() / 4).saturating_add(1) as u64;
            total.saturating_add(role_cost).saturating_add(content_cost)
        });

        assert!(estimated_tokens <= u64::from(prompt_debug.prompt_budget_tokens));
        assert!(prompt_debug.summarized_turns > 0);
        assert!(prompt_debug.compact_boundary_count > 0);
        assert!(prompt_debug.history_message_count > 0);
    }

    #[test]
    fn context_overflow_error_detection_matches_llama_error() {
        assert!(is_context_overflow_error(
            "context build error: prompt token count (7828) exceeds context (4096)"
        ));
        assert!(!is_context_overflow_error(
            "inference error: llama decode failed"
        ));
    }

    #[test]
    fn confirmed_group_move_action_uses_verified_titles() {
        let response = assistant_text_for_confirmed_action(&AssistantToolContextBlock {
            tool: "conversations_move_to_group_selection",
            label: "Move AI conversations".to_string(),
            status: "ok",
            data: json!({
                "conversation_count": 1,
                "group_name": "test",
                "conversations": [
                    {
                        "id": "conversation-1",
                        "title": "Alpha"
                    }
                ]
            }),
        });

        assert_eq!(response, "I moved \"Alpha\" into group \"test\".");
    }

    #[test]
    fn same_model_roles_reuse_loaded_backend() {
        let shared_backend: Arc<dyn InferenceBackend> = Arc::new(DummyBackend);
        let mut loaded = HashMap::new();
        loaded.insert(
            ModelRole::Answer,
            LoadedRoleModel {
                model_name: "planner.gguf".to_string(),
                backend_id: "local_llama".to_string(),
                backend_kind: BackendKind::Local,
                backend: shared_backend.clone(),
            },
        );

        let route = crate::ai_model_routing::ResolvedRoleRouting {
            selection: rustfin_ai_agent::RoleModelSelection {
                model_name: "planner.gguf".to_string(),
                source: rustfin_ai_agent::ModelSelectionSource::StoredRecommendation,
            },
            decision: crate::ai_model_routing::RoleRoutingDecision {
                role: ModelRole::Planner,
                model_name: "planner.gguf".to_string(),
                backend_id: "local_llama".to_string(),
                backend_kind: BackendKind::Local,
                selection_source: rustfin_ai_agent::ModelSelectionSource::StoredRecommendation,
                recommendation_status:
                    crate::ai_benchmark_recommendations::BenchmarkRecommendationStatus::Applied,
                recommendation_note: None,
                recommendation_model_name: Some("planner.gguf".to_string()),
                recommendation_updated_ts: Some(123),
            },
            tuning_profile: None,
        };

        let reused = find_loaded_backend_for_route(&loaded, &route)
            .expect("planner route should reuse the loaded answer backend");
        assert!(Arc::ptr_eq(&reused.backend, &shared_backend));
    }

    #[test]
    fn grounding_recovery_plan_expands_empty_birthday_lookup_for_thinking_mode() {
        let planned_tools = vec![PlannedToolCall {
            tool: AssistantToolName::CalendarUpcomingBirthdays,
            input: AssistantToolInput::CalendarWindow {
                from_date: "2026-04-04".to_string(),
                to_date: "2026-05-04".to_string(),
                label: "the next 30 days".to_string(),
                query: Some("next".to_string()),
            },
        }];
        let grounding_blocks = vec![AssistantToolContextBlock {
            tool: "calendar_upcoming_birthdays",
            label: "Birthdays matching \"next\" for the next 30 days".to_string(),
            status: "ok",
            data: json!({
                "window": { "label": "the next 30 days" },
                "query": "next",
                "birthdays": [],
            }),
        }];

        let recovery = grounding_recovery_plan(
            "What's the next birthday in my calendar?",
            AssistantResponseMode::Thinking,
            &planned_tools,
            &grounding_blocks,
        )
        .expect("expected a recovery plan");

        assert_eq!(recovery.replace_index, 0);
        match recovery.call.input {
            AssistantToolInput::CalendarWindow { label, query, .. } => {
                assert_eq!(label, "the next 366 days");
                assert_eq!(query, None);
            }
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn grounding_recovery_plan_skips_instant_mode() {
        let planned_tools = vec![PlannedToolCall {
            tool: AssistantToolName::CalendarUpcomingBirthdays,
            input: AssistantToolInput::CalendarWindow {
                from_date: "2026-04-04".to_string(),
                to_date: "2026-05-04".to_string(),
                label: "the next 30 days".to_string(),
                query: Some("next".to_string()),
            },
        }];
        let grounding_blocks = vec![AssistantToolContextBlock {
            tool: "calendar_upcoming_birthdays",
            label: "Birthdays matching \"next\" for the next 30 days".to_string(),
            status: "ok",
            data: json!({
                "window": { "label": "the next 30 days" },
                "query": "next",
                "birthdays": [],
            }),
        }];

        assert!(
            grounding_recovery_plan(
                "What's the next birthday in my calendar?",
                AssistantResponseMode::Instant,
                &planned_tools,
                &grounding_blocks,
            )
            .is_none()
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
            .unwrap_or("Rustyfin AI could not complete that calendar action.")
            .to_string();
    }

    if block.tool == "conversations_archive_selection" {
        let count = block
            .data
            .get("conversation_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let titles = block
            .data
            .get("conversations")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(3)
                    .filter_map(|item| item.get("title").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut response = if count == 1 {
            format!(
                "I moved \"{}\" into the archive.",
                titles.first().copied().unwrap_or("that conversation")
            )
        } else {
            format!("I moved {count} AI conversations into the archive")
        };
        if count > 1 && !titles.is_empty() {
            response.push_str(": ");
            response.push_str(
                &titles
                    .into_iter()
                    .map(|title| format!("\"{title}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if count > 3 {
                response.push_str(&format!(", and {} more", count.saturating_sub(3)));
            }
            response.push('.');
        } else if !response.ends_with('.') {
            response.push('.');
        }
        return response;
    }

    if block.tool == "conversations_delete_selection" {
        let count = block
            .data
            .get("conversation_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let titles = block
            .data
            .get("conversations")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(3)
                    .filter_map(|item| item.get("title").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut response = if count == 1 {
            format!(
                "I permanently deleted \"{}\" from your AI history.",
                titles.first().copied().unwrap_or("that conversation")
            )
        } else {
            format!("I permanently deleted {count} AI conversations")
        };
        if count > 1 && !titles.is_empty() {
            response.push_str(": ");
            response.push_str(
                &titles
                    .into_iter()
                    .map(|title| format!("\"{title}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if count > 3 {
                response.push_str(&format!(", and {} more", count.saturating_sub(3)));
            }
            response.push('.');
        } else if !response.ends_with('.') {
            response.push('.');
        }
        return response;
    }

    if block.tool == "conversations_move_to_group_selection" {
        let count = block
            .data
            .get("conversation_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let group_name = block
            .data
            .get("group_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("that group");
        let titles = block
            .data
            .get("conversations")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(3)
                    .filter_map(|item| item.get("title").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut response = if count == 1 {
            format!(
                "I moved \"{}\" into group \"{group_name}\".",
                titles.first().copied().unwrap_or("that conversation")
            )
        } else {
            format!("I moved {count} AI conversations into group \"{group_name}\"")
        };
        if count > 1 && !titles.is_empty() {
            response.push_str(": ");
            response.push_str(
                &titles
                    .into_iter()
                    .map(|title| format!("\"{title}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if count > 3 {
                response.push_str(&format!(", and {} more", count.saturating_sub(3)));
            }
            response.push('.');
        } else if !response.ends_with('.') {
            response.push('.');
        }
        return response;
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

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn build_server_authored_turn_stats(
    request: &AssistantChatRequest,
    assistant_content: &str,
    grounding_chunks: &[AssistantGroundingChunk],
    planner_duration_ms: u64,
    tool_duration_ms: u64,
    end_to_end_duration_ms: u64,
    queue_duration_ms: u64,
    model_load_duration_ms: u64,
    planner_debug: Option<&AssistantPlannerDebug>,
    execution_trace: Option<&AssistantExecutionTrace>,
) -> AssistantTurnStats {
    let prompt_tokens = estimated_prompt_tokens(request, grounding_chunks);
    let completion_tokens = estimated_completion_tokens(assistant_content);
    let tokens_per_second = estimated_server_authored_tokens_per_second(
        completion_tokens,
        planner_duration_ms,
        tool_duration_ms,
        end_to_end_duration_ms,
        queue_duration_ms,
        model_load_duration_ms,
    );
    build_turn_stats_with_planner(
        prompt_tokens,
        completion_tokens,
        0,
        planner_duration_ms,
        tool_duration_ms,
        end_to_end_duration_ms,
        queue_duration_ms,
        model_load_duration_ms,
        tokens_per_second,
        planner_debug,
        execution_trace,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_estimated_model_turn_stats(
    request: &AssistantChatRequest,
    assistant_content: &str,
    grounding_chunks: &[AssistantGroundingChunk],
    generation_duration_ms: u64,
    planner_duration_ms: u64,
    tool_duration_ms: u64,
    end_to_end_duration_ms: u64,
    queue_duration_ms: u64,
    model_load_duration_ms: u64,
    planner_debug: Option<&AssistantPlannerDebug>,
    execution_trace: Option<&AssistantExecutionTrace>,
    prompt_debug: Option<&ConversationPromptDebug>,
) -> AssistantTurnStats {
    let prompt_tokens = estimated_prompt_tokens(request, grounding_chunks);
    let completion_tokens = estimated_completion_tokens(assistant_content);
    let tokens_per_second = if generation_duration_ms > 0 && completion_tokens > 0 {
        (completion_tokens as f64) / ((generation_duration_ms as f64) / 1000.0)
    } else {
        0.0
    };
    let mut stats = build_turn_stats_with_planner(
        prompt_tokens,
        completion_tokens,
        generation_duration_ms,
        planner_duration_ms,
        tool_duration_ms,
        end_to_end_duration_ms,
        queue_duration_ms,
        model_load_duration_ms,
        tokens_per_second,
        planner_debug,
        execution_trace,
    );
    if let Some(prompt_debug) = prompt_debug {
        apply_prompt_debug_to_stats(&mut stats, prompt_debug);
    }
    stats
}

fn estimated_prompt_tokens(
    request: &AssistantChatRequest,
    grounding_chunks: &[AssistantGroundingChunk],
) -> u64 {
    let messages = build_assistant_messages(request.clone(), grounding_chunks);
    estimate_chat_message_tokens(&messages)
}

fn estimated_completion_tokens(assistant_content: &str) -> u64 {
    if assistant_content.trim().is_empty() {
        return 0;
    }
    estimate_chat_message_tokens(&[rustfin_ai_agent::ChatMessage {
        role: "assistant".to_string(),
        content: assistant_content.to_string(),
    }])
}

fn estimate_chat_message_tokens(messages: &[rustfin_ai_agent::ChatMessage]) -> u64 {
    messages
        .iter()
        .fold(0_u64, |total, message| {
            let role_cost = (message.role.len() / 4).saturating_add(4) as u64;
            let content_cost = (message.content.len() / 4).saturating_add(1) as u64;
            total.saturating_add(role_cost).saturating_add(content_cost)
        })
        .max(1)
}

fn estimated_server_authored_tokens_per_second(
    completion_tokens: u64,
    planner_duration_ms: u64,
    tool_duration_ms: u64,
    end_to_end_duration_ms: u64,
    queue_duration_ms: u64,
    model_load_duration_ms: u64,
) -> f64 {
    if completion_tokens == 0 {
        return 0.0;
    }

    let estimated_authoring_duration_ms = if planner_duration_ms > 0 {
        planner_duration_ms
    } else {
        end_to_end_duration_ms
            .saturating_sub(tool_duration_ms)
            .saturating_sub(queue_duration_ms)
            .saturating_sub(model_load_duration_ms)
    };

    if estimated_authoring_duration_ms == 0 {
        return 0.0;
    }

    (completion_tokens as f64) / ((estimated_authoring_duration_ms as f64) / 1000.0)
}

#[allow(clippy::too_many_arguments)]
fn build_turn_stats_with_planner(
    prompt_tokens: u64,
    completion_tokens: u64,
    generation_duration_ms: u64,
    planner_duration_ms: u64,
    tool_duration_ms: u64,
    end_to_end_duration_ms: u64,
    queue_duration_ms: u64,
    model_load_duration_ms: u64,
    tokens_per_second: f64,
    planner_debug: Option<&AssistantPlannerDebug>,
    execution_trace: Option<&AssistantExecutionTrace>,
) -> AssistantTurnStats {
    let mut stats = AssistantTurnStats {
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
        ..AssistantTurnStats::default()
    };

    if let Some(debug) = planner_debug {
        stats.planner_validation_error_count = debug.validation_errors.len() as u32;
        stats.planner_repair_count = debug.repair_attempt_count;
        stats.planner_parse_attempts = debug.execution.parse_attempts;
        stats.planner_validation_failures = debug.execution.validation_failures;
        stats.planner_repair_attempts = debug.execution.repair_attempts;
        stats.planner_repair_successes = debug.execution.repair_successes;
        stats.planner_fallback_reason = debug
            .execution
            .fallback_reason
            .map(|reason| reason.as_str().to_string());
    }

    if let Some(trace) = execution_trace {
        stats.tool_step_count = trace.tool_step_count;
        stats.alternate_tool_count = trace.alternate_tool_count;
        stats.recovery_step_count = trace.recovery_step_count;
        stats.attempt_count = trace.attempts.len() as u32;
        stats.clarification_count = trace.clarification_count;
        stats.conflict_count = trace.conflict_count;
        stats.stop_reason = Some(trace.stop_reason.as_str().to_string());
        stats.final_outcome_kind = trace
            .final_outcome_kind
            .map(|kind| kind.as_str().to_string());
        stats.deterministic_answer_used = trace.deterministic_answer_used;
        stats.synthesis_used = trace.synthesis_used;
        stats.role_backend_usage = trace.used_role_backends.clone();
        stats.execution_trace = Some(trace.clone());
    }

    stats
}

fn apply_prompt_debug_to_stats(
    stats: &mut AssistantTurnStats,
    prompt_debug: &ConversationPromptDebug,
) {
    stats.context_length_tokens = prompt_debug.context_length_tokens;
    stats.prompt_budget_tokens = prompt_debug.prompt_budget_tokens;
    stats.reserved_completion_tokens = prompt_debug.reserved_completion_tokens;
    stats.completion_budget_tokens = prompt_debug.completion_budget_tokens;
    stats.loaded_history_turns = prompt_debug.loaded_history_turns;
    stats.retained_raw_turns = prompt_debug.retained_raw_turns;
    stats.summarized_turns = prompt_debug.summarized_turns;
    stats.recent_grounded_context_count = prompt_debug.grounding_chunk_count;
    stats.compact_boundary_count = prompt_debug.compact_boundary_count;
}

fn is_context_overflow_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("context build error")
        && (lower.contains("prompt token count") || lower.contains("exceeds context"))
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
            scope,
            title,
            event_date,
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
        AssistantToolInput::ConversationArchive {
            conversation_ids,
            selection_label,
            archived,
            ..
        } => format!(
            "conversations_archive:count={}:archived={archived}:selection={selection_label}",
            conversation_ids.len()
        ),
        AssistantToolInput::ConversationDelete {
            conversation_ids,
            selection_label,
            ..
        } => format!(
            "conversations_delete:count={}:selection={selection_label}",
            conversation_ids.len()
        ),
        AssistantToolInput::ConversationMoveToGroup {
            conversation_ids,
            selection_label,
            group_name,
            ..
        } => format!(
            "conversations_move_to_group:count={}:group={group_name}:selection={selection_label}",
            conversation_ids.len()
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
        AssistantToolInput::CurrentDateTime { location } => format!(
            "current_datetime:location={}",
            location.as_deref().unwrap_or("host")
        ),
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

#[derive(Debug, Clone)]
#[cfg(test)]
struct GroundingRecoveryPlan {
    replace_index: usize,
    call: PlannedToolCall,
}

#[cfg(test)]
fn grounding_recovery_plan(
    message: &str,
    response_mode: AssistantResponseMode,
    planned_tools: &[PlannedToolCall],
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<GroundingRecoveryPlan> {
    if matches!(response_mode, AssistantResponseMode::Instant) {
        return None;
    }

    planned_tools
        .iter()
        .zip(grounding_blocks.iter())
        .enumerate()
        .find_map(|(index, (call, block))| {
            if call.tool
                != crate::ai_assistant::registry::AssistantToolName::CalendarUpcomingBirthdays
                || block.status != "ok"
                || !birthday_block_is_empty(block)
            {
                return None;
            }

            let recovered_input = crate::ai_assistant::orchestrator::birthday_calendar_window_input(
                message,
                crate::ai_assistant::orchestrator::extract_birthday_query(message),
            );
            if recovered_input == call.input {
                return None;
            }

            Some(GroundingRecoveryPlan {
                replace_index: index,
                call: PlannedToolCall {
                    tool: call.tool,
                    input: recovered_input,
                },
            })
        })
}

#[cfg(test)]
fn birthday_block_is_empty(block: &AssistantToolContextBlock) -> bool {
    block.tool == "calendar_upcoming_birthdays"
        && block
            .data
            .get("birthdays")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
}
