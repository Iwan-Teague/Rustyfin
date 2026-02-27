use crate::DbPool;

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
    pub audio_source: String,
    pub audio_library_id: Option<String>,
    pub youtube_video_id: Option<String>,
    pub web_url: Option<String>,
    pub create_tool: String,
    pub create_document_name: String,
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
pub struct WatchPartyMemberWithUsernameRow {
    pub room_id: String,
    pub user_id: String,
    pub role: String,
    pub status: String,
    pub invited_by: Option<String>,
    pub invited_ts: Option<i64>,
    pub joined_ts: Option<i64>,
    pub last_seen_ts: Option<i64>,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct WatchPartyCreateStateRow {
    pub room_id: String,
    pub active_tool: String,
    pub document_name: String,
    pub text_format: String,
    pub text_content: String,
    pub canvas_strokes_json: String,
    pub updated_ts: i64,
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

#[allow(clippy::too_many_arguments)]
pub async fn create_room_with_members(
    pool: &DbPool,
    host_user_id: &str,
    room_name: Option<&str>,
    item_id: Option<&str>,
    policy_json: &str,
    invite_only: bool,
    join_password_hash: Option<&str>,
    members: &[NewWatchPartyMember],
    room_mode: Option<&str>,
    audio_source: Option<&str>,
    audio_library_id: Option<&str>,
    web_url: Option<&str>,
    create_tool: Option<&str>,
    create_document_name: Option<&str>,
) -> Result<WatchPartyRoomRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let room_id = uuid::Uuid::new_v4().to_string();
    let mode = room_mode.unwrap_or("video");
    let source = audio_source.unwrap_or("library");
    let name = room_name.unwrap_or("").trim();
    let tool = create_tool.unwrap_or("text");
    let document_name = create_document_name.unwrap_or("Untitled Document").trim();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO watch_party_room \
         (id, room_name, host_user_id, item_id, status, policy_json, invite_only, join_password_hash, created_ts, updated_ts, room_mode, audio_source, audio_library_id, web_url, create_tool, create_document_name) \
         VALUES ($1, $2, $3, $4, 'lobby', $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(&room_id)
    .bind(name)
    .bind(host_user_id)
    .bind(item_id)
    .bind(policy_json)
    .bind(if invite_only { 1_i64 } else { 0_i64 })
    .bind(join_password_hash)
    .bind(now)
    .bind(now)
    .bind(mode)
    .bind(source)
    .bind(audio_library_id)
    .bind(web_url)
    .bind(tool)
    .bind(document_name)
    .execute(&mut *tx)
    .await?;

    for member in members {
        sqlx::query(
            "INSERT INTO watch_party_member \
             (room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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
        audio_source: source.to_string(),
        audio_library_id: audio_library_id.map(str::to_string),
        youtube_video_id: None,
        web_url: web_url.map(str::to_string),
        create_tool: tool.to_string(),
        create_document_name: document_name.to_string(),
    })
}

pub async fn get_room(
    pool: &DbPool,
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
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
    )> =
        sqlx::query_as(
            "SELECT id, COALESCE(room_name, ''), host_user_id, COALESCE(item_id, ''), status, policy_json, join_password_hash, created_ts, updated_ts, room_mode, COALESCE(audio_source, 'library'), audio_library_id, youtube_video_id, web_url, COALESCE(create_tool, 'text'), COALESCE(create_document_name, 'Untitled Document') \
             FROM watch_party_room WHERE id = $1",
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
            audio_source,
            audio_library_id,
            youtube_video_id,
            web_url,
            create_tool,
            create_document_name,
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
                audio_source,
                audio_library_id,
                youtube_video_id,
                web_url,
                create_tool,
                create_document_name,
            }
        },
    ))
}

pub async fn list_members(
    pool: &DbPool,
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
             FROM watch_party_member WHERE room_id = $1 ORDER BY invited_ts ASC, user_id ASC",
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

pub async fn list_members_with_usernames(
    pool: &DbPool,
    room_id: &str,
) -> Result<Vec<WatchPartyMemberWithUsernameRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
    )> = sqlx::query_as(
        "SELECT m.room_id, m.user_id, m.role, m.status, m.invited_by, m.invited_ts, m.joined_ts, m.last_seen_ts, u.username \
         FROM watch_party_member m \
         JOIN \"user\" u ON u.id = m.user_id \
         WHERE m.room_id = $1 \
         ORDER BY m.invited_ts ASC, m.user_id ASC",
    )
    .bind(room_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                room_id,
                user_id,
                role,
                status,
                invited_by,
                invited_ts,
                joined_ts,
                last_seen_ts,
                username,
            )| WatchPartyMemberWithUsernameRow {
                room_id,
                user_id,
                role,
                status,
                invited_by,
                invited_ts,
                joined_ts,
                last_seen_ts,
                username,
            },
        )
        .collect())
}

pub async fn get_member(
    pool: &DbPool,
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
             FROM watch_party_member WHERE room_id = $1 AND user_id = $2",
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
    pool: &DbPool,
    room_id: &str,
    member: &NewWatchPartyMember,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO watch_party_member \
         (room_id, user_id, role, status, invited_by, invited_ts, joined_ts, last_seen_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
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
    pool: &DbPool,
    room_id: &str,
    user_id: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_member \
         SET status = $1, \
             joined_ts = CASE WHEN $2 = 'joined' THEN COALESCE(joined_ts, $3) ELSE joined_ts END, \
             last_seen_ts = $4 \
         WHERE room_id = $5 AND user_id = $6",
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
    pool: &DbPool,
    room_id: &str,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result =
        sqlx::query("UPDATE watch_party_room SET status = $1, updated_ts = $2 WHERE id = $3")
            .bind(status)
            .bind(now)
            .bind(room_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn touch_room_updated(pool: &DbPool, room_id: &str) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query("UPDATE watch_party_room SET updated_ts = $1 WHERE id = $2")
        .bind(now)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_purgeable_room_ids_updated_before(
    pool: &DbPool,
    max_updated_ts: i64,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id \
         FROM watch_party_room \
         WHERE status IN ('lobby', 'ended') \
           AND updated_ts <= $1",
    )
    .bind(max_updated_ts)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn touch_member_last_seen(
    pool: &DbPool,
    room_id: &str,
    user_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_member SET last_seen_ts = $1 WHERE room_id = $2 AND user_id = $3",
    )
    .bind(now)
    .bind(room_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_invites_for_user(
    pool: &DbPool,
    user_id: &str,
) -> Result<Vec<WatchPartyInviteSummary>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, i64, i64, String, String)> = sqlx::query_as(
        "SELECT m.room_id, COALESCE(r.item_id, ''), COALESCE(NULLIF(r.room_name, ''), COALESCE(i.title, r.room_mode)), r.host_user_id, host.username, r.created_ts, \
                CASE WHEN r.join_password_hash IS NULL OR r.join_password_hash = '' THEN 0 ELSE 1 END AS password_required, \
                m.role, m.status \
         FROM watch_party_member m \
         JOIN watch_party_room r ON r.id = m.room_id \
         LEFT JOIN item i ON i.id = r.item_id \
         JOIN \"user\" host ON host.id = r.host_user_id \
         WHERE m.user_id = $1 AND m.status = 'invited' AND r.status = 'lobby' \
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
    pub audio_source: String,
    pub audio_library_name: String,
    pub web_url: String,
    pub password_required: bool,
    pub member_count: i64,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct AdminRoomRow {
    pub id: String,
    pub room_name: String,
    pub host_user_id: String,
    pub host_username: String,
    pub item_id: String,
    pub item_title: String,
    pub room_mode: String,
    pub audio_source: String,
    pub audio_library_name: String,
    pub web_url: String,
    pub password_required: bool,
    pub invite_only: bool,
    pub member_count: i64,
    pub status: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

/// List all non-invite-only rooms that are currently in the lobby.
pub async fn list_public_rooms(pool: &DbPool) -> Result<Vec<PublicRoomRow>, sqlx::Error> {
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
        String,
        i64,
        i64,
        i64,
    )> =
        sqlx::query_as(
            "SELECT r.id, COALESCE(r.room_name, ''), r.host_user_id, host.username, COALESCE(r.item_id, ''), \
                    COALESCE(i.title, ''), r.room_mode, COALESCE(r.audio_source, 'library'), COALESCE(lib.name, ''), COALESCE(r.web_url, ''), \
                    CASE WHEN r.join_password_hash IS NOT NULL AND r.join_password_hash != '' THEN 1 ELSE 0 END, \
                    COUNT(CASE WHEN m.status = 'joined' THEN 1 END), \
                    r.created_ts \
             FROM watch_party_room r \
             JOIN \"user\" host ON host.id = r.host_user_id \
             LEFT JOIN item i ON i.id = r.item_id \
             LEFT JOIN library lib ON lib.id = r.audio_library_id \
             LEFT JOIN watch_party_member m ON m.room_id = r.id \
             WHERE r.status = 'lobby' \
               AND r.invite_only = 0 \
             GROUP BY r.id, r.room_name, r.host_user_id, host.username, r.item_id, i.title, \
                      r.room_mode, r.audio_source, lib.name, r.web_url, r.join_password_hash, r.created_ts \
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
                audio_source,
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
                    audio_source,
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

pub async fn list_admin_rooms(pool: &DbPool) -> Result<Vec<AdminRoomRow>, sqlx::Error> {
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
        String,
        i64,
        i64,
        i64,
        String,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT r.id, COALESCE(r.room_name, ''), r.host_user_id, host.username, COALESCE(r.item_id, ''), \
                COALESCE(i.title, ''), r.room_mode, COALESCE(r.audio_source, 'library'), COALESCE(lib.name, ''), COALESCE(r.web_url, ''), \
                CASE WHEN r.join_password_hash IS NOT NULL AND r.join_password_hash != '' THEN 1 ELSE 0 END, \
                r.invite_only, \
                COUNT(CASE WHEN m.status = 'joined' THEN 1 END), \
                r.status, r.created_ts, r.updated_ts \
         FROM watch_party_room r \
         JOIN \"user\" host ON host.id = r.host_user_id \
         LEFT JOIN item i ON i.id = r.item_id \
         LEFT JOIN library lib ON lib.id = r.audio_library_id \
         LEFT JOIN watch_party_member m ON m.room_id = r.id \
         GROUP BY r.id, r.room_name, r.host_user_id, host.username, r.item_id, i.title, \
                  r.room_mode, r.audio_source, lib.name, r.web_url, r.join_password_hash, r.invite_only, \
                  r.status, r.created_ts, r.updated_ts \
         ORDER BY CASE WHEN r.status = 'lobby' THEN 0 ELSE 1 END, r.updated_ts DESC, r.created_ts DESC",
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
                audio_source,
                audio_library_name,
                web_url,
                password_required,
                invite_only,
                member_count,
                status,
                created_ts,
                updated_ts,
            )| AdminRoomRow {
                id,
                room_name,
                host_user_id,
                host_username,
                item_id,
                item_title,
                room_mode,
                audio_source,
                audio_library_name,
                web_url,
                password_required: password_required != 0,
                invite_only: invite_only != 0,
                member_count,
                status,
                created_ts,
                updated_ts,
            },
        )
        .collect())
}

pub async fn update_room_name(
    pool: &DbPool,
    room_id: &str,
    room_name: &str,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result =
        sqlx::query("UPDATE watch_party_room SET room_name = $1, updated_ts = $2 WHERE id = $3")
            .bind(room_name)
            .bind(now)
            .bind(room_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_room(pool: &DbPool, room_id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query("DELETE FROM watch_party_online_audio_track_fts WHERE room_id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;

    let result = sqlx::query("DELETE FROM watch_party_room WHERE id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Upsert the audio queue for a room.
pub async fn upsert_audio_queue(
    pool: &DbPool,
    room_id: &str,
    track_ids_json: &str,
    current_index: usize,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO watch_party_audio_queue (room_id, track_ids_json, current_index, updated_ts) \
         VALUES ($1, $2, $3, $4) \
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
    pool: &DbPool,
    room_id: &str,
) -> Result<Option<(Vec<String>, usize)>, sqlx::Error> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT track_ids_json, current_index FROM watch_party_audio_queue WHERE room_id = $1",
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
    pool: &DbPool,
    library_id: &str,
    query: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<AudioTrackRow>, sqlx::Error> {
    let search = query
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|q| format!("%{}%", q.to_lowercase()));
    let limit = limit.max(1);

    let mut sql = String::from(
        "SELECT t.id, t.title, \
                    album.title, \
                    artist.title, \
                    album.poster_url, \
                    ( \
                        SELECT mf.duration_ms \
                        FROM episode_file_map efm \
                        JOIN media_file mf ON mf.id = efm.file_id \
                        WHERE efm.episode_item_id = t.id \
                        ORDER BY efm.created_ts ASC \
                        LIMIT 1 \
                    ) AS duration_ms \
             FROM item t \
             LEFT JOIN item album ON album.id = t.parent_id \
             LEFT JOIN item artist ON artist.id = album.parent_id \
             WHERE t.library_id = $1 AND t.kind = 'track'",
    );
    let mut next_param = 2;
    if search.is_some() {
        let p1 = next_param;
        let p2 = next_param + 1;
        let p3 = next_param + 2;
        next_param += 3;
        sql.push_str(&format!(
            " AND ( \
                 LOWER(t.title) LIKE ${p1} \
              OR LOWER(COALESCE(artist.title, '')) LIKE ${p2} \
              OR LOWER(COALESCE(album.title, '')) LIKE ${p3} \
            )"
        ));
    }
    let limit_param = next_param;
    let offset_param = next_param + 1;
    sql.push_str(&format!(
        " ORDER BY artist.title NULLS LAST, album.title NULLS LAST, t.title \
          LIMIT ${limit_param} OFFSET ${offset_param}"
    ));

    let mut query_builder = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        ),
    >(&sql)
    .bind(library_id);

    if let Some(search) = search {
        query_builder = query_builder
            .bind(search.clone())
            .bind(search.clone())
            .bind(search);
    }

    let rows = query_builder
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await?;

    Ok(rows
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
        .collect())
}

/// Get selected track items from a music library by item IDs.
pub async fn get_library_tracks_by_item_ids(
    pool: &DbPool,
    library_id: &str,
    item_ids: &[String],
) -> Result<Vec<AudioTrackRow>, sqlx::Error> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(2, item_ids.len());
    let sql = format!(
        "SELECT t.id, t.title, \
                    album.title, \
                    artist.title, \
                    album.poster_url, \
                    ( \
                        SELECT mf.duration_ms \
                        FROM episode_file_map efm \
                        JOIN media_file mf ON mf.id = efm.file_id \
                        WHERE efm.episode_item_id = t.id \
                        ORDER BY efm.created_ts ASC \
                        LIMIT 1 \
                    ) AS duration_ms \
             FROM item t \
             LEFT JOIN item album ON album.id = t.parent_id \
             LEFT JOIN item artist ON artist.id = album.parent_id \
             WHERE t.library_id = $1 AND t.kind = 'track' AND t.id IN ({placeholders})"
    );

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        ),
    >(&sql)
    .bind(library_id);
    for item_id in item_ids {
        query = query.bind(item_id);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
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
        .collect())
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

#[derive(Debug, Clone)]
pub struct OnlineAudioTrackRow {
    pub id: String,
    pub room_id: String,
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: Option<String>,
    pub file_path: String,
    pub duration_ms: Option<u64>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type OnlineAudioTrackTuple = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<i64>,
    i64,
    i64,
);

fn map_online_audio_tuple(
    (
        id,
        room_id,
        video_id,
        title,
        channel,
        thumbnail_url,
        file_path,
        duration_ms,
        created_ts,
        updated_ts,
    ): OnlineAudioTrackTuple,
) -> OnlineAudioTrackRow {
    OnlineAudioTrackRow {
        id,
        room_id,
        video_id,
        title,
        channel,
        thumbnail_url,
        file_path,
        duration_ms: duration_ms.map(|v| v as u64),
        created_ts,
        updated_ts,
    }
}

fn search_tokens(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|term| {
            term.chars()
                .filter(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|term| !term.is_empty())
        .collect()
}

fn build_fts_prefix_query(raw: &str) -> Option<String> {
    let tokens = search_tokens(raw);
    if tokens.is_empty() {
        return None;
    }

    Some(
        tokens
            .into_iter()
            .map(|term| format!("{term}*"))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

fn build_pg_prefix_tsquery(raw: &str) -> Option<String> {
    let tokens = search_tokens(raw);
    if tokens.is_empty() {
        return None;
    }

    Some(
        tokens
            .into_iter()
            .map(|term| format!("{term}:*"))
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_online_audio_track(
    pool: &DbPool,
    room_id: &str,
    track_id: &str,
    video_id: &str,
    title: &str,
    channel: &str,
    thumbnail_url: Option<&str>,
    file_path: &str,
    duration_ms: Option<u64>,
) -> Result<OnlineAudioTrackRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO watch_party_online_audio_track \
         (id, room_id, video_id, title, channel, thumbnail_url, file_path, duration_ms, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT(room_id, video_id) DO UPDATE SET \
            title = excluded.title, \
            channel = excluded.channel, \
            thumbnail_url = excluded.thumbnail_url, \
            file_path = excluded.file_path, \
            duration_ms = excluded.duration_ms, \
            updated_ts = excluded.updated_ts",
    )
    .bind(track_id)
    .bind(room_id)
    .bind(video_id)
    .bind(title)
    .bind(channel)
    .bind(thumbnail_url)
    .bind(file_path)
    .bind(duration_ms.map(|v| v as i64))
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query(
        "DELETE FROM watch_party_online_audio_track_fts \
         WHERE track_id = $1 AND room_id = $2",
    )
    .bind(track_id)
    .bind(room_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO watch_party_online_audio_track_fts \
         (track_id, room_id, title, channel) VALUES ($1, $2, $3, $4)",
    )
    .bind(track_id)
    .bind(room_id)
    .bind(title)
    .bind(channel)
    .execute(pool)
    .await?;

    get_online_audio_track_by_video_id(pool, room_id, video_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn get_online_audio_track_by_video_id(
    pool: &DbPool,
    room_id: &str,
    video_id: &str,
) -> Result<Option<OnlineAudioTrackRow>, sqlx::Error> {
    let row: Option<OnlineAudioTrackTuple> = sqlx::query_as(
        "SELECT id, room_id, video_id, title, channel, thumbnail_url, file_path, duration_ms, created_ts, updated_ts \
         FROM watch_party_online_audio_track \
         WHERE room_id = $1 AND video_id = $2",
    )
    .bind(room_id)
    .bind(video_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_online_audio_tuple))
}

pub async fn get_online_audio_track(
    pool: &DbPool,
    room_id: &str,
    track_id: &str,
) -> Result<Option<OnlineAudioTrackRow>, sqlx::Error> {
    let row: Option<OnlineAudioTrackTuple> = sqlx::query_as(
        "SELECT id, room_id, video_id, title, channel, thumbnail_url, file_path, duration_ms, created_ts, updated_ts \
         FROM watch_party_online_audio_track \
         WHERE room_id = $1 AND id = $2",
    )
    .bind(room_id)
    .bind(track_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(map_online_audio_tuple))
}

pub async fn list_online_audio_tracks(
    pool: &DbPool,
    room_id: &str,
    query: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Vec<OnlineAudioTrackRow>, sqlx::Error> {
    let limit = limit.max(1);
    let search = query.map(str::trim).filter(|s| !s.is_empty());
    let backend = crate::active_backend().unwrap_or(crate::DatabaseBackend::Sqlite);

    let rows: Vec<OnlineAudioTrackTuple> = if let Some(search) = search {
        if backend == crate::DatabaseBackend::Postgres {
            if let Some(ts_query) = build_pg_prefix_tsquery(search) {
                sqlx::query_as(
                    "SELECT t.id, t.room_id, t.video_id, t.title, t.channel, t.thumbnail_url, t.file_path, t.duration_ms, t.created_ts, t.updated_ts \
                     FROM watch_party_online_audio_track t \
                     JOIN watch_party_online_audio_track_fts f \
                       ON f.track_id = t.id \
                      AND f.room_id = t.room_id \
                     WHERE t.room_id = $1 \
                       AND to_tsvector('simple', COALESCE(f.title, '') || ' ' || COALESCE(f.channel, '')) @@ to_tsquery('simple', $2) \
                     ORDER BY t.created_ts DESC, t.updated_ts DESC \
                     LIMIT $3 OFFSET $4",
                )
                .bind(room_id)
                .bind(ts_query)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(pool)
                .await?
            } else {
                let search = format!("%{}%", search.to_lowercase());
                sqlx::query_as(
                    "SELECT t.id, t.room_id, t.video_id, t.title, t.channel, t.thumbnail_url, t.file_path, t.duration_ms, t.created_ts, t.updated_ts \
                     FROM watch_party_online_audio_track t \
                     JOIN watch_party_online_audio_track_fts f \
                       ON f.track_id = t.id \
                      AND f.room_id = t.room_id \
                     WHERE t.room_id = $1 \
                       AND (LOWER(f.title) LIKE $2 OR LOWER(f.channel) LIKE $2) \
                     ORDER BY t.created_ts DESC, t.updated_ts DESC \
                     LIMIT $3 OFFSET $4",
                )
                .bind(room_id)
                .bind(search)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(pool)
                .await?
            }
        } else if let Some(fts_query) = build_fts_prefix_query(search) {
            sqlx::query_as(
                "SELECT t.id, t.room_id, t.video_id, t.title, t.channel, t.thumbnail_url, t.file_path, t.duration_ms, t.created_ts, t.updated_ts \
                 FROM watch_party_online_audio_track t \
                 JOIN watch_party_online_audio_track_fts \
                   ON watch_party_online_audio_track_fts.track_id = t.id \
                  AND watch_party_online_audio_track_fts.room_id = t.room_id \
                 WHERE t.room_id = $1 \
                   AND watch_party_online_audio_track_fts MATCH $2 \
                 ORDER BY t.created_ts DESC, t.updated_ts DESC \
                 LIMIT $3 OFFSET $4",
            )
            .bind(room_id)
            .bind(fts_query)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(pool)
            .await?
        } else {
            Vec::new()
        }
    } else {
        sqlx::query_as(
            "SELECT id, room_id, video_id, title, channel, thumbnail_url, file_path, duration_ms, created_ts, updated_ts \
             FROM watch_party_online_audio_track \
             WHERE room_id = $1 \
             ORDER BY created_ts DESC, updated_ts DESC \
             LIMIT $2 OFFSET $3",
        )
        .bind(room_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows.into_iter().map(map_online_audio_tuple).collect())
}

#[cfg(test)]
mod search_query_tests {
    use super::{build_fts_prefix_query, build_pg_prefix_tsquery};

    #[test]
    fn sqlite_fts_prefix_query_tokenizes_to_prefix_and() {
        let q = build_fts_prefix_query("Tory Lanez 1985").expect("query");
        assert_eq!(q, "tory* AND lanez* AND 1985*");
    }

    #[test]
    fn postgres_tsquery_tokenizes_to_prefix_and() {
        let q = build_pg_prefix_tsquery("Tory Lanez 1985").expect("query");
        assert_eq!(q, "tory:* & lanez:* & 1985:*");
    }

    #[test]
    fn search_query_builder_rejects_punctuation_only() {
        assert!(build_fts_prefix_query("!!! ...").is_none());
        assert!(build_pg_prefix_tsquery("!!! ...").is_none());
    }
}

pub async fn list_online_audio_tracks_by_ids(
    pool: &DbPool,
    room_id: &str,
    track_ids: &[String],
) -> Result<Vec<OnlineAudioTrackRow>, sqlx::Error> {
    if track_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(2, track_ids.len());
    let sql = format!(
        "SELECT id, room_id, video_id, title, channel, thumbnail_url, file_path, duration_ms, created_ts, updated_ts \
         FROM watch_party_online_audio_track \
         WHERE room_id = $1 AND id IN ({placeholders})"
    );

    let mut query = sqlx::query_as::<_, OnlineAudioTrackTuple>(&sql).bind(room_id);
    for track_id in track_ids {
        query = query.bind(track_id);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(map_online_audio_tuple).collect())
}

pub async fn clear_online_audio_tracks(pool: &DbPool, room_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watch_party_online_audio_track_fts WHERE room_id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM watch_party_online_audio_track WHERE room_id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Persist the current YouTube video ID for a room.
pub async fn update_youtube_video_id(
    pool: &DbPool,
    room_id: &str,
    video_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE watch_party_room SET youtube_video_id = $1 WHERE id = $2")
        .bind(video_id)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_web_url(
    pool: &DbPool,
    room_id: &str,
    web_url: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE watch_party_room SET web_url = $1 WHERE id = $2")
        .bind(web_url)
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_create_state(
    pool: &DbPool,
    room_id: &str,
) -> Result<Option<WatchPartyCreateStateRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, String, String, i64)> = sqlx::query_as(
        "SELECT room_id, active_tool, document_name, text_format, text_content, canvas_strokes_json, updated_ts \
         FROM watch_party_create_state WHERE room_id = $1",
    )
    .bind(room_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            room_id,
            active_tool,
            document_name,
            text_format,
            text_content,
            canvas_strokes_json,
            updated_ts,
        )| WatchPartyCreateStateRow {
            room_id,
            active_tool,
            document_name,
            text_format,
            text_content,
            canvas_strokes_json,
            updated_ts,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_create_state(
    pool: &DbPool,
    room_id: &str,
    active_tool: &str,
    document_name: &str,
    text_format: &str,
    text_content: &str,
    canvas_strokes_json: &str,
    updated_ts: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO watch_party_create_state \
         (room_id, active_tool, document_name, text_format, text_content, canvas_strokes_json, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT(room_id) DO UPDATE SET \
           active_tool = excluded.active_tool, \
           document_name = excluded.document_name, \
           text_format = excluded.text_format, \
           text_content = excluded.text_content, \
           canvas_strokes_json = excluded.canvas_strokes_json, \
           updated_ts = excluded.updated_ts",
    )
    .bind(room_id)
    .bind(active_tool)
    .bind(document_name)
    .bind(text_format)
    .bind(text_content)
    .bind(canvas_strokes_json)
    .bind(updated_ts)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn reconfigure_room_mode(
    pool: &DbPool,
    room_id: &str,
    room_mode: &str,
    audio_source: &str,
    item_id: Option<&str>,
    audio_library_id: Option<&str>,
    youtube_video_id: Option<&str>,
    web_url: Option<&str>,
    create_tool: Option<&str>,
    create_document_name: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE watch_party_room \
         SET room_mode = $1, audio_source = $2, item_id = $3, audio_library_id = $4, youtube_video_id = $5, web_url = $6, create_tool = COALESCE($7, create_tool), create_document_name = COALESCE($8, create_document_name), updated_ts = $9 \
         WHERE id = $10",
    )
    .bind(room_mode)
    .bind(audio_source)
    .bind(item_id)
    .bind(audio_library_id)
    .bind(youtube_video_id)
    .bind(web_url)
    .bind(create_tool)
    .bind(create_document_name)
    .bind(now)
    .bind(room_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn clear_audio_queue(pool: &DbPool, room_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM watch_party_audio_queue WHERE room_id = $1")
        .bind(room_id)
        .execute(pool)
        .await?;
    Ok(())
}
