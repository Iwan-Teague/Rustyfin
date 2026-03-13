use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rustfin_core::error::ErrorEnvelope;

use crate::rustyvault_host::service;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

const VAULT_KEY_PREFIX_LEN: usize = 16;

pub const VAULT_BOOTSTRAP_RATE_LIMIT_REQUESTS: u64 = 4;
pub const VAULT_REKEY_RATE_LIMIT_REQUESTS: u64 = 6;
pub const VAULT_EXPORT_RATE_LIMIT_REQUESTS: u64 = 8;
pub const VAULT_IMPORT_RATE_LIMIT_REQUESTS: u64 = 6;
pub const VAULT_DESTROY_RATE_LIMIT_REQUESTS: u64 = 3;
pub const VAULT_DEVICE_SESSION_PAIR_RATE_LIMIT_REQUESTS: u64 = 12;
pub const VAULT_PROTECTED_ACTION_CHALLENGE_RATE_LIMIT_REQUESTS: u64 = 10;
pub const VAULT_LOOKUP_RATE_LIMIT_REQUESTS: u64 = 120;

#[derive(Clone)]
pub struct RustyVaultRateLimiters {
    bootstrap: RateLimiter,
    rekey: RateLimiter,
    export: RateLimiter,
    import_overwrite: RateLimiter,
    destroy: RateLimiter,
    device_session_pair: RateLimiter,
    protected_action_challenge: RateLimiter,
    lookup: RateLimiter,
}

impl RustyVaultRateLimiters {
    pub fn new() -> Self {
        Self {
            bootstrap: RateLimiter::new(VAULT_BOOTSTRAP_RATE_LIMIT_REQUESTS, 15 * 60),
            rekey: RateLimiter::new(VAULT_REKEY_RATE_LIMIT_REQUESTS, 15 * 60),
            export: RateLimiter::new(VAULT_EXPORT_RATE_LIMIT_REQUESTS, 15 * 60),
            import_overwrite: RateLimiter::new(VAULT_IMPORT_RATE_LIMIT_REQUESTS, 15 * 60),
            destroy: RateLimiter::new(VAULT_DESTROY_RATE_LIMIT_REQUESTS, 15 * 60),
            device_session_pair: RateLimiter::new(
                VAULT_DEVICE_SESSION_PAIR_RATE_LIMIT_REQUESTS,
                15 * 60,
            ),
            protected_action_challenge: RateLimiter::new(
                VAULT_PROTECTED_ACTION_CHALLENGE_RATE_LIMIT_REQUESTS,
                10 * 60,
            ),
            lookup: RateLimiter::new(VAULT_LOOKUP_RATE_LIMIT_REQUESTS, 60),
        }
    }
}

impl Default for RustyVaultRateLimiters {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum RustyVaultLimitedAction {
    Bootstrap,
    Rekey,
    Export,
    ImportOverwrite,
    Destroy,
    DeviceSessionPair,
    ProtectedActionChallenge,
    Lookup,
}

impl RustyVaultLimitedAction {
    fn slug(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Rekey => "rekey",
            Self::Export => "export",
            Self::ImportOverwrite => "import",
            Self::Destroy => "destroy",
            Self::DeviceSessionPair => "device-session-pair",
            Self::ProtectedActionChallenge => "protected-action-challenge",
            Self::Lookup => "lookup",
        }
    }

    fn limiter(self, limiters: &RustyVaultRateLimiters) -> &RateLimiter {
        match self {
            Self::Bootstrap => &limiters.bootstrap,
            Self::Rekey => &limiters.rekey,
            Self::Export => &limiters.export,
            Self::ImportOverwrite => &limiters.import_overwrite,
            Self::Destroy => &limiters.destroy,
            Self::DeviceSessionPair => &limiters.device_session_pair,
            Self::ProtectedActionChallenge => &limiters.protected_action_challenge,
            Self::Lookup => &limiters.lookup,
        }
    }
}

fn classify_rustyvault_limited_action(
    method: &Method,
    path: &str,
) -> Option<RustyVaultLimitedAction> {
    if method == Method::POST && (path == "/bootstrap" || path.ends_with("/vault/bootstrap")) {
        return Some(RustyVaultLimitedAction::Bootstrap);
    }
    if method == Method::POST && (path == "/rekey" || path.ends_with("/vault/rekey")) {
        return Some(RustyVaultLimitedAction::Rekey);
    }
    if method == Method::POST && (path == "/export" || path.ends_with("/vault/export")) {
        return Some(RustyVaultLimitedAction::Export);
    }
    if method == Method::POST
        && (path == "/import/bitwarden" || path.ends_with("/vault/import/bitwarden"))
    {
        return Some(RustyVaultLimitedAction::ImportOverwrite);
    }
    if method == Method::DELETE && (path == "/" || path.ends_with("/vault")) {
        return Some(RustyVaultLimitedAction::Destroy);
    }
    if method == Method::POST
        && (path == "/device-sessions/pair"
            || path.ends_with("/vault/device-sessions/pair")
            || path == "/device-sessions/pair/consume"
            || path.ends_with("/vault/device-sessions/pair/consume"))
    {
        return Some(RustyVaultLimitedAction::DeviceSessionPair);
    }
    if method == Method::POST
        && (path == "/protected-actions/challenge"
            || path.ends_with("/vault/protected-actions/challenge"))
    {
        return Some(RustyVaultLimitedAction::ProtectedActionChallenge);
    }
    if method == Method::POST && (path == "/lookup" || path.ends_with("/vault/lookup")) {
        return Some(RustyVaultLimitedAction::Lookup);
    }

    None
}

fn extract_rustyvault_rate_limit_identity(headers: &HeaderMap) -> String {
    if let Some(token) = headers
        .get("x-rustyvault-access")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
    {
        let hash = service::hash_secret(token);
        return format!("rustyvault-access:{}", &hash[..VAULT_KEY_PREFIX_LEN]);
    }

    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .filter(|value| !value.trim().is_empty())
    {
        let hash = service::hash_secret(token);
        return format!("bearer:{}", &hash[..VAULT_KEY_PREFIX_LEN]);
    }

    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("xff:{value}"))
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("xreal:{value}"))
        })
        .or_else(|| {
            headers
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("host:{value}"))
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn apply_rustyvault_api_security_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store, max-age=0, must-revalidate"),
    );
    headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, header::HeaderValue::from_static("0"));
    headers.insert(
        header::REFERRER_POLICY,
        header::HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
}

pub async fn rustyvault_availability_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.rustyvault.available {
        return next.run(request).await;
    }

    let envelope = ErrorEnvelope {
        error: rustfin_core::error::ErrorBody {
            code: "service_unavailable".to_string(),
            message: state.rustyvault.public_reason().to_string(),
            details: serde_json::json!({}),
        },
    };
    let mut response = (StatusCode::SERVICE_UNAVAILABLE, Json(envelope)).into_response();
    apply_rustyvault_api_security_headers(&mut response);
    response
}

pub async fn rustyvault_rate_limit_middleware(request: Request, next: Next) -> Response {
    let limiters = match request
        .extensions()
        .get::<RustyVaultRateLimiters>()
        .cloned()
    {
        Some(limiters) => limiters,
        None => return next.run(request).await,
    };

    let action = classify_rustyvault_limited_action(request.method(), request.uri().path());
    let Some(action) = action else {
        return next.run(request).await;
    };

    let key = format!(
        "rustyvault:{}:{}",
        action.slug(),
        extract_rustyvault_rate_limit_identity(request.headers())
    );

    match action.limiter(&limiters).check(&key).await {
        Ok(_) => next.run(request).await,
        Err(retry_after) => {
            let envelope = ErrorEnvelope {
                error: rustfin_core::error::ErrorBody {
                    code: "too_many_requests".to_string(),
                    message: "too many rustyvault requests".to_string(),
                    details: serde_json::json!({ "retry_after_seconds": retry_after }),
                },
            };
            let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(envelope)).into_response();
            response.headers_mut().insert(
                header::RETRY_AFTER,
                header::HeaderValue::from_str(&retry_after.to_string())
                    .unwrap_or_else(|_| header::HeaderValue::from_static("1")),
            );
            apply_rustyvault_api_security_headers(&mut response);
            response
        }
    }
}

pub async fn rustyvault_response_headers_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    apply_rustyvault_api_security_headers(&mut response);
    response
}
