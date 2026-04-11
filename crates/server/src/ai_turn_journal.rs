use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::types::{
    AssistantArtifactVerificationDebug, AssistantPlannerDebug, AssistantTurnStats,
    ConversationPromptDebug,
};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Copy)]
pub enum AiTurnJournalStatus {
    Accepted,
    Grounded,
    Generating,
    Completed,
    Failed,
    Overloaded,
}

impl AiTurnJournalStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Grounded => "grounded",
            Self::Generating => "generating",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Overloaded => "overloaded",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnJournalHandle {
    pub id: String,
    pub user_id: String,
    pub conversation_id: Option<String>,
    pub request_turn_id: Option<String>,
    pub request_turn_index: Option<i64>,
    pub trace_id: String,
    pub request_message: String,
    pub model_name: String,
    pub response_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTurnJournalSummary {
    pub id: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_turn_index: Option<i64>,
    pub trace_id: String,
    pub request_message: String,
    pub model_name: String,
    pub response_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_mode: Option<String>,
    pub status: String,
    pub current_phase: String,
    pub history_len: i64,
    pub planner_debug: AssistantPlannerDebug,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_debug: Option<ConversationPromptDebug>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<AssistantTurnStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overload_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub compact_boundary_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_verification: Option<AssistantArtifactVerificationDebug>,
    pub created_ts: i64,
    pub updated_ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiCompactBoundarySummary {
    pub id: String,
    pub conversation_id: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub from_turn_index: i64,
    pub to_turn_index: i64,
    pub summarized_turn_count: i64,
    pub memory_state_json: String,
    pub created_ts: i64,
}

pub async fn create_turn_journal(
    state: &AppState,
    handle: &TurnJournalHandle,
    history_len: usize,
) -> Result<(), AppError> {
    rustfin_db::repo::ai_assistant_turn_journals::create_journal(
        &state.db,
        rustfin_db::repo::ai_assistant_turn_journals::CreateAiAssistantTurnJournalParams {
            id: &handle.id,
            user_id: &handle.user_id,
            conversation_id: handle.conversation_id.as_deref(),
            request_turn_id: handle.request_turn_id.as_deref(),
            request_turn_index: handle.request_turn_index,
            trace_id: &handle.trace_id,
            request_message: &handle.request_message,
            model_name: &handle.model_name,
            response_mode: &handle.response_mode,
            planner_mode: None,
            status: AiTurnJournalStatus::Accepted.as_str(),
            current_phase: "planning",
            history_len: history_len as i64,
            planner_debug_json: "{}",
            prompt_debug_json: None,
            metrics_json: None,
            overload_reason: None,
            error_message: None,
            compact_boundary_count: 0,
            artifact_verification_json: None,
            finished_ts: None,
        },
    )
    .await
    .map(|_| ())
    .map_err(|e| ApiError::Internal(format!("db error: {e}")).into())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_turn_journal(
    state: &AppState,
    handle: &TurnJournalHandle,
    status: AiTurnJournalStatus,
    history_len: usize,
    planner_mode: Option<&str>,
    planner_debug: &AssistantPlannerDebug,
    prompt_debug: Option<&ConversationPromptDebug>,
    stats: Option<&AssistantTurnStats>,
    overload_reason: Option<&str>,
    error_message: Option<&str>,
    compact_boundary_count: u32,
    artifact_verification: Option<&AssistantArtifactVerificationDebug>,
    finished_ts: Option<i64>,
) -> Result<(), AppError> {
    let planner_debug_json = serde_json::to_string(planner_debug)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let prompt_debug_json = prompt_debug
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let metrics_json = stats
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let artifact_verification_json = artifact_verification
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;

    rustfin_db::repo::ai_assistant_turn_journals::update_journal(
        &state.db,
        &handle.id,
        rustfin_db::repo::ai_assistant_turn_journals::UpdateAiAssistantTurnJournalParams {
            id: &handle.id,
            user_id: &handle.user_id,
            conversation_id: handle.conversation_id.as_deref(),
            request_turn_id: handle.request_turn_id.as_deref(),
            request_turn_index: handle.request_turn_index,
            trace_id: &handle.trace_id,
            request_message: &handle.request_message,
            model_name: &handle.model_name,
            response_mode: &handle.response_mode,
            planner_mode,
            status: status.as_str(),
            current_phase: match status {
                AiTurnJournalStatus::Accepted => "planning",
                AiTurnJournalStatus::Grounded => "grounding",
                AiTurnJournalStatus::Generating => "generating",
                AiTurnJournalStatus::Completed => "completed",
                AiTurnJournalStatus::Failed => "failed",
                AiTurnJournalStatus::Overloaded => "overloaded",
            },
            history_len: history_len as i64,
            planner_debug_json: &planner_debug_json,
            prompt_debug_json: prompt_debug_json.as_deref(),
            metrics_json: metrics_json.as_deref(),
            overload_reason,
            error_message,
            compact_boundary_count: i64::from(compact_boundary_count),
            artifact_verification_json: artifact_verification_json.as_deref(),
            finished_ts,
        },
    )
    .await
    .map(|_| ())
    .map_err(|e| ApiError::Internal(format!("db error: {e}")).into())
}

pub fn parse_turn_journal_row(
    row: rustfin_db::repo::ai_assistant_turn_journals::AiAssistantTurnJournalRow,
) -> AiTurnJournalSummary {
    AiTurnJournalSummary {
        id: row.id,
        user_id: row.user_id,
        conversation_id: row.conversation_id,
        request_turn_id: row.request_turn_id,
        request_turn_index: row.request_turn_index,
        trace_id: row.trace_id,
        request_message: row.request_message,
        model_name: row.model_name,
        response_mode: row.response_mode,
        planner_mode: row.planner_mode,
        status: row.status,
        current_phase: row.current_phase,
        history_len: row.history_len,
        planner_debug: serde_json::from_str(&row.planner_debug_json).unwrap_or_default(),
        prompt_debug: row
            .prompt_debug_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        stats: row
            .metrics_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        overload_reason: row.overload_reason,
        error_message: row.error_message,
        compact_boundary_count: row.compact_boundary_count,
        artifact_verification: row
            .artifact_verification_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
        finished_ts: row.finished_ts,
    }
}

pub fn parse_compact_boundary_row(
    row: rustfin_db::repo::ai_compact_boundaries::AiConversationCompactBoundaryRow,
) -> AiCompactBoundarySummary {
    AiCompactBoundarySummary {
        id: row.id,
        conversation_id: row.conversation_id,
        user_id: row.user_id,
        trace_id: row.trace_id,
        from_turn_index: row.from_turn_index,
        to_turn_index: row.to_turn_index,
        summarized_turn_count: row.summarized_turn_count,
        memory_state_json: row.memory_state_json,
        created_ts: row.created_ts,
    }
}
