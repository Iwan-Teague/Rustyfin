use crate::DbPool;
use sqlx::{FromRow, Row, postgres::PgRow};

#[derive(Debug, Clone)]
pub struct AiAssistantTurnJournalRow {
    pub id: String,
    pub user_id: String,
    pub conversation_id: Option<String>,
    pub request_turn_id: Option<String>,
    pub request_turn_index: Option<i64>,
    pub trace_id: String,
    pub request_message: String,
    pub model_name: String,
    pub response_mode: String,
    pub planner_mode: Option<String>,
    pub status: String,
    pub current_phase: String,
    pub history_len: i64,
    pub planner_debug_json: String,
    pub prompt_debug_json: Option<String>,
    pub metrics_json: Option<String>,
    pub overload_reason: Option<String>,
    pub error_message: Option<String>,
    pub compact_boundary_count: i64,
    pub artifact_verification_json: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub finished_ts: Option<i64>,
}

pub struct CreateAiAssistantTurnJournalParams<'a> {
    pub id: &'a str,
    pub user_id: &'a str,
    pub conversation_id: Option<&'a str>,
    pub request_turn_id: Option<&'a str>,
    pub request_turn_index: Option<i64>,
    pub trace_id: &'a str,
    pub request_message: &'a str,
    pub model_name: &'a str,
    pub response_mode: &'a str,
    pub planner_mode: Option<&'a str>,
    pub status: &'a str,
    pub current_phase: &'a str,
    pub history_len: i64,
    pub planner_debug_json: &'a str,
    pub prompt_debug_json: Option<&'a str>,
    pub metrics_json: Option<&'a str>,
    pub overload_reason: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub compact_boundary_count: i64,
    pub artifact_verification_json: Option<&'a str>,
    pub finished_ts: Option<i64>,
}

pub type UpdateAiAssistantTurnJournalParams<'a> = CreateAiAssistantTurnJournalParams<'a>;

impl<'r> FromRow<'r, PgRow> for AiAssistantTurnJournalRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            conversation_id: row.try_get("conversation_id")?,
            request_turn_id: row.try_get("request_turn_id")?,
            request_turn_index: row.try_get("request_turn_index")?,
            trace_id: row.try_get("trace_id")?,
            request_message: row.try_get("request_message")?,
            model_name: row.try_get("model_name")?,
            response_mode: row.try_get("response_mode")?,
            planner_mode: row.try_get("planner_mode")?,
            status: row.try_get("status")?,
            current_phase: row.try_get("current_phase")?,
            history_len: row.try_get("history_len")?,
            planner_debug_json: row.try_get("planner_debug_json")?,
            prompt_debug_json: row.try_get("prompt_debug_json")?,
            metrics_json: row.try_get("metrics_json")?,
            overload_reason: row.try_get("overload_reason")?,
            error_message: row.try_get("error_message")?,
            compact_boundary_count: row.try_get("compact_boundary_count")?,
            artifact_verification_json: row.try_get("artifact_verification_json")?,
            created_ts: row.try_get("created_ts")?,
            updated_ts: row.try_get("updated_ts")?,
            finished_ts: row.try_get("finished_ts")?,
        })
    }
}

pub async fn create_journal(
    pool: &DbPool,
    params: CreateAiAssistantTurnJournalParams<'_>,
) -> Result<AiAssistantTurnJournalRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO ai_assistant_turn_journal (
            id, user_id, conversation_id, request_turn_id, request_turn_index,
            trace_id, request_message, model_name, response_mode, planner_mode,
            status, current_phase, history_len, planner_debug_json, prompt_debug_json,
            metrics_json, overload_reason, error_message, compact_boundary_count,
            artifact_verification_json, created_ts, updated_ts, finished_ts
        ) VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8, $9, $10,
            $11, $12, $13, $14, $15,
            $16, $17, $18, $19,
            $20, $21, $22, $23
        )",
    )
    .bind(params.id)
    .bind(params.user_id)
    .bind(params.conversation_id)
    .bind(params.request_turn_id)
    .bind(params.request_turn_index)
    .bind(params.trace_id)
    .bind(params.request_message)
    .bind(params.model_name)
    .bind(params.response_mode)
    .bind(params.planner_mode)
    .bind(params.status)
    .bind(params.current_phase)
    .bind(params.history_len)
    .bind(params.planner_debug_json)
    .bind(params.prompt_debug_json)
    .bind(params.metrics_json)
    .bind(params.overload_reason)
    .bind(params.error_message)
    .bind(params.compact_boundary_count)
    .bind(params.artifact_verification_json)
    .bind(now)
    .bind(now)
    .bind(params.finished_ts)
    .execute(pool)
    .await?;

    get_journal(pool, params.id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn update_journal(
    pool: &DbPool,
    id: &str,
    params: UpdateAiAssistantTurnJournalParams<'_>,
) -> Result<Option<AiAssistantTurnJournalRow>, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "UPDATE ai_assistant_turn_journal
         SET planner_mode = $1,
             status = $2,
             current_phase = $3,
             history_len = $4,
             planner_debug_json = $5,
             prompt_debug_json = $6,
             metrics_json = $7,
             overload_reason = $8,
             error_message = $9,
             compact_boundary_count = $10,
             artifact_verification_json = $11,
             updated_ts = $12,
             finished_ts = $13
         WHERE id = $14
           AND user_id = $15",
    )
    .bind(params.planner_mode)
    .bind(params.status)
    .bind(params.current_phase)
    .bind(params.history_len)
    .bind(params.planner_debug_json)
    .bind(params.prompt_debug_json)
    .bind(params.metrics_json)
    .bind(params.overload_reason)
    .bind(params.error_message)
    .bind(params.compact_boundary_count)
    .bind(params.artifact_verification_json)
    .bind(now)
    .bind(params.finished_ts)
    .bind(id)
    .bind(params.user_id)
    .execute(pool)
    .await?;

    get_journal(pool, id).await
}

pub async fn get_journal(
    pool: &DbPool,
    journal_id: &str,
) -> Result<Option<AiAssistantTurnJournalRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, user_id, conversation_id, request_turn_id, request_turn_index,
                trace_id, request_message, model_name, response_mode, planner_mode,
                status, current_phase, history_len, planner_debug_json, prompt_debug_json,
                metrics_json, overload_reason, error_message, compact_boundary_count,
                artifact_verification_json, created_ts, updated_ts, finished_ts
         FROM ai_assistant_turn_journal
         WHERE id = $1",
    )
    .bind(journal_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_recent_journals(
    pool: &DbPool,
    limit: i64,
) -> Result<Vec<AiAssistantTurnJournalRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, user_id, conversation_id, request_turn_id, request_turn_index,
                trace_id, request_message, model_name, response_mode, planner_mode,
                status, current_phase, history_len, planner_debug_json, prompt_debug_json,
                metrics_json, overload_reason, error_message, compact_boundary_count,
                artifact_verification_json, created_ts, updated_ts, finished_ts
         FROM ai_assistant_turn_journal
         ORDER BY created_ts DESC, id DESC
         LIMIT $1",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await
}
