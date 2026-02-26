use std::time::Duration;

use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const WEB_URL_MAX_LEN: usize = 2048;
const YOUTUBE_SEARCH_MIN_QUERY_LEN: usize = 2;
const YOUTUBE_SEARCH_MAX_QUERY_LEN: usize = 100;
const YOUTUBE_SEARCH_DEFAULT_LIMIT: usize = 10;
const YOUTUBE_SEARCH_MAX_LIMIT: usize = 20;
const YOUTUBE_SEARCH_TIMEOUT_SECONDS: u64 = 8;
const YOUTUBE_LOOKUP_TIMEOUT_SECONDS: u64 = 5;
const YOUTUBE_VALIDATION_TIMEOUT_SECONDS: u64 = 6;
const YOUTUBE_SEARCH_URL: &str = "https://www.youtube.com/results";
const YOUTUBE_WATCH_URL: &str = "https://www.youtube.com/watch";
const YOUTUBE_SEARCH_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const YOUTUBE_VALIDATION_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

pub(crate) const YOUTUBE_LOOKUP_MAX_IDS: usize = 25;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct YouTubeSearchResult {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_count: Option<u64>,
}

pub(crate) fn is_valid_youtube_video_id(value: &str) -> bool {
    value.len() == 11
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn extract_youtube_video_id_from_input(raw: &str) -> Option<String> {
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

pub(crate) fn normalize_web_room_url(raw: &str) -> Result<String, AppError> {
    let mut candidate = raw.trim().to_string();
    if candidate.is_empty() {
        candidate = "https://www.mozilla.org/".to_string();
    }
    if candidate.len() > WEB_URL_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "web_url must be <= {WEB_URL_MAX_LEN} characters"
        ))
        .into());
    }

    if !candidate.contains("://") {
        candidate = format!("https://{candidate}");
    }

    let parsed = reqwest::Url::parse(&candidate)
        .map_err(|_| ApiError::BadRequest("web_url must be a valid http(s) URL".into()))?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(ApiError::BadRequest("web_url must use http:// or https://".into()).into());
        }
    }

    if parsed.host_str().is_none() {
        return Err(ApiError::BadRequest("web_url must include a host".into()).into());
    }

    Ok(parsed.to_string())
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
    if parts.len() > 1
        && matches!(parts[1], "k" | "m" | "b")
        && !matches!(token.chars().last(), Some('k' | 'm' | 'b'))
    {
        token.push_str(parts[1]);
    }

    if token.is_empty() {
        return None;
    }

    if let Some(suffix) = token.chars().last()
        && matches!(suffix, 'k' | 'm' | 'b')
        && token.len() > 1
    {
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

pub(crate) async fn fetch_youtube_video_metadata(
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
    let url = reqwest::Url::parse_with_params(YOUTUBE_WATCH_URL, &[("v", video_id), ("hl", "en")])
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
    if let Some(video_id) = extract_youtube_video_id_from_input(&query)
        && let Some(metadata) = fetch_youtube_video_metadata(&client, &video_id).await
    {
        results.push(metadata);
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

fn youtube_playability_reason(playability: &serde_json::Value) -> String {
    if let Some(reason) = playability
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return reason.to_string();
    }

    if let Some(reason) = playability
        .pointer("/errorScreen/playerErrorMessageRenderer/reason/simpleText")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return reason.to_string();
    }

    if let Some(runs) = playability
        .pointer("/errorScreen/playerErrorMessageRenderer/reason/runs")
        .and_then(serde_json::Value::as_array)
    {
        let mut merged = String::new();
        for run in runs {
            if let Some(text) = run.get("text").and_then(serde_json::Value::as_str) {
                merged.push_str(text);
            }
        }
        let merged = merged.trim();
        if !merged.is_empty() {
            return merged.to_string();
        }
    }

    "This YouTube video cannot be embedded by the uploader. Try another video.".to_string()
}

pub(crate) async fn youtube_embed_block_reason(video_id: &str) -> Option<String> {
    let url = reqwest::Url::parse_with_params(
        YOUTUBE_WATCH_URL,
        &[("v", video_id), ("hl", "en"), ("persist_hl", "1")],
    )
    .ok()?;

    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(YOUTUBE_VALIDATION_TIMEOUT_SECONDS))
        .header(reqwest::header::USER_AGENT, YOUTUBE_VALIDATION_USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .ok()?;

    if response.url().as_str().contains("consent.youtube.com") {
        return Some(
            "YouTube validation was blocked by a consent/interstitial page on this network."
                .to_string(),
        );
    }

    if !response.status().is_success() {
        return None;
    }

    let html = response.text().await.ok()?;
    let initial_player_response =
        extract_json_object_after_marker(&html, "var ytInitialPlayerResponse = ")
            .or_else(|| extract_json_object_after_marker(&html, "ytInitialPlayerResponse = "))?;

    let player_json: serde_json::Value = serde_json::from_str(&initial_player_response).ok()?;
    let playability = player_json.get("playabilityStatus")?;

    if playability
        .get("playableInEmbed")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return Some(youtube_playability_reason(playability));
    }

    match playability
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
    {
        "ERROR" | "UNPLAYABLE" | "LOGIN_REQUIRED" => Some(youtube_playability_reason(playability)),
        _ => None,
    }
}
