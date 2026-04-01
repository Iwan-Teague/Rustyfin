use crate::DbPool;

#[derive(Debug, Clone)]
pub struct AiConversationRow {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub archived: bool,
    pub last_message_preview: Option<String>,
    pub last_model_name: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

#[derive(Debug, Clone)]
pub struct AiConversationTurnRow {
    pub id: String,
    pub conversation_id: String,
    pub user_id: String,
    pub turn_index: i64,
    pub role: String,
    pub content: String,
    pub model_name: Option<String>,
    pub grounding_tools_json: String,
    pub follow_up_contexts_json: String,
    pub grounding_sources_json: String,
    pub activity_trace_json: String,
    pub stats_json: Option<String>,
    pub trace_id: Option<String>,
    pub created_ts: i64,
}

pub struct CreateAiConversationParams<'a> {
    pub user_id: &'a str,
    pub title: &'a str,
}

pub struct CreateAiConversationTurnParams<'a> {
    pub conversation_id: &'a str,
    pub user_id: &'a str,
    pub role: &'a str,
    pub content: &'a str,
    pub model_name: Option<&'a str>,
    pub grounding_tools_json: &'a str,
    pub follow_up_contexts_json: &'a str,
    pub grounding_sources_json: &'a str,
    pub activity_trace_json: &'a str,
    pub stats_json: Option<&'a str>,
    pub trace_id: Option<&'a str>,
}

fn map_conversation_row(
    row: (
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        i64,
        i64,
    ),
) -> AiConversationRow {
    let (
        id,
        user_id,
        title,
        archived,
        last_message_preview,
        last_model_name,
        created_ts,
        updated_ts,
    ) = row;

    AiConversationRow {
        id,
        user_id,
        title,
        archived,
        last_message_preview,
        last_model_name,
        created_ts,
        updated_ts,
    }
}

fn map_turn_row(
    row: (
        String,
        String,
        String,
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
    ),
) -> AiConversationTurnRow {
    let (
        id,
        conversation_id,
        user_id,
        turn_index,
        role,
        content,
        model_name,
        grounding_tools_json,
        follow_up_contexts_json,
        grounding_sources_json,
        activity_trace_json,
        stats_json,
        trace_id,
        created_ts,
    ) = row;

    AiConversationTurnRow {
        id,
        conversation_id,
        user_id,
        turn_index,
        role,
        content,
        model_name,
        grounding_tools_json,
        follow_up_contexts_json,
        grounding_sources_json,
        activity_trace_json,
        stats_json,
        trace_id,
        created_ts,
    }
}

pub async fn create_conversation(
    pool: &DbPool,
    params: CreateAiConversationParams<'_>,
) -> Result<AiConversationRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO ai_conversation (
            id, user_id, title, archived, last_message_preview, last_model_name, created_ts, updated_ts
        ) VALUES ($1, $2, $3, FALSE, NULL, NULL, $4, $5)",
    )
    .bind(&id)
    .bind(params.user_id)
    .bind(params.title)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    get_conversation_for_user(pool, &id, params.user_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn list_conversations_for_user(
    pool: &DbPool,
    user_id: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<AiConversationRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, user_id, title, archived, last_message_preview, last_model_name, created_ts, updated_ts
         FROM ai_conversation
         WHERE user_id = $1
           AND ($2 = TRUE OR archived = FALSE)
         ORDER BY updated_ts DESC, id DESC
         LIMIT $3",
    )
    .bind(user_id)
    .bind(include_archived)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_conversation_row).collect())
}

pub async fn get_conversation_for_user(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
) -> Result<Option<AiConversationRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, user_id, title, archived, last_message_preview, last_model_name, created_ts, updated_ts
         FROM ai_conversation
         WHERE id = $1 AND user_id = $2",
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_conversation_row))
}

pub async fn update_conversation_for_user(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
    title: Option<&str>,
    archived: Option<bool>,
) -> Result<Option<AiConversationRow>, sqlx::Error> {
    let Some(current) = get_conversation_for_user(pool, conversation_id, user_id).await? else {
        return Ok(None);
    };

    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE ai_conversation
         SET title = $1, archived = $2, updated_ts = $3
         WHERE id = $4 AND user_id = $5",
    )
    .bind(title.unwrap_or(&current.title))
    .bind(archived.unwrap_or(current.archived))
    .bind(now)
    .bind(conversation_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    get_conversation_for_user(pool, conversation_id, user_id).await
}

pub async fn delete_conversation_for_user(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM ai_conversation WHERE id = $1 AND user_id = $2")
        .bind(conversation_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn replace_title_if_default(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
    title: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE ai_conversation
         SET title = $1, updated_ts = $2
         WHERE id = $3 AND user_id = $4 AND title = 'New chat'",
    )
    .bind(title)
    .bind(chrono::Utc::now().timestamp())
    .bind(conversation_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn touch_conversation_from_turn(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
    last_message_preview: &str,
    last_model_name: Option<&str>,
    archived: Option<bool>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE ai_conversation
         SET last_message_preview = $1,
             last_model_name = COALESCE($2, last_model_name),
             archived = COALESCE($3, archived),
             updated_ts = $4
         WHERE id = $5 AND user_id = $6",
    )
    .bind(last_message_preview)
    .bind(last_model_name)
    .bind(archived)
    .bind(chrono::Utc::now().timestamp())
    .bind(conversation_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_turns_for_conversation(
    pool: &DbPool,
    conversation_id: &str,
    user_id: &str,
) -> Result<Vec<AiConversationTurnRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        "SELECT t.id, t.conversation_id, t.user_id, t.turn_index, t.role, t.content, t.model_name,
                t.grounding_tools_json, t.follow_up_contexts_json, t.grounding_sources_json,
                t.activity_trace_json, t.stats_json, t.trace_id, t.created_ts
         FROM ai_conversation_turn t
         INNER JOIN ai_conversation c ON c.id = t.conversation_id
         WHERE t.conversation_id = $1 AND c.user_id = $2
         ORDER BY t.turn_index ASC, t.created_ts ASC, t.id ASC",
    )
    .bind(conversation_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(map_turn_row).collect())
}

pub async fn create_turn(
    pool: &DbPool,
    params: CreateAiConversationTurnParams<'_>,
) -> Result<AiConversationTurnRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let mut tx = pool.begin().await?;

    let next_turn_index: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(turn_index), -1) + 1
         FROM ai_conversation_turn
         WHERE conversation_id = $1",
    )
    .bind(params.conversation_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO ai_conversation_turn (
            id, conversation_id, user_id, turn_index, role, content, model_name,
            grounding_tools_json, follow_up_contexts_json, grounding_sources_json,
            activity_trace_json, stats_json, trace_id, created_ts
         ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10,
            $11, $12, $13, $14
         )",
    )
    .bind(&id)
    .bind(params.conversation_id)
    .bind(params.user_id)
    .bind(next_turn_index)
    .bind(params.role)
    .bind(params.content)
    .bind(params.model_name)
    .bind(params.grounding_tools_json)
    .bind(params.follow_up_contexts_json)
    .bind(params.grounding_sources_json)
    .bind(params.activity_trace_json)
    .bind(params.stats_json)
    .bind(params.trace_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(AiConversationTurnRow {
        id,
        conversation_id: params.conversation_id.to_string(),
        user_id: params.user_id.to_string(),
        turn_index: next_turn_index,
        role: params.role.to_string(),
        content: params.content.to_string(),
        model_name: params.model_name.map(str::to_string),
        grounding_tools_json: params.grounding_tools_json.to_string(),
        follow_up_contexts_json: params.follow_up_contexts_json.to_string(),
        grounding_sources_json: params.grounding_sources_json.to_string(),
        activity_trace_json: params.activity_trace_json.to_string(),
        stats_json: params.stats_json.map(str::to_string),
        trace_id: params.trace_id.map(str::to_string),
        created_ts: now,
    })
}
