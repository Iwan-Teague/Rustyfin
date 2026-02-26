use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub position: i64,
    pub is_private: bool,
    pub created_by: String,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct MessageAttachmentRow {
    pub id: String,
    pub message_id: String,
    pub channel_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub storage_path: String,
    pub created_ts: i64,
}

pub async fn list_channels(pool: &SqlitePool) -> Result<Vec<ChannelRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, name, kind, position, is_private, created_by, created_ts \
         FROM channel ORDER BY position, created_ts",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, name, kind, position, is_private, created_by, created_ts)| ChannelRow {
                id,
                name,
                kind,
                position,
                is_private: is_private != 0,
                created_by,
                created_ts,
            },
        )
        .collect())
}

pub async fn get_channel(pool: &SqlitePool, id: &str) -> Result<Option<ChannelRow>, sqlx::Error> {
    let row: Option<(String, String, String, i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, name, kind, position, is_private, created_by, created_ts \
         FROM channel WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, name, kind, position, is_private, created_by, created_ts)| ChannelRow {
            id,
            name,
            kind,
            position,
            is_private: is_private != 0,
            created_by,
            created_ts,
        },
    ))
}

pub async fn create_channel(
    pool: &SqlitePool,
    name: &str,
    kind: &str,
    is_private: bool,
    created_by: &str,
    position: i64,
) -> Result<ChannelRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let is_private_int: i64 = if is_private { 1 } else { 0 };

    sqlx::query(
        "INSERT INTO channel (id, name, kind, position, is_private, created_by, created_ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(kind)
    .bind(position)
    .bind(is_private_int)
    .bind(created_by)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(ChannelRow {
        id,
        name: name.to_string(),
        kind: kind.to_string(),
        position,
        is_private,
        created_by: created_by.to_string(),
        created_ts: now,
    })
}

pub async fn rename_channel(pool: &SqlitePool, id: &str, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE channel SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_channel(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    is_private: bool,
) -> Result<(), sqlx::Error> {
    let is_private_int: i64 = if is_private { 1 } else { 0 };
    sqlx::query("UPDATE channel SET name = ?, is_private = ? WHERE id = ?")
        .bind(name)
        .bind(is_private_int)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_channel(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM channel WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_message(pool: &SqlitePool, id: &str) -> Result<Option<MessageRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, channel_id, user_id, username, content, created_ts \
         FROM channel_message WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, channel_id, user_id, username, content, created_ts)| MessageRow {
            id,
            channel_id,
            user_id,
            username,
            content,
            created_ts,
        },
    ))
}

pub async fn delete_message(pool: &SqlitePool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM channel_message WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_messages(
    pool: &SqlitePool,
    channel_id: &str,
    limit: i64,
    before_ts: i64,
    before_id: Option<&str>,
) -> Result<Vec<MessageRow>, sqlx::Error> {
    let normalized_before_id = before_id.map(str::trim).filter(|id| !id.is_empty());
    let rows: Vec<(String, String, String, String, String, i64)> =
        if let Some(before_id) = normalized_before_id {
            sqlx::query_as(
                "SELECT id, channel_id, user_id, username, content, created_ts \
                 FROM channel_message \
                 WHERE channel_id = ? \
                   AND (created_ts < ? OR (created_ts = ? AND id < ?)) \
                 ORDER BY created_ts DESC, id DESC \
                 LIMIT ?",
            )
            .bind(channel_id)
            .bind(before_ts)
            .bind(before_ts)
            .bind(before_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id, channel_id, user_id, username, content, created_ts \
                 FROM channel_message \
                 WHERE channel_id = ? AND created_ts < ? \
                 ORDER BY created_ts DESC, id DESC \
                 LIMIT ?",
            )
            .bind(channel_id)
            .bind(before_ts)
            .bind(limit)
            .fetch_all(pool)
            .await?
        };

    let mut messages: Vec<MessageRow> = rows
        .into_iter()
        .map(
            |(id, channel_id, user_id, username, content, created_ts)| MessageRow {
                id,
                channel_id,
                user_id,
                username,
                content,
                created_ts,
            },
        )
        .collect();

    // Reverse to ascending order
    messages.reverse();
    Ok(messages)
}

pub async fn create_message(
    pool: &SqlitePool,
    channel_id: &str,
    user_id: &str,
    username: &str,
    content: &str,
) -> Result<MessageRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO channel_message (id, channel_id, user_id, username, content, created_ts) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(channel_id)
    .bind(user_id)
    .bind(username)
    .bind(content)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(MessageRow {
        id,
        channel_id: channel_id.to_string(),
        user_id: user_id.to_string(),
        username: username.to_string(),
        content: content.to_string(),
        created_ts: now,
    })
}

pub async fn create_message_attachment(
    pool: &SqlitePool,
    message_id: &str,
    channel_id: &str,
    filename: &str,
    content_type: &str,
    size_bytes: i64,
    storage_path: &str,
) -> Result<MessageAttachmentRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO channel_message_attachment \
         (id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(message_id)
    .bind(channel_id)
    .bind(filename)
    .bind(content_type)
    .bind(size_bytes)
    .bind(storage_path)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(MessageAttachmentRow {
        id,
        message_id: message_id.to_string(),
        channel_id: channel_id.to_string(),
        filename: filename.to_string(),
        content_type: content_type.to_string(),
        size_bytes,
        storage_path: storage_path.to_string(),
        created_ts: now,
    })
}

pub async fn list_message_attachments(
    pool: &SqlitePool,
    message_id: &str,
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE message_id = ? \
         ORDER BY created_ts, id",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            )| MessageAttachmentRow {
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            },
        )
        .collect())
}

pub async fn list_message_attachments_for_messages(
    pool: &SqlitePool,
    message_ids: &[String],
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(message_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE message_id IN ({placeholders}) \
         ORDER BY message_id, created_ts, id"
    );

    let mut query =
        sqlx::query_as::<_, (String, String, String, String, String, i64, String, i64)>(&sql);
    for message_id in message_ids {
        query = query.bind(message_id);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            )| MessageAttachmentRow {
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            },
        )
        .collect())
}

pub async fn list_channel_attachments(
    pool: &SqlitePool,
    channel_id: &str,
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE channel_id = ? \
         ORDER BY created_ts, id",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            )| MessageAttachmentRow {
                id,
                message_id,
                channel_id,
                filename,
                content_type,
                size_bytes,
                storage_path,
                created_ts,
            },
        )
        .collect())
}

pub async fn get_message_attachment(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<MessageAttachmentRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            message_id,
            channel_id,
            filename,
            content_type,
            size_bytes,
            storage_path,
            created_ts,
        )| MessageAttachmentRow {
            id,
            message_id,
            channel_id,
            filename,
            content_type,
            size_bytes,
            storage_path,
            created_ts,
        },
    ))
}
