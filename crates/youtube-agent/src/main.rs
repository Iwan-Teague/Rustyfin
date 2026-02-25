use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use regex::Regex;
use reqwest::header::HeaderValue;
use rustfin_core::error::{ApiError, ErrorEnvelope};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};
use std::sync::OnceLock;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

const YOUTUBE_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const YOUTUBE_CONSENT_COOKIE: &str = "CONSENT=YES+cb.20210328-17-p0.en+FX+471";
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MIN_SOURCE_BYTES: u64 = 32 * 1024;
const DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS: u64 = 20;
const DOWNLOAD_TOTAL_TIMEOUT_SECONDS: u64 = 90;
const MAX_YOUTUBEI_DIRECT_CANDIDATES: usize = 8;
const CONVERT_TO_MP3_TIMEOUT_SECONDS: u64 = 180;

#[derive(Clone)]
struct AppState {
    cache_dir: PathBuf,
    ffmpeg_path: PathBuf,
    ffprobe_path: PathBuf,
    youtube_cookie: Option<String>,
    youtube_cookie_file: Option<PathBuf>,
    agent_token: Option<String>,
}

struct AppError(ApiError);

impl From<ApiError> for AppError {
    fn from(value: ApiError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadAudioRequest {
    room_id: String,
    video_id: String,
}

#[derive(Debug, Serialize)]
struct DownloadAudioResponse {
    room_id: String,
    video_id: String,
    file_path: String,
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct DownloadedAudio {
    file_path: PathBuf,
    duration_ms: Option<u64>,
}

fn normalized_secret(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn verify_agent_token(headers: &HeaderMap, expected: Option<&str>) -> Result<(), AppError> {
    let Some(expected_token) = expected.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };

    let supplied = headers
        .get("x-agent-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();

    if supplied != expected_token {
        return Err(ApiError::Unauthorized("missing or invalid youtube-agent token".into()).into());
    }
    Ok(())
}

fn room_id_regex() -> &'static Regex {
    static ROOM_ID_RE: OnceLock<Regex> = OnceLock::new();
    ROOM_ID_RE.get_or_init(|| {
        Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$").expect("valid room id regex")
    })
}

fn normalize_room_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if room_id_regex().is_match(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn parse_video_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:youtu\.be/|youtube\.com/(?:watch\?.*v=|shorts/|embed/))([A-Za-z0-9_-]{11})",
        )
        .expect("valid youtube id regex")
    })
}

fn normalize_video_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() == 11
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some(trimmed.to_string());
    }

    parse_video_id_regex()
        .captures(trimmed)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

fn room_audio_dir(state: &AppState, room_id: &str) -> PathBuf {
    state.cache_dir.join("watch_party_audio").join(room_id)
}

async fn ensure_room_audio_dir(state: &AppState, room_id: &str) -> Result<PathBuf, AppError> {
    let dir = room_audio_dir(state, room_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to create room audio dir: {e}")))?;
    Ok(dir)
}

async fn probe_audio_duration_ms(
    ffprobe_path: &StdPath,
    path: &StdPath,
) -> Result<Option<u64>, AppError> {
    let output = tokio::process::Command::new(ffprobe_path)
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .await
        .map_err(|e| ApiError::Internal(format!("ffprobe execution failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::Internal(format!(
            "ffprobe failed while probing downloaded audio: {}",
            stderr.trim()
        ))
        .into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let seconds = stdout.trim().parse::<f64>().ok();
    Ok(seconds.and_then(|s| {
        if !s.is_finite() || s < 0.0 {
            None
        } else {
            Some((s * 1000.0).round() as u64)
        }
    }))
}

async fn convert_to_mp3(
    ffmpeg_path: &StdPath,
    input_path: &StdPath,
    output_path: &StdPath,
) -> Result<(), AppError> {
    let output = tokio::process::Command::new(ffmpeg_path)
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-vn")
        .arg("-acodec")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("192k")
        .arg(output_path)
        .output()
        .await
        .map_err(|e| ApiError::Internal(format!("ffmpeg execution failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::Internal(format!(
            "ffmpeg failed while converting downloaded audio to mp3: {}",
            stderr.trim()
        ))
        .into());
    }
    Ok(())
}

fn load_cookie_header_from_file(path: &StdPath) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut pairs: Vec<String> = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 7 {
            let domain = cols[0].trim().to_ascii_lowercase();
            if domain.contains("youtube.com")
                || domain.contains("googlevideo.com")
                || domain.contains("google.com")
            {
                let name = cols[5].trim();
                let value = cols[6].trim();
                if !name.is_empty() && !value.is_empty() {
                    pairs.push(format!("{name}={value}"));
                }
            }
        }
    }

    if !pairs.is_empty() {
        return Some(pairs.join("; "));
    }

    Some(trimmed.to_string())
}

fn youtube_cookie_header(state: &AppState) -> Option<String> {
    let value = state.youtube_cookie.clone().or_else(|| {
        state
            .youtube_cookie_file
            .as_deref()
            .and_then(load_cookie_header_from_file)
    });

    match value {
        Some(mut cookie) => {
            if !cookie.to_ascii_lowercase().contains("consent=") {
                cookie.push_str("; ");
                cookie.push_str(YOUTUBE_CONSENT_COOKIE);
            }
            Some(cookie)
        }
        None => Some(YOUTUBE_CONSENT_COOKIE.to_string()),
    }
}

fn youtube_video_options(
    quality: rusty_ytdl::VideoQuality,
    filter: rusty_ytdl::VideoSearchOptions,
    cookie_header: Option<String>,
) -> rusty_ytdl::VideoOptions {
    let client = build_ytdl_request_client(cookie_header.as_deref());
    let fallback_cookie = if client.is_none() {
        cookie_header
    } else {
        None
    };

    rusty_ytdl::VideoOptions {
        quality,
        filter,
        download_options: rusty_ytdl::DownloadOptions {
            dl_chunk_size: Some(10 * 1024 * 1024),
        },
        request_options: rusty_ytdl::RequestOptions {
            client,
            cookies: fallback_cookie,
            max_retries: Some(8),
            ..Default::default()
        },
    }
}

fn build_ytdl_request_client(cookie_header: Option<&str>) -> Option<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_static(YOUTUBE_USER_AGENT),
    );
    headers.insert(reqwest::header::ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        reqwest::header::REFERER,
        HeaderValue::from_static("https://www.youtube.com/"),
    );
    headers.insert(
        reqwest::header::ORIGIN,
        HeaderValue::from_static("https://www.youtube.com"),
    );

    if let Some(cookie) = cookie_header {
        let parsed = HeaderValue::from_str(cookie).ok()?;
        headers.insert(reqwest::header::COOKIE, parsed);
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .ok()
}

fn compact_source_url_for_error(source_url: &str) -> String {
    reqwest::Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| format!("{}://{}{}", url.scheme(), host, url.path()))
        })
        .unwrap_or_else(|| source_url.chars().take(200).collect::<String>())
}

fn sanitize_youtube_download_error(raw: &str) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let re = URL_RE
        .get_or_init(|| Regex::new(r"https?://[^\s\)]+").expect("valid youtube sanitizer regex"));
    re.replace_all(raw, |captures: &regex::Captures<'_>| {
        compact_source_url_for_error(&captures[0])
    })
    .to_string()
}

fn remaining_download_timeout(started_at: std::time::Instant) -> Option<std::time::Duration> {
    let total = std::time::Duration::from_secs(DOWNLOAD_TOTAL_TIMEOUT_SECONDS);
    let elapsed = started_at.elapsed();
    if elapsed >= total {
        return None;
    }

    let remaining_total = total.saturating_sub(elapsed);
    let per_attempt = std::time::Duration::from_secs(DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS);
    Some(std::cmp::min(remaining_total, per_attempt))
}

fn extract_url_from_signature_cipher(cipher: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(&format!("https://example.invalid/?{cipher}")).ok()?;
    let mut url = parsed
        .query_pairs()
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.to_string())?;
    let sp = parsed
        .query_pairs()
        .find(|(k, _)| k == "sp")
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "signature".to_string());

    if let Some(sig) = parsed
        .query_pairs()
        .find(|(k, _)| k == "sig" || k == "signature")
        .map(|(_, v)| v.to_string())
    {
        let joiner = if url.contains('?') { "&" } else { "?" };
        url.push_str(joiner);
        url.push_str(&sp);
        url.push('=');
        url.push_str(&sig);
        return Some(url);
    }

    if parsed.query_pairs().any(|(k, _)| k == "s") {
        return None;
    }

    Some(url)
}

fn extract_innertube_api_key(html: &str) -> Option<String> {
    static KEY_RE: OnceLock<Regex> = OnceLock::new();
    KEY_RE
        .get_or_init(|| {
            Regex::new(r#""INNERTUBE_API_KEY":"([^"]+)""#).expect("valid innertube key regex")
        })
        .captures(html)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

#[derive(Default, Clone)]
struct YouTubeIContextHints {
    signature_timestamp: Option<u64>,
    visitor_data: Option<String>,
    hl: Option<String>,
    gl: Option<String>,
}

fn extract_ytcfg_json(html: &str) -> Option<serde_json::Value> {
    let marker = "ytcfg.set(";
    let marker_idx = html.find(marker)?;
    let after_marker = &html[marker_idx + marker.len()..];
    let brace_start = after_marker.find('{')?;
    let json_slice = &after_marker[brace_start..];

    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in json_slice.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let json = &json_slice[..=idx];
                    return serde_json::from_str::<serde_json::Value>(json).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_youtubei_context_hints(html: &str) -> YouTubeIContextHints {
    let cfg = extract_ytcfg_json(html);
    let signature_timestamp = cfg.as_ref().and_then(|v| v.get("STS")).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    });

    let visitor_data = cfg
        .as_ref()
        .and_then(|v| v.pointer("/INNERTUBE_CONTEXT/client/visitorData"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let hl = cfg
        .as_ref()
        .and_then(|v| v.pointer("/INNERTUBE_CONTEXT/client/hl"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let gl = cfg
        .as_ref()
        .and_then(|v| v.pointer("/INNERTUBE_CONTEXT/client/gl"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    YouTubeIContextHints {
        signature_timestamp,
        visitor_data,
        hl,
        gl,
    }
}

#[derive(Clone, Copy)]
struct YouTubeIClientProfile {
    client_name: &'static str,
    client_version: &'static str,
    x_youtube_client_name: Option<&'static str>,
    x_youtube_client_version: Option<&'static str>,
    include_third_party_embed: bool,
}

const YOUTUBEI_CLIENT_PROFILES: [YouTubeIClientProfile; 4] = [
    YouTubeIClientProfile {
        client_name: "ANDROID",
        client_version: "19.08.35",
        x_youtube_client_name: Some("3"),
        x_youtube_client_version: Some("19.08.35"),
        include_third_party_embed: false,
    },
    YouTubeIClientProfile {
        client_name: "IOS",
        client_version: "19.09.3",
        x_youtube_client_name: Some("5"),
        x_youtube_client_version: Some("19.09.3"),
        include_third_party_embed: false,
    },
    YouTubeIClientProfile {
        client_name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
        client_version: "2.0",
        x_youtube_client_name: Some("85"),
        x_youtube_client_version: Some("2.0"),
        include_third_party_embed: true,
    },
    YouTubeIClientProfile {
        client_name: "WEB",
        client_version: "2.20250220.00.00",
        x_youtube_client_name: Some("1"),
        x_youtube_client_version: Some("2.20250220.00.00"),
        include_third_party_embed: false,
    },
];

fn extract_candidate_urls_from_youtubei_payload(payload: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in ["/streamingData/adaptiveFormats", "/streamingData/formats"] {
        let Some(formats) = payload.pointer(path).and_then(|v| v.as_array()) else {
            continue;
        };
        for format in formats {
            let mime = format
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let has_audio = format.get("audioQuality").is_some()
                || format.get("audioSampleRate").is_some()
                || mime.starts_with("audio/");
            if !has_audio {
                continue;
            }

            if let Some(url) = format.get("url").and_then(|v| v.as_str()) {
                if !url.trim().is_empty() {
                    out.push(url.to_string());
                    continue;
                }
            }
            if let Some(cipher) = format.get("signatureCipher").and_then(|v| v.as_str()) {
                if let Some(url) = extract_url_from_signature_cipher(cipher) {
                    out.push(url);
                }
            }
        }
    }
    out.retain(|v| !v.trim().is_empty());
    out.dedup();
    out
}

fn youtubei_playability_reason(payload: &serde_json::Value) -> Option<String> {
    let status = payload
        .pointer("/playabilityStatus/status")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let reason = payload
        .pointer("/playabilityStatus/reason")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if status.is_empty() && reason.is_empty() {
        return None;
    }
    Some(format!("{status} {reason}").trim().to_string())
}

async fn youtubei_audio_candidate_urls(
    video_id: &str,
    cookie_header: Option<&str>,
) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("youtubei client build failed: {e}"))?;

    let watch_url = format!("https://www.youtube.com/watch?v={video_id}");
    let mut watch_req = client
        .get(&watch_url)
        .header(reqwest::header::USER_AGENT, YOUTUBE_USER_AGENT)
        .header(reqwest::header::REFERER, "https://www.youtube.com/")
        .header(reqwest::header::ORIGIN, "https://www.youtube.com")
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9");
    if let Some(cookie) = cookie_header {
        watch_req = watch_req.header(reqwest::header::COOKIE, cookie);
    }

    let watch_html = watch_req
        .send()
        .await
        .map_err(|e| format!("watch page request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("watch page body read failed: {e}"))?;
    let api_key = extract_innertube_api_key(&watch_html)
        .ok_or_else(|| "missing INNERTUBE_API_KEY in watch page".to_string())?;
    let hints = extract_youtubei_context_hints(&watch_html);

    let youtubei_url =
        format!("https://www.youtube.com/youtubei/v1/player?key={api_key}&prettyPrint=false");
    let mut errors: Vec<String> = Vec::new();

    for profile in YOUTUBEI_CLIENT_PROFILES {
        let mut context = serde_json::json!({
            "client": {
                "clientName": profile.client_name,
                "clientVersion": profile.client_version,
                "hl": hints.hl.clone().unwrap_or_else(|| "en".to_string()),
                "gl": hints.gl.clone().unwrap_or_else(|| "US".to_string()),
            }
        });
        if let Some(visitor_data) = hints.visitor_data.as_deref() {
            context["client"]["visitorData"] = serde_json::json!(visitor_data);
        }
        if profile.include_third_party_embed {
            context["thirdParty"] = serde_json::json!({
                "embedUrl": "https://www.youtube.com/"
            });
        }

        let mut payload = serde_json::json!({
            "videoId": video_id,
            "context": context,
            "contentCheckOk": true,
            "racyCheckOk": true
        });
        if let Some(sts) = hints.signature_timestamp {
            payload["playbackContext"] = serde_json::json!({
                "contentPlaybackContext": {
                    "signatureTimestamp": sts,
                    "html5Preference": "HTML5_PREF_WANTS"
                }
            });
        }

        let mut player_req = client
            .post(&youtubei_url)
            .header(reqwest::header::USER_AGENT, YOUTUBE_USER_AGENT)
            .header(reqwest::header::REFERER, "https://www.youtube.com/")
            .header(reqwest::header::ORIGIN, "https://www.youtube.com")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .json(&payload);
        if let Some(cookie) = cookie_header {
            player_req = player_req.header(reqwest::header::COOKIE, cookie);
        }
        if let Some(name) = profile.x_youtube_client_name {
            player_req = player_req.header("x-youtube-client-name", name);
        }
        if let Some(version) = profile.x_youtube_client_version {
            player_req = player_req.header("x-youtube-client-version", version);
        }

        let response = player_req
            .send()
            .await
            .map_err(|e| format!("youtubei player request failed: {e}"))?;

        if !response.status().is_success() {
            errors.push(format!(
                "{}: http {}",
                profile.client_name,
                response.status()
            ));
            continue;
        }

        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("youtubei player response parse failed: {e}"))?;
        let out = extract_candidate_urls_from_youtubei_payload(&payload);
        if !out.is_empty() {
            return Ok(out);
        }

        if let Some(reason) = youtubei_playability_reason(&payload) {
            errors.push(format!("{}: {}", profile.client_name, reason));
        } else {
            errors.push(format!("{}: no audio URLs in payload", profile.client_name));
        }
    }

    Err(format!(
        "youtubei returned no usable audio candidate URLs ({})",
        errors.join(" | ")
    ))
}

fn format_matches_filter(
    format: &rusty_ytdl::VideoFormat,
    filter: &rusty_ytdl::VideoSearchOptions,
) -> bool {
    match filter {
        rusty_ytdl::VideoSearchOptions::Audio => {
            (!format.has_video && format.has_audio) || format.is_live
        }
        rusty_ytdl::VideoSearchOptions::Video => {
            (format.has_video && !format.has_audio) || format.is_live
        }
        rusty_ytdl::VideoSearchOptions::VideoAudio => {
            (format.has_video && format.has_audio) || format.is_live
        }
        rusty_ytdl::VideoSearchOptions::Custom(func) => func(format) || format.is_live,
    }
}

fn candidate_format_urls(
    info: &rusty_ytdl::VideoInfo,
    options: &rusty_ytdl::VideoOptions,
) -> Vec<String> {
    let mut formats: Vec<&rusty_ytdl::VideoFormat> = info
        .formats
        .iter()
        .filter(|format| {
            !format.url.trim().is_empty()
                && !format.is_hls
                && !format.is_dash_mpd
                && format_matches_filter(format, &options.filter)
        })
        .collect();

    if formats.is_empty() {
        formats = info
            .formats
            .iter()
            .filter(|format| {
                !format.url.trim().is_empty()
                    && !format.is_hls
                    && !format.is_dash_mpd
                    && format.has_audio
            })
            .collect();
    }

    match options.quality {
        rusty_ytdl::VideoQuality::Lowest
        | rusty_ytdl::VideoQuality::LowestAudio
        | rusty_ytdl::VideoQuality::LowestVideo => formats.reverse(),
        _ => {}
    }

    formats
        .into_iter()
        .take(12)
        .map(|format| format.url.trim().to_string())
        .collect()
}

async fn download_url_to_temp_with_headers(
    source_url: &str,
    temp_path: &StdPath,
    cookie: Option<&str>,
    watch_referer: Option<&str>,
) -> Result<u64, String> {
    let compact_source_url = compact_source_url_for_error(source_url);
    if !source_url.starts_with("https://") {
        return Err(format!(
            "non-https source URL is not allowed ({compact_source_url})"
        ));
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| format!("http client build failed: {e}"))?;

    let desktop_ua = YOUTUBE_USER_AGENT;
    let android_ua = "com.google.android.youtube/19.08.35 (Linux; U; Android 14; en_US; Pixel 8 Pro Build/UQ1A.240205.002; Cronet/119.0.6045.194)";
    let ios_ua = "com.google.ios.youtube/19.09.3 (iPhone16,2; U; CPU iOS 17_4 like Mac OS X)";

    let profiles: [(&str, bool, Option<&str>, Option<&str>, &str); 7] = [
        (
            "desktop+range+watch-referer",
            true,
            watch_referer,
            Some("https://www.youtube.com"),
            desktop_ua,
        ),
        (
            "desktop+no-range+watch-referer",
            false,
            watch_referer,
            Some("https://www.youtube.com"),
            desktop_ua,
        ),
        (
            "desktop+no-range+youtube-referer",
            false,
            Some("https://www.youtube.com/"),
            Some("https://www.youtube.com"),
            desktop_ua,
        ),
        ("desktop+no-range+minimal", false, None, None, desktop_ua),
        (
            "android+no-range+watch-referer",
            false,
            watch_referer,
            Some("https://www.youtube.com"),
            android_ua,
        ),
        (
            "ios+no-range+watch-referer",
            false,
            watch_referer,
            Some("https://www.youtube.com"),
            ios_ua,
        ),
        ("android+no-range+minimal", false, None, None, android_ua),
    ];

    let mut profile_errors: Vec<String> = Vec::new();
    for (label, include_range, referer, origin, user_agent) in profiles {
        let mut request = client
            .get(source_url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9");

        if include_range {
            request = request.header(reqwest::header::RANGE, "bytes=0-");
        }
        if let Some(referer_value) = referer {
            request = request.header(reqwest::header::REFERER, referer_value);
        }
        if let Some(origin_value) = origin {
            request = request.header(reqwest::header::ORIGIN, origin_value);
        }
        if let Some(cookie_value) = cookie {
            request = request.header(reqwest::header::COOKIE, cookie_value);
        }

        let mut response = match request.send().await {
            Ok(res) => res,
            Err(err) => {
                profile_errors.push(format!("{label}: http request failed: {err}"));
                let _ = tokio::fs::remove_file(temp_path).await;
                continue;
            }
        };

        if !response.status().is_success() {
            profile_errors.push(format!(
                "{label}: http status {} for source ({compact_source_url})",
                response.status()
            ));
            let _ = tokio::fs::remove_file(temp_path).await;
            continue;
        }

        let mut file = match tokio::fs::File::create(temp_path).await {
            Ok(file) => file,
            Err(err) => {
                profile_errors.push(format!("{label}: temp file create failed: {err}"));
                continue;
            }
        };

        let mut downloaded_bytes: u64 = 0;
        let mut read_failed = false;
        loop {
            let maybe_chunk = match response.chunk().await {
                Ok(chunk) => chunk,
                Err(err) => {
                    profile_errors.push(format!("{label}: http response read failed: {err}"));
                    read_failed = true;
                    break;
                }
            };
            let Some(chunk) = maybe_chunk else {
                break;
            };
            downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
            if let Err(err) = tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await {
                profile_errors.push(format!("{label}: temp file write failed: {err}"));
                read_failed = true;
                break;
            }
        }

        if !read_failed {
            if let Err(err) = tokio::io::AsyncWriteExt::flush(&mut file).await {
                profile_errors.push(format!("{label}: temp file flush failed: {err}"));
                read_failed = true;
            }
        }

        if read_failed {
            let _ = tokio::fs::remove_file(temp_path).await;
            continue;
        }

        if downloaded_bytes < MIN_SOURCE_BYTES {
            profile_errors.push(format!(
                "{label}: downloaded too few bytes ({downloaded_bytes}), likely blocked source"
            ));
            let _ = tokio::fs::remove_file(temp_path).await;
            continue;
        }

        return Ok(downloaded_bytes);
    }

    Err(format!(
        "all request profiles failed. {}",
        profile_errors.join(" | ")
    ))
}

async fn download_youtube_source_to_temp_with_stream(
    watch_url: &str,
    options: rusty_ytdl::VideoOptions,
    temp_path: &StdPath,
) -> Result<u64, String> {
    let video = rusty_ytdl::Video::new_with_options(watch_url, options)
        .map_err(|e| format!("download init failed: {e}"))?;
    let stream = video
        .stream()
        .await
        .map_err(|e| format!("stream open failed: {e}"))?;

    let mut file = tokio::fs::File::create(temp_path)
        .await
        .map_err(|e| format!("temp file create failed: {e}"))?;

    let mut downloaded_bytes: u64 = 0;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|e| format!("stream read failed: {e}"))?
    {
        downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| format!("temp file write failed: {e}"))?;
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| format!("temp file flush failed: {e}"))?;

    if downloaded_bytes < MIN_SOURCE_BYTES {
        return Err(format!(
            "downloaded too few bytes ({downloaded_bytes}), likely blocked source"
        ));
    }

    Ok(downloaded_bytes)
}

async fn download_youtube_source_to_temp(
    watch_url: &str,
    options: rusty_ytdl::VideoOptions,
    temp_path: &StdPath,
    cookie_header: Option<&str>,
) -> Result<u64, String> {
    let video = rusty_ytdl::Video::new_with_options(watch_url, options.clone())
        .map_err(|e| format!("download init failed: {e}"))?;
    let info = video
        .get_info()
        .await
        .map_err(|e| format!("format resolution failed: {e}"))?;
    let candidate_urls = candidate_format_urls(&info, &options);
    if candidate_urls.is_empty() {
        return Err("no candidate formats with downloadable URL".to_string());
    }

    let mut errors: Vec<String> = Vec::new();
    for (idx, source_url) in candidate_urls.iter().enumerate() {
        match download_url_to_temp_with_headers(
            source_url,
            temp_path,
            cookie_header,
            Some(watch_url),
        )
        .await
        {
            Ok(bytes) => return Ok(bytes),
            Err(reason) => {
                errors.push(format!("candidate #{}: {}", idx + 1, reason));
                let _ = tokio::fs::remove_file(temp_path).await;
            }
        }
    }

    Err(format!(
        "all candidate format URLs failed. {}",
        errors.join(" | ")
    ))
}

async fn try_youtubei_direct_download(
    video_id: &str,
    temp_path: &StdPath,
    cookie_header: Option<&str>,
    started_at: std::time::Instant,
    last_errors: &mut Vec<String>,
) -> bool {
    let Some(resolve_timeout) = remaining_download_timeout(started_at) else {
        last_errors.push(format!(
            "youtubei-direct budget exceeded after {}s before resolution",
            DOWNLOAD_TOTAL_TIMEOUT_SECONDS
        ));
        return false;
    };

    let resolved = tokio::time::timeout(
        resolve_timeout,
        youtubei_audio_candidate_urls(video_id, cookie_header),
    )
    .await;

    let youtube_watch_referer = format!("https://www.youtube.com/watch?v={video_id}");
    let candidate_urls = match resolved {
        Ok(Ok(candidate_urls)) => candidate_urls,
        Ok(Err(reason)) => {
            last_errors.push(format!(
                "youtubei-direct resolution failed: {}",
                sanitize_youtube_download_error(&reason)
            ));
            return false;
        }
        Err(_) => {
            last_errors.push(format!(
                "youtubei-direct resolution timed out after {}s",
                DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS
            ));
            return false;
        }
    };

    for (idx, source_url) in candidate_urls
        .iter()
        .take(MAX_YOUTUBEI_DIRECT_CANDIDATES)
        .enumerate()
    {
        let Some(timeout) = remaining_download_timeout(started_at) else {
            last_errors.push(format!(
                "youtubei-direct budget exceeded after {}s",
                DOWNLOAD_TOTAL_TIMEOUT_SECONDS
            ));
            return false;
        };

        match tokio::time::timeout(
            timeout,
            download_url_to_temp_with_headers(
                source_url,
                temp_path,
                cookie_header,
                Some(&youtube_watch_referer),
            ),
        )
        .await
        {
            Ok(Ok(_bytes)) => return true,
            Ok(Err(reason)) => {
                last_errors.push(format!(
                    "youtubei-direct candidate #{}: {}",
                    idx + 1,
                    sanitize_youtube_download_error(&reason)
                ));
                let _ = tokio::fs::remove_file(temp_path).await;
            }
            Err(_) => {
                last_errors.push(format!(
                    "youtubei-direct candidate #{}: attempt timed out after {}s",
                    idx + 1,
                    DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS
                ));
                let _ = tokio::fs::remove_file(temp_path).await;
            }
        }
    }

    false
}

async fn download_youtube_audio_mp3_for_room(
    state: &AppState,
    room_id: &str,
    video_id: &str,
) -> Result<DownloadedAudio, AppError> {
    let room_dir = ensure_room_audio_dir(state, room_id).await?;
    let temp_file_name = format!("{}.source", uuid::Uuid::new_v4());
    let output_file_name = format!("{}.mp3", uuid::Uuid::new_v4());
    let temp_path = room_dir.join(temp_file_name);
    let output_path = room_dir.join(output_file_name);
    let cookie_header = youtube_cookie_header(state);

    let watch_urls = [
        format!("https://www.youtube.com/watch?v={video_id}"),
        format!("https://youtu.be/{video_id}"),
        format!("https://music.youtube.com/watch?v={video_id}"),
    ];

    let mut last_errors: Vec<String> = Vec::new();
    let started_at = std::time::Instant::now();

    let mut source_downloaded = try_youtubei_direct_download(
        video_id,
        &temp_path,
        cookie_header.as_deref(),
        started_at,
        &mut last_errors,
    )
    .await;

    for watch_url in watch_urls {
        if source_downloaded {
            break;
        }
        if remaining_download_timeout(started_at).is_none() {
            last_errors.push(format!(
                "download budget exceeded after {}s before trying watch URL {}",
                DOWNLOAD_TOTAL_TIMEOUT_SECONDS,
                compact_source_url_for_error(&watch_url)
            ));
            break;
        }

        let attempts: [(&str, rusty_ytdl::VideoOptions); 3] = [
            (
                "highest-audio-only",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::HighestAudio,
                    rusty_ytdl::VideoSearchOptions::Audio,
                    cookie_header.clone(),
                ),
            ),
            (
                "lowest-audio-only",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::LowestAudio,
                    rusty_ytdl::VideoSearchOptions::Audio,
                    cookie_header.clone(),
                ),
            ),
            (
                "highest-muxed",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::Highest,
                    rusty_ytdl::VideoSearchOptions::VideoAudio,
                    cookie_header.clone(),
                ),
            ),
        ];

        for (label, options) in attempts {
            let Some(stream_timeout) = remaining_download_timeout(started_at) else {
                last_errors.push(format!(
                    "{label} @ {}: stream/direct budget exceeded after {}s",
                    compact_source_url_for_error(&watch_url),
                    DOWNLOAD_TOTAL_TIMEOUT_SECONDS
                ));
                break;
            };
            let stream_attempt = tokio::time::timeout(
                stream_timeout,
                download_youtube_source_to_temp_with_stream(
                    &watch_url,
                    options.clone(),
                    &temp_path,
                ),
            )
            .await;

            match stream_attempt {
                Ok(Ok(_bytes)) => {
                    source_downloaded = true;
                    break;
                }
                Ok(Err(stream_reason)) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    let Some(direct_timeout) = remaining_download_timeout(started_at) else {
                        last_errors.push(format!(
                            "{label} @ {}: stream={} | direct=budget exceeded after {}s",
                            compact_source_url_for_error(&watch_url),
                            sanitize_youtube_download_error(&stream_reason),
                            DOWNLOAD_TOTAL_TIMEOUT_SECONDS
                        ));
                        break;
                    };
                    let direct_attempt = tokio::time::timeout(
                        direct_timeout,
                        download_youtube_source_to_temp(
                            &watch_url,
                            options,
                            &temp_path,
                            cookie_header.as_deref(),
                        ),
                    )
                    .await;

                    match direct_attempt {
                        Ok(Ok(_bytes)) => {
                            source_downloaded = true;
                            break;
                        }
                        Ok(Err(candidate_reason)) => {
                            let stream_reason = sanitize_youtube_download_error(&stream_reason);
                            let candidate_reason =
                                sanitize_youtube_download_error(&candidate_reason);
                            last_errors.push(format!(
                                "{label} @ {}: stream={stream_reason} | direct={candidate_reason}",
                                compact_source_url_for_error(&watch_url)
                            ));
                            let _ = tokio::fs::remove_file(&temp_path).await;
                        }
                        Err(_) => {
                            let stream_reason = sanitize_youtube_download_error(&stream_reason);
                            last_errors.push(format!(
                                "{label} @ {}: stream={stream_reason} | direct=attempt timed out after {}s",
                                compact_source_url_for_error(&watch_url),
                                DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS
                            ));
                            let _ = tokio::fs::remove_file(&temp_path).await;
                        }
                    }
                }
                Err(_) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    let Some(direct_timeout) = remaining_download_timeout(started_at) else {
                        last_errors.push(format!(
                            "{label} @ {}: stream=attempt timed out | direct=budget exceeded after {}s",
                            compact_source_url_for_error(&watch_url),
                            DOWNLOAD_TOTAL_TIMEOUT_SECONDS
                        ));
                        break;
                    };
                    let direct_attempt = tokio::time::timeout(
                        direct_timeout,
                        download_youtube_source_to_temp(
                            &watch_url,
                            options,
                            &temp_path,
                            cookie_header.as_deref(),
                        ),
                    )
                    .await;

                    match direct_attempt {
                        Ok(Ok(_bytes)) => {
                            source_downloaded = true;
                            break;
                        }
                        Ok(Err(candidate_reason)) => {
                            let candidate_reason =
                                sanitize_youtube_download_error(&candidate_reason);
                            last_errors.push(format!(
                                "{label} @ {}: stream=attempt timed out after {}s | direct={candidate_reason}",
                                compact_source_url_for_error(&watch_url),
                                DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS
                            ));
                            let _ = tokio::fs::remove_file(&temp_path).await;
                        }
                        Err(_) => {
                            last_errors.push(format!(
                                "{label} @ {}: stream=attempt timed out after {}s | direct=attempt timed out after {}s",
                                compact_source_url_for_error(&watch_url),
                                DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS,
                                DOWNLOAD_ATTEMPT_TIMEOUT_SECONDS
                            ));
                            let _ = tokio::fs::remove_file(&temp_path).await;
                        }
                    }
                }
            }
        }

        if source_downloaded {
            break;
        }
    }

    if !source_downloaded {
        source_downloaded = try_youtubei_direct_download(
            video_id,
            &temp_path,
            cookie_header.as_deref(),
            started_at,
            &mut last_errors,
        )
        .await;
    }

    if !source_downloaded {
        let cookie_hint = if state.youtube_cookie.is_none() && state.youtube_cookie_file.is_none() {
            " Configure RUSTFIN_YOUTUBE_COOKIE (or RUSTFIN_YOUTUBE_COOKIE_FILE) and retry."
        } else {
            ""
        };
        return Err(ApiError::BadRequest(format!(
            "failed to open YouTube audio stream for this video. {}{}",
            last_errors.join(" | "),
            cookie_hint
        ))
        .into());
    }

    tokio::time::timeout(
        std::time::Duration::from_secs(CONVERT_TO_MP3_TIMEOUT_SECONDS),
        convert_to_mp3(&state.ffmpeg_path, &temp_path, &output_path),
    )
    .await
    .map_err(|_| {
        ApiError::Internal(format!(
            "ffmpeg conversion timed out after {}s",
            CONVERT_TO_MP3_TIMEOUT_SECONDS
        ))
    })??;
    let duration_ms = probe_audio_duration_ms(&state.ffprobe_path, &output_path).await?;
    let _ = tokio::fs::remove_file(&temp_path).await;

    Ok(DownloadedAudio {
        file_path: output_path,
        duration_ms,
    })
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn download_audio(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DownloadAudioRequest>,
) -> Result<Json<DownloadAudioResponse>, AppError> {
    verify_agent_token(&headers, state.agent_token.as_deref())?;

    let room_id = normalize_room_id(&body.room_id).ok_or_else(|| {
        ApiError::BadRequest(
            "room_id must contain only letters, numbers, underscores, and dashes".into(),
        )
    })?;
    let video_id = normalize_video_id(&body.video_id)
        .ok_or_else(|| ApiError::BadRequest("video_id must be a valid YouTube URL or ID".into()))?;

    info!(room_id = %room_id, video_id = %video_id, "youtube-agent audio download requested");
    let downloaded = download_youtube_audio_mp3_for_room(&state, &room_id, &video_id).await?;
    info!(
        room_id = %room_id,
        video_id = %video_id,
        file_path = %downloaded.file_path.display(),
        "youtube-agent audio download completed"
    );

    Ok(Json(DownloadAudioResponse {
        room_id,
        video_id,
        file_path: downloaded.file_path.to_string_lossy().to_string(),
        duration_ms: downloaded.duration_ms,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind_addr =
        std::env::var("RUSTFIN_YOUTUBE_AGENT_BIND").unwrap_or_else(|_| "0.0.0.0:8101".to_string());
    let cache_dir =
        PathBuf::from(std::env::var("RUSTFIN_CACHE_DIR").unwrap_or_else(|_| "/cache".to_string()));
    std::fs::create_dir_all(&cache_dir).context("failed to create cache dir")?;
    std::fs::create_dir_all(cache_dir.join("watch_party_audio"))
        .context("failed to create watch_party_audio cache dir")?;

    let ffmpeg_path = PathBuf::from(
        std::env::var("RUSTFIN_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string()),
    );
    let ffprobe_path = PathBuf::from(
        std::env::var("RUSTFIN_FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string()),
    );

    let youtube_cookie = normalized_secret(std::env::var("RUSTFIN_YOUTUBE_COOKIE").ok());
    let youtube_cookie_file =
        normalized_secret(std::env::var("RUSTFIN_YOUTUBE_COOKIE_FILE").ok()).map(PathBuf::from);
    let agent_token = normalized_secret(std::env::var("RUSTFIN_YOUTUBE_AGENT_TOKEN").ok());

    let state = AppState {
        cache_dir,
        ffmpeg_path,
        ffprobe_path,
        youtube_cookie,
        youtube_cookie_file,
        agent_token,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/download/audio", post(download_audio))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind youtube-agent at {bind_addr}"))?;
    info!(addr = %bind_addr, "youtube-agent listening");

    if let Err(err) = axum::serve(listener, app).await {
        warn!(error = %err, "youtube-agent server exited with error");
        return Err(anyhow::anyhow!(err));
    }
    Ok(())
}
