use crate::DbPool;

#[derive(Debug, Clone)]
pub struct AiAssistantConfirmationTokenRow {
    pub id: String,
    pub user_id: String,
    pub action_kind: String,
    pub payload_json: String,
    pub expires_ts: i64,
    pub consumed_ts: Option<i64>,
}

pub struct CreateAiAssistantConfirmationTokenParams<'a> {
    pub user_id: &'a str,
    pub action_kind: &'a str,
    pub payload_json: &'a str,
    pub expires_ts: i64,
}

fn map_token_row(
    row: (String, String, String, String, i64, Option<i64>),
) -> AiAssistantConfirmationTokenRow {
    let (id, user_id, action_kind, payload_json, expires_ts, consumed_ts) = row;
    AiAssistantConfirmationTokenRow {
        id,
        user_id,
        action_kind,
        payload_json,
        expires_ts,
        consumed_ts,
    }
}

pub async fn create_confirmation_token(
    pool: &DbPool,
    params: CreateAiAssistantConfirmationTokenParams<'_>,
) -> Result<AiAssistantConfirmationTokenRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO ai_assistant_confirmation_token (
            id, user_id, action_kind, payload_json, expires_ts, consumed_ts
         ) VALUES ($1, $2, $3, $4, $5, NULL)",
    )
    .bind(&id)
    .bind(params.user_id)
    .bind(params.action_kind)
    .bind(params.payload_json)
    .bind(params.expires_ts)
    .execute(pool)
    .await?;

    Ok(AiAssistantConfirmationTokenRow {
        id,
        user_id: params.user_id.to_string(),
        action_kind: params.action_kind.to_string(),
        payload_json: params.payload_json.to_string(),
        expires_ts: params.expires_ts,
        consumed_ts: None,
    })
}

pub async fn get_confirmation_token_for_user(
    pool: &DbPool,
    token_id: &str,
    user_id: &str,
) -> Result<Option<AiAssistantConfirmationTokenRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, user_id, action_kind, payload_json, expires_ts, consumed_ts
         FROM ai_assistant_confirmation_token
         WHERE id = $1 AND user_id = $2",
    )
    .bind(token_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_token_row))
}

pub async fn consume_confirmation_token(
    pool: &DbPool,
    token_id: &str,
    user_id: &str,
    consumed_ts: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE ai_assistant_confirmation_token
         SET consumed_ts = $1
         WHERE id = $2 AND user_id = $3 AND consumed_ts IS NULL",
    )
    .bind(consumed_ts)
    .bind(token_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
