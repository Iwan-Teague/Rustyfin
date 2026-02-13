use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use rustfin_core::error::ApiError;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

use crate::error::AppError;
use crate::state::AppState;

/// Parse an HTTP Range header per RFC 7233.
/// Only supports single byte ranges: `bytes=start-end` or `bytes=start-`.
pub struct ByteRange {
    pub start: u64,
    pub end_inclusive: u64,
}

pub fn parse_range_header(range_str: &str, file_size: u64) -> Result<ByteRange, ApiError> {
    let range_str = range_str.trim();
    if !range_str.starts_with("bytes=") {
        return Err(ApiError::BadRequest("only bytes ranges supported".into()));
    }

    let spec = &range_str["bytes=".len()..];

    // Reject multi-range
    if spec.contains(',') {
        return Err(ApiError::BadRequest(
            "multi-range not supported".into(),
        ));
    }

    let mut parts = spec.splitn(2, '-');
    let start_s = parts.next().unwrap_or("");
    let end_s = parts.next().unwrap_or("");

    if start_s.is_empty() {
        // Suffix range: bytes=-500 means last 500 bytes
        let suffix: u64 = end_s
            .parse()
            .map_err(|_| ApiError::BadRequest("bad range suffix".into()))?;
        let start = file_size.saturating_sub(suffix);
        return Ok(ByteRange {
            start,
            end_inclusive: file_size - 1,
        });
    }

    let start: u64 = start_s
        .parse()
        .map_err(|_| ApiError::BadRequest("bad range start".into()))?;

    let end: u64 = if end_s.is_empty() {
        file_size - 1
    } else {
        end_s
            .parse()
            .map_err(|_| ApiError::BadRequest("bad range end".into()))?
    };

    // Validate
    if start >= file_size {
        return Err(ApiError::BadRequest(format!(
            "range start {start} >= file size {file_size}"
        )));
    }

    let end = end.min(file_size - 1);

    if start > end {
        return Err(ApiError::BadRequest("range start > end".into()));
    }

    Ok(ByteRange {
        start,
        end_inclusive: end,
    })
}

/// Content-type guess from file extension.
fn content_type_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",
        Some("ts") => "video/mp2t",
        Some("mpg" | "mpeg") => "video/mpeg",
        _ => "application/octet-stream",
    }
}

/// Stream a file with HTTP Range support (Direct Play).
/// GET /stream/file/{file_id}
pub async fn stream_file_range(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // Look up media file
    let media_file = rustfin_db::repo::media_files::get_media_file(&state.db, &file_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("media file not found".into()))?;

    let file_path = PathBuf::from(&media_file.path);

    // Security: verify path exists and is a regular file
    if !file_path.exists() || !file_path.is_file() {
        return Err(ApiError::NotFound("file not found on disk".into()).into());
    }

    // Security: verify the path is under a library root
    validate_path_in_library(&state, &file_path).await?;

    let file_size = media_file.size_bytes as u64;
    let content_type = content_type_for_path(&file_path);

    // Check for Range header
    if let Some(range_header) = headers.get("range").and_then(|v| v.to_str().ok()) {
        let range = match parse_range_header(range_header, file_size) {
            Ok(r) => r,
            Err(_) => {
                // 416 Range Not Satisfiable
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("Content-Range", format!("bytes */{file_size}"))
                    .body(Body::empty())
                    .unwrap());
            }
        };

        let content_length = range.end_inclusive - range.start + 1;

        // Open file and seek
        let mut file = tokio::fs::File::open(&file_path)
            .await
            .map_err(|e| ApiError::Internal(format!("file open error: {e}")))?;
        file.seek(std::io::SeekFrom::Start(range.start))
            .await
            .map_err(|e| ApiError::Internal(format!("seek error: {e}")))?;

        // Stream the requested range
        let stream = tokio_util::io::ReaderStream::new(file.take(content_length));

        Ok(Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Type", content_type)
            .header("Content-Length", content_length.to_string())
            .header(
                "Content-Range",
                format!(
                    "bytes {}-{}/{}",
                    range.start, range.end_inclusive, file_size
                ),
            )
            .header("Accept-Ranges", "bytes")
            .body(Body::from_stream(stream))
            .unwrap())
    } else {
        // Full file response (200)
        let file = tokio::fs::File::open(&file_path)
            .await
            .map_err(|e| ApiError::Internal(format!("file open error: {e}")))?;

        let stream = tokio_util::io::ReaderStream::new(file);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", content_type)
            .header("Content-Length", file_size.to_string())
            .header("Accept-Ranges", "bytes")
            .body(Body::from_stream(stream))
            .unwrap())
    }
}

/// Verify that a file path is under one of the configured library paths.
async fn validate_path_in_library(state: &AppState, file_path: &PathBuf) -> Result<(), AppError> {
    let canonical = file_path
        .canonicalize()
        .map_err(|e| ApiError::Internal(format!("canonicalize error: {e}")))?;

    let libs = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    for lib in &libs {
        let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &lib.id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        for lp in &paths {
            let lib_root = PathBuf::from(&lp.path);
            if let Ok(lib_canonical) = lib_root.canonicalize() {
                if canonical.starts_with(&lib_canonical) {
                    return Ok(());
                }
            }
        }
    }

    Err(ApiError::Forbidden("file not in any library path".into()).into())
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_basic() {
        let r = parse_range_header("bytes=0-999", 5000).unwrap();
        assert_eq!(r.start, 0);
        assert_eq!(r.end_inclusive, 999);
    }

    #[test]
    fn parse_range_open_end() {
        let r = parse_range_header("bytes=1000-", 5000).unwrap();
        assert_eq!(r.start, 1000);
        assert_eq!(r.end_inclusive, 4999);
    }

    #[test]
    fn parse_range_suffix() {
        let r = parse_range_header("bytes=-500", 5000).unwrap();
        assert_eq!(r.start, 4500);
        assert_eq!(r.end_inclusive, 4999);
    }

    #[test]
    fn parse_range_clamps_end() {
        let r = parse_range_header("bytes=0-99999", 5000).unwrap();
        assert_eq!(r.start, 0);
        assert_eq!(r.end_inclusive, 4999);
    }

    #[test]
    fn parse_range_start_beyond_size() {
        let r = parse_range_header("bytes=5000-", 5000);
        assert!(r.is_err());
    }

    #[test]
    fn parse_range_multi_rejected() {
        let r = parse_range_header("bytes=0-100, 200-300", 5000);
        assert!(r.is_err());
    }

    #[test]
    fn content_type_detection() {
        assert_eq!(
            content_type_for_path(std::path::Path::new("movie.mp4")),
            "video/mp4"
        );
        assert_eq!(
            content_type_for_path(std::path::Path::new("video.mkv")),
            "video/x-matroska"
        );
        assert_eq!(
            content_type_for_path(std::path::Path::new("clip.webm")),
            "video/webm"
        );
    }
}
