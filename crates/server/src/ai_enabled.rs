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
use crate::ai_assistant::dates::assistant_local_now;
use crate::ai_assistant::memory::{
    ConversationMemoryState, ConversationPromptDebug, build_generation_prompt_messages,
    build_memory_update_messages, fallback_memory_update, parse_memory_update_response,
};
use crate::ai_assistant::profiles::{
    AssistantModelProfile, assistant_model_profile, configured_ai_max_concurrent_requests,
};
use crate::ai_assistant::tools::{build_follow_up_context, execute_tool, source_from_block};
use crate::ai_assistant::types::{
    AssistantActivityTraceItem, AssistantArtifactVerificationDebug, AssistantConfirmationPayload,
    AssistantConfirmationRequiredEvent, AssistantFollowUpContext, AssistantGroundingChunk,
    AssistantGroundingSource, AssistantHistoryMessage, AssistantPendingAction,
    AssistantPendingActionStatus, AssistantPhase, AssistantPhaseEvent, AssistantPlannerDebug,
    AssistantResponseMode, AssistantRuntimePhase, AssistantStatusEvent, AssistantStatusKind,
    AssistantToolActivityEvent, AssistantToolActivityState, AssistantToolContextBlock,
    AssistantToolInput, AssistantTurnStats,
};
use crate::ai_assistant::memory::{
    augment_history_with_entity_graph, build_grounding_chunks_for_turn, persist_grounding_artifacts,
};
use crate::ai_assistant::weather::deterministic_weather_reply;
use crate::ai_assistant::{
    AssistantChatRequest, build_system_prompt, deterministic_calendar_reply,
    deterministic_current_datetime_reply, deterministic_downloads_reply,
    deterministic_network_reply, deterministic_profile_reply, deterministic_rooms_reply,
    deterministic_runtime_reply, deterministic_service_reply, immediate_response_for_message,
    plan_tool_calls_with_model_assist, status_label_for_tool_call,
    unsupported_write_response_for_message,
};
use crate::ai_audit::{AiAssistantAuditResponseKind, persist_chat_audit_event};
use crate::ai_conversations::{
    ConversationMessageRequest, load_conversation_memory_checkpoint, persist_conversation_memory,
};
use crate::ai_storage::{
    AiModelSummary, current_model_dir, list_models_with_storage_status, model_file_path,
};
use crate::ai_turn_journal::{
    AiTurnJournalStatus, TurnJournalHandle, create_turn_journal, update_turn_journal,
};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
    pub active_phase: AssistantRuntimePhase,
    pub last_prompt_debug: Option<ConversationPromptDebug>,
    pub last_turn_stats: Option<AssistantTurnStats>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
            active_phase: AssistantRuntimePhase::Idle,
            last_prompt_debug: None,
            last_turn_stats: None,
        }
    }
}

#[derive(Clone)]
struct ConversationPersistence {
    conversation_id: String,
    request_turn_id: String,
    request_turn_index: i64,
    memory_state: ConversationMemoryState,
    memory_turn_index: i64,
    recovered_from_compact_boundary: bool,
}

const MIN_CONTEXT_WINDOW_TOKENS: u32 = 1024;
const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 4096;
const MAX_CONTEXT_WINDOW_TOKENS: u32 = 32768;
const MEMORY_SUMMARY_MAX_DURATION_MS: u64 = 15_000;

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
    let (conversation, _, history) = crate::ai_conversations::load_conversation_request_context(
        &state,
        &user.user_id,
        &conversation_id,
    )
    .await?;

    let user_turn = crate::ai_conversations::persist_user_turn(
        &state,
        &user.user_id,
        &conversation_id,
        &req.message,
    )
    .await?;
    let (memory_state, memory_turn_index, recovered_from_compact_boundary) =
        load_conversation_memory_checkpoint(&state, &user.user_id, &conversation).await?;

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
        Some(ConversationPersistence {
            conversation_id,
            request_turn_id: user_turn.id,
            request_turn_index: user_turn.turn_index,
            memory_state,
            memory_turn_index,
            recovered_from_compact_boundary,
        }),
    ))
}

#[rustfmt::skip]
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
        let journal_handle = TurnJournalHandle {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user.user_id.clone(),
            conversation_id: persistence.as_ref().map(|value| value.conversation_id.clone()),
            request_turn_id: persistence.as_ref().map(|value| value.request_turn_id.clone()),
            request_turn_index: persistence.as_ref().map(|value| value.request_turn_index),
            trace_id: trace_id.clone(),
            request_message: req.message.clone(),
            model_name: model_name.clone(),
            response_mode: req.response_mode.as_str().to_string(),
        };
        let assistant_context = AssistantContext::new(&user, trace_id.clone());
        let turn_started = Instant::now();
        let chat_metrics = state.runtime_metrics.start_ai_chat_request();
        set_last_prompt_debug(&state, None).await;
        set_last_turn_stats(&state, None).await;
        let mut audit_written = false;
        let mut assistant_content = String::new();
        let mut activity_trace = Vec::<AssistantActivityTraceItem>::new();
        let mut grounding_blocks = Vec::<AssistantToolContextBlock>::new();
        let mut grounding_sources = Vec::<AssistantGroundingSource>::new();
        let mut grounding_chunks = Vec::<AssistantGroundingChunk>::new();
        let mut follow_up_contexts = Vec::<AssistantFollowUpContext>::new();
        let mut stats: Option<AssistantTurnStats> = None;
        let mut tool_duration_ms = 0_u64;
        let mut planner_debug = AssistantPlannerDebug::default();
        planner_debug.schema_version = crate::ai_assistant::profiles::PLANNER_SCHEMA_VERSION;
        let mut prompt_debug: Option<ConversationPromptDebug> = None;
        let mut compact_boundary_count = 0_u32;
        let mut artifact_verification = AssistantArtifactVerificationDebug::default();

        info!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            username = %user.username,
            model = %model_name,
            history_len,
            "ai chat request received"
        );

        if let Err(error) = create_turn_journal(&state, &journal_handle, req.history.len()).await {
            let error_message = format!("failed to persist AI turn journal before planning: {error:?}");
            warn!(
                trace_id = %trace_id,
                user_id = %user.user_id,
                model = %model_name,
                error = %error_message,
                "ai chat failed before turn journal persistence"
            );
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("error")
                    .data(json!({ "message": error_message }).to_string()),
            );
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            return;
        }

        let max_concurrent_requests = configured_ai_max_concurrent_requests();
        let active_request_count = state.runtime_metrics.snapshot().assistant.chats.calls_in_flight;
        if active_request_count > max_concurrent_requests {
            let overload_message = format!(
                "Rustyfin AI is busy right now with {active_request_count} active turn{} on this host. Please retry in a moment.",
                if active_request_count == 1 { "" } else { "s" }
            );
            let mut overload_stats = build_turn_stats(
                0,
                0,
                0,
                0,
                0,
                turn_started.elapsed().as_millis() as u64,
                0,
                0,
                0.0,
            );
            overload_stats.overload = true;
            overload_stats.overload_reason = Some(format!(
                "active_requests={active_request_count} exceeded limit={max_concurrent_requests}"
            ));
            overload_stats.journal_persisted = true;
            stats = Some(overload_stats.clone());
            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Overloaded,
                req.history.len(),
                None,
                &planner_debug,
                None,
                Some(&overload_stats),
                overload_stats.overload_reason.as_deref(),
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;
            set_last_turn_stats(&state, stats.clone()).await;
            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("token")
                    .data(json!({ "text": overload_message }).to_string()),
            );
            yield Ok::<Event, Infallible>(sse_json_event("stats", &stats));
            yield Ok::<Event, Infallible>(Event::default().event("done").data("{}"));
            set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
            return;
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
                    if let Some(stats_mut) = stats.as_mut() {
                        enrich_turn_stats(
                            stats_mut,
                            &planner_debug,
                            prompt_debug.as_ref(),
                            compact_boundary_count,
                            None,
                            None,
                        );
                    }
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
                            &[],
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
                    let _ = update_turn_journal(
                        &state,
                        &journal_handle,
                        AiTurnJournalStatus::Completed,
                        req.history.len(),
                        None,
                        &planner_debug,
                        prompt_debug.as_ref(),
                        stats.as_ref(),
                        None,
                        None,
                        compact_boundary_count,
                        None,
                        Some(now_ts_ms()),
                    )
                    .await;
                    chat_metrics.mark_success();
                    set_last_turn_stats(&state, stats.clone()).await;
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
            if let Some(verification) = artifact_verification_from_block(&block) {
                artifact_verification = verification;
            }
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
            if let Some(stats_mut) = stats.as_mut() {
                enrich_turn_stats(
                    stats_mut,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    Some(&artifact_verification),
                    None,
                );
            }

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

            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Completed,
                req.history.len(),
                None,
                &planner_debug,
                prompt_debug.as_ref(),
                stats.as_ref(),
                None,
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;

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
            set_last_turn_stats(&state, stats.clone()).await;
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
            if let Some(stats_mut) = stats.as_mut() {
                enrich_turn_stats(
                    stats_mut,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    Some(&artifact_verification),
                    None,
                );
            }
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
                    &[],
                    &[],
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
            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Completed,
                req.history.len(),
                None,
                &planner_debug,
                prompt_debug.as_ref(),
                stats.as_ref(),
                None,
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;
            chat_metrics.mark_success();
            set_last_turn_stats(&state, stats.clone()).await;
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
            if let Some(stats_mut) = stats.as_mut() {
                enrich_turn_stats(
                    stats_mut,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
            }
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
                    &[],
                    &[],
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
            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Completed,
                req.history.len(),
                None,
                &planner_debug,
                prompt_debug.as_ref(),
                stats.as_ref(),
                None,
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;
            chat_metrics.mark_success();
            set_last_turn_stats(&state, stats.clone()).await;
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
            if let Some(stats_mut) = stats.as_mut() {
                enrich_turn_stats(
                    stats_mut,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
            }
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
                    &[],
                    &[],
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
            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Completed,
                req.history.len(),
                None,
                &planner_debug,
                prompt_debug.as_ref(),
                stats.as_ref(),
                None,
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;
            chat_metrics.mark_success();
            set_last_turn_stats(&state, stats.clone()).await;
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
        let gguf_path = match model_file_path(&model_dir, &model_name) {
            Ok(path) => path,
            Err(error) => {
                let error_message = error.to_string();
                let mut error_stats = build_turn_stats(
                    0,
                    0,
                    0,
                    0,
                    0,
                    turn_started.elapsed().as_millis() as u64,
                    0,
                    0,
                    0.0,
                );
                enrich_turn_stats(
                    &mut error_stats,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
                stats = Some(error_stats.clone());
                let _ = update_turn_journal(
                    &state,
                    &journal_handle,
                    AiTurnJournalStatus::Failed,
                    req.history.len(),
                    None,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    Some(&error_stats),
                    None,
                    Some(&error_message),
                    compact_boundary_count,
                    None,
                    Some(now_ts_ms()),
                )
                .await;
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
                set_last_turn_stats(&state, stats.clone()).await;
                set_engine_phase(&state, AssistantRuntimePhase::Idle).await;
                return;
            }
        };

        let (engine, queue_duration_ms, model_load_duration_ms) = match load_engine_for_chat(
            &state,
            &model_name,
            &gguf_path,
        )
        .await
        {
            Ok((engine, queue_ms, load_ms)) => (engine, queue_ms, load_ms),
            Err(error_message) => {
                let mut error_stats = build_turn_stats(
                    0,
                    0,
                    0,
                    0,
                    0,
                    turn_started.elapsed().as_millis() as u64,
                    0,
                    0,
                    0.0,
                );
                enrich_turn_stats(
                    &mut error_stats,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
                stats = Some(error_stats.clone());
                let _ = update_turn_journal(
                    &state,
                    &journal_handle,
                    AiTurnJournalStatus::Failed,
                    req.history.len(),
                    None,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    Some(&error_stats),
                    None,
                    Some(&error_message),
                    compact_boundary_count,
                    None,
                    Some(now_ts_ms()),
                )
                .await;
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
                set_last_turn_stats(&state, stats.clone()).await;
                return;
            }
        };

        let planner_started = Instant::now();
        let planned_tool_set =
            plan_tool_calls_with_model_assist(&engine, &user, &req.message, &augmented_history)
                .await;
        let planner_duration_ms = planner_started.elapsed().as_millis() as u64;
        planner_debug = planned_tool_set.debug.clone();
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

        let _ = update_turn_journal(
            &state,
            &journal_handle,
            AiTurnJournalStatus::Accepted,
            req.history.len(),
            Some(planned_tool_set.mode.as_str()),
            &planner_debug,
            None,
            None,
            None,
            None,
            compact_boundary_count,
            None,
            None,
        )
        .await;

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
                if let Some(verification) = artifact_verification_from_block(&block) {
                    artifact_verification = verification;
                }
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
                            "chunks": &grounding_chunks,
                        }).to_string()),
                );
            }

            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Grounded,
                req.history.len(),
                Some(planned_tool_set.mode.as_str()),
                &planner_debug,
                None,
                None,
                None,
                None,
                compact_boundary_count,
                None,
                None,
            )
            .await;
        }

        if let Some(deterministic_reply) =
            deterministic_grounded_reply(&req.message, &req.history, &grounding_blocks)
        {
            assistant_content = deterministic_reply;
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

        if let Some(weather_reply) = deterministic_weather_reply(&req.message, &grounding_blocks) {
            assistant_content = weather_reply;
        }
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
            if let Some(stats_mut) = stats.as_mut() {
                enrich_turn_stats(
                    stats_mut,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
            }
            let grounding_tools = planned_tools
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect::<Vec<_>>();
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
            let _ = update_turn_journal(
                &state,
                &journal_handle,
                AiTurnJournalStatus::Completed,
                req.history.len(),
                Some(planned_tool_set.mode.as_str()),
                &planner_debug,
                prompt_debug.as_ref(),
                stats.as_ref(),
                None,
                None,
                compact_boundary_count,
                None,
                Some(now_ts_ms()),
            )
            .await;
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
            set_last_turn_stats(&state, stats.clone()).await;
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

        let (messages, prepared_prompt_debug, completion_budget, profile, compacted_boundaries) = match prepare_generation_messages(
            &state,
            &user,
            persistence.as_ref(),
            &engine,
            &req,
            &grounding_chunks,
        )
        .await {
            Ok(prepared) => prepared,
            Err(error_message) => {
                let mut error_stats = build_turn_stats(
                    0,
                    0,
                    0,
                    planner_duration_ms,
                    tool_duration_ms,
                    turn_started.elapsed().as_millis() as u64,
                    queue_duration_ms,
                    model_load_duration_ms,
                    0.0,
                );
                enrich_turn_stats(
                    &mut error_stats,
                    &planner_debug,
                    prompt_debug.as_ref(),
                    compact_boundary_count,
                    None,
                    None,
                );
                stats = Some(error_stats.clone());
                let _ = update_turn_journal(
                    &state,
                    &journal_handle,
                    AiTurnJournalStatus::Failed,
                    req.history.len(),
                    Some(planned_tool_set.mode.as_str()),
                    &planner_debug,
                    prompt_debug.as_ref(),
                    Some(&error_stats),
                    None,
                    Some(&error_message),
                    compact_boundary_count,
                    None,
                    Some(now_ts_ms()),
                )
                .await;
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
                yield Ok::<Event, Infallible>(
                    Event::default()
                        .event("error")
                        .data(json!({ "message": error_message }).to_string()),
                );
                set_last_turn_stats(&state, stats.clone()).await;
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
        compact_boundary_count = compact_boundary_count.max(compacted_boundaries);
        prompt_debug = Some(prepared_prompt_debug.clone());
        set_last_prompt_debug(&state, Some(prepared_prompt_debug.clone())).await;
        if prepared_prompt_debug.summarized_turns > 0 {
            info!(
                trace_id = %trace_id,
                user_id = %user.user_id,
                model = %model_name,
                prompt_tokens = prepared_prompt_debug.prompt_tokens_estimate,
                retained_raw_turns = prepared_prompt_debug.retained_raw_turns,
                summarized_turns = prepared_prompt_debug.summarized_turns,
                context_window_tokens = prepared_prompt_debug.context_length,
                "ai chat compacted conversation history to fit context window"
            );
        }

        let mut sampling = profile.answer_sampling.clone();
        sampling.max_tokens = sampling.max_tokens.min(completion_budget);
        let _ = update_turn_journal(
            &state,
            &journal_handle,
            AiTurnJournalStatus::Generating,
            req.history.len(),
            Some(planned_tool_set.mode.as_str()),
            &planner_debug,
            prompt_debug.as_ref(),
            None,
            None,
            None,
            compact_boundary_count,
            None,
            None,
        )
        .await;
        let raw_stream = engine.chat_stream(messages, sampling);
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
                    if let Some(stats_mut) = stats.as_mut() {
                        stats_mut.completion_budget_tokens = completion_budget;
                        enrich_turn_stats(
                            stats_mut,
                            &planner_debug,
                            prompt_debug.as_ref(),
                            compact_boundary_count,
                            Some(&artifact_verification),
                            None,
                        );
                    }
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
                        if let Some(stats_mut) = stats.as_mut() {
                            stats_mut.completion_budget_tokens = completion_budget;
                            enrich_turn_stats(
                                stats_mut,
                                &planner_debug,
                                prompt_debug.as_ref(),
                                compact_boundary_count,
                                Some(&artifact_verification),
                                None,
                            );
                        }
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

                    let _ = update_turn_journal(
                        &state,
                        &journal_handle,
                        AiTurnJournalStatus::Completed,
                        req.history.len(),
                        Some(planned_tool_set.mode.as_str()),
                        &planner_debug,
                        prompt_debug.as_ref(),
                        stats.as_ref(),
                        None,
                        None,
                        compact_boundary_count,
                        Some(&artifact_verification),
                        Some(now_ts_ms()),
                    )
                    .await;

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
                    set_last_turn_stats(&state, stats.clone()).await;
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
                        let mut error_stats = build_turn_stats(
                            0,
                            0,
                            0,
                            planner_duration_ms,
                            tool_duration_ms,
                            turn_started.elapsed().as_millis() as u64,
                            queue_duration_ms,
                            model_load_duration_ms,
                            0.0,
                        );
                        enrich_turn_stats(
                            &mut error_stats,
                            &planner_debug,
                            prompt_debug.as_ref(),
                            compact_boundary_count,
                            Some(&artifact_verification),
                            None,
                        );
                        stats = Some(error_stats.clone());
                        let _ = update_turn_journal(
                            &state,
                            &journal_handle,
                            AiTurnJournalStatus::Failed,
                            req.history.len(),
                            Some(planned_tool_set.mode.as_str()),
                            &planner_debug,
                            prompt_debug.as_ref(),
                            Some(&error_stats),
                            None,
                            Some(&error_message),
                            compact_boundary_count,
                            Some(&artifact_verification),
                            Some(now_ts_ms()),
                        )
                        .await;
                        persist_chat_audit_event(
                            &state,
                            &user,
                            &req,
                            &trace_id,
                            AiAssistantAuditResponseKind::StreamError,
                            &planned_tools,
                            &grounding_blocks,
                            &grounding_chunks,
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
                    set_last_turn_stats(&state, stats.clone()).await;
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

async fn prepare_generation_messages(
    state: &AppState,
    user: &AuthUser,
    persistence: Option<&ConversationPersistence>,
    engine: &rustfin_ai_agent::LlamaEngine,
    request: &AssistantChatRequest,
    grounding_chunks: &[AssistantGroundingChunk],
) -> Result<
    (
        Vec<rustfin_ai_agent::ChatMessage>,
        ConversationPromptDebug,
        u32,
        AssistantModelProfile,
        u32,
    ),
    String,
> {
    let profile = assistant_model_profile(request.response_mode, engine.params().n_ctx);
    let context_length = profile.turn_budget.context_length_tokens;
    let reserved_completion_tokens = profile.turn_budget.reserved_completion_tokens;
    let prompt_budget = profile.turn_budget.prompt_budget_tokens;
    let system_prompt = build_system_prompt();
    let local_now_text = format!(
        "Current Rustyfin host local date/time for this turn: {}. Use this when interpreting relative dates like today, tomorrow, and next Tuesday.",
        assistant_local_now().format("%Y-%m-%d %H:%M:%S %:z (%A)")
    );

    let mut memory_state = persistence
        .map(|value| value.memory_state.clone())
        .unwrap_or_default();
    let mut memory_turn_index = persistence
        .map(|value| value.memory_turn_index)
        .unwrap_or(-1);
    let mut compact_boundary_count = 0_u32;
    let recovered_from_compact_boundary = persistence
        .map(|value| value.recovered_from_compact_boundary)
        .unwrap_or(false);

    for _ in 0..3 {
        let mut assembly = build_generation_prompt_messages(
            &system_prompt,
            &local_now_text,
            grounding_chunks,
            &request.history,
            &request.message,
            &memory_state,
            memory_turn_index,
            context_length,
            prompt_budget,
            reserved_completion_tokens,
            |messages| engine.count_chat_tokens(messages).unwrap_or(u32::MAX),
        );
        assembly.debug.compact_boundary_count = compact_boundary_count;
        assembly.debug.recovered_from_compact_boundary = recovered_from_compact_boundary;

        if assembly.pending_summary_turns.is_empty() || persistence.is_none() {
            let remaining_tokens = context_length
                .saturating_sub(assembly.debug.prompt_tokens_estimate.saturating_add(8));
            if remaining_tokens == 0 {
                return Err(
                    "AI prompt exhausted the model context window after compaction.".to_string(),
                );
            }
            return Ok((
                assembly.messages,
                assembly.debug,
                remaining_tokens.min(reserved_completion_tokens.max(1)),
                profile,
                compact_boundary_count,
            ));
        }

        let persistence = persistence.expect("checked is_some above");
        let refresh = refresh_conversation_memory(
            state,
            user,
            persistence,
            engine,
            &memory_state,
            &assembly.pending_summary_turns,
            assembly
                .pending_summary_last_turn_index
                .expect("missing summary boundary"),
        )
        .await?;
        memory_state = refresh.memory_state;
        memory_turn_index = assembly
            .pending_summary_last_turn_index
            .unwrap_or(memory_turn_index);
        compact_boundary_count =
            compact_boundary_count.saturating_add(refresh.compact_boundary_count);
    }

    let mut assembly = build_generation_prompt_messages(
        &system_prompt,
        &local_now_text,
        grounding_chunks,
        &request.history,
        &request.message,
        &memory_state,
        memory_turn_index,
        context_length,
        prompt_budget,
        reserved_completion_tokens,
        |messages| engine.count_chat_tokens(messages).unwrap_or(u32::MAX),
    );
    assembly.debug.compact_boundary_count = compact_boundary_count;
    assembly.debug.recovered_from_compact_boundary = recovered_from_compact_boundary;
    let remaining_tokens =
        context_length.saturating_sub(assembly.debug.prompt_tokens_estimate.saturating_add(8));
    if remaining_tokens == 0 {
        return Err("AI prompt exhausted the model context window after compaction.".to_string());
    }

    Ok((
        assembly.messages,
        assembly.debug,
        remaining_tokens.min(reserved_completion_tokens.max(1)),
        profile,
        compact_boundary_count,
    ))
}

struct ConversationMemoryRefreshResult {
    memory_state: ConversationMemoryState,
    compact_boundary_count: u32,
}

async fn refresh_conversation_memory(
    state: &AppState,
    user: &AuthUser,
    persistence: &ConversationPersistence,
    engine: &rustfin_ai_agent::LlamaEngine,
    existing_memory: &ConversationMemoryState,
    turns: &[AssistantHistoryMessage],
    memory_turn_index: i64,
) -> Result<ConversationMemoryRefreshResult, String> {
    if turns.is_empty() {
        return Ok(ConversationMemoryRefreshResult {
            memory_state: existing_memory.clone(),
            compact_boundary_count: 0,
        });
    }

    let updated_memory = generate_memory_state_with_model(engine, existing_memory, turns).await;
    persist_conversation_memory(
        state,
        &user.user_id,
        &persistence.conversation_id,
        &updated_memory,
        memory_turn_index,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;

    let from_turn_index = persistence.memory_turn_index.saturating_add(1).max(0);
    let memory_state_json = crate::ai_assistant::memory::memory_state_json(&updated_memory);
    let _ = rustfin_db::repo::ai_compact_boundaries::create_compact_boundary(
        &state.db,
        rustfin_db::repo::ai_compact_boundaries::CreateAiConversationCompactBoundaryParams {
            conversation_id: &persistence.conversation_id,
            user_id: &user.user_id,
            trace_id: None,
            from_turn_index,
            to_turn_index: memory_turn_index,
            summarized_turn_count: turns.len() as i64,
            memory_state_json: &memory_state_json,
        },
    )
    .await
    .map_err(|error| format!("failed to store conversation compact boundary: {error}"))?;

    Ok(ConversationMemoryRefreshResult {
        memory_state: updated_memory,
        compact_boundary_count: 1,
    })
}

async fn generate_memory_state_with_model(
    engine: &rustfin_ai_agent::LlamaEngine,
    existing_memory: &ConversationMemoryState,
    turns: &[AssistantHistoryMessage],
) -> ConversationMemoryState {
    if turns.is_empty() {
        return existing_memory.clone();
    }

    let messages = build_memory_update_messages(existing_memory, turns);
    let mut sampling =
        assistant_model_profile(AssistantResponseMode::Thinking, engine.params().n_ctx)
            .memory_sampling;
    sampling.max_duration_ms = Some(MEMORY_SUMMARY_MAX_DURATION_MS);
    let stream = engine.chat_stream(messages, sampling);
    futures::pin_mut!(stream);

    let mut content = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(rustfin_ai_agent::ChatChunk::Token(text)) => content.push_str(&text),
            Ok(rustfin_ai_agent::ChatChunk::Stats { .. })
            | Ok(rustfin_ai_agent::ChatChunk::Done) => {}
            Err(_) => return fallback_memory_update(existing_memory, turns),
        }
    }

    parse_memory_update_response(&content)
        .unwrap_or_else(|| fallback_memory_update(existing_memory, turns))
}

async fn set_last_prompt_debug(state: &AppState, debug: Option<ConversationPromptDebug>) {
    let mut guard = state.engine.lock().await;
    guard.last_prompt_debug = debug;
}

async fn set_last_turn_stats(state: &AppState, stats: Option<AssistantTurnStats>) {
    let mut guard = state.engine.lock().await;
    guard.last_turn_stats = stats;
}

#[cfg(test)]
mod tests {
    use super::{
        parse_device_indices_override, parse_gpu_split_mode_override, parse_i32_override,
        resolve_effective_context_window, suggested_context_window_from_host_memory,
    };
    use crate::ai_assistant::profiles::{
        answer_sampling_params, prompt_budget_tokens, response_mode_completion_reserve_tokens,
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
        let instant = answer_sampling_params(AssistantResponseMode::Instant);
        let thinking = answer_sampling_params(AssistantResponseMode::Thinking);
        let extended = answer_sampling_params(AssistantResponseMode::Extended);
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
        ..AssistantTurnStats::default()
    }
}

fn enrich_turn_stats(
    stats: &mut AssistantTurnStats,
    planner_debug: &AssistantPlannerDebug,
    prompt_debug: Option<&ConversationPromptDebug>,
    compact_boundary_count: u32,
    artifact_verification: Option<&AssistantArtifactVerificationDebug>,
    overload_reason: Option<&str>,
) {
    stats.planner_validation_error_count = planner_debug.validation_errors.len() as u32;
    stats.planner_repair_count = planner_debug.repair_attempt_count;
    stats.compact_boundary_count = compact_boundary_count;
    stats.journal_persisted = true;

    if let Some(prompt_debug) = prompt_debug {
        stats.context_length_tokens = prompt_debug.context_length;
        stats.prompt_budget_tokens = prompt_debug.prompt_budget_tokens;
        stats.reserved_completion_tokens = prompt_debug.reserved_completion_tokens;
        stats.loaded_history_turns = prompt_debug.loaded_history_turns;
        stats.retained_raw_turns = prompt_debug.retained_raw_turns;
        stats.summarized_turns = prompt_debug.summarized_turns;
        stats.recent_grounded_context_count = prompt_debug.recent_grounded_context_count;
        stats.memory_turn_index = prompt_debug.memory_turn_index;
        stats.compact_boundary_count = stats
            .compact_boundary_count
            .max(prompt_debug.compact_boundary_count);
    }

    if let Some(artifact_verification) = artifact_verification {
        stats.artifact_verification_attempts = artifact_verification.attempts;
        stats.artifact_revision_count = artifact_verification.revision_count;
    }

    if let Some(overload_reason) = overload_reason {
        stats.overload = true;
        stats.overload_reason = Some(overload_reason.to_string());
    }
}

fn deterministic_grounded_reply(
    message: &str,
    history: &[AssistantHistoryMessage],
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    deterministic_current_datetime_reply(message, history, grounding_blocks)
        .or_else(|| deterministic_calendar_reply(message, grounding_blocks))
        .or_else(|| deterministic_network_reply(message, grounding_blocks))
        .or_else(|| deterministic_runtime_reply(message, grounding_blocks))
        .or_else(|| deterministic_profile_reply(message, grounding_blocks))
        .or_else(|| deterministic_service_reply(message, grounding_blocks))
        .or_else(|| deterministic_downloads_reply(message, grounding_blocks))
        .or_else(|| deterministic_rooms_reply(message, grounding_blocks))
        .or_else(|| deterministic_weather_reply(message, grounding_blocks))
}

fn artifact_verification_from_block(
    block: &AssistantToolContextBlock,
) -> Option<AssistantArtifactVerificationDebug> {
    if block.tool != "document_create_download" || block.status != "ok" {
        return None;
    }

    block
        .data
        .get("verification")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
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
