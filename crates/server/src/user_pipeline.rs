use regex::Regex;
use rustfin_core::error::ApiError;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::LazyLock;

use crate::error::AppError;
use crate::state::AppState;

pub const MIN_PASSWORD_LEN: usize = 12;

static USERNAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._-]{3,32}$").unwrap());

/// Validate username and password. Returns field-level errors or `None`.
pub fn validate_username_password(username: &str, password: &str) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if username.len() < 3 || username.len() > 32 || !USERNAME_RE.is_match(username) {
        fields.insert(
            "username".to_string(),
            json!(["must match ^[a-zA-Z0-9._-]{3,32}$"]),
        );
    }

    if password.len() < MIN_PASSWORD_LEN || password.len() > 1024 {
        fields.insert(
            "password".to_string(),
            json!([format!(
                "must be between {MIN_PASSWORD_LEN} and 1024 characters"
            )]),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

/// Deduplicate and trim library IDs.
pub fn normalize_library_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in ids {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if seen.insert(id.to_string()) {
            out.push(id.to_string());
        }
    }
    out
}

/// Ensure every library ID exists in the database.
pub async fn validate_library_ids_exist(
    state: &AppState,
    library_ids: &[String],
) -> Result<(), AppError> {
    for library_id in library_ids {
        let exists = rustfin_db::repo::libraries::get_library(&state.db, library_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .is_some();
        if !exists {
            return Err(ApiError::validation(json!({
                "library_ids": [format!("unknown library id: {library_id}")]
            }))
            .into());
        }
    }
    Ok(())
}

/// Create a user with role and optional library access, using shared validation.
pub async fn create_user_with_access(
    state: &AppState,
    username: &str,
    password: &str,
    role: &str,
    library_ids: &[String],
) -> Result<String, AppError> {
    if let Some(fields) = validate_username_password(username, password) {
        return Err(ApiError::validation(fields).into());
    }

    if role != "admin" && role != "user" {
        return Err(ApiError::validation(json!({
            "role": ["must be 'admin' or 'user'"]
        }))
        .into());
    }

    let library_ids = normalize_library_ids(library_ids);

    if role == "user" && library_ids.is_empty() {
        return Err(ApiError::validation(json!({
            "library_ids": ["user accounts must include at least one library"]
        }))
        .into());
    }

    if role == "admin" && !library_ids.is_empty() {
        return Err(ApiError::validation(json!({
            "library_ids": ["admin users cannot be limited to specific libraries"]
        }))
        .into());
    }

    validate_library_ids_exist(state, &library_ids).await?;

    let id = rustfin_db::repo::users::create_user(&state.db, username, password, role)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if role == "user" {
        rustfin_db::repo::users::set_library_access(&state.db, &id, &library_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    Ok(id)
}
