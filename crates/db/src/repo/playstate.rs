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

#[derive(Debug, Clone)]
pub struct ContinueWatchingRow {
    pub item_id: String,
    pub library_id: String,
    pub kind: String,
    pub title: String,
    pub year: Option<i64>,
    pub poster_url: Option<String>,
    pub progress_ms: i64,
    pub duration_ms: Option<i64>,
    pub last_played_ts: i64,
}

pub async fn list_continue_watching(
    pool: &DbPool,
    user_id: &str,
    allowed_library_ids: Option<&[String]>,
    limit: i64,
) -> Result<Vec<ContinueWatchingRow>, sqlx::Error> {
    if matches!(allowed_library_ids, Some(ids) if ids.is_empty()) {
        return Ok(Vec::new());
    }

    let mut sql = String::from(
        "SELECT i.id, i.library_id, i.kind, i.title, i.year, i.poster_url, \
                CAST(uis.progress_ms AS BIGINT) AS progress_ms, \
                (SELECT mf.duration_ms FROM episode_file_map efm \
                 JOIN media_file mf ON mf.id = efm.file_id \
                 WHERE efm.episode_item_id = i.id LIMIT 1) AS duration_ms, \
                COALESCE(CAST(uis.last_played_ts AS BIGINT), 0) AS last_played_ts \
         FROM user_item_state uis \
         JOIN item i ON i.id = uis.item_id \
         WHERE uis.user_id = $1 \
           AND uis.played = 0 \
           AND uis.progress_ms > 0 \
           AND i.kind IN ('movie', 'episode')",
    );

    let mut next_param = 2;
    if let Some(library_ids) = allowed_library_ids {
        let placeholders = crate::repo::dollar_placeholders(next_param, library_ids.len());
        sql.push_str(&format!(" AND i.library_id IN ({placeholders})"));
        next_param += library_ids.len();
    }

    sql.push_str(&format!(
        " ORDER BY COALESCE(uis.last_played_ts, 0) DESC, i.title ASC LIMIT ${next_param}"
    ));

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            i64,
            Option<i64>,
            i64,
        ),
    >(&sql)
    .bind(user_id);

    if let Some(library_ids) = allowed_library_ids {
        for library_id in library_ids {
            query = query.bind(library_id);
        }
    }

    let rows = query.bind(limit).fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                item_id,
                library_id,
                kind,
                title,
                year,
                poster_url,
                progress_ms,
                duration_ms,
                last_played_ts,
            )| ContinueWatchingRow {
                item_id,
                library_id,
                kind,
                title,
                year,
                poster_url,
                progress_ms,
                duration_ms,
                last_played_ts,
            },
        )
        .collect())
}
