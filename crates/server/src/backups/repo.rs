use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use rustfin_core::error::ApiError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupPolicy {
    pub id: String,
    pub name: String,
    pub schedule_cron: Option<String>,
    pub retention_count: i32,
    pub target_type: String,
    pub target_path: Option<String>,
    pub include_database: bool,
    pub include_server_config: bool,
    pub include_server_worlds: bool,
    pub enabled: bool,
    pub last_run_ts: Option<i64>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type PolicyTuple = (
    String, String, Option<String>, i32, String, Option<String>,
    bool, bool, bool, bool, Option<i64>, i64, i64,
);

fn map_policy(row: PolicyTuple) -> BackupPolicy {
    BackupPolicy {
        id: row.0,
        name: row.1,
        schedule_cron: row.2,
        retention_count: row.3,
        target_type: row.4,
        target_path: row.5,
        include_database: row.6,
        include_server_config: row.7,
        include_server_worlds: row.8,
        enabled: row.9,
        last_run_ts: row.10,
        created_ts: row.11,
        updated_ts: row.12,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupJob {
    pub id: String,
    pub policy_id: Option<String>,
    pub status: String,
    pub trigger_type: String,
    pub start_ts: i64,
    pub end_ts: Option<i64>,
    pub log_text: Option<String>,
    pub error_message: Option<String>,
    pub total_size_bytes: Option<i64>,
}

type JobTuple = (
    String, Option<String>, String, String, i64, Option<i64>,
    Option<String>, Option<String>, Option<i64>,
);

fn map_job(row: JobTuple) -> BackupJob {
    BackupJob {
        id: row.0,
        policy_id: row.1,
        status: row.2,
        trigger_type: row.3,
        start_ts: row.4,
        end_ts: row.5,
        log_text: row.6,
        error_message: row.7,
        total_size_bytes: row.8,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupArtifact {
    pub id: String,
    pub job_id: String,
    pub artifact_type: String,
    pub filename: String,
    pub file_path: String,
    pub file_size_bytes: i64,
    pub checksum_sha256: Option<String>,
    pub created_ts: i64,
}

pub async fn list_policies(pool: &PgPool) -> Result<Vec<BackupPolicy>, ApiError> {
    let rows: Vec<PolicyTuple> = sqlx::query_as(
        "SELECT id, name, schedule_cron, retention_count, target_type, target_path, include_database, include_server_config, include_server_worlds, enabled, last_run_ts, created_ts, updated_ts FROM backup_policy ORDER BY name"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(rows.into_iter().map(map_policy).collect())
}

pub async fn get_policy(pool: &PgPool, id: &str) -> Result<Option<BackupPolicy>, ApiError> {
    let row: Option<PolicyTuple> = sqlx::query_as(
        "SELECT id, name, schedule_cron, retention_count, target_type, target_path, include_database, include_server_config, include_server_worlds, enabled, last_run_ts, created_ts, updated_ts FROM backup_policy WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(row.map(map_policy))
}

pub async fn create_policy(pool: &PgPool, policy: &BackupPolicy) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO backup_policy (id, name, schedule_cron, retention_count, target_type, target_path, include_database, include_server_config, include_server_worlds, enabled, created_ts, updated_ts) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
    )
    .bind(&policy.id)
    .bind(&policy.name)
    .bind(&policy.schedule_cron)
    .bind(policy.retention_count)
    .bind(&policy.target_type)
    .bind(&policy.target_path)
    .bind(policy.include_database)
    .bind(policy.include_server_config)
    .bind(policy.include_server_worlds)
    .bind(policy.enabled)
    .bind(policy.created_ts)
    .bind(policy.updated_ts)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(())
}

pub async fn list_jobs(pool: &PgPool) -> Result<Vec<BackupJob>, ApiError> {
    let rows: Vec<JobTuple> = sqlx::query_as(
        "SELECT id, policy_id, status, trigger_type, start_ts, end_ts, log_text, error_message, total_size_bytes FROM backup_job ORDER BY start_ts DESC LIMIT 50"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(rows.into_iter().map(map_job).collect())
}

pub async fn create_job(pool: &PgPool, job: &BackupJob) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO backup_job (id, policy_id, status, trigger_type, start_ts, end_ts, log_text, error_message, total_size_bytes) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
    )
    .bind(&job.id)
    .bind(&job.policy_id)
    .bind(&job.status)
    .bind(&job.trigger_type)
    .bind(job.start_ts)
    .bind(job.end_ts)
    .bind(&job.log_text)
    .bind(&job.error_message)
    .bind(job.total_size_bytes)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(())
}

pub async fn get_job(pool: &PgPool, id: &str) -> Result<Option<BackupJob>, ApiError> {
    let row: Option<JobTuple> = sqlx::query_as(
        "SELECT id, policy_id, status, trigger_type, start_ts, end_ts, log_text, error_message, total_size_bytes FROM backup_job WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(row.map(map_job))
}

pub async fn update_job_status(pool: &PgPool, id: &str, status: &str, end_ts: Option<i64>, error: Option<&str>, size: Option<i64>) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE backup_job SET status = $1, end_ts = $2, error_message = $3, total_size_bytes = $4 WHERE id = $5"
    )
    .bind(status)
    .bind(end_ts)
    .bind(error)
    .bind(size)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(())
}

pub async fn update_policy_last_run(pool: &PgPool, id: &str, last_run: i64) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE backup_policy SET last_run_ts = $1 WHERE id = $2"
    )
    .bind(last_run)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {}", e)))?;
    Ok(())
}
