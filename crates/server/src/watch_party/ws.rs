use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use futures::SinkExt;
use rustfin_core::error::ApiError;
use tracing::{info, warn};

use crate::auth::{issue_room_track_stream_token, validate_token};
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::handlers::{normalize_web_room_url, perform_youtube_search};
use super::manager::{AudioAction, CreateState, PlaybackAction, RoomRuntime};
use super::permissions::{RoomPolicy, can_play_pause, can_seek};
use super::protocol::{
    ClientMessage, CreateCanvasStroke, PresenceMember, QueueEntry, ServerMessage,
    YouTubeSearchEntry,
};

const MAX_WS_FRAME_BYTES: usize = 256 * 1024;
const MAX_WS_TEXT_BYTES: usize = 128 * 1024;
const AUTH_DEADLINE_SECONDS: u64 = 3;
const IDLE_TIMEOUT_SECONDS: u64 = 120;
const PING_INTERVAL_SECONDS: u64 = 20;
const MESSAGE_RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: usize = 80;
const YOUTUBE_VALIDATION_TIMEOUT_SECONDS: u64 = 6;
const YOUTUBE_WATCH_URL: &str = "https://www.youtube.com/watch";
const YOUTUBE_VALIDATION_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const MAX_CREATE_TEXT_LEN: usize = 100_000;
const MAX_CREATE_DOCUMENT_NAME_LEN: usize = 120;
const MAX_CANVAS_STROKES: usize = 512;
const MAX_CANVAS_POINTS_PER_STROKE: usize = 1024;
const MAX_CANVAS_STROKE_ID_LEN: usize = 64;
const MAX_CANVAS_BYTES: usize = 30 * 1024 * 1024;

static WS_CONNECT_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static WS_ALLOWED_ORIGINS: OnceLock<Vec<String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct ConnectionContext {
    room_id: String,
    item_id: String,
    user_id: String,
    room_mode: String,
    audio_source: Option<String>,
    audio_library_id: Option<String>,
    web_url: Option<String>,
}

pub async fn ws_connect(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    validate_origin(&headers)?;
    check_ws_connect_rate_limit(&headers).await?;

    Ok(ws
        .max_frame_size(MAX_WS_FRAME_BYTES)
        .max_message_size(MAX_WS_FRAME_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state, room_id)))
}

fn ws_connect_rate_limiter() -> &'static RateLimiter {
    WS_CONNECT_RATE_LIMITER.get_or_init(|| RateLimiter::new(120, 60))
}

fn ws_allowed_origins() -> &'static Vec<String> {
    WS_ALLOWED_ORIGINS.get_or_init(|| {
        std::env::var("RUSTFIN_WS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| origin.to_ascii_lowercase())
            .collect()
    })
}

fn extract_client_key(headers: &HeaderMap) -> String {
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return format!("xff:{forwarded}");
    }

    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return format!("rip:{real_ip}");
    }

    if let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        return format!("host:{host}");
    }

    "unknown".to_string()
}

async fn check_ws_connect_rate_limit(headers: &HeaderMap) -> Result<(), AppError> {
    let key = format!("ws-connect:{}", extract_client_key(headers));
    match ws_connect_rate_limiter().check(&key).await {
        Ok(_) => Ok(()),
        Err(retry_after) => Err(ApiError::TooManyRequests {
            retry_after_seconds: retry_after,
        }
        .into()),
    }
}

fn validate_origin(headers: &HeaderMap) -> Result<(), AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase())
        .ok_or_else(|| ApiError::Forbidden("missing origin header".into()))?;

    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase())
        .ok_or_else(|| ApiError::Forbidden("missing host header".into()))?;

    let same_origin_http = format!("http://{host}");
    let same_origin_https = format!("https://{host}");
    if origin == same_origin_http || origin == same_origin_https {
        return Ok(());
    }

    if ws_allowed_origins()
        .iter()
        .any(|allowed| allowed == &origin)
    {
        return Ok(());
    }

    Err(ApiError::Forbidden("origin is not allowed for websocket".into()).into())
}

async fn handle_socket(mut socket: WebSocket, state: AppState, room_id: String) {
    let auth_result =
        tokio::time::timeout(Duration::from_secs(AUTH_DEADLINE_SECONDS), socket.recv()).await;

    let first_msg = match auth_result {
        Ok(Some(Ok(msg))) => msg,
        Ok(Some(Err(err))) => {
            warn!(room_id = %room_id, error = %err, "watch party ws receive failed before auth");
            let _ = socket.close().await;
            return;
        }
        Ok(None) => {
            let _ = socket.close().await;
            return;
        }
        Err(_) => {
            let _ = send_error(&mut socket, "authentication message timeout").await;
            let _ = socket.close().await;
            return;
        }
    };

    let first_msg = match decode_client_message(first_msg) {
        Ok(msg) => msg,
        Err(err) => {
            let _ = send_error(&mut socket, &err.0.to_string()).await;
            let _ = socket.close().await;
            return;
        }
    };

    let token = match first_msg {
        ClientMessage::Auth { token } => token,
        _ => {
            let _ = send_error(&mut socket, "first websocket message must be auth").await;
            let _ = socket.close().await;
            return;
        }
    };

    let claims = match validate_token(&token, &state.jwt_secret) {
        Ok(claims) => claims,
        Err(err) => {
            let _ = send_error(&mut socket, &err.to_string()).await;
            let _ = socket.close().await;
            return;
        }
    };

    let context = match authorize_ws_connection(&state, &room_id, &claims).await {
        Ok(ctx) => ctx,
        Err(err) => {
            let _ = send_error(&mut socket, &err.0.to_string()).await;
            let _ = socket.close().await;
            return;
        }
    };

    info!(room_id = %room_id, user_id = %context.user_id, "watch party ws authenticated");

    // Load audio queue from DB for audio rooms to pass to get_or_create_runtime
    let audio_track_ids = if context.room_mode == "audio" {
        rustfin_db::repo::watch_party::get_audio_queue(&state.db, &context.room_id)
            .await
            .ok()
            .flatten()
            .map(|(ids, _)| ids)
    } else {
        None
    };

    // Load youtube_video_id from DB for YouTube rooms
    let youtube_video_id = if context.room_mode == "youtube" {
        rustfin_db::repo::watch_party::get_room(&state.db, &context.room_id)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.youtube_video_id)
    } else {
        None
    };

    // Load persisted initial URL for web rooms.
    let web_url = if context.room_mode == "web" {
        rustfin_db::repo::watch_party::get_room(&state.db, &context.room_id)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.web_url)
    } else {
        context.web_url.clone()
    };

    let create_state = if context.room_mode == "create" {
        load_create_state_from_db(&state, &context.room_id)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let runtime = state
        .watch_party
        .get_or_create_runtime(
            &context.room_id,
            &context.item_id,
            &context.room_mode,
            context.audio_source.as_deref(),
            context.audio_library_id.as_deref(),
            audio_track_ids,
            youtube_video_id,
            web_url,
            create_state,
        )
        .await;

    {
        let mut connected = runtime.connected_user_ids.write().await;
        connected.insert(context.user_id.clone());
    }
    let _ = rustfin_db::repo::watch_party::touch_room_updated(&state.db, &context.room_id).await;
    runtime.touch_activity().await;

    let mut subscription = runtime.tx.subscribe();
    let _ = runtime.tx.send(ServerMessage::Presence {
        user_id: context.user_id.clone(),
        connected: true,
    });

    if let Err(err) = send_current_state(&mut socket, &state, &runtime, &context.room_id).await {
        warn!(
            room_id = %context.room_id,
            user_id = %context.user_id,
            error = %err.0,
            "failed to send initial watch party state"
        );
        let _ = socket.close().await;
        return;
    }

    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECONDS));
    let mut message_timestamps: VecDeque<Instant> = VecDeque::new();
    let mut last_client_activity = Instant::now();

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if last_client_activity.elapsed() > Duration::from_secs(IDLE_TIMEOUT_SECONDS) {
                    let _ = send_error(&mut socket, "websocket idle timeout").await;
                    break;
                }
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            outbound = subscription.recv() => {
                match outbound {
                    Ok(message) => {
                        let is_terminal = matches!(
                            message,
                            ServerMessage::RoomEnded | ServerMessage::RoomReconfigured { .. }
                        );
                        if send_server_message(&mut socket, &message).await.is_err() {
                            break;
                        }
                        if is_terminal {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if send_current_state(&mut socket, &state, &runtime, &context.room_id).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            inbound = socket.recv() => {
                let inbound = match inbound {
                    Some(Ok(message)) => message,
                    Some(Err(err)) => {
                        warn!(room_id = %context.room_id, user_id = %context.user_id, error = %err, "watch party ws receive failed");
                        break;
                    }
                    None => break,
                };

                if !consume_message_budget(&mut message_timestamps) {
                    let _ = send_error(&mut socket, "websocket message rate limit exceeded").await;
                    break;
                }

                last_client_activity = Instant::now();

                let client_message = match decode_client_message(inbound) {
                    Ok(msg) => msg,
                    Err(err) => {
                        let _ = send_error(&mut socket, &err.0.to_string()).await;
                        break;
                    }
                };

                if handle_client_message(&state, &runtime, &context, &mut socket, client_message).await.is_err() {
                    break;
                }
            }
        }
    }

    {
        let mut connected = runtime.connected_user_ids.write().await;
        connected.remove(&context.user_id);
    }
    let _ = runtime.tx.send(ServerMessage::Presence {
        user_id: context.user_id.clone(),
        connected: false,
    });

    let _ = rustfin_db::repo::watch_party::touch_member_last_seen(
        &state.db,
        &context.room_id,
        &context.user_id,
    )
    .await;

    let connected_count = runtime.connected_user_ids.read().await.len();
    if connected_count == 0 {
        if let Ok(Some(room)) =
            rustfin_db::repo::watch_party::get_room(&state.db, &context.room_id).await
        {
            if room.status == "ended" {
                state.watch_party.remove_runtime(&context.room_id).await;
            } else {
                // Mark when the room became empty. A background sweeper purges
                // empty rooms after 5 minutes across all watch-party modes.
                let _ =
                    rustfin_db::repo::watch_party::touch_room_updated(&state.db, &context.room_id)
                        .await;
            }
        }
    }

    let _ = socket.close().await;
}

fn consume_message_budget(timestamps: &mut VecDeque<Instant>) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(MESSAGE_RATE_WINDOW_SECONDS);

    while let Some(front) = timestamps.front() {
        if now.duration_since(*front) > window {
            let _ = timestamps.pop_front();
        } else {
            break;
        }
    }

    if timestamps.len() >= MAX_MESSAGES_PER_WINDOW {
        return false;
    }

    timestamps.push_back(now);
    true
}

fn is_valid_youtube_video_id(value: &str) -> bool {
    value.len() == 11
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

async fn youtube_embed_block_reason(video_id: &str) -> Option<String> {
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

fn decode_client_message(message: Message) -> Result<ClientMessage, AppError> {
    match message {
        Message::Text(payload) => {
            if payload.len() > MAX_WS_TEXT_BYTES {
                return Err(
                    ApiError::BadRequest("websocket message exceeds size limit".into()).into(),
                );
            }
            serde_json::from_str::<ClientMessage>(payload.as_ref())
                .map_err(|_| ApiError::BadRequest("invalid websocket message".into()).into())
        }
        Message::Binary(_) => {
            Err(ApiError::BadRequest("binary websocket messages are not supported".into()).into())
        }
        Message::Ping(_) => Ok(ClientMessage::Ping),
        Message::Pong(_) => Ok(ClientMessage::Pong),
        Message::Close(_) => Err(ApiError::BadRequest("websocket closed".into()).into()),
    }
}

async fn authorize_ws_connection(
    state: &AppState,
    room_id: &str,
    claims: &crate::auth::Claims,
) -> Result<ConnectionContext, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    // For video rooms only: verify library access
    if room.room_mode == "video" && !room.item_id.trim().is_empty() {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;

        if claims.role != "admin" {
            let allowed = rustfin_db::repo::users::is_library_allowed(
                &state.db,
                &claims.sub,
                &item.library_id,
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
            if !allowed {
                return Err(ApiError::Forbidden("library access denied".into()).into());
            }
        }
    }

    // Audio rooms may optionally have a backing local music library for offline search.
    if room.room_mode == "audio" {
        if let Some(audio_library_id) = room.audio_library_id.as_deref() {
            if claims.role != "admin" {
                let allowed = rustfin_db::repo::users::is_library_allowed(
                    &state.db,
                    &claims.sub,
                    audio_library_id,
                )
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
                if !allowed {
                    return Err(ApiError::Forbidden("library access denied".into()).into());
                }
            }
        }
    }

    let member = rustfin_db::repo::watch_party::get_member(&state.db, room_id, &claims.sub)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("room membership not found; join first".into()))?;

    if member.status != "joined" {
        return Err(ApiError::Forbidden("room membership is not joined".into()).into());
    }

    Ok(ConnectionContext {
        room_id: room.id,
        item_id: room.item_id,
        user_id: claims.sub.clone(),
        room_mode: room.room_mode,
        audio_source: Some(room.audio_source),
        audio_library_id: room.audio_library_id,
        web_url: room.web_url,
    })
}

async fn load_create_state_from_db(
    state: &AppState,
    room_id: &str,
) -> Result<Option<CreateState>, AppError> {
    let row = rustfin_db::repo::watch_party::get_create_state(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let Some(row) = row else {
        return Ok(None);
    };

    let canvas_strokes = serde_json::from_str::<Vec<CreateCanvasStroke>>(&row.canvas_strokes_json)
        .unwrap_or_default();

    Ok(Some(CreateState {
        active_tool: row.active_tool,
        document_name: row.document_name,
        text_format: row.text_format,
        text_content: row.text_content,
        canvas_strokes,
        updated_ts_ms: row.updated_ts,
    }))
}

async fn persist_create_state(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<(), AppError> {
    let snapshot = runtime
        .snapshot_create_state()
        .await
        .ok_or_else(|| ApiError::Internal("create state not initialized".into()))?;
    let canvas_strokes_json = serde_json::to_string(&snapshot.canvas_strokes)
        .map_err(|e| ApiError::Internal(format!("failed to serialize canvas state: {e}")))?;

    rustfin_db::repo::watch_party::upsert_create_state(
        &state.db,
        room_id,
        &snapshot.active_tool,
        &snapshot.document_name,
        &snapshot.text_format,
        &snapshot.text_content,
        &canvas_strokes_json,
        snapshot.updated_ts_ms,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let _ = rustfin_db::repo::watch_party::touch_room_updated(&state.db, room_id).await;
    Ok(())
}

fn normalize_create_tool(raw: &str) -> Result<String, AppError> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "text" | "canvas" => Ok(normalized),
        _ => Err(ApiError::BadRequest("tool must be either 'text' or 'canvas'".into()).into()),
    }
}

fn normalize_create_document_name(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("document_name cannot be empty".into()).into());
    }
    if trimmed.len() > MAX_CREATE_DOCUMENT_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "document_name must be <= {MAX_CREATE_DOCUMENT_NAME_LEN} characters"
        ))
        .into());
    }
    Ok(trimmed.to_string())
}

fn normalize_create_text_format(raw: &str) -> Result<&'static str, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "plain" => Ok("plain"),
        "markdown" => Ok("markdown"),
        "pdf_text" => Ok("pdf_text"),
        _ => Err(
            ApiError::BadRequest("text_format must be plain, markdown, or pdf_text".into()).into(),
        ),
    }
}

fn normalize_create_text_content(value: String) -> Result<String, AppError> {
    if value.len() > MAX_CREATE_TEXT_LEN {
        return Err(ApiError::BadRequest(format!(
            "text_content must be <= {MAX_CREATE_TEXT_LEN} bytes"
        ))
        .into());
    }
    Ok(value)
}

fn is_valid_hex_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| (*b as char).is_ascii_hexdigit())
}

fn normalize_canvas_strokes(
    strokes: Vec<CreateCanvasStroke>,
) -> Result<Vec<CreateCanvasStroke>, AppError> {
    if strokes.len() > MAX_CANVAS_STROKES {
        return Err(ApiError::BadRequest(format!(
            "too many canvas strokes; max is {MAX_CANVAS_STROKES}"
        ))
        .into());
    }

    let mut normalized = Vec::with_capacity(strokes.len());
    for mut stroke in strokes {
        if stroke.id.trim().is_empty() || stroke.id.len() > MAX_CANVAS_STROKE_ID_LEN {
            return Err(ApiError::BadRequest("invalid canvas stroke id".into()).into());
        }
        if !is_valid_hex_color(&stroke.color) {
            return Err(ApiError::BadRequest(
                "canvas stroke color must be a hex color like #ff00aa".into(),
            )
            .into());
        }
        if !(0.5..=64.0).contains(&stroke.size) {
            return Err(ApiError::BadRequest(
                "canvas stroke size must be between 0.5 and 64".into(),
            )
            .into());
        }
        if stroke.points.len() > MAX_CANVAS_POINTS_PER_STROKE {
            return Err(ApiError::BadRequest(format!(
                "canvas stroke has too many points; max is {MAX_CANVAS_POINTS_PER_STROKE}"
            ))
            .into());
        }
        if stroke
            .points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(
                ApiError::BadRequest("canvas stroke contains invalid coordinates".into()).into(),
            );
        }

        stroke.id = stroke.id.trim().to_string();
        stroke.color = stroke.color.to_ascii_lowercase();
        normalized.push(stroke);
    }

    let serialized_len = serde_json::to_vec(&normalized)
        .map_err(|e| ApiError::Internal(format!("failed to serialize canvas strokes: {e}")))?
        .len();
    if serialized_len > MAX_CANVAS_BYTES {
        return Err(ApiError::BadRequest(format!(
            "canvas payload exceeds {}MB limit",
            MAX_CANVAS_BYTES / (1024 * 1024)
        ))
        .into());
    }

    Ok(normalized)
}

async fn handle_client_message(
    state: &AppState,
    runtime: &RoomRuntime,
    context: &ConnectionContext,
    socket: &mut WebSocket,
    message: ClientMessage,
) -> Result<(), AppError> {
    match message {
        ClientMessage::Ping => {
            send_server_message(socket, &ServerMessage::Pong).await?;
            Ok(())
        }
        ClientMessage::Pong => Ok(()),
        ClientMessage::Auth { .. } => {
            send_error(
                socket,
                "auth message is only allowed as the first websocket message",
            )
            .await?;
            Err(ApiError::BadRequest("duplicate auth message".into()).into())
        }
        ClientMessage::Play { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    status = %room_status,
                    "rejecting play command: room not active"
                );
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode == "web" || context.room_mode == "create" {
                send_error(socket, "play is not valid in this room mode").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    role = %role,
                    "rejecting play command: permission denied"
                );
                send_error(socket, "play/pause is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }
            if context.room_mode == "youtube" {
                info!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    position_ms = position_ms,
                    "accepted youtube play command"
                );
            }

            if context.room_mode == "audio" {
                runtime
                    .apply_audio_action(AudioAction::SetPlayingState {
                        position_ms,
                        playing: true,
                    })
                    .await;
            } else {
                runtime
                    .apply_action(PlaybackAction::Play { position_ms })
                    .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::Pause { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    status = %room_status,
                    "rejecting pause command: room not active"
                );
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode == "web" || context.room_mode == "create" {
                send_error(socket, "pause is not valid in this room mode").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    role = %role,
                    "rejecting pause command: permission denied"
                );
                send_error(socket, "play/pause is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }
            if context.room_mode == "youtube" {
                info!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    position_ms = position_ms,
                    "accepted youtube pause command"
                );
            }

            if context.room_mode == "audio" {
                runtime
                    .apply_audio_action(AudioAction::SetPlayingState {
                        position_ms,
                        playing: false,
                    })
                    .await;
            } else {
                runtime
                    .apply_action(PlaybackAction::Pause { position_ms })
                    .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::Seek { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    status = %room_status,
                    "rejecting seek command: room not active"
                );
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode == "web" || context.room_mode == "create" {
                send_error(socket, "seek is not valid in this room mode").await?;
                return Ok(());
            }
            if !can_seek(&role, &policy) {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    role = %role,
                    "rejecting seek command: permission denied"
                );
                send_error(socket, "seek is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }
            if context.room_mode == "youtube" {
                info!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    position_ms = position_ms,
                    "accepted youtube seek command"
                );
            }

            if context.room_mode == "audio" {
                runtime
                    .apply_audio_action(AudioAction::SetPlayingState {
                        position_ms,
                        playing: runtime
                            .snapshot_audio_queue()
                            .await
                            .map(|q| q.playing)
                            .unwrap_or(false),
                    })
                    .await;
            } else {
                runtime
                    .apply_action(PlaybackAction::Seek { position_ms })
                    .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::SkipNext => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "skip is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            if let Some(new_queue) = runtime.apply_audio_action(AudioAction::SkipNext).await {
                let track_ids_json = serde_json::to_string(&new_queue.track_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                let _ = rustfin_db::repo::watch_party::upsert_audio_queue(
                    &state.db,
                    &context.room_id,
                    &track_ids_json,
                    new_queue.current_index,
                )
                .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::SkipPrev => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "skip is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            if let Some(new_queue) = runtime.apply_audio_action(AudioAction::SkipPrev).await {
                let track_ids_json = serde_json::to_string(&new_queue.track_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                let _ = rustfin_db::repo::watch_party::upsert_audio_queue(
                    &state.db,
                    &context.room_id,
                    &track_ids_json,
                    new_queue.current_index,
                )
                .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::PlayTrack { track_id } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "play track is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            if let Some(new_queue) = runtime
                .apply_audio_action(AudioAction::PlayTrack { track_id })
                .await
            {
                let track_ids_json = serde_json::to_string(&new_queue.track_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                let _ = rustfin_db::repo::watch_party::upsert_audio_queue(
                    &state.db,
                    &context.room_id,
                    &track_ids_json,
                    new_queue.current_index,
                )
                .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::TrackEnded { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode != "audio" {
                send_error(socket, "track_ended is only valid in audio rooms").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "track end handling is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            if let Some(new_queue) = runtime.handle_audio_track_ended(position_ms).await {
                let track_ids_json = serde_json::to_string(&new_queue.track_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                let _ = rustfin_db::repo::watch_party::upsert_audio_queue(
                    &state.db,
                    &context.room_id,
                    &track_ids_json,
                    new_queue.current_index,
                )
                .await;
            }
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::ReorderAudioQueue {
            from_index,
            to_index,
        } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode != "audio" {
                send_error(socket, "reorder_audio_queue is only valid in audio rooms").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "reordering queue is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let Some(new_queue) = runtime.reorder_audio_queue(from_index, to_index).await else {
                send_error(socket, "invalid queue indexes").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };

            let track_ids_json =
                serde_json::to_string(&new_queue.track_ids).unwrap_or_else(|_| "[]".to_string());
            let _ = rustfin_db::repo::watch_party::upsert_audio_queue(
                &state.db,
                &context.room_id,
                &track_ids_json,
                new_queue.current_index,
            )
            .await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::SetAudioShuffle { enabled } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode != "audio" {
                send_error(socket, "set_audio_shuffle is only valid in audio rooms").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "shuffle control is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            runtime.set_audio_shuffle_enabled(enabled).await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::SetAudioRepeatMode { mode } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if context.room_mode != "audio" {
                send_error(socket, "set_audio_repeat_mode is only valid in audio rooms").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "repeat control is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            runtime.set_audio_repeat_mode(mode).await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::ChangeVideo { video_id } => {
            let video_id = video_id.trim().to_string();
            if context.room_mode != "youtube" {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    room_mode = %context.room_mode,
                    "rejecting change_video: invalid room mode"
                );
                send_error(socket, "change_video is only valid in YouTube rooms").await?;
                return Ok(());
            }
            if !is_valid_youtube_video_id(&video_id) {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    raw_video_id = %video_id,
                    "rejecting change_video: invalid video id format"
                );
                send_error(socket, "video_id must be a valid 11-character YouTube ID").await?;
                return Ok(());
            }
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    status = %room_status,
                    "rejecting change_video: room not active"
                );
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    role = %role,
                    "rejecting change_video: permission denied"
                );
                send_error(socket, "changing video is not allowed for this user").await?;
                return Ok(());
            }

            if let Some(reason) = youtube_embed_block_reason(&video_id).await {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    video_id = %video_id,
                    reason = %reason,
                    "rejecting change_video: video is not embeddable"
                );
                send_error(
                    socket,
                    &format!("YouTube rejected this video for embed playback: {reason}"),
                )
                .await?;
                return Ok(());
            }

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                video_id = %video_id,
                "accepted youtube change_video command"
            );

            runtime.set_youtube_video_id(video_id.clone()).await;
            let _ = rustfin_db::repo::watch_party::update_youtube_video_id(
                &state.db,
                &context.room_id,
                &video_id,
            )
            .await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::QueueVideo { video_id } => {
            let video_id = video_id.trim().to_string();
            if context.room_mode != "youtube" {
                send_error(socket, "queue_video is only valid in YouTube rooms").await?;
                return Ok(());
            }
            if !is_valid_youtube_video_id(&video_id) {
                send_error(socket, "video_id must be a valid 11-character YouTube ID").await?;
                return Ok(());
            }

            let (room_status, _policy, _role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }

            if let Some(reason) = youtube_embed_block_reason(&video_id).await {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    video_id = %video_id,
                    reason = %reason,
                    "rejecting queue_video: video is not embeddable"
                );
                send_error(
                    socket,
                    &format!("YouTube rejected this queued video for embed playback: {reason}"),
                )
                .await?;
                return Ok(());
            }

            let Some(queue) = runtime.enqueue_youtube_video_unique(video_id.clone()).await else {
                send_error(
                    socket,
                    "video is already queued or currently playing in this room",
                )
                .await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };
            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                video_id = %video_id,
                queue_len = queue.len(),
                "accepted youtube queue_video command"
            );
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::AdvanceQueue { expected_video_id } => {
            if context.room_mode != "youtube" {
                send_error(socket, "advance_queue is only valid in YouTube rooms").await?;
                return Ok(());
            }
            let expected_video_id = expected_video_id.trim().to_string();
            if !expected_video_id.is_empty() && !is_valid_youtube_video_id(&expected_video_id) {
                send_error(
                    socket,
                    "expected_video_id must be a valid 11-character YouTube ID",
                )
                .await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "advance queue is not allowed for this user").await?;
                return Ok(());
            }

            // Drop non-embeddable queued videos so auto-advance lands on something playable.
            for _ in 0..5 {
                let Some(next_video_id) = runtime.youtube_queue_video_at(0).await else {
                    break;
                };
                let Some(reason) = youtube_embed_block_reason(&next_video_id).await else {
                    break;
                };
                let _ = runtime.remove_youtube_queue_index(0).await;
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    video_id = %next_video_id,
                    reason = %reason,
                    "dropped non-embeddable video while advancing queue"
                );
            }

            let advanced = runtime.advance_youtube_queue(&expected_video_id).await;
            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                expected_video_id = %expected_video_id,
                advanced = advanced,
                "processed youtube advance_queue command"
            );
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::PlayQueuedVideo { queue_index } => {
            if context.room_mode != "youtube" {
                send_error(socket, "play_queued_video is only valid in YouTube rooms").await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "playing queued videos is not allowed for this user").await?;
                return Ok(());
            }

            let Some(queued_video_id) = runtime.youtube_queue_video_at(queue_index).await else {
                send_error(socket, "invalid queue index").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };

            if let Some(reason) = youtube_embed_block_reason(&queued_video_id).await {
                warn!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    queue_index = queue_index,
                    video_id = %queued_video_id,
                    reason = %reason,
                    "rejecting play_queued_video: video is not embeddable"
                );
                send_error(
                    socket,
                    &format!("Queued YouTube video cannot be embedded: {reason}"),
                )
                .await?;
                return Ok(());
            }

            let Some(video_id) = runtime.play_youtube_queue_index_now(queue_index).await else {
                send_error(socket, "invalid queue index").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };

            let _ = rustfin_db::repo::watch_party::update_youtube_video_id(
                &state.db,
                &context.room_id,
                &video_id,
            )
            .await;

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                queue_index = queue_index,
                video_id = %video_id,
                "accepted youtube play_queued_video command"
            );
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::RemoveQueuedVideo { queue_index } => {
            if context.room_mode != "youtube" {
                send_error(socket, "remove_queued_video is only valid in YouTube rooms").await?;
                return Ok(());
            }

            let (room_status, _policy, _role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }

            let Some(queue) = runtime.remove_youtube_queue_index(queue_index).await else {
                send_error(socket, "invalid queue index").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                queue_index = queue_index,
                queue_len = queue.len(),
                "accepted youtube remove_queued_video command"
            );
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::MoveQueuedVideo {
            from_index,
            to_index,
        } => {
            if context.room_mode != "youtube" {
                send_error(socket, "move_queued_video is only valid in YouTube rooms").await?;
                return Ok(());
            }

            let (room_status, _policy, _role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }

            let Some(queue) = runtime.move_youtube_queue_item(from_index, to_index).await else {
                send_error(socket, "invalid queue indexes").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            };

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                from_index = from_index,
                to_index = to_index,
                queue_len = queue.len(),
                "accepted youtube move_queued_video command"
            );
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::ChangeWebUrl { url } => {
            if context.room_mode != "web" {
                send_error(socket, "change_web_url is only valid in web rooms").await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "changing web URL is not allowed for this user").await?;
                return Ok(());
            }

            let normalized_url = normalize_web_room_url(&url)?;
            runtime.set_web_url(normalized_url.clone()).await;
            let _ = rustfin_db::repo::watch_party::update_web_url(
                &state.db,
                &context.room_id,
                &normalized_url,
            )
            .await;

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                url = %normalized_url,
                "accepted web change_web_url command"
            );

            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::CreateSetTool { tool } => {
            if context.room_mode != "create" {
                send_error(
                    socket,
                    "create_set_tool is only valid in create-together rooms",
                )
                .await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "editing is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let normalized_tool = normalize_create_tool(&tool)?;
            let _ = runtime.set_create_tool(normalized_tool).await;
            persist_create_state(state, runtime, &context.room_id).await?;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::CreateSetDocumentName { document_name } => {
            if context.room_mode != "create" {
                send_error(
                    socket,
                    "create_set_document_name is only valid in create-together rooms",
                )
                .await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "editing is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let normalized_name = normalize_create_document_name(&document_name)?;
            let _ = runtime.set_create_document_name(normalized_name).await;
            persist_create_state(state, runtime, &context.room_id).await?;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::CreateSetText {
            text_content,
            text_format,
        } => {
            if context.room_mode != "create" {
                send_error(
                    socket,
                    "create_set_text is only valid in create-together rooms",
                )
                .await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "editing is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let normalized_text = normalize_create_text_content(text_content)?;
            let normalized_format = text_format
                .as_deref()
                .map(normalize_create_text_format)
                .transpose()?
                .map(str::to_string);

            let _ = runtime
                .set_create_text(normalized_text, normalized_format)
                .await;
            persist_create_state(state, runtime, &context.room_id).await?;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::CreateSetCanvas { canvas_strokes } => {
            if context.room_mode != "create" {
                send_error(
                    socket,
                    "create_set_canvas is only valid in create-together rooms",
                )
                .await?;
                return Ok(());
            }

            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "editing is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let normalized_canvas = normalize_canvas_strokes(canvas_strokes)?;
            let _ = runtime.set_create_canvas(normalized_canvas).await;
            persist_create_state(state, runtime, &context.room_id).await?;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::SearchYouTube { query } => {
            if context.room_mode != "youtube" {
                send_error(socket, "search_youtube is only valid in YouTube rooms").await?;
                return Ok(());
            }

            let (room_status, _policy, _role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }

            let trimmed_query = query.trim();
            if trimmed_query.is_empty() {
                runtime
                    .set_youtube_search_state(String::new(), Vec::new())
                    .await;
                info!(
                    room_id = %context.room_id,
                    user_id = %context.user_id,
                    "cleared youtube shared search state"
                );
                broadcast_current_state(state, runtime, &context.room_id).await?;
                return Ok(());
            }

            let (search_query, search_results) =
                match perform_youtube_search(&query, Some(12)).await {
                    Ok((search_query, search_results)) => (search_query, search_results),
                    Err(err) => {
                        send_error(socket, &err.0.to_string()).await?;
                        return Ok(());
                    }
                };

            let shared_results: Vec<YouTubeSearchEntry> = search_results
                .into_iter()
                .map(|entry| YouTubeSearchEntry {
                    video_id: entry.video_id,
                    title: entry.title,
                    channel: entry.channel,
                    thumbnail_url: entry.thumbnail_url,
                    view_count: entry.view_count,
                })
                .collect();

            runtime
                .set_youtube_search_state(search_query.clone(), shared_results.clone())
                .await;

            info!(
                room_id = %context.room_id,
                user_id = %context.user_id,
                query = %search_query,
                results = shared_results.len(),
                "accepted youtube search_youtube command"
            );

            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
    }
}

async fn refresh_membership_and_policy(
    state: &AppState,
    room_id: &str,
    user_id: &str,
) -> Result<(String, RoomPolicy, String), AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    let member = rustfin_db::repo::watch_party::get_member(&state.db, room_id, user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("room membership not found".into()))?;

    if member.status != "joined" {
        return Err(ApiError::Forbidden("room membership is not joined".into()).into());
    }

    let policy: RoomPolicy = serde_json::from_str(&room.policy_json)
        .map_err(|e| ApiError::Internal(format!("invalid room policy JSON: {e}")))?;

    Ok((room.status, policy, member.role))
}

pub(crate) async fn broadcast_current_state(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<(), AppError> {
    let message = build_state_message(state, runtime, room_id).await?;
    let _ = runtime.tx.send(message);
    Ok(())
}

async fn send_current_state(
    socket: &mut WebSocket,
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<(), AppError> {
    let message = build_state_message(state, runtime, room_id).await?;
    send_server_message(socket, &message).await
}

async fn build_presence_members(
    state: &AppState,
    room_id: &str,
    connected: &HashSet<String>,
) -> Result<Vec<PresenceMember>, AppError> {
    let members = rustfin_db::repo::watch_party::list_members_with_usernames(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(members
        .into_iter()
        .filter(|member| member.status != "declined" && member.status != "left")
        .map(|member| PresenceMember {
            connected: connected.contains(&member.user_id) && member.status == "joined",
            user_id: member.user_id,
            username: member.username,
            role: member.role,
        })
        .collect())
}

async fn build_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
    if runtime.room_mode == "audio" {
        return build_audio_state_message(state, runtime, room_id).await;
    }
    if runtime.room_mode == "youtube" {
        return build_youtube_state_message(state, runtime, room_id).await;
    }
    if runtime.room_mode == "web" {
        return build_web_state_message(state, runtime, room_id).await;
    }
    if runtime.room_mode == "create" {
        return build_create_state_message(state, runtime, room_id).await;
    }

    let snapshot = runtime.snapshot_state().await;
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    // While actively playing, project the current position forward from the last
    // authoritative update so late joiners sync close to live host time.
    let (position_ms, updated_ts_ms) = if snapshot.playing && now_ms > snapshot.updated_ts_ms {
        let elapsed_ms = (now_ms - snapshot.updated_ts_ms) as u64;
        (snapshot.position_ms.saturating_add(elapsed_ms), now_ms)
    } else {
        (snapshot.position_ms, snapshot.updated_ts_ms)
    };

    let member_summaries = build_presence_members(state, room_id, &connected).await?;

    Ok(ServerMessage::State {
        room_id: room_id.to_string(),
        item_id: runtime.item_id.clone(),
        playing: snapshot.playing,
        position_ms,
        updated_ts_ms,
        server_ts_ms: now_ms,
        members: member_summaries,
    })
}

async fn build_web_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
    let (url, updated_ts_ms) = runtime.snapshot_web_state().await;
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let member_summaries = build_presence_members(state, room_id, &connected).await?;

    Ok(ServerMessage::WebState {
        room_id: room_id.to_string(),
        url,
        updated_ts_ms,
        server_ts_ms: now_ms,
        members: member_summaries,
    })
}

async fn build_create_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
    let create_state = runtime
        .snapshot_create_state()
        .await
        .ok_or_else(|| ApiError::Internal("create state not initialized".into()))?;
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let member_summaries = build_presence_members(state, room_id, &connected).await?;

    Ok(ServerMessage::CreateState {
        room_id: room_id.to_string(),
        active_tool: create_state.active_tool,
        document_name: create_state.document_name,
        text_format: create_state.text_format,
        text_content: create_state.text_content,
        canvas_strokes: create_state.canvas_strokes,
        updated_ts_ms: create_state.updated_ts_ms,
        server_ts_ms: now_ms,
        members: member_summaries,
    })
}

async fn build_audio_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
    const AUDIO_QUEUE_LOCAL_PREFIX: &str = "local:";
    const AUDIO_QUEUE_ONLINE_PREFIX: &str = "online:";

    type LocalTrackMetadata = (
        String,         // title
        String,         // album
        String,         // artist
        Option<String>, // album_art_url
        Option<u64>,    // duration_ms
    );
    type OnlineTrackMetadata = (
        String,         // title
        String,         // channel
        Option<String>, // thumbnail_url
        Option<u64>,    // duration_ms
        String,         // video_id
    );

    enum QueueTrackRef {
        Local { item_id: String },
        Online { track_id: String },
    }

    let queue_snapshot = match runtime.snapshot_audio_queue().await {
        Some(q) => q,
        None => {
            return Err(ApiError::Internal("audio queue not initialized".into()).into());
        }
    };

    let now_ms = chrono::Utc::now().timestamp_millis();

    let (position_ms, updated_ts_ms) =
        if queue_snapshot.playing && now_ms > queue_snapshot.updated_ts_ms {
            let elapsed_ms = (now_ms - queue_snapshot.updated_ts_ms) as u64;
            (
                queue_snapshot.position_ms.saturating_add(elapsed_ms),
                now_ms,
            )
        } else {
            (queue_snapshot.position_ms, queue_snapshot.updated_ts_ms)
        };

    // Get current track ID
    let current_queue_track_id = queue_snapshot
        .track_ids
        .get(queue_snapshot.current_index)
        .cloned()
        .unwrap_or_default();
    let audio_source = runtime
        .audio_source
        .as_deref()
        .unwrap_or("online")
        .to_string();

    let mut local_item_ids: Vec<String> = Vec::new();
    let mut online_track_ids: Vec<String> = Vec::new();
    let mut legacy_track_ids: Vec<String> = Vec::new();
    let mut local_seen: HashSet<String> = HashSet::new();
    let mut online_seen: HashSet<String> = HashSet::new();
    let mut legacy_seen: HashSet<String> = HashSet::new();

    for raw_track_id in &queue_snapshot.track_ids {
        if let Some(item_id) = raw_track_id.strip_prefix(AUDIO_QUEUE_LOCAL_PREFIX) {
            if !item_id.is_empty() && local_seen.insert(item_id.to_string()) {
                local_item_ids.push(item_id.to_string());
            }
            continue;
        }
        if let Some(track_id) = raw_track_id.strip_prefix(AUDIO_QUEUE_ONLINE_PREFIX) {
            if !track_id.is_empty() && online_seen.insert(track_id.to_string()) {
                online_track_ids.push(track_id.to_string());
            }
            continue;
        }
        if !raw_track_id.is_empty() && legacy_seen.insert(raw_track_id.clone()) {
            legacy_track_ids.push(raw_track_id.clone());
        }
    }

    for legacy_track_id in &legacy_track_ids {
        if online_seen.insert(legacy_track_id.clone()) {
            online_track_ids.push(legacy_track_id.clone());
        }
    }

    let online_track_metadata: HashMap<String, OnlineTrackMetadata> =
        rustfin_db::repo::watch_party::list_online_audio_tracks_by_ids(
            &state.db,
            room_id,
            &online_track_ids,
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|track| {
            (
                track.id,
                (
                    track.title,
                    track.channel,
                    track.thumbnail_url,
                    track.duration_ms,
                    track.video_id,
                ),
            )
        })
        .collect();

    for legacy_track_id in &legacy_track_ids {
        if !online_track_metadata.contains_key(legacy_track_id)
            && local_seen.insert(legacy_track_id.clone())
        {
            local_item_ids.push(legacy_track_id.clone());
        }
    }

    let local_track_metadata: HashMap<String, LocalTrackMetadata> =
        if let Some(audio_library_id) = runtime.audio_library_id.as_deref() {
            rustfin_db::repo::watch_party::get_library_tracks_by_item_ids(
                &state.db,
                audio_library_id,
                &local_item_ids,
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .map(|track| {
                (
                    track.id,
                    (
                        track.title,
                        track.album,
                        track.artist,
                        track.album_art_url,
                        track.duration_ms,
                    ),
                )
            })
            .collect()
        } else {
            HashMap::new()
        };

    let resolve_queue_track_ref = |raw_id: &str| -> QueueTrackRef {
        if let Some(item_id) = raw_id.strip_prefix(AUDIO_QUEUE_LOCAL_PREFIX) {
            return QueueTrackRef::Local {
                item_id: item_id.to_string(),
            };
        }
        if let Some(track_id) = raw_id.strip_prefix(AUDIO_QUEUE_ONLINE_PREFIX) {
            return QueueTrackRef::Online {
                track_id: track_id.to_string(),
            };
        }
        if online_track_metadata.contains_key(raw_id) {
            return QueueTrackRef::Online {
                track_id: raw_id.to_string(),
            };
        }
        QueueTrackRef::Local {
            item_id: raw_id.to_string(),
        }
    };

    let current_track_ref = if current_queue_track_id.is_empty() {
        None
    } else {
        Some(resolve_queue_track_ref(&current_queue_track_id))
    };

    let (current_title, current_album, current_artist, current_art, current_dur) =
        match current_track_ref.as_ref() {
            Some(QueueTrackRef::Local { item_id }) => local_track_metadata
                .get(item_id)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        "Unknown".to_string(),
                        String::new(),
                        String::new(),
                        None,
                        None,
                    )
                }),
            Some(QueueTrackRef::Online { track_id }) => online_track_metadata
                .get(track_id)
                .map(|(title, channel, thumbnail, duration_ms, _)| {
                    (
                        title.clone(),
                        "YouTube".to_string(),
                        channel.clone(),
                        thumbnail.clone(),
                        *duration_ms,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        "Unknown".to_string(),
                        String::new(),
                        String::new(),
                        None,
                        None,
                    )
                }),
            None => (
                "Unknown".to_string(),
                String::new(),
                String::new(),
                None,
                None,
            ),
        };

    let stream_url = match current_track_ref.as_ref() {
        Some(QueueTrackRef::Online { track_id }) => {
            let token =
                issue_room_track_stream_token(room_id, track_id, 60 * 60, &state.jwt_secret)
                    .map_err(|_| {
                        ApiError::Internal("failed to issue room-track stream token".to_string())
                    })?;
            Some(format!(
                "/api/v1/watch-party/rooms/{room_id}/audio/online/tracks/{track_id}/stream?st={token}"
            ))
        }
        _ => None,
    };

    let queue: Vec<QueueEntry> = queue_snapshot
        .track_ids
        .iter()
        .map(|raw_track_id| match resolve_queue_track_ref(raw_track_id) {
            QueueTrackRef::Local { item_id } => {
                let (title, album, artist, art, dur) = local_track_metadata
                    .get(&item_id)
                    .cloned()
                    .unwrap_or_else(|| (item_id.clone(), String::new(), String::new(), None, None));
                QueueEntry {
                    track_id: raw_track_id.clone(),
                    title,
                    artist,
                    album,
                    album_art_url: art,
                    video_id: None,
                    duration_ms: dur,
                }
            }
            QueueTrackRef::Online { track_id } => {
                let (title, channel, thumbnail, duration_ms, video_id) = online_track_metadata
                    .get(&track_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        (track_id.clone(), String::new(), None, None, String::new())
                    });
                QueueEntry {
                    track_id: raw_track_id.clone(),
                    title,
                    artist: channel,
                    album: "YouTube".to_string(),
                    album_art_url: thumbnail,
                    video_id: if video_id.is_empty() {
                        None
                    } else {
                        Some(video_id)
                    },
                    duration_ms,
                }
            }
        })
        .collect();

    let connected = runtime.connected_user_ids.read().await.clone();

    let member_summaries = build_presence_members(state, room_id, &connected).await?;

    Ok(ServerMessage::AudioState {
        room_id: room_id.to_string(),
        audio_source,
        track_id: current_queue_track_id,
        title: current_title,
        artist: current_artist,
        album: current_album,
        album_art_url: current_art,
        stream_url,
        duration_ms: current_dur,
        position_ms,
        playing: queue_snapshot.playing,
        updated_ts_ms,
        server_ts_ms: now_ms,
        queue,
        queue_index: queue_snapshot.current_index,
        shuffle_enabled: queue_snapshot.shuffle_enabled,
        repeat_mode: queue_snapshot.repeat_mode,
        members: member_summaries,
    })
}

async fn build_youtube_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
    let snapshot = runtime.snapshot_state().await;
    let video_id = runtime.get_youtube_video_id().await.unwrap_or_default();
    let queue = runtime.snapshot_youtube_queue().await;
    let (search_query, search_results) = runtime.snapshot_youtube_search().await;
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let (position_ms, updated_ts_ms) = if snapshot.playing && now_ms > snapshot.updated_ts_ms {
        let elapsed_ms = (now_ms - snapshot.updated_ts_ms) as u64;
        (snapshot.position_ms.saturating_add(elapsed_ms), now_ms)
    } else {
        (snapshot.position_ms, snapshot.updated_ts_ms)
    };

    let member_summaries = build_presence_members(state, room_id, &connected).await?;

    Ok(ServerMessage::YouTubeState {
        room_id: room_id.to_string(),
        video_id,
        playing: snapshot.playing,
        position_ms,
        updated_ts_ms,
        server_ts_ms: now_ms,
        queue,
        search_query,
        search_results,
        members: member_summaries,
    })
}

async fn send_server_message(
    socket: &mut WebSocket,
    message: &ServerMessage,
) -> Result<(), AppError> {
    let payload = serde_json::to_string(message)
        .map_err(|e| ApiError::Internal(format!("ws serialization error: {e}")))?;

    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ApiError::Internal("failed to send websocket message".into()).into())
}

async fn send_error(socket: &mut WebSocket, msg: &str) -> Result<(), AppError> {
    send_server_message(
        socket,
        &ServerMessage::Error {
            message: msg.to_string(),
        },
    )
    .await
}
