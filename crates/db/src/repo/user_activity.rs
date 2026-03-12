use crate::DbPool;
use serde::{Deserialize, Serialize};

pub const KIND_BROWSER_SECTION: &str = "browser_section";
pub const KIND_VOICE_CHANNEL: &str = "voice_channel";
pub const KIND_WATCH_ROOM: &str = "watch_room";
pub const KIND_MEDIA_WATCH: &str = "media_watch";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserActivitySessionRow {
    pub id: String,
    pub user_id: String,
    pub activity_kind: String,
    pub section_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub tab_id: Option<String>,
    pub client_session_id: Option<String>,
    pub started_ts: i64,
    pub last_heartbeat_ts: i64,
    pub ended_ts: Option<i64>,
    pub accumulated_ms: i64,
    pub last_position_ms: Option<i64>,
    pub rolled_up_ts: Option<i64>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

type ActivitySessionTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
);

fn map_session_row(
    (
        id,
        user_id,
        activity_kind,
        section_key,
        subject_type,
        subject_id,
        tab_id,
        client_session_id,
        started_ts,
        last_heartbeat_ts,
        ended_ts,
        accumulated_ms,
        last_position_ms,
        rolled_up_ts,
        created_ts,
        updated_ts,
    ): ActivitySessionTuple,
) -> UserActivitySessionRow {
    UserActivitySessionRow {
        id,
        user_id,
        activity_kind,
        section_key,
        subject_type,
        subject_id,
        tab_id,
        client_session_id,
        started_ts,
        last_heartbeat_ts,
        ended_ts,
        accumulated_ms,
        last_position_ms,
        rolled_up_ts,
        created_ts,
        updated_ts,
    }
}

const SESSION_SELECT_COLUMNS: &str = "id, user_id, activity_kind, section_key, subject_type, subject_id, tab_id, \
     client_session_id, started_ts, last_heartbeat_ts, ended_ts, accumulated_ms, \
     last_position_ms, rolled_up_ts, created_ts, updated_ts";

fn bounded_forward_progress_delta(
    previous_ms: i64,
    next_ms: i64,
    max_forward_delta_ms: i64,
) -> i64 {
    (next_ms - previous_ms)
        .max(0)
        .min(max_forward_delta_ms.max(0))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserActivityDailyRow {
    pub user_id: String,
    pub day_utc: String,
    pub activity_kind: String,
    pub section_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub total_ms: i64,
    pub session_count: i64,
    pub first_started_ts: Option<i64>,
    pub last_ended_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ActivityDailyUpsert {
    pub user_id: String,
    pub day_utc: String,
    pub activity_kind: String,
    pub section_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub total_ms: i64,
    pub session_count: i64,
    pub first_started_ts: Option<i64>,
    pub last_ended_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ActivityListRow {
    pub activity_kind: String,
    pub section_key: String,
    pub subject_type: String,
    pub subject_id: String,
    pub started_ts: i64,
    pub last_heartbeat_ts: i64,
    pub ended_ts: Option<i64>,
    pub accumulated_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ActivityAggregateRow {
    pub key: String,
    pub total_ms: i64,
    pub session_count: i64,
}

fn duration_sql(now_ts_param: usize) -> String {
    format!(
        "CASE \
            WHEN activity_kind = '{KIND_MEDIA_WATCH}' THEN accumulated_ms \
            ELSE GREATEST(COALESCE(ended_ts, ${now_ts_param}) - started_ts, 0) \
         END"
    )
}

pub async fn get_open_browser_session(
    pool: &DbPool,
    user_id: &str,
    client_session_id: &str,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {SESSION_SELECT_COLUMNS} \
         FROM user_activity_session \
         WHERE user_id = $1 AND activity_kind = $2 AND client_session_id = $3 AND ended_ts IS NULL \
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(user_id)
        .bind(KIND_BROWSER_SECTION)
        .bind(client_session_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn upsert_browser_session(
    pool: &DbPool,
    user_id: &str,
    client_session_id: &str,
    tab_id: &str,
    section_key: &str,
    now_ts: i64,
) -> Result<UserActivitySessionRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO user_activity_session \
         (id, user_id, activity_kind, section_key, subject_type, subject_id, tab_id, client_session_id, \
          started_ts, last_heartbeat_ts, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, '', '', $5, $1, $6, $6, $6, $6) \
         ON CONFLICT (id) DO UPDATE SET \
           section_key = EXCLUDED.section_key, \
           tab_id = EXCLUDED.tab_id, \
           last_heartbeat_ts = EXCLUDED.last_heartbeat_ts, \
           updated_ts = EXCLUDED.updated_ts, \
           ended_ts = NULL \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(client_session_id)
        .bind(user_id)
        .bind(KIND_BROWSER_SECTION)
        .bind(section_key)
        .bind(tab_id)
        .bind(now_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_session_row(row))
}

pub async fn end_browser_session(
    pool: &DbPool,
    user_id: &str,
    client_session_id: &str,
    now_ts: i64,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "UPDATE user_activity_session \
         SET ended_ts = COALESCE(ended_ts, $3), last_heartbeat_ts = $3, updated_ts = $3 \
         WHERE user_id = $1 AND activity_kind = $2 AND client_session_id = $4 AND ended_ts IS NULL \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(user_id)
        .bind(KIND_BROWSER_SECTION)
        .bind(now_ts)
        .bind(client_session_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn start_open_subject_session(
    pool: &DbPool,
    user_id: &str,
    activity_kind: &str,
    subject_type: &str,
    subject_id: &str,
    now_ts: i64,
) -> Result<UserActivitySessionRow, sqlx::Error> {
    if let Some(existing) =
        find_open_subject_session(pool, user_id, activity_kind, subject_type, subject_id).await?
    {
        return heartbeat_subject_session(pool, &existing.id, now_ts)
            .await
            .map(|row| row.unwrap_or(existing));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let sql = format!(
        "INSERT INTO user_activity_session \
         (id, user_id, activity_kind, section_key, subject_type, subject_id, started_ts, \
          last_heartbeat_ts, created_ts, updated_ts) \
         VALUES ($1, $2, $3, '', $4, $5, $6, $6, $6, $6) \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(id)
        .bind(user_id)
        .bind(activity_kind)
        .bind(subject_type)
        .bind(subject_id)
        .bind(now_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_session_row(row))
}

pub async fn find_open_subject_session(
    pool: &DbPool,
    user_id: &str,
    activity_kind: &str,
    subject_type: &str,
    subject_id: &str,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {SESSION_SELECT_COLUMNS} \
         FROM user_activity_session \
         WHERE user_id = $1 AND activity_kind = $2 AND subject_type = $3 AND subject_id = $4 AND ended_ts IS NULL \
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(user_id)
        .bind(activity_kind)
        .bind(subject_type)
        .bind(subject_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn heartbeat_subject_session(
    pool: &DbPool,
    session_id: &str,
    now_ts: i64,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "UPDATE user_activity_session \
         SET last_heartbeat_ts = $2, updated_ts = $2 \
         WHERE id = $1 AND ended_ts IS NULL \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(session_id)
        .bind(now_ts)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn end_open_subject_session(
    pool: &DbPool,
    user_id: &str,
    activity_kind: &str,
    subject_type: &str,
    subject_id: &str,
    now_ts: i64,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "UPDATE user_activity_session \
         SET ended_ts = COALESCE(ended_ts, $5), last_heartbeat_ts = $5, updated_ts = $5 \
         WHERE user_id = $1 AND activity_kind = $2 AND subject_type = $3 AND subject_id = $4 AND ended_ts IS NULL \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(user_id)
        .bind(activity_kind)
        .bind(subject_type)
        .bind(subject_id)
        .bind(now_ts)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn create_media_session(
    pool: &DbPool,
    session_id: &str,
    user_id: &str,
    item_id: &str,
    file_id: &str,
    now_ts: i64,
) -> Result<UserActivitySessionRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO user_activity_session \
         (id, user_id, activity_kind, section_key, subject_type, subject_id, client_session_id, \
          started_ts, last_heartbeat_ts, created_ts, updated_ts) \
         VALUES ($1, $2, $3, '', 'item', $4, $5, $6, $6, $6, $6) \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(session_id)
        .bind(user_id)
        .bind(KIND_MEDIA_WATCH)
        .bind(item_id)
        .bind(file_id)
        .bind(now_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_session_row(row))
}

pub async fn record_media_progress(
    pool: &DbPool,
    user_id: &str,
    session_id: &str,
    progress_ms: i64,
    max_forward_delta_ms: i64,
    now_ts: i64,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let existing = sqlx::query_as::<_, ActivitySessionTuple>(&format!(
        "SELECT {SESSION_SELECT_COLUMNS} FROM user_activity_session \
         WHERE id = $1 AND user_id = $2 AND activity_kind = $3 LIMIT 1"
    ))
    .bind(session_id)
    .bind(user_id)
    .bind(KIND_MEDIA_WATCH)
    .fetch_optional(pool)
    .await?
    .map(map_session_row);

    let Some(existing) = existing else {
        return Ok(None);
    };

    let previous = existing.last_position_ms.unwrap_or(0);
    let delta = bounded_forward_progress_delta(previous, progress_ms, max_forward_delta_ms);
    let sql = format!(
        "UPDATE user_activity_session \
         SET accumulated_ms = accumulated_ms + $3, \
             last_position_ms = $4, \
             last_heartbeat_ts = $5, \
             updated_ts = $5 \
         WHERE id = $1 AND user_id = $2 \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(session_id)
        .bind(user_id)
        .bind(delta)
        .bind(progress_ms.max(0))
        .bind(now_ts)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn end_media_session(
    pool: &DbPool,
    user_id: &str,
    session_id: &str,
    now_ts: i64,
) -> Result<Option<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "UPDATE user_activity_session \
         SET ended_ts = COALESCE(ended_ts, $3), last_heartbeat_ts = $3, updated_ts = $3 \
         WHERE id = $1 AND user_id = $2 AND activity_kind = $4 \
         RETURNING {SESSION_SELECT_COLUMNS}"
    );
    let row = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(session_id)
        .bind(user_id)
        .bind(now_ts)
        .bind(KIND_MEDIA_WATCH)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_session_row))
}

pub async fn cleanup_stale_open_sessions(
    pool: &DbPool,
    browser_cutoff_ts: i64,
    realtime_cutoff_ts: i64,
    now_ts: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE user_activity_session \
         SET ended_ts = $1, updated_ts = $1 \
         WHERE ended_ts IS NULL AND ( \
             (activity_kind = $2 AND last_heartbeat_ts < $3) OR \
             (activity_kind IN ($4, $5, $6) AND last_heartbeat_ts < $7) \
         )",
    )
    .bind(now_ts)
    .bind(KIND_BROWSER_SECTION)
    .bind(browser_cutoff_ts)
    .bind(KIND_VOICE_CHANNEL)
    .bind(KIND_WATCH_ROOM)
    .bind(KIND_MEDIA_WATCH)
    .bind(realtime_cutoff_ts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn list_closed_unrolled_sessions(
    pool: &DbPool,
    limit: i64,
) -> Result<Vec<UserActivitySessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {SESSION_SELECT_COLUMNS} \
         FROM user_activity_session \
         WHERE ended_ts IS NOT NULL AND rolled_up_ts IS NULL \
         ORDER BY ended_ts ASC \
         LIMIT $1"
    );
    let rows = sqlx::query_as::<_, ActivitySessionTuple>(&sql)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_session_row).collect())
}

pub async fn mark_sessions_rolled_up(
    pool: &DbPool,
    session_ids: &[String],
    rolled_up_ts: i64,
) -> Result<(), sqlx::Error> {
    if session_ids.is_empty() {
        return Ok(());
    }
    let placeholders = crate::repo::dollar_placeholders(2, session_ids.len());
    let sql = format!(
        "UPDATE user_activity_session SET rolled_up_ts = $1, updated_ts = $1 WHERE id IN ({placeholders})"
    );
    let mut query = sqlx::query(&sql).bind(rolled_up_ts);
    for session_id in session_ids {
        query = query.bind(session_id);
    }
    query.execute(pool).await?;
    Ok(())
}

pub async fn upsert_daily_rows(
    pool: &DbPool,
    rows: &[ActivityDailyUpsert],
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for row in rows {
        sqlx::query(
            "INSERT INTO user_activity_daily \
             (user_id, day_utc, activity_kind, section_key, subject_type, subject_id, total_ms, session_count, first_started_ts, last_ended_ts) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (user_id, day_utc, activity_kind, section_key, subject_type, subject_id) DO UPDATE SET \
               total_ms = user_activity_daily.total_ms + EXCLUDED.total_ms, \
               session_count = user_activity_daily.session_count + EXCLUDED.session_count, \
               first_started_ts = CASE \
                   WHEN user_activity_daily.first_started_ts IS NULL THEN EXCLUDED.first_started_ts \
                   WHEN EXCLUDED.first_started_ts IS NULL THEN user_activity_daily.first_started_ts \
                   ELSE LEAST(user_activity_daily.first_started_ts, EXCLUDED.first_started_ts) \
               END, \
               last_ended_ts = CASE \
                   WHEN user_activity_daily.last_ended_ts IS NULL THEN EXCLUDED.last_ended_ts \
                   WHEN EXCLUDED.last_ended_ts IS NULL THEN user_activity_daily.last_ended_ts \
                   ELSE GREATEST(user_activity_daily.last_ended_ts, EXCLUDED.last_ended_ts) \
               END"
        )
        .bind(&row.user_id)
        .bind(&row.day_utc)
        .bind(&row.activity_kind)
        .bind(&row.section_key)
        .bind(&row.subject_type)
        .bind(&row.subject_id)
        .bind(row.total_ms)
        .bind(row.session_count)
        .bind(row.first_started_ts)
        .bind(row.last_ended_ts)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn get_daily_rows_for_user(
    pool: &DbPool,
    user_id: &str,
) -> Result<Vec<UserActivityDailyRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, Option<i64>, Option<i64>)>(
        "SELECT user_id, day_utc, activity_kind, section_key, subject_type, subject_id, total_ms, session_count, first_started_ts, last_ended_ts \
         FROM user_activity_daily WHERE user_id = $1 ORDER BY day_utc ASC, activity_kind ASC"
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                user_id,
                day_utc,
                activity_kind,
                section_key,
                subject_type,
                subject_id,
                total_ms,
                session_count,
                first_started_ts,
                last_ended_ts,
            )| UserActivityDailyRow {
                user_id,
                day_utc,
                activity_kind,
                section_key,
                subject_type,
                subject_id,
                total_ms,
                session_count,
                first_started_ts,
                last_ended_ts,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::bounded_forward_progress_delta;

    #[test]
    fn bounded_forward_progress_delta_ignores_seek_inflation() {
        assert_eq!(
            bounded_forward_progress_delta(10_000, 12_000, 15_000),
            2_000
        );
        assert_eq!(
            bounded_forward_progress_delta(10_000, 130_000, 15_000),
            15_000
        );
        assert_eq!(bounded_forward_progress_delta(10_000, 5_000, 15_000), 0);
    }
}

pub async fn clear_user_activity(pool: &DbPool, user_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM user_activity_daily WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM user_activity_session WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn list_activity_rows_for_range(
    pool: &DbPool,
    user_id: &str,
    from_ts: Option<i64>,
) -> Result<Vec<ActivityListRow>, sqlx::Error> {
    let mut sql = "SELECT activity_kind, section_key, subject_type, subject_id, started_ts, last_heartbeat_ts, ended_ts, accumulated_ms \
         FROM user_activity_session WHERE user_id = $1"
        .to_string();
    if from_ts.is_some() {
        sql.push_str(" AND COALESCE(ended_ts, last_heartbeat_ts, started_ts) >= $2");
    }
    sql.push_str(" ORDER BY COALESCE(ended_ts, last_heartbeat_ts, started_ts) DESC");

    let mut query =
        sqlx::query_as::<_, (String, String, String, String, i64, i64, Option<i64>, i64)>(&sql)
            .bind(user_id);
    if let Some(from_ts) = from_ts {
        query = query.bind(from_ts);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                activity_kind,
                section_key,
                subject_type,
                subject_id,
                started_ts,
                last_heartbeat_ts,
                ended_ts,
                accumulated_ms,
            )| ActivityListRow {
                activity_kind,
                section_key,
                subject_type,
                subject_id,
                started_ts,
                last_heartbeat_ts,
                ended_ts,
                accumulated_ms,
            },
        )
        .collect())
}

pub async fn aggregate_browser_sections(
    pool: &DbPool,
    user_id: &str,
    from_ts: Option<i64>,
    now_ts: i64,
) -> Result<Vec<ActivityAggregateRow>, sqlx::Error> {
    let duration = duration_sql(3);
    let mut sql = format!(
        "SELECT section_key, COALESCE(SUM(({duration})::BIGINT), 0::BIGINT) AS total_ms, COUNT(*) AS session_count \
         FROM user_activity_session \
         WHERE user_id = $1 AND activity_kind = $2"
    );
    if from_ts.is_some() {
        sql.push_str(" AND COALESCE(ended_ts, last_heartbeat_ts, started_ts) >= $4");
    }
    sql.push_str(" GROUP BY section_key ORDER BY total_ms DESC, section_key ASC");
    let mut query = sqlx::query_as::<_, (String, i64, i64)>(&sql)
        .bind(user_id)
        .bind(KIND_BROWSER_SECTION)
        .bind(now_ts);
    if let Some(from_ts) = from_ts {
        query = query.bind(from_ts);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(key, total_ms, session_count)| ActivityAggregateRow {
            key,
            total_ms,
            session_count,
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
pub async fn aggregate_named_subjects(
    pool: &DbPool,
    user_id: &str,
    activity_kind: &str,
    join_table: &str,
    join_name_column: &str,
    from_ts: Option<i64>,
    now_ts: i64,
    limit: i64,
) -> Result<Vec<ActivityAggregateRow>, sqlx::Error> {
    let duration = duration_sql(3);
    let mut sql = format!(
        "SELECT COALESCE(t.{join_name_column}, s.subject_id) AS label, COALESCE(SUM(({duration})::BIGINT), 0::BIGINT) AS total_ms, COUNT(*) AS session_count \
         FROM user_activity_session s \
         LEFT JOIN {join_table} t ON t.id = s.subject_id \
         WHERE s.user_id = $1 AND s.activity_kind = $2"
    );
    if from_ts.is_some() {
        sql.push_str(" AND COALESCE(s.ended_ts, s.last_heartbeat_ts, s.started_ts) >= $4");
    }
    sql.push_str(" GROUP BY label ORDER BY total_ms DESC, label ASC LIMIT ");
    sql.push_str(&limit.to_string());
    let mut query = sqlx::query_as::<_, (String, i64, i64)>(&sql)
        .bind(user_id)
        .bind(activity_kind)
        .bind(now_ts);
    if let Some(from_ts) = from_ts {
        query = query.bind(from_ts);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(key, total_ms, session_count)| ActivityAggregateRow {
            key,
            total_ms,
            session_count,
        })
        .collect())
}
