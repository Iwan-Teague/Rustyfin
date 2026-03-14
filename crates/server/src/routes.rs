use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, FromRequestParts, Multipart, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
#[cfg(feature = "rustyvault")]
use axum::middleware::from_fn_with_state;
#[cfg(not(feature = "rustyvault"))]
use axum::response::IntoResponse;
use axum::response::Response;
#[cfg(not(feature = "rustyvault"))]
use axum::routing::any;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::path::{Path as StdPath, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;
use tracing::warn;

use crate::account_prefs::UserPreferences;
use crate::auth::{
    AdminUser, AuthUser, issue_stream_token, issue_token, validate_stream_token, validate_token,
};
use crate::error::AppError;
use crate::host_directories::{HostDirectoryListResponse, build_host_directory_listing};
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;
use crate::user_activity::{self, ActivityRange, BrowserActivityEventRequest};
use crate::user_pipeline;

const DEFAULT_STREAM_TOKEN_TTL_SECONDS: i64 = 6 * 60 * 60;
const MAX_AVATAR_UPLOAD_BYTES: usize = 5 * 1024 * 1024;
const USER_AVATAR_DIR: &str = "user_avatars";
const YOUTUBE_AGENT_IMPORT_TIMEOUT_SECONDS: u64 = 240;
const MAX_MUSIC_METADATA_LEN: usize = 180;
const LOGIN_INITIAL_ATTEMPTS: u64 = 5;
const LOGIN_COOLDOWN_SECONDS: u64 = 30;
const LOGIN_COOLDOWN_ATTEMPTS: u64 = 2;
const LOGIN_BUCKET_IDLE_TTL_SECONDS: u64 = 30 * 60;
const CONTINUE_WATCHING_MIN_PROGRESS_MS: i64 = 30_000;
const CONTINUE_WATCHING_PERCENT_NUMERATOR: i64 = 5;
const CONTINUE_WATCHING_PERCENT_DENOMINATOR: i64 = 100;
const PASSWORD_CHANGE_ATTEMPTS: u64 = 8;
const PASSWORD_CHANGE_WINDOW_SECONDS: u64 = 15 * 60;
const DOWNLOAD_TRANSCODE_STARTUP_POLL_MS: u64 = 100;
const DOWNLOAD_TRANSCODE_STARTUP_TIMEOUT_MS: u64 = 2500;
const DOWNLOAD_TRANSCODE_STDERR_TAIL_LINES: usize = 8;

#[cfg(feature = "rustyvault")]
fn mounted_rustyvault_router(state: AppState) -> Router<AppState> {
    crate::rustyvault_host::router::rustyvault_router().layer(from_fn_with_state(
        state,
        crate::rustyvault_host::middleware::rustyvault_availability_middleware,
    ))
}

#[cfg(not(feature = "rustyvault"))]
fn mounted_rustyvault_router(_state: AppState) -> Router<AppState> {
    async fn rustyvault_unavailable() -> impl IntoResponse {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "service_unavailable",
                    "message": "RustyVault is disabled on this host."
                }
            })),
        )
    }

    Router::new().fallback(any(rustyvault_unavailable))
}

#[derive(Debug, Clone)]
struct LoginAttemptBucket {
    remaining_attempts: u64,
    cooldown_until: Option<Instant>,
    last_seen: Instant,
}

static LOGIN_ATTEMPT_BUCKETS: LazyLock<tokio::sync::Mutex<HashMap<String, LoginAttemptBucket>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy)]
struct MaybeConnectInfo(Option<ConnectInfo<std::net::SocketAddr>>);

impl<S> FromRequestParts<S> for MaybeConnectInfo
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<std::net::SocketAddr>>()
                .copied(),
        ))
    }
}

fn extract_login_client_identity(
    connect_info: Option<&ConnectInfo<std::net::SocketAddr>>,
    headers: &HeaderMap,
) -> String {
    if let Some(ci) = connect_info {
        return format!("peer:{}", ci.0.ip());
    }

    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| format!("xff:{v}"))
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| format!("xreal:{v}"))
        })
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| format!("host:{v}"))
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn login_attempt_key(
    connect_info: Option<&ConnectInfo<std::net::SocketAddr>>,
    headers: &HeaderMap,
    username: &str,
) -> String {
    format!(
        "{}|{}",
        username.trim().to_ascii_lowercase(),
        extract_login_client_identity(connect_info, headers)
    )
}

fn cooldown_retry_after_seconds(now: Instant, until: Instant) -> u64 {
    if until <= now {
        return 0;
    }
    let remaining = until.duration_since(now);
    if remaining.subsec_nanos() > 0 {
        remaining.as_secs() + 1
    } else {
        remaining.as_secs()
    }
}

async fn enforce_login_rate_limit(
    connect_info: Option<&ConnectInfo<std::net::SocketAddr>>,
    headers: &HeaderMap,
    username: &str,
) -> Result<String, AppError> {
    let key = login_attempt_key(connect_info, headers, username);
    let now = Instant::now();
    let mut guard = LOGIN_ATTEMPT_BUCKETS.lock().await;

    // Trim stale entries so this map does not grow forever.
    let ttl = Duration::from_secs(LOGIN_BUCKET_IDLE_TTL_SECONDS);
    guard.retain(|_, bucket| now.duration_since(bucket.last_seen) <= ttl);

    let bucket = guard
        .entry(key.clone())
        .or_insert_with(|| LoginAttemptBucket {
            remaining_attempts: LOGIN_INITIAL_ATTEMPTS,
            cooldown_until: None,
            last_seen: now,
        });

    if let Some(cooldown_until) = bucket.cooldown_until {
        if now < cooldown_until {
            let retry_after = cooldown_retry_after_seconds(now, cooldown_until);
            return Err(ApiError::TooManyRequests {
                retry_after_seconds: retry_after.max(1),
            }
            .into());
        }

        // Cooldown expired: allow two attempts in the next window.
        bucket.cooldown_until = None;
        bucket.remaining_attempts = LOGIN_COOLDOWN_ATTEMPTS;
    }

    if bucket.remaining_attempts == 0 {
        let cooldown_until = now + Duration::from_secs(LOGIN_COOLDOWN_SECONDS);
        bucket.cooldown_until = Some(cooldown_until);
        bucket.last_seen = now;
        return Err(ApiError::TooManyRequests {
            retry_after_seconds: LOGIN_COOLDOWN_SECONDS,
        }
        .into());
    }

    bucket.remaining_attempts -= 1;
    bucket.last_seen = now;
    Ok(key)
}

async fn reset_login_rate_limit(key: &str) {
    let mut guard = LOGIN_ATTEMPT_BUCKETS.lock().await;
    guard.remove(key);
}

fn stream_token_ttl_seconds() -> i64 {
    std::env::var("RUSTFIN_STREAM_TOKEN_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v >= 300)
        .unwrap_or(DEFAULT_STREAM_TOKEN_TTL_SECONDS)
}

#[derive(Debug, Clone)]
struct StreamRequestIdentity {
    user_id: String,
    role: String,
    stream_claims: Option<crate::auth::StreamClaims>,
}

fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::to_string)
}

fn resolve_stream_request_identity(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    stream_token: Option<&str>,
) -> Result<StreamRequestIdentity, AppError> {
    if let Some(token) = extract_bearer_token(headers) {
        let claims = validate_token(&token, &state.jwt_secret)?;
        return Ok(StreamRequestIdentity {
            user_id: claims.sub,
            role: claims.role,
            stream_claims: None,
        });
    }

    let st = stream_token.ok_or_else(|| {
        ApiError::Unauthorized("missing authorization header or stream token".into())
    })?;
    let claims = validate_stream_token(st, &state.jwt_secret)?;
    Ok(StreamRequestIdentity {
        user_id: claims.sub.clone(),
        role: claims.role.clone(),
        stream_claims: Some(claims),
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api_router(state.clone()))
        .nest("/stream", stream_router())
        .with_state(state)
}

fn stream_router() -> Router<AppState> {
    Router::new()
        .route("/file/{file_id}", get(crate::streaming::stream_file_range))
        .route("/hls/{sid}/master.m3u8", get(hls_master))
        .route("/hls/{sid}/{filename}", get(hls_segment))
        .route("/subtitles/{sub_path}", get(serve_subtitle))
}

fn api_router(state: AppState) -> Router<AppState> {
    Router::new()
        // Public system info (unauthenticated)
        .route(
            "/system/info/public",
            get(crate::setup::handlers::get_public_system_info),
        )
        // Setup routes
        .nest("/setup", setup_router())
        .route("/auth/login", post(auth_login))
        .route("/users", post(create_user_route).get(list_users_route))
        .route(
            "/users/{id}",
            axum::routing::delete(delete_user_route).patch(update_user_route),
        )
        .route("/users/me", get(users_me))
        .route(
            "/users/me/profile",
            get(get_my_profile).patch(update_my_profile),
        )
        .route("/users/me/password", post(change_my_password))
        .route(
            "/users/me/avatar",
            post(upload_my_avatar).delete(delete_my_avatar),
        )
        .route("/users/avatar/{id}", get(download_user_avatar))
        .route("/users/me/preferences", get(get_prefs).patch(update_prefs))
        .route(
            "/users/me/activity",
            get(get_my_activity_summary).delete(delete_my_activity),
        )
        .route("/users/me/activity/browser", post(post_browser_activity))
        .route(
            "/downloads/catalog",
            get(crate::downloads::get_download_catalog),
        )
        .route(
            "/downloads/artifacts/{artifact_id}/package",
            get(crate::downloads::download_artifact_package),
        )
        // Libraries
        .route("/libraries", post(create_library).get(list_libraries))
        .route(
            "/libraries/{id}",
            get(get_library)
                .patch(update_library)
                .delete(delete_library),
        )
        .route("/libraries/{id}/scan", post(scan_library))
        .route("/libraries/{id}/tmdb-sync", post(sync_library_tmdb))
        .route("/libraries/{id}/items", get(list_library_items))
        .route(
            "/libraries/{id}/music/import-youtube",
            post(import_library_music_from_youtube),
        )
        // Items
        .route("/items/{id}", get(get_item))
        .route("/items/{id}/playback", get(get_item_playback))
        .route("/items/{id}/children", get(get_item_children))
        .route("/items/{id}/subtitles", get(get_item_subtitles))
        .route("/items/{id}/images/{img_type}", get(get_item_image))
        .route("/items/{id}/metadata/refresh", post(refresh_item_metadata))
        .route("/items/{id}/providers", get(get_item_providers))
        .route(
            "/items/{id}/field-locks",
            post(lock_item_field).delete(unlock_item_field),
        )
        // TV expected episodes
        .route("/items/{id}/expected-episodes", get(get_expected_episodes))
        .route("/items/{id}/missing-episodes", get(get_missing_episodes))
        // Playback
        .route("/playback/progress", post(update_progress))
        .route("/playback/continue", get(list_continue_watching))
        .route("/playback/state/{item_id}", get(get_play_state))
        .route("/playback/sessions", post(create_playback_session))
        .route("/playback/sessions/{sid}/stop", post(stop_playback_session))
        .route("/playback/info/{file_id}", get(get_media_info))
        .route("/playback/download/{file_id}", get(download_playback_media))
        .route("/system/host-directories", get(list_host_directories))
        .route("/system/pick-directory", post(pick_directory))
        .route("/system/gpu", get(get_gpu_caps))
        .route("/system/tmdb", get(get_tmdb_config).put(update_tmdb_config))
        .route("/system/runtime-diagnostics", get(get_runtime_diagnostics))
        .nest("/vault", mounted_rustyvault_router(state))
        .nest("/servers", crate::servers::router::servers_router())
        .nest(
            "/watch-party",
            crate::watch_party::router::watch_party_router(),
        )
        .nest("/channels", crate::channels::router::channels_router())
        .route("/events", get(sse_events))
        // Jobs
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job).delete(delete_job))
        .route("/jobs/{id}/cancel", post(cancel_job))
}

fn setup_router() -> Router<AppState> {
    let rate_limiter = RateLimiter::new(30, 60); // 30 requests per 60s window
    Router::new()
        .route(
            "/session/claim",
            post(crate::setup::handlers::claim_session),
        )
        .route(
            "/session/release",
            post(crate::setup::handlers::release_session),
        )
        .route(
            "/config",
            get(crate::setup::handlers::get_setup_config)
                .put(crate::setup::handlers::put_setup_config),
        )
        .route("/admin", post(crate::setup::handlers::create_admin))
        .route(
            "/paths/validate",
            post(crate::setup::handlers::validate_path),
        )
        .route(
            "/host-directories",
            get(crate::setup::handlers::list_host_directories),
        )
        .route("/libraries", post(crate::setup::handlers::create_libraries))
        .route(
            "/metadata",
            get(crate::setup::handlers::get_setup_metadata)
                .put(crate::setup::handlers::put_setup_metadata),
        )
        .route(
            "/network",
            get(crate::setup::handlers::get_setup_network)
                .put(crate::setup::handlers::put_setup_network),
        )
        .route("/complete", post(crate::setup::handlers::complete_setup))
        .route("/reset", post(crate::setup::handlers::reset_setup))
        .layer(axum::middleware::from_fn(
            crate::setup::rate_limit::rate_limit_middleware,
        ))
        .layer(Extension(rate_limiter))
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("database check failed: {e}")))?;

    Ok(Json(HealthResponse {
        status: "ok".to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user_id: String,
    username: String,
    role: String,
}

async fn auth_login(
    State(state): State<AppState>,
    MaybeConnectInfo(connect_info): MaybeConnectInfo,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let login_limit_key =
        enforce_login_rate_limit(connect_info.as_ref(), &headers, &body.username).await?;

    let user = rustfin_db::repo::users::find_by_username(&state.db, &body.username)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("invalid credentials".into()))?;

    let valid = rustfin_db::repo::users::verify_password(&body.password, &user.password_hash)
        .map_err(|e| ApiError::Internal(format!("hash error: {e}")))?;

    if !valid {
        return Err(ApiError::Unauthorized("invalid credentials".into()).into());
    }

    let token = issue_token(&user.id, &user.username, &user.role, &state.jwt_secret)?;
    reset_login_rate_limit(&login_limit_key).await;

    Ok(Json(LoginResponse {
        token,
        user_id: user.id,
        username: user.username,
        role: user.role,
    }))
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct UserMeResponse {
    id: String,
    username: String,
    login_username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    role: String,
}

#[derive(Serialize)]
struct MyProfileResponse {
    id: String,
    username: String,
    login_username: String,
    role: String,
    created_ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time_zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct UpdateMyProfileRequest {
    display_name: String,
    #[serde(default)]
    time_zone: Option<String>,
}

#[derive(Deserialize)]
struct ChangeMyPasswordRequest {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

#[derive(Serialize)]
struct ChangeMyPasswordResponse {
    ok: bool,
    relogin_required: bool,
}

#[derive(Default, Deserialize)]
struct ActivitySummaryQuery {
    range: Option<String>,
}

fn avatar_url_for_user(user_id: &str, avatar_path: Option<&str>) -> Option<String> {
    avatar_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|_| format!("/api/v1/users/avatar/{user_id}"))
}

fn normalize_display_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() < 2 || collapsed.chars().count() > 40 {
        return None;
    }
    Some(collapsed)
}

fn avatar_kind_from(
    file_name: Option<&str>,
    content_type: Option<&str>,
) -> Option<(&'static str, &'static str)> {
    let normalized_ct = content_type
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match normalized_ct.as_str() {
        "image/jpeg" | "image/jpg" => return Some(("jpg", "image/jpeg")),
        "image/png" => return Some(("png", "image/png")),
        "image/webp" => return Some(("webp", "image/webp")),
        "image/gif" => return Some(("gif", "image/gif")),
        _ => {}
    }

    let ext = file_name
        .and_then(|value| StdPath::new(value).extension().and_then(|v| v.to_str()))
        .map(|value| value.to_ascii_lowercase())?;
    match ext.as_str() {
        "jpg" | "jpeg" => Some(("jpg", "image/jpeg")),
        "png" => Some(("png", "image/png")),
        "webp" => Some(("webp", "image/webp")),
        "gif" => Some(("gif", "image/gif")),
        _ => None,
    }
}

fn profile_to_response(row: &rustfin_db::repo::users::UserRow) -> MyProfileResponse {
    MyProfileResponse {
        id: row.id.clone(),
        username: row.display_name.clone(),
        login_username: row.username.clone(),
        role: row.role.clone(),
        created_ts: row.created_ts,
        time_zone: row.time_zone.clone(),
        avatar_url: avatar_url_for_user(&row.id, row.avatar_path.as_deref()),
    }
}

fn normalize_time_zone(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<chrono_tz::Tz>()
        .map(|_| Some(trimmed.to_string()))
        .map_err(|_| ApiError::BadRequest("time_zone must be a valid IANA time zone".into()).into())
}

fn validate_password_only(password: &str) -> Result<(), AppError> {
    if let Some(fields) = user_pipeline::validate_username_password("valid_user", password) {
        if let Some(message) = fields
            .get("password")
            .and_then(|value| value.as_array())
            .and_then(|values| values.first())
            .and_then(|value| value.as_str())
        {
            return Err(ApiError::BadRequest(message.to_string()).into());
        }
    }
    Ok(())
}

async fn users_me(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<UserMeResponse>, AppError> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    Ok(Json(UserMeResponse {
        id: user.id.clone(),
        username: user.display_name,
        login_username: user.username,
        time_zone: user.time_zone,
        avatar_url: avatar_url_for_user(&user.id, user.avatar_path.as_deref()),
        role: user.role,
    }))
}

async fn get_my_profile(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<MyProfileResponse>, AppError> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    Ok(Json(profile_to_response(&user)))
}

async fn update_my_profile(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateMyProfileRequest>,
) -> Result<Json<MyProfileResponse>, AppError> {
    let display_name = normalize_display_name(&body.display_name).ok_or_else(|| {
        ApiError::BadRequest("display_name must be between 2 and 40 characters".into())
    })?;
    let time_zone = normalize_time_zone(body.time_zone.as_deref())?;
    rustfin_db::repo::users::update_profile(
        &state.db,
        &auth.user_id,
        &display_name,
        time_zone.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    Ok(Json(profile_to_response(&user)))
}

fn password_change_limiter() -> &'static RateLimiter {
    static LIMITER: std::sync::OnceLock<RateLimiter> = std::sync::OnceLock::new();
    LIMITER
        .get_or_init(|| RateLimiter::new(PASSWORD_CHANGE_ATTEMPTS, PASSWORD_CHANGE_WINDOW_SECONDS))
}

async fn change_my_password(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ChangeMyPasswordRequest>,
) -> Result<Json<ChangeMyPasswordResponse>, AppError> {
    let key = format!("password-change:{}", auth.user_id);
    password_change_limiter()
        .check(&key)
        .await
        .map_err(|retry_after| ApiError::TooManyRequests {
            retry_after_seconds: retry_after,
        })?;

    if body.new_password != body.confirm_password {
        return Err(ApiError::BadRequest("new password and confirmation must match".into()).into());
    }
    if body.current_password == body.new_password {
        return Err(
            ApiError::BadRequest("new password must differ from current password".into()).into(),
        );
    }
    validate_password_only(&body.new_password)?;

    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;

    let valid =
        rustfin_db::repo::users::verify_password(&body.current_password, &user.password_hash)
            .map_err(|e| ApiError::Internal(format!("hash error: {e}")))?;
    if !valid {
        return Err(ApiError::Unauthorized("current password is incorrect".into()).into());
    }

    rustfin_db::repo::users::update_password(&state.db, &auth.user_id, &body.new_password)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(ChangeMyPasswordResponse {
        ok: true,
        relogin_required: true,
    }))
}

async fn upload_my_avatar(
    auth: AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<MyProfileResponse>, AppError> {
    let mut file_name: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid multipart form: {e}")))?
    {
        if field.name().unwrap_or_default() != "file" || bytes.is_some() {
            continue;
        }
        file_name = field.file_name().map(|value| value.to_string());
        content_type = field.content_type().map(|value| value.to_string());
        let payload = field
            .bytes()
            .await
            .map_err(|e| ApiError::BadRequest(format!("invalid avatar upload: {e}")))?;
        if payload.is_empty() {
            return Err(ApiError::BadRequest("uploaded avatar is empty".into()).into());
        }
        if payload.len() > MAX_AVATAR_UPLOAD_BYTES {
            return Err(ApiError::BadRequest("avatar exceeds 5MB size limit".into()).into());
        }
        bytes = Some(payload.to_vec());
    }

    let payload =
        bytes.ok_or_else(|| ApiError::BadRequest("multipart form requires file".into()))?;
    let (ext, normalized_content_type) =
        avatar_kind_from(file_name.as_deref(), content_type.as_deref()).ok_or_else(|| {
            ApiError::BadRequest("avatar must be a jpg, png, webp, or gif image".into())
        })?;
    let avatar_dir = state.cache_dir.join(USER_AVATAR_DIR);
    fs::create_dir_all(&avatar_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to create avatar directory: {e}")))?;

    let target_path = avatar_dir.join(format!("{}.{}", auth.user_id, ext));
    fs::write(&target_path, payload)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to save avatar: {e}")))?;

    let existing = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    if let Some(old_path) = existing
        .avatar_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let old = std::path::PathBuf::from(old_path);
        if old != target_path && old.starts_with(&avatar_dir) {
            if let Err(err) = fs::remove_file(&old).await {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!(path = %old.display(), error = %err, "failed removing old avatar file");
                }
            }
        }
    }

    rustfin_db::repo::users::update_avatar(
        &state.db,
        &auth.user_id,
        Some(target_path.to_string_lossy().as_ref()),
        Some(normalized_content_type),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    Ok(Json(profile_to_response(&user)))
}

async fn delete_my_avatar(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<MyProfileResponse>, AppError> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    if let Some(path) = user
        .avatar_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let avatar_dir = state.cache_dir.join(USER_AVATAR_DIR);
        let target = std::path::PathBuf::from(path);
        if target.starts_with(&avatar_dir) {
            if let Err(err) = fs::remove_file(&target).await {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!(path = %target.display(), error = %err, "failed removing avatar file");
                }
            }
        }
    }
    rustfin_db::repo::users::update_avatar(&state.db, &auth.user_id, None, None)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let updated = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Unauthorized("user not found".into()))?;
    Ok(Json(profile_to_response(&updated)))
}

async fn download_user_avatar(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Response, AppError> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))?;
    let avatar_path = user
        .avatar_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::NotFound("avatar not found".into()))?;
    let avatar_dir = state.cache_dir.join(USER_AVATAR_DIR);
    let avatar_path_buf = std::path::PathBuf::from(avatar_path);
    if !avatar_path_buf.starts_with(&avatar_dir) {
        return Err(ApiError::Forbidden("avatar path is outside allowed scope".into()).into());
    }
    let data = fs::read(&avatar_path_buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::NotFound("avatar not found".into())
        } else {
            ApiError::Internal(format!("failed reading avatar: {e}"))
        }
    })?;
    let content_type = user
        .avatar_content_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("application/octet-stream");

    let mut response = Response::new(Body::from(data));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    Ok(response)
}

// ---------------------------------------------------------------------------
// User management (admin)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
    #[serde(default = "default_user_role")]
    role: String,
    #[serde(default)]
    library_ids: Vec<String>,
}

fn default_user_role() -> String {
    "user".to_string()
}

#[derive(Serialize)]
struct CreateUserResponse {
    id: String,
    username: String,
    role: String,
    library_ids: Vec<String>,
}

fn normalize_library_ids(ids: &[String]) -> Vec<String> {
    user_pipeline::normalize_library_ids(ids)
}

async fn validate_library_ids_exist(
    state: &AppState,
    library_ids: &[String],
) -> Result<(), AppError> {
    user_pipeline::validate_library_ids_exist(state, library_ids).await
}

async fn ensure_library_access(
    auth: &AuthUser,
    state: &AppState,
    library_id: &str,
) -> Result<(), AppError> {
    if auth.role == "admin" {
        return Ok(());
    }
    let allowed = rustfin_db::repo::users::is_library_allowed(&state.db, &auth.user_id, library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !allowed {
        return Err(ApiError::Forbidden("library access denied".into()).into());
    }
    Ok(())
}

async fn create_user_route(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<CreateUserResponse>, AppError> {
    let role = body.role.clone();
    let library_ids = user_pipeline::normalize_library_ids(&body.library_ids);
    let id = user_pipeline::create_user_with_access(
        &state,
        &body.username,
        &body.password,
        &role,
        &library_ids,
    )
    .await?;

    crate::audit_log::record_event(
        &state,
        "admin.users.create",
        serde_json::json!({
            "scope": "users",
            "action": "create",
            "admin_user_id": admin.user_id,
            "user_id": id,
            "username": body.username,
            "role": role,
            "library_ids": if role == "user" { library_ids.clone() } else { Vec::<String>::new() },
        }),
    )
    .await;

    Ok(Json(CreateUserResponse {
        id,
        username: body.username,
        role: role.clone(),
        library_ids: if role == "user" { library_ids } else { vec![] },
    }))
}

#[derive(Serialize)]
struct UserListItem {
    id: String,
    username: String,
    role: String,
    created_ts: i64,
    library_ids: Vec<String>,
}

async fn list_users_route(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserListItem>>, AppError> {
    let users = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let user_ids: Vec<String> = users
        .iter()
        .filter(|user| user.role == "user")
        .map(|user| user.id.clone())
        .collect();
    let mut library_ids_by_user: HashMap<String, Vec<String>> = HashMap::new();
    let access_rows = rustfin_db::repo::users::list_library_access_for_users(&state.db, &user_ids)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    for (user_id, library_id) in access_rows {
        library_ids_by_user
            .entry(user_id)
            .or_default()
            .push(library_id);
    }

    let out = users
        .into_iter()
        .map(|user| UserListItem {
            library_ids: if user.role == "user" {
                library_ids_by_user.remove(&user.id).unwrap_or_default()
            } else {
                vec![]
            },
            id: user.id,
            username: user.username,
            role: user.role,
            created_ts: user.created_ts,
        })
        .collect();
    Ok(Json(out))
}

#[derive(Deserialize)]
struct UpdateUserRequest {
    role: Option<String>,
    library_ids: Option<Vec<String>>,
}

#[derive(Serialize)]
struct UpdateUserResponse {
    id: String,
    username: String,
    role: String,
    library_ids: Vec<String>,
}

async fn update_user_route(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UpdateUserResponse>, AppError> {
    let existing = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    let target_role = body.role.unwrap_or_else(|| existing.role.clone());
    if target_role != "admin" && target_role != "user" {
        return Err(ApiError::BadRequest("role must be 'admin' or 'user'".into()).into());
    }
    if admin.user_id == user_id && target_role != "admin" {
        return Err(ApiError::BadRequest("cannot remove your own admin role".into()).into());
    }

    let requested_library_ids = body
        .library_ids
        .as_ref()
        .map(|v| normalize_library_ids(v))
        .unwrap_or_default();
    validate_library_ids_exist(&state, &requested_library_ids).await?;

    if existing.role != target_role {
        rustfin_db::repo::users::update_user_role(&state.db, &user_id, &target_role)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    let final_library_ids = if target_role == "user" {
        let final_ids = if body.library_ids.is_some() {
            requested_library_ids
        } else {
            rustfin_db::repo::users::get_library_access(&state.db, &user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        };
        rustfin_db::repo::users::set_library_access(&state.db, &user_id, &final_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        final_ids
    } else {
        rustfin_db::repo::users::set_library_access(&state.db, &user_id, &[])
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        vec![]
    };

    let updated = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

    Ok(Json(UpdateUserResponse {
        id: updated.id,
        username: updated.username,
        role: updated.role,
        library_ids: final_library_ids,
    }))
}

async fn delete_user_route(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if user_id == admin.user_id {
        return Err(ApiError::BadRequest("cannot delete yourself".into()).into());
    }
    let deleted = rustfin_db::repo::users::delete_user(&state.db, &user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !deleted {
        return Err(ApiError::NotFound("user not found".into()).into());
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

async fn get_prefs(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<UserPreferences>, AppError> {
    Ok(Json(
        user_activity::load_preferences(&state, &auth.user_id).await?,
    ))
}

async fn update_prefs(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<UserPreferences>,
) -> Result<Json<UserPreferences>, AppError> {
    let prefs = body.normalized();
    user_activity::save_preferences(&state, &auth.user_id, &prefs).await?;
    Ok(Json(prefs))
}

async fn post_browser_activity(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<BrowserActivityEventRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    user_activity::handle_browser_event(&state, &auth.user_id, &body).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_my_activity_summary(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ActivitySummaryQuery>,
) -> Result<Json<user_activity::ActivitySummaryResponse>, AppError> {
    let prefs = user_activity::load_preferences(&state, &auth.user_id).await?;
    let range = query
        .range
        .as_deref()
        .map(ActivityRange::from_raw)
        .unwrap_or_else(|| ActivityRange::from_raw(&prefs.activity.default_range));
    Ok(Json(
        user_activity::summarize_user_activity(&state, &auth.user_id, range).await?,
    ))
}

async fn delete_my_activity(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    user_activity::clear_user_history(&state, &auth.user_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
struct LibrarySettingsPatchRequest {
    show_images: Option<bool>,
    prefer_local_artwork: Option<bool>,
    fetch_online_artwork: Option<bool>,
    tmdb_store_in_media_dir: Option<bool>,
    tmdb_sync_on_new_media: Option<bool>,
    tmdb_sync_schedule: Option<String>,
    tmdb_fetch_posters: Option<bool>,
    tmdb_fetch_backdrops: Option<bool>,
    tmdb_fetch_metadata: Option<bool>,
    tmdb_fetch_reviews: Option<bool>,
}

#[derive(Deserialize)]
struct CreateLibraryRequest {
    name: String,
    kind: String,
    paths: Vec<String>,
    #[serde(default)]
    settings: LibrarySettingsPatchRequest,
}

#[derive(Serialize)]
struct LibrarySettingsResponse {
    show_images: bool,
    prefer_local_artwork: bool,
    fetch_online_artwork: bool,
    tmdb_store_in_media_dir: bool,
    tmdb_sync_on_new_media: bool,
    tmdb_sync_schedule: String,
    tmdb_last_sync_ts: Option<i64>,
    tmdb_fetch_posters: bool,
    tmdb_fetch_backdrops: bool,
    tmdb_fetch_metadata: bool,
    tmdb_fetch_reviews: bool,
}

#[derive(Serialize)]
struct LibraryResponse {
    id: String,
    name: String,
    kind: String,
    paths: Vec<LibraryPathResponse>,
    settings: LibrarySettingsResponse,
    item_count: i64,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Serialize)]
struct LibraryPathResponse {
    id: String,
    path: String,
    is_read_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportLibraryMusicFromYoutubeRequest {
    source: String,
    artist: String,
    #[serde(default)]
    album: Option<String>,
    title: String,
}

#[derive(Debug, Serialize)]
struct ImportLibraryMusicFromYoutubeResponse {
    library_id: String,
    video_id: String,
    artist: String,
    album: String,
    title: String,
    file_path: String,
    duration_ms: Option<u64>,
    scan_job: JobResponse,
}

#[derive(Debug)]
struct DownloadedLibraryImportAudio {
    file_path: PathBuf,
    duration_ms: Option<u64>,
    video_id: String,
    import_scope_id: String,
}

#[derive(Debug, Serialize)]
struct YouTubeAgentLibraryImportRequest<'a> {
    room_id: &'a str,
    video_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct YouTubeAgentLibraryImportResponse {
    file_path: String,
    duration_ms: Option<u64>,
}

fn path_contains_component(path: &StdPath, component: &str) -> bool {
    let expected = std::ffi::OsStr::new(component);
    path.components().any(|part| part.as_os_str() == expected)
}

fn normalize_music_segment(raw: &str, field: &str) -> Result<String, AppError> {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        if ch.is_control() {
            continue;
        }
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }

    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let out = out.trim_matches([' ', '.']).to_string();
    if out.is_empty() {
        return Err(
            ApiError::BadRequest(format!("{field} is required and must not be empty")).into(),
        );
    }
    if out == "." || out == ".." {
        return Err(ApiError::BadRequest(format!("{field} must be a valid name")).into());
    }
    if out.chars().count() > MAX_MUSIC_METADATA_LEN {
        return Err(ApiError::BadRequest(format!(
            "{field} must be <= {MAX_MUSIC_METADATA_LEN} characters"
        ))
        .into());
    }
    Ok(out)
}

fn strip_known_youtube_title_suffixes(raw: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "(official video)",
        "(official music video)",
        "(official audio)",
        "(audio)",
        "(lyrics)",
        "[official video]",
        "[official music video]",
        "[official audio]",
        "[audio]",
        "[lyrics]",
    ];

    let mut out = raw.trim().to_string();
    loop {
        let lowered = out.to_ascii_lowercase();
        let mut removed = false;
        for suffix in SUFFIXES {
            if lowered.ends_with(suffix) {
                let keep_len = out.len().saturating_sub(suffix.len());
                out = out[..keep_len].trim_end().to_string();
                removed = true;
                break;
            }
        }
        if !removed {
            break;
        }
    }
    out
}

fn strip_artist_prefix(title: &str, artist: &str) -> String {
    let lowered_title = title.to_ascii_lowercase();
    let lowered_artist = artist.to_ascii_lowercase();
    for sep in [" - ", " – ", " — ", ": "] {
        let prefix = format!("{lowered_artist}{sep}");
        if lowered_title.starts_with(&prefix) {
            let skip = artist.chars().count() + sep.chars().count();
            return title
                .chars()
                .skip(skip)
                .collect::<String>()
                .trim()
                .to_string();
        }
    }
    title.to_string()
}

fn normalize_track_title(raw: &str, artist: &str) -> Result<String, AppError> {
    let without_ext = raw.trim().trim_end_matches(".mp3");
    let stripped_suffixes = strip_known_youtube_title_suffixes(without_ext);
    let without_artist = strip_artist_prefix(&stripped_suffixes, artist);
    normalize_music_segment(&without_artist, "title")
}

fn normalize_album(raw: Option<&str>) -> Result<String, AppError> {
    match raw {
        Some(value) if !value.trim().is_empty() => normalize_music_segment(value, "album"),
        _ => Ok("Singles".to_string()),
    }
}

fn choose_music_library_root(
    paths: &[rustfin_db::repo::libraries::LibraryPathRow],
) -> Result<PathBuf, AppError> {
    for path in paths {
        let candidate = StdPath::new(path.path.trim());
        if candidate.is_absolute() && candidate.exists() && candidate.is_dir() {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(ApiError::BadRequest(
        "music library does not have a usable directory path on this server".into(),
    )
    .into())
}

fn normalize_import_scope_id(library_id: &str) -> String {
    let clean_library_id: String = library_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let mut scope = format!("libimp_{clean_library_id}_{}", &unique[..10]);
    if scope.len() > 128 {
        scope.truncate(128);
    }
    if !scope
        .chars()
        .next()
        .map(|ch| ch.is_ascii_alphanumeric())
        .unwrap_or(false)
    {
        scope.insert(0, 'l');
    }
    scope
}

async fn reconcile_agent_download_path_for_import(
    state: &AppState,
    import_scope_id: &str,
    raw_path: &StdPath,
) -> PathBuf {
    if fs::metadata(raw_path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return raw_path.to_path_buf();
    }

    if !path_contains_component(raw_path, import_scope_id) {
        return raw_path.to_path_buf();
    }

    let Some(file_name) = raw_path.file_name() else {
        return raw_path.to_path_buf();
    };

    let remapped = state
        .watch_party_audio_dir
        .join(import_scope_id)
        .join(file_name);
    if fs::metadata(&remapped)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
    {
        return remapped;
    }

    raw_path.to_path_buf()
}

async fn canonical_watch_party_audio_root_for_validation(
    state: &AppState,
) -> Result<PathBuf, AppError> {
    fs::create_dir_all(&state.watch_party_audio_dir)
        .await
        .map_err(|e| {
            ApiError::Internal(format!(
                "failed to ensure watch-party audio root directory: {e}"
            ))
        })?;
    state.watch_party_audio_dir.canonicalize().map_err(|e| {
        ApiError::Internal(format!(
            "failed to canonicalize watch-party audio root for validation: {e}"
        ))
        .into()
    })
}

async fn canonical_import_scope_dir_for_validation(
    state: &AppState,
    import_scope_id: &str,
) -> Result<PathBuf, AppError> {
    let scope_dir = state.watch_party_audio_dir.join(import_scope_id);
    fs::create_dir_all(&scope_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to ensure import scope directory: {e}")))?;
    scope_dir.canonicalize().map_err(|e| {
        ApiError::Internal(format!(
            "failed to canonicalize import scope directory for validation: {e}"
        ))
        .into()
    })
}

fn validate_import_audio_scope(
    canonical_file: &StdPath,
    canonical_scope_root: &StdPath,
    canonical_cache_root: &StdPath,
    import_scope_id: &str,
) -> Result<(), AppError> {
    if canonical_file.starts_with(canonical_scope_root) {
        return Ok(());
    }
    if canonical_file.starts_with(canonical_cache_root)
        && path_contains_component(canonical_file, import_scope_id)
    {
        return Ok(());
    }
    Err(ApiError::Forbidden(format!(
        "youtube-agent returned a file path outside import scope (file={}, scope_root={}, cache_root={}, scope_id={})",
        canonical_file.display(),
        canonical_scope_root.display(),
        canonical_cache_root.display(),
        import_scope_id
    ))
    .into())
}

async fn download_youtube_audio_for_library_import(
    state: &AppState,
    library_id: &str,
    source: &str,
) -> Result<DownloadedLibraryImportAudio, AppError> {
    let video_id = crate::watch_party::youtube::extract_youtube_video_id_from_input(source)
        .ok_or_else(|| {
            ApiError::BadRequest(
                "source must be a valid YouTube URL or 11-character video ID".into(),
            )
        })?;
    let import_scope_id = normalize_import_scope_id(library_id);
    let request_url = format!(
        "{}/api/v1/download/audio",
        state.youtube_agent_url.trim_end_matches('/')
    );
    let mut request = state
        .http
        .post(&request_url)
        .timeout(Duration::from_secs(YOUTUBE_AGENT_IMPORT_TIMEOUT_SECONDS))
        .json(&YouTubeAgentLibraryImportRequest {
            room_id: &import_scope_id,
            video_id: &video_id,
        });
    if let Some(token) = state
        .youtube_agent_token
        .as_ref()
        .filter(|token| !token.is_empty())
    {
        request = request.header("x-agent-token", token);
    }

    let response = request
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("youtube-agent request failed: {e}")))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read youtube-agent response>".to_string());
    if !status.is_success() {
        let sanitized = body_text.replace('\n', " ").trim().to_string();
        return Err(ApiError::BadRequest(format!(
            "failed to download YouTube audio via youtube-agent: {sanitized}"
        ))
        .into());
    }

    let payload: YouTubeAgentLibraryImportResponse =
        serde_json::from_str(&body_text).map_err(|e| {
            ApiError::Internal(format!(
                "youtube-agent returned invalid JSON payload for download response: {e}"
            ))
        })?;
    let file_path = reconcile_agent_download_path_for_import(
        state,
        &import_scope_id,
        StdPath::new(&payload.file_path),
    )
    .await;

    let file_meta = fs::metadata(&file_path)
        .await
        .map_err(|e| ApiError::Internal(format!("downloaded audio path missing from disk: {e}")))?;
    if !file_meta.is_file() {
        return Err(ApiError::Internal(
            "youtube-agent returned a path that is not a regular file".into(),
        )
        .into());
    }

    let canonical_file = file_path.canonicalize().map_err(|e| {
        ApiError::Internal(format!("failed to canonicalize downloaded audio path: {e}"))
    })?;
    let canonical_scope_root =
        canonical_import_scope_dir_for_validation(state, &import_scope_id).await?;
    let canonical_cache_root = canonical_watch_party_audio_root_for_validation(state).await?;
    validate_import_audio_scope(
        &canonical_file,
        &canonical_scope_root,
        &canonical_cache_root,
        &import_scope_id,
    )?;

    Ok(DownloadedLibraryImportAudio {
        file_path,
        duration_ms: payload.duration_ms,
        video_id,
        import_scope_id,
    })
}

async fn pick_unique_target_mp3_path(
    album_dir: &StdPath,
    title: &str,
) -> Result<PathBuf, AppError> {
    let base_name = normalize_music_segment(title, "title")?;
    for attempt in 0..1000 {
        let file_name = if attempt == 0 {
            format!("{base_name}.mp3")
        } else {
            format!("{base_name} ({}).mp3", attempt + 1)
        };
        let candidate = album_dir.join(file_name);
        match fs::metadata(&candidate).await {
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(candidate),
            Err(err) => {
                return Err(ApiError::Internal(format!(
                    "failed to inspect target file path: {err}"
                ))
                .into());
            }
        }
    }

    Err(ApiError::BadRequest(
        "could not allocate a unique filename in the target album directory".into(),
    )
    .into())
}

async fn move_downloaded_audio_to_target(
    source_path: &StdPath,
    target_path: &StdPath,
) -> Result<(), AppError> {
    match fs::rename(source_path, target_path).await {
        Ok(()) => Ok(()),
        Err(err)
            if err.kind() == std::io::ErrorKind::CrossesDevices
                || err.raw_os_error() == Some(18) =>
        {
            fs::copy(source_path, target_path)
                .await
                .map_err(|e| ApiError::Internal(format!("failed to copy downloaded audio: {e}")))?;
            fs::remove_file(source_path).await.map_err(|e| {
                ApiError::Internal(format!("failed to remove source audio after copy: {e}"))
            })?;
            Ok(())
        }
        Err(err) => Err(ApiError::Internal(format!(
            "failed to move downloaded audio into the library: {err}"
        ))
        .into()),
    }
}

async fn cleanup_import_scope_dir(state: &AppState, import_scope_id: &str) {
    let path = state.watch_party_audio_dir.join(import_scope_id);
    if let Err(err) = fs::remove_dir_all(&path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            scope_id = %import_scope_id,
            error = %err,
            "failed to clean temporary import scope directory"
        );
    }
}

fn validate_and_normalize_paths(paths: &[String]) -> Result<Vec<String>, AppError> {
    if paths.is_empty() {
        return Err(ApiError::BadRequest("at least one path required".into()).into());
    }

    let mut normalized_paths = Vec::with_capacity(paths.len());
    for (i, raw) in paths.iter().enumerate() {
        let p = raw.trim();
        if p.is_empty() {
            return Err(ApiError::validation(json!({
                format!("paths[{i}]"): ["must not be empty"]
            }))
            .into());
        }
        let path = std::path::Path::new(p);
        if !path.is_absolute() {
            return Err(ApiError::validation(json!({
                format!("paths[{i}]"): ["must be an absolute path"]
            }))
            .into());
        }
        if !path.exists() {
            return Err(ApiError::validation(json!({
                format!("paths[{i}]"): ["path does not exist on the server"]
            }))
            .into());
        }
        if !path.is_dir() {
            return Err(ApiError::validation(json!({
                format!("paths[{i}]"): ["path is not a directory"]
            }))
            .into());
        }
        if path.read_dir().is_err() {
            return Err(ApiError::validation(json!({
                format!("paths[{i}]"): ["directory is not readable by the server process"]
            }))
            .into());
        }
        normalized_paths.push(p.to_string());
    }
    Ok(normalized_paths)
}

fn normalize_tmdb_sync_schedule(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "manual" => Some("manual"),
        "hourly" => Some("hourly"),
        "daily" => Some("daily"),
        "weekly" => Some("weekly"),
        "monthly" => Some("monthly"),
        _ => None,
    }
}

fn default_library_settings_row(
    library_id: &str,
) -> rustfin_db::repo::libraries::LibrarySettingsRow {
    rustfin_db::repo::libraries::LibrarySettingsRow {
        library_id: library_id.to_string(),
        show_images: true,
        prefer_local_artwork: true,
        fetch_online_artwork: true,
        tmdb_store_in_media_dir: false,
        tmdb_sync_on_new_media: true,
        tmdb_sync_schedule: "manual".to_string(),
        tmdb_last_sync_ts: None,
        tmdb_fetch_posters: true,
        tmdb_fetch_backdrops: true,
        tmdb_fetch_metadata: true,
        tmdb_fetch_reviews: false,
        updated_ts: chrono::Utc::now().timestamp(),
    }
}

fn library_settings_row_to_response(
    settings: &rustfin_db::repo::libraries::LibrarySettingsRow,
) -> LibrarySettingsResponse {
    LibrarySettingsResponse {
        show_images: settings.show_images,
        prefer_local_artwork: settings.prefer_local_artwork,
        fetch_online_artwork: settings.fetch_online_artwork,
        tmdb_store_in_media_dir: settings.tmdb_store_in_media_dir,
        tmdb_sync_on_new_media: settings.tmdb_sync_on_new_media,
        tmdb_sync_schedule: settings.tmdb_sync_schedule.clone(),
        tmdb_last_sync_ts: settings.tmdb_last_sync_ts,
        tmdb_fetch_posters: settings.tmdb_fetch_posters,
        tmdb_fetch_backdrops: settings.tmdb_fetch_backdrops,
        tmdb_fetch_metadata: settings.tmdb_fetch_metadata,
        tmdb_fetch_reviews: settings.tmdb_fetch_reviews,
    }
}

async fn load_library_settings_response(
    state: &AppState,
    library_id: &str,
) -> Result<LibrarySettingsResponse, AppError> {
    let settings = rustfin_db::repo::libraries::get_library_settings(&state.db, library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| default_library_settings_row(library_id));
    Ok(library_settings_row_to_response(&settings))
}

async fn library_row_to_response(
    state: &AppState,
    lib: rustfin_db::repo::libraries::LibraryRow,
) -> Result<LibraryResponse, AppError> {
    let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &lib.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::libraries::count_library_items(&state.db, &lib.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let settings = load_library_settings_response(state, &lib.id).await?;

    Ok(LibraryResponse {
        id: lib.id,
        name: lib.name,
        kind: lib.kind,
        paths: paths
            .into_iter()
            .map(|p| LibraryPathResponse {
                id: p.id,
                path: p.path,
                is_read_only: p.is_read_only,
            })
            .collect(),
        settings,
        item_count,
        created_ts: lib.created_ts,
        updated_ts: lib.updated_ts,
    })
}

async fn create_library(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateLibraryRequest>,
) -> Result<(axum::http::StatusCode, Json<LibraryResponse>), AppError> {
    // Validate kind
    if body.kind != "movies" && body.kind != "tv_shows" && body.kind != "music" {
        return Err(
            ApiError::BadRequest("kind must be 'movies', 'tv_shows', or 'music'".into()).into(),
        );
    }
    let normalized_paths = validate_and_normalize_paths(&body.paths)?;

    let lib = rustfin_db::repo::libraries::create_library(
        &state.db,
        &body.name,
        &body.kind,
        &normalized_paths,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let sync_schedule = normalize_tmdb_sync_schedule(
        body.settings
            .tmdb_sync_schedule
            .as_deref()
            .unwrap_or("manual"),
    )
    .ok_or_else(|| {
        ApiError::BadRequest(
            "tmdb_sync_schedule must be one of: manual, hourly, daily, weekly, monthly".into(),
        )
    })?;

    rustfin_db::repo::libraries::upsert_library_settings(
        &state.db,
        rustfin_db::repo::libraries::UpsertLibrarySettingsParams {
            library_id: &lib.id,
            show_images: body.settings.show_images.unwrap_or(true),
            prefer_local_artwork: body.settings.prefer_local_artwork.unwrap_or(true),
            fetch_online_artwork: body.settings.fetch_online_artwork.unwrap_or(true),
            tmdb_store_in_media_dir: body.settings.tmdb_store_in_media_dir.unwrap_or(false),
            tmdb_sync_on_new_media: body.settings.tmdb_sync_on_new_media.unwrap_or(true),
            tmdb_sync_schedule: sync_schedule,
            tmdb_last_sync_ts: None,
            tmdb_fetch_posters: body.settings.tmdb_fetch_posters.unwrap_or(true),
            tmdb_fetch_backdrops: body.settings.tmdb_fetch_backdrops.unwrap_or(true),
            tmdb_fetch_metadata: body.settings.tmdb_fetch_metadata.unwrap_or(true),
            tmdb_fetch_reviews: body.settings.tmdb_fetch_reviews.unwrap_or(false),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let response = library_row_to_response(&state, lib).await?;

    // Auto-scan newly created libraries so items populate without manual scan.
    if let Err(e) =
        crate::library_scan::enqueue_library_scan(&state, &response.id, &response.kind).await
    {
        tracing::warn!(
            library_id = %response.id,
            status = e.0.status_code(),
            "library created but auto-scan enqueue failed"
        );
    }

    crate::audit_log::record_event(
        &state,
        "admin.libraries.create",
        serde_json::json!({
            "scope": "libraries",
            "action": "create",
            "admin_user_id": admin.user_id,
            "library_id": response.id,
            "name": response.name,
            "kind": response.kind,
            "path_count": response.paths.len(),
        }),
    )
    .await;

    Ok((axum::http::StatusCode::CREATED, Json(response)))
}

async fn list_libraries(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<LibraryResponse>>, AppError> {
    let allowed_library_ids = if auth.role == "admin" {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &auth.user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .into_iter()
                .collect::<HashSet<_>>(),
        )
    };

    let libs = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let libs: Vec<rustfin_db::repo::libraries::LibraryRow> = libs
        .into_iter()
        .filter(|lib| {
            allowed_library_ids
                .as_ref()
                .map(|allowed| allowed.contains(&lib.id))
                .unwrap_or(true)
        })
        .collect();

    if libs.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let library_ids: Vec<String> = libs.iter().map(|lib| lib.id.clone()).collect();
    let paths =
        rustfin_db::repo::libraries::get_library_paths_for_libraries(&state.db, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let counts =
        rustfin_db::repo::libraries::count_library_items_for_libraries(&state.db, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let settings_rows =
        rustfin_db::repo::libraries::get_library_settings_for_libraries(&state.db, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut paths_by_library: HashMap<String, Vec<LibraryPathResponse>> = HashMap::new();
    for path in paths {
        paths_by_library
            .entry(path.library_id)
            .or_default()
            .push(LibraryPathResponse {
                id: path.id,
                path: path.path,
                is_read_only: path.is_read_only,
            });
    }
    let item_count_by_library: HashMap<String, i64> = counts.into_iter().collect();
    let mut settings_by_library: HashMap<String, LibrarySettingsResponse> = settings_rows
        .into_iter()
        .map(|settings| {
            (
                settings.library_id.clone(),
                library_settings_row_to_response(&settings),
            )
        })
        .collect();

    let mut result = Vec::with_capacity(libs.len());
    for lib in libs {
        let lib_id = lib.id.clone();
        let settings = settings_by_library.remove(&lib_id).unwrap_or_else(|| {
            library_settings_row_to_response(&default_library_settings_row(&lib_id))
        });
        result.push(LibraryResponse {
            paths: paths_by_library.remove(&lib_id).unwrap_or_default(),
            item_count: item_count_by_library.get(&lib_id).copied().unwrap_or(0),
            settings,
            id: lib.id,
            name: lib.name,
            kind: lib.kind,
            created_ts: lib.created_ts,
            updated_ts: lib.updated_ts,
        });
    }

    Ok(Json(result))
}

async fn get_library(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LibraryResponse>, AppError> {
    let lib = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;
    ensure_library_access(&auth, &state, &lib.id).await?;

    Ok(Json(library_row_to_response(&state, lib).await?))
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct UpdateLibraryRequest {
    name: Option<String>,
    paths: Option<Vec<String>>,
    settings: LibrarySettingsPatchRequest,
}

async fn update_library(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateLibraryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let existing = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;

    let mut did_update = false;
    let mut should_rescan = false;

    if body.name.is_some() {
        let updated =
            rustfin_db::repo::libraries::update_library(&state.db, &id, body.name.as_deref())
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        did_update |= updated;
    }

    if let Some(paths) = &body.paths {
        let normalized_paths = validate_and_normalize_paths(paths)?;
        let replaced =
            rustfin_db::repo::libraries::replace_library_paths(&state.db, &id, &normalized_paths)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        did_update |= replaced;
        should_rescan |= replaced;
    }

    if body.settings.show_images.is_some()
        || body.settings.prefer_local_artwork.is_some()
        || body.settings.fetch_online_artwork.is_some()
        || body.settings.tmdb_store_in_media_dir.is_some()
        || body.settings.tmdb_sync_on_new_media.is_some()
        || body.settings.tmdb_sync_schedule.is_some()
        || body.settings.tmdb_fetch_posters.is_some()
        || body.settings.tmdb_fetch_backdrops.is_some()
        || body.settings.tmdb_fetch_metadata.is_some()
        || body.settings.tmdb_fetch_reviews.is_some()
    {
        let current = rustfin_db::repo::libraries::get_library_settings(&state.db, &id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .unwrap_or(rustfin_db::repo::libraries::LibrarySettingsRow {
                library_id: id.clone(),
                show_images: true,
                prefer_local_artwork: true,
                fetch_online_artwork: true,
                tmdb_store_in_media_dir: false,
                tmdb_sync_on_new_media: true,
                tmdb_sync_schedule: "manual".to_string(),
                tmdb_last_sync_ts: None,
                tmdb_fetch_posters: true,
                tmdb_fetch_backdrops: true,
                tmdb_fetch_metadata: true,
                tmdb_fetch_reviews: false,
                updated_ts: chrono::Utc::now().timestamp(),
            });

        let tmdb_sync_schedule = if let Some(raw) = body.settings.tmdb_sync_schedule.as_deref() {
            normalize_tmdb_sync_schedule(raw).ok_or_else(|| {
                ApiError::BadRequest(
                    "tmdb_sync_schedule must be one of: manual, hourly, daily, weekly, monthly"
                        .into(),
                )
            })?
        } else {
            current.tmdb_sync_schedule.as_str()
        };

        let _ = rustfin_db::repo::libraries::upsert_library_settings(
            &state.db,
            rustfin_db::repo::libraries::UpsertLibrarySettingsParams {
                library_id: &id,
                show_images: body.settings.show_images.unwrap_or(current.show_images),
                prefer_local_artwork: body
                    .settings
                    .prefer_local_artwork
                    .unwrap_or(current.prefer_local_artwork),
                fetch_online_artwork: body
                    .settings
                    .fetch_online_artwork
                    .unwrap_or(current.fetch_online_artwork),
                tmdb_store_in_media_dir: body
                    .settings
                    .tmdb_store_in_media_dir
                    .unwrap_or(current.tmdb_store_in_media_dir),
                tmdb_sync_on_new_media: body
                    .settings
                    .tmdb_sync_on_new_media
                    .unwrap_or(current.tmdb_sync_on_new_media),
                tmdb_sync_schedule,
                tmdb_last_sync_ts: current.tmdb_last_sync_ts,
                tmdb_fetch_posters: body
                    .settings
                    .tmdb_fetch_posters
                    .unwrap_or(current.tmdb_fetch_posters),
                tmdb_fetch_backdrops: body
                    .settings
                    .tmdb_fetch_backdrops
                    .unwrap_or(current.tmdb_fetch_backdrops),
                tmdb_fetch_metadata: body
                    .settings
                    .tmdb_fetch_metadata
                    .unwrap_or(current.tmdb_fetch_metadata),
                tmdb_fetch_reviews: body
                    .settings
                    .tmdb_fetch_reviews
                    .unwrap_or(current.tmdb_fetch_reviews),
            },
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        did_update = true;
        should_rescan = true;
    }

    if !did_update {
        return Err(ApiError::BadRequest("no update fields provided".into()).into());
    }

    if should_rescan {
        if let Err(e) =
            crate::library_scan::enqueue_library_scan(&state, &existing.id, &existing.kind).await
        {
            tracing::warn!(
                library_id = %existing.id,
                status = e.0.status_code(),
                "library updated but auto-scan enqueue failed"
            );
        }
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_library(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = rustfin_db::repo::libraries::delete_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !deleted {
        return Err(ApiError::NotFound("library not found".into()).into());
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn scan_library(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(axum::http::StatusCode, Json<JobResponse>), AppError> {
    // Verify library exists
    let lib = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;

    let job = crate::library_scan::enqueue_library_scan(&state, &lib.id, &lib.kind).await?;

    Ok((axum::http::StatusCode::ACCEPTED, Json(job_to_response(job))))
}

async fn import_library_music_from_youtube(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ImportLibraryMusicFromYoutubeRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<ImportLibraryMusicFromYoutubeResponse>,
    ),
    AppError,
> {
    let library = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;
    ensure_library_access(&auth, &state, &library.id).await?;

    if library.kind != "music" {
        return Err(ApiError::BadRequest(
            "YouTube music import is only supported for music libraries".into(),
        )
        .into());
    }

    let source = body.source.trim();
    if source.is_empty() {
        return Err(ApiError::BadRequest("source is required".into()).into());
    }

    let artist = normalize_music_segment(&body.artist, "artist")?;
    let album = normalize_album(body.album.as_deref())?;
    let title = normalize_track_title(&body.title, &artist)?;

    let library_paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &library.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let library_root = choose_music_library_root(&library_paths)?;

    let downloaded = download_youtube_audio_for_library_import(&state, &library.id, source).await?;
    let import_scope_id = downloaded.import_scope_id.clone();

    let import_result = async {
        let album_dir = library_root.join(&artist).join(&album);
        fs::create_dir_all(&album_dir)
            .await
            .map_err(|e| ApiError::Internal(format!("failed to create album directory: {e}")))?;

        let target_path = pick_unique_target_mp3_path(&album_dir, &title).await?;
        move_downloaded_audio_to_target(&downloaded.file_path, &target_path).await?;

        let scan_job =
            crate::library_scan::enqueue_library_scan(&state, &library.id, &library.kind).await?;

        Ok::<(PathBuf, rustfin_db::repo::jobs::JobRow), AppError>((target_path, scan_job))
    }
    .await;

    cleanup_import_scope_dir(&state, &import_scope_id).await;

    let (target_path, scan_job) = import_result?;

    crate::audit_log::record_event(
        &state,
        "libraries.music.import_youtube",
        serde_json::json!({
            "scope": "libraries",
            "action": "music_import_youtube",
            "user_id": auth.user_id,
            "user_role": auth.role,
            "library_id": library.id,
            "artist": artist.clone(),
            "album": album.clone(),
            "title": title.clone(),
            "video_id": downloaded.video_id.clone(),
            "file_path": target_path.to_string_lossy(),
        }),
    )
    .await;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ImportLibraryMusicFromYoutubeResponse {
            library_id: library.id,
            video_id: downloaded.video_id,
            artist,
            album,
            title,
            file_path: target_path.to_string_lossy().to_string(),
            duration_ms: downloaded.duration_ms,
            scan_job: job_to_response(scan_job),
        }),
    ))
}

async fn sync_library_tmdb(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(axum::http::StatusCode, Json<JobResponse>), AppError> {
    let lib = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;

    let job = crate::tmdb_sync::enqueue_library_tmdb_sync(&state, &lib.id).await?;

    Ok((axum::http::StatusCode::ACCEPTED, Json(job_to_response(job))))
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JobResponse {
    id: String,
    kind: String,
    status: String,
    progress: f64,
    payload: Option<serde_json::Value>,
    error: Option<String>,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Deserialize)]
struct JobsQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

fn parse_job_status_filter(raw: Option<&str>) -> Result<Option<Vec<&'static str>>, AppError> {
    let value = raw.map(str::trim).unwrap_or_default();
    if value.is_empty() || value.eq_ignore_ascii_case("all") {
        return Ok(None);
    }

    if value.eq_ignore_ascii_case("complete") || value.eq_ignore_ascii_case("completed") {
        return Ok(Some(vec!["completed"]));
    }

    if value.eq_ignore_ascii_case("failed") {
        return Ok(Some(vec!["failed"]));
    }

    if value.eq_ignore_ascii_case("in_progress") {
        return Ok(Some(vec!["queued", "running"]));
    }

    let normalized = value.to_ascii_lowercase();
    match normalized.as_str() {
        "queued" => Ok(Some(vec!["queued"])),
        "running" => Ok(Some(vec!["running"])),
        "cancelled" => Ok(Some(vec!["cancelled"])),
        "error" => Ok(Some(vec!["error"])),
        _ => Err(ApiError::BadRequest(
            "status must be one of: all, complete, failed, in_progress, queued, running, cancelled, error"
                .into(),
        )
        .into()),
    }
}

fn job_to_response(job: rustfin_db::repo::jobs::JobRow) -> JobResponse {
    let payload = job
        .payload_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    JobResponse {
        id: job.id,
        kind: job.kind,
        status: job.status,
        progress: job.progress,
        payload,
        error: job.error,
        created_ts: job.created_ts,
        updated_ts: job.updated_ts,
    }
}

async fn list_jobs(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(params): Query<JobsQuery>,
) -> Result<Json<Vec<JobResponse>>, AppError> {
    let statuses = parse_job_status_filter(params.status.as_deref())?;
    let kind = params
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let limit = Some(params.limit.unwrap_or(100).clamp(1, 1000));
    let offset = params.offset.map(|value| value.clamp(0, 1_000_000));

    let jobs = rustfin_db::repo::jobs::list_jobs_filtered(
        &state.db,
        statuses.as_deref().unwrap_or(&[]),
        kind,
        limit,
        offset,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(jobs.into_iter().map(job_to_response).collect()))
}

async fn get_job(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobResponse>, AppError> {
    let job = rustfin_db::repo::jobs::get_job(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("job not found".into()))?;

    Ok(Json(job_to_response(job)))
}

async fn cancel_job(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let cancelled = rustfin_db::repo::jobs::cancel_job(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !cancelled {
        return Err(ApiError::BadRequest("job not found or not cancellable".into()).into());
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_job(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    let deleted = rustfin_db::repo::jobs::delete_job(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !deleted {
        return Err(ApiError::BadRequest("job not found or still active".into()).into());
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ItemResponse {
    id: String,
    library_id: String,
    kind: String,
    parent_id: Option<String>,
    title: String,
    sort_title: Option<String>,
    year: Option<i64>,
    overview: Option<String>,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    logo_url: Option<String>,
    thumb_url: Option<String>,
    created_ts: i64,
    updated_ts: i64,
    duration_ms: Option<i64>,
}

#[derive(Serialize)]
struct PlaybackDescriptorResponse {
    item_id: String,
    file_id: String,
    direct_url: String,
    hls_start_url: String,
    media_info_url: String,
    duration_ms: Option<i64>,
}

fn item_image_url(item_id: &str, img_type: &str, include_images: bool) -> Option<String> {
    if include_images {
        Some(format!("/api/v1/items/{item_id}/images/{img_type}"))
    } else {
        None
    }
}

fn supports_generated_item_images(kind: &str) -> bool {
    matches!(kind, "movie" | "series" | "season" | "episode")
}

fn item_to_response(item: rustfin_db::repo::items::ItemRow, include_images: bool) -> ItemResponse {
    let include_generated = supports_generated_item_images(&item.kind);
    ItemResponse {
        id: item.id.clone(),
        library_id: item.library_id,
        kind: item.kind,
        parent_id: item.parent_id,
        title: item.title,
        sort_title: item.sort_title,
        year: item.year,
        overview: item.overview,
        poster_url: if item.poster_url.is_some() || include_generated {
            item_image_url(&item.id, "poster", include_images)
        } else {
            None
        },
        backdrop_url: if item.backdrop_url.is_some() || include_generated {
            item_image_url(&item.id, "backdrop", include_images)
        } else {
            None
        },
        logo_url: if item.logo_url.is_some() {
            item_image_url(&item.id, "logo", include_images)
        } else {
            None
        },
        thumb_url: if item.thumb_url.is_some() || include_generated {
            item_image_url(&item.id, "thumb", include_images)
        } else {
            None
        },
        created_ts: item.created_ts,
        updated_ts: item.updated_ts,
        duration_ms: item.duration_ms,
    }
}

async fn list_library_items(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ItemResponse>>, AppError> {
    let lib = rustfin_db::repo::libraries::get_library(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;
    ensure_library_access(&auth, &state, &lib.id).await?;

    let items = rustfin_db::repo::items::get_library_items(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let show_images = rustfin_db::repo::libraries::get_library_settings(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .map(|s| s.show_images)
        .unwrap_or(true);

    Ok(Json(
        items
            .into_iter()
            .map(|item| item_to_response(item, show_images))
            .collect(),
    ))
}

async fn get_item(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ItemResponse>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;
    let show_images =
        rustfin_db::repo::libraries::get_library_settings(&state.db, &item.library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .map(|s| s.show_images)
            .unwrap_or(true);

    Ok(Json(item_to_response(item, show_images)))
}

async fn get_item_playback(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PlaybackDescriptorResponse>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let file_id = rustfin_db::repo::items::get_item_file_id(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| {
            ApiError::Conflict("No playable file mapped to this item; rescan library.".into())
        })?;

    let duration_ms = resolve_and_persist_media_duration_ms(
        &state,
        &file_id,
        item.duration_ms.filter(|value| *value > 0),
    )
    .await;

    let token = issue_stream_token(
        &auth.user_id,
        &auth.role,
        Some(&file_id),
        None,
        stream_token_ttl_seconds(),
        &state.jwt_secret,
    )?;

    Ok(Json(PlaybackDescriptorResponse {
        item_id: id,
        file_id: file_id.clone(),
        direct_url: format!("/stream/file/{file_id}?st={token}"),
        hls_start_url: "/api/v1/playback/sessions".to_string(),
        media_info_url: format!("/api/v1/playback/info/{file_id}"),
        duration_ms,
    }))
}

async fn resolve_and_persist_media_duration_ms(
    state: &AppState,
    file_id: &str,
    preferred_duration_ms: Option<i64>,
) -> Option<i64> {
    let mut duration_ms = preferred_duration_ms.filter(|value| *value > 0);
    let Ok(Some(file)) = rustfin_db::repo::media_files::get_media_file(&state.db, file_id).await
    else {
        return duration_ms;
    };

    let mut file_duration_ms = file.duration_ms.filter(|value| *value > 0);
    let media_path = std::path::Path::new(&file.path);
    if media_path.exists()
        && media_path.is_file()
        && (duration_ms.is_none() || file_duration_ms.is_none())
    {
        if let Ok(info) =
            rustfin_transcoder::ffprobe::probe(state.transcoder.ffprobe_path(), media_path).await
        {
            let probed_duration_ms = (info.duration_secs * 1000.0).round() as i64;
            if probed_duration_ms > 0 {
                duration_ms = Some(probed_duration_ms);
                if file_duration_ms != Some(probed_duration_ms) {
                    let _ = rustfin_db::repo::media_files::update_media_file_duration(
                        &state.db,
                        file_id,
                        probed_duration_ms,
                    )
                    .await;
                    file_duration_ms = Some(probed_duration_ms);
                }
            }
        }
    }

    if duration_ms.is_none() {
        duration_ms = file_duration_ms;
    }

    duration_ms
}

fn parse_first_u32(value: &str) -> Option<u32> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .find(|segment| !segment.is_empty())
        .and_then(|segment| segment.parse::<u32>().ok())
}

fn season_sort_title(season: u32) -> String {
    format!("rf-season-{season:05}")
}

fn episode_sort_title(season: u32, episode: u32) -> String {
    format!("rf-season-{season:05}-episode-{episode:05}")
}

fn parse_episode_order_from_sort_title(sort_title: &str) -> Option<(u32, u32)> {
    let lower = sort_title.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("rf-season-")?;
    let (season, episode) = rest.split_once("-episode-")?;
    Some((season.parse().ok()?, episode.parse().ok()?))
}

fn parse_season_order_from_sort_title(sort_title: &str) -> Option<u32> {
    if let Some((season, _)) = parse_episode_order_from_sort_title(sort_title) {
        return Some(season);
    }
    let lower = sort_title.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rf-season-") {
        return rest.parse::<u32>().ok();
    }
    None
}

fn parse_season_order_from_title(title: &str) -> Option<u32> {
    let lower = title.trim().to_ascii_lowercase();
    if lower == "specials" {
        return Some(0);
    }
    if let Some(rest) = lower.strip_prefix("season") {
        return parse_first_u32(rest);
    }
    if let Some(rest) = lower.strip_prefix('s') {
        return parse_first_u32(rest);
    }
    None
}

fn parse_episode_order_from_title(title: &str) -> Option<(u32, u32)> {
    let lower = title.trim().to_ascii_lowercase();
    lower
        .strip_prefix("episode")
        .and_then(parse_first_u32)
        .map(|episode| (0, episode))
}

fn parse_episode_order_from_media_path(path: &str) -> Option<(u32, u32)> {
    let file_name = StdPath::new(path).file_name()?.to_str()?;
    match rustfin_scanner::parser::parse_filename(file_name) {
        rustfin_scanner::parser::ParsedMedia::Episode(info) => Some((info.season, info.episode)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildWatchOrderMode {
    Season,
    Episode,
    Default,
}

fn resolve_child_watch_order_mode(
    parent_kind: &str,
    children: &[rustfin_db::repo::items::ItemRow],
) -> ChildWatchOrderMode {
    let has_season_children = children.iter().any(|child| child.kind == "season");
    let has_episode_children = children.iter().any(|child| child.kind == "episode");

    match parent_kind {
        "series" if has_season_children => ChildWatchOrderMode::Season,
        "series" if has_episode_children => ChildWatchOrderMode::Episode,
        "season" if has_episode_children => ChildWatchOrderMode::Episode,
        _ => ChildWatchOrderMode::Default,
    }
}

async fn reorder_children_for_watch_order(
    state: &AppState,
    parent_kind: &str,
    children: &mut Vec<rustfin_db::repo::items::ItemRow>,
) -> Result<(), sqlx::Error> {
    match resolve_child_watch_order_mode(parent_kind, children) {
        ChildWatchOrderMode::Season => {
            let mut season_keys: HashMap<String, u32> = HashMap::new();
            for child in children.iter_mut().filter(|child| child.kind == "season") {
                let season_order = child
                    .sort_title
                    .as_deref()
                    .and_then(parse_season_order_from_sort_title)
                    .or_else(|| parse_season_order_from_title(&child.title));
                if let Some(season_order) = season_order {
                    season_keys.insert(child.id.clone(), season_order);
                    if child.sort_title.is_none() {
                        let sort_title = season_sort_title(season_order);
                        rustfin_db::repo::items::update_item_sort_title(
                            &state.db,
                            &child.id,
                            Some(&sort_title),
                        )
                        .await?;
                        child.sort_title = Some(sort_title);
                    }
                }
            }

            children.sort_by_cached_key(|child| {
                (
                    if child.kind == "season" {
                        season_keys.get(&child.id).copied().unwrap_or(u32::MAX)
                    } else {
                        u32::MAX
                    },
                    child
                        .sort_title
                        .as_deref()
                        .unwrap_or(&child.title)
                        .to_ascii_lowercase(),
                    child.title.to_ascii_lowercase(),
                )
            });
            Ok(())
        }
        ChildWatchOrderMode::Episode => {
            let mut episode_keys: HashMap<String, (u32, u32)> = HashMap::new();
            let mut unresolved_episode_ids: Vec<String> = Vec::new();

            for child in children.iter().filter(|child| child.kind == "episode") {
                let episode_key = child
                    .sort_title
                    .as_deref()
                    .and_then(parse_episode_order_from_sort_title)
                    .or_else(|| parse_episode_order_from_title(&child.title));
                if let Some(episode_key) = episode_key {
                    episode_keys.insert(child.id.clone(), episode_key);
                } else {
                    unresolved_episode_ids.push(child.id.clone());
                }
            }

            if !unresolved_episode_ids.is_empty() {
                let media_paths = rustfin_db::repo::items::get_item_media_paths(
                    &state.db,
                    &unresolved_episode_ids,
                )
                .await?;

                for child in children.iter_mut().filter(|child| child.kind == "episode") {
                    if episode_keys.contains_key(&child.id) {
                        continue;
                    }
                    let Some(path) = media_paths.get(&child.id) else {
                        continue;
                    };
                    let Some(episode_key) = parse_episode_order_from_media_path(path) else {
                        continue;
                    };

                    episode_keys.insert(child.id.clone(), episode_key);
                    if child.sort_title.is_none() {
                        let sort_title = episode_sort_title(episode_key.0, episode_key.1);
                        rustfin_db::repo::items::update_item_sort_title(
                            &state.db,
                            &child.id,
                            Some(&sort_title),
                        )
                        .await?;
                        child.sort_title = Some(sort_title);
                    }
                }
            }

            children.sort_by_cached_key(|child| {
                (
                    if child.kind == "episode" {
                        episode_keys
                            .get(&child.id)
                            .copied()
                            .unwrap_or((u32::MAX, u32::MAX))
                    } else {
                        (u32::MAX, u32::MAX)
                    },
                    child
                        .sort_title
                        .as_deref()
                        .unwrap_or(&child.title)
                        .to_ascii_lowercase(),
                    child.title.to_ascii_lowercase(),
                )
            });
            Ok(())
        }
        ChildWatchOrderMode::Default => Ok(()),
    }
}

async fn get_item_children(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ItemResponse>>, AppError> {
    let parent = rustfin_db::repo::items::get_item(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &parent.library_id).await?;

    let mut children = rustfin_db::repo::items::get_children(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    reorder_children_for_watch_order(&state, &parent.kind, &mut children)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let show_images =
        rustfin_db::repo::libraries::get_library_settings(&state.db, &parent.library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .map(|s| s.show_images)
            .unwrap_or(true);

    Ok(Json(
        children
            .into_iter()
            .map(|item| item_to_response(item, show_images))
            .collect(),
    ))
}

// ---------------------------------------------------------------------------
// Playback progress
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProgressRequest {
    item_id: String,
    progress_ms: i64,
    #[serde(default)]
    played: bool,
    #[serde(default)]
    playback_session_id: Option<String>,
}

fn progress_threshold_ms(duration_ms: Option<i64>, prefer_larger: bool) -> i64 {
    let percent_threshold = duration_ms
        .filter(|value| *value > 0)
        .map(|value| {
            (value * CONTINUE_WATCHING_PERCENT_NUMERATOR / CONTINUE_WATCHING_PERCENT_DENOMINATOR)
                .max(1)
        })
        .unwrap_or(CONTINUE_WATCHING_MIN_PROGRESS_MS);

    if prefer_larger {
        percent_threshold.max(CONTINUE_WATCHING_MIN_PROGRESS_MS)
    } else {
        percent_threshold.min(CONTINUE_WATCHING_MIN_PROGRESS_MS)
    }
}

fn normalize_progress_state(
    requested_progress_ms: i64,
    played: bool,
    duration_ms: Option<i64>,
) -> (i64, bool) {
    let clamped_duration_ms = duration_ms.filter(|value| *value > 0);
    let mut progress_ms = requested_progress_ms.max(0);
    if let Some(duration_ms) = clamped_duration_ms {
        progress_ms = progress_ms.min(duration_ms);
    }

    if played {
        return (0, true);
    }

    if progress_ms <= 0 {
        return (0, false);
    }

    let start_threshold_ms = progress_threshold_ms(clamped_duration_ms, false);
    if progress_ms < start_threshold_ms {
        return (0, false);
    }

    if let Some(duration_ms) = clamped_duration_ms {
        let completion_threshold_ms = progress_threshold_ms(Some(duration_ms), true);
        if duration_ms.saturating_sub(progress_ms) <= completion_threshold_ms {
            return (0, true);
        }
    }

    (progress_ms, false)
}

async fn update_progress(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ProgressRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &body.item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let progress_duration_ms =
        if let Some(duration_ms) = item.duration_ms.filter(|value| *value > 0) {
            Some(duration_ms)
        } else if let Some(file_id) =
            rustfin_db::repo::items::get_item_file_id(&state.db, &body.item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        {
            rustfin_db::repo::media_files::get_media_file(&state.db, &file_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .and_then(|file| file.duration_ms.filter(|value| *value > 0))
        } else {
            None
        };

    let (progress_ms, played) =
        normalize_progress_state(body.progress_ms, body.played, progress_duration_ms);

    rustfin_db::repo::playstate::update_progress(
        &state.db,
        &auth.user_id,
        &body.item_id,
        progress_ms,
        played,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if let Some(playback_session_id) = body
        .playback_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        user_activity::record_media_progress(
            &state,
            &auth.user_id,
            playback_session_id,
            progress_ms,
        )
        .await?;
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Serialize)]
struct PlayStateResponse {
    item_id: String,
    played: bool,
    progress_ms: i64,
    last_played_ts: Option<i64>,
    favorite: bool,
}

#[derive(Serialize)]
struct ContinueWatchingResponse {
    id: String,
    library_id: String,
    kind: String,
    title: String,
    year: Option<i64>,
    poster_url: Option<String>,
    progress_ms: i64,
    duration_ms: Option<i64>,
    last_played_ts: i64,
}

async fn get_play_state(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<PlayStateResponse>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let state_row = rustfin_db::repo::playstate::get_play_state(&state.db, &auth.user_id, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    match state_row {
        Some(s) => Ok(Json(PlayStateResponse {
            item_id: s.item_id,
            played: s.played,
            progress_ms: s.progress_ms,
            last_played_ts: s.last_played_ts,
            favorite: s.favorite,
        })),
        None => Ok(Json(PlayStateResponse {
            item_id,
            played: false,
            progress_ms: 0,
            last_played_ts: None,
            favorite: false,
        })),
    }
}

async fn list_continue_watching(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ContinueWatchingResponse>>, AppError> {
    let allowed_library_ids = if auth.role == "admin" {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &auth.user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?,
        )
    };

    let rows = rustfin_db::repo::playstate::list_continue_watching(
        &state.db,
        &auth.user_id,
        allowed_library_ids.as_deref(),
        12,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let library_ids: Vec<String> = rows.iter().map(|row| row.library_id.clone()).collect();
    let settings_rows =
        rustfin_db::repo::libraries::get_library_settings_for_libraries(&state.db, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let show_images_by_library: HashMap<String, bool> = settings_rows
        .into_iter()
        .map(|settings| (settings.library_id, settings.show_images))
        .collect();

    let mut response_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let duration_ms = if row.duration_ms.is_some() {
            row.duration_ms
        } else if let Ok(Some(file_id)) =
            rustfin_db::repo::items::get_item_file_id(&state.db, &row.item_id).await
        {
            resolve_and_persist_media_duration_ms(&state, &file_id, None).await
        } else {
            None
        };

        let include_generated_poster = supports_generated_item_images(&row.kind);

        response_rows.push(ContinueWatchingResponse {
            id: row.item_id.clone(),
            library_id: row.library_id.clone(),
            kind: row.kind,
            title: row.title,
            year: row.year,
            poster_url: if (row.poster_url.is_some() || include_generated_poster)
                && show_images_by_library
                    .get(&row.library_id)
                    .copied()
                    .unwrap_or(true)
            {
                item_image_url(&row.item_id, "poster", true)
            } else {
                None
            },
            progress_ms: row.progress_ms,
            duration_ms,
            last_played_ts: row.last_played_ts,
        });
    }

    Ok(Json(response_rows))
}

// ---------------------------------------------------------------------------
// Playback sessions (HLS transcode)
// ---------------------------------------------------------------------------

fn map_transcode_session_error(err: rustfin_transcoder::TranscodeError) -> ApiError {
    match err {
        rustfin_transcoder::TranscodeError::MaxTranscodesReached(n) => {
            ApiError::BadRequest(format!("max concurrent transcodes reached ({n})"))
        }
        rustfin_transcoder::TranscodeError::FfmpegFailed(msg) => {
            let lower = msg.to_lowercase();
            if lower.contains("spawn")
                && (lower.contains("no such file") || lower.contains("not found"))
            {
                ApiError::Internal(
                    "ffmpeg is not available; configure RUSTFIN_FFMPEG_PATH or install ffmpeg"
                        .into(),
                )
            } else if lower.contains("permission denied") {
                ApiError::Internal(
                    "transcode directory is not writable by the server process".into(),
                )
            } else {
                ApiError::Internal(format!("ffmpeg failed: {msg}"))
            }
        }
        rustfin_transcoder::TranscodeError::Io(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                ApiError::Internal(
                    "transcode directory is not writable by the server process".into(),
                )
            } else if e.kind() == std::io::ErrorKind::NotFound {
                ApiError::Internal("input media file is not readable or no longer exists".into())
            } else {
                ApiError::Internal(format!("transcoder IO error: {e}"))
            }
        }
        other => ApiError::Internal(format!("transcode error: {other}")),
    }
}

#[derive(Deserialize)]
struct CreateSessionRequest {
    file_id: String,
    #[serde(default)]
    start_time_secs: Option<f64>,
    #[serde(default)]
    target_height: Option<u32>,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
    hls_url: String,
}

async fn create_playback_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>, AppError> {
    let requested_target_height = normalize_transcode_height(body.target_height)?;
    let file = get_accessible_media_file(&auth, &state, &body.file_id).await?;
    let item_id = rustfin_db::repo::items::get_item_id_by_file_id(&state.db, &body.file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found for media file".into()))?;
    let input_path = std::path::PathBuf::from(&file.path);
    let target_height = if requested_target_height.is_some() {
        let media_info = probe_media_info_for_file(&state, &input_path).await?;
        validate_target_height_for_source(
            requested_target_height,
            media_info.video.as_ref().map(|video| video.height),
        )?
    } else {
        None
    };

    let start_time_secs = normalize_session_start_time_secs(body.start_time_secs, file.duration_ms);

    let session_id = state
        .transcoder
        .create_session(
            input_path,
            start_time_secs,
            target_height,
            None,
            auth.user_id.clone(),
            body.file_id.clone(),
        )
        .await
        .map_err(map_transcode_session_error)?;

    let stream_token = issue_stream_token(
        &auth.user_id,
        &auth.role,
        Some(&body.file_id),
        Some(&session_id),
        stream_token_ttl_seconds(),
        &state.jwt_secret,
    )?;
    let hls_url = format!("/stream/hls/{session_id}/master.m3u8?st={stream_token}");

    user_activity::start_media_watch(&state, &auth.user_id, &session_id, &item_id, &body.file_id)
        .await?;

    Ok(Json(SessionResponse {
        session_id,
        hls_url,
    }))
}

fn normalize_transcode_height(raw: Option<u32>) -> Result<Option<u32>, AppError> {
    let Some(height) = raw else {
        return Ok(None);
    };
    match height {
        360 | 480 | 720 | 1080 | 1440 | 2160 => Ok(Some(height)),
        _ => Err(ApiError::BadRequest(
            "invalid target_height. allowed: 360, 480, 720, 1080, 1440, 2160".into(),
        )
        .into()),
    }
}

fn normalize_session_start_time_secs(
    start_time_secs: Option<f64>,
    duration_ms: Option<i64>,
) -> Option<f64> {
    let requested_secs = start_time_secs?;
    if !requested_secs.is_finite() {
        return None;
    }

    let sanitized_secs = requested_secs.max(0.0);
    let Some(duration_ms) = duration_ms.filter(|value| *value > 0) else {
        return Some(sanitized_secs);
    };

    let duration_secs = duration_ms as f64 / 1000.0;
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        return Some(sanitized_secs);
    }

    // Keep HLS session seeks inside a decodable window; ffmpeg at exact EOF can produce empty
    // playlists that render as a blank player in some browsers.
    let safe_max_secs = (duration_secs - 0.5).max(0.0);
    Some(sanitized_secs.min(safe_max_secs))
}

#[derive(Debug, Deserialize)]
struct DownloadPlaybackQuery {
    target_height: Option<u32>,
}

async fn get_accessible_media_file(
    auth: &AuthUser,
    state: &AppState,
    file_id: &str,
) -> Result<rustfin_db::repo::media_files::MediaFileRow, AppError> {
    if auth.role != "admin" {
        let item_id = rustfin_db::repo::items::get_item_id_by_file_id(&state.db, file_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not accessible for this account".into()))?;
        let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not accessible for this account".into()))?;
        ensure_library_access(auth, state, &item.library_id).await?;
    }

    let file = rustfin_db::repo::media_files::get_media_file(&state.db, file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("media file not found".into()))?;

    let media_path = StdPath::new(&file.path);
    if !media_path.exists() {
        return Err(ApiError::NotFound("media file does not exist on disk".into()).into());
    }
    if !media_path.is_file() {
        return Err(ApiError::BadRequest("media path is not a regular file".into()).into());
    }
    if std::fs::File::open(media_path).is_err() {
        return Err(ApiError::BadRequest(
            "media file is not readable by the server process".into(),
        )
        .into());
    }

    Ok(file)
}

async fn probe_media_info_for_file(
    state: &AppState,
    media_path: &StdPath,
) -> Result<rustfin_transcoder::ffprobe::MediaInfo, AppError> {
    rustfin_transcoder::ffprobe::probe(state.transcoder.ffprobe_path(), media_path)
        .await
        .map_err(|e| {
            let message = e.to_string().to_lowercase();
            if message.contains("spawn failed")
                && (message.contains("no such file") || message.contains("not found"))
            {
                ApiError::Internal(
                    "ffprobe is not available; configure RUSTFIN_FFPROBE_PATH or install ffprobe"
                        .into(),
                )
            } else if message.contains("permission denied") {
                ApiError::Internal("media file is not readable by ffprobe".into())
            } else {
                ApiError::Internal(format!("ffprobe error: {e}"))
            }
        })
        .map_err(AppError::from)
}

fn validate_target_height_for_source(
    target_height: Option<u32>,
    source_height: Option<u32>,
) -> Result<Option<u32>, AppError> {
    let Some(height) = target_height else {
        return Ok(None);
    };
    let Some(source_height) = source_height.filter(|value| *value > 0) else {
        return Err(ApiError::BadRequest(
            "source video height is unavailable for this media file".into(),
        )
        .into());
    };
    if height > source_height {
        return Err(ApiError::BadRequest(format!(
            "requested quality {height}p exceeds source height {source_height}p"
        ))
        .into());
    }
    Ok(Some(height))
}

fn playback_download_content_type(path: &StdPath) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn playback_download_filename(path: &StdPath, target_height: Option<u32>) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("rustyfin-media");
    let stem = stem.replace(['"', ';'], "");
    match target_height {
        Some(height) => format!("{stem}-{height}p.mp4"),
        None => path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.replace(['"', ';'], ""))
            .unwrap_or_else(|| format!("{stem}.bin")),
    }
}

fn attachment_disposition(filename: &str) -> String {
    let sanitized = filename.replace('"', "");
    format!("attachment; filename=\"{sanitized}\"")
}

async fn spawn_download_transcode_ffmpeg(
    state: &AppState,
    input_path: &StdPath,
    target_height: u32,
) -> Result<tokio::process::Child, AppError> {
    let active_hw_accel = state.transcoder.hw_accel().cloned();
    let initial_args = build_download_transcode_ffmpeg_args(
        input_path,
        target_height,
        active_hw_accel.as_ref(),
        state.transcoder.hw_device_path(),
    );
    let mut child = spawn_download_transcode_ffmpeg_process(state, &initial_args)?;

    if let Some(status) = wait_for_download_transcode_start(&mut child).await? {
        let startup_tail =
            read_child_stderr_tail(&mut child, DOWNLOAD_TRANSCODE_STDERR_TAIL_LINES).await;
        if let Some(hw) = active_hw_accel.as_ref() {
            warn!(
                input_path = %input_path.display(),
                ?hw,
                ?status,
                ffmpeg_stderr = startup_tail.as_deref().unwrap_or(""),
                "download transcode ffmpeg exited during startup; retrying with software fallback"
            );

            let fallback_args =
                build_download_transcode_ffmpeg_args(input_path, target_height, None, None);
            let mut fallback_child =
                spawn_download_transcode_ffmpeg_process(state, &fallback_args)?;
            if let Some(fallback_status) =
                wait_for_download_transcode_start(&mut fallback_child).await?
            {
                let fallback_tail = read_child_stderr_tail(
                    &mut fallback_child,
                    DOWNLOAD_TRANSCODE_STDERR_TAIL_LINES,
                )
                .await;
                return Err(ApiError::Internal(build_download_startup_error(
                    fallback_status,
                    fallback_tail.as_deref(),
                    true,
                ))
                .into());
            }

            warn!(
                input_path = %input_path.display(),
                "download transcode switched to software fallback"
            );
            return Ok(fallback_child);
        }

        return Err(ApiError::Internal(build_download_startup_error(
            status,
            startup_tail.as_deref(),
            false,
        ))
        .into());
    }

    Ok(child)
}

fn build_download_transcode_ffmpeg_args(
    input_path: &StdPath,
    target_height: u32,
    active_hw_accel: Option<&rustfin_transcoder::HwAccel>,
    hw_device_path: Option<&StdPath>,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-i".into(),
        input_path.to_string_lossy().into_owned(),
        "-map".into(),
        "0:v:0?".into(),
        "-map".into(),
        "0:a:0?".into(),
        "-sn".into(),
        "-dn".into(),
    ];

    if let Some(hw) = active_hw_accel {
        match hw {
            rustfin_transcoder::HwAccel::Nvenc => {
                // Decode in software for broader codec/profile compatibility while
                // still using NVENC for accelerated H.264 encode.
            }
            rustfin_transcoder::HwAccel::Vaapi => {
                let device = hw_device_path
                    .unwrap_or_else(|| StdPath::new("/dev/dri/renderD128"))
                    .to_string_lossy()
                    .into_owned();
                args.splice(0..0, ["-vaapi_device".into(), device]);
            }
            rustfin_transcoder::HwAccel::Qsv => {
                args.splice(0..0, ["-hwaccel".into(), "qsv".into()]);
                if let Some(device) = hw_device_path {
                    args.splice(
                        2..2,
                        ["-qsv_device".into(), device.to_string_lossy().into_owned()],
                    );
                }
            }
            rustfin_transcoder::HwAccel::VideoToolbox => {
                args.splice(0..0, ["-hwaccel".into(), "videotoolbox".into()]);
            }
        }
    }

    let vf = match active_hw_accel {
        Some(rustfin_transcoder::HwAccel::Vaapi) => Some(format!(
            "format=nv12,hwupload,scale_vaapi=w=-2:h={target_height}"
        )),
        Some(rustfin_transcoder::HwAccel::Qsv) => Some(format!("vpp_qsv=w=-2:h={target_height}")),
        _ => Some(format!("scale=-2:min(ih\\,{target_height})")),
    };
    if let Some(filter) = vf {
        args.extend(["-vf".into(), filter]);
    }

    let vcodec = match active_hw_accel {
        Some(rustfin_transcoder::HwAccel::Nvenc) => "h264_nvenc",
        Some(rustfin_transcoder::HwAccel::Vaapi) => "h264_vaapi",
        Some(rustfin_transcoder::HwAccel::Qsv) => "h264_qsv",
        Some(rustfin_transcoder::HwAccel::VideoToolbox) => "h264_videotoolbox",
        None => "libx264",
    };
    args.extend(["-c:v".into(), vcodec.into()]);
    if matches!(active_hw_accel, Some(rustfin_transcoder::HwAccel::Vaapi)) {
        args.extend(["-profile:v".into(), "high".into()]);
    }
    if matches!(active_hw_accel, Some(rustfin_transcoder::HwAccel::Nvenc)) {
        // Keep NVENC output in 8-bit H.264 for Main10 and other high-bit-depth inputs.
        args.extend(["-pix_fmt".into(), "yuv420p".into()]);
    }
    if active_hw_accel.is_none() {
        args.extend([
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "23".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
    }

    args.extend([
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        "48000".into(),
        "-movflags".into(),
        "frag_keyframe+empty_moov+faststart".into(),
        "-f".into(),
        "mp4".into(),
        "pipe:1".into(),
    ]);

    args
}

fn spawn_download_transcode_ffmpeg_process(
    state: &AppState,
    args: &[String],
) -> Result<tokio::process::Child, AppError> {
    tokio::process::Command::new(state.transcoder.ffmpeg_path())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let message = e.to_string().to_lowercase();
            if message.contains("no such file") || message.contains("not found") {
                ApiError::Internal(
                    "ffmpeg is not available; configure RUSTFIN_FFMPEG_PATH or install ffmpeg"
                        .into(),
                )
            } else {
                ApiError::Internal(format!("failed to start media transcode download: {e}"))
            }
        })
        .map_err(AppError::from)
}

async fn wait_for_download_transcode_start(
    child: &mut tokio::process::Child,
) -> Result<Option<std::process::ExitStatus>, AppError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(error) => {
                return Err(ApiError::Internal(format!(
                    "failed to poll media transcode process startup: {error}"
                ))
                .into());
            }
        }

        if started.elapsed() >= Duration::from_millis(DOWNLOAD_TRANSCODE_STARTUP_TIMEOUT_MS) {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(DOWNLOAD_TRANSCODE_STARTUP_POLL_MS)).await;
    }
}

async fn read_child_stderr_tail(
    child: &mut tokio::process::Child,
    max_lines: usize,
) -> Option<String> {
    let mut stderr = child.stderr.take()?;
    let mut bytes = Vec::new();
    if stderr.read_to_end(&mut bytes).await.is_err() {
        return None;
    }
    let content = String::from_utf8_lossy(&bytes);
    let tail = last_non_empty_lines(&content, max_lines);
    if tail.is_empty() { None } else { Some(tail) }
}

fn build_download_startup_error(
    status: std::process::ExitStatus,
    stderr_tail: Option<&str>,
    was_fallback: bool,
) -> String {
    let phase = if was_fallback {
        "software fallback"
    } else {
        "initial"
    };
    let mut message =
        format!("media transcode process exited during {phase} startup with status {status}");
    if let Some(tail) = stderr_tail.filter(|value| !value.is_empty()) {
        message.push_str("; ffmpeg stderr: ");
        message.push_str(tail);
    }
    message
}

async fn download_playback_media(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    Query(query): Query<DownloadPlaybackQuery>,
) -> Result<Response, AppError> {
    let requested_target_height = normalize_transcode_height(query.target_height)?;
    let file = get_accessible_media_file(&auth, &state, &file_id).await?;
    let media_path = PathBuf::from(&file.path);

    if requested_target_height.is_none() {
        let file_handle = tokio::fs::File::open(&media_path)
            .await
            .map_err(|e| ApiError::Internal(format!("file open error: {e}")))?;
        let stream = ReaderStream::new(file_handle);
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                playback_download_content_type(&media_path),
            )
            .header(
                header::CONTENT_DISPOSITION,
                attachment_disposition(&playback_download_filename(&media_path, None)),
            )
            .header("Cache-Control", "no-store")
            .header("Referrer-Policy", "no-referrer")
            .header("X-Content-Type-Options", "nosniff")
            .body(Body::from_stream(stream))
            .map_err(|e| ApiError::Internal(format!("failed to build download response: {e}")))?);
    }

    let media_info = probe_media_info_for_file(&state, &media_path).await?;
    let target_height = validate_target_height_for_source(
        requested_target_height,
        media_info.video.as_ref().map(|v| v.height),
    )?
    .ok_or_else(|| ApiError::Internal("validated target height was unexpectedly missing".into()))?;

    let mut child = spawn_download_transcode_ffmpeg(&state, &media_path, target_height).await?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ApiError::Internal("ffmpeg download pipe was not available".into()))?;
    let stream = async_stream::stream! {
        let mut stdout = stdout;
        let mut child = child;
        let mut buffer = vec![0u8; 64 * 1024];

        loop {
            let read = match stdout.read(&mut buffer).await {
                Ok(read) => read,
                Err(e) => {
                    yield Err(std::io::Error::other(format!("transcode read error: {e}")));
                    break;
                }
            };
            if read == 0 {
                break;
            }
            yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buffer[..read]));
        }

        match child.wait().await {
            Ok(status) if status.success() => {}
            Ok(status) => {
                warn!(file_id = %file_id, ?status, "playback download transcode exited with non-zero status");
            }
            Err(error) => {
                warn!(file_id = %file_id, %error, "failed waiting for playback download transcode");
            }
        }
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(
            header::CONTENT_DISPOSITION,
            attachment_disposition(&playback_download_filename(
                &media_path,
                Some(target_height),
            )),
        )
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from_stream(stream))
        .map_err(|e| {
            ApiError::Internal(format!("failed to build transcode download response: {e}"))
        })?)
}

async fn stop_playback_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(sid): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .transcoder
        .stop_session(&sid)
        .await
        .map_err(|e| match e {
            rustfin_transcoder::TranscodeError::SessionNotFound(_) => {
                ApiError::NotFound("session not found".into())
            }
            other => ApiError::Internal(format!("transcode error: {other}")),
        })?;

    user_activity::stop_media_watch(&state, &auth.user_id, &sid).await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Media info (ffprobe)
// ---------------------------------------------------------------------------

async fn get_media_info(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let file = get_accessible_media_file(&auth, &state, &file_id).await?;
    let media_path = std::path::Path::new(&file.path);
    let info = probe_media_info_for_file(&state, media_path).await?;

    let payload = serde_json::to_value(&info)
        .map_err(|e| ApiError::Internal(format!("failed to serialize media info: {e}")))?;
    Ok(Json(payload))
}

// ---------------------------------------------------------------------------
// HLS serving
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct HlsAuthQuery {
    st: Option<String>,
}

#[derive(Debug)]
struct AuthorizedHlsSession {
    user_id: String,
    role: String,
    file_id: String,
    stream_token: Option<String>,
}

fn attach_stream_token_to_playlist(playlist: &str, token: &str) -> String {
    let mut out = String::with_capacity(playlist.len() + 64);
    let mut first = true;
    for line in playlist.lines() {
        if !first {
            out.push('\n');
        }
        first = false;

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
            continue;
        }

        if trimmed.contains("st=") {
            out.push_str(trimmed);
            continue;
        }

        let sep = if trimmed.contains('?') { "&" } else { "?" };
        out.push_str(trimmed);
        out.push_str(sep);
        out.push_str("st=");
        out.push_str(token);
    }
    if playlist.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn last_non_empty_lines(content: &str, max_lines: usize) -> String {
    let mut lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    lines.join(" | ")
}

async fn authorize_hls_session_request(
    state: &AppState,
    sid: &str,
    headers: &axum::http::HeaderMap,
    query: &HlsAuthQuery,
) -> Result<AuthorizedHlsSession, AppError> {
    let identity = resolve_stream_request_identity(state, headers, query.st.as_deref())?;

    let session = state
        .transcoder
        .get_session_access(sid)
        .await
        .ok_or_else(|| ApiError::NotFound("HLS session not found".into()))?;

    if session.owner_user_id != identity.user_id {
        return Err(
            ApiError::Forbidden("HLS session does not belong to this account".into()).into(),
        );
    }

    if let Some(claims) = &identity.stream_claims {
        if claims.session_id.as_deref() != Some(sid) {
            return Err(ApiError::Forbidden(
                "stream token is not scoped to this HLS session".into(),
            )
            .into());
        }
        if claims.file_id.as_deref() != Some(session.file_id.as_str()) {
            return Err(ApiError::Forbidden(
                "stream token is not scoped to this media file".into(),
            )
            .into());
        }
    }

    Ok(AuthorizedHlsSession {
        user_id: identity.user_id,
        role: identity.role,
        file_id: session.file_id,
        stream_token: query.st.clone(),
    })
}

async fn hls_master(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<HlsAuthQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    let authorized = authorize_hls_session_request(&state, &sid, &headers, &query).await?;

    // Ping the session
    if !state.transcoder.ping(&sid).await {
        return Err(ApiError::NotFound("HLS session not found".into()).into());
    }

    let path = state
        .transcoder
        .get_file_path(&sid, "master.m3u8")
        .await
        .map_err(|e| ApiError::NotFound(format!("session error: {e}")))?;

    // Wait for ffmpeg to write the playlist (up to 30s)
    for _ in 0..150 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !path.exists() {
        let ffmpeg_log_tail = state
            .transcoder
            .get_file_path(&sid, "ffmpeg.log")
            .await
            .ok()
            .and_then(|log_path| std::fs::read_to_string(log_path).ok())
            .map(|content| last_non_empty_lines(&content, 6))
            .filter(|tail| !tail.is_empty());

        let message = if let Some(tail) = ffmpeg_log_tail {
            format!("playlist not ready yet; ffmpeg log: {tail}")
        } else {
            "playlist not ready yet".to_string()
        };
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
                (header::RETRY_AFTER, "1"),
            ],
            Body::from(message),
        )
            .into_response());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("read playlist: {e}")))?;
    let stream_token = match authorized.stream_token {
        Some(t) => t,
        None => issue_stream_token(
            &authorized.user_id,
            &authorized.role,
            Some(&authorized.file_id),
            Some(&sid),
            stream_token_ttl_seconds(),
            &state.jwt_secret,
        )?,
    };
    let content = attach_stream_token_to_playlist(&content, &stream_token);

    Ok((
        [
            (
                header::CONTENT_TYPE,
                rustfin_transcoder::hls::PLAYLIST_CONTENT_TYPE,
            ),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::HeaderName::from_static("referrer-policy"),
                "no-referrer",
            ),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff",
            ),
        ],
        Body::from(content),
    )
        .into_response())
}

async fn hls_segment(
    State(state): State<AppState>,
    Path((sid, filename)): Path<(String, String)>,
    Query(query): Query<HlsAuthQuery>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    let _authorized = authorize_hls_session_request(&state, &sid, &headers, &query).await?;

    // Ping the session
    if !state.transcoder.ping(&sid).await {
        return Err(ApiError::NotFound("HLS session not found".into()).into());
    }

    // Validate filename (prevent traversal)
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(ApiError::BadRequest("invalid filename".into()).into());
    }

    let path = state
        .transcoder
        .get_file_path(&sid, &filename)
        .await
        .map_err(|e| ApiError::NotFound(format!("session error: {e}")))?;

    // Wait for segment to appear (up to 20s)
    for _ in 0..100 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !path.exists() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
                (header::RETRY_AFTER, "1"),
            ],
            Body::from("segment not ready"),
        )
            .into_response());
    }

    let content_type = if filename.ends_with(".m3u8") {
        rustfin_transcoder::hls::PLAYLIST_CONTENT_TYPE
    } else {
        rustfin_transcoder::hls::segment_content_type(&filename)
    };

    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("read segment: {e}")))?;

    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::HeaderName::from_static("referrer-policy"),
                "no-referrer",
            ),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff",
            ),
        ],
        Body::from(data),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Artwork / Images
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ImageQuery {
    w: Option<u32>,
    h: Option<u32>,
    format: Option<String>,
}

fn normalize_image_ext(ext: &str) -> Option<&'static str> {
    match ext
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" | "jpe" | "jfif" | "tbn" => Some("jpg"),
        "png" => Some("png"),
        "webp" => Some("webp"),
        "avif" => Some("avif"),
        "bmp" => Some("bmp"),
        _ => None,
    }
}

fn infer_image_ext_from_source(source: &str) -> Option<&'static str> {
    let without_query = source
        .split('#')
        .next()
        .unwrap_or(source)
        .split('?')
        .next()
        .unwrap_or(source);
    let file_component = without_query.rsplit('/').next().unwrap_or(without_query);
    let ext = StdPath::new(file_component).extension()?.to_str()?;
    normalize_image_ext(ext)
}

fn resolve_image_ext(format: Option<&str>, source: Option<&str>) -> &'static str {
    format
        .and_then(normalize_image_ext)
        .or_else(|| source.and_then(infer_image_ext_from_source))
        .unwrap_or("jpg")
}

fn content_type_for_image_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    }
}

fn cached_image_source_token(source: &str) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

async fn generate_fallback_item_image(
    state: &AppState,
    item_id: &str,
    img_type: &str,
    output_path: &StdPath,
) -> Result<bool, AppError> {
    if img_type == "logo" {
        return Ok(false);
    }

    let media_path = rustfin_db::repo::items::get_item_media_path(&state.db, item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .or(
            rustfin_db::repo::items::get_first_descendant_media_path(&state.db, item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?,
        );

    let Some(media_path) = media_path else {
        return Ok(false);
    };

    let media_file = StdPath::new(&media_path);
    if !media_file.exists() || !media_file.is_file() {
        return Ok(false);
    }

    let seek_secs =
        match rustfin_transcoder::ffprobe::probe(state.transcoder.ffprobe_path(), media_file).await
        {
            Ok(info) if info.duration_secs.is_finite() && info.duration_secs > 0.0 => {
                (info.duration_secs * 0.15).clamp(5.0, 120.0)
            }
            _ => 30.0,
        };

    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Internal(format!("cache dir error: {e}")))?;
    }

    let mut command = tokio::process::Command::new(state.transcoder.ffmpeg_path());
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(format!("{seek_secs:.3}"))
        .arg("-i")
        .arg(&media_path)
        .arg("-frames:v")
        .arg("1")
        .arg(output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let output = match command.output().await {
        Ok(output) => output,
        Err(err) => {
            warn!(
                item_id = %item_id,
                image_type = %img_type,
                error = %err,
                "fallback artwork generation could not start"
            );
            return Ok(false);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            item_id = %item_id,
            image_type = %img_type,
            stderr = %stderr,
            "fallback artwork generation failed"
        );
        return Ok(false);
    }

    Ok(true)
}

async fn get_item_image(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((item_id, img_type)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<ImageQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;

    let valid_types = ["poster", "backdrop", "logo", "thumb"];
    if !valid_types.contains(&img_type.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "invalid image type '{img_type}', must be one of: {valid_types:?}"
        ))
        .into());
    }

    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;
    let show_images =
        rustfin_db::repo::libraries::get_library_settings(&state.db, &item.library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .map(|s| s.show_images)
            .unwrap_or(true);
    if !show_images {
        return Err(ApiError::NotFound("images are disabled for this library".into()).into());
    }

    // Get the image URL from DB
    let mut image_url = rustfin_db::repo::items::get_item_image_url(&state.db, &item_id, &img_type)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if image_url.is_none() && supports_generated_item_images(&item.kind) {
        image_url = crate::artwork::resolve_fallback_item_image_path(
            &state.db,
            &item.id,
            &item.kind,
            Some(item.title.as_str()),
            item.year,
            &img_type,
        )
        .await;
    }

    // Build cache key from item_id + source + type + resize params so source changes
    // (e.g. generated frame -> Jellyfin artwork) naturally bust stale image cache.
    let source_cache_key = image_url
        .as_deref()
        .map(cached_image_source_token)
        .unwrap_or_else(|| "generated".to_string());
    let cache_key = format!(
        "{}_{}_{}_{}_{}",
        item_id,
        source_cache_key,
        img_type,
        query.w.unwrap_or(0),
        query.h.unwrap_or(0)
    );
    let images_dir = state.cache_dir.join("images");
    tokio::fs::create_dir_all(&images_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("cache dir error: {e}")))?;

    let ext = resolve_image_ext(query.format.as_deref(), image_url.as_deref());
    let cache_path = images_dir.join(format!("{cache_key}.{ext}"));

    // Check cache
    if !tokio::fs::try_exists(&cache_path)
        .await
        .map_err(|e| ApiError::Internal(format!("cache existence error: {e}")))?
    {
        if let Some(image_url) = image_url.as_deref() {
            if image_url.starts_with("http://") || image_url.starts_with("https://") {
                let resp = state
                    .http
                    .get(image_url)
                    .send()
                    .await
                    .map_err(|e| ApiError::Internal(format!("download error: {e}")))?;

                if !resp.status().is_success() {
                    return Err(ApiError::Internal(format!(
                        "image download failed: {}",
                        resp.status()
                    ))
                    .into());
                }

                let bytes = resp
                    .bytes()
                    .await
                    .map_err(|e| ApiError::Internal(format!("download error: {e}")))?;

                tokio::fs::write(&cache_path, &bytes)
                    .await
                    .map_err(|e| ApiError::Internal(format!("cache write error: {e}")))?;
            } else if tokio::fs::try_exists(StdPath::new(image_url))
                .await
                .map_err(|e| ApiError::Internal(format!("image source existence error: {e}")))?
            {
                tokio::fs::copy(image_url, &cache_path)
                    .await
                    .map_err(|e| ApiError::Internal(format!("copy error: {e}")))?;
            } else {
                return Err(ApiError::NotFound("image source not available".into()).into());
            }
        } else {
            let generated =
                generate_fallback_item_image(&state, &item_id, &img_type, &cache_path).await?;
            if !generated {
                return Err(ApiError::NotFound(format!("no {img_type} image for item")).into());
            }
        }
    }

    let metadata = tokio::fs::metadata(&cache_path)
        .await
        .map_err(|e| ApiError::Internal(format!("metadata error: {e}")))?;
    let buf = tokio::fs::read(&cache_path)
        .await
        .map_err(|e| ApiError::Internal(format!("read error: {e}")))?;

    // ETag from file size + modified time
    let etag = format!(
        "\"{:x}-{:x}\"",
        metadata.len(),
        metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

    let content_type = content_type_for_image_ext(ext);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::ETAG, etag),
            (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
        ],
        buf,
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// Subtitles
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SubtitleInfo {
    #[serde(rename = "type")]
    sub_type: String, // "sidecar" or "embedded"
    format: String,
    language: Option<String>,
    title: Option<String>,
    forced: bool,
    sdh: bool,
    /// For sidecar: URL to serve the file. For embedded: stream index.
    source: String,
}

async fn get_item_subtitles(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<Vec<SubtitleInfo>>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    // Get the media file for this item
    let file_id = rustfin_db::repo::items::get_item_file_id(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("item has no media file".into()))?;

    let file = rustfin_db::repo::media_files::get_media_file(&state.db, &file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("media file not found".into()))?;

    let media_path = std::path::Path::new(&file.path);
    let mut subtitles = Vec::new();

    // 1. Sidecar subtitles
    let sidecars = rustfin_scanner::subtitles::discover_sidecars(media_path);
    for sub in &sidecars {
        let encoded_path = base64_url_encode(&sub.path.to_string_lossy());
        subtitles.push(SubtitleInfo {
            sub_type: "sidecar".into(),
            format: format!("{:?}", sub.format).to_lowercase(),
            language: sub.language.clone(),
            title: sub.title.clone(),
            forced: sub.forced,
            sdh: sub.sdh,
            source: format!("/stream/subtitles/{encoded_path}"),
        });
    }

    // 2. Embedded subtitles (via ffprobe)
    if media_path.exists() {
        if let Ok(info) =
            rustfin_transcoder::ffprobe::probe(state.transcoder.ffprobe_path(), media_path).await
        {
            for sub in &info.subtitles {
                subtitles.push(SubtitleInfo {
                    sub_type: "embedded".into(),
                    format: sub.codec.clone(),
                    language: sub.language.clone(),
                    title: sub.title.clone(),
                    forced: sub.is_forced,
                    sdh: false,
                    source: format!("stream:{}", sub.index),
                });
            }
        }
    }

    Ok(Json(subtitles))
}

fn base64_url_encode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<String> {
    let bytes: Result<Vec<u8>, _> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect();
    bytes.ok().and_then(|b| String::from_utf8(b).ok())
}

async fn serve_subtitle(
    State(state): State<AppState>,
    Path(sub_path): Path<String>,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::response::IntoResponse;

    let decoded =
        hex_decode(&sub_path).ok_or(ApiError::BadRequest("invalid subtitle path".into()))?;

    let path = std::path::Path::new(&decoded);

    // Security: verify the path is under a library root
    let canonical = path
        .canonicalize()
        .map_err(|_| ApiError::NotFound("subtitle file not found".into()))?;

    let lib_paths = rustfin_db::repo::libraries::get_all_library_paths(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let allowed = lib_paths.iter().any(|lp| {
        if let Ok(root) = std::path::Path::new(lp).canonicalize() {
            canonical.starts_with(&root)
        } else {
            false
        }
    });

    if !allowed {
        return Err(ApiError::Forbidden("path not in allowed library".into()).into());
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("srt");

    let content_type = rustfin_scanner::subtitles::SubtitleFormat::from_extension(ext)
        .map(|f| f.mime_type())
        .unwrap_or("application/octet-stream");

    let data = tokio::fs::read(&canonical)
        .await
        .map_err(|e| ApiError::Internal(format!("read subtitle: {e}")))?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, content_type)],
        Body::from(data),
    )
        .into_response())
}

// ---------------------------------------------------------------------------
// System / GPU
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PickDirectoryResponse {
    path: String,
}

#[derive(Deserialize)]
struct HostDirectoryListQuery {
    path: Option<String>,
}

async fn list_host_directories(
    _admin: AdminUser,
    Query(query): Query<HostDirectoryListQuery>,
) -> Result<Json<HostDirectoryListResponse>, AppError> {
    let requested = query.path.and_then(|p| {
        let trimmed = p.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let response = tokio::task::spawn_blocking(move || build_host_directory_listing(requested))
        .await
        .map_err(|e| ApiError::Internal(format!("host directory listing task failed: {e}")))??;

    Ok(Json(response))
}

async fn pick_directory(_admin: AdminUser) -> Result<Json<PickDirectoryResponse>, AppError> {
    let path = tokio::task::spawn_blocking(open_directory_picker)
        .await
        .map_err(|e| ApiError::Internal(format!("directory picker task failed: {e}")))??;
    Ok(Json(PickDirectoryResponse { path }))
}

fn host_start_script() -> &'static str {
    "./scripts/start.sh"
}

fn open_directory_picker() -> Result<String, ApiError> {
    if let Ok(url) = std::env::var("RUSTFIN_DIRECTORY_PICKER_HELPER_URL") {
        let helper_url = url.trim();
        if !helper_url.is_empty() {
            let path = open_directory_picker_via_helper(helper_url)?;
            return validate_selected_media_path(&path);
        }
    }

    if let Ok(raw) = std::env::var("RUSTFIN_DIRECTORY_PICKER_PATH") {
        let path = raw.trim().to_string();
        if path.is_empty() {
            return Err(ApiError::BadRequest(
                "RUSTFIN_DIRECTORY_PICKER_PATH must not be empty".into(),
            ));
        }
        return validate_selected_media_path(&path);
    }

    let path = open_directory_picker_native()?;
    validate_selected_media_path(&path)
}

#[derive(Deserialize)]
struct DirectoryPickerHelperResponse {
    path: String,
}

fn open_directory_picker_via_helper(url: &str) -> Result<String, ApiError> {
    let client = reqwest::blocking::Client::new();
    let res = client.post(url).send().map_err(|e| {
        ApiError::BadRequest(format!(
            "directory picker helper is unavailable ({e}); restart with {}",
            host_start_script()
        ))
    })?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().unwrap_or_default();
        let detail = body.trim();
        if detail.is_empty() {
            return Err(ApiError::BadRequest(format!(
                "directory picker helper error: HTTP {status}"
            )));
        }
        return Err(ApiError::BadRequest(format!(
            "directory picker helper error: {detail}"
        )));
    }

    let parsed = res.json::<DirectoryPickerHelperResponse>().map_err(|e| {
        ApiError::Internal(format!(
            "directory picker helper returned invalid response payload: {e}"
        ))
    })?;

    let path = parsed.path.trim().to_string();
    if path.is_empty() {
        return Err(ApiError::BadRequest("directory selection cancelled".into()));
    }

    Ok(path)
}

fn validate_selected_media_path(selected_path: &str) -> Result<String, ApiError> {
    let media_root = std::env::var("RUSTFIN_MEDIA_PATH").unwrap_or_default();
    let media_root = media_root.trim();
    if media_root.is_empty() {
        return Ok(selected_path.to_string());
    }

    let normalize = |s: &str| -> String {
        s.trim()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string()
    };

    let media_root_norm = normalize(media_root);
    let selected_norm = normalize(selected_path);

    if selected_norm == media_root_norm {
        return Ok(selected_norm);
    }

    let prefix = format!("{media_root_norm}/");
    if selected_norm.strip_prefix(&prefix).is_some() {
        return Ok(selected_norm);
    }

    Err(ApiError::BadRequest(format!(
        "selected path is outside the configured media root ({media_root}); choose a folder inside it"
    )))
}

#[cfg(target_os = "macos")]
fn open_directory_picker_native() -> Result<String, ApiError> {
    let script = r#"set chosenFolder to choose folder with prompt "Select a media directory for Rustyfin"
POSIX path of chosenFolder"#;

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| ApiError::Internal(format!("failed to launch folder picker: {e}")))?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).map_err(|e| {
            ApiError::Internal(format!("folder picker returned invalid UTF-8: {e}"))
        })?;
        let path = path.trim().to_string();
        if path.is_empty() {
            return Err(ApiError::BadRequest("no directory selected".into()));
        }
        return Ok(path);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("User canceled") || stderr.contains("(-128)") {
        return Err(ApiError::BadRequest("directory selection cancelled".into()));
    }

    let detail = stderr.trim();
    if detail.is_empty() {
        return Err(ApiError::Internal(
            "folder picker failed with an unknown error".into(),
        ));
    }

    Err(ApiError::Internal(format!(
        "folder picker failed: {detail}"
    )))
}

#[cfg(not(target_os = "macos"))]
fn open_directory_picker_native() -> Result<String, ApiError> {
    #[cfg(target_os = "linux")]
    {
        open_directory_picker_linux()
    }

    #[cfg(target_os = "windows")]
    {
        open_directory_picker_windows()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err(ApiError::BadRequest(
            "directory picker is unavailable on this OS in this build; enter the path manually"
                .into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn open_directory_picker_linux() -> Result<String, ApiError> {
    let has_display = std::env::var_os("DISPLAY").is_some();
    let has_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !has_display && !has_wayland {
        return Err(ApiError::BadRequest(
            "directory picker is unavailable on a headless host; use Browse Host Directories or enter the path manually".into(),
        ));
    }

    let mut last_err: Option<ApiError> = None;

    if command_exists("zenity") {
        match run_linux_folder_picker(
            "zenity",
            &[
                "--file-selection",
                "--directory",
                "--title=Select a media directory for Rustyfin",
            ],
        ) {
            Ok(path) => return Ok(path),
            Err(err) => last_err = Some(err),
        }
    }

    if command_exists("kdialog") {
        match run_linux_folder_picker(
            "kdialog",
            &[
                "--getexistingdirectory",
                ".",
                "Select a media directory for Rustyfin",
            ],
        ) {
            Ok(path) => return Ok(path),
            Err(err) => last_err = Some(err),
        }
    }

    if std::path::Path::new("/media").exists() {
        return Ok("/media".into());
    }

    if let Some(err) = last_err {
        return Err(err);
    }

    Err(ApiError::BadRequest(
        "directory picker is unavailable: install zenity or kdialog, or use Browse Host Directories / enter the path manually"
            .into(),
    ))
}

#[cfg(target_os = "linux")]
fn command_exists(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .output()
        .is_ok()
}

#[cfg(target_os = "linux")]
fn run_linux_folder_picker(cmd: &str, args: &[&str]) -> Result<String, ApiError> {
    let output = std::process::Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| ApiError::Internal(format!("failed to launch {cmd}: {e}")))?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout)
            .map_err(|e| ApiError::Internal(format!("{cmd} returned invalid UTF-8: {e}")))?;
        let path = path.trim().to_string();
        if path.is_empty() {
            return Err(ApiError::BadRequest("no directory selected".into()));
        }
        return Ok(path);
    }

    if output.status.code() == Some(1) {
        return Err(ApiError::BadRequest("directory selection cancelled".into()));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        return Err(ApiError::Internal(format!(
            "{cmd} folder picker failed with unknown error"
        )));
    }
    Err(ApiError::Internal(format!(
        "{cmd} folder picker failed: {detail}"
    )))
}

#[cfg(target_os = "windows")]
fn open_directory_picker_windows() -> Result<String, ApiError> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a media directory for Rustyfin'
$result = $dialog.ShowDialog()
if ($result -eq [System.Windows.Forms.DialogResult]::OK) {
  Write-Output $dialog.SelectedPath
}
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| {
            ApiError::Internal(format!("failed to launch PowerShell folder picker: {e}"))
        })?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout).map_err(|e| {
            ApiError::Internal(format!(
                "PowerShell folder picker returned invalid UTF-8: {e}"
            ))
        })?;
        let path = path.trim().to_string();
        if path.is_empty() {
            return Err(ApiError::BadRequest("directory selection cancelled".into()));
        }
        return Ok(path);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        return Err(ApiError::Internal(
            "PowerShell folder picker failed with unknown error".into(),
        ));
    }
    Err(ApiError::Internal(format!(
        "PowerShell folder picker failed: {detail}"
    )))
}

async fn get_gpu_caps(
    _auth: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let caps = rustfin_transcoder::gpu::detect(std::path::Path::new("ffmpeg")).await;
    Ok(Json(serde_json::json!({
        "detected_caps": caps,
        "selected_hw_accel": state.transcoder_hw_accel,
        "require_hw_accel": state.transcoder_hw_accel_required,
    })))
}

#[derive(Debug, Serialize)]
struct TranscodeDiagnosticsResponse {
    active_sessions: usize,
    created_total: u64,
    create_failures_total: u64,
    create_failures_last_minute: u64,
    create_failures_last_five_minutes: u64,
    cleaned_total: u64,
}

#[derive(Debug, Serialize)]
struct RuntimeDiagnosticsResponse {
    runtime: crate::runtime_metrics::RuntimeMetricsSnapshot,
    transcoding: TranscodeDiagnosticsResponse,
}

async fn get_runtime_diagnostics(
    _auth: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<RuntimeDiagnosticsResponse>, AppError> {
    let runtime = state.runtime_metrics.snapshot();
    let transcoding = TranscodeDiagnosticsResponse {
        active_sessions: state.transcoder.active_count().await,
        created_total: state.transcoder.created_total(),
        create_failures_total: state.transcoder.create_failures_total(),
        create_failures_last_minute: state.transcoder.create_failures_last_minute(),
        create_failures_last_five_minutes: state.transcoder.create_failures_last_five_minutes(),
        cleaned_total: state.transcoder.cleaned_total(),
    };

    Ok(Json(RuntimeDiagnosticsResponse {
        runtime,
        transcoding,
    }))
}

#[derive(Serialize)]
struct TmdbConfigResponse {
    configured: bool,
    key_preview: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct UpdateTmdbConfigRequest {
    api_key: String,
}

fn normalize_secret(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn secret_preview(secret: &str) -> String {
    let len = secret.chars().count();
    let suffix: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if len <= 4 {
        "****".to_string()
    } else {
        format!("****{suffix}")
    }
}

async fn resolve_tmdb_key_for_admin(
    state: &AppState,
) -> Result<(Option<String>, Option<String>), AppError> {
    let db_key = rustfin_db::repo::settings::get(&state.db, "tmdb_api_key")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .and_then(|value| normalize_secret(&value));
    if let Some(key) = db_key {
        return Ok((Some(key), Some("database".to_string())));
    }

    let env_key = std::env::var("RUSTFIN_TMDB_KEY")
        .ok()
        .and_then(|value| normalize_secret(&value));
    if let Some(key) = env_key {
        return Ok((Some(key), Some("environment".to_string())));
    }

    Ok((None, None))
}

async fn get_tmdb_config(
    _auth: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<TmdbConfigResponse>, AppError> {
    let (key, source) = resolve_tmdb_key_for_admin(&state).await?;
    Ok(Json(TmdbConfigResponse {
        configured: key.is_some(),
        key_preview: key.as_deref().map(secret_preview),
        source,
    }))
}

async fn update_tmdb_config(
    _auth: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateTmdbConfigRequest>,
) -> Result<Json<TmdbConfigResponse>, AppError> {
    if let Some(key) = normalize_secret(&body.api_key) {
        rustfin_db::repo::settings::set(&state.db, "tmdb_api_key", &key)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    } else {
        let _ = rustfin_db::repo::settings::delete(&state.db, "tmdb_api_key")
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    let (key, source) = resolve_tmdb_key_for_admin(&state).await?;
    Ok(Json(TmdbConfigResponse {
        configured: key.is_some(),
        key_preview: key.as_deref().map(secret_preview),
        source,
    }))
}

// ---------------------------------------------------------------------------
// Metadata management
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RefreshMetadataRequest {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    provider_id: Option<String>,
}

async fn refresh_item_metadata(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    Json(body): Json<RefreshMetadataRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Check item exists
    let _item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &_item.library_id).await?;

    // If provider_id given, store it
    if let (Some(provider), Some(pid)) = (&body.provider, &body.provider_id) {
        rustfin_metadata::merge::set_provider_id(&state.db, &item_id, provider, pid)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    Ok(Json(serde_json::json!({
        "status": "metadata refresh queued",
        "item_id": item_id,
        "note": "TMDB API key required for provider fetch. Configure in Admin or set RUSTFIN_TMDB_KEY."
    })))
}

async fn get_item_providers(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let providers = rustfin_metadata::merge::get_provider_ids(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let map: serde_json::Map<String, serde_json::Value> = providers
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();

    Ok(Json(serde_json::Value::Object(map)))
}

#[derive(Deserialize)]
struct FieldLockRequest {
    field: String,
}

async fn lock_item_field(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    Json(body): Json<FieldLockRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    rustfin_metadata::merge::lock_field(&state.db, &item_id, &body.field)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "locked": body.field }),
    ))
}

async fn unlock_item_field(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    Json(body): Json<FieldLockRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    rustfin_metadata::merge::unlock_field(&state.db, &item_id, &body.field)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "unlocked": body.field }),
    ))
}

// ---------------------------------------------------------------------------
// TV expected / missing episodes
// ---------------------------------------------------------------------------

async fn get_expected_episodes(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<Vec<rustfin_db::repo::episodes::ExpectedEpisodeRow>>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let episodes = rustfin_db::repo::episodes::get_expected_episodes(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(episodes))
}

async fn get_missing_episodes(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<Vec<rustfin_db::repo::episodes::MissingEpisode>>, AppError> {
    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    let missing = rustfin_db::repo::episodes::get_missing_episodes(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(missing))
}

// ---------------------------------------------------------------------------
// SSE events
// ---------------------------------------------------------------------------

async fn sse_events(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> axum::response::Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::Event;
    use std::time::Duration;

    let mut rx = state.events.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(evt) => {
                    let event_type = match &evt {
                        crate::state::ServerEvent::ScanProgress { .. } => "scan_progress",
                        crate::state::ServerEvent::ScanComplete { .. } => "scan_complete",
                        crate::state::ServerEvent::MetadataRefresh { .. } => "metadata_refresh",
                        crate::state::ServerEvent::JobUpdate { .. } => "job_update",
                        crate::state::ServerEvent::Heartbeat { .. } => "heartbeat",
                    };
                    if let Ok(data) = serde_json::to_string(&evt) {
                        yield Ok(Event::default().event(event_type).data(data));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    yield Ok(Event::default()
                        .event("error")
                        .data(format!(r#"{{"lagged":{n}}}"#)));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    axum::response::Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ChildWatchOrderMode, LOGIN_ATTEMPT_BUCKETS, LOGIN_COOLDOWN_SECONDS, LOGIN_INITIAL_ATTEMPTS,
        Router, cached_image_source_token, enforce_login_rate_limit, extract_login_client_identity,
        infer_image_ext_from_source, login_attempt_key, mounted_rustyvault_router,
        normalize_session_start_time_secs, parse_episode_order_from_media_path,
        parse_episode_order_from_sort_title, parse_season_order_from_sort_title,
        parse_season_order_from_title, reset_login_rate_limit, resolve_child_watch_order_mode,
        resolve_image_ext, supports_generated_item_images,
    };
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
    use axum::routing::get;
    use axum_test::TestServer;
    use rustfin_core::error::ApiError;
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::LazyLock;
    use std::time::{Duration, Instant};

    fn build_test_state(
        rustyvault: crate::state::RustyVaultRuntimeState,
    ) -> crate::state::AppState {
        let db = PgPoolOptions::new()
            .connect_lazy("postgresql://postgres:postgres@localhost/rustfin")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig::default();
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder = Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(16);

        crate::state::AppState {
            db,
            rustyvault,
            jwt_secret: "test-secret-key".to_string(),
            http: reqwest::Client::builder().build().expect("reqwest client"),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url: "http://127.0.0.1:8102".to_string(),
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: PathBuf::from("/tmp/rustfin-rustyvault-test-cache"),
            watch_party_audio_dir: PathBuf::from("/tmp/rustfin-rustyvault-test-audio"),
            events: events_tx,
            watch_party: Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    static LOGIN_LIMIT_TEST_MUTEX: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    fn base_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("localhost:3000"));
        headers
    }

    async fn clear_login_buckets() {
        LOGIN_ATTEMPT_BUCKETS.lock().await.clear();
    }

    #[test]
    fn login_identity_prefers_peer_address_when_available() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.10, 198.51.100.5"),
        );
        let peer = ConnectInfo(
            "127.0.0.1:43210"
                .parse::<SocketAddr>()
                .expect("valid socket addr"),
        );
        let identity = extract_login_client_identity(Some(&peer), &headers);
        assert_eq!(identity, "peer:127.0.0.1");
    }

    #[test]
    fn playback_session_start_time_rejects_non_finite_inputs() {
        assert_eq!(normalize_session_start_time_secs(None, Some(90_000)), None);
        assert_eq!(
            normalize_session_start_time_secs(Some(f64::NAN), Some(90_000)),
            None
        );
        assert_eq!(
            normalize_session_start_time_secs(Some(f64::INFINITY), Some(90_000)),
            None
        );
    }

    #[test]
    fn playback_session_start_time_clamps_to_safe_duration_window() {
        assert_eq!(
            normalize_session_start_time_secs(Some(-5.0), Some(120_000)),
            Some(0.0)
        );
        assert_eq!(
            normalize_session_start_time_secs(Some(500.0), Some(120_000)),
            Some(119.5)
        );
        assert_eq!(
            normalize_session_start_time_secs(Some(24.0), Some(120_000)),
            Some(24.0)
        );
        assert_eq!(
            normalize_session_start_time_secs(Some(42.0), None),
            Some(42.0)
        );
    }

    #[tokio::test]
    async fn login_limiter_allows_five_attempts_then_cooldown() {
        let _guard = LOGIN_LIMIT_TEST_MUTEX.lock().await;
        clear_login_buckets().await;

        let headers = base_headers();
        let username = "admin";
        for _ in 0..LOGIN_INITIAL_ATTEMPTS {
            let result = enforce_login_rate_limit(None, &headers, username).await;
            assert!(result.is_ok());
        }

        let err = enforce_login_rate_limit(None, &headers, username)
            .await
            .expect_err("expected cooldown after initial budget");
        match err.0 {
            ApiError::TooManyRequests {
                retry_after_seconds,
            } => assert_eq!(retry_after_seconds, LOGIN_COOLDOWN_SECONDS),
            other => panic!("expected TooManyRequests, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn login_limiter_allows_two_attempts_after_cooldown() {
        let _guard = LOGIN_LIMIT_TEST_MUTEX.lock().await;
        clear_login_buckets().await;

        let headers = base_headers();
        let username = "admin";
        for _ in 0..LOGIN_INITIAL_ATTEMPTS {
            enforce_login_rate_limit(None, &headers, username)
                .await
                .expect("attempt should pass");
        }
        let _ = enforce_login_rate_limit(None, &headers, username)
            .await
            .expect_err("cooldown should start after initial window is exhausted");

        let key = login_attempt_key(None, &headers, username);
        {
            let mut buckets = LOGIN_ATTEMPT_BUCKETS.lock().await;
            let bucket = buckets
                .get_mut(&key)
                .expect("bucket should exist after cooldown starts");
            bucket.cooldown_until = Some(Instant::now() - Duration::from_secs(1));
            bucket.last_seen = Instant::now();
        }

        enforce_login_rate_limit(None, &headers, username)
            .await
            .expect("first post-cooldown attempt should pass");
        enforce_login_rate_limit(None, &headers, username)
            .await
            .expect("second post-cooldown attempt should pass");
        let err = enforce_login_rate_limit(None, &headers, username)
            .await
            .expect_err("third attempt in post-cooldown window should be blocked");
        match err.0 {
            ApiError::TooManyRequests {
                retry_after_seconds,
            } => assert_eq!(retry_after_seconds, LOGIN_COOLDOWN_SECONDS),
            other => panic!("expected TooManyRequests, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn login_limiter_resets_bucket_on_successful_auth() {
        let _guard = LOGIN_LIMIT_TEST_MUTEX.lock().await;
        clear_login_buckets().await;

        let headers = base_headers();
        let key = enforce_login_rate_limit(None, &headers, "admin")
            .await
            .expect("first attempt should pass");
        assert!(LOGIN_ATTEMPT_BUCKETS.lock().await.contains_key(&key));

        reset_login_rate_limit(&key).await;
        assert!(!LOGIN_ATTEMPT_BUCKETS.lock().await.contains_key(&key));
    }

    #[tokio::test]
    async fn rustyvault_unavailable_isolated_from_non_vault_routes() {
        let reason = "RustyVault is unavailable on this host.";
        let state = build_test_state(crate::state::RustyVaultRuntimeState::unavailable(reason));
        let app = Router::new()
            .route("/ok", get(|| async { StatusCode::OK }))
            .nest("/api/v1/vault", mounted_rustyvault_router(state.clone()))
            .with_state(state);
        let server = TestServer::new(app).expect("test server");

        let ok = server.get("/ok").await;
        ok.assert_status_ok();

        let vault = server.get("/api/v1/vault/config").await;
        vault.assert_status(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            vault
                .maybe_header(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok().map(str::to_string)),
            Some("no-store, max-age=0, must-revalidate".to_string())
        );
        let body: serde_json::Value = vault.json();
        assert_eq!(body["error"]["code"], "service_unavailable");
        assert_eq!(body["error"]["message"], reason);
    }

    #[test]
    fn season_order_parsing_handles_specials_and_standard_titles() {
        assert_eq!(parse_season_order_from_title("Specials"), Some(0));
        assert_eq!(parse_season_order_from_title("Season 12"), Some(12));
        assert_eq!(parse_season_order_from_title("S03"), Some(3));
    }

    #[test]
    fn internal_sort_title_parsers_extract_season_and_episode() {
        assert_eq!(
            parse_season_order_from_sort_title("rf-season-00008"),
            Some(8)
        );
        assert_eq!(
            parse_episode_order_from_sort_title("rf-season-00002-episode-00015"),
            Some((2, 15))
        );
    }

    #[test]
    fn media_path_episode_parser_reads_watch_order_tokens() {
        assert_eq!(
            parse_episode_order_from_media_path("/media/Show/Season 01/Show.S01E09.Title.mkv"),
            Some((1, 9))
        );
    }

    #[test]
    fn child_watch_order_mode_prefers_season_order_for_series_with_seasons() {
        let season_child = rustfin_db::repo::items::ItemRow {
            id: "season-1".into(),
            library_id: "lib".into(),
            kind: "season".into(),
            parent_id: Some("series-1".into()),
            title: "Season 1".into(),
            sort_title: None,
            year: None,
            overview: None,
            poster_url: None,
            backdrop_url: None,
            logo_url: None,
            thumb_url: None,
            created_ts: 0,
            updated_ts: 0,
            duration_ms: None,
        };
        let mode = resolve_child_watch_order_mode("series", &[season_child]);
        assert_eq!(mode, ChildWatchOrderMode::Season);
    }

    #[test]
    fn infer_image_ext_from_source_handles_case_and_query_strings() {
        assert_eq!(
            infer_image_ext_from_source("https://cdn.example.com/path/Poster.WEBP?x=1"),
            Some("webp")
        );
        assert_eq!(
            infer_image_ext_from_source("/mnt/media/Movie/folder.PNG"),
            Some("png")
        );
        assert_eq!(
            infer_image_ext_from_source("/mnt/media/Movie/folder.tbn"),
            Some("jpg")
        );
    }

    #[test]
    fn resolve_image_ext_prefers_explicit_query_format() {
        assert_eq!(
            resolve_image_ext(Some("png"), Some("https://cdn.example.com/poster.webp")),
            "png"
        );
        assert_eq!(resolve_image_ext(None, Some("/tmp/poster.avif")), "avif");
        assert_eq!(resolve_image_ext(None, None), "jpg");
    }

    #[test]
    fn cached_image_source_token_is_stable_and_source_specific() {
        let source_a = "/mnt/truenas_media/Movies/Example/poster.jpg";
        let source_b = "/mnt/truenas_media/Movies/Example2/poster.jpg";
        assert_eq!(
            cached_image_source_token(source_a),
            cached_image_source_token(source_a)
        );
        assert_ne!(
            cached_image_source_token(source_a),
            cached_image_source_token(source_b)
        );
    }

    #[test]
    fn generated_artwork_support_is_limited_to_video_hierarchy() {
        assert!(supports_generated_item_images("movie"));
        assert!(supports_generated_item_images("series"));
        assert!(supports_generated_item_images("episode"));
        assert!(!supports_generated_item_images("album"));
    }
}
