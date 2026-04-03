use anyhow::{Context, bail};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::{get, patch},
};
use chrono::{Datelike, NaiveDate};
use rustfin_core::axum_error::AppError;
use rustfin_core::error::ApiError;
use rustfin_db::DbPool;
use rustfin_db::repo::calendar::{
    CalendarEventRow, NewCalendarEvent, UpdateCalendarEvent, create_event as db_create_event,
    delete_event as db_delete_event, get_event as db_get_event, list_personal_events,
    list_visible_events, update_event as db_update_event,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    db: DbPool,
    auth_base_url: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

fn env_bool_with_fallback(primary: &str, fallback: &str, default: bool) -> bool {
    let parse = |raw: String| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    };

    std::env::var(primary)
        .ok()
        .map(parse)
        .or_else(|| std::env::var(fallback).ok().map(parse))
        .unwrap_or(default)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthUser {
    id: String,
    username: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct ListEventsQuery {
    from: String,
    to: String,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateEventRequest {
    title: String,
    description: Option<String>,
    event_date: String,
    scope: Option<String>,
    owner_user_id: Option<String>,
    event_type: Option<String>,
    recurrence: Option<String>,
    birthday_year: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateEventRequest {
    title: Option<String>,
    description: Option<String>,
    event_date: Option<String>,
    scope: Option<String>,
    owner_user_id: Option<String>,
    event_type: Option<String>,
    recurrence: Option<String>,
    birthday_year: Option<i32>,
}

#[derive(Debug, Serialize)]
struct CalendarEventResponse {
    id: String,
    occurrence_id: String,
    title: String,
    description: Option<String>,
    display_description: Option<String>,
    event_date: String,
    source_event_date: String,
    scope: String,
    owner_user_id: Option<String>,
    owner_username: Option<String>,
    event_type: String,
    recurrence: String,
    birthday_year: Option<i32>,
    derived_age: Option<i32>,
    created_by_user_id: String,
    created_by_username: Option<String>,
    can_edit: bool,
    can_delete: bool,
}

#[derive(Debug, Serialize)]
struct CalendarUserResponse {
    id: String,
    username: String,
    role: String,
}

#[derive(Debug, Serialize)]
struct EventsEnvelope {
    events: Vec<CalendarEventResponse>,
}

fn parse_ymd(raw: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        ApiError::validation(serde_json::json!({
            field: ["must be in YYYY-MM-DD format"]
        }))
        .into()
    })
}

fn format_ymd(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn normalize_optional_string(raw: Option<String>) -> Option<String> {
    raw.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn ensure_scope(value: Option<&str>) -> Result<&str, AppError> {
    match value.unwrap_or("personal") {
        "global" => Ok("global"),
        "personal" => Ok("personal"),
        _ => Err(ApiError::validation(serde_json::json!({
            "scope": ["must be either 'global' or 'personal'"]
        }))
        .into()),
    }
}

fn ensure_event_type(value: Option<&str>) -> Result<&str, AppError> {
    match value.unwrap_or("event") {
        "event" => Ok("event"),
        "birthday" => Ok("birthday"),
        _ => Err(ApiError::validation(serde_json::json!({
            "event_type": ["must be either 'event' or 'birthday'"]
        }))
        .into()),
    }
}

fn ensure_recurrence(value: Option<&str>) -> Result<&str, AppError> {
    match value.unwrap_or("none") {
        "none" => Ok("none"),
        "yearly" => Ok("yearly"),
        _ => Err(ApiError::validation(serde_json::json!({
            "recurrence": ["must be either 'none' or 'yearly'"]
        }))
        .into()),
    }
}

fn ensure_title(raw: &str) -> Result<String, AppError> {
    let title = raw.trim();
    if title.is_empty() {
        return Err(ApiError::validation(serde_json::json!({
            "title": ["cannot be empty"]
        }))
        .into());
    }
    if title.chars().count() > 140 {
        return Err(ApiError::validation(serde_json::json!({
            "title": ["must be 140 characters or fewer"]
        }))
        .into());
    }
    Ok(title.to_string())
}

fn validate_birthday_year(year: Option<i32>) -> Result<Option<i32>, AppError> {
    let Some(year) = year else {
        return Ok(None);
    };
    let current_year = chrono::Utc::now().year();
    if year < 1900 || year > current_year {
        return Err(ApiError::validation(serde_json::json!({
            "birthday_year": [format!("must be between 1900 and {current_year}")]
        }))
        .into());
    }
    Ok(Some(year))
}

fn can_manage_event(user: &AuthUser, row: &CalendarEventRow) -> bool {
    if user.role == "admin" {
        return true;
    }
    row.scope == "personal" && row.owner_user_id.as_deref() == Some(user.id.as_str())
}

fn with_year_safe(date: NaiveDate, year: i32) -> Option<NaiveDate> {
    if let Some(updated) = date.with_year(year) {
        return Some(updated);
    }
    if date.month() == 2 && date.day() == 29 {
        return NaiveDate::from_ymd_opt(year, 2, 28);
    }
    None
}

fn expanded_occurrences(
    row: &CalendarEventRow,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<(NaiveDate, Option<i32>)>, AppError> {
    let source_date = parse_ymd(&row.event_date, "event_date")?;

    if row.recurrence != "yearly" {
        if source_date < from || source_date > to {
            return Ok(Vec::new());
        }
        let age = row
            .birthday_year
            .filter(|_| row.event_type == "birthday")
            .map(|birth_year| source_date.year() - birth_year);
        return Ok(vec![(source_date, age)]);
    }

    let mut out = Vec::new();
    for year in from.year()..=to.year() {
        let Some(occurrence_date) = with_year_safe(source_date, year) else {
            continue;
        };
        if occurrence_date < from || occurrence_date > to {
            continue;
        }
        let age = row
            .birthday_year
            .filter(|_| row.event_type == "birthday")
            .map(|birth_year| occurrence_date.year() - birth_year);
        out.push((occurrence_date, age));
    }
    Ok(out)
}

fn display_description(
    description: &Option<String>,
    event_type: &str,
    derived_age: Option<i32>,
) -> Option<String> {
    if event_type == "birthday" {
        let age_note = derived_age.map(|age| format!("Turns {age}"));
        return match (description.as_ref(), age_note) {
            (Some(desc), Some(age)) => Some(format!("{desc} • {age}")),
            (Some(desc), None) => Some(desc.clone()),
            (None, Some(age)) => Some(age),
            (None, None) => None,
        };
    }
    description.clone()
}

fn to_response(
    row: &CalendarEventRow,
    user: &AuthUser,
    occurrence_date: NaiveDate,
    derived_age: Option<i32>,
) -> CalendarEventResponse {
    let can_manage = can_manage_event(user, row);
    CalendarEventResponse {
        id: row.id.clone(),
        occurrence_id: format!("{}:{}", row.id, format_ymd(occurrence_date)),
        title: row.title.clone(),
        description: row.description.clone(),
        display_description: display_description(&row.description, &row.event_type, derived_age),
        event_date: format_ymd(occurrence_date),
        source_event_date: row.event_date.clone(),
        scope: row.scope.clone(),
        owner_user_id: row.owner_user_id.clone(),
        owner_username: row.owner_username.clone(),
        event_type: row.event_type.clone(),
        recurrence: row.recurrence.clone(),
        birthday_year: row.birthday_year,
        derived_age,
        created_by_user_id: row.created_by_user_id.clone(),
        created_by_username: row.created_by_username.clone(),
        can_edit: can_manage,
        can_delete: can_manage,
    }
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<AuthUser, AppError> {
    let auth_value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::Unauthorized("missing bearer token".to_string()))?;

    if !auth_value.to_ascii_lowercase().starts_with("bearer ") {
        return Err(ApiError::Unauthorized("invalid authorization header".to_string()).into());
    }

    let url = format!(
        "{}/api/v1/users/me",
        state.auth_base_url.trim_end_matches('/')
    );
    let response = state
        .http_client
        .get(url)
        .header(header::AUTHORIZATION, auth_value)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("auth upstream unreachable: {e}")))?;

    match response.status() {
        StatusCode::OK => response
            .json::<AuthUser>()
            .await
            .map_err(|e| ApiError::Internal(format!("failed to decode auth response: {e}")).into()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(ApiError::Unauthorized("invalid token".to_string()).into())
        }
        code => Err(
            ApiError::Internal(format!("auth upstream returned unexpected status {code}")).into(),
        ),
    }
}

async fn require_admin(user: &AuthUser) -> Result<(), AppError> {
    if user.role != "admin" {
        return Err(ApiError::Forbidden("admin access required".to_string()).into());
    }
    Ok(())
}

fn enforce_range_bounds(from: NaiveDate, to: NaiveDate) -> Result<(), AppError> {
    if to < from {
        return Err(ApiError::validation(serde_json::json!({
            "to": ["must be on or after from"]
        }))
        .into());
    }
    let days = (to - from).num_days();
    if days > 370 {
        return Err(ApiError::validation(serde_json::json!({
            "range": ["maximum query range is 370 days"]
        }))
        .into());
    }
    Ok(())
}

async fn ensure_user_exists(pool: &DbPool, user_id: &str) -> Result<(), AppError> {
    let user = rustfin_db::repo::users::find_by_id(pool, user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if user.is_none() {
        return Err(ApiError::NotFound(format!("user {user_id} not found")).into());
    }
    Ok(())
}

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<CalendarUserResponse>>, AppError> {
    let auth = authenticate(&state, &headers).await?;
    if auth.role != "admin" {
        return Ok(Json(vec![CalendarUserResponse {
            id: auth.id,
            username: auth.username,
            role: auth.role,
        }]));
    }

    let users = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let response = users
        .into_iter()
        .map(|u| CalendarUserResponse {
            id: u.id,
            username: u.username,
            role: u.role,
        })
        .collect();
    Ok(Json(response))
}

async fn list_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<EventsEnvelope>, AppError> {
    let auth = authenticate(&state, &headers).await?;
    let from = parse_ymd(&query.from, "from")?;
    let to = parse_ymd(&query.to, "to")?;
    enforce_range_bounds(from, to)?;

    let rows = list_visible_events(
        &state.db,
        &auth.id,
        auth.role == "admin",
        &query.from,
        &query.to,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let scope_filter = query.scope.as_deref();
    if let Some(filter) = scope_filter
        && filter != "all"
        && filter != "global"
        && filter != "personal"
    {
        return Err(ApiError::validation(serde_json::json!({
            "scope": ["must be one of: all, global, personal"]
        }))
        .into());
    }
    let mut events = Vec::new();
    for row in rows {
        for (occurrence_date, derived_age) in expanded_occurrences(&row, from, to)? {
            if let Some(filter) = scope_filter
                && filter != "all"
                && filter != row.scope
            {
                continue;
            }
            events.push(to_response(&row, &auth, occurrence_date, derived_age));
        }
    }

    events.sort_by(|a, b| {
        a.event_date
            .cmp(&b.event_date)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(Json(EventsEnvelope { events }))
}

async fn list_personal_events_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListEventsQuery>,
) -> Result<Json<EventsEnvelope>, AppError> {
    let auth = authenticate(&state, &headers).await?;
    require_admin(&auth).await?;

    let from = parse_ymd(&query.from, "from")?;
    let to = parse_ymd(&query.to, "to")?;
    enforce_range_bounds(from, to)?;

    let rows = list_personal_events(&state.db, &query.from, &query.to)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut events = Vec::new();
    for row in rows {
        for (occurrence_date, derived_age) in expanded_occurrences(&row, from, to)? {
            events.push(to_response(&row, &auth, occurrence_date, derived_age));
        }
    }
    events.sort_by(|a, b| {
        a.event_date
            .cmp(&b.event_date)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(Json(EventsEnvelope { events }))
}

async fn create_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateEventRequest>,
) -> Result<Json<CalendarEventResponse>, AppError> {
    let auth = authenticate(&state, &headers).await?;
    let title = ensure_title(&body.title)?;
    let event_date = parse_ymd(&body.event_date, "event_date")?;
    let event_type = ensure_event_type(body.event_type.as_deref())?;
    let mut recurrence = ensure_recurrence(body.recurrence.as_deref())?;
    let scope = ensure_scope(body.scope.as_deref())?;
    let description = normalize_optional_string(body.description);
    let mut birthday_year = validate_birthday_year(body.birthday_year)?;

    if event_type == "birthday" {
        recurrence = "yearly";
        if birthday_year.is_none() {
            return Err(ApiError::validation(serde_json::json!({
                "birthday_year": ["is required for birthday events"]
            }))
            .into());
        }
    } else {
        birthday_year = None;
    }

    if scope == "global" && auth.role != "admin" {
        return Err(ApiError::Forbidden("only admins can create global events".to_string()).into());
    }

    let owner_user_id = if scope == "personal" {
        if auth.role == "admin" {
            let owner = body.owner_user_id.unwrap_or_else(|| auth.id.clone());
            ensure_user_exists(&state.db, &owner).await?;
            Some(owner)
        } else {
            Some(auth.id.clone())
        }
    } else {
        None
    };

    let created = db_create_event(
        &state.db,
        &NewCalendarEvent {
            scope: scope.to_string(),
            owner_user_id,
            title,
            description,
            event_date: format_ymd(event_date),
            event_type: event_type.to_string(),
            recurrence: recurrence.to_string(),
            birthday_year,
            created_by_user_id: auth.id.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let derived_age = created
        .birthday_year
        .filter(|_| created.event_type == "birthday")
        .map(|birth_year| event_date.year() - birth_year);

    Ok(Json(to_response(&created, &auth, event_date, derived_age)))
}

async fn update_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(event_id): Path<String>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<Json<CalendarEventResponse>, AppError> {
    let auth = authenticate(&state, &headers).await?;
    let existing = db_get_event(&state.db, &event_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("event {event_id} not found")))?;

    if !can_manage_event(&auth, &existing) {
        return Err(ApiError::Forbidden("you cannot modify this event".to_string()).into());
    }

    let title = ensure_title(body.title.as_deref().unwrap_or(&existing.title))?;
    let event_date_raw = body
        .event_date
        .unwrap_or_else(|| existing.event_date.clone());
    let event_date = parse_ymd(&event_date_raw, "event_date")?;
    let event_type = ensure_event_type(body.event_type.as_deref().or(Some(&existing.event_type)))?;
    let mut recurrence =
        ensure_recurrence(body.recurrence.as_deref().or(Some(&existing.recurrence)))?;
    let scope = ensure_scope(body.scope.as_deref().or(Some(&existing.scope)))?;
    let description = if body.description.is_some() {
        normalize_optional_string(body.description)
    } else {
        existing.description.clone()
    };

    if scope == "global" && auth.role != "admin" {
        return Err(ApiError::Forbidden("only admins can set global scope".to_string()).into());
    }

    let owner_user_id = if scope == "personal" {
        if auth.role == "admin" {
            let owner = body
                .owner_user_id
                .or_else(|| existing.owner_user_id.clone())
                .unwrap_or_else(|| auth.id.clone());
            ensure_user_exists(&state.db, &owner).await?;
            Some(owner)
        } else {
            Some(auth.id.clone())
        }
    } else {
        None
    };

    let mut birthday_year = if body.birthday_year.is_some() {
        validate_birthday_year(body.birthday_year)?
    } else {
        existing.birthday_year
    };

    if event_type == "birthday" {
        recurrence = "yearly";
        if birthday_year.is_none() {
            return Err(ApiError::validation(serde_json::json!({
                "birthday_year": ["is required for birthday events"]
            }))
            .into());
        }
    } else {
        birthday_year = None;
    }

    let updated = db_update_event(
        &state.db,
        &event_id,
        &UpdateCalendarEvent {
            scope: scope.to_string(),
            owner_user_id,
            title,
            description,
            event_date: format_ymd(event_date),
            event_type: event_type.to_string(),
            recurrence: recurrence.to_string(),
            birthday_year,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !updated {
        return Err(ApiError::NotFound(format!("event {event_id} not found")).into());
    }

    let event = db_get_event(&state.db, &event_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("event {event_id} not found")))?;
    let derived_age = event
        .birthday_year
        .filter(|_| event.event_type == "birthday")
        .map(|birth_year| event_date.year() - birth_year);
    Ok(Json(to_response(&event, &auth, event_date, derived_age)))
}

async fn delete_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(event_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let auth = authenticate(&state, &headers).await?;
    let existing = db_get_event(&state.db, &event_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("event {event_id} not found")))?;

    if !can_manage_event(&auth, &existing) {
        return Err(ApiError::Forbidden("you cannot delete this event".to_string()).into());
    }

    let deleted = db_delete_event(&state.db, &event_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !deleted {
        return Err(ApiError::NotFound(format!("event {event_id} not found")).into());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("database check failed: {e}")))?;
    Ok(Json(HealthResponse { status: "ok" }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let db_target = std::env::var("RUSTFIN_DATABASE_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "postgresql://rustfin:rustfin@postgres:5432/rustfin".to_string());
    let db_target_lc = db_target.to_ascii_lowercase();
    if !db_target_lc.starts_with("postgres://") && !db_target_lc.starts_with("postgresql://") {
        bail!(
            "RUSTFIN_DATABASE_URL must be a PostgreSQL URL (postgres:// or postgresql://); non-PostgreSQL targets are not supported"
        );
    }
    let auth_base_url = std::env::var("RUSTFIN_AUTH_BASE_URL")
        .unwrap_or_else(|_| "http://rustfin:8096".to_string());
    let bind_addr =
        std::env::var("RUSTFIN_CALENDAR_BIND").unwrap_or_else(|_| "0.0.0.0:8099".to_string());

    let db = rustfin_db::connect(&db_target)
        .await
        .context("failed to connect to database")?;
    let db_backend = rustfin_db::DatabaseBackend::Postgres;
    let run_migrations = env_bool_with_fallback(
        "RUSTFIN_CALENDAR_RUN_MIGRATIONS",
        "RUSTFIN_RUN_MIGRATIONS",
        true,
    );
    if run_migrations {
        rustfin_db::migrate::run(&db, db_backend)
            .await
            .context("failed to run migrations")?;
    } else {
        warn!(
            "RUSTFIN_CALENDAR_RUN_MIGRATIONS disabled for calendar service; assuming schema is pre-migrated"
        );
    }

    let state = AppState {
        db,
        auth_base_url,
        http_client: reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("failed to initialize http client")?,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/api/v1/calendar/events",
            get(list_events).post(create_event),
        )
        .route(
            "/api/v1/calendar/events/personal",
            get(list_personal_events_admin),
        )
        .route(
            "/api/v1/calendar/events/{event_id}",
            patch(update_event).delete(delete_event),
        )
        .route("/api/v1/calendar/users", get(list_users))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .context("failed to bind calendar listener")?;
    info!(addr = %bind_addr, "calendar service listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HealthResponse, env_bool_with_fallback};
    use serde_json::json;

    #[test]
    fn health_response_serializes_minimally() {
        let payload = serde_json::to_value(HealthResponse { status: "ok" }).unwrap();
        assert_eq!(payload, json!({ "status": "ok" }));
    }

    #[test]
    fn service_specific_migration_flag_overrides_global_value() {
        // SAFETY: this test mutates process env only for the duration of the assertion.
        unsafe {
            std::env::remove_var("RUSTFIN_CALENDAR_RUN_MIGRATIONS");
            std::env::remove_var("RUSTFIN_RUN_MIGRATIONS");
            std::env::set_var("RUSTFIN_RUN_MIGRATIONS", "true");
            std::env::set_var("RUSTFIN_CALENDAR_RUN_MIGRATIONS", "false");
        }

        assert!(!env_bool_with_fallback(
            "RUSTFIN_CALENDAR_RUN_MIGRATIONS",
            "RUSTFIN_RUN_MIGRATIONS",
            true,
        ));

        // SAFETY: restore process env after the test.
        unsafe {
            std::env::remove_var("RUSTFIN_CALENDAR_RUN_MIGRATIONS");
            std::env::remove_var("RUSTFIN_RUN_MIGRATIONS");
        }
    }
}
