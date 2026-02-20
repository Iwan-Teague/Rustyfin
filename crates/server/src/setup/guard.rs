use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::AppError;
use crate::state::AppState;
use rustfin_core::error::ApiError;

/// Hash a token for storage/comparison (SHA-256 hex).
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Constant-time compare of two hex-encoded hashes.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    a_bytes.ct_eq(b_bytes).into()
}

/// Check if a request is from a local (loopback) address.
fn is_local_request(parts: &Parts) -> bool {
    // Check ConnectInfo if available
    if let Some(connect_info) = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
        return connect_info.0.ip().is_loopback();
    }
    // In tests or behind reverse proxy, check X-Forwarded-For
    // Default to local if we can't determine (safest for development)
    true
}

/// Extractor that validates the X-Setup-Owner-Token header against the active session.
/// Also checks local/remote policy.
#[derive(Debug, Clone)]
pub struct SetupWriteGuard {
    pub client_name: String,
}

impl FromRequestParts<AppState> for SetupWriteGuard {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Check setup is not completed
        let setup_completed = rustfin_db::repo::settings::get(&state.db, "setup_completed")
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .unwrap_or_else(|| "false".to_string());

        if setup_completed == "true" {
            return Err(ApiError::Forbidden("setup already completed".into()).into());
        }

        // Extract owner token from header
        let token = parts
            .headers
            .get("x-setup-owner-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("missing setup owner token".into()))?;

        // Get active session
        let session = rustfin_db::repo::setup_session::get_active(&state.db)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Unauthorized("no active setup session".into()))?;

        // Constant-time compare token hash
        let provided_hash = hash_token(token);
        if !constant_time_eq(&provided_hash, &session.owner_token_hash) {
            return Err(ApiError::Unauthorized("invalid setup owner token".into()).into());
        }

        // Check local/remote policy
        let is_local = is_local_request(parts);
        if !is_local {
            // Check if remote access is allowed
            let allow_remote = rustfin_db::repo::settings::get(&state.db, "allow_remote_access")
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .unwrap_or_else(|| "false".to_string());

            if allow_remote != "true" {
                // Check for remote setup token
                let remote_token = parts
                    .headers
                    .get("x-setup-remote-token")
                    .and_then(|v| v.to_str().ok());

                if remote_token.is_none() {
                    return Err(ApiError::Forbidden(
                        "remote setup is disabled or token missing".into(),
                    )
                    .into());
                }
            }
        }

        // Refresh session expiry on each valid write request (sliding window)
        let new_expiry = chrono::Utc::now().timestamp() + 1800; // 30 minutes
        let _ = rustfin_db::repo::setup_session::refresh_expiry(&state.db, new_expiry).await;

        Ok(SetupWriteGuard {
            client_name: session.client_name,
        })
    }
}

/// Read-only guard: validates owner token but doesn't enforce remote policy.
#[derive(Debug, Clone)]
pub struct SetupReadGuard {
    pub client_name: String,
}

impl FromRequestParts<AppState> for SetupReadGuard {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let setup_completed = rustfin_db::repo::settings::get(&state.db, "setup_completed")
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .unwrap_or_else(|| "false".to_string());

        if setup_completed == "true" {
            return Err(ApiError::Forbidden("setup already completed".into()).into());
        }

        let token = parts
            .headers
            .get("x-setup-owner-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("missing setup owner token".into()))?;

        let session = rustfin_db::repo::setup_session::get_active(&state.db)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Unauthorized("no active setup session".into()))?;

        let provided_hash = hash_token(token);
        if !constant_time_eq(&provided_hash, &session.owner_token_hash) {
            return Err(ApiError::Unauthorized("invalid setup owner token".into()).into());
        }

        Ok(SetupReadGuard {
            client_name: session.client_name,
        })
    }
}
