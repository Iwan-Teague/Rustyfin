use axum::Json;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rustfin_core::error::ErrorEnvelope;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Simple in-memory rate limiter state.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
    max_requests: u64,
    window_secs: u64,
}

struct RateLimiterInner {
    buckets: HashMap<String, RateLimitBucket>,
    last_sweep: Instant,
}

struct RateLimitBucket {
    timestamps: VecDeque<Instant>,
    last_seen: Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u64, window_secs: u64) -> Self {
        let now = Instant::now();
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                buckets: HashMap::new(),
                last_sweep: now,
            })),
            max_requests,
            window_secs,
        }
    }

    /// Check if a request should be rate limited. Returns remaining count or Err with retry_after.
    pub async fn check(&self, key: &str) -> Result<u64, u64> {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        let window = Duration::from_secs(self.window_secs);

        if now.duration_since(inner.last_sweep) >= window {
            inner.buckets.retain(|_, bucket| {
                prune_bucket(bucket, now, window);
                !bucket.timestamps.is_empty() || now.duration_since(bucket.last_seen) < window
            });
            inner.last_sweep = now;
        }

        let bucket = inner
            .buckets
            .entry(key.to_string())
            .or_insert_with(|| RateLimitBucket {
                timestamps: VecDeque::new(),
                last_seen: now,
            });

        prune_bucket(bucket, now, window);
        bucket.last_seen = now;

        if bucket.timestamps.len() as u64 >= self.max_requests {
            let retry_after = bucket
                .timestamps
                .front()
                .map(|front| {
                    let remaining = front
                        .checked_add(window)
                        .and_then(|until| until.checked_duration_since(now))
                        .unwrap_or_default();
                    if remaining.subsec_nanos() > 0 {
                        remaining.as_secs() + 1
                    } else {
                        remaining.as_secs()
                    }
                })
                .unwrap_or(self.window_secs)
                .max(1);
            Err(retry_after)
        } else {
            bucket.timestamps.push_back(now);
            Ok(self.max_requests - bucket.timestamps.len() as u64)
        }
    }
}

fn prune_bucket(bucket: &mut RateLimitBucket, now: Instant, window: Duration) {
    while let Some(front) = bucket.timestamps.front().copied() {
        if now.duration_since(front) < window {
            break;
        }
        bucket.timestamps.pop_front();
    }
}

/// Rate limiting middleware for setup write routes.
pub async fn rate_limit_middleware(request: Request, next: Next) -> Response {
    // Only rate-limit write methods (POST, PUT, PATCH, DELETE)
    if request.method() == axum::http::Method::GET {
        return next.run(request).await;
    }

    // Extract rate limiter from Extension layer
    let rate_limiter = request.extensions().get::<RateLimiter>().cloned();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_prunes_expired_entries() {
        let limiter = RateLimiter::new(2, 1);

        assert_eq!(limiter.check("alpha").await.unwrap(), 1);
        assert_eq!(limiter.check("alpha").await.unwrap(), 0);
        assert!(limiter.check("alpha").await.is_err());

        tokio::time::sleep(Duration::from_millis(1100)).await;

        assert_eq!(limiter.check("alpha").await.unwrap(), 1);

        let inner = limiter.inner.lock().await;
        let bucket = inner.buckets.get("alpha").expect("bucket retained");
        assert_eq!(bucket.timestamps.len(), 1);
    }
}
