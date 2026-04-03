use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;

use super::types::{
    AiTaskArtifactRecord, AiTaskCheckpointRecord, AiTaskEventRecord, AiTaskPhase, AiTaskRecord,
    AiTaskStatus, AiTaskType, CreateAiTaskRequest, TaskUserContext, valid_status_transition,
};

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait AiTaskStore: Clone + Send + Sync + 'static {
    async fn create_task(
        &self,
        owner: &TaskUserContext,
        request: &CreateAiTaskRequest,
    ) -> Result<AiTaskRecord, String>;
    async fn list_tasks_for_user(&self, user_id: &str) -> Result<Vec<AiTaskRecord>, String>;
    async fn get_task_for_user(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String>;
    async fn get_task(&self, task_id: &str) -> Result<Option<AiTaskRecord>, String>;
    async fn get_task_user_context(&self, task_id: &str)
    -> Result<Option<TaskUserContext>, String>;
    async fn list_recoverable_tasks(&self) -> Result<Vec<AiTaskRecord>, String>;
    async fn append_event(
        &self,
        task_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<AiTaskEventRecord, String>;
    async fn list_events_for_user(
        &self,
        task_id: &str,
        user_id: &str,
        after_id: Option<i64>,
    ) -> Result<Vec<AiTaskEventRecord>, String>;
    async fn write_checkpoint(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        payload: Value,
    ) -> Result<AiTaskCheckpointRecord, String>;
    async fn list_checkpoints(&self, task_id: &str) -> Result<Vec<AiTaskCheckpointRecord>, String>;
    async fn update_progress(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        progress_pct: f64,
        effective_answer_model: Option<&str>,
        effective_planner_model: Option<&str>,
    ) -> Result<Option<AiTaskRecord>, String>;
    async fn transition_status(
        &self,
        task_id: &str,
        expected: &[AiTaskStatus],
        next: AiTaskStatus,
        phase: AiTaskPhase,
        result_json: Option<Value>,
        error_json: Option<Value>,
        clear_cancel_requested: bool,
    ) -> Result<Option<AiTaskRecord>, String>;
    async fn request_cancel(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String>;
    async fn resume_task(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String>;
    async fn create_artifact(
        &self,
        task_id: &str,
        user_id: &str,
        kind: &str,
        file_name: &str,
        media_type: &str,
        storage_path: &str,
        size_bytes: i64,
    ) -> Result<AiTaskArtifactRecord, String>;
    async fn get_artifact_for_user(
        &self,
        task_id: &str,
        artifact_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskArtifactRecord>, String>;
}

#[derive(Clone)]
pub struct DbAiTaskStore {
    pool: rustfin_db::DbPool,
}

impl DbAiTaskStore {
    pub fn new(pool: rustfin_db::DbPool) -> Self {
        Self { pool }
    }
}

fn parse_json_or_null(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

fn parse_optional_json(raw: Option<String>) -> Option<Value> {
    raw.map(|value| parse_json_or_null(&value))
}

fn dollar_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|idx| format!("${idx}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn task_from_row(row: &sqlx::postgres::PgRow) -> Result<AiTaskRecord, String> {
    let task_type = row
        .try_get::<String, _>("task_type")
        .map_err(|e| e.to_string())?;
    let status = row
        .try_get::<String, _>("status")
        .map_err(|e| e.to_string())?;
    let phase = row
        .try_get::<String, _>("phase")
        .map_err(|e| e.to_string())?;
    let input_json = row
        .try_get::<String, _>("input_json")
        .map_err(|e| e.to_string())?;

    Ok(AiTaskRecord {
        id: row
            .try_get::<uuid::Uuid, _>("id")
            .map_err(|e| e.to_string())?
            .to_string(),
        owner_user_id: row
            .try_get::<String, _>("owner_user_id")
            .map_err(|e| e.to_string())?,
        created_ts: row
            .try_get::<i64, _>("created_ts")
            .map_err(|e| e.to_string())?,
        updated_ts: row
            .try_get::<i64, _>("updated_ts")
            .map_err(|e| e.to_string())?,
        task_type: AiTaskType::from_str(&task_type)
            .ok_or_else(|| format!("unknown ai_task.task_type {task_type}"))?,
        status: AiTaskStatus::from_str(&status)
            .ok_or_else(|| format!("unknown ai_task.status {status}"))?,
        requested_model: row
            .try_get::<Option<String>, _>("requested_model")
            .map_err(|e| e.to_string())?,
        effective_answer_model: row
            .try_get::<Option<String>, _>("effective_answer_model")
            .map_err(|e| e.to_string())?,
        effective_planner_model: row
            .try_get::<Option<String>, _>("effective_planner_model")
            .map_err(|e| e.to_string())?,
        input_json: parse_json_or_null(&input_json),
        result_json: parse_optional_json(
            row.try_get::<Option<String>, _>("result_json")
                .map_err(|e| e.to_string())?,
        ),
        error_json: parse_optional_json(
            row.try_get::<Option<String>, _>("error_json")
                .map_err(|e| e.to_string())?,
        ),
        progress_pct: row
            .try_get::<f64, _>("progress_pct")
            .map_err(|e| e.to_string())?,
        phase: AiTaskPhase::from_str(&phase)
            .ok_or_else(|| format!("unknown ai_task.phase {phase}"))?,
        cancel_requested: row
            .try_get::<bool, _>("cancel_requested")
            .map_err(|e| e.to_string())?,
        checkpoint_version: row
            .try_get::<i32, _>("checkpoint_version")
            .map_err(|e| e.to_string())?,
        last_checkpoint_json: parse_optional_json(
            row.try_get::<Option<String>, _>("last_checkpoint_json")
                .map_err(|e| e.to_string())?,
        ),
        artifacts: Vec::new(),
    })
}

fn artifact_from_row(row: &sqlx::postgres::PgRow) -> Result<AiTaskArtifactRecord, String> {
    Ok(AiTaskArtifactRecord {
        id: row
            .try_get::<uuid::Uuid, _>("id")
            .map_err(|e| e.to_string())?
            .to_string(),
        task_id: row
            .try_get::<uuid::Uuid, _>("task_id")
            .map_err(|e| e.to_string())?
            .to_string(),
        kind: row
            .try_get::<String, _>("kind")
            .map_err(|e| e.to_string())?,
        file_name: row
            .try_get::<String, _>("file_name")
            .map_err(|e| e.to_string())?,
        media_type: row
            .try_get::<String, _>("media_type")
            .map_err(|e| e.to_string())?,
        storage_path: row
            .try_get::<String, _>("storage_path")
            .map_err(|e| e.to_string())?,
        size_bytes: row
            .try_get::<i64, _>("size_bytes")
            .map_err(|e| e.to_string())?,
        created_ts: row
            .try_get::<i64, _>("created_ts")
            .map_err(|e| e.to_string())?,
    })
}

fn event_from_row(row: &sqlx::postgres::PgRow) -> Result<AiTaskEventRecord, String> {
    Ok(AiTaskEventRecord {
        id: row.try_get::<i64, _>("id").map_err(|e| e.to_string())?,
        task_id: row
            .try_get::<uuid::Uuid, _>("task_id")
            .map_err(|e| e.to_string())?
            .to_string(),
        created_ts: row
            .try_get::<i64, _>("created_ts")
            .map_err(|e| e.to_string())?,
        event_type: row
            .try_get::<String, _>("event_type")
            .map_err(|e| e.to_string())?,
        payload: parse_json_or_null(
            &row.try_get::<String, _>("payload_json")
                .map_err(|e| e.to_string())?,
        ),
    })
}

fn checkpoint_from_row(row: &sqlx::postgres::PgRow) -> Result<AiTaskCheckpointRecord, String> {
    let phase = row
        .try_get::<String, _>("phase")
        .map_err(|e| e.to_string())?;
    Ok(AiTaskCheckpointRecord {
        id: row.try_get::<i64, _>("id").map_err(|e| e.to_string())?,
        task_id: row
            .try_get::<uuid::Uuid, _>("task_id")
            .map_err(|e| e.to_string())?
            .to_string(),
        version: row
            .try_get::<i32, _>("version")
            .map_err(|e| e.to_string())?,
        phase: AiTaskPhase::from_str(&phase)
            .ok_or_else(|| format!("unknown ai_task_checkpoint.phase {phase}"))?,
        payload: parse_json_or_null(
            &row.try_get::<String, _>("payload_json")
                .map_err(|e| e.to_string())?,
        ),
        created_ts: row
            .try_get::<i64, _>("created_ts")
            .map_err(|e| e.to_string())?,
    })
}

async fn load_artifacts(
    pool: &rustfin_db::DbPool,
    task_id: &str,
) -> Result<Vec<AiTaskArtifactRecord>, String> {
    let rows = sqlx::query(
        "SELECT id, task_id, kind, file_name, media_type, storage_path, size_bytes,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_ts
         FROM ai_task_artifact
         WHERE task_id = $1::uuid
         ORDER BY created_at ASC",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("failed to load ai task artifacts: {e}"))?;

    rows.into_iter()
        .map(|row| artifact_from_row(&row))
        .collect()
}

async fn load_task_row(
    pool: &rustfin_db::DbPool,
    task_id: &str,
    owner_user_id: Option<&str>,
) -> Result<Option<AiTaskRecord>, String> {
    let row = if let Some(owner_user_id) = owner_user_id {
        sqlx::query(
            "SELECT id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                    requested_model, effective_answer_model, effective_planner_model,
                    input_json::text AS input_json, result_json::text AS result_json,
                    error_json::text AS error_json, progress_pct, phase, cancel_requested,
                    checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json
             FROM ai_task
             WHERE id = $1::uuid AND owner_user_id = $2",
        )
        .bind(task_id)
        .bind(owner_user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("failed to load ai task: {e}"))?
    } else {
        sqlx::query(
            "SELECT id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                    requested_model, effective_answer_model, effective_planner_model,
                    input_json::text AS input_json, result_json::text AS result_json,
                    error_json::text AS error_json, progress_pct, phase, cancel_requested,
                    checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json
             FROM ai_task
             WHERE id = $1::uuid",
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("failed to load ai task: {e}"))?
    };

    let Some(row) = row else {
        return Ok(None);
    };
    let mut task = task_from_row(&row)?;
    task.artifacts = load_artifacts(pool, &task.id).await?;
    Ok(Some(task))
}

fn update_transition_query(status_count: usize) -> String {
    let placeholders = dollar_placeholders(2, status_count);
    format!(
        "UPDATE ai_task
         SET status = ${status_param},
             phase = ${phase_param},
             result_json = ${result_param}::jsonb,
             error_json = ${error_param}::jsonb,
             cancel_requested = CASE WHEN ${clear_cancel_param} THEN FALSE ELSE cancel_requested END,
             updated_at = NOW()
         WHERE id = $1::uuid
           AND status IN ({placeholders})
         RETURNING id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                   EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                   requested_model, effective_answer_model, effective_planner_model,
                   input_json::text AS input_json, result_json::text AS result_json,
                   error_json::text AS error_json, progress_pct, phase, cancel_requested,
                   checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json",
        status_param = status_count + 2,
        phase_param = status_count + 3,
        result_param = status_count + 4,
        error_param = status_count + 5,
        clear_cancel_param = status_count + 6
    )
}

#[async_trait]
impl AiTaskStore for DbAiTaskStore {
    async fn create_task(
        &self,
        owner: &TaskUserContext,
        request: &CreateAiTaskRequest,
    ) -> Result<AiTaskRecord, String> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let input_json = serde_json::to_string(&request.input)
            .map_err(|e| format!("failed to serialize ai task input: {e}"))?;

        sqlx::query(
            "INSERT INTO ai_task (
                id, owner_user_id, task_type, status, requested_model, input_json, progress_pct, phase
            ) VALUES ($1::uuid, $2, $3, $4, $5, $6::jsonb, $7, $8)",
        )
        .bind(&task_id)
        .bind(&owner.user_id)
        .bind(request.input.task_type().as_str())
        .bind(AiTaskStatus::Queued.as_str())
        .bind(request.requested_model.as_deref())
        .bind(&input_json)
        .bind(0.0f64)
        .bind(AiTaskPhase::Queued.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("failed to create ai task: {e}"))?;

        load_task_row(&self.pool, &task_id, Some(&owner.user_id))
            .await?
            .ok_or_else(|| "created ai task is missing".to_string())
    }

    async fn list_tasks_for_user(&self, user_id: &str) -> Result<Vec<AiTaskRecord>, String> {
        let rows = sqlx::query(
            "SELECT id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                    requested_model, effective_answer_model, effective_planner_model,
                    input_json::text AS input_json, result_json::text AS result_json,
                    error_json::text AS error_json, progress_pct, phase, cancel_requested,
                    checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json
             FROM ai_task
             WHERE owner_user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("failed to list ai tasks: {e}"))?;

        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            let mut task = task_from_row(&row)?;
            task.artifacts = load_artifacts(&self.pool, &task.id).await?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    async fn get_task_for_user(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        load_task_row(&self.pool, task_id, Some(user_id)).await
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<AiTaskRecord>, String> {
        load_task_row(&self.pool, task_id, None).await
    }

    async fn get_task_user_context(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskUserContext>, String> {
        let row = sqlx::query(
            "SELECT u.id, u.username, u.role
             FROM ai_task t
             INNER JOIN \"user\" u ON u.id = t.owner_user_id
             WHERE t.id = $1::uuid",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("failed to load ai task user context: {e}"))?;

        row.map(|row| {
            Ok(TaskUserContext {
                user_id: row.try_get::<String, _>("id").map_err(|e| e.to_string())?,
                username: row
                    .try_get::<String, _>("username")
                    .map_err(|e| e.to_string())?,
                role: row
                    .try_get::<String, _>("role")
                    .map_err(|e| e.to_string())?,
                is_admin: row
                    .try_get::<String, _>("role")
                    .map_err(|e| e.to_string())?
                    == "admin",
            })
        })
        .transpose()
    }

    async fn list_recoverable_tasks(&self) -> Result<Vec<AiTaskRecord>, String> {
        let rows = sqlx::query(
            "SELECT id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                    EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                    requested_model, effective_answer_model, effective_planner_model,
                    input_json::text AS input_json, result_json::text AS result_json,
                    error_json::text AS error_json, progress_pct, phase, cancel_requested,
                    checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json
             FROM ai_task
             WHERE status IN ('queued', 'running', 'verifying')
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("failed to list recoverable ai tasks: {e}"))?;

        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            let mut task = task_from_row(&row)?;
            task.artifacts = load_artifacts(&self.pool, &task.id).await?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    async fn append_event(
        &self,
        task_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<AiTaskEventRecord, String> {
        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| format!("failed to serialize ai task event payload: {e}"))?;
        let row = sqlx::query(
            "INSERT INTO ai_task_event (task_id, event_type, payload_json)
             VALUES ($1::uuid, $2, $3::jsonb)
             RETURNING id, task_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                       event_type, payload_json::text AS payload_json",
        )
        .bind(task_id)
        .bind(event_type)
        .bind(payload_json)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("failed to append ai task event: {e}"))?;
        event_from_row(&row)
    }

    async fn list_events_for_user(
        &self,
        task_id: &str,
        user_id: &str,
        after_id: Option<i64>,
    ) -> Result<Vec<AiTaskEventRecord>, String> {
        let rows = if let Some(after_id) = after_id {
            sqlx::query(
                "SELECT e.id, e.task_id, EXTRACT(EPOCH FROM e.created_at)::bigint AS created_ts,
                        e.event_type, e.payload_json::text AS payload_json
                 FROM ai_task_event e
                 INNER JOIN ai_task t ON t.id = e.task_id
                 WHERE e.task_id = $1::uuid
                   AND t.owner_user_id = $2
                   AND e.id > $3
                 ORDER BY e.id ASC",
            )
            .bind(task_id)
            .bind(user_id)
            .bind(after_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("failed to list ai task events: {e}"))?
        } else {
            sqlx::query(
                "SELECT e.id, e.task_id, EXTRACT(EPOCH FROM e.created_at)::bigint AS created_ts,
                        e.event_type, e.payload_json::text AS payload_json
                 FROM ai_task_event e
                 INNER JOIN ai_task t ON t.id = e.task_id
                 WHERE e.task_id = $1::uuid
                   AND t.owner_user_id = $2
                 ORDER BY e.id ASC",
            )
            .bind(task_id)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| format!("failed to list ai task events: {e}"))?
        };

        rows.into_iter().map(|row| event_from_row(&row)).collect()
    }

    async fn write_checkpoint(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        payload: Value,
    ) -> Result<AiTaskCheckpointRecord, String> {
        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| format!("failed to serialize ai task checkpoint: {e}"))?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| format!("failed to begin ai task checkpoint transaction: {e}"))?;

        let current = sqlx::query(
            "SELECT checkpoint_version
             FROM ai_task
             WHERE id = $1::uuid",
        )
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("failed to load ai task checkpoint version: {e}"))?;
        let next_version = current
            .try_get::<i32, _>("checkpoint_version")
            .map_err(|e| e.to_string())?
            + 1;

        let row = sqlx::query(
            "INSERT INTO ai_task_checkpoint (task_id, version, phase, payload_json)
             VALUES ($1::uuid, $2, $3, $4::jsonb)
             RETURNING id, task_id, version, phase, payload_json::text AS payload_json,
                       EXTRACT(EPOCH FROM created_at)::bigint AS created_ts",
        )
        .bind(task_id)
        .bind(next_version)
        .bind(phase.as_str())
        .bind(&payload_json)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("failed to insert ai task checkpoint: {e}"))?;

        sqlx::query(
            "UPDATE ai_task
             SET checkpoint_version = $2,
                 last_checkpoint_json = $3::jsonb,
                 phase = $4,
                 updated_at = NOW()
             WHERE id = $1::uuid",
        )
        .bind(task_id)
        .bind(next_version)
        .bind(&payload_json)
        .bind(phase.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("failed to update ai task checkpoint state: {e}"))?;

        tx.commit()
            .await
            .map_err(|e| format!("failed to commit ai task checkpoint transaction: {e}"))?;

        checkpoint_from_row(&row)
    }

    async fn list_checkpoints(&self, task_id: &str) -> Result<Vec<AiTaskCheckpointRecord>, String> {
        let rows = sqlx::query(
            "SELECT id, task_id, version, phase, payload_json::text AS payload_json,
                    EXTRACT(EPOCH FROM created_at)::bigint AS created_ts
             FROM ai_task_checkpoint
             WHERE task_id = $1::uuid
             ORDER BY version ASC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("failed to list ai task checkpoints: {e}"))?;

        rows.into_iter()
            .map(|row| checkpoint_from_row(&row))
            .collect()
    }

    async fn update_progress(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        progress_pct: f64,
        effective_answer_model: Option<&str>,
        effective_planner_model: Option<&str>,
    ) -> Result<Option<AiTaskRecord>, String> {
        let row = sqlx::query(
            "UPDATE ai_task
             SET progress_pct = $2,
                 phase = $3,
                 effective_answer_model = COALESCE($4, effective_answer_model),
                 effective_planner_model = COALESCE($5, effective_planner_model),
                 updated_at = NOW()
             WHERE id = $1::uuid
             RETURNING id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                       EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                       requested_model, effective_answer_model, effective_planner_model,
                       input_json::text AS input_json, result_json::text AS result_json,
                       error_json::text AS error_json, progress_pct, phase, cancel_requested,
                       checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json",
        )
        .bind(task_id)
        .bind(progress_pct.clamp(0.0, 100.0))
        .bind(phase.as_str())
        .bind(effective_answer_model)
        .bind(effective_planner_model)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("failed to update ai task progress: {e}"))?;

        match row {
            Some(row) => {
                let mut task = task_from_row(&row)?;
                task.artifacts = load_artifacts(&self.pool, &task.id).await?;
                Ok(Some(task))
            }
            None => Ok(None),
        }
    }

    async fn transition_status(
        &self,
        task_id: &str,
        expected: &[AiTaskStatus],
        next: AiTaskStatus,
        phase: AiTaskPhase,
        result_json: Option<Value>,
        error_json: Option<Value>,
        clear_cancel_requested: bool,
    ) -> Result<Option<AiTaskRecord>, String> {
        if expected.is_empty() {
            return Ok(None);
        }
        for status in expected {
            if !valid_status_transition(*status, next) && *status != next {
                return Err(format!(
                    "invalid ai task status transition from {} to {}",
                    status.as_str(),
                    next.as_str()
                ));
            }
        }

        let sql = update_transition_query(expected.len());
        let mut query = sqlx::query(&sql).bind(task_id);
        for status in expected {
            query = query.bind(status.as_str());
        }
        let row = query
            .bind(next.as_str())
            .bind(phase.as_str())
            .bind(
                result_json
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| format!("failed to serialize ai task result payload: {e}"))?,
            )
            .bind(
                error_json
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|e| format!("failed to serialize ai task error payload: {e}"))?,
            )
            .bind(clear_cancel_requested)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| format!("failed to transition ai task status: {e}"))?;

        match row {
            Some(row) => {
                let mut task = task_from_row(&row)?;
                task.artifacts = load_artifacts(&self.pool, &task.id).await?;
                Ok(Some(task))
            }
            None => Ok(None),
        }
    }

    async fn request_cancel(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        let Some(task) = self.get_task_for_user(task_id, user_id).await? else {
            return Ok(None);
        };

        match task.status {
            AiTaskStatus::Queued => {
                self.transition_status(
                    task_id,
                    &[AiTaskStatus::Queued],
                    AiTaskStatus::Cancelled,
                    AiTaskPhase::Cancelled,
                    None,
                    None,
                    true,
                )
                .await
            }
            AiTaskStatus::Running | AiTaskStatus::Verifying => {
                let row = sqlx::query(
                    "UPDATE ai_task
                     SET cancel_requested = TRUE, updated_at = NOW()
                     WHERE id = $1::uuid AND owner_user_id = $2
                     RETURNING id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                               EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                               requested_model, effective_answer_model, effective_planner_model,
                               input_json::text AS input_json, result_json::text AS result_json,
                               error_json::text AS error_json, progress_pct, phase, cancel_requested,
                               checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json",
                )
                .bind(task_id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| format!("failed to request ai task cancellation: {e}"))?;
                match row {
                    Some(row) => {
                        let mut task = task_from_row(&row)?;
                        task.artifacts = load_artifacts(&self.pool, &task.id).await?;
                        Ok(Some(task))
                    }
                    None => Ok(None),
                }
            }
            AiTaskStatus::Completed | AiTaskStatus::Failed | AiTaskStatus::Cancelled => {
                Ok(Some(task))
            }
        }
    }

    async fn resume_task(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        let Some(task) = self.get_task_for_user(task_id, user_id).await? else {
            return Ok(None);
        };
        if !matches!(task.status, AiTaskStatus::Failed | AiTaskStatus::Cancelled) {
            return Ok(None);
        }
        let row = sqlx::query(
            "UPDATE ai_task
             SET status = 'queued',
                 phase = 'queued',
                 cancel_requested = FALSE,
                 error_json = NULL,
                 result_json = NULL,
                 updated_at = NOW()
             WHERE id = $1::uuid AND owner_user_id = $2
             RETURNING id, owner_user_id, EXTRACT(EPOCH FROM created_at)::bigint AS created_ts,
                       EXTRACT(EPOCH FROM updated_at)::bigint AS updated_ts, task_type, status,
                       requested_model, effective_answer_model, effective_planner_model,
                       input_json::text AS input_json, result_json::text AS result_json,
                       error_json::text AS error_json, progress_pct, phase, cancel_requested,
                       checkpoint_version, last_checkpoint_json::text AS last_checkpoint_json",
        )
        .bind(task_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("failed to resume ai task: {e}"))?;
        match row {
            Some(row) => {
                let mut task = task_from_row(&row)?;
                task.artifacts = load_artifacts(&self.pool, &task.id).await?;
                Ok(Some(task))
            }
            None => Ok(None),
        }
    }

    async fn create_artifact(
        &self,
        task_id: &str,
        user_id: &str,
        kind: &str,
        file_name: &str,
        media_type: &str,
        storage_path: &str,
        size_bytes: i64,
    ) -> Result<AiTaskArtifactRecord, String> {
        let task = self
            .get_task_for_user(task_id, user_id)
            .await?
            .ok_or_else(|| "ai task artifact owner mismatch".to_string())?;
        let artifact_id = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT INTO ai_task_artifact (id, task_id, kind, file_name, media_type, storage_path, size_bytes)
             VALUES ($1::uuid, $2::uuid, $3, $4, $5, $6, $7)",
        )
        .bind(&artifact_id)
        .bind(&task.id)
        .bind(kind)
        .bind(file_name)
        .bind(media_type)
        .bind(storage_path)
        .bind(size_bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("failed to create ai task artifact: {e}"))?;

        self.get_artifact_for_user(task_id, &artifact_id, user_id)
            .await?
            .ok_or_else(|| "created ai task artifact is missing".to_string())
    }

    async fn get_artifact_for_user(
        &self,
        task_id: &str,
        artifact_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskArtifactRecord>, String> {
        let row = sqlx::query(
            "SELECT a.id, a.task_id, a.kind, a.file_name, a.media_type, a.storage_path, a.size_bytes,
                    EXTRACT(EPOCH FROM a.created_at)::bigint AS created_ts
             FROM ai_task_artifact a
             INNER JOIN ai_task t ON t.id = a.task_id
             WHERE a.task_id = $1::uuid
               AND a.id = $2::uuid
               AND t.owner_user_id = $3",
        )
        .bind(task_id)
        .bind(artifact_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("failed to load ai task artifact: {e}"))?;

        row.map(|row| artifact_from_row(&row)).transpose()
    }
}

#[derive(Debug, Clone, Default)]
struct MemoryStoreState {
    tasks: HashMap<String, AiTaskRecord>,
    events: HashMap<String, Vec<AiTaskEventRecord>>,
    checkpoints: HashMap<String, Vec<AiTaskCheckpointRecord>>,
    artifacts: HashMap<String, Vec<AiTaskArtifactRecord>>,
    next_event_id: i64,
    next_checkpoint_id: i64,
}

#[derive(Clone, Default)]
pub struct MemoryAiTaskStore {
    state: Arc<tokio::sync::Mutex<MemoryStoreState>>,
}

impl MemoryAiTaskStore {
    fn attach_artifacts(state: &MemoryStoreState, task: &AiTaskRecord) -> AiTaskRecord {
        let mut task = task.clone();
        task.artifacts = state.artifacts.get(&task.id).cloned().unwrap_or_default();
        task
    }
}

#[async_trait]
impl AiTaskStore for MemoryAiTaskStore {
    async fn create_task(
        &self,
        owner: &TaskUserContext,
        request: &CreateAiTaskRequest,
    ) -> Result<AiTaskRecord, String> {
        let mut state = self.state.lock().await;
        let now = chrono::Utc::now().timestamp();
        let task = AiTaskRecord {
            id: uuid::Uuid::new_v4().to_string(),
            owner_user_id: owner.user_id.clone(),
            created_ts: now,
            updated_ts: now,
            task_type: request.input.task_type(),
            status: AiTaskStatus::Queued,
            requested_model: request.requested_model.clone(),
            effective_answer_model: None,
            effective_planner_model: None,
            input_json: serde_json::to_value(&request.input)
                .map_err(|e| format!("failed to serialize memory ai task input: {e}"))?,
            result_json: None,
            error_json: None,
            progress_pct: 0.0,
            phase: AiTaskPhase::Queued,
            cancel_requested: false,
            checkpoint_version: 0,
            last_checkpoint_json: None,
            artifacts: Vec::new(),
        };
        state.tasks.insert(task.id.clone(), task.clone());
        Ok(task)
    }

    async fn list_tasks_for_user(&self, user_id: &str) -> Result<Vec<AiTaskRecord>, String> {
        let state = self.state.lock().await;
        let mut tasks = state
            .tasks
            .values()
            .filter(|task| task.owner_user_id == user_id)
            .map(|task| Self::attach_artifacts(&state, task))
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| -task.created_ts);
        Ok(tasks)
    }

    async fn get_task_for_user(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        let state = self.state.lock().await;
        Ok(state
            .tasks
            .get(task_id)
            .filter(|task| task.owner_user_id == user_id)
            .map(|task| Self::attach_artifacts(&state, task)))
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<AiTaskRecord>, String> {
        let state = self.state.lock().await;
        Ok(state
            .tasks
            .get(task_id)
            .map(|task| Self::attach_artifacts(&state, task)))
    }

    async fn get_task_user_context(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskUserContext>, String> {
        let state = self.state.lock().await;
        Ok(state.tasks.get(task_id).map(|task| TaskUserContext {
            user_id: task.owner_user_id.clone(),
            username: task.owner_user_id.clone(),
            role: "user".to_string(),
            is_admin: false,
        }))
    }

    async fn list_recoverable_tasks(&self) -> Result<Vec<AiTaskRecord>, String> {
        let state = self.state.lock().await;
        Ok(state
            .tasks
            .values()
            .filter(|task| {
                matches!(
                    task.status,
                    AiTaskStatus::Queued | AiTaskStatus::Running | AiTaskStatus::Verifying
                )
            })
            .map(|task| Self::attach_artifacts(&state, task))
            .collect())
    }

    async fn append_event(
        &self,
        task_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<AiTaskEventRecord, String> {
        let mut state = self.state.lock().await;
        state.next_event_id += 1;
        let event = AiTaskEventRecord {
            id: state.next_event_id,
            task_id: task_id.to_string(),
            created_ts: chrono::Utc::now().timestamp(),
            event_type: event_type.to_string(),
            payload,
        };
        state
            .events
            .entry(task_id.to_string())
            .or_default()
            .push(event.clone());
        Ok(event)
    }

    async fn list_events_for_user(
        &self,
        task_id: &str,
        user_id: &str,
        after_id: Option<i64>,
    ) -> Result<Vec<AiTaskEventRecord>, String> {
        let state = self.state.lock().await;
        let Some(task) = state.tasks.get(task_id) else {
            return Ok(Vec::new());
        };
        if task.owner_user_id != user_id {
            return Ok(Vec::new());
        }
        Ok(state
            .events
            .get(task_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| after_id.map(|id| event.id > id).unwrap_or(true))
            .collect())
    }

    async fn write_checkpoint(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        payload: Value,
    ) -> Result<AiTaskCheckpointRecord, String> {
        let mut state = self.state.lock().await;
        state.next_checkpoint_id += 1;
        let checkpoint_id = state.next_checkpoint_id;
        let now = chrono::Utc::now().timestamp();
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| "memory ai task not found".to_string())?;
        task.checkpoint_version += 1;
        let version = task.checkpoint_version;
        task.phase = phase;
        task.last_checkpoint_json = Some(payload.clone());
        task.updated_ts = now;
        let checkpoint = AiTaskCheckpointRecord {
            id: checkpoint_id,
            task_id: task_id.to_string(),
            version,
            phase,
            payload,
            created_ts: now,
        };
        state
            .checkpoints
            .entry(task_id.to_string())
            .or_default()
            .push(checkpoint.clone());
        Ok(checkpoint)
    }

    async fn list_checkpoints(&self, task_id: &str) -> Result<Vec<AiTaskCheckpointRecord>, String> {
        let state = self.state.lock().await;
        Ok(state.checkpoints.get(task_id).cloned().unwrap_or_default())
    }

    async fn update_progress(
        &self,
        task_id: &str,
        phase: AiTaskPhase,
        progress_pct: f64,
        effective_answer_model: Option<&str>,
        effective_planner_model: Option<&str>,
    ) -> Result<Option<AiTaskRecord>, String> {
        let mut state = self.state.lock().await;
        let Some(task) = state.tasks.get_mut(task_id) else {
            return Ok(None);
        };
        task.phase = phase;
        task.progress_pct = progress_pct.clamp(0.0, 100.0);
        if let Some(value) = effective_answer_model {
            task.effective_answer_model = Some(value.to_string());
        }
        if let Some(value) = effective_planner_model {
            task.effective_planner_model = Some(value.to_string());
        }
        task.updated_ts = chrono::Utc::now().timestamp();
        let task_snapshot = task.clone();
        Ok(Some(Self::attach_artifacts(&state, &task_snapshot)))
    }

    async fn transition_status(
        &self,
        task_id: &str,
        expected: &[AiTaskStatus],
        next: AiTaskStatus,
        phase: AiTaskPhase,
        result_json: Option<Value>,
        error_json: Option<Value>,
        clear_cancel_requested: bool,
    ) -> Result<Option<AiTaskRecord>, String> {
        let mut state = self.state.lock().await;
        let Some(task) = state.tasks.get_mut(task_id) else {
            return Ok(None);
        };
        if !expected.contains(&task.status) {
            return Ok(None);
        }
        if task.status != next && !valid_status_transition(task.status, next) {
            return Err(format!(
                "invalid ai task status transition from {} to {}",
                task.status.as_str(),
                next.as_str()
            ));
        }
        task.status = next;
        task.phase = phase;
        task.result_json = result_json;
        task.error_json = error_json;
        if clear_cancel_requested {
            task.cancel_requested = false;
        }
        task.updated_ts = chrono::Utc::now().timestamp();
        let task_snapshot = task.clone();
        Ok(Some(Self::attach_artifacts(&state, &task_snapshot)))
    }

    async fn request_cancel(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        let mut state = self.state.lock().await;
        let Some(task) = state.tasks.get_mut(task_id) else {
            return Ok(None);
        };
        if task.owner_user_id != user_id {
            return Ok(None);
        }
        match task.status {
            AiTaskStatus::Queued => {
                task.status = AiTaskStatus::Cancelled;
                task.phase = AiTaskPhase::Cancelled;
                task.updated_ts = chrono::Utc::now().timestamp();
            }
            AiTaskStatus::Running | AiTaskStatus::Verifying => {
                task.cancel_requested = true;
                task.updated_ts = chrono::Utc::now().timestamp();
            }
            AiTaskStatus::Completed | AiTaskStatus::Failed | AiTaskStatus::Cancelled => {}
        }
        let task_snapshot = task.clone();
        Ok(Some(Self::attach_artifacts(&state, &task_snapshot)))
    }

    async fn resume_task(
        &self,
        task_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskRecord>, String> {
        let mut state = self.state.lock().await;
        let Some(task) = state.tasks.get_mut(task_id) else {
            return Ok(None);
        };
        if task.owner_user_id != user_id {
            return Ok(None);
        }
        if !matches!(task.status, AiTaskStatus::Failed | AiTaskStatus::Cancelled) {
            return Ok(None);
        }
        task.status = AiTaskStatus::Queued;
        task.phase = AiTaskPhase::Queued;
        task.cancel_requested = false;
        task.error_json = None;
        task.result_json = None;
        task.updated_ts = chrono::Utc::now().timestamp();
        let task_snapshot = task.clone();
        Ok(Some(Self::attach_artifacts(&state, &task_snapshot)))
    }

    async fn create_artifact(
        &self,
        task_id: &str,
        user_id: &str,
        kind: &str,
        file_name: &str,
        media_type: &str,
        storage_path: &str,
        size_bytes: i64,
    ) -> Result<AiTaskArtifactRecord, String> {
        let mut state = self.state.lock().await;
        let task = state
            .tasks
            .get(task_id)
            .ok_or_else(|| "memory ai task not found".to_string())?;
        if task.owner_user_id != user_id {
            return Err("memory ai task artifact owner mismatch".to_string());
        }
        let artifact = AiTaskArtifactRecord {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            kind: kind.to_string(),
            file_name: file_name.to_string(),
            media_type: media_type.to_string(),
            storage_path: storage_path.to_string(),
            size_bytes,
            created_ts: chrono::Utc::now().timestamp(),
        };
        state
            .artifacts
            .entry(task_id.to_string())
            .or_default()
            .push(artifact.clone());
        Ok(artifact)
    }

    async fn get_artifact_for_user(
        &self,
        task_id: &str,
        artifact_id: &str,
        user_id: &str,
    ) -> Result<Option<AiTaskArtifactRecord>, String> {
        let state = self.state.lock().await;
        let Some(task) = state.tasks.get(task_id) else {
            return Ok(None);
        };
        if task.owner_user_id != user_id {
            return Ok(None);
        }
        Ok(state.artifacts.get(task_id).and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact.id == artifact_id)
                .cloned()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{AiTaskStore, MemoryAiTaskStore};
    use crate::ai_tasks::types::{
        AiTaskInput, AiTaskPhase, AiTaskStatus, CreateAiTaskRequest, TaskUserContext,
    };
    use serde_json::json;

    fn user() -> TaskUserContext {
        TaskUserContext {
            user_id: "user-1".to_string(),
            username: "tester".to_string(),
            role: "user".to_string(),
            is_admin: false,
        }
    }

    #[tokio::test]
    async fn memory_store_supports_task_create_list_and_get() {
        let store = MemoryAiTaskStore::default();
        let created = store
            .create_task(
                &user(),
                &CreateAiTaskRequest {
                    requested_model: Some("tiny.gguf".to_string()),
                    input: AiTaskInput::GroundedDocumentGeneration {
                        prompt: "Summarize the network".to_string(),
                        title: None,
                        format: None,
                    },
                },
            )
            .await
            .expect("task created");

        let listed = store
            .list_tasks_for_user(&user().user_id)
            .await
            .expect("tasks listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);

        let loaded = store
            .get_task_for_user(&created.id, &user().user_id)
            .await
            .expect("task loaded")
            .expect("task exists");
        assert_eq!(loaded.task_type.as_str(), "grounded_document_generation");
    }

    #[tokio::test]
    async fn memory_store_can_cancel_resume_and_replay_events() {
        let store = MemoryAiTaskStore::default();
        let created = store
            .create_task(
                &user(),
                &CreateAiTaskRequest {
                    requested_model: None,
                    input: AiTaskInput::DeepResearchReport {
                        objective: "Research host health".to_string(),
                        max_workers: Some(2),
                    },
                },
            )
            .await
            .expect("task created");

        let running = store
            .transition_status(
                &created.id,
                &[AiTaskStatus::Queued],
                AiTaskStatus::Running,
                AiTaskPhase::Planning,
                None,
                None,
                true,
            )
            .await
            .expect("transition ok")
            .expect("task transitioned");
        assert_eq!(running.status, AiTaskStatus::Running);

        store
            .append_event(&created.id, "planning_started", json!({ "step": 1 }))
            .await
            .expect("event appended");

        let cancelled = store
            .request_cancel(&created.id, &user().user_id)
            .await
            .expect("cancel requested")
            .expect("task exists");
        assert!(cancelled.cancel_requested);

        let failed = store
            .transition_status(
                &created.id,
                &[AiTaskStatus::Running],
                AiTaskStatus::Failed,
                AiTaskPhase::Failed,
                None,
                Some(json!({ "message": "boom" })),
                true,
            )
            .await
            .expect("transition ok")
            .expect("task transitioned");
        assert_eq!(failed.status, AiTaskStatus::Failed);

        let resumed = store
            .resume_task(&created.id, &user().user_id)
            .await
            .expect("resume ok")
            .expect("task resumed");
        assert_eq!(resumed.status, AiTaskStatus::Queued);

        let events = store
            .list_events_for_user(&created.id, &user().user_id, None)
            .await
            .expect("events listed");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "planning_started");
    }

    #[tokio::test]
    async fn memory_store_persists_checkpoints_and_artifacts() {
        let store = MemoryAiTaskStore::default();
        let created = store
            .create_task(
                &user(),
                &CreateAiTaskRequest {
                    requested_model: None,
                    input: AiTaskInput::GroundedDocumentGeneration {
                        prompt: "Summarize channels".to_string(),
                        title: Some("channels.md".to_string()),
                        format: None,
                    },
                },
            )
            .await
            .expect("task created");

        let checkpoint = store
            .write_checkpoint(
                &created.id,
                AiTaskPhase::Drafting,
                json!({ "draft": "hello" }),
            )
            .await
            .expect("checkpoint written");
        assert_eq!(checkpoint.version, 1);

        let artifact = store
            .create_artifact(
                &created.id,
                &user().user_id,
                "report",
                "channels.md",
                "text/markdown; charset=utf-8",
                "/tmp/channels.md",
                42,
            )
            .await
            .expect("artifact created");

        let loaded = store
            .get_artifact_for_user(&created.id, &artifact.id, &user().user_id)
            .await
            .expect("artifact load ok")
            .expect("artifact exists");
        assert_eq!(loaded.file_name, "channels.md");

        let checkpoints = store
            .list_checkpoints(&created.id)
            .await
            .expect("checkpoints listed");
        assert_eq!(checkpoints.len(), 1);
    }
}
