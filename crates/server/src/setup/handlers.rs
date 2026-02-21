use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rustfin_core::error::{ApiError, ErrorBody, ErrorEnvelope};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::setup::guard::{SetupReadGuard, SetupWriteGuard, hash_token};
use crate::setup::state_machine::SetupState;
use crate::setup::validation;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helper: get current setup state from DB
// ---------------------------------------------------------------------------

async fn get_setup_state(db: &sqlx::SqlitePool) -> Result<SetupState, AppError> {
    let state_str = rustfin_db::repo::settings::get(db, "setup_state")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "NotStarted".to_string());

    SetupState::from_str(&state_str)
        .ok_or_else(|| ApiError::Internal(format!("invalid setup state: {state_str}")).into())
}

/// Return a 409 setup_state_violation error response.
fn state_violation_response(current: SetupState, expected_min: SetupState) -> Response {
    let envelope = ErrorEnvelope {
        error: ErrorBody {
            code: "setup_state_violation".to_string(),
            message: "conflict: setup state violation".to_string(),
            details: json!({
                "current_state": current.as_str(),
                "expected_min_state": expected_min.as_str(),
            }),
        },
    };
    (StatusCode::CONFLICT, Json(envelope)).into_response()
}

/// Return a custom error response.
fn custom_error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    details: serde_json::Value,
) -> Response {
    let envelope = ErrorEnvelope {
        error: ErrorBody {
            code: code.to_string(),
            message: message.to_string(),
            details,
        },
    };
    (status, Json(envelope)).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/v1/system/info/public
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PublicSystemInfo {
    server_name: String,
    version: String,
    setup_completed: bool,
    setup_state: String,
}

pub async fn get_public_system_info(
    State(state): State<AppState>,
) -> Result<Json<PublicSystemInfo>, AppError> {
    let server_name = rustfin_db::repo::settings::get(&state.db, "server_name")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "Rustyfin".to_string());

    let setup_completed = rustfin_db::repo::settings::get(&state.db, "setup_completed")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "false".to_string());

    let setup_state = rustfin_db::repo::settings::get(&state.db, "setup_state")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or_else(|| "NotStarted".to_string());

    Ok(Json(PublicSystemInfo {
        server_name,
        version: env!("CARGO_PKG_VERSION").to_string(),
        setup_completed: setup_completed == "true",
        setup_state,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/session/claim
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ClaimSessionRequest {
    client_name: String,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    confirm_takeover: bool,
}

#[derive(Serialize)]
pub struct ClaimSessionResponse {
    owner_token: String,
    expires_at: String,
    claimed_by: String,
    setup_state: String,
}

pub async fn claim_session(
    State(state): State<AppState>,
    Json(body): Json<ClaimSessionRequest>,
) -> Response {
    // Check setup not completed
    let setup_completed = match rustfin_db::repo::settings::get(&state.db, "setup_completed").await
    {
        Ok(Some(v)) => v == "true",
        Ok(None) => false,
        Err(e) => {
            return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
        }
    };

    if setup_completed {
        return custom_error_response(
            StatusCode::CONFLICT,
            "setup_state_violation",
            "conflict: setup already completed",
            json!({}),
        );
    }

    // Validate client_name
    if body.client_name.is_empty() || body.client_name.len() > 128 {
        return AppError::from(ApiError::validation(json!({
            "client_name": ["must be between 1 and 128 characters"]
        })))
        .into_response();
    }

    // Check if session already exists
    let existing = match rustfin_db::repo::setup_session::get_active(&state.db).await {
        Ok(s) => s,
        Err(e) => {
            return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
        }
    };

    if let Some(session) = &existing {
        if !body.force || !body.confirm_takeover {
            let expires = chrono::DateTime::from_timestamp(session.expires_at, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            return custom_error_response(
                StatusCode::CONFLICT,
                "setup_claimed",
                "Setup is currently being configured.",
                json!({
                    "claimed_by": session.client_name,
                    "expires_at": expires,
                }),
            );
        }
        // Force takeover: fall through to claim
    }

    // Generate token
    let token = uuid::Uuid::new_v4().to_string();
    let token_hash = hash_token(&token);
    let expires_at = chrono::Utc::now().timestamp() + 1800; // 30 min

    if let Err(e) = rustfin_db::repo::setup_session::claim(
        &state.db,
        &token_hash,
        &body.client_name,
        expires_at,
    )
    .await
    {
        return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
    }

    // Update setup state to SessionClaimed if currently NotStarted
    let current_state = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if current_state == SetupState::NotStarted {
        if let Err(e) =
            rustfin_db::repo::settings::set(&state.db, "setup_state", "SessionClaimed").await
        {
            return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
        }
    }

    let setup_state = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let expires_dt = chrono::DateTime::from_timestamp(expires_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    info!(client = %body.client_name, "setup session claimed");

    (
        StatusCode::OK,
        Json(ClaimSessionResponse {
            owner_token: token,
            expires_at: expires_dt,
            claimed_by: body.client_name,
            setup_state: setup_state.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/session/release
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReleaseSessionResponse {
    released: bool,
}

pub async fn release_session(
    _guard: SetupReadGuard,
    State(state): State<AppState>,
) -> Result<Json<ReleaseSessionResponse>, AppError> {
    let released = rustfin_db::repo::setup_session::release(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    info!("setup session released");

    Ok(Json(ReleaseSessionResponse { released }))
}

// ---------------------------------------------------------------------------
// GET /api/v1/setup/config
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct SetupConfig {
    server_name: String,
    default_ui_locale: String,
    default_region: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_time_zone: Option<String>,
}

pub async fn get_setup_config(_guard: SetupReadGuard, State(state): State<AppState>) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if current == SetupState::Completed {
        return state_violation_response(current, SetupState::SessionClaimed);
    }

    let server_name = rustfin_db::repo::settings::get(&state.db, "server_name")
        .await
        .unwrap_or(Some("Rustyfin".to_string()))
        .unwrap_or_else(|| "Rustyfin".to_string());

    let locale = rustfin_db::repo::settings::get(&state.db, "default_ui_locale")
        .await
        .unwrap_or(Some("en".to_string()))
        .unwrap_or_else(|| "en".to_string());

    let region = rustfin_db::repo::settings::get(&state.db, "default_region")
        .await
        .unwrap_or(Some("US".to_string()))
        .unwrap_or_else(|| "US".to_string());

    let tz = rustfin_db::repo::settings::get(&state.db, "default_time_zone")
        .await
        .unwrap_or(None);
    let tz = tz.and_then(|t| if t.is_empty() { None } else { Some(t) });

    (
        StatusCode::OK,
        Json(SetupConfig {
            server_name,
            default_ui_locale: locale,
            default_region: region,
            default_time_zone: tz,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// PUT /api/v1/setup/config
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct OkWithSetupState {
    ok: bool,
    setup_state: String,
}

pub async fn put_setup_config(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    Json(body): Json<SetupConfig>,
) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !current.is_at_least(SetupState::SessionClaimed) || current.is_completed() {
        return state_violation_response(current, SetupState::SessionClaimed);
    }

    // Validate
    if let Some(fields) = validation::validate_config(
        &body.server_name,
        &body.default_ui_locale,
        &body.default_region,
        &body.default_time_zone,
    ) {
        return AppError::from(ApiError::validation(fields)).into_response();
    }

    // Save settings
    let db = &state.db;
    macro_rules! set {
        ($k:expr, $v:expr) => {
            if let Err(e) = rustfin_db::repo::settings::set(db, $k, $v).await {
                return AppError::from(ApiError::Internal(format!("db error: {e}")))
                    .into_response();
            }
        };
    }

    set!("server_name", &body.server_name);
    set!("default_ui_locale", &body.default_ui_locale);
    set!("default_region", &body.default_region);
    set!(
        "default_time_zone",
        body.default_time_zone.as_deref().unwrap_or("")
    );
    set!("setup_state", SetupState::ServerConfigSaved.as_str());

    info!("setup config saved");

    (
        StatusCode::OK,
        Json(OkWithSetupState {
            ok: true,
            setup_state: SetupState::ServerConfigSaved.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/admin
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateAdminRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct CreateAdminResponse {
    user_id: String,
    setup_state: String,
}

pub async fn create_admin(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateAdminRequest>,
) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !current.is_at_least(SetupState::SessionClaimed) || current.is_completed() {
        return state_violation_response(current, SetupState::SessionClaimed);
    }

    // Validate
    if let Some(fields) = validation::validate_admin(&body.username, &body.password) {
        return AppError::from(ApiError::validation(fields)).into_response();
    }

    // Check idempotency key
    let idem_key = match headers.get("idempotency-key").and_then(|v| v.to_str().ok()) {
        Some(k) if k.len() >= 8 && k.len() <= 128 => k.to_string(),
        Some(_) => {
            return AppError::from(ApiError::validation(json!({
                "Idempotency-Key": ["must be between 8 and 128 characters"]
            })))
            .into_response();
        }
        None => {
            return AppError::from(ApiError::BadRequest(
                "Idempotency-Key header is required".into(),
            ))
            .into_response();
        }
    };

    // Compute payload hash (don't include password in hash for security)
    let payload_hash = {
        let mut hasher = Sha256::new();
        hasher.update(body.username.as_bytes());
        hasher.update(b":create_admin");
        hex::encode(hasher.finalize())
    };

    // Check for existing idempotency key
    match rustfin_db::repo::idempotency::lookup(&state.db, &idem_key).await {
        Ok(Some(record)) => {
            if record.payload_hash == payload_hash {
                // Replay cached response
                let resp_body: serde_json::Value =
                    serde_json::from_str(&record.response).unwrap_or(json!({}));
                return (
                    StatusCode::from_u16(record.status_code as u16).unwrap_or(StatusCode::CREATED),
                    Json(resp_body),
                )
                    .into_response();
            } else {
                return custom_error_response(
                    StatusCode::CONFLICT,
                    "idempotency_conflict",
                    "conflict: idempotency key payload mismatch",
                    json!({}),
                );
            }
        }
        Ok(None) => {} // proceed
        Err(e) => {
            return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
        }
    }

    // Check if any admin user already exists
    let user_count = match rustfin_db::repo::users::count_users(&state.db).await {
        Ok(c) => c,
        Err(e) => {
            return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
        }
    };

    if user_count > 0 {
        return custom_error_response(
            StatusCode::CONFLICT,
            "admin_already_exists",
            "conflict: admin already exists",
            json!({}),
        );
    }

    // Create admin user
    let user_id = match rustfin_db::repo::users::create_user(
        &state.db,
        &body.username,
        &body.password,
        "admin",
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            return AppError::from(ApiError::Internal(format!("failed to create admin: {e}")))
                .into_response();
        }
    };

    // Update setup state
    if let Err(e) =
        rustfin_db::repo::settings::set(&state.db, "setup_state", SetupState::AdminCreated.as_str())
            .await
    {
        return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
    }

    let response = CreateAdminResponse {
        user_id: user_id.clone(),
        setup_state: SetupState::AdminCreated.as_str().to_string(),
    };

    // Cache idempotency response
    let resp_json = serde_json::to_string(&response).unwrap_or_default();
    let _ = rustfin_db::repo::idempotency::store(
        &state.db,
        &idem_key,
        "create_admin",
        &payload_hash,
        &resp_json,
        201,
    )
    .await;

    info!(username = %body.username, "admin user created during setup");

    (StatusCode::CREATED, Json(response)).into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/paths/validate
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ValidatePathRequest {
    path: String,
}

#[derive(Serialize)]
pub struct ValidatePathResponse {
    path: String,
    exists: bool,
    readable: bool,
    writable: bool,
    hint: Option<String>,
}

pub async fn validate_path(
    _guard: SetupWriteGuard,
    State(_state): State<AppState>,
    Json(body): Json<ValidatePathRequest>,
) -> Response {
    // Validate input
    if let Some(fields) = validation::validate_path_input(&body.path) {
        return AppError::from(ApiError::validation(fields)).into_response();
    }

    let path = std::path::Path::new(&body.path);
    let exists = path.exists();
    let readable = if exists {
        path.read_dir().is_ok()
    } else {
        false
    };

    // Check writable by attempting to check metadata permissions
    let writable = if exists {
        // Try to check if we can write to the directory
        let test_file = path.join(".rustyfin_write_test");
        let can_write = std::fs::write(&test_file, b"test").is_ok();
        if can_write {
            let _ = std::fs::remove_file(&test_file);
        }
        can_write
    } else {
        false
    };

    let hint = if !exists {
        Some("Path does not exist on the server filesystem".to_string())
    } else if !readable {
        Some("Path exists but is not readable by the server process".to_string())
    } else if !writable {
        // Not an error — read-only paths are valid for media
        None
    } else {
        None
    };

    // Do not log the actual path for security
    info!("path validation completed");

    (
        StatusCode::OK,
        Json(ValidatePathResponse {
            path: body.path,
            exists,
            readable,
            writable,
            hint,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/libraries
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LibrarySpec {
    name: String,
    kind: String,
    paths: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    is_read_only: bool,
}

#[derive(Deserialize)]
pub struct CreateLibrariesRequest {
    libraries: Vec<LibrarySpec>,
}

#[derive(Serialize)]
pub struct LibraryRef {
    id: String,
    name: String,
}

#[derive(Serialize)]
pub struct CreateLibrariesResponse {
    created: usize,
    libraries: Vec<LibraryRef>,
    setup_state: String,
}

pub async fn create_libraries(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    Json(body): Json<CreateLibrariesRequest>,
) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !current.is_at_least(SetupState::AdminCreated) || current.is_completed() {
        return state_violation_response(current, SetupState::AdminCreated);
    }

    // Validate libraries count
    if body.libraries.is_empty() || body.libraries.len() > 32 {
        return AppError::from(ApiError::validation(json!({
            "libraries": ["must have between 1 and 32 libraries"]
        })))
        .into_response();
    }

    // Validate each library
    for (i, lib) in body.libraries.iter().enumerate() {
        if let Some(fields) = validation::validate_library_spec(&lib.name, &lib.kind, &lib.paths) {
            let mut wrapped = serde_json::Map::new();
            for (k, v) in fields.as_object().unwrap() {
                wrapped.insert(format!("libraries[{i}].{k}"), v.clone());
            }
            return AppError::from(ApiError::validation(serde_json::Value::Object(wrapped)))
                .into_response();
        }
    }

    // Map OpenAPI kinds to DB kinds
    fn map_kind(kind: &str) -> &str {
        match kind {
            "movie" => "movies",
            "show" => "tv_shows",
            "music" => "music",
            "mixed" => "mixed",
            other => other,
        }
    }

    let mut created_libs = Vec::new();
    for lib in &body.libraries {
        let db_kind = map_kind(&lib.kind);
        match rustfin_db::repo::libraries::create_library(&state.db, &lib.name, db_kind, &lib.paths)
            .await
        {
            Ok(row) => {
                if let Err(e) =
                    crate::library_scan::enqueue_library_scan(&state, &row.id, &row.kind).await
                {
                    tracing::warn!(
                        library_id = %row.id,
                        status = e.0.status_code(),
                        "setup library created but auto-scan enqueue failed"
                    );
                }
                created_libs.push(LibraryRef {
                    id: row.id,
                    name: row.name,
                });
            }
            Err(e) => {
                return AppError::from(ApiError::Internal(format!(
                    "failed to create library '{}': {e}",
                    lib.name
                )))
                .into_response();
            }
        }
    }

    // Advance state
    if let Err(e) = rustfin_db::repo::settings::set(
        &state.db,
        "setup_state",
        SetupState::LibrariesSaved.as_str(),
    )
    .await
    {
        return AppError::from(ApiError::Internal(format!("db error: {e}"))).into_response();
    }

    info!(count = created_libs.len(), "libraries created during setup");

    (
        StatusCode::OK,
        Json(CreateLibrariesResponse {
            created: created_libs.len(),
            libraries: created_libs,
            setup_state: SetupState::LibrariesSaved.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /api/v1/setup/metadata
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct SetupMetadata {
    metadata_language: String,
    metadata_region: String,
}

pub async fn get_setup_metadata(_guard: SetupReadGuard, State(state): State<AppState>) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if current.is_completed() {
        return state_violation_response(current, SetupState::SessionClaimed);
    }

    let language = rustfin_db::repo::settings::get(&state.db, "metadata_language")
        .await
        .unwrap_or(Some("en".to_string()))
        .unwrap_or_else(|| "en".to_string());

    let region = rustfin_db::repo::settings::get(&state.db, "metadata_region")
        .await
        .unwrap_or(Some("US".to_string()))
        .unwrap_or_else(|| "US".to_string());

    (
        StatusCode::OK,
        Json(SetupMetadata {
            metadata_language: language,
            metadata_region: region,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// PUT /api/v1/setup/metadata
// ---------------------------------------------------------------------------

pub async fn put_setup_metadata(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    Json(body): Json<SetupMetadata>,
) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Must be at least AdminCreated (libraries step is optional, so skip that gate)
    if !current.is_at_least(SetupState::AdminCreated) || current.is_completed() {
        return state_violation_response(current, SetupState::AdminCreated);
    }

    if let Some(fields) =
        validation::validate_metadata(&body.metadata_language, &body.metadata_region)
    {
        return AppError::from(ApiError::validation(fields)).into_response();
    }

    let db = &state.db;
    macro_rules! set {
        ($k:expr, $v:expr) => {
            if let Err(e) = rustfin_db::repo::settings::set(db, $k, $v).await {
                return AppError::from(ApiError::Internal(format!("db error: {e}")))
                    .into_response();
            }
        };
    }

    set!("metadata_language", &body.metadata_language);
    set!("metadata_region", &body.metadata_region);
    set!("setup_state", SetupState::MetadataSaved.as_str());

    info!("metadata config saved during setup");

    (
        StatusCode::OK,
        Json(OkWithSetupState {
            ok: true,
            setup_state: SetupState::MetadataSaved.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /api/v1/setup/network
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct SetupNetwork {
    allow_remote_access: bool,
    enable_automatic_port_mapping: bool,
    trusted_proxies: Vec<String>,
}

pub async fn get_setup_network(_guard: SetupReadGuard, State(state): State<AppState>) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if current.is_completed() {
        return state_violation_response(current, SetupState::SessionClaimed);
    }

    let allow_remote = rustfin_db::repo::settings::get(&state.db, "allow_remote_access")
        .await
        .unwrap_or(Some("false".to_string()))
        .unwrap_or_else(|| "false".to_string())
        == "true";

    let auto_port = rustfin_db::repo::settings::get(&state.db, "enable_automatic_port_mapping")
        .await
        .unwrap_or(Some("false".to_string()))
        .unwrap_or_else(|| "false".to_string())
        == "true";

    let proxies_json = rustfin_db::repo::settings::get(&state.db, "trusted_proxies")
        .await
        .unwrap_or(Some("[]".to_string()))
        .unwrap_or_else(|| "[]".to_string());

    let proxies: Vec<String> = serde_json::from_str(&proxies_json).unwrap_or_default();

    (
        StatusCode::OK,
        Json(SetupNetwork {
            allow_remote_access: allow_remote,
            enable_automatic_port_mapping: auto_port,
            trusted_proxies: proxies,
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// PUT /api/v1/setup/network
// ---------------------------------------------------------------------------

pub async fn put_setup_network(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    Json(body): Json<SetupNetwork>,
) -> Response {
    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !current.is_at_least(SetupState::AdminCreated) || current.is_completed() {
        return state_violation_response(current, SetupState::AdminCreated);
    }

    if let Some(fields) = validation::validate_network(&body.trusted_proxies) {
        return AppError::from(ApiError::validation(fields)).into_response();
    }

    let db = &state.db;
    macro_rules! set {
        ($k:expr, $v:expr) => {
            if let Err(e) = rustfin_db::repo::settings::set(db, $k, $v).await {
                return AppError::from(ApiError::Internal(format!("db error: {e}")))
                    .into_response();
            }
        };
    }

    set!(
        "allow_remote_access",
        if body.allow_remote_access {
            "true"
        } else {
            "false"
        }
    );
    set!(
        "enable_automatic_port_mapping",
        if body.enable_automatic_port_mapping {
            "true"
        } else {
            "false"
        }
    );
    let proxies_json =
        serde_json::to_string(&body.trusted_proxies).unwrap_or_else(|_| "[]".to_string());
    set!("trusted_proxies", &proxies_json);
    set!("setup_state", SetupState::NetworkSaved.as_str());

    info!("network config saved during setup");

    (
        StatusCode::OK,
        Json(OkWithSetupState {
            ok: true,
            setup_state: SetupState::NetworkSaved.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/complete
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CompleteSetupRequest {
    confirm: bool,
}

#[derive(Serialize)]
pub struct CompleteSetupResponse {
    setup_completed: bool,
    setup_state: String,
}

pub async fn complete_setup(
    _guard: SetupWriteGuard,
    State(state): State<AppState>,
    Json(body): Json<CompleteSetupRequest>,
) -> Response {
    if !body.confirm {
        return AppError::from(ApiError::validation(json!({
            "confirm": ["must be true to complete setup"]
        })))
        .into_response();
    }

    let current = match get_setup_state(&state.db).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Already completed — idempotent
    if current.is_completed() {
        return (
            StatusCode::OK,
            Json(CompleteSetupResponse {
                setup_completed: true,
                setup_state: SetupState::Completed.as_str().to_string(),
            }),
        )
            .into_response();
    }

    // Must have at least created admin
    if !current.is_at_least(SetupState::AdminCreated) {
        return state_violation_response(current, SetupState::AdminCreated);
    }

    let db = &state.db;
    macro_rules! set {
        ($k:expr, $v:expr) => {
            if let Err(e) = rustfin_db::repo::settings::set(db, $k, $v).await {
                return AppError::from(ApiError::Internal(format!("db error: {e}")))
                    .into_response();
            }
        };
    }

    set!("setup_state", SetupState::Completed.as_str());
    set!("setup_completed", "true");

    // Release setup session
    let _ = rustfin_db::repo::setup_session::release(&state.db).await;

    info!("setup completed successfully");

    (
        StatusCode::OK,
        Json(CompleteSetupResponse {
            setup_completed: true,
            setup_state: SetupState::Completed.as_str().to_string(),
        }),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/v1/setup/reset (admin-only, JWT auth)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ResetSetupRequest {
    confirm: String,
    delete_users: bool,
    delete_settings: bool,
}

#[derive(Serialize)]
pub struct ResetSetupResponse {
    reset: bool,
    setup_completed: bool,
    setup_state: String,
}

pub async fn reset_setup(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<ResetSetupRequest>,
) -> Response {
    if body.confirm != "RESET" {
        return AppError::from(ApiError::validation(json!({
            "confirm": ["must be the string \"RESET\""]
        })))
        .into_response();
    }

    let db = &state.db;

    // Delete users if requested
    if body.delete_users {
        let users = match rustfin_db::repo::users::list_users(db).await {
            Ok(u) => u,
            Err(e) => {
                return AppError::from(ApiError::Internal(format!("db error: {e}")))
                    .into_response();
            }
        };
        for user in users {
            let _ = rustfin_db::repo::users::delete_user(db, &user.id).await;
        }
    }

    // Delete settings if requested, then re-insert defaults
    if body.delete_settings {
        let _ = rustfin_db::repo::settings::delete_all(db).await;
        let _ = rustfin_db::repo::settings::insert_defaults(db).await;
    } else {
        // Just reset setup state
        let _ = rustfin_db::repo::settings::set(db, "setup_completed", "false").await;
        let _ = rustfin_db::repo::settings::set(db, "setup_state", "NotStarted").await;
    }

    // Clear setup session and idempotency keys
    let _ = rustfin_db::repo::setup_session::release(db).await;
    let _ = rustfin_db::repo::idempotency::delete_all(db).await;

    info!("setup reset performed");

    (
        StatusCode::OK,
        Json(ResetSetupResponse {
            reset: true,
            setup_completed: false,
            setup_state: SetupState::NotStarted.as_str().to_string(),
        }),
    )
        .into_response()
}
