use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct WatchPartyRoomRow {
    pub id: String,
    pub host_user_id: String,
    pub item_id: String,
    pub status: String,
    pub policy_json: String,
    pub join_password_hash: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
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
    item_id: &str,
    policy_json: &str,
    join_password_hash: Option<&str>,
    members: &[NewWatchPartyMember],
) -> Result<WatchPartyRoomRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let room_id = uuid::Uuid::new_v4().to_string();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO watch_party_room \
         (id, host_user_id, item_id, status, policy_json, join_password_hash, created_ts, updated_ts) \
         VALUES (?, ?, ?, 'lobby', ?, ?, ?, ?)",
    )
    .bind(&room_id)
    .bind(host_user_id)
    .bind(item_id)
    .bind(policy_json)
    .bind(join_password_hash)
    .bind(now)
    .bind(now)
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
        host_user_id: host_user_id.to_string(),
        item_id: item_id.to_string(),
        status: "lobby".to_string(),
        policy_json: policy_json.to_string(),
        join_password_hash: join_password_hash.map(str::to_string),
        created_ts: now,
        updated_ts: now,
    })
}

pub async fn get_room(
    pool: &SqlitePool,
    room_id: &str,
) -> Result<Option<WatchPartyRoomRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, String, Option<String>, i64, i64)> =
        sqlx::query_as(
            "SELECT id, host_user_id, item_id, status, policy_json, join_password_hash, created_ts, updated_ts \
             FROM watch_party_room WHERE id = ?",
        )
        .bind(room_id)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(
        |(
            id,
            host_user_id,
            item_id,
            status,
            policy_json,
            join_password_hash,
            created_ts,
            updated_ts,
        )| {
            WatchPartyRoomRow {
                id,
                host_user_id,
                item_id,
                status,
                policy_json,
                join_password_hash,
                created_ts,
                updated_ts,
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
        "SELECT m.room_id, r.item_id, i.title, r.host_user_id, host.username, r.created_ts, \
                CASE WHEN r.join_password_hash IS NULL OR r.join_password_hash = '' THEN 0 ELSE 1 END AS password_required, \
                m.role, m.status \
         FROM watch_party_member m \
         JOIN watch_party_room r ON r.id = m.room_id \
         JOIN item i ON i.id = r.item_id \
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
