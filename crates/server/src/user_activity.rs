use chrono::{Duration, TimeZone, Utc};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::account_prefs::UserPreferences;
use crate::error::AppError;
use crate::state::AppState;

const BROWSER_SESSION_STALE_SECONDS: i64 = 90;
const REALTIME_SESSION_STALE_SECONDS: i64 = 12 * 60 * 60;
const MEDIA_PROGRESS_MAX_FORWARD_DELTA_MS: i64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityRange {
    SevenDays,
    ThirtyDays,
    All,
}

impl ActivityRange {
    pub fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "30d" => Self::ThirtyDays,
            "all" => Self::All,
            _ => Self::SevenDays,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SevenDays => "7d",
            Self::ThirtyDays => "30d",
            Self::All => "all",
        }
    }

    pub fn from_ts(self, now_ts: i64) -> Option<i64> {
        match self {
            Self::SevenDays => Some(now_ts - 7 * 24 * 60 * 60),
            Self::ThirtyDays => Some(now_ts - 30 * 24 * 60 * 60),
            Self::All => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrowserActivityEventRequest {
    pub client_session_id: String,
    pub tab_id: String,
    pub section: String,
    pub event: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivitySummaryResponse {
    pub range: String,
    pub generated_ts: i64,
    pub activity_enabled: bool,
    pub totals: ActivityTotals,
    pub most_used_sections: Vec<ActivityBucket>,
    pub top_rooms: Vec<ActivityBucket>,
    pub top_voice_channels: Vec<ActivityBucket>,
    pub top_watched_media: Vec<ActivityBucket>,
    pub recent_activity: Vec<ActivityRecentEntry>,
    pub session_counts: ActivitySessionCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityTotals {
    pub total_time_ms: i64,
    pub rooms_time_ms: i64,
    pub voice_time_ms: i64,
    pub media_watch_time_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityBucket {
    pub key: String,
    pub label: String,
    pub total_ms: i64,
    pub session_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityRecentEntry {
    pub activity_kind: String,
    pub label: String,
    pub started_ts: i64,
    pub ended_ts: Option<i64>,
    pub total_ms: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ActivitySessionCounts {
    pub room_sessions: i64,
    pub voice_sessions: i64,
    pub media_sessions: i64,
}

pub async fn load_preferences(
    state: &AppState,
    user_id: &str,
) -> Result<UserPreferences, AppError> {
    let json_str = rustfin_db::repo::users::get_preferences(&state.db, user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "{}".to_string());
    UserPreferences::from_json_str(&json_str)
        .map_err(|e| ApiError::Internal(format!("invalid prefs JSON: {e}")).into())
}

pub async fn save_preferences(
    state: &AppState,
    user_id: &str,
    prefs: &UserPreferences,
) -> Result<(), AppError> {
    let json_str = serde_json::to_string(prefs)
        .map_err(|e| ApiError::Internal(format!("json serialize error: {e}")))?;
    rustfin_db::repo::users::update_preferences(&state.db, user_id, &json_str)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn is_activity_enabled(state: &AppState, user_id: &str) -> Result<bool, AppError> {
    Ok(load_preferences(state, user_id)
        .await?
        .privacy
        .personal_activity_enabled)
}

pub fn normalize_section_key(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "channels" | "rooms" | "servers" | "calendar" | "libraries" | "admin" | "account"
        | "home" => Some(normalized),
        _ => None,
    }
}

pub async fn handle_browser_event(
    state: &AppState,
    user_id: &str,
    body: &BrowserActivityEventRequest,
) -> Result<(), AppError> {
    if !is_activity_enabled(state, user_id).await? {
        return Ok(());
    }

    let client_session_id = body.client_session_id.trim();
    let tab_id = body.tab_id.trim();
    let section = normalize_section_key(&body.section)
        .ok_or_else(|| ApiError::BadRequest("invalid activity section".into()))?;
    let event = body.event.trim().to_ascii_lowercase();
    let now_ts = Utc::now().timestamp();

    if client_session_id.is_empty() || tab_id.is_empty() {
        return Err(
            ApiError::BadRequest("activity session identifiers are required".into()).into(),
        );
    }

    match event.as_str() {
        "start" | "heartbeat" => {
            rustfin_db::repo::user_activity::upsert_browser_session(
                &state.db,
                user_id,
                client_session_id,
                tab_id,
                &section,
                now_ts,
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        }
        "stop" => {
            rustfin_db::repo::user_activity::end_browser_session(
                &state.db,
                user_id,
                client_session_id,
                now_ts,
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        }
        _ => {
            return Err(ApiError::BadRequest("invalid browser activity event".into()).into());
        }
    }
    Ok(())
}

pub async fn track_voice_join(
    state: &AppState,
    user_id: &str,
    channel_id: &str,
) -> Result<(), AppError> {
    if !is_activity_enabled(state, user_id).await? {
        return Ok(());
    }
    rustfin_db::repo::user_activity::start_open_subject_session(
        &state.db,
        user_id,
        rustfin_db::repo::user_activity::KIND_VOICE_CHANNEL,
        "channel",
        channel_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn track_voice_leave(
    state: &AppState,
    user_id: &str,
    channel_id: &str,
) -> Result<(), AppError> {
    rustfin_db::repo::user_activity::end_open_subject_session(
        &state.db,
        user_id,
        rustfin_db::repo::user_activity::KIND_VOICE_CHANNEL,
        "channel",
        channel_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn track_room_join(
    state: &AppState,
    user_id: &str,
    room_id: &str,
) -> Result<(), AppError> {
    if !is_activity_enabled(state, user_id).await? {
        return Ok(());
    }
    rustfin_db::repo::user_activity::start_open_subject_session(
        &state.db,
        user_id,
        rustfin_db::repo::user_activity::KIND_WATCH_ROOM,
        "room",
        room_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn track_room_heartbeat(
    state: &AppState,
    user_id: &str,
    room_id: &str,
) -> Result<(), AppError> {
    let Some(existing) = rustfin_db::repo::user_activity::find_open_subject_session(
        &state.db,
        user_id,
        rustfin_db::repo::user_activity::KIND_WATCH_ROOM,
        "room",
        room_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Ok(());
    };
    rustfin_db::repo::user_activity::heartbeat_subject_session(
        &state.db,
        &existing.id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn track_room_leave(
    state: &AppState,
    user_id: &str,
    room_id: &str,
) -> Result<(), AppError> {
    rustfin_db::repo::user_activity::end_open_subject_session(
        &state.db,
        user_id,
        rustfin_db::repo::user_activity::KIND_WATCH_ROOM,
        "room",
        room_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn start_media_watch(
    state: &AppState,
    user_id: &str,
    session_id: &str,
    item_id: &str,
    file_id: &str,
) -> Result<(), AppError> {
    if !is_activity_enabled(state, user_id).await? {
        return Ok(());
    }
    rustfin_db::repo::user_activity::create_media_session(
        &state.db,
        session_id,
        user_id,
        item_id,
        file_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn record_media_progress(
    state: &AppState,
    user_id: &str,
    session_id: &str,
    progress_ms: i64,
) -> Result<(), AppError> {
    rustfin_db::repo::user_activity::record_media_progress(
        &state.db,
        user_id,
        session_id,
        progress_ms,
        MEDIA_PROGRESS_MAX_FORWARD_DELTA_MS,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn stop_media_watch(
    state: &AppState,
    user_id: &str,
    session_id: &str,
) -> Result<(), AppError> {
    rustfin_db::repo::user_activity::end_media_session(
        &state.db,
        user_id,
        session_id,
        Utc::now().timestamp(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn summarize_user_activity(
    state: &AppState,
    user_id: &str,
    range: ActivityRange,
) -> Result<ActivitySummaryResponse, AppError> {
    let now_ts = Utc::now().timestamp();
    let from_ts = range.from_ts(now_ts);
    let activity_enabled = is_activity_enabled(state, user_id).await?;
    let recent_rows =
        rustfin_db::repo::user_activity::list_activity_rows_for_range(&state.db, user_id, from_ts)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let total_time_ms: i64 = recent_rows
        .iter()
        .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_BROWSER_SECTION)
        .map(|row| compute_row_duration_ms(row, now_ts))
        .sum();
    let rooms_time_ms: i64 = recent_rows
        .iter()
        .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_WATCH_ROOM)
        .map(|row| compute_row_duration_ms(row, now_ts))
        .sum();
    let voice_time_ms: i64 = recent_rows
        .iter()
        .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_VOICE_CHANNEL)
        .map(|row| compute_row_duration_ms(row, now_ts))
        .sum();
    let media_watch_time_ms: i64 = recent_rows
        .iter()
        .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_MEDIA_WATCH)
        .map(|row| compute_row_duration_ms(row, now_ts))
        .sum();
    let session_counts = ActivitySessionCounts {
        room_sessions: recent_rows
            .iter()
            .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_WATCH_ROOM)
            .count() as i64,
        voice_sessions: recent_rows
            .iter()
            .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_VOICE_CHANNEL)
            .count() as i64,
        media_sessions: recent_rows
            .iter()
            .filter(|row| row.activity_kind == rustfin_db::repo::user_activity::KIND_MEDIA_WATCH)
            .count() as i64,
    };

    Ok(ActivitySummaryResponse {
        range: range.as_str().to_string(),
        generated_ts: now_ts,
        activity_enabled,
        totals: ActivityTotals {
            total_time_ms,
            rooms_time_ms,
            voice_time_ms,
            media_watch_time_ms,
        },
        most_used_sections: Vec::new(),
        top_rooms: Vec::new(),
        top_voice_channels: Vec::new(),
        top_watched_media: Vec::new(),
        recent_activity: Vec::new(),
        session_counts,
    })
}

pub async fn clear_user_history(state: &AppState, user_id: &str) -> Result<(), AppError> {
    rustfin_db::repo::user_activity::clear_user_activity(&state.db, user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn cleanup_and_rollup_once(pool: &rustfin_db::DbPool) -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    rustfin_db::repo::user_activity::cleanup_stale_open_sessions(
        pool,
        now - BROWSER_SESSION_STALE_SECONDS,
        now - REALTIME_SESSION_STALE_SECONDS,
        now,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let sessions = rustfin_db::repo::user_activity::list_closed_unrolled_sessions(pool, 256)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if sessions.is_empty() {
        return Ok(());
    }

    let mut upserts = Vec::new();
    let mut session_ids = Vec::with_capacity(sessions.len());
    for session in sessions {
        session_ids.push(session.id.clone());
        upserts.extend(split_session_into_daily_rows(&session));
    }
    rustfin_db::repo::user_activity::upsert_daily_rows(pool, &upserts)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    rustfin_db::repo::user_activity::mark_sessions_rolled_up(pool, &session_ids, now)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

pub async fn run_maintenance_loop(pool: rustfin_db::DbPool, shutdown: CancellationToken) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = interval.tick() => {
                if let Err(err) = cleanup_and_rollup_once(&pool).await {
                    warn!(error = ?err, "user activity maintenance tick failed");
                }
            }
        }
    }
}

fn compute_row_duration_ms(
    row: &rustfin_db::repo::user_activity::ActivityListRow,
    now_ts: i64,
) -> i64 {
    if row.activity_kind == rustfin_db::repo::user_activity::KIND_MEDIA_WATCH {
        return row.accumulated_ms.max(0);
    }
    (row.ended_ts.unwrap_or(now_ts) - row.started_ts).max(0) * 1000
}

fn split_session_into_daily_rows(
    session: &rustfin_db::repo::user_activity::UserActivitySessionRow,
) -> Vec<rustfin_db::repo::user_activity::ActivityDailyUpsert> {
    let ended_ts = session.ended_ts.unwrap_or(session.last_heartbeat_ts);
    if ended_ts <= session.started_ts {
        return Vec::new();
    }
    if session.activity_kind == rustfin_db::repo::user_activity::KIND_MEDIA_WATCH {
        return vec![rustfin_db::repo::user_activity::ActivityDailyUpsert {
            user_id: session.user_id.clone(),
            day_utc: Utc
                .timestamp_opt(session.started_ts, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .date_naive()
                .to_string(),
            activity_kind: session.activity_kind.clone(),
            section_key: session.section_key.clone(),
            subject_type: session.subject_type.clone(),
            subject_id: session.subject_id.clone(),
            total_ms: session.accumulated_ms.max(0),
            session_count: 1,
            first_started_ts: Some(session.started_ts),
            last_ended_ts: Some(ended_ts),
        }];
    }

    let mut cursor = Utc
        .timestamp_opt(session.started_ts, 0)
        .single()
        .unwrap_or_else(Utc::now);
    let end = Utc
        .timestamp_opt(ended_ts, 0)
        .single()
        .unwrap_or_else(Utc::now);
    let mut rows = Vec::new();
    while cursor < end {
        let next_day = (cursor.date_naive() + Duration::days(1)).and_hms_opt(0, 0, 0);
        let next_boundary = next_day
            .map(|naive| Utc.from_utc_datetime(&naive))
            .unwrap_or(end);
        let slice_end = std::cmp::min(end, next_boundary);
        let total_ms = (slice_end.timestamp() - cursor.timestamp()).max(0) * 1000;
        if total_ms > 0 {
            rows.push(rustfin_db::repo::user_activity::ActivityDailyUpsert {
                user_id: session.user_id.clone(),
                day_utc: cursor.date_naive().to_string(),
                activity_kind: session.activity_kind.clone(),
                section_key: session.section_key.clone(),
                subject_type: session.subject_type.clone(),
                subject_id: session.subject_id.clone(),
                total_ms,
                session_count: 1,
                first_started_ts: Some(cursor.timestamp()),
                last_ended_ts: Some(slice_end.timestamp()),
            });
        }
        cursor = slice_end;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::split_session_into_daily_rows;

    #[test]
    fn split_room_session_across_midnight() {
        let start = 1_710_108_000;
        let end = start + 3600;
        let rows = split_session_into_daily_rows(
            &rustfin_db::repo::user_activity::UserActivitySessionRow {
                id: "s-1".to_string(),
                user_id: "u-1".to_string(),
                activity_kind: rustfin_db::repo::user_activity::KIND_WATCH_ROOM.to_string(),
                section_key: "".to_string(),
                subject_type: "room".to_string(),
                subject_id: "room-1".to_string(),
                tab_id: None,
                client_session_id: None,
                started_ts: start,
                last_heartbeat_ts: end,
                ended_ts: Some(end),
                accumulated_ms: 0,
                last_position_ms: None,
                rolled_up_ts: None,
                created_ts: start,
                updated_ts: end,
            },
        );
        assert!(!rows.is_empty());
        assert_eq!(rows.iter().map(|row| row.total_ms).sum::<i64>(), 3_600_000);
    }
}
