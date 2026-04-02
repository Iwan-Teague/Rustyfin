use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use rustfin_core::error::ApiError;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub fn artifact_download_path(artifact_id: &str) -> String {
    format!("/api/v1/ai/artifacts/{artifact_id}/download")
}

pub async fn download_generated_artifact(
    user: AuthUser,
    State(state): State<AppState>,
    Path(artifact_id): Path<String>,
) -> Result<Response, AppError> {
    let artifact = rustfin_db::repo::ai_generated_artifacts::get_artifact_for_user(
        &state.db,
        &artifact_id,
        &user.user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let Some(artifact) = artifact else {
        return Err(ApiError::NotFound("generated AI download not found".into()).into());
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CACHE_CONTROL, "no-store")
        .header(
            header::CONTENT_TYPE,
            sanitize_media_type(&artifact.media_type),
        )
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{}\"",
                artifact.file_name.replace('"', "")
            ),
        )
        .body(Body::from(artifact.content_text.into_bytes()))
        .map_err(|error| {
            ApiError::Internal(format!(
                "failed to build generated AI artifact download response: {error}"
            ))
            .into()
        })
}

fn sanitize_media_type(media_type: &str) -> &'static str {
    match media_type {
        "text/markdown; charset=utf-8" => "text/markdown; charset=utf-8",
        "text/plain; charset=utf-8" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
