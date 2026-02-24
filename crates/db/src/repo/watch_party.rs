use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct WatchPartyRoomRow {
    pub id: String,
    pub room_name: String,
    pub host_user_id: String,
    pub item_id: String,
    pub status: String,
    pub policy_json: String,
    pub join_password_hash: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub room_mode: String,
    pub audio_library_id: Option<String>,
    pub youtube_video_id: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WatchPartyMemberRow {
    pub room_id: String,
    pub user_id: String,
    pub role: String,
    pub status: String,
    pub invited_by: Option<String>,
    pub invited_ts: Option<i64>,
    pub joined_ts: Option<i64>,
    pub last_seen_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct WatchPartyInviteSummary {
    pub room_id: String,
    pub item_id: String,
    pub item_title: String,
    pub host_user_id: String,
    pub host_username: String,
    pub created_ts: i64,
    pub password_required: bool,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct NewWatchPartyMember {
    pub user_id: String,
    pub role: String,
    pub status: String,
    pub invited_by: Option<String>,
    pub invited_ts: Option<i64>,
    pub joined_ts: Option<i64>,
}

pub async fn create_room_with_members(
    pool: &SqlitePool,
    host_user_id: &str,
    room_name: Option<&str>,
    item_id: Option<&str>,
    policy_json: &str,
    join_password_hash: Option<&str>,
    members: &[NewWatchPartyMember],
    room_mode: Option<&str>,
    audio_library_id: Option<&str>,
    web_url: Option<&str>,
) -> Result<WatchPartyRoomRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let room_id = uuid::Uuid::new_v4().to_string();
    let mode = room_mode.unwrap_or("video");
    let name = room_name.unwrap_or("").trim();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO watch_party_room \
         (id, room_name, host_user_id, item_id, status, policy_json, join_password_hash, created_ts, updated_ts, room_mode, audio_library_id, web_url) \
         VALUES (?, ?, ?, ?, 'lobby', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&room_id)
    .bind(name)
    .bind(host_user_id)
    .bind(item_id)
    .bind(policy_json)
    .bind(join_password_hash)
    .bind(now)
    .bind(now)
    .bind(mode)
    .bind(audio_library_id)
    .bind(web_url)
    .execute(&mut *tx)
    .await?;

    for member in members {
        sqlx::query(
            "INSERT INTO watch_party_member \
             (room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&room_id)
        .bind(&member.user_id)
        .bind(&member.role)
        .bind(&member.status)
        .bind(&member.invited_by)
        .bind(member.invited_ts)
        .bind(member.joined_ts)
        .bind(member.joined_ts)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(WatchPartyRoomRow {
        id: room_id,
        room_name: name.to_string(),
        host_user_id: host_user_id.to_string(),
        item_id: item_id.unwrap_or_default().to_string(),
        status: "lobby".to_string(),
        policy_json: policy_json.to_string(),
        join_password_hash: join_password_hash.map(str::to_string),
        created_ts: now,
        updated_ts: now,
        room_mode: mode.to_string(),
        audio_library_id: audio_library_id.map(str::to_string),
        youtube_video_id: None,
        web_url: web_url.map(str::to_string),
    })
}

pub async fn get_room(
    pool: &SqlitePool,
    room_id: &str,
) -> Result<Option<WatchPartyRoomRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> =
        sqlx::query_as(
            "SELECT id, COALESCE(room_name, ''), host_user_id, COALESCE(item_id, ''), status, policy_json, join_password_hash, created_ts, updated_ts, room_mode, audio_library_id, youtube_video_id, web_url \
             FROM watch_party_room WHERE id = ?",
        )
        .bind(room_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(
        |(
            id,
            room_name,
            host_user_id,
            item_id,
            status,
            policy_json,
            join_password_hash,
            created_ts,
            updated_ts,
            room_mode,
            audio_library_id,
            youtube_video_id,
            web_url,
        )| {
            WatchPartyRoomRow {
                id,
                room_name,
                host_user_id,
                item_id,
                status,
                policy_json,
                join_password_hash,
                created_ts,
                updated_ts,
                room_mode,
                audio_library_id,
                youtube_video_id,
                web_url,
            }
        },
    ))
}

pub async fn list_members(
    pool: &SqlitePool,
    room_id: &str,
) -> Result<Vec<WatchPartyMemberRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts \
             FROM watch_party_member WHERE room_id = ? ORDER BY invited_ts ASC, user_id ASC",
    )
    .bind(room_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts)| {
                WatchPartyMemberRow {
                    room_id,
                    user_id,
                    role,
                    status,
                    invited_by,
                    invited_ts,
                    joined_ts,
                    last_seen_ts,
                }
            },
        )
        .collect())
}

pub async fn get_member(
    pool: &SqlitePool,
    room_id: &str,
    user_id: &str,
) -> Result<Option<WatchPartyMemberRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts \
             FROM watch_party_member WHERE room_id = ? AND user_id = ?",
    )
    .bind(room_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts)| {
            WatchPartyMemberRow {
                room_id,
                user_id,
                role,
                status,
                invited_by,
                invited_ts,
                joined_ts,
                last_seen_ts,
            }
        },
    ))
}

pub async fn upsert_member(
    pool: &SqlitePool,
    room_id: &str,
    member: &NewWatchPartyMember,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO watch_party_member \
         (room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(room_id, user_id) DO UPDATE SET \
           role = excluded.role, \
           status = excluded.status, \
           invited_by = excluded.invited_by, \
           invited_ts = COALESCE(excluded.invited_ts, watch_party_member.invited_ts), \
           joined_ts = COALESCE(excluded.joined_ts, watch_party_member.joined_ts), \
           last_seen_ts = COALESCE(excluded.joined_ts, watch_party_member.last_seen_ts)",
    )
    .bind(room_id)
    .bind(&member.user_id)
    .bind(&member.role)
    .bind(&member.status)
    .bind(&member.invited_by)
    .bind(member.invited_ts)
    .bind(member.joined_ts)
    .bind(member.joined_ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_member_status(
    pool: &SqlitePool,
    room_id: &str,
    user_id: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_member \
         SET status = ?, \
             joined_ts = CASE WHEN ? = 'joined' THEN COALESCE(joined_ts, ?) ELSE joined_ts END, \
             last_seen_ts = ? \
         WHERE room_id = ? AND user_id = ?",
    )
    .bind(status)
    .bind(status)
    .bind(now)
    .bind(now)
    .bind(room_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_room_status(
    pool: &SqlitePool,
    room_id: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query("UPDATE watch_party_room SET status = ?, updated_ts = ? WHERE id = ?")
        .bind(status)
        .bind(now)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn touch_room_updated(pool: &SqlitePool, room_id: &str) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query("UPDATE watch_party_room SET updated_ts = ? WHERE id = ?")
        .bind(now)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_lobby_room_ids_updated_before(
    pool: &SqlitePool,
    max_updated_ts: i64,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM watch_party_room WHERE status = 'lobby' AND updated_ts <= ?",
    )
    .bind(max_updated_ts)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn touch_member_last_seen(
    pool: &SqlitePool,
    room_id: &str,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_member SET last_seen_ts = ? WHERE room_id = ? AND user_id = ?",
    )
    .bind(now)
    .bind(room_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_invites_for_user(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<WatchPartyInviteSummary>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, i64, String, String)> = sqlx::query_as(
        "SELECT m.room_id, COALESCE(r.item_id, ''), COALESCE(NULLIF(r.room_name, ''), COALESCE(i.title, r.room_mode)), r.host_user_id, host.username, r.created_ts, \
                CASE WHEN r.join_password_hash IS NULL OR r.join_password_hash = '' THEN 0 ELSE 1 END AS password_required, \
                m.role, m.status \
         FROM watch_party_member m \
         JOIN watch_party_room r ON r.id = m.room_id \
         LEFT JOIN item i ON i.id = r.item_id \
         JOIN user host ON host.id = r.host_user_id \
         WHERE m.user_id = ? AND m.status = 'invited' AND r.status = 'lobby' \
         ORDER BY r.created_ts DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                room_id,
                item_id,
                item_title,
                host_user_id,
                host_username,
                created_ts,
                password_required,
                role,
                status,
            )| {
                WatchPartyInviteSummary {
                    room_id,
                    item_id,
                    item_title,
                    host_user_id,
                    host_username,
                    created_ts,
                    password_required: password_required != 0,
                    role,
                    status,
                }
            },
        )
        .collect())
}

#[derive(Debug, Clone)]
pub struct PublicRoomRow {
    pub id: String,
    pub room_name: String,
    pub host_user_id: String,
    pub host_username: String,
    pub item_id: String,
    pub item_title: String,
    pub room_mode: String,
    pub audio_library_name: String,
    pub web_url: String,
    pub password_required: bool,
    pub member_count: i64,
    pub created_ts: i64,
}

/// List all non-invite-only rooms that are currently in the lobby.
pub async fn list_public_rooms(pool: &SqlitePool) -> Result<Vec<PublicRoomRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
    )> =
        sqlx::query_as(
            "SELECT r.id, COALESCE(r.room_name, ''), r.host_user_id, host.username, COALESCE(r.item_id, ''), \
                    COALESCE(i.title, ''), r.room_mode, COALESCE(lib.name, ''), COALESCE(r.web_url, ''), \
                    CASE WHEN r.join_password_hash IS NOT NULL AND r.join_password_hash != '' THEN 1 ELSE 0 END, \
                    COUNT(CASE WHEN m.status = 'joined' THEN 1 END), \
                    r.created_ts \
             FROM watch_party_room r \
             JOIN user host ON host.id = r.host_user_id \
             LEFT JOIN item i ON i.id = r.item_id \
             LEFT JOIN library lib ON lib.id = r.audio_library_id \
             LEFT JOIN watch_party_member m ON m.room_id = r.id \
             WHERE r.status = 'lobby' \
               AND (json_extract(r.policy_json, '$.invite_only') = 0 \
                    OR json_extract(r.policy_json, '$.invite_only') IS NULL) \
             GROUP BY r.id \
             ORDER BY r.created_ts DESC",
        )
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                room_name,
                host_user_id,
                host_username,
                item_id,
                item_title,
                room_mode,
                audio_library_name,
                web_url,
                password_required,
                member_count,
                created_ts,
            )| {
                PublicRoomRow {
                    id,
                    room_name,
                    host_user_id,
                    host_username,
                    item_id,
                    item_title,
                    room_mode,
                    audio_library_name,
                    web_url,
                    password_required: password_required != 0,
                    member_count,
                    created_ts,
                }
            },
        )
        .collect())
}

/// Upsert the audio queue for a room.
pub async fn upsert_audio_queue(
    pool: &SqlitePool,
    room_id: &str,
    track_ids_json: &str,
    current_index: usize,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO watch_party_audio_queue (room_id, track_ids_json, current_index, updated_ts) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(room_id) DO UPDATE SET \
           track_ids_json = excluded.track_ids_json, \
           current_index = excluded.current_index, \
           updated_ts = excluded.updated_ts",
    )
    .bind(room_id)
    .bind(track_ids_json)
    .bind(current_index as i64)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get the audio queue for a room. Returns (track_ids, current_index) or None.
pub async fn get_audio_queue(
    pool: &SqlitePool,
    room_id: &str,
) -> Result<Option<(Vec<String>, usize)>, sqlx::Error> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT track_ids_json, current_index FROM watch_party_audio_queue WHERE room_id = ?",
    )
    .bind(room_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(|(json, idx)| {
        serde_json::from_str::<Vec<String>>(&json)
            .ok()
            .map(|ids| (ids, idx as usize))
    }))
}

/// Get all track items from a music library, optionally filtered by search query.
pub async fn get_library_tracks(
    pool: &SqlitePool,
    library_id: &str,
    query: Option<&str>,
) -> Result<Vec<AudioTrackRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT t.id, t.title, \
                    album.title, \
                    artist.title, \
                    album.poster_url, \
                    mf.duration_ms \
             FROM item t \
             LEFT JOIN item album ON album.id = t.parent_id \
             LEFT JOIN item artist ON artist.id = album.parent_id \
             LEFT JOIN episode_file_map efm ON efm.episode_item_id = t.id \
             LEFT JOIN media_file mf ON mf.id = efm.file_id \
             WHERE t.library_id = ? AND t.kind = 'track' \
             ORDER BY artist.title NULLS LAST, album.title NULLS LAST, t.title",
    )
    .bind(library_id)
    .fetch_all(pool)
    .await?;

    let tracks: Vec<AudioTrackRow> = rows
        .into_iter()
        .map(
            |(id, title, album, artist, album_art_url, duration_ms)| AudioTrackRow {
                id,
                title,
                album: album.unwrap_or_default(),
                artist: artist.unwrap_or_default(),
                album_art_url,
                duration_ms: duration_ms.map(|ms| ms as u64),
            },
        )
        .collect();

    if let Some(q) = query.filter(|s| !s.trim().is_empty()) {
        let lower = q.to_ascii_lowercase();
        Ok(tracks
            .into_iter()
            .filter(|t| {
                t.title.to_ascii_lowercase().contains(&lower)
                    || t.artist.to_ascii_lowercase().contains(&lower)
                    || t.album.to_ascii_lowercase().contains(&lower)
            })
            .collect())
    } else {
        Ok(tracks)
    }
}

#[derive(Debug, Clone)]
pub struct AudioTrackRow {
    pub id: String,
    pub title: String,
    pub album: String,
    pub artist: String,
    pub album_art_url: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Persist the current YouTube video ID for a room.
pub async fn update_youtube_video_id(
    pool: &SqlitePool,
    room_id: &str,
    video_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE watch_party_room SET youtube_video_id = ? WHERE id = ?")
        .bind(video_id)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_web_url(
    pool: &SqlitePool,
    room_id: &str,
    web_url: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE watch_party_room SET web_url = ? WHERE id = ?")
        .bind(web_url)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn reconfigure_room_mode(
    pool: &SqlitePool,
    room_id: &str,
    room_mode: &str,
    item_id: Option<&str>,
    audio_library_id: Option<&str>,
    youtube_video_id: Option<&str>,
    web_url: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_room \
         SET room_mode = ?, item_id = ?, audio_library_id = ?, youtube_video_id = ?, web_url = ?, updated_ts = ? \
         WHERE id = ?",
    )
    .bind(room_mode)
    .bind(item_id)
    .bind(audio_library_id)
    .bind(youtube_video_id)
    .bind(web_url)
    .bind(now)
    .bind(room_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn clear_audio_queue(pool: &SqlitePool, room_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watch_party_audio_queue WHERE room_id = ?")
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}
