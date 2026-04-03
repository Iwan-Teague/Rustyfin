use crate::DbPool;

#[derive(Debug, Clone)]
pub struct AiConversationCompactBoundaryRow {
    pub id: String,
    pub conversation_id: String,
    pub user_id: String,
    pub trace_id: Option<String>,
    pub from_turn_index: i64,
    pub to_turn_index: i64,
    pub summarized_turn_count: i64,
    pub memory_state_json: String,
    pub created_ts: i64,
}

pub struct CreateAiConversationCompactBoundaryParams<'a> {
    pub conversation_id: &'a str,
    pub user_id: &'a str,
    pub trace_id: Option<&'a str>,
    pub from_turn_index: i64,
    pub to_turn_index: i64,
    pub summarized_turn_count: i64,
    pub memory_state_json: &'a str,
}

fn map_row(
    row: (
        String,
        String,
        String,
        Option<String>,
        i64,
        i64,
        i64,
        String,
        i64,
    ),
) -> AiConversationCompactBoundaryRow {
    let (
        id,
        conversation_id,
        user_id,
        trace_id,
        from_turn_index,
        to_turn_index,
        summarized_turn_count,
        memory_state_json,
        created_ts,
    ) = row;

    AiConversationCompactBoundaryRow {
        id,
        conversation_id,
        user_id,
        trace_id,
        from_turn_index,
        to_turn_index,
        summarized_turn_count,
        memory_state_json,
        created_ts,
    }
}

pub async fn create_compact_boundary(
    pool: &DbPool,
    params: CreateAiConversationCompactBoundaryParams<'_>,
) -> Result<AiConversationCompactBoundaryRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO ai_conversation_compact_boundary (
            id, conversation_id, user_id, trace_id, from_turn_index, to_turn_index,
            summarized_turn_count, memory_state_json, created_ts
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(&id)
    .bind(params.conversation_id)
    .bind(params.user_id)
    .bind(params.trace_id)
    .bind(params.from_turn_index)
    .bind(params.to_turn_index)
    .bind(params.summarized_turn_count)
    .bind(params.memory_state_json)
    .bind(now)
    .execute(pool)
    .await?;

    latest_compact_boundary_for_conversation(pool, params.conversation_id, params.user_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn latest_compact_boundary_for_conversation(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
) -> Result<Option<AiConversationCompactBoundaryRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        Option<String>,
        i64,
        i64,
        i64,
        String,
        i64,
    )> = sqlx::query_as(
        "SELECT id, conversation_id, user_id, trace_id, from_turn_index, to_turn_index,
                summarized_turn_count, memory_state_json, created_ts
         FROM ai_conversation_compact_boundary
         WHERE conversation_id = $1
           AND user_id = $2
         ORDER BY to_turn_index DESC, created_ts DESC, id DESC
         LIMIT 1",
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_row))
}

pub async fn list_recent_compact_boundaries(
    pool: &DbPool,
    limit: i64,
) -> Result<Vec<AiConversationCompactBoundaryRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        i64,
        i64,
        i64,
        String,
        i64,
    )> = sqlx::query_as(
        "SELECT id, conversation_id, user_id, trace_id, from_turn_index, to_turn_index,
                summarized_turn_count, memory_state_json, created_ts
         FROM ai_conversation_compact_boundary
         ORDER BY created_ts DESC, id DESC
         LIMIT $1",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_row).collect())
}
