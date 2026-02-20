use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::auth::{AdminUser, AuthUser, issue_token};
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api_router())
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

fn api_router() -> Router<AppState> {
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
        .route("/users/me/preferences", get(get_prefs).patch(update_prefs))
        // Libraries
        .route("/libraries", post(create_library).get(list_libraries))
        .route("/libraries/{id}", get(get_library).patch(update_library))
        .route("/libraries/{id}/scan", post(scan_library))
        .route("/libraries/{id}/items", get(list_library_items))
        // Items
        .route("/items/{id}", get(get_item))
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
        .route("/playback/state/{item_id}", get(get_play_state))
        .route("/playback/sessions", post(create_playback_session))
        .route("/playback/sessions/{sid}/stop", post(stop_playback_session))
        .route("/playback/info/{file_id}", get(get_media_info))
        .route("/system/pick-directory", post(pick_directory))
        .route("/system/gpu", get(get_gpu_caps))
        .route("/events", get(sse_events))
        // Jobs
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job))
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
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
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
    role: String,
}

async fn users_me(auth: AuthUser) -> Json<UserMeResponse> {
    Json(UserMeResponse {
        id: auth.user_id,
        username: auth.username,
        role: auth.role,
    })
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
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in ids {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if seen.insert(id.to_string()) {
            normalized.push(id.to_string());
        }
    }
    normalized
}

async fn validate_library_ids_exist(
    state: &AppState,
    library_ids: &[String],
) -> Result<(), AppError> {
    for library_id in library_ids {
        let exists = rustfin_db::repo::libraries::get_library(&state.db, library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .is_some();
        if !exists {
            return Err(ApiError::BadRequest(format!("unknown library id: {library_id}")).into());
        }
    }
    Ok(())
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
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<CreateUserResponse>, AppError> {
    if body.username.is_empty() || body.password.len() < 4 {
        return Err(ApiError::BadRequest(
            "username must be non-empty and password at least 4 chars".into(),
        )
        .into());
    }
    let role = body.role;
    if role != "admin" && role != "user" {
        return Err(ApiError::BadRequest("role must be 'admin' or 'user'".into()).into());
    }
    let library_ids = normalize_library_ids(&body.library_ids);
    if role == "user" && library_ids.is_empty() {
        return Err(
            ApiError::BadRequest("user accounts must include at least one library".into()).into(),
        );
    }
    if role == "admin" && !library_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "admin users cannot be limited to specific libraries".into(),
        )
        .into());
    }
    validate_library_ids_exist(&state, &library_ids).await?;

    let id = rustfin_db::repo::users::create_user(&state.db, &body.username, &body.password, &role)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if role == "user" {
        rustfin_db::repo::users::set_library_access(&state.db, &id, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

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

    Ok(Json({
        let mut out = Vec::with_capacity(users.len());
        for u in users {
            let library_ids = if u.role == "user" {
                rustfin_db::repo::users::get_library_access(&state.db, &u.id)
                    .await
                    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            } else {
                vec![]
            };
            out.push(UserListItem {
                id: u.id,
                username: u.username,
                role: u.role,
                created_ts: u.created_ts,
                library_ids,
            });
        }
        out
    }))
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
        if final_ids.is_empty() {
            return Err(ApiError::BadRequest(
                "user accounts must include at least one library".into(),
            )
            .into());
        }
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
) -> Result<Json<serde_json::Value>, AppError> {
    let json_str = rustfin_db::repo::users::get_preferences(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "{}".to_string());

    let val: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| ApiError::Internal(format!("invalid prefs JSON: {e}")))?;

    Ok(Json(val))
}

async fn update_prefs(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, AppError> {
    let json_str = serde_json::to_string(&body)
        .map_err(|e| ApiError::Internal(format!("json serialize error: {e}")))?;

    rustfin_db::repo::users::update_preferences(&state.db, &auth.user_id, &json_str)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(body))
}

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateLibraryRequest {
    name: String,
    kind: String,
    paths: Vec<String>,
}

#[derive(Serialize)]
struct LibraryResponse {
    id: String,
    name: String,
    kind: String,
    paths: Vec<LibraryPathResponse>,
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

async fn create_library(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateLibraryRequest>,
) -> Result<(axum::http::StatusCode, Json<LibraryResponse>), AppError> {
    // Validate kind
    if body.kind != "movies" && body.kind != "tv_shows" {
        return Err(ApiError::BadRequest("kind must be 'movies' or 'tv_shows'".into()).into());
    }
    if body.paths.is_empty() {
        return Err(ApiError::BadRequest("at least one path required".into()).into());
    }

    let lib =
        rustfin_db::repo::libraries::create_library(&state.db, &body.name, &body.kind, &body.paths)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &lib.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(LibraryResponse {
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
            item_count: 0,
            created_ts: lib.created_ts,
            updated_ts: lib.updated_ts,
        }),
    ))
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

    let mut result = Vec::with_capacity(libs.len());
    for lib in libs {
        if let Some(allowed) = &allowed_library_ids {
            if !allowed.contains(&lib.id) {
                continue;
            }
        }
        let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &lib.id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let item_count = rustfin_db::repo::libraries::count_library_items(&state.db, &lib.id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        result.push(LibraryResponse {
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
            item_count,
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

    let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &lib.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let item_count = rustfin_db::repo::libraries::count_library_items(&state.db, &lib.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(LibraryResponse {
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
        item_count,
        created_ts: lib.created_ts,
        updated_ts: lib.updated_ts,
    }))
}

#[derive(Deserialize)]
struct UpdateLibraryRequest {
    name: Option<String>,
}

async fn update_library(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateLibraryRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = rustfin_db::repo::libraries::update_library(&state.db, &id, body.name.as_deref())
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !updated {
        return Err(ApiError::NotFound("library not found".into()).into());
    }

    Ok(Json(serde_json::json!({ "ok": true })))
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

    let payload = serde_json::json!({ "library_id": id });
    let job =
        rustfin_db::repo::jobs::create_job(&state.db, "library_scan", Some(&payload.to_string()))
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // Spawn scan in background
    let job_id = job.id.clone();
    let pool = state.db.clone();
    let lib_id = lib.id.clone();
    let lib_kind = lib.kind.clone();
    let events_tx = state.events.clone();
    tokio::spawn(async move {
        // Mark running
        let _ =
            rustfin_db::repo::jobs::update_job_status(&pool, &job_id, "running", 0.0, None).await;
        let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
            job_id: job_id.clone(),
            status: "running".into(),
            progress: 0.0,
        });

        match rustfin_scanner::scan::run_library_scan(&pool, &lib_id, &lib_kind).await {
            Ok(result) => {
                tracing::info!(
                    job_id = %job_id,
                    added = result.added,
                    skipped = result.skipped,
                    "scan completed"
                );
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &pool,
                    &job_id,
                    "completed",
                    1.0,
                    None,
                )
                .await;
                let _ = events_tx.send(crate::state::ServerEvent::ScanComplete {
                    library_id: lib_id,
                    job_id: job_id.clone(),
                    items_added: result.added as u64,
                });
                let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
                    job_id,
                    status: "completed".into(),
                    progress: 1.0,
                });
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "scan failed");
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &pool,
                    &job_id,
                    "failed",
                    0.0,
                    Some(&e.to_string()),
                )
                .await;
                let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
                    job_id,
                    status: "failed".into(),
                    progress: 0.0,
                });
            }
        }
    });

    Ok((axum::http::StatusCode::ACCEPTED, Json(job_to_response(job))))
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
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
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<JobResponse>>, AppError> {
    let jobs = rustfin_db::repo::jobs::list_jobs(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(jobs.into_iter().map(job_to_response).collect()))
}

async fn get_job(
    _auth: AuthUser,
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
    created_ts: i64,
    updated_ts: i64,
}

fn item_to_response(item: rustfin_db::repo::items::ItemRow) -> ItemResponse {
    ItemResponse {
        id: item.id,
        library_id: item.library_id,
        kind: item.kind,
        parent_id: item.parent_id,
        title: item.title,
        sort_title: item.sort_title,
        year: item.year,
        overview: item.overview,
        created_ts: item.created_ts,
        updated_ts: item.updated_ts,
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

    Ok(Json(items.into_iter().map(item_to_response).collect()))
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

    Ok(Json(item_to_response(item)))
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

    let children = rustfin_db::repo::items::get_children(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(children.into_iter().map(item_to_response).collect()))
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

    rustfin_db::repo::playstate::update_progress(
        &state.db,
        &auth.user_id,
        &body.item_id,
        body.progress_ms,
        body.played,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

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

// ---------------------------------------------------------------------------
// Playback sessions (HLS transcode)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateSessionRequest {
    file_id: String,
    #[serde(default)]
    start_time_secs: Option<f64>,
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
    if auth.role != "admin" {
        let item_id = rustfin_db::repo::items::get_item_id_by_file_id(&state.db, &body.file_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not playable for this account".into()))?;

        let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not playable for this account".into()))?;
        ensure_library_access(&auth, &state, &item.library_id).await?;
    }

    // Look up the media file
    let file = rustfin_db::repo::media_files::get_media_file(&state.db, &body.file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("media file not found".into()))?;

    let input_path = std::path::PathBuf::from(&file.path);
    if !input_path.exists() {
        return Err(ApiError::NotFound("media file does not exist on disk".into()).into());
    }

    let session_id = state
        .transcoder
        .create_session(input_path, body.start_time_secs, None)
        .await
        .map_err(|e| match e {
            rustfin_transcoder::TranscodeError::MaxTranscodesReached(n) => {
                ApiError::BadRequest(format!("max concurrent transcodes reached ({n})"))
            }
            other => ApiError::Internal(format!("transcode error: {other}")),
        })?;

    let hls_url = format!("/stream/hls/{session_id}/master.m3u8");

    Ok(Json(SessionResponse {
        session_id,
        hls_url,
    }))
}

async fn stop_playback_session(
    _auth: AuthUser,
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
    if auth.role != "admin" {
        let item_id = rustfin_db::repo::items::get_item_id_by_file_id(&state.db, &file_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not accessible for this account".into()))?;
        let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Forbidden("file is not accessible for this account".into()))?;
        ensure_library_access(&auth, &state, &item.library_id).await?;
    }

    let file = rustfin_db::repo::media_files::get_media_file(&state.db, &file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or(ApiError::NotFound("media file not found".into()))?;

    let info = rustfin_transcoder::ffprobe::probe(
        std::path::Path::new("ffprobe"),
        std::path::Path::new(&file.path),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("ffprobe error: {e}")))?;

    Ok(Json(serde_json::to_value(&info).unwrap()))
}

// ---------------------------------------------------------------------------
// HLS serving
// ---------------------------------------------------------------------------

async fn hls_master(
    State(state): State<AppState>,
    Path(sid): Path<String>,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::response::IntoResponse;

    // Ping the session
    if !state.transcoder.ping(&sid).await {
        return Err(ApiError::NotFound("HLS session not found".into()).into());
    }

    let path = state
        .transcoder
        .get_file_path(&sid, "master.m3u8")
        .await
        .map_err(|e| ApiError::NotFound(format!("session error: {e}")))?;

    // Wait for ffmpeg to write the playlist (up to 10s)
    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !path.exists() {
        return Err(ApiError::Internal("playlist not ready yet".into()).into());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("read playlist: {e}")))?;

    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            rustfin_transcoder::hls::PLAYLIST_CONTENT_TYPE,
        )],
        Body::from(content),
    )
        .into_response())
}

async fn hls_segment(
    State(state): State<AppState>,
    Path((sid, filename)): Path<(String, String)>,
) -> Result<axum::response::Response, AppError> {
    use axum::body::Body;
    use axum::response::IntoResponse;

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

    // Wait for segment to appear (up to 5s)
    for _ in 0..25 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !path.exists() {
        return Err(ApiError::NotFound("segment not ready".into()).into());
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
        [(axum::http::header::CONTENT_TYPE, content_type)],
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

async fn get_item_image(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((item_id, img_type)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<ImageQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    use std::io::Read;

    let valid_types = ["poster", "backdrop", "logo", "thumb"];
    if !valid_types.contains(&img_type.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "invalid image type '{}', must be one of: {:?}",
            img_type, valid_types
        ))
        .into());
    }

    let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
    ensure_library_access(&auth, &state, &item.library_id).await?;

    // Get the image URL from DB
    let image_url = rustfin_db::repo::items::get_item_image_url(&state.db, &item_id, &img_type)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("no {} image for item", img_type)))?;

    // Build cache key from item_id + type + resize params
    let cache_key = format!(
        "{}_{}_{}_{}",
        item_id,
        img_type,
        query.w.unwrap_or(0),
        query.h.unwrap_or(0)
    );
    let images_dir = state.cache_dir.join("images");
    std::fs::create_dir_all(&images_dir)
        .map_err(|e| ApiError::Internal(format!("cache dir error: {e}")))?;

    let ext = if let Some(ref fmt) = query.format {
        fmt.clone()
    } else if image_url.contains(".png") {
        "png".to_string()
    } else {
        "jpg".to_string()
    };
    let cache_path = images_dir.join(format!("{}.{}", cache_key, ext));

    // Check cache
    if !cache_path.exists() {
        // Download the image
        if image_url.starts_with("http://") || image_url.starts_with("https://") {
            let client = reqwest::Client::new();
            let resp = client
                .get(&image_url)
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

            std::fs::write(&cache_path, &bytes)
                .map_err(|e| ApiError::Internal(format!("cache write error: {e}")))?;
        } else if std::path::Path::new(&image_url).exists() {
            // Local file — copy to cache
            std::fs::copy(&image_url, &cache_path)
                .map_err(|e| ApiError::Internal(format!("copy error: {e}")))?;
        } else {
            return Err(ApiError::NotFound("image source not available".into()).into());
        }
    }

    // Read the cached file
    let mut file = std::fs::File::open(&cache_path)
        .map_err(|e| ApiError::Internal(format!("cache read error: {e}")))?;
    let metadata = file
        .metadata()
        .map_err(|e| ApiError::Internal(format!("metadata error: {e}")))?;
    let mut buf = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut buf)
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

    let content_type = match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };

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
            rustfin_transcoder::ffprobe::probe(std::path::Path::new("ffprobe"), media_path).await
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
    use std::fmt::Write;
    let mut out = String::new();
    for b in s.bytes() {
        write!(&mut out, "{:02x}", b).unwrap();
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

async fn pick_directory(_admin: AdminUser) -> Result<Json<PickDirectoryResponse>, AppError> {
    let path = tokio::task::spawn_blocking(open_directory_picker)
        .await
        .map_err(|e| ApiError::Internal(format!("directory picker task failed: {e}")))??;
    Ok(Json(PickDirectoryResponse { path }))
}

fn open_directory_picker() -> Result<String, ApiError> {
    if let Ok(raw) = std::env::var("RUSTFIN_DIRECTORY_PICKER_PATH") {
        let path = raw.trim().to_string();
        if path.is_empty() {
            return Err(ApiError::BadRequest(
                "RUSTFIN_DIRECTORY_PICKER_PATH must not be empty".into(),
            ));
        }
        return Ok(path);
    }

    open_directory_picker_native()
}

#[cfg(target_os = "macos")]
fn open_directory_picker_native() -> Result<String, ApiError> {
    if std::path::Path::new("/.dockerenv").exists() {
        return Err(ApiError::BadRequest(
            "directory picker is unavailable in Docker containers; enter the path manually".into(),
        ));
    }

    let script = r#"set chosenFolder to choose folder with prompt "Select a media directory for Rustyfin"
POSIX path of chosenFolder"#;

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| ApiError::Internal(format!("failed to launch folder picker: {e}")))?;

    if output.status.success() {
        let path = String::from_utf8(output.stdout)
            .map_err(|e| ApiError::Internal(format!("folder picker returned invalid UTF-8: {e}")))?;
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

    Err(ApiError::Internal(format!("folder picker failed: {detail}")))
}

#[cfg(not(target_os = "macos"))]
fn open_directory_picker_native() -> Result<String, ApiError> {
    Err(ApiError::BadRequest(
        "directory picker is only supported on macOS in this build; enter the path manually"
            .into(),
    ))
}

async fn get_gpu_caps(_auth: AdminUser) -> Result<Json<serde_json::Value>, AppError> {
    let caps = rustfin_transcoder::gpu::detect(std::path::Path::new("ffmpeg")).await;
    Ok(Json(serde_json::to_value(&caps).unwrap()))
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
        "note": "TMDB API key required for actual provider fetch. Set RUSTFIN_TMDB_KEY env var."
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
