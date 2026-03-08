use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::net::IpAddr;
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

fn first_forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .and_then(|first| first.trim().parse::<IpAddr>().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|raw| raw.trim().parse::<IpAddr>().ok())
        })
}

async fn trusted_proxy_ips(state: &AppState) -> HashSet<IpAddr> {
    let raw = rustfin_db::repo::settings::get(&state.db, "trusted_proxies")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "[]".to_string());
    let list: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    let mut out = HashSet::new();
    for candidate in list {
        if let Ok(ip) = candidate.trim().parse::<IpAddr>() {
            out.insert(ip);
        }
    }
    out
}

async fn resolve_client_ip(parts: &Parts, state: &AppState) -> Result<IpAddr, AppError> {
    let peer_ip = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip())
        .ok_or_else(|| ApiError::Forbidden("unable to determine client network address".into()))?;

    let trusted = trusted_proxy_ips(state).await;
    if !trusted.contains(&peer_ip) {
        return Ok(peer_ip);
    }

    first_forwarded_ip(&parts.headers).ok_or_else(|| {
        ApiError::Forbidden(
            "trusted proxy request is missing a valid forwarded client address".into(),
        )
        .into()
    })
}

/// Check if a request is from a local (loopback) address.
async fn is_local_request(parts: &Parts, state: &AppState) -> Result<bool, AppError> {
    Ok(resolve_client_ip(parts, state).await?.is_loopback())
}

async fn setup_remote_allowed(state: &AppState) -> Result<bool, AppError> {
    let allow_remote = rustfin_db::repo::settings::get(&state.db, "allow_remote_access")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(matches!(allow_remote.as_deref(), Some("true")))
}

fn host_header_suggests_local(parts: &Parts) -> bool {
    parts
        .headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| {
            let host = host.trim().to_ascii_lowercase();
            host.starts_with("localhost")
                || host.starts_with("127.0.0.1")
                || host.starts_with("[::1]")
        })
        .unwrap_or(false)
}

async fn enforce_setup_remote_policy(
    parts: &Parts,
    state: &AppState,
    session_owner_token_hash: &str,
) -> Result<(), AppError> {
    let is_local = match is_local_request(parts, state).await {
        Ok(value) => value,
        Err(err) => {
            // If ConnectInfo is unavailable but request host is loopback-like, allow local dev/tests.
            // Otherwise fail closed.
            if host_header_suggests_local(parts) {
                true
            } else {
                return Err(err);
            }
        }
    };
    if is_local {
        return Ok(());
    }

    if setup_remote_allowed(state).await? {
        return Ok(());
    }

    let remote_token = parts
        .headers
        .get("x-setup-remote-token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            ApiError::Forbidden(
                "remote setup is disabled; enable remote setup or provide a valid setup remote token"
                    .into(),
            )
        })?;

    let remote_hash = hash_token(remote_token);
    if !constant_time_eq(&remote_hash, session_owner_token_hash) {
        return Err(ApiError::Forbidden("invalid setup remote token".into()).into());
    }

    Ok(())
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

        enforce_setup_remote_policy(parts, state, &session.owner_token_hash).await?;

        // Refresh session expiry on each valid write request (sliding window)
        let new_expiry = chrono::Utc::now().timestamp() + 1800; // 30 minutes
        let _ = rustfin_db::repo::setup_session::refresh_expiry(&state.db, new_expiry).await;

        Ok(SetupWriteGuard {
            client_name: session.client_name,
        })
    }
}

/// Read-only guard: validates owner token and enforces the same local/remote policy.
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

        enforce_setup_remote_policy(parts, state, &session.owner_token_hash).await?;

        Ok(SetupReadGuard {
            client_name: session.client_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{first_forwarded_ip, host_header_suggests_local};
    use axum::http::header;
    use axum::http::{HeaderValue, Request};
    use std::net::IpAddr;

    #[test]
    fn forwarded_for_uses_first_ip() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.2, 198.51.100.19"),
        );
        let parsed = first_forwarded_ip(&headers);
        assert_eq!(parsed, Some(IpAddr::from([203, 0, 113, 2])));
    }

    #[test]
    fn real_ip_used_when_forwarded_for_missing() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.25"));
        let parsed = first_forwarded_ip(&headers);
        assert_eq!(parsed, Some(IpAddr::from([198, 51, 100, 25])));
    }

    #[test]
    fn host_header_local_detection_supports_loopback_hosts() {
        let mut request = Request::new(());
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_static("localhost:3000"));
        let (parts, _body) = request.into_parts();
        assert!(host_header_suggests_local(&parts));

        let mut request = Request::new(());
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_static("127.0.0.1:3000"));
        let (parts, _body) = request.into_parts();
        assert!(host_header_suggests_local(&parts));
    }

    #[test]
    fn host_header_local_detection_rejects_remote_hosts() {
        let mut request = Request::new(());
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_static("example.com"));
        let (parts, _body) = request.into_parts();
        assert!(!host_header_suggests_local(&parts));
    }

    #[test]
    fn host_header_local_detection_handles_missing_host() {
        let request = Request::new(());
        let (parts, _body) = request.into_parts();
        assert!(!host_header_suggests_local(&parts));
    }
}
