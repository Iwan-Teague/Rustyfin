use crate::DbPool;

#[derive(Debug, Clone)]
pub struct AiAssistantAuditEventRow {
    pub id: String,
    pub trace_id: String,
    pub user_id: String,
    pub username: String,
    pub user_role: String,
    pub model_name: String,
    pub message_preview: String,
    pub history_len: i64,
    pub response_kind: String,
    pub planned_tools_json: String,
    pub executed_tools_json: String,
    pub grounding_chunks_json: String,
    pub grounding_sources_json: String,
    pub error_message: Option<String>,
    pub created_ts: i64,
}

pub struct CreateAiAssistantAuditEventParams<'a> {
    pub trace_id: &'a str,
    pub user_id: &'a str,
    pub username: &'a str,
    pub user_role: &'a str,
    pub model_name: &'a str,
    pub message_preview: &'a str,
    pub history_len: i64,
    pub response_kind: &'a str,
    pub planned_tools_json: &'a str,
    pub executed_tools_json: &'a str,
    pub grounding_chunks_json: &'a str,
    pub grounding_sources_json: &'a str,
    pub error_message: Option<&'a str>,
}

pub async fn create_audit_event(
    pool: &DbPool,
    params: CreateAiAssistantAuditEventParams<'_>,
) -> Result<AiAssistantAuditEventRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO ai_assistant_audit_event (
            id, trace_id, user_id, username, user_role, model_name, message_preview,
            history_len, response_kind, planned_tools_json, executed_tools_json,
            grounding_chunks_json, grounding_sources_json, error_message, created_ts
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10, $11,
            $12, $13, $14, $15
        )",
    )
    .bind(&id)
    .bind(params.trace_id)
    .bind(params.user_id)
    .bind(params.username)
    .bind(params.user_role)
    .bind(params.model_name)
    .bind(params.message_preview)
    .bind(params.history_len)
    .bind(params.response_kind)
    .bind(params.planned_tools_json)
    .bind(params.executed_tools_json)
    .bind(params.grounding_chunks_json)
    .bind(params.grounding_sources_json)
    .bind(params.error_message)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(AiAssistantAuditEventRow {
        id,
        trace_id: params.trace_id.to_string(),
        user_id: params.user_id.to_string(),
        username: params.username.to_string(),
        user_role: params.user_role.to_string(),
        model_name: params.model_name.to_string(),
        message_preview: params.message_preview.to_string(),
        history_len: params.history_len,
        response_kind: params.response_kind.to_string(),
        planned_tools_json: params.planned_tools_json.to_string(),
        executed_tools_json: params.executed_tools_json.to_string(),
        grounding_chunks_json: params.grounding_chunks_json.to_string(),
        grounding_sources_json: params.grounding_sources_json.to_string(),
        error_message: params.error_message.map(str::to_string),
        created_ts: now,
    })
}

pub async fn list_audit_events(
    pool: &DbPool,
    limit: i64,
) -> Result<Vec<AiAssistantAuditEventRow>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        "SELECT id, trace_id, user_id, username, user_role, model_name, message_preview,
                history_len, response_kind, planned_tools_json, executed_tools_json,
                grounding_chunks_json, grounding_sources_json, error_message, created_ts
         FROM ai_assistant_audit_event
         ORDER BY created_ts DESC, id DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                trace_id,
                user_id,
                username,
                user_role,
                model_name,
                message_preview,
                history_len,
                response_kind,
                planned_tools_json,
                executed_tools_json,
                grounding_chunks_json,
                grounding_sources_json,
                error_message,
                created_ts,
            )| AiAssistantAuditEventRow {
                id,
                trace_id,
                user_id,
                username,
                user_role,
                model_name,
                message_preview,
                history_len,
                response_kind,
                planned_tools_json,
                executed_tools_json,
                grounding_chunks_json,
                grounding_sources_json,
                error_message,
                created_ts,
            },
        )
        .collect())
}

pub async fn delete_audit_events_older_than(
    pool: &DbPool,
    cutoff_ts: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ai_assistant_audit_event WHERE created_ts < $1")
        .bind(cutoff_ts)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
