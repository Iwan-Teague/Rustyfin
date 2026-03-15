use std::convert::Infallible;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use rustfin_core::error::ApiError;
use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::ai_storage::{
    AI_MODEL_DIR_SETTING_KEY, AiModelDirectoryState, ModelPullChunk, current_model_dir,
    delete_model_file, download_model_from_url, list_models_from_state, resolve_model_dir,
    set_model_dir, validate_model_dir,
};
use crate::auth::AdminUser;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UpdateAiAdminConfigRequest {
    pub model_dir: String,
}

#[derive(Deserialize)]
pub struct PullAiModelRequest {
    pub url: String,
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
        let (resolved, _) = resolve_model_dir(&state.db).await?;
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

async fn build_ai_admin_state(state: &AppState) -> Result<AiModelDirectoryState, AppError> {
    let (_, source) = resolve_model_dir(&state.db).await?;
    let model_dir = current_model_dir(state).await;
    let models = list_models_from_state(state).await?;

    Ok(AiModelDirectoryState {
        available: crate::ai::inference_available(),
        model_dir: model_dir.to_string_lossy().to_string(),
        default_model_dir: crate::ai_storage::DEFAULT_AI_MODEL_DIR.to_string(),
        model_dir_source: source,
        models,
    })
}
