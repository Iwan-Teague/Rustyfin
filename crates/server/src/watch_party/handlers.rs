use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::permissions::RoomPolicy;

const MAX_INVITEES: usize = 100;
const ROOM_PASSWORD_MIN_LEN: usize = 4;
const ROOM_PASSWORD_MAX_LEN: usize = 128;
const YOUTUBE_SEARCH_MIN_QUERY_LEN: usize = 2;
const YOUTUBE_SEARCH_MAX_QUERY_LEN: usize = 100;
const YOUTUBE_SEARCH_DEFAULT_LIMIT: usize = 10;
const YOUTUBE_SEARCH_MAX_LIMIT: usize = 20;
const YOUTUBE_SEARCH_TIMEOUT_SECONDS: u64 = 8;
const YOUTUBE_LOOKUP_MAX_IDS: usize = 25;
const YOUTUBE_LOOKUP_TIMEOUT_SECONDS: u64 = 5;
const INVITE_RESEND_COOLDOWN_SECONDS: i64 = 5;
const YOUTUBE_SEARCH_URL: &str = "https://www.youtube.com/results";
const YOUTUBE_SEARCH_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

static CREATE_ROOM_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static JOIN_ROOM_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static YOUTUBE_SEARCH_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Debug, Serialize)]
pub struct MinimalUser {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct EligibleLibrariesRequest {
    #[serde(default)]
    pub user_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EligibleLibrariesResponse {
    pub library_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomPolicy {
    #[serde(default = "default_true")]
    pub allow_non_host_play_pause: bool,
    #[serde(default = "default_false")]
    pub allow_non_host_seek: bool,
    #[serde(default = "default_join_role")]
    pub default_join_role: String,
    #[serde(default)]
    pub invite_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomInvite {
    pub user_id: String,
    #[serde(default = "default_join_role")]
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    /// For video rooms: the item to watch.
    pub item_id: Option<String>,
    /// For audio rooms: the music library to use.
    pub audio_library_id: Option<String>,
    /// Explicit room mode override. Use "youtube" for YouTube watch parties.
    pub room_mode: Option<String>,
    #[serde(default)]
    pub invites: Vec<CreateRoomInvite>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub policy: Option<CreateRoomPolicy>,
}

#[derive(Debug, Serialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub join_path: String,
}

#[derive(Debug, Serialize)]
pub struct RoomMemberResponse {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct RoomResponse {
    pub room_id: String,
    pub item_id: String,
    pub host_user_id: String,
    pub status: String,
    pub created_ts: i64,
    pub ended_ts: Option<i64>,
    pub password_required: bool,
    pub policy: serde_json::Value,
    pub members: Vec<RoomMemberResponse>,
    pub room_mode: String,
    pub audio_library_id: Option<String>,
    pub youtube_video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JoinRoomRequest {
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JoinRoomResponse {
    pub ok: bool,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct ReconfigureRoomRequest {
    pub room_mode: String,
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub audio_library_id: Option<String>,
    #[serde(default)]
    pub youtube_video_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReconfigureRoomResponse {
    pub ok: bool,
    pub room_mode: String,
}

#[derive(Debug, Serialize)]
pub struct InviteResponse {
    pub room_id: String,
    pub item_id: String,
    pub item_title: String,
    pub host_user_id: String,
    pub host_username: String,
    pub created_ts: i64,
    pub password_required: bool,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct AudioTracksQuery {
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AudioTrackResponse {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_art_url: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct YouTubeSearchQuery {
    pub q: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct YouTubeSearchResult {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: String,
    pub view_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct YouTubeLookupRequest {
    pub video_ids: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_join_role() -> String {
    "viewer".to_string()
}

fn shuffle_track_ids(track_ids: &mut [String]) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let seed = chrono::Utc::now().timestamp_millis() as u64;
    for i in (1..track_ids.len()).rev() {
        let mut hasher = DefaultHasher::new();
        (seed ^ i as u64).hash(&mut hasher);
        let j = (hasher.finish() as usize) % (i + 1);
        track_ids.swap(i, j);
    }
}

fn create_room_rate_limiter() -> &'static RateLimiter {
    CREATE_ROOM_RATE_LIMITER.get_or_init(|| RateLimiter::new(20, 60))
}

fn join_room_rate_limiter() -> &'static RateLimiter {
    JOIN_ROOM_RATE_LIMITER.get_or_init(|| RateLimiter::new(25, 60))
}

fn youtube_search_rate_limiter() -> &'static RateLimiter {
    YOUTUBE_SEARCH_RATE_LIMITER.get_or_init(|| RateLimiter::new(60, 60))
}

async fn check_rate_limit(limiter: &RateLimiter, key: &str) -> Result<(), AppError> {
    match limiter.check(key).await {
        Ok(_) => Ok(()),
        Err(retry_after) => Err(ApiError::TooManyRequests {
            retry_after_seconds: retry_after,
        }
        .into()),
    }
}

fn is_valid_youtube_video_id(value: &str) -> bool {
    value.len() == 11
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn extract_youtube_video_id_from_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if is_valid_youtube_video_id(trimmed) {
        return Some(trimmed.to_string());
    }

    let url = reqwest::Url::parse(trimmed).ok()?;
    let host = url
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(&host);

    match host {
        "youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if let Some(video_id) = url
                .query_pairs()
                .find_map(|(k, v)| if k == "v" { Some(v.into_owned()) } else { None })
            {
                if is_valid_youtube_video_id(&video_id) {
                    return Some(video_id);
                }
            }

            let segments: Vec<&str> = url.path_segments()?.collect();
            if segments.len() >= 2 && matches!(segments[0], "embed" | "shorts" | "live") {
                let candidate = segments[1].trim();
                if is_valid_youtube_video_id(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        "youtube-nocookie.com" => {
            let segments: Vec<&str> = url.path_segments()?.collect();
            if segments.len() >= 2 && segments[0] == "embed" {
                let candidate = segments[1].trim();
                if is_valid_youtube_video_id(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        "youtu.be" => {
            let candidate = url.path().trim_matches('/').trim();
            if is_valid_youtube_video_id(candidate) {
                return Some(candidate.to_string());
            }
        }
        _ => {}
    }

    None
}

fn normalize_youtube_search_query(raw_query: &str) -> Result<String, AppError> {
    let query = raw_query.trim().to_string();
    if query.len() < YOUTUBE_SEARCH_MIN_QUERY_LEN {
        return Err(ApiError::BadRequest(format!(
            "search query must be at least {YOUTUBE_SEARCH_MIN_QUERY_LEN} characters"
        ))
        .into());
    }
    if query.len() > YOUTUBE_SEARCH_MAX_QUERY_LEN {
        return Err(ApiError::BadRequest(format!(
            "search query must be <= {YOUTUBE_SEARCH_MAX_QUERY_LEN} characters"
        ))
        .into());
    }
    Ok(query)
}

fn normalize_youtube_search_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(YOUTUBE_SEARCH_DEFAULT_LIMIT)
        .clamp(1, YOUTUBE_SEARCH_MAX_LIMIT)
}

fn extract_json_object_after_marker(source: &str, marker: &str) -> Option<String> {
    let marker_idx = source.find(marker)?;
    let bytes = source.as_bytes();
    let mut start = marker_idx + marker.len();

    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }

    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in source[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            continue;
        }

        if ch == '{' {
            depth += 1;
            continue;
        }

        if ch == '}' {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                let end = start + offset + ch.len_utf8();
                return Some(source[start..end].to_string());
            }
        }
    }

    None
}

fn renderer_text(value: &serde_json::Value) -> Option<String> {
    if let Some(simple) = value.get("simpleText").and_then(serde_json::Value::as_str) {
        let simple = simple.trim();
        if !simple.is_empty() {
            return Some(simple.to_string());
        }
    }

    let runs = value.get("runs").and_then(serde_json::Value::as_array)?;
    let mut merged = String::new();
    for run in runs {
        if let Some(text) = run.get("text").and_then(serde_json::Value::as_str) {
            merged.push_str(text);
        }
    }
    let merged = merged.trim();
    if merged.is_empty() {
        None
    } else {
        Some(merged.to_string())
    }
}

fn parse_youtube_view_count_text(raw: &str) -> Option<u64> {
    let normalized = raw.trim().to_lowercase().replace('\u{a0}', " ");
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("no views") {
        return Some(0);
    }

    let parts: Vec<&str> = normalized.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let mut token = parts[0].trim().trim_end_matches('+').replace(',', "");
    if parts.len() > 1 {
        if matches!(parts[1], "k" | "m" | "b")
            && !matches!(token.chars().last(), Some('k' | 'm' | 'b'))
        {
            token.push_str(parts[1]);
        }
    }

    if token.is_empty() {
        return None;
    }

    if let Some(suffix) = token.chars().last() {
        if matches!(suffix, 'k' | 'm' | 'b') && token.len() > 1 {
            let numeric = &token[..token.len() - 1];
            let value = numeric.parse::<f64>().ok()?;
            let multiplier = match suffix {
                'k' => 1_000_f64,
                'm' => 1_000_000_f64,
                'b' => 1_000_000_000_f64,
                _ => 1_f64,
            };
            return Some((value * multiplier).round() as u64);
        }
    }

    let digits_only: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits_only.is_empty() {
        return None;
    }
    digits_only.parse::<u64>().ok()
}

fn parse_youtube_watch_view_count_html(html: &str) -> Option<u64> {
    const ITEMPROP_MARKER: &str = "itemprop=\"interactionCount\" content=\"";
    if let Some(start_idx) = html.find(ITEMPROP_MARKER) {
        let start = start_idx + ITEMPROP_MARKER.len();
        if let Some(end_rel) = html[start..].find('"') {
            let count_raw = &html[start..start + end_rel];
            if let Ok(parsed) = count_raw.trim().parse::<u64>() {
                return Some(parsed);
            }
        }
    }

    const JSON_MARKER: &str = "\"viewCount\":\"";
    if let Some(start_idx) = html.find(JSON_MARKER) {
        let start = start_idx + JSON_MARKER.len();
        if let Some(end_rel) = html[start..].find('"') {
            let count_raw = &html[start..start + end_rel];
            if let Ok(parsed) = count_raw.trim().parse::<u64>() {
                return Some(parsed);
            }
        }
    }

    None
}

fn extract_video_renderer_view_count(video_renderer: &serde_json::Value) -> Option<u64> {
    video_renderer
        .get("viewCountText")
        .and_then(renderer_text)
        .or_else(|| {
            video_renderer
                .get("shortViewCountText")
                .and_then(renderer_text)
        })
        .and_then(|txt| parse_youtube_view_count_text(&txt))
}

fn parse_youtube_initial_data_results(
    initial_data: &serde_json::Value,
    limit: usize,
) -> Vec<YouTubeSearchResult> {
    let mut results = Vec::with_capacity(limit.min(YOUTUBE_SEARCH_MAX_LIMIT));

    let Some(sections) = initial_data
        .pointer(
            "/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents",
        )
        .and_then(serde_json::Value::as_array)
    else {
        return results;
    };

    'outer: for section in sections {
        let Some(items) = section
            .pointer("/itemSectionRenderer/contents")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };

        for item in items {
            let Some(video_renderer) = item.get("videoRenderer") else {
                continue;
            };

            let Some(video_id) = video_renderer
                .get("videoId")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
            else {
                continue;
            };

            if !is_valid_youtube_video_id(video_id) {
                continue;
            }

            let title = video_renderer
                .get("title")
                .and_then(renderer_text)
                .unwrap_or_else(|| "Untitled video".to_string());

            let channel = video_renderer
                .get("ownerText")
                .and_then(renderer_text)
                .or_else(|| {
                    video_renderer
                        .get("shortBylineText")
                        .and_then(renderer_text)
                })
                .unwrap_or_else(|| "Unknown channel".to_string());

            let thumbnail_url = video_renderer
                .pointer("/thumbnail/thumbnails")
                .and_then(serde_json::Value::as_array)
                .and_then(|thumbs| thumbs.last())
                .and_then(|thumb| thumb.get("url"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));
            let view_count = extract_video_renderer_view_count(video_renderer);

            results.push(YouTubeSearchResult {
                video_id: video_id.to_string(),
                title,
                channel,
                thumbnail_url,
                view_count,
            });

            if results.len() >= limit {
                break 'outer;
            }
        }
    }

    results
}

fn parse_youtube_search_results(search_html: &str, limit: usize) -> Vec<YouTubeSearchResult> {
    const MARKERS: [&str; 3] = [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "ytInitialData = ",
    ];

    for marker in MARKERS {
        let Some(json_blob) = extract_json_object_after_marker(search_html, marker) else {
            continue;
        };

        let Ok(initial_data) = serde_json::from_str::<serde_json::Value>(&json_blob) else {
            continue;
        };

        let results = parse_youtube_initial_data_results(&initial_data, limit);
        if !results.is_empty() {
            return results;
        }
    }

    Vec::new()
}

#[derive(Debug, Deserialize)]
struct YouTubeOEmbedResponse {
    title: String,
    author_name: String,
    thumbnail_url: Option<String>,
}

async fn fetch_youtube_video_metadata(
    client: &reqwest::Client,
    video_id: &str,
) -> Option<YouTubeSearchResult> {
    let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
    let url = reqwest::Url::parse_with_params(
        "https://www.youtube.com/oembed",
        &[("url", watch_url.as_str()), ("format", "json")],
    )
    .ok()?;

    let response = client
        .get(url)
        .timeout(Duration::from_secs(YOUTUBE_LOOKUP_TIMEOUT_SECONDS))
        .header(reqwest::header::USER_AGENT, YOUTUBE_SEARCH_USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let payload = response.json::<YouTubeOEmbedResponse>().await.ok()?;
    let view_count = fetch_youtube_view_count(client, video_id).await;
    Some(YouTubeSearchResult {
        video_id: video_id.to_string(),
        title: payload.title.trim().to_string(),
        channel: payload.author_name.trim().to_string(),
        thumbnail_url: payload
            .thumbnail_url
            .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg")),
        view_count,
    })
}

async fn fetch_youtube_view_count(client: &reqwest::Client, video_id: &str) -> Option<u64> {
    let url = reqwest::Url::parse_with_params(
        "https://www.youtube.com/watch",
        &[("v", video_id), ("hl", "en")],
    )
    .ok()?;

    let response = client
        .get(url)
        .timeout(Duration::from_secs(YOUTUBE_LOOKUP_TIMEOUT_SECONDS))
        .header(reqwest::header::USER_AGENT, YOUTUBE_SEARCH_USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let html = response.text().await.ok()?;
    parse_youtube_watch_view_count_html(&html)
}

pub(crate) async fn perform_youtube_search(
    raw_query: &str,
    limit: Option<usize>,
) -> Result<(String, Vec<YouTubeSearchResult>), AppError> {
    let query = normalize_youtube_search_query(raw_query)?;
    let limit = normalize_youtube_search_limit(limit);
    let client = reqwest::Client::new();

    let mut results = Vec::new();
    if let Some(video_id) = extract_youtube_video_id_from_input(&query) {
        if let Some(metadata) = fetch_youtube_video_metadata(&client, &video_id).await {
            results.push(metadata);
        }
    }

    if results.is_empty() {
        let url = reqwest::Url::parse_with_params(
            YOUTUBE_SEARCH_URL,
            &[
                ("search_query", query.as_str()),
                ("hl", "en"),
                ("persist_hl", "1"),
            ],
        )
        .map_err(|e| ApiError::Internal(format!("failed to build youtube search url: {e}")))?;

        let response = client
            .get(url)
            .timeout(Duration::from_secs(YOUTUBE_SEARCH_TIMEOUT_SECONDS))
            .header(reqwest::header::USER_AGENT, YOUTUBE_SEARCH_USER_AGENT)
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await
            .map_err(|e| ApiError::Internal(format!("youtube search request failed: {e}")))?;

        if response.url().as_str().contains("consent.youtube.com") {
            return Err(ApiError::BadRequest(
                "youtube search was blocked by a consent/interstitial page on this network".into(),
            )
            .into());
        }

        if !response.status().is_success() {
            return Err(ApiError::BadRequest(format!(
                "youtube search failed with status {}",
                response.status()
            ))
            .into());
        }

        let search_html = response
            .text()
            .await
            .map_err(|e| ApiError::Internal(format!("youtube search response read failed: {e}")))?;

        results = parse_youtube_search_results(&search_html, limit);
    }

    if results.is_empty() {
        return Err(ApiError::BadRequest(
            "youtube search did not return parseable video results".into(),
        )
        .into());
    }

    Ok((query, results))
}

fn normalize_member_role(role: &str) -> Result<String, AppError> {
    let normalized = role.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "viewer" | "controller" | "host" => Ok(normalized),
        _ => Err(ApiError::BadRequest(
            "invalid member role; expected one of: host, controller, viewer".into(),
        )
        .into()),
    }
}

fn normalize_default_join_role(role: &str) -> Result<String, AppError> {
    let normalized = role.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "viewer" | "controller" => Ok(normalized),
        _ => Err(ApiError::BadRequest(
            "invalid default join role; expected viewer or controller".into(),
        )
        .into()),
    }
}

fn parse_policy(input: Option<CreateRoomPolicy>) -> Result<RoomPolicy, AppError> {
    match input {
        Some(policy) => Ok(RoomPolicy {
            allow_non_host_play_pause: policy.allow_non_host_play_pause,
            allow_non_host_seek: policy.allow_non_host_seek,
            default_join_role: normalize_default_join_role(&policy.default_join_role)?,
            invite_only: policy.invite_only,
        }),
        None => Ok(RoomPolicy::default()),
    }
}

fn normalize_password(password: Option<String>) -> Result<Option<String>, AppError> {
    match password {
        Some(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.len() < ROOM_PASSWORD_MIN_LEN {
                return Err(ApiError::BadRequest(format!(
                    "room password must be at least {ROOM_PASSWORD_MIN_LEN} characters"
                ))
                .into());
            }
            if trimmed.len() > ROOM_PASSWORD_MAX_LEN {
                return Err(ApiError::BadRequest(format!(
                    "room password must be <= {ROOM_PASSWORD_MAX_LEN} characters"
                ))
                .into());
            }
            Ok(Some(trimmed))
        }
        None => Ok(None),
    }
}

fn hash_room_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(format!("password hash error: {e}")))?;
    Ok(hash.to_string())
}

fn verify_room_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| ApiError::Internal(format!("password hash parse error: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

async fn ensure_library_access_for_user(
    state: &AppState,
    user_id: &str,
    user_role: &str,
    library_id: &str,
) -> Result<(), AppError> {
    if user_role == "admin" {
        return Ok(());
    }

    let allowed = rustfin_db::repo::users::is_library_allowed(&state.db, user_id, library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !allowed {
        return Err(ApiError::Forbidden("library access denied".into()).into());
    }
    Ok(())
}

fn room_member_username<'a>(
    usernames: &'a std::collections::HashMap<String, String>,
    user_id: &str,
) -> &'a str {
    usernames
        .get(user_id)
        .map(String::as_str)
        .unwrap_or("unknown")
}

fn is_password_required(hash: Option<&str>) -> bool {
    hash.is_some_and(|value| !value.trim().is_empty())
}

#[derive(Debug, Serialize)]
pub struct PublicRoomEntry {
    pub room_id: String,
    pub host_username: String,
    pub title: String,
    pub room_mode: String,
    pub password_required: bool,
    pub member_count: i64,
    pub created_ts: i64,
}

pub async fn list_public_rooms(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<PublicRoomEntry>>, AppError> {
    let rooms = rustfin_db::repo::watch_party::list_public_rooms(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let entries = rooms
        .into_iter()
        .map(|r| {
            let title = if r.room_mode == "audio" {
                if r.audio_library_name.is_empty() {
                    "Music Party".to_string()
                } else {
                    format!("Music: {}", r.audio_library_name)
                }
            } else if r.room_mode == "youtube" {
                "YouTube Party".to_string()
            } else {
                r.item_title
            };
            PublicRoomEntry {
                room_id: r.id,
                host_username: r.host_username,
                title,
                room_mode: r.room_mode,
                password_required: r.password_required,
                member_count: r.member_count,
                created_ts: r.created_ts,
            }
        })
        .collect();

    Ok(Json(entries))
}

pub async fn list_inviteable_users(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<MinimalUser>>, AppError> {
    let users = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")));

    let mut users = users?;
    users.sort_by(|a, b| a.username.cmp(&b.username));

    Ok(Json(
        users
            .into_iter()
            .map(|u| MinimalUser {
                id: u.id,
                username: u.username,
            })
            .collect(),
    ))
}

pub async fn eligible_libraries(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<EligibleLibrariesRequest>,
) -> Result<Json<EligibleLibrariesResponse>, AppError> {
    if body.user_ids.len() > MAX_INVITEES {
        return Err(
            ApiError::BadRequest(format!("too many user IDs; maximum is {MAX_INVITEES}")).into(),
        );
    }

    let mut requested_user_ids = Vec::with_capacity(body.user_ids.len() + 1);
    requested_user_ids.push(auth.user_id.clone());

    let mut seen = HashSet::new();
    seen.insert(auth.user_id.clone());

    for user_id in body.user_ids {
        let trimmed = user_id.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            requested_user_ids.push(trimmed.to_string());
        }
    }

    let all_library_ids: Vec<String> = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|lib| lib.id)
        .collect();

    let mut intersection: Option<HashSet<String>> = None;

    for user_id in requested_user_ids {
        let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

        let current: HashSet<String> = if user.role == "admin" {
            all_library_ids.iter().cloned().collect()
        } else {
            rustfin_db::repo::users::get_library_access(&state.db, &user.id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .into_iter()
                .collect()
        };

        intersection = match intersection {
            Some(existing) => Some(existing.intersection(&current).cloned().collect()),
            None => Some(current),
        };
    }

    let mut library_ids: Vec<String> = intersection.unwrap_or_default().into_iter().collect();
    library_ids.sort();
    Ok(Json(EligibleLibrariesResponse { library_ids }))
}

pub async fn create_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<CreateRoomResponse>), AppError> {
    check_rate_limit(
        create_room_rate_limiter(),
        &format!("create-room:{}", auth.user_id),
    )
    .await?;

    let policy = parse_policy(body.policy)?;
    let password = normalize_password(body.password)?;
    let password_hash = match password {
        Some(value) => Some(hash_room_password(&value)?),
        None => None,
    };

    let mut deduped_invites = Vec::with_capacity(body.invites.len());
    let mut seen = HashSet::new();

    for invite in body.invites {
        let user_id = invite.user_id.trim();
        if user_id.is_empty() || user_id == auth.user_id {
            continue;
        }
        if !seen.insert(user_id.to_string()) {
            continue;
        }

        let role = normalize_member_role(&invite.role)?;
        if role == "host" {
            return Err(ApiError::BadRequest("invite role cannot be host".into()).into());
        }

        deduped_invites.push((user_id.to_string(), role));
    }

    if deduped_invites.len() > MAX_INVITEES {
        return Err(
            ApiError::BadRequest(format!("too many invitees; maximum is {MAX_INVITEES}")).into(),
        );
    }

    // Determine room mode and item_id
    let (room_mode, item_id, audio_library_id, track_ids): (
        String,
        Option<String>,
        Option<String>,
        Option<Vec<String>>,
    ) = if body.room_mode.as_deref() == Some("youtube") {
        // YouTube room — no media item or library required
        ("youtube".to_string(), None, None, None)
    } else if let Some(audio_lib_id) = body
        .audio_library_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        // Audio room
        let library = rustfin_db::repo::libraries::get_library(&state.db, audio_lib_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("audio library not found".into()))?;

        if library.kind != "music" {
            return Err(ApiError::BadRequest(
                "audio_library_id must refer to a music library".into(),
            )
            .into());
        }

        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, audio_lib_id).await?;

        // Get all tracks from the library
        let tracks =
            rustfin_db::repo::watch_party::get_library_tracks(&state.db, audio_lib_id, None)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        if tracks.is_empty() {
            return Err(ApiError::BadRequest(
                "the music library has no tracks; scan it first".into(),
            )
            .into());
        }

        // Shuffle tracks
        let mut track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        shuffle_track_ids(&mut track_ids);

        let first_track_id = track_ids[0].clone();
        (
            "audio".to_string(),
            Some(first_track_id),
            Some(audio_lib_id.to_string()),
            Some(track_ids),
        )
    } else {
        // Video room
        let item_id = body
            .item_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ApiError::BadRequest("item_id is required for video rooms".into()))?;

        let item = rustfin_db::repo::items::get_item(&state.db, item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("item not found".into()))?;

        if item.kind != "movie" && item.kind != "episode" {
            return Err(ApiError::BadRequest(
                "watch parties currently support movie and episode items only".into(),
            )
            .into());
        }

        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;

        ("video".to_string(), Some(item.id.clone()), None, None)
    };

    let now = chrono::Utc::now().timestamp();

    let mut members = Vec::with_capacity(deduped_invites.len() + 1);
    members.push(rustfin_db::repo::watch_party::NewWatchPartyMember {
        user_id: auth.user_id.clone(),
        role: "host".to_string(),
        status: "joined".to_string(),
        invited_by: Some(auth.user_id.clone()),
        invited_ts: Some(now),
        joined_ts: Some(now),
    });

    // Validate invitees for video rooms (check library access per invitee)
    for (user_id, role) in &deduped_invites {
        let user = rustfin_db::repo::users::find_by_id(&state.db, user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("invited user not found".into()))?;

        if room_mode == "video" {
            // For video rooms, we already have the item's library_id baked in item above.
            // Re-fetch item to get library_id.
            let item = rustfin_db::repo::items::get_item(
                &state.db,
                item_id.as_deref().unwrap_or_default(),
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
            ensure_library_access_for_user(&state, &user.id, &user.role, &item.library_id).await?;
        } else if let Some(ref lib_id) = audio_library_id {
            ensure_library_access_for_user(&state, &user.id, &user.role, lib_id).await?;
        }

        members.push(rustfin_db::repo::watch_party::NewWatchPartyMember {
            user_id: user_id.clone(),
            role: role.clone(),
            status: "invited".to_string(),
            invited_by: Some(auth.user_id.clone()),
            invited_ts: Some(now),
            joined_ts: None,
        });
    }

    let policy_json = serde_json::to_string(&policy)
        .map_err(|e| ApiError::Internal(format!("policy serialization error: {e}")))?;

    let created = rustfin_db::repo::watch_party::create_room_with_members(
        &state.db,
        &auth.user_id,
        item_id.as_deref(),
        &policy_json,
        password_hash.as_deref(),
        &members,
        Some(&room_mode),
        audio_library_id.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // For audio rooms, persist the track queue
    if let Some(ref track_ids) = track_ids {
        let track_ids_json = serde_json::to_string(track_ids)
            .map_err(|e| ApiError::Internal(format!("queue serialization error: {e}")))?;
        rustfin_db::repo::watch_party::upsert_audio_queue(
            &state.db,
            &created.id,
            &track_ids_json,
            0,
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_id: created.id.clone(),
            join_path: format!("/rooms/{}", created.id),
        }),
    ))
}

pub async fn reconfigure_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<ReconfigureRoomRequest>,
) -> Result<Json<ReconfigureRoomResponse>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.host_user_id != auth.user_id {
        return Err(ApiError::Forbidden("only host can reconfigure the room".into()).into());
    }

    if room.status != "lobby" {
        return Err(ApiError::Conflict("room is not active".into()).into());
    }

    let members = rustfin_db::repo::watch_party::list_members(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut active_users = Vec::new();
    for member in members
        .into_iter()
        .filter(|m| m.status != "left" && m.status != "declined")
    {
        if let Some(user) = rustfin_db::repo::users::find_by_id(&state.db, &member.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        {
            active_users.push(user);
        }
    }

    let target_mode = body.room_mode.trim().to_ascii_lowercase();
    let (target_item_id, target_audio_library_id, target_youtube_video_id, target_audio_queue): (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<Vec<String>>,
    ) = match target_mode.as_str() {
        "video" => {
            let item_id = body
                .item_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ApiError::BadRequest("item_id is required when switching to video mode".into())
                })?;

            let item = rustfin_db::repo::items::get_item(&state.db, item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("item not found".into()))?;

            if item.kind != "movie" && item.kind != "episode" {
                return Err(ApiError::BadRequest(
                    "watch parties currently support movie and episode items only".into(),
                )
                .into());
            }

            for user in &active_users {
                if ensure_library_access_for_user(&state, &user.id, &user.role, &item.library_id)
                    .await
                    .is_err()
                {
                    return Err(ApiError::BadRequest(format!(
                        "user '{}' does not have access to the selected video library",
                        user.username
                    ))
                    .into());
                }
            }

            (Some(item.id), None, None, None)
        }
        "audio" => {
            let library_id = body
                .audio_library_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ApiError::BadRequest(
                        "audio_library_id is required when switching to audio mode".into(),
                    )
                })?;

            let library = rustfin_db::repo::libraries::get_library(&state.db, library_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("audio library not found".into()))?;

            if library.kind != "music" {
                return Err(ApiError::BadRequest(
                    "audio_library_id must refer to a music library".into(),
                )
                .into());
            }

            for user in &active_users {
                if ensure_library_access_for_user(&state, &user.id, &user.role, library_id)
                    .await
                    .is_err()
                {
                    return Err(ApiError::BadRequest(format!(
                        "user '{}' does not have access to the selected music library",
                        user.username
                    ))
                    .into());
                }
            }

            let tracks =
                rustfin_db::repo::watch_party::get_library_tracks(&state.db, library_id, None)
                    .await
                    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

            if tracks.is_empty() {
                return Err(ApiError::BadRequest(
                    "the music library has no tracks; scan it first".into(),
                )
                .into());
            }

            let mut track_ids: Vec<String> = tracks.into_iter().map(|t| t.id).collect();
            shuffle_track_ids(&mut track_ids);
            let first_track = track_ids.first().cloned().ok_or_else(|| {
                ApiError::BadRequest("the music library has no tracks; scan it first".into())
            })?;

            (
                Some(first_track),
                Some(library_id.to_string()),
                None,
                Some(track_ids),
            )
        }
        "youtube" => {
            let youtube_video_id = body
                .youtube_video_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|raw| {
                    extract_youtube_video_id_from_input(raw).ok_or_else(|| {
                        ApiError::BadRequest(
                            "youtube_video_id must be a valid YouTube URL or 11-character ID"
                                .into(),
                        )
                    })
                })
                .transpose()?;

            (None, None, youtube_video_id, None)
        }
        _ => {
            return Err(ApiError::BadRequest(
                "room_mode must be one of: video, audio, youtube".into(),
            )
            .into());
        }
    };

    rustfin_db::repo::watch_party::reconfigure_room_mode(
        &state.db,
        &room_id,
        &target_mode,
        target_item_id.as_deref(),
        target_audio_library_id.as_deref(),
        target_youtube_video_id.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if let Some(track_ids) = target_audio_queue {
        let track_ids_json = serde_json::to_string(&track_ids)
            .map_err(|e| ApiError::Internal(format!("queue serialization error: {e}")))?;
        rustfin_db::repo::watch_party::upsert_audio_queue(&state.db, &room_id, &track_ids_json, 0)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    } else {
        rustfin_db::repo::watch_party::clear_audio_queue(&state.db, &room_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    if let Some(runtime) = state.watch_party.get_runtime(&room_id).await {
        let _ = runtime
            .tx
            .send(super::protocol::ServerMessage::RoomReconfigured {
                room_mode: target_mode.clone(),
                item_id: target_item_id.clone().unwrap_or_default(),
                audio_library_id: target_audio_library_id.clone(),
                youtube_video_id: target_youtube_video_id.clone(),
            });
    }
    state.watch_party.remove_runtime(&room_id).await;

    Ok(Json(ReconfigureRoomResponse {
        ok: true,
        room_mode: target_mode,
    }))
}

pub async fn get_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<RoomResponse>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    // For video rooms only: verify item and library access
    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let policy: RoomPolicy = serde_json::from_str(&room.policy_json)
        .map_err(|e| ApiError::Internal(format!("invalid room policy JSON: {e}")))?;

    let me = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if policy.invite_only && me.is_none() {
        return Err(ApiError::Forbidden(
            "room is invite-only; this account must be invited by the host".into(),
        )
        .into());
    }

    let members = rustfin_db::repo::watch_party::list_members(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let usernames: std::collections::HashMap<String, String> =
        rustfin_db::repo::users::list_users(&state.db)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect();

    let members = members
        .into_iter()
        .filter(|member| member.status != "left" && member.status != "declined")
        .map(|member| RoomMemberResponse {
            username: room_member_username(&usernames, &member.user_id).to_string(),
            user_id: member.user_id,
            role: member.role,
            status: member.status,
        })
        .collect();

    // For YouTube rooms, reflect the live runtime state of the video ID if available
    let youtube_video_id = if room.room_mode == "youtube" {
        if let Some(runtime) = state.watch_party.get_runtime(&room_id).await {
            runtime
                .get_youtube_video_id()
                .await
                .or(room.youtube_video_id)
        } else {
            room.youtube_video_id
        }
    } else {
        None
    };

    let ended_ts = if room.status == "ended" {
        Some(room.updated_ts)
    } else {
        None
    };

    Ok(Json(RoomResponse {
        room_id: room.id,
        item_id: room.item_id,
        host_user_id: room.host_user_id,
        status: room.status,
        created_ts: room.created_ts,
        ended_ts,
        password_required: is_password_required(room.join_password_hash.as_deref()),
        policy: serde_json::to_value(policy)
            .map_err(|e| ApiError::Internal(format!("policy serialization error: {e}")))?,
        members,
        room_mode: room.room_mode,
        audio_library_id: room.audio_library_id,
        youtube_video_id,
    }))
}

pub async fn join_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, AppError> {
    check_rate_limit(
        join_room_rate_limiter(),
        &format!("join-room:{}:{}", room_id, auth.user_id),
    )
    .await?;

    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.status != "lobby" {
        return Err(ApiError::Conflict("room is not accepting new joins".into()).into());
    }

    let policy: RoomPolicy = serde_json::from_str(&room.policy_json)
        .map_err(|e| ApiError::Internal(format!("invalid room policy JSON: {e}")))?;

    // For video rooms only: verify item and library access
    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let existing_member =
        rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if policy.invite_only && existing_member.is_none() {
        return Err(ApiError::Forbidden(
            "room is invite-only; password is not enough without an invite".into(),
        )
        .into());
    }

    if auth.user_id != room.host_user_id {
        if let Some(hash) = room.join_password_hash.as_deref() {
            let required = is_password_required(Some(hash));
            if required {
                let provided = body.password.unwrap_or_default();
                let valid = verify_room_password(provided.trim(), hash)?;
                if !valid {
                    return Err(ApiError::Forbidden("invalid room password".into()).into());
                }
            }
        }
    }

    let role = existing_member
        .as_ref()
        .map(|member| member.role.clone())
        .unwrap_or_else(|| policy.default_join_role.clone());

    let now = chrono::Utc::now().timestamp();
    rustfin_db::repo::watch_party::upsert_member(
        &state.db,
        &room_id,
        &rustfin_db::repo::watch_party::NewWatchPartyMember {
            user_id: auth.user_id.clone(),
            role: role.clone(),
            status: "joined".to_string(),
            invited_by: Some(room.host_user_id.clone()),
            invited_ts: existing_member.as_ref().and_then(|m| m.invited_ts),
            joined_ts: Some(now),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let _ = rustfin_db::repo::watch_party::touch_room_updated(&state.db, &room_id).await;

    Ok(Json(JoinRoomResponse { ok: true, role }))
}

pub async fn leave_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let updated = rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &room_id,
        &auth.user_id,
        "left",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !updated {
        return Err(ApiError::NotFound("room membership not found".into()).into());
    }
    let _ = rustfin_db::repo::watch_party::touch_room_updated(&state.db, &room_id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn end_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.host_user_id != auth.user_id {
        return Err(ApiError::Forbidden("only host can end the room".into()).into());
    }

    rustfin_db::repo::watch_party::set_room_status(&state.db, &room_id, "ended")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if let Some(runtime) = state.watch_party.get_runtime(&room_id).await {
        let _ = runtime.tx.send(super::protocol::ServerMessage::RoomEnded);
    }

    state.watch_party.remove_runtime(&room_id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn list_invites(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<InviteResponse>>, AppError> {
    let invites = rustfin_db::repo::watch_party::list_invites_for_user(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(
        invites
            .into_iter()
            .map(|row| InviteResponse {
                room_id: row.room_id,
                item_id: row.item_id,
                item_title: row.item_title,
                host_user_id: row.host_user_id,
                host_username: row.host_username,
                created_ts: row.created_ts,
                password_required: row.password_required,
                role: row.role,
                status: row.status,
            })
            .collect(),
    ))
}

pub async fn decline_invite(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let member = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("invite not found".into()))?;

    if member.status != "invited" {
        return Err(ApiError::BadRequest("invite is not pending".into()).into());
    }

    rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &room_id,
        &auth.user_id,
        "declined",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct InviteMembersRequest {
    invites: Vec<InviteInput>,
}

#[derive(Deserialize)]
pub struct InviteInput {
    user_id: String,
    role: String,
}

#[derive(Serialize)]
pub struct InviteMembersResponse {
    ok: bool,
    invited: u32,
    cooldown_blocked_users: Vec<String>,
}

pub async fn invite_members(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<InviteMembersRequest>,
) -> Result<Json<InviteMembersResponse>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.status != "lobby" {
        return Err(ApiError::BadRequest("room is not active".into()).into());
    }

    let caller = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("you are not in this room".into()))?;

    if caller.status != "joined" {
        return Err(ApiError::Forbidden("you must be joined to invite others".into()).into());
    }

    let now = chrono::Utc::now().timestamp();
    let mut count: u32 = 0;
    let mut cooldown_blocked_users: Vec<String> = Vec::new();

    for invite in body.invites {
        let user_id = invite.user_id.trim().to_string();
        if user_id.is_empty() || user_id == auth.user_id {
            continue;
        }

        let role = normalize_member_role(&invite.role)?;
        if role == "host" {
            return Err(ApiError::BadRequest("invite role cannot be host".into()).into());
        }

        if let Some(existing_member) =
            rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &user_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        {
            if existing_member.status == "joined" {
                continue;
            }

            if let Some(last_invited_ts) = existing_member.invited_ts {
                if now - last_invited_ts < INVITE_RESEND_COOLDOWN_SECONDS {
                    if let Some(user) = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
                        .await
                        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                    {
                        cooldown_blocked_users.push(user.username);
                    } else {
                        cooldown_blocked_users.push(user_id.clone());
                    }
                    continue;
                }
            }
        }

        let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("invited user not found".into()))?;

        if room.room_mode == "video" {
            let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
            ensure_library_access_for_user(&state, &user.id, &user.role, &item.library_id).await?;
        } else if let Some(ref lib_id) = room.audio_library_id {
            ensure_library_access_for_user(&state, &user.id, &user.role, lib_id).await?;
        }

        rustfin_db::repo::watch_party::upsert_member(
            &state.db,
            &room_id,
            &rustfin_db::repo::watch_party::NewWatchPartyMember {
                user_id: user_id.clone(),
                role,
                status: "invited".to_string(),
                invited_by: Some(auth.user_id.clone()),
                invited_ts: Some(now),
                joined_ts: None,
            },
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        count += 1;
    }

    let _ = rustfin_db::repo::watch_party::touch_room_updated(&state.db, &room_id).await;

    Ok(Json(InviteMembersResponse {
        ok: true,
        invited: count,
        cooldown_blocked_users,
    }))
}

pub async fn search_youtube(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(params): Query<YouTubeSearchQuery>,
) -> Result<Json<Vec<YouTubeSearchResult>>, AppError> {
    check_rate_limit(
        youtube_search_rate_limiter(),
        &format!("youtube-search:{}:{}", room_id, auth.user_id),
    )
    .await?;

    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.room_mode != "youtube" {
        return Err(ApiError::BadRequest(
            "youtube search is only available in YouTube rooms".into(),
        )
        .into());
    }

    let member = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("room membership not found; join first".into()))?;

    if member.status != "joined" {
        return Err(ApiError::Forbidden("room membership is not joined".into()).into());
    }

    let (_, results) = perform_youtube_search(&params.q, params.limit).await?;
    Ok(Json(results))
}

pub async fn lookup_youtube_videos(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<YouTubeLookupRequest>,
) -> Result<Json<Vec<YouTubeSearchResult>>, AppError> {
    check_rate_limit(
        youtube_search_rate_limiter(),
        &format!("youtube-lookup:{}:{}", room_id, auth.user_id),
    )
    .await?;

    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.room_mode != "youtube" {
        return Err(ApiError::BadRequest(
            "youtube lookup is only available in YouTube rooms".into(),
        )
        .into());
    }

    let member = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("room membership not found; join first".into()))?;

    if member.status != "joined" {
        return Err(ApiError::Forbidden("room membership is not joined".into()).into());
    }

    if body.video_ids.is_empty() {
        return Ok(Json(Vec::new()));
    }

    if body.video_ids.len() > YOUTUBE_LOOKUP_MAX_IDS {
        return Err(ApiError::BadRequest(format!(
            "too many video_ids; maximum is {YOUTUBE_LOOKUP_MAX_IDS}"
        ))
        .into());
    }

    let mut deduped = Vec::with_capacity(body.video_ids.len());
    let mut seen = std::collections::HashSet::new();
    for raw in body.video_ids {
        let video_id = raw.trim();
        if !is_valid_youtube_video_id(video_id) {
            continue;
        }
        if seen.insert(video_id.to_string()) {
            deduped.push(video_id.to_string());
        }
    }

    if deduped.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let client = reqwest::Client::new();
    let mut resolved = Vec::with_capacity(deduped.len());
    for video_id in deduped {
        if let Some(metadata) = fetch_youtube_video_metadata(&client, &video_id).await {
            resolved.push(metadata);
        }
    }

    Ok(Json(resolved))
}

pub async fn list_audio_tracks(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(params): Query<AudioTracksQuery>,
) -> Result<Json<Vec<AudioTrackResponse>>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    let audio_lib_id = room
        .audio_library_id
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("this room is not an audio room".into()))?;

    ensure_library_access_for_user(&state, &auth.user_id, &auth.role, audio_lib_id).await?;

    let tracks = rustfin_db::repo::watch_party::get_library_tracks(
        &state.db,
        audio_lib_id,
        params.q.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // Convert local poster paths to API URLs
    let responses = tracks
        .into_iter()
        .map(|t| {
            let album_art_url = t.album_art_url.map(|url| {
                if url.starts_with("http://") || url.starts_with("https://") {
                    url
                } else {
                    // It's a local path — find the album item to get the proper API URL.
                    // We'll return a placeholder that can be constructed by the client.
                    // For now return the path as-is so the image API can serve it.
                    url
                }
            });
            AudioTrackResponse {
                id: t.id,
                title: t.title,
                artist: t.artist,
                album: t.album,
                album_art_url,
                duration_ms: t.duration_ms,
            }
        })
        .collect();

    Ok(Json(responses))
}
