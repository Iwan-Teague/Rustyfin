use super::repo;
use super::service;
use crate::auth::AdminUser;
use crate::error::AppError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
};
use rustfin_core::error::ApiError;

pub async fn list_policies(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<repo::BackupPolicy>>, AppError> {
    let policies = repo::list_policies(&state.db).await?;
    Ok(Json(policies))
}

pub async fn create_policy(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(policy): Json<repo::BackupPolicy>,
) -> Result<Json<repo::BackupPolicy>, AppError> {
    // Basic validation could go here
    repo::create_policy(&state.db, &policy).await?;
    Ok(Json(policy))
}

pub async fn list_jobs(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<repo::BackupJob>>, AppError> {
    let jobs = repo::list_jobs(&state.db).await?;
    Ok(Json(jobs))
}

pub async fn create_backup_job(
    _admin: AdminUser,
    State(state): State<AppState>,
    // Optional policy ID in body? or just trigger manual
) -> Result<Json<String>, AppError> {
    let job_id = service::trigger_backup(&state.db, None)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(job_id))
}

pub async fn restore_backup(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<(), AppError> {
    service::restore_backup(&state.db, &job_id)
        .await
        .map_err(ApiError::Internal)?;
    Ok(())
}
