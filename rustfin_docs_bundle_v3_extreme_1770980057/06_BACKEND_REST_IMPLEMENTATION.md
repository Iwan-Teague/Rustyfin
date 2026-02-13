# Backend REST Implementation (Extreme Expansion, Axum)

## 1. Router skeleton
```rust
use axum::{Router, routing::{get, post, patch}};

pub fn router() -> Router {
    Router::new()
        .nest("/api/v1", api_router())
        .nest("/stream", stream_router())
}

fn api_router() -> Router {
    Router::new()
        .route("/auth/login", post(auth_login))
        .route("/users/me", get(users_me))
        .route("/users/me/preferences", patch(update_prefs))
        .route("/libraries", post(create_library).get(list_libraries))
        .route("/libraries/:id/scan", post(scan_library))
        .route("/libraries/:id/items", get(list_library_items))
        .route("/items/:id", get(get_item).patch(update_item))
        .route("/items/:id/refresh", post(refresh_item))
        .route("/playback/sessions", post(create_playback_session))
        .route("/playback/sessions/:sid/progress", post(update_progress))
        .route("/playback/sessions/:sid/stop", post(stop_session))
        .route("/events", get(sse_events))
}

fn stream_router() -> Router {
    Router::new()
        .route("/file/:file_id", get(stream_file_range))
        .route("/hls/:sid/master.m3u8", get(hls_master))
        .route("/hls/:sid/variant_:vid.m3u8", get(hls_variant))
        .route("/hls/:sid/seg_:n.:ext", get(hls_segment))
}
```

## 2. Range streaming correctness
RFC 7233: https://www.rfc-editor.org/rfc/rfc7233

Minimal single-range parser:
```rust
pub struct ByteRange { pub start: u64, pub end_inclusive: u64 }

pub fn parse_range_header(range: &str, size: u64) -> Result<ByteRange, ApiError> {
    if !range.trim().starts_with("bytes=") {
        return Err(ApiError::BadRequest("Only bytes ranges supported".into()));
    }
    let spec = &range.trim()["bytes=".len()..];
    let mut parts = spec.split('-');
    let start_s = parts.next().unwrap_or("");
    let end_s = parts.next().unwrap_or("");
    if start_s.is_empty() { return Err(ApiError::BadRequest("Suffix ranges not implemented".into())); }
    let start: u64 = start_s.parse().map_err(|_| ApiError::BadRequest("Bad range start".into()))?;
    let end: u64 = if end_s.is_empty() { size.saturating_sub(1) } else {
        end_s.parse().map_err(|_| ApiError::BadRequest("Bad range end".into()))?
    };
    if start >= size || end >= size || start > end { return Err(ApiError::BadRequest("Invalid range".into())); }
    Ok(ByteRange { start, end_inclusive: end })
}
```
