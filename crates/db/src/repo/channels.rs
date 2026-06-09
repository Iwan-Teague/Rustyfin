use std::collections::HashMap;

use crate::DbPool;

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
    pub avatar_path: Option<String>,
    pub content: String,
    pub created_ts: i64,
    /// Monotonic ordering key (migration 030). Surfaced so clients can report the highest
    /// message they have seen back to the channel read-cursor (`set_channel_last_read`).
    pub sort_seq: i64,
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

pub async fn list_channels(pool: &DbPool) -> Result<Vec<ChannelRow>, sqlx::Error> {
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

pub async fn get_channel(pool: &DbPool, id: &str) -> Result<Option<ChannelRow>, sqlx::Error> {
    let row: Option<(String, String, String, i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, name, kind, position, is_private, created_by, created_ts \
         FROM channel WHERE id = $1",
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
    pool: &DbPool,
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
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
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

pub async fn rename_channel(pool: &DbPool, id: &str, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE channel SET name = $1 WHERE id = $2")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_channel(
    pool: &DbPool,
    id: &str,
    name: &str,
    is_private: bool,
) -> Result<(), sqlx::Error> {
    let is_private_int: i64 = if is_private { 1 } else { 0 };
    sqlx::query("UPDATE channel SET name = $1, is_private = $2 WHERE id = $3")
        .bind(name)
        .bind(is_private_int)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_channel(pool: &DbPool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM channel WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_message(pool: &DbPool, id: &str) -> Result<Option<MessageRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, Option<String>, String, i64, i64)> = sqlx::query_as(
        "SELECT m.id, m.channel_id, m.user_id, COALESCE(NULLIF(u.display_name, ''), m.username) AS username, \
                u.avatar_path, m.content, m.created_ts, m.sort_seq \
         FROM channel_message m \
         LEFT JOIN \"user\" u ON u.id = m.user_id \
         WHERE m.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, channel_id, user_id, username, avatar_path, content, created_ts, sort_seq)| {
            MessageRow {
                id,
                channel_id,
                user_id,
                username,
                avatar_path,
                content,
                created_ts,
                sort_seq,
            }
        },
    ))
}

pub async fn delete_message(pool: &DbPool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM channel_message WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_messages(
    pool: &DbPool,
    channel_id: &str,
    limit: i64,
    before_ts: i64,
    before_id: Option<&str>,
) -> Result<Vec<MessageRow>, sqlx::Error> {
    let normalized_before_id = before_id.map(str::trim).filter(|id| !id.is_empty());
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        i64,
        i64,
    )> = if let Some(before_id) = normalized_before_id {
        sqlx::query_as(
                "SELECT m.id, m.channel_id, m.user_id, COALESCE(NULLIF(u.display_name, ''), m.username) AS username, \
                        u.avatar_path, m.content, m.created_ts, m.sort_seq \
                 FROM channel_message m \
                 LEFT JOIN \"user\" u ON u.id = m.user_id \
                 WHERE m.channel_id = $1 \
                   AND (
                        m.created_ts < $2
                        OR (
                            EXISTS(SELECT 1 FROM channel_message pivot WHERE pivot.id = $3)
                            AND m.sort_seq < (
                                SELECT pivot.sort_seq FROM channel_message pivot WHERE pivot.id = $3
                            )
                        )
                   ) \
                 ORDER BY m.sort_seq DESC \
                 LIMIT $4",
            )
            .bind(channel_id)
            .bind(before_ts)
            .bind(before_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_as(
                "SELECT m.id, m.channel_id, m.user_id, COALESCE(NULLIF(u.display_name, ''), m.username) AS username, \
                        u.avatar_path, m.content, m.created_ts, m.sort_seq \
                 FROM channel_message m \
                 LEFT JOIN \"user\" u ON u.id = m.user_id \
                 WHERE m.channel_id = $1 AND m.created_ts < $2 \
                 ORDER BY m.sort_seq DESC \
                 LIMIT $3",
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
            |(id, channel_id, user_id, username, avatar_path, content, created_ts, sort_seq)| {
                MessageRow {
                    id,
                    channel_id,
                    user_id,
                    username,
                    avatar_path,
                    content,
                    created_ts,
                    sort_seq,
                }
            },
        )
        .collect();

    // Reverse to ascending order
    messages.reverse();
    Ok(messages)
}

pub async fn create_message(
    pool: &DbPool,
    channel_id: &str,
    user_id: &str,
    username: &str,
    content: &str,
) -> Result<MessageRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let (sort_seq,): (i64,) = sqlx::query_as(
        "INSERT INTO channel_message (id, channel_id, user_id, username, content, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING sort_seq",
    )
    .bind(&id)
    .bind(channel_id)
    .bind(user_id)
    .bind(username)
    .bind(content)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(MessageRow {
        id,
        channel_id: channel_id.to_string(),
        user_id: user_id.to_string(),
        username: username.to_string(),
        avatar_path: None,
        content: content.to_string(),
        created_ts: now,
        sort_seq,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn create_message_with_attachment(
    pool: &DbPool,
    channel_id: &str,
    user_id: &str,
    username: &str,
    content: &str,
    filename: &str,
    content_type: &str,
    size_bytes: i64,
    storage_path: &str,
) -> Result<(MessageRow, MessageAttachmentRow), sqlx::Error> {
    let message_id = uuid::Uuid::new_v4().to_string();
    let attachment_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let mut tx = pool.begin().await?;

    let (sort_seq,): (i64,) = sqlx::query_as(
        "INSERT INTO channel_message (id, channel_id, user_id, username, content, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING sort_seq",
    )
    .bind(&message_id)
    .bind(channel_id)
    .bind(user_id)
    .bind(username)
    .bind(content)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO channel_message_attachment \
         (id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&attachment_id)
    .bind(&message_id)
    .bind(channel_id)
    .bind(filename)
    .bind(content_type)
    .bind(size_bytes)
    .bind(storage_path)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((
        MessageRow {
            id: message_id.clone(),
            channel_id: channel_id.to_string(),
            user_id: user_id.to_string(),
            username: username.to_string(),
            avatar_path: None,
            content: content.to_string(),
            created_ts: now,
            sort_seq,
        },
        MessageAttachmentRow {
            id: attachment_id,
            message_id,
            channel_id: channel_id.to_string(),
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            size_bytes,
            storage_path: storage_path.to_string(),
            created_ts: now,
        },
    ))
}

pub async fn create_message_attachment(
    pool: &DbPool,
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
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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
    pool: &DbPool,
    message_id: &str,
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE message_id = $1 \
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
    pool: &DbPool,
    message_ids: &[String],
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(1, message_ids.len());
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
    pool: &DbPool,
    channel_id: &str,
) -> Result<Vec<MessageAttachmentRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE channel_id = $1 \
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
    pool: &DbPool,
    id: &str,
) -> Result<Option<MessageAttachmentRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, String, i64, String, i64)> = sqlx::query_as(
        "SELECT id, message_id, channel_id, filename, content_type, size_bytes, storage_path, created_ts \
         FROM channel_message_attachment \
         WHERE id = $1",
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

// ── Per-user channel read state (unread / "new activity") ──────────────────────

/// Records that `user_id` has read `channel_id` up to `seq` (a `channel_message.sort_seq`).
///
/// The cursor is monotonic: `GREATEST` keeps the stored value if it is already ahead of
/// `seq`, so an out-of-order or stale mark-read request can never move the read cursor
/// backwards and resurrect already-seen messages as unread.
pub async fn set_channel_last_read(
    pool: &DbPool,
    user_id: &str,
    channel_id: &str,
    seq: i64,
    now_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO channel_read_state (user_id, channel_id, last_read_sort_seq, updated_ts) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, channel_id) DO UPDATE SET \
             last_read_sort_seq = GREATEST(channel_read_state.last_read_sort_seq, EXCLUDED.last_read_sort_seq), \
             updated_ts = EXCLUDED.updated_ts",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(seq)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

/// Returns the per-channel unread message count for `user_id` across the given channels.
///
/// One query: for each requested channel, counts the messages whose `sort_seq` is greater
/// than the user's `last_read_sort_seq` (treated as 0 when no read-state row exists, via a
/// per-user `LEFT JOIN`). Channels with no unread messages — including those the user has
/// never opened but that hold no newer messages — map to 0. Channel ids not present in the
/// returned map should likewise be treated as 0 by the caller.
///
/// Empty input short-circuits to an empty map with no query.
pub async fn channel_unread_counts(
    pool: &DbPool,
    user_id: &str,
    channel_ids: &[String],
) -> sqlx::Result<HashMap<String, i64>> {
    if channel_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // $1 = user_id; channel ids occupy $2..$(2 + len).
    let placeholders = crate::repo::dollar_placeholders(2, channel_ids.len());
    let sql = format!(
        "SELECT c.id, \
                COUNT(m.id) FILTER ( \
                    WHERE m.sort_seq > COALESCE(rs.last_read_sort_seq, 0) \
                ) AS unread \
         FROM channel c \
         LEFT JOIN channel_read_state rs \
                ON rs.channel_id = c.id AND rs.user_id = $1 \
         LEFT JOIN channel_message m \
                ON m.channel_id = c.id \
         WHERE c.id IN ({placeholders}) \
         GROUP BY c.id, rs.last_read_sort_seq"
    );

    let mut query = sqlx::query_as::<_, (String, i64)>(&sql).bind(user_id);
    for channel_id in channel_ids {
        query = query.bind(channel_id);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows.into_iter().collect())
}

/// Returns the highest `channel_message.sort_seq` in a channel, or 0 if it has no messages.
///
/// Useful for "mark everything read" callers (and tests) that need the channel's current
/// high-water mark to pass to [`set_channel_last_read`].
pub async fn channel_max_sort_seq(pool: &DbPool, channel_id: &str) -> Result<i64, sqlx::Error> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT MAX(sort_seq) FROM channel_message WHERE channel_id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(max,)| max).unwrap_or(0))
}
