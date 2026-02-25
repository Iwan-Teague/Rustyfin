use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use futures::SinkExt;
use rustfin_core::error::ApiError;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::auth::validate_token;
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::protocol::{ChannelEvent, ClientMsg, MessageInfo, VoiceTranscriptionStateInfo};

const MAX_WS_FRAME_BYTES: usize = 32 * 1024;
const MAX_WS_TEXT_BYTES: usize = 8 * 1024;
const AUTH_DEADLINE_SECONDS: u64 = 3;
const PING_INTERVAL_SECONDS: u64 = 20;
const MESSAGE_RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: usize = 300;

static WS_CONNECT_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

fn ws_connect_rate_limiter() -> &'static RateLimiter {
    WS_CONNECT_RATE_LIMITER.get_or_init(|| RateLimiter::new(120, 60))
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
    let key = format!("channels-ws-connect:{}", extract_client_key(headers));
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

    let allowed: Vec<String> = std::env::var("RUSTFIN_WS_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();

    if allowed.iter().any(|a| a == &origin) {
        return Ok(());
    }

    Err(ApiError::Forbidden("origin is not allowed for websocket".into()).into())
}

pub async fn ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    validate_origin(&headers)?;
    check_ws_connect_rate_limit(&headers).await?;

    Ok(ws
        .max_frame_size(MAX_WS_FRAME_BYTES)
        .max_message_size(MAX_WS_FRAME_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state)))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // ── Auth phase ────────────────────────────────────────────────────────────
    let first_msg =
        match tokio::time::timeout(Duration::from_secs(AUTH_DEADLINE_SECONDS), socket.recv()).await
        {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(err))) => {
                warn!(error = %err, "channels ws receive failed before auth");
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

    let token = match decode_client_msg(first_msg) {
        Ok(ClientMsg::Auth { token }) => token,
        Ok(_) => {
            let _ = send_error(&mut socket, "first websocket message must be auth").await;
            let _ = socket.close().await;
            return;
        }
        Err(err) => {
            let _ = send_error(&mut socket, &err.0.to_string()).await;
            let _ = socket.close().await;
            return;
        }
    };

    let claims = match validate_token(&token, &state.jwt_secret) {
        Ok(c) => c,
        Err(err) => {
            let _ = send_error(&mut socket, &err.to_string()).await;
            let _ = socket.close().await;
            return;
        }
    };

    let user_id = claims.sub.clone();
    let username = claims.username.clone();
    let role = claims.role.clone();

    debug!(user_id = %user_id, "channels ws authenticated");

    // ── Register personal mpsc channel ───────────────────────────────────────
    let (personal_tx, mut personal_rx) = mpsc::unbounded_channel();
    state
        .channel_manager
        .register_user(&user_id, personal_tx)
        .await;

    // ── Subscribe to broadcast ────────────────────────────────────────────────
    let mut broadcast_rx = state.channel_manager.subscribe();

    // ── Send Hello ────────────────────────────────────────────────────────────
    let hello = build_hello(&state).await;
    if send_event(&mut socket, &hello).await.is_err() {
        state.channel_manager.unregister_user(&user_id).await;
        let _ = socket.close().await;
        return;
    }

    // ── Main loop ─────────────────────────────────────────────────────────────
    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECONDS));
    let mut message_timestamps: VecDeque<Instant> = VecDeque::new();

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }

            outbound = broadcast_rx.recv() => {
                match outbound {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let hello = build_hello(&state).await;
                        if send_event(&mut socket, &hello).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            personal = personal_rx.recv() => {
                match personal {
                    Some(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }

            inbound = socket.recv() => {
                let inbound = match inbound {
                    Some(Ok(m)) => m,
                    Some(Err(err)) => {
                        warn!(user_id = %user_id, error = %err, "channels ws receive failed");
                        break;
                    }
                    None => break,
                };

                if !consume_message_budget(&mut message_timestamps) {
                    let _ = send_error(&mut socket, "websocket message rate limit exceeded").await;
                    break;
                }

                let msg = match decode_client_msg(inbound) {
                    Ok(m) => m,
                    Err(err) => {
                        let _ = send_error(&mut socket, &err.0.to_string()).await;
                        continue;
                    }
                };

                if dispatch(&state, &user_id, &username, &role, &mut socket, msg).await.is_err() {
                    break;
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    state.channel_manager.unregister_user(&user_id).await;

    let left_channels = state.channel_manager.leave_all_voice(&user_id).await;
    for left in left_channels {
        state
            .channel_manager
            .broadcast(ChannelEvent::VoicePresence {
                channel_id: left.channel_id,
                user_id: user_id.clone(),
                username: username.clone(),
                joined: false,
                active_since_ts: left.active_since_ts,
            });
    }

    let _ = socket.close().await;
}

async fn dispatch(
    state: &AppState,
    user_id: &str,
    username: &str,
    role: &str,
    socket: &mut WebSocket,
    msg: ClientMsg,
) -> Result<(), AppError> {
    match msg {
        ClientMsg::Auth { .. } => {
            send_error(
                socket,
                "auth message is only allowed as the first websocket message",
            )
            .await?;
            Err(ApiError::BadRequest("duplicate auth message".into()).into())
        }

        ClientMsg::JoinVoice { channel_id } => {
            let join_result = state
                .channel_manager
                .join_voice(&channel_id, user_id, username)
                .await;

            // Broadcast to everyone that this user joined
            state
                .channel_manager
                .broadcast(ChannelEvent::VoicePresence {
                    channel_id: channel_id.clone(),
                    user_id: user_id.to_string(),
                    username: username.to_string(),
                    joined: true,
                    active_since_ts: Some(join_result.active_since_ts),
                });

            // Tell the joiner who's already here
            state
                .channel_manager
                .send_to_user(
                    user_id,
                    ChannelEvent::VoiceJoined {
                        channel_id,
                        existing_members: join_result.existing_members,
                    },
                )
                .await;

            Ok(())
        }

        ClientMsg::LeaveVoice { channel_id } => {
            let active_since_ts = state
                .channel_manager
                .leave_voice(&channel_id, user_id)
                .await;

            state
                .channel_manager
                .broadcast(ChannelEvent::VoicePresence {
                    channel_id,
                    user_id: user_id.to_string(),
                    username: username.to_string(),
                    joined: false,
                    active_since_ts,
                });

            Ok(())
        }

        ClientMsg::RtcOffer {
            to_user_id,
            channel_id,
            sdp,
        } => {
            state
                .channel_manager
                .send_to_user(
                    &to_user_id,
                    ChannelEvent::RtcOffer {
                        from_user_id: user_id.to_string(),
                        channel_id,
                        sdp,
                    },
                )
                .await;
            Ok(())
        }

        ClientMsg::RtcAnswer {
            to_user_id,
            channel_id,
            sdp,
        } => {
            state
                .channel_manager
                .send_to_user(
                    &to_user_id,
                    ChannelEvent::RtcAnswer {
                        from_user_id: user_id.to_string(),
                        channel_id,
                        sdp,
                    },
                )
                .await;
            Ok(())
        }

        ClientMsg::RtcIce {
            to_user_id,
            channel_id,
            candidate,
        } => {
            state
                .channel_manager
                .send_to_user(
                    &to_user_id,
                    ChannelEvent::RtcIce {
                        from_user_id: user_id.to_string(),
                        channel_id,
                        candidate,
                    },
                )
                .await;
            Ok(())
        }

        ClientMsg::SendMessage {
            channel_id,
            content,
        } => {
            let content = content.trim().to_string();
            if content.is_empty() {
                let _ = send_error(socket, "message content cannot be empty").await;
                return Ok(());
            }
            if content.len() > 2000 {
                let _ = send_error(socket, "message content too long (max 2000 chars)").await;
                return Ok(());
            }

            // Verify channel exists
            let ch = rustfin_db::repo::channels::get_channel(&state.db, &channel_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

            let Some(ch) = ch else {
                let _ = send_error(socket, "channel not found").await;
                return Ok(());
            };
            if ch.kind != "text" {
                let _ = send_error(socket, "messages are only supported in text channels").await;
                return Ok(());
            }
            if ch.is_private && role != "admin" {
                let _ = send_error(socket, "channel access denied").await;
                return Ok(());
            }

            let row = rustfin_db::repo::channels::create_message(
                &state.db,
                &channel_id,
                user_id,
                username,
                &content,
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

            state.channel_manager.broadcast(ChannelEvent::NewMessage {
                msg: MessageInfo {
                    id: row.id,
                    channel_id: row.channel_id,
                    user_id: row.user_id,
                    username: row.username,
                    content: row.content,
                    attachments: vec![],
                    created_ts: row.created_ts,
                },
            });

            Ok(())
        }

        ClientMsg::Ping => {
            send_event(socket, &ChannelEvent::Pong).await?;
            Ok(())
        }
    }
}

async fn build_hello(state: &AppState) -> ChannelEvent {
    let channels = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .map(|c| super::protocol::ChannelInfo {
            id: c.id.clone(),
            name: c.name.clone(),
            kind: c.kind.clone(),
            position: c.position,
            is_private: c.is_private,
        })
        .collect();

    let voice_presence = state.channel_manager.voice_snapshot().await;
    let voice_active_since_ts = state.channel_manager.voice_active_since_snapshot().await;
    let voice_transcriptions =
        rustfin_db::repo::channel_transcripts::list_running_sessions(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|session| {
                (
                    session.channel_id.clone(),
                    VoiceTranscriptionStateInfo {
                        status: session.status,
                        session_id: Some(session.id),
                        started_by_username: Some(session.started_by_username),
                        started_ts: Some(session.started_ts),
                        ended_ts: session.ended_ts,
                        output_available: session.output_path.is_some(),
                        message: session.failure_reason,
                    },
                )
            })
            .collect();

    ChannelEvent::Hello {
        channels,
        voice_presence,
        voice_active_since_ts,
        voice_transcriptions,
    }
}

fn consume_message_budget(timestamps: &mut VecDeque<Instant>) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(MESSAGE_RATE_WINDOW_SECONDS);

    while let Some(front) = timestamps.front() {
        if now.duration_since(*front) > window {
            timestamps.pop_front();
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

fn decode_client_msg(message: Message) -> Result<ClientMsg, AppError> {
    match message {
        Message::Text(payload) => {
            if payload.len() > MAX_WS_TEXT_BYTES {
                return Err(
                    ApiError::BadRequest("websocket message exceeds size limit".into()).into(),
                );
            }
            serde_json::from_str::<ClientMsg>(payload.as_ref())
                .map_err(|_| ApiError::BadRequest("invalid websocket message".into()).into())
        }
        Message::Binary(_) => {
            Err(ApiError::BadRequest("binary websocket messages are not supported".into()).into())
        }
        Message::Ping(_) => Ok(ClientMsg::Ping),
        Message::Pong(_) => Ok(ClientMsg::Ping),
        Message::Close(_) => Err(ApiError::BadRequest("websocket closed".into()).into()),
    }
}

async fn send_event(socket: &mut WebSocket, event: &ChannelEvent) -> Result<(), AppError> {
    let payload = serde_json::to_string(event)
        .map_err(|e| ApiError::Internal(format!("ws serialization error: {e}")))?;
    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ApiError::Internal("failed to send websocket message".into()).into())
}

async fn send_error(socket: &mut WebSocket, msg: &str) -> Result<(), AppError> {
    send_event(
        socket,
        &ChannelEvent::Error {
            message: msg.to_string(),
        },
    )
    .await
}
