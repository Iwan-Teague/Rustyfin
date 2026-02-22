use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use futures::SinkExt;
use rustfin_core::error::ApiError;
use tracing::{debug, warn};

use crate::auth::validate_token;
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::manager::{PlaybackAction, RoomRuntime};
use super::permissions::{RoomPolicy, can_play_pause, can_seek};
use super::protocol::{ClientMessage, PresenceMember, ServerMessage};

const MAX_WS_FRAME_BYTES: usize = 32 * 1024;
const MAX_WS_TEXT_BYTES: usize = 8 * 1024;
const AUTH_DEADLINE_SECONDS: u64 = 3;
const IDLE_TIMEOUT_SECONDS: u64 = 120;
const PING_INTERVAL_SECONDS: u64 = 20;
const MESSAGE_RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: usize = 80;

static WS_CONNECT_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static WS_ALLOWED_ORIGINS: OnceLock<Vec<String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct ConnectionContext {
    room_id: String,
    item_id: String,
    user_id: String,
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

    debug!(room_id = %room_id, user_id = %context.user_id, "watch party ws authenticated");

    let runtime = state
        .watch_party
        .get_or_create_runtime(&context.room_id, &context.item_id)
        .await;

    {
        let mut connected = runtime.connected_user_ids.write().await;
        connected.insert(context.user_id.clone());
    }
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
                        if send_server_message(&mut socket, &message).await.is_err() {
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

    let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;

    if claims.role != "admin" {
        let allowed =
            rustfin_db::repo::users::is_library_allowed(&state.db, &claims.sub, &item.library_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        if !allowed {
            return Err(ApiError::Forbidden("library access denied".into()).into());
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
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "play/pause is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            runtime
                .apply_action(PlaybackAction::Play { position_ms })
                .await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::Pause { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_play_pause(&role, &policy) {
                send_error(socket, "play/pause is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            runtime
                .apply_action(PlaybackAction::Pause { position_ms })
                .await;
            runtime.touch_activity().await;
            broadcast_current_state(state, runtime, &context.room_id).await?;
            Ok(())
        }
        ClientMessage::Seek { position_ms } => {
            let (room_status, policy, role) =
                refresh_membership_and_policy(state, &context.room_id, &context.user_id).await?;
            if room_status != "lobby" {
                send_error(socket, "room is not active").await?;
                return Ok(());
            }
            if !can_seek(&role, &policy) {
                send_error(socket, "seek is not allowed for this user").await?;
                send_current_state(socket, state, runtime, &context.room_id).await?;
                return Ok(());
            }

            runtime
                .apply_action(PlaybackAction::Seek { position_ms })
                .await;
            runtime.touch_activity().await;
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
    let snapshot = runtime.snapshot_state().await;
    let connected = runtime.connected_user_ids.read().await.clone();
    let now_ms = chrono::Utc::now().timestamp_millis();

    // While actively playing, project the current position forward from the last
    // authoritative update so late joiners sync close to live host time.
    let (position_ms, updated_ts_ms) = if snapshot.playing && now_ms > snapshot.updated_ts_ms {
        let elapsed_ms = (now_ms - snapshot.updated_ts_ms) as u64;
        (
            snapshot.position_ms.saturating_add(elapsed_ms),
            now_ms,
        )
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
        .filter(|m| m.status != "declined")
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
