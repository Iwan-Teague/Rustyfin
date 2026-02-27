use crate::DbPool;

fn db_int_to_bool(v: i64) -> bool {
    v != 0
}

/// We store playback sessions in memory for now (they're ephemeral).
/// Progress is persisted via user_item_state.

pub async fn update_progress(
    pool: &DbPool,
    user_id: &str,
    item_id: &str,
    progress_ms: i64,
    played: bool,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO user_item_state (user_id, item_id, played, progress_ms, last_played_ts) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT(user_id, item_id) DO UPDATE SET \
         played = excluded.played, progress_ms = excluded.progress_ms, \
         last_played_ts = excluded.last_played_ts",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(played as i32)
    .bind(progress_ms)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PlayStateRow {
    pub user_id: String,
    pub item_id: String,
    pub played: bool,
    pub progress_ms: i64,
    pub last_played_ts: Option<i64>,
    pub favorite: bool,
}

pub async fn get_play_state(
    pool: &DbPool,
    user_id: &str,
    item_id: &str,
) -> Result<Option<PlayStateRow>, sqlx::Error> {
    let row: Option<(String, String, i64, i64, Option<i64>, i64)> = sqlx::query_as(
        "SELECT user_id, item_id, played, progress_ms, last_played_ts, favorite \
         FROM user_item_state WHERE user_id = $1 AND item_id = $2",
    )
    .bind(user_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| PlayStateRow {
        user_id: r.0,
        item_id: r.1,
        played: db_int_to_bool(r.2),
        progress_ms: r.3,
        last_played_ts: r.4,
        favorite: db_int_to_bool(r.5),
    }))
}
