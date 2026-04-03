use crate::DbPool;

#[derive(Debug, Clone)]
pub struct AiGeneratedArtifactRow {
    pub id: String,
    pub user_id: String,
    pub conversation_id: Option<String>,
    pub title: String,
    pub file_name: String,
    pub media_type: String,
    pub content_text: String,
    pub byte_size: i64,
    pub verification_status: String,
    pub verification_attempts: i32,
    pub verification_notes_json: String,
    pub verified_ts: Option<i64>,
    pub trace_id: Option<String>,
    pub created_ts: i64,
}

pub struct CreateAiGeneratedArtifactParams<'a> {
    pub user_id: &'a str,
    pub conversation_id: Option<&'a str>,
    pub title: &'a str,
    pub file_name: &'a str,
    pub media_type: &'a str,
    pub content_text: &'a str,
    pub byte_size: i64,
    pub verification_status: &'a str,
    pub verification_attempts: i32,
    pub verification_notes_json: &'a str,
    pub verified_ts: Option<i64>,
    pub trace_id: Option<&'a str>,
}

fn map_row(
    row: (
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        i64,
        String,
        i32,
        String,
        Option<i64>,
        Option<String>,
        i64,
    ),
) -> AiGeneratedArtifactRow {
    let (
        id,
        user_id,
        conversation_id,
        title,
        file_name,
        media_type,
        content_text,
        byte_size,
        verification_status,
        verification_attempts,
        verification_notes_json,
        verified_ts,
        trace_id,
        created_ts,
    ) = row;

    AiGeneratedArtifactRow {
        id,
        user_id,
        conversation_id,
        title,
        file_name,
        media_type,
        content_text,
        byte_size,
        verification_status,
        verification_attempts,
        verification_notes_json,
        verified_ts,
        trace_id,
        created_ts,
    }
}

pub async fn create_artifact(
    pool: &DbPool,
    params: CreateAiGeneratedArtifactParams<'_>,
) -> Result<AiGeneratedArtifactRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_ts = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO ai_generated_artifact (
            id, user_id, conversation_id, title, file_name, media_type, content_text, byte_size,
            verification_status, verification_attempts, verification_notes_json, verified_ts,
            trace_id, created_ts
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(&id)
    .bind(params.user_id)
    .bind(params.conversation_id)
    .bind(params.title)
    .bind(params.file_name)
    .bind(params.media_type)
    .bind(params.content_text)
    .bind(params.byte_size)
    .bind(params.verification_status)
    .bind(params.verification_attempts)
    .bind(params.verification_notes_json)
    .bind(params.verified_ts)
    .bind(params.trace_id)
    .bind(created_ts)
    .execute(pool)
    .await?;

    get_artifact_for_user(pool, &id, params.user_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn get_artifact_for_user(
    pool: &DbPool,
    artifact_id: &str,
    user_id: &str,
) -> Result<Option<AiGeneratedArtifactRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        i64,
        String,
        i32,
        String,
        Option<i64>,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        "SELECT id, user_id, conversation_id, title, file_name, media_type, content_text, byte_size,
                verification_status, verification_attempts, verification_notes_json, verified_ts,
                trace_id, created_ts
         FROM ai_generated_artifact
         WHERE id = $1 AND user_id = $2",
    )
    .bind(artifact_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_row))
}
