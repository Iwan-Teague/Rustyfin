use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use futures::SinkExt;
use rustfin_core::error::ApiError;
use tracing::{info, warn};

use crate::auth::validate_token;
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::manager::{AudioAction, PlaybackAction, RoomRuntime};
use super::permissions::{RoomPolicy, can_play_pause, can_seek};
use super::protocol::{ClientMessage, PresenceMember, QueueEntry, ServerMessage};

const MAX_WS_FRAME_BYTES: usize = 32 * 1024;
const MAX_WS_TEXT_BYTES: usize = 8 * 1024;
const AUTH_DEADLINE_SECONDS: u64 = 3;
const IDLE_TIMEOUT_SECONDS: u64 = 120;
const PING_INTERVAL_SECONDS: u64 = 20;
const MESSAGE_RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: usize = 80;
const YOUTUBE_VALIDATION_TIMEOUT_SECONDS: u64 = 6;
const YOUTUBE_WATCH_URL: &str = "https://www.youtube.com/watch";
const YOUTUBE_VALIDATION_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

static WS_CONNECT_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static WS_ALLOWED_ORIGINS: OnceLock<Vec<String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct ConnectionContext {
    room_id: String,
    item_id: String,
    user_id: String,
    room_mode: String,
    audio_library_id: Option<String>,
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

    let runtime = state
        .watch_party
        .get_or_create_runtime(
            &context.room_id,
            &context.item_id,
            &context.room_mode,
            context.audio_library_id.as_deref(),
            audio_track_ids,
            youtube_video_id,
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
                        let is_terminal = matches!(message, ServerMessage::RoomEnded);
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

    let _ = rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &context.room_id,
        &context.user_id,
        "left",
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
                // Mark the time the lobby became empty. A background sweeper ends
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
    if room.room_mode == "video" {
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
        audio_library_id: room.audio_library_id,
    })
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

async fn broadcast_current_state(
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

    let members = rustfin_db::repo::watch_party::list_members(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let usernames: HashMap<String, String> = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect();

    let member_summaries = members
        .into_iter()
        .filter(|m| m.status != "declined" && m.status != "left")
        .map(|member| PresenceMember {
            user_id: member.user_id.clone(),
            username: usernames
                .get(&member.user_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            role: member.role,
            connected: connected.contains(&member.user_id) && member.status == "joined",
        })
        .collect();

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

async fn build_audio_state_message(
    state: &AppState,
    runtime: &RoomRuntime,
    room_id: &str,
) -> Result<ServerMessage, AppError> {
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
    let current_track_id = queue_snapshot
        .track_ids
        .get(queue_snapshot.current_index)
        .cloned()
        .unwrap_or_default();

    // Fetch metadata for all tracks in one query via the library
    let audio_library_id = match runtime.audio_library_id.as_deref() {
        Some(id) => id,
        None => return Err(ApiError::Internal("audio room missing library id".into()).into()),
    };

    let all_db_tracks =
        rustfin_db::repo::watch_party::get_library_tracks(&state.db, audio_library_id, None)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let track_metadata: HashMap<String, (String, String, String, Option<String>, Option<u64>)> =
        all_db_tracks
            .into_iter()
            .map(|t| {
                (
                    t.id.clone(),
                    (t.title, t.album, t.artist, t.album_art_url, t.duration_ms),
                )
            })
            .collect();

    let (current_title, current_album, current_artist, current_art, current_dur) = track_metadata
        .get(&current_track_id)
        .cloned()
        .unwrap_or_else(|| {
            (
                "Unknown".to_string(),
                String::new(),
                String::new(),
                None,
                None,
            )
        });

    let queue: Vec<QueueEntry> = queue_snapshot
        .track_ids
        .iter()
        .map(|tid| {
            let (title, album, artist, art, dur) = track_metadata
                .get(tid)
                .cloned()
                .unwrap_or_else(|| (tid.clone(), String::new(), String::new(), None, None));
            QueueEntry {
                track_id: tid.clone(),
                title,
                artist,
                album,
                album_art_url: art,
                duration_ms: dur,
            }
        })
        .collect();

    let connected = runtime.connected_user_ids.read().await.clone();

    let members_db = rustfin_db::repo::watch_party::list_members(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let usernames: HashMap<String, String> = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect();

    let member_summaries = members_db
        .into_iter()
        .filter(|m| m.status != "declined" && m.status != "left")
        .map(|member| PresenceMember {
            user_id: member.user_id.clone(),
            username: usernames
                .get(&member.user_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            role: member.role,
            connected: connected.contains(&member.user_id) && member.status == "joined",
        })
        .collect();

    Ok(ServerMessage::AudioState {
        room_id: room_id.to_string(),
        track_id: current_track_id,
        title: current_title,
        artist: current_artist,
        album: current_album,
        album_art_url: current_art,
        duration_ms: current_dur,
        position_ms,
        playing: queue_snapshot.playing,
        updated_ts_ms,
        server_ts_ms: now_ms,
        queue,
        queue_index: queue_snapshot.current_index,
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
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let (position_ms, updated_ts_ms) = if snapshot.playing && now_ms > snapshot.updated_ts_ms {
        let elapsed_ms = (now_ms - snapshot.updated_ts_ms) as u64;
        (snapshot.position_ms.saturating_add(elapsed_ms), now_ms)
    } else {
        (snapshot.position_ms, snapshot.updated_ts_ms)
    };

    let members = rustfin_db::repo::watch_party::list_members(&state.db, room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let usernames: HashMap<String, String> = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect();

    let member_summaries = members
        .into_iter()
        .filter(|m| m.status != "declined" && m.status != "left")
        .map(|member| PresenceMember {
            user_id: member.user_id.clone(),
            username: usernames
                .get(&member.user_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            role: member.role,
            connected: connected.contains(&member.user_id) && member.status == "joined",
        })
        .collect();

    Ok(ServerMessage::YouTubeState {
        room_id: room_id.to_string(),
        video_id,
        playing: snapshot.playing,
        position_ms,
        updated_ts_ms,
        server_ts_ms: now_ms,
        queue,
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
