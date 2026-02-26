use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct TranscriptSessionRow {
    pub id: String,
    pub channel_id: String,
    pub status: String,
    pub started_by_user_id: String,
    pub started_by_username: String,
    pub started_ts: i64,
    pub ended_ts: Option<i64>,
    pub output_path: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptEntryRow {
    pub id: String,
    pub session_id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub started_ts_ms: i64,
    pub ended_ts_ms: i64,
    pub text: String,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct NewTranscriptEntry<'a> {
    pub session_id: &'a str,
    pub channel_id: &'a str,
    pub user_id: &'a str,
    pub username: &'a str,
    pub started_ts_ms: i64,
    pub ended_ts_ms: i64,
    pub text: &'a str,
}

fn map_session(
    (
        id,
        channel_id,
        status,
        started_by_user_id,
        started_by_username,
        started_ts,
        ended_ts,
        output_path,
        failure_reason,
    ): (
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    ),
) -> TranscriptSessionRow {
    TranscriptSessionRow {
        id,
        channel_id,
        status,
        started_by_user_id,
        started_by_username,
        started_ts,
        ended_ts,
        output_path,
        failure_reason,
    }
}

fn map_entry(
    (id, session_id, channel_id, user_id, username, started_ts_ms, ended_ts_ms, text, created_ts): (
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        i64,
    ),
) -> TranscriptEntryRow {
    TranscriptEntryRow {
        id,
        session_id,
        channel_id,
        user_id,
        username,
        started_ts_ms,
        ended_ts_ms,
        text,
        created_ts,
    }
}

pub async fn create_running_session(
    pool: &SqlitePool,
    channel_id: &str,
    started_by_user_id: &str,
    started_by_username: &str,
) -> Result<TranscriptSessionRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let started_ts = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO channel_transcript_session \
         (id, channel_id, status, started_by_user_id, started_by_username, started_ts) \
         VALUES (?, ?, 'running', ?, ?, ?)",
    )
    .bind(&id)
    .bind(channel_id)
    .bind(started_by_user_id)
    .bind(started_by_username)
    .bind(started_ts)
    .execute(pool)
    .await?;

    Ok(TranscriptSessionRow {
        id,
        channel_id: channel_id.to_string(),
        status: "running".to_string(),
        started_by_user_id: started_by_user_id.to_string(),
        started_by_username: started_by_username.to_string(),
        started_ts,
        ended_ts: None,
        output_path: None,
        failure_reason: None,
    })
}

pub async fn get_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<TranscriptSessionRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, status, started_by_user_id, started_by_username, \
                started_ts, ended_ts, output_path, failure_reason \
         FROM channel_transcript_session \
         WHERE id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_session))
}

pub async fn get_running_session_for_channel(
    pool: &SqlitePool,
    channel_id: &str,
) -> Result<Option<TranscriptSessionRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, status, started_by_user_id, started_by_username, \
                started_ts, ended_ts, output_path, failure_reason \
         FROM channel_transcript_session \
         WHERE channel_id = ? AND status = 'running' \
         ORDER BY started_ts DESC \
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_session))
}

pub async fn get_latest_session_for_channel(
    pool: &SqlitePool,
    channel_id: &str,
) -> Result<Option<TranscriptSessionRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, status, started_by_user_id, started_by_username, \
                started_ts, ended_ts, output_path, failure_reason \
         FROM channel_transcript_session \
         WHERE channel_id = ? \
         ORDER BY started_ts DESC \
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(map_session))
}

pub async fn list_sessions_for_channel(
    pool: &SqlitePool,
    channel_id: &str,
    limit: i64,
) -> Result<Vec<TranscriptSessionRow>, sqlx::Error> {
    let clamped_limit = limit.clamp(1, 500);
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, status, started_by_user_id, started_by_username, \
                started_ts, ended_ts, output_path, failure_reason \
         FROM channel_transcript_session \
         WHERE channel_id = ? \
         ORDER BY started_ts DESC \
         LIMIT ?",
    )
    .bind(channel_id)
    .bind(clamped_limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_session).collect())
}

pub async fn list_running_sessions(
    pool: &SqlitePool,
) -> Result<Vec<TranscriptSessionRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, channel_id, status, started_by_user_id, started_by_username, \
                started_ts, ended_ts, output_path, failure_reason \
         FROM channel_transcript_session \
         WHERE status = 'running' \
         ORDER BY started_ts ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_session).collect())
}

pub async fn append_entry(
    pool: &SqlitePool,
    entry: NewTranscriptEntry<'_>,
) -> Result<TranscriptEntryRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_ts = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO channel_transcript_entry \
         (id, session_id, channel_id, user_id, username, started_ts_ms, ended_ts_ms, text, created_ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(entry.session_id)
    .bind(entry.channel_id)
    .bind(entry.user_id)
    .bind(entry.username)
    .bind(entry.started_ts_ms)
    .bind(entry.ended_ts_ms)
    .bind(entry.text)
    .bind(created_ts)
    .execute(pool)
    .await?;

    Ok(TranscriptEntryRow {
        id,
        session_id: entry.session_id.to_string(),
        channel_id: entry.channel_id.to_string(),
        user_id: entry.user_id.to_string(),
        username: entry.username.to_string(),
        started_ts_ms: entry.started_ts_ms,
        ended_ts_ms: entry.ended_ts_ms,
        text: entry.text.to_string(),
        created_ts,
    })
}

pub async fn list_entries_for_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<TranscriptEntryRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        String,
        i64,
    )> = sqlx::query_as(
        "SELECT id, session_id, channel_id, user_id, username, started_ts_ms, ended_ts_ms, text, created_ts \
         FROM channel_transcript_entry \
         WHERE session_id = ? \
         ORDER BY started_ts_ms ASC, created_ts ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_entry).collect())
}

pub async fn count_entries_for_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM channel_transcript_entry WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

pub async fn count_entries_for_sessions(
    pool: &SqlitePool,
    session_ids: &[String],
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(session_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT session_id, COUNT(*) \
         FROM channel_transcript_entry \
         WHERE session_id IN ({placeholders}) \
         GROUP BY session_id"
    );

    let mut query = sqlx::query_as::<_, (String, i64)>(&sql);
    for session_id in session_ids {
        query = query.bind(session_id);
    }
    query.fetch_all(pool).await
}

pub async fn delete_session(pool: &SqlitePool, session_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM channel_transcript_session WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn complete_session(
    pool: &SqlitePool,
    session_id: &str,
    output_path: &str,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE channel_transcript_session \
         SET status = 'completed', ended_ts = ?, output_path = ?, failure_reason = NULL \
         WHERE id = ?",
    )
    .bind(now)
    .bind(output_path)
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn cancel_session(pool: &SqlitePool, session_id: &str) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE channel_transcript_session \
         SET status = 'cancelled', ended_ts = ?, output_path = NULL, failure_reason = NULL \
         WHERE id = ?",
    )
    .bind(now)
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fail_session(
    pool: &SqlitePool,
    session_id: &str,
    reason: &str,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE channel_transcript_session \
         SET status = 'failed', ended_ts = ?, output_path = NULL, failure_reason = ? \
         WHERE id = ?",
    )
    .bind(now)
    .bind(reason)
    .bind(session_id)
    .execute(pool)
    .await?;
    Ok(())
}
