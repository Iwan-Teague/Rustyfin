use std::convert::Infallible;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use crate::ai_audit::{AiAssistantAuditEventResponse, parse_audit_event_row};
use crate::ai_storage::{
    AI_MODEL_DIR_SETTING_KEY, AiModelDirectoryState, ModelPullChunk, current_model_dir,
    delete_model_file, download_model_from_url, list_models_with_storage_status, resolve_model_dir,
    resolve_runtime_model_dir, set_model_dir, validate_model_dir,
};
use crate::auth::AdminUser;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
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
    pub planner_debug: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_debug: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overload_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub compact_boundary_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_verification: Option<serde_json::Value>,
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

fn parse_turn_journal_row(
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
        planner_debug: serde_json::from_str(&row.planner_debug_json).unwrap_or_else(|_| json!({})),
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

fn parse_compact_boundary_row(
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

#[derive(Deserialize)]
pub struct UpdateAiAdminConfigRequest {
    pub model_dir: String,
}

#[derive(Deserialize)]
pub struct PullAiModelRequest {
    pub url: String,
}

#[derive(Deserialize)]
pub struct ListAiAuditQuery {
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ListAiTurnJournalQuery {
    pub limit: Option<i64>,
}

pub async fn get_ai_admin_state(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    Ok(Json(build_ai_admin_state(&state).await?))
}

pub async fn update_ai_admin_config(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateAiAdminConfigRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    let trimmed = body.model_dir.trim();
    if trimmed.is_empty() {
        let _ = rustfin_db::repo::settings::delete(&state.db, AI_MODEL_DIR_SETTING_KEY)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let (resolved, _, _) = resolve_runtime_model_dir(&state.db).await?;
        set_model_dir(&state, resolved).await;
    } else {
        let validated = validate_model_dir(&PathBuf::from(trimmed))?;
        rustfin_db::repo::settings::set(
            &state.db,
            AI_MODEL_DIR_SETTING_KEY,
            validated.to_string_lossy().as_ref(),
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        set_model_dir(&state, validated).await;
    }

    crate::ai::clear_loaded_model_state(&state).await;
    Ok(Json(build_ai_admin_state(&state).await?))
}

pub async fn pull_ai_model(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<PullAiModelRequest>,
) -> Response {
    let model_dir = current_model_dir(&state).await;
    let raw = download_model_from_url(body.url, model_dir, state.http.clone());
    let sse = raw.map(|chunk| {
        let event = match chunk {
            ModelPullChunk::Progress {
                status,
                bytes_done,
                bytes_total,
                percent,
            } => Event::default().event("progress").data(
                json!({
                    "status": status,
                    "bytes_done": bytes_done,
                    "bytes_total": bytes_total,
                    "percent": percent,
                })
                .to_string(),
            ),
            ModelPullChunk::Done => Event::default().event("done").data("{}"),
            ModelPullChunk::Error(message) => Event::default()
                .event("error")
                .data(json!({ "message": message }).to_string()),
        };
        Ok::<Event, Infallible>(event)
    });

    Sse::new(Box::pin(sse))
        .keep_alive(KeepAlive::default())
        .into_response()
}

pub async fn delete_ai_model(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = delete_model_file(&state, &name).await?;
    if deleted {
        crate::ai::clear_loaded_model_if_matching(&state, &name).await;
        return Ok(StatusCode::NO_CONTENT);
    }

    warn!(model = %name, "admin requested delete for missing AI model");
    Ok(StatusCode::NOT_FOUND)
}

pub async fn list_ai_audit_events(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiAuditQuery>,
) -> Result<Json<Vec<AiAssistantAuditEventResponse>>, AppError> {
    let rows = rustfin_db::repo::ai_assistant_audit::list_audit_events(
        &state.db,
        query.limit.unwrap_or(40),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_audit_event_row)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_ai_turn_journals(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiTurnJournalQuery>,
) -> Result<Json<Vec<AiTurnJournalSummary>>, AppError> {
    let rows = rustfin_db::repo::ai_assistant_turn_journals::list_recent_journals(
        &state.db,
        query.limit.unwrap_or(30),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_turn_journal_row)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_ai_compact_boundaries(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiTurnJournalQuery>,
) -> Result<Json<Vec<AiCompactBoundarySummary>>, AppError> {
    let rows = rustfin_db::repo::ai_compact_boundaries::list_recent_compact_boundaries(
        &state.db,
        query.limit.unwrap_or(20),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_compact_boundary_row)
            .collect::<Vec<_>>(),
    ))
}

async fn build_ai_admin_state(state: &AppState) -> Result<AiModelDirectoryState, AppError> {
    let (configured_model_dir, configured_source) = resolve_model_dir(&state.db).await?;
    let model_dir = current_model_dir(state).await;
    let source = if configured_source == "default" && model_dir != configured_model_dir {
        "default_fallback".to_string()
    } else {
        configured_source
    };
    let (models, model_storage_available, model_storage_error) =
        list_models_with_storage_status(state).await;

    Ok(AiModelDirectoryState {
        available: crate::ai::inference_available(),
        model_dir: model_dir.to_string_lossy().to_string(),
        default_model_dir: crate::ai_storage::DEFAULT_AI_MODEL_DIR.to_string(),
        model_dir_source: source,
        model_storage_available,
        model_storage_error,
        audit_retention_days: crate::ai_audit::audit_retention_days(),
        audit_prune_interval_seconds: crate::ai_audit::AI_AUDIT_PRUNE_INTERVAL_SECS,
        models,
    })
}
