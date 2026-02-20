use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rustfin_core::error::ErrorEnvelope;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Simple in-memory rate limiter state.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    max_requests: u64,
    window_secs: u64,
}

struct RateLimiterInner {
    buckets: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                buckets: HashMap::new(),
            })),
            max_requests,
            window_secs,
        }
    }

    /// Check if a request should be rate limited. Returns remaining count or Err with retry_after.
    pub async fn check(&self, key: &str) -> Result<u64, u64> {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);

        let entries = inner.buckets.entry(key.to_string()).or_default();

        // Remove expired entries
        entries.retain(|t| now.duration_since(*t) < window);

        if entries.len() as u64 >= self.max_requests {
            Err(self.window_secs)
        } else {
            entries.push(now);
            Ok(self.max_requests - entries.len() as u64)
        }
    }
}

/// Rate limiting middleware for setup write routes.
pub async fn rate_limit_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Only rate-limit write methods (POST, PUT, PATCH, DELETE)
    if request.method() == axum::http::Method::GET {
        return next.run(request).await;
    }

    // Extract rate limiter from Extension layer
    let rate_limiter = request
        .extensions()
        .get::<RateLimiter>()
        .cloned();

    let rate_limiter = match rate_limiter {
        Some(rl) => rl,
        None => return next.run(request).await,
    };

    // Use client IP or owner token as key
    let key = request
        .headers()
        .get("x-setup-owner-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| format!("token:{}", &t[..t.len().min(8)]))
        .unwrap_or_else(|| {
            request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|ci| format!("ip:{}", ci.0.ip()))
                .unwrap_or_else(|| "unknown".to_string())
        });

    match rate_limiter.check(&key).await {
        Ok(_remaining) => next.run(request).await,
        Err(retry_after) => {
            let envelope = ErrorEnvelope {
                error: rustfin_core::error::ErrorBody {
                    code: "too_many_requests".to_string(),
                    message: "too many requests".to_string(),
                    details: serde_json::json!({ "retry_after_seconds": retry_after }),
                },
            };
            (StatusCode::TOO_MANY_REQUESTS, Json(envelope)).into_response()
        }
    }
}
