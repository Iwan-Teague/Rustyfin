use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use regex::Regex;
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
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
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
        Regex::new(r"(?i)(?:youtu\.be/|youtube\.com/(?:watch\?.*v=|shorts/|embed/))([A-Za-z0-9_-]{11})")
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
            if !cookie
                .to_ascii_lowercase()
                .contains("consent=")
            {
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
    rusty_ytdl::VideoOptions {
        quality,
        filter,
        download_options: rusty_ytdl::DownloadOptions {
            dl_chunk_size: Some(10 * 1024 * 1024),
        },
        request_options: rusty_ytdl::RequestOptions {
            cookies: cookie_header,
            max_retries: Some(8),
            ..Default::default()
        },
    }
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
) -> Result<u64, String> {
    let compact_source_url = compact_source_url_for_error(source_url);

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| format!("http client build failed: {e}"))?;

    let mut request = client
        .get(source_url)
        .header(reqwest::header::USER_AGENT, YOUTUBE_USER_AGENT)
        .header(reqwest::header::REFERER, "https://www.youtube.com/")
        .header(reqwest::header::ORIGIN, "https://www.youtube.com")
        .header(reqwest::header::ACCEPT, "*/*")
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(reqwest::header::RANGE, "bytes=0-");

    if let Some(cookie_value) = cookie {
        request = request.header(reqwest::header::COOKIE, cookie_value);
    }

    let mut response = request
        .send()
        .await
        .map_err(|e| format!("http request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "http status {} for source ({compact_source_url})",
            response.status(),
        ));
    }

    let mut file = tokio::fs::File::create(temp_path)
        .await
        .map_err(|e| format!("temp file create failed: {e}"))?;

    let mut downloaded_bytes: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("http response read failed: {e}"))?
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
        match download_url_to_temp_with_headers(source_url, temp_path, cookie_header).await {
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
    let mut source_downloaded = false;

    for watch_url in watch_urls {
        let attempts: [(&str, rusty_ytdl::VideoOptions); 4] = [
            (
                "highest-audio-only",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::HighestAudio,
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
            (
                "lowest-audio-only",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::LowestAudio,
                    rusty_ytdl::VideoSearchOptions::Audio,
                    cookie_header.clone(),
                ),
            ),
            (
                "lowest-muxed",
                youtube_video_options(
                    rusty_ytdl::VideoQuality::Lowest,
                    rusty_ytdl::VideoSearchOptions::VideoAudio,
                    cookie_header.clone(),
                ),
            ),
        ];

        for (label, options) in attempts {
            match download_youtube_source_to_temp_with_stream(&watch_url, options.clone(), &temp_path)
                .await
            {
                Ok(_bytes) => {
                    source_downloaded = true;
                    break;
                }
                Err(stream_reason) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    match download_youtube_source_to_temp(
                        &watch_url,
                        options,
                        &temp_path,
                        cookie_header.as_deref(),
                    )
                    .await
                    {
                        Ok(_bytes) => {
                            source_downloaded = true;
                            break;
                        }
                        Err(candidate_reason) => {
                            let stream_reason = sanitize_youtube_download_error(&stream_reason);
                            let candidate_reason =
                                sanitize_youtube_download_error(&candidate_reason);
                            last_errors.push(format!(
                                "{label} @ {}: stream={stream_reason} | direct={candidate_reason}",
                                compact_source_url_for_error(&watch_url)
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

    convert_to_mp3(&state.ffmpeg_path, &temp_path, &output_path).await?;
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
        ApiError::BadRequest("room_id must contain only letters, numbers, underscores, and dashes".into())
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
    let cache_dir = PathBuf::from(
        std::env::var("RUSTFIN_CACHE_DIR").unwrap_or_else(|_| "/cache".to_string()),
    );
    std::fs::create_dir_all(&cache_dir).context("failed to create cache dir")?;
    std::fs::create_dir_all(cache_dir.join("watch_party_audio"))
        .context("failed to create watch_party_audio cache dir")?;

    let ffmpeg_path =
        PathBuf::from(std::env::var("RUSTFIN_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string()));
    let ffprobe_path = PathBuf::from(
        std::env::var("RUSTFIN_FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string()),
    );

    let youtube_cookie = normalized_secret(std::env::var("RUSTFIN_YOUTUBE_COOKIE").ok());
    let youtube_cookie_file = normalized_secret(std::env::var("RUSTFIN_YOUTUBE_COOKIE_FILE").ok())
        .map(PathBuf::from);
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
