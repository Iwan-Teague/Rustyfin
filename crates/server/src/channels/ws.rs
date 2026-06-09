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
use crate::user_activity;

use super::handlers::finalizing_transcription_session_id;
use super::protocol::{ChannelEvent, ClientMsg, MessageInfo, VoiceTranscriptionStateInfo};

const MAX_WS_FRAME_BYTES: usize = 32 * 1024;
const MAX_WS_TEXT_BYTES: usize = 8 * 1024;
const PERSONAL_EVENT_BUFFER: usize = 256;
const AUTH_DEADLINE_SECONDS: u64 = 3;
const PING_INTERVAL_SECONDS: u64 = 20;
const MESSAGE_RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: usize = 300;

static WS_CONNECT_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static WS_ALLOWED_ORIGINS: OnceLock<Vec<String>> = OnceLock::new();

fn avatar_url_for_user(user_id: &str, avatar_path: Option<&str>) -> Option<String> {
    avatar_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|_| format!("/api/v1/users/avatar/{user_id}"))
}

async fn resolve_runtime_profile(
    state: &AppState,
    user_id: &str,
    fallback_username: &str,
    fallback_avatar_url: &Option<String>,
) -> (String, Option<String>) {
    match rustfin_db::repo::users::find_by_id(&state.db, user_id).await {
        Ok(Some(row)) => (
            row.display_name,
            avatar_url_for_user(user_id, row.avatar_path.as_deref()),
        ),
        _ => (fallback_username.to_string(), fallback_avatar_url.clone()),
    }
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
    let key = format!("channels-ws-connect:{}", extract_client_key(headers));
    match ws_connect_rate_limiter().check(&key).await {
        Ok(_) => Ok(()),
        Err(retry_after) => Err(ApiError::TooManyRequests {
            retry_after_seconds: retry_after,
        }
        .into()),
    }
}

async fn validate_voice_channel_access(
    state: &AppState,
    role: &str,
    channel_id: &str,
) -> Result<(), AppError> {
    let channel = rustfin_db::repo::channels::get_channel(&state.db, channel_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let Some(channel) = channel else {
        return Err(ApiError::NotFound("channel not found".into()).into());
    };
    if channel.kind != "voice" {
        return Err(
            ApiError::BadRequest("voice join is only supported for voice channels".into()).into(),
        );
    }
    if channel.is_private && role != "admin" {
        return Err(ApiError::Forbidden("channel access denied".into()).into());
    }

    Ok(())
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
        // Some browser/proxy combinations on the Debian runtime emit unmasked
        // control frames after a successful upgrade. Accept them so voice
        // channels don't churn on otherwise healthy connections.
        .accept_unmasked_frames(true)
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
    let role = claims.role.clone();
    let connection_id = uuid::Uuid::new_v4().to_string();
    let profile = match rustfin_db::repo::users::find_by_id(&state.db, &user_id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            let _ = send_error(&mut socket, "user not found").await;
            let _ = socket.close().await;
            return;
        }
        Err(err) => {
            let _ = send_error(&mut socket, &format!("db error: {err}")).await;
            let _ = socket.close().await;
            return;
        }
    };
    let username = profile.display_name;
    let avatar_url = avatar_url_for_user(&user_id, profile.avatar_path.as_deref());

    debug!(user_id = %user_id, "channels ws authenticated");
    let _ws_metrics_guard = state.runtime_metrics.track_channels_ws_connection();

    // ── Register personal mpsc channel ───────────────────────────────────────
    let (personal_tx, mut personal_rx) = mpsc::channel(PERSONAL_EVENT_BUFFER);
    state
        .channel_manager
        .register_user(&user_id, &connection_id, personal_tx)
        .await;

    // ── Subscribe to broadcast ────────────────────────────────────────────────
    let mut broadcast_rx = state.channel_manager.subscribe();

    // ── Send Hello ────────────────────────────────────────────────────────────
    let hello = build_hello(&state, &user_id, &role).await;
    if send_event(&mut socket, &hello).await.is_err() {
        state
            .channel_manager
            .unregister_user(&user_id, &connection_id)
            .await;
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
                        // Withhold private-channel events from non-admin sockets. The
                        // process-wide broadcast fans out to every socket, so private
                        // channel activity (messages, presence, transcription, channel
                        // metadata) would otherwise leak. The private-id set is kept in
                        // memory (no per-event DB query) and gates by the event's channel
                        // id. Events with no channel id (e.g. Pong) are always delivered.
                        if !is_event_visible_to_role(&state, &role, &event).await {
                            continue;
                        }
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let hello = build_hello(&state, &user_id, &role).await;
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
                        // Personal-queue events (RTC signaling, VoiceJoined) are already
                        // access-checked at their emit sites, but apply the same private
                        // gate as a uniform defense-in-depth layer.
                        if !is_event_visible_to_role(&state, &role, &event).await {
                            continue;
                        }
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

                if dispatch(
                    &state,
                    &user_id,
                    &connection_id,
                    &username,
                    &avatar_url,
                    &role,
                    &mut socket,
                    msg,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    state
        .channel_manager
        .unregister_user(&user_id, &connection_id)
        .await;

    let left_channels = state
        .channel_manager
        .leave_all_voice(&user_id, &connection_id)
        .await;
    let (runtime_username, runtime_avatar_url) =
        resolve_runtime_profile(&state, &user_id, &username, &avatar_url).await;
    for left in left_channels {
        let _ = user_activity::track_voice_leave(&state, &user_id, &left.channel_id).await;
        state
            .channel_manager
            .broadcast(ChannelEvent::VoicePresence {
                channel_id: left.channel_id,
                user_id: user_id.clone(),
                username: runtime_username.clone(),
                avatar_url: runtime_avatar_url.clone(),
                joined: false,
                active_since_ts: left.active_since_ts,
            });
    }

    let _ = socket.close().await;
}

#[allow(clippy::too_many_arguments)]
async fn dispatch(
    state: &AppState,
    user_id: &str,
    connection_id: &str,
    username: &str,
    avatar_url: &Option<String>,
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
            if let Err(err) = validate_voice_channel_access(state, role, &channel_id).await {
                let message = match &err.0 {
                    ApiError::Internal(_) => "internal error".to_string(),
                    other => other.to_string(),
                };
                let _ = send_error(socket, &message).await;
                return Ok(());
            }

            let left_channels = state
                .channel_manager
                .leave_other_voice(user_id, &channel_id)
                .await;
            let (runtime_username, runtime_avatar_url) =
                resolve_runtime_profile(state, user_id, username, avatar_url).await;

            for left in left_channels {
                let _ = user_activity::track_voice_leave(state, user_id, &left.channel_id).await;
                state
                    .channel_manager
                    .broadcast(ChannelEvent::VoicePresence {
                        channel_id: left.channel_id,
                        user_id: user_id.to_string(),
                        username: runtime_username.clone(),
                        avatar_url: runtime_avatar_url.clone(),
                        joined: false,
                        active_since_ts: left.active_since_ts,
                    });
            }

            let join_result = state
                .channel_manager
                .join_voice(
                    &channel_id,
                    user_id,
                    connection_id,
                    &runtime_username,
                    runtime_avatar_url.as_deref(),
                )
                .await;
            let _ = user_activity::track_voice_join(state, user_id, &channel_id).await;

            // Broadcast to everyone that this user joined
            state
                .channel_manager
                .broadcast(ChannelEvent::VoicePresence {
                    channel_id: channel_id.clone(),
                    user_id: user_id.to_string(),
                    username: runtime_username,
                    avatar_url: runtime_avatar_url,
                    joined: true,
                    active_since_ts: Some(join_result.active_since_ts),
                });

            // Tell the joiner who's already here
            state
                .channel_manager
                .send_to_user_connection(
                    user_id,
                    connection_id,
                    ChannelEvent::VoiceJoined {
                        channel_id,
                        existing_members: join_result.existing_members,
                    },
                )
                .await;

            Ok(())
        }

        ClientMsg::LeaveVoice { channel_id } => {
            let (runtime_username, runtime_avatar_url) =
                resolve_runtime_profile(state, user_id, username, avatar_url).await;
            let active_since_ts = state
                .channel_manager
                .leave_voice(&channel_id, user_id, connection_id)
                .await;
            let _ = user_activity::track_voice_leave(state, user_id, &channel_id).await;

            state
                .channel_manager
                .broadcast(ChannelEvent::VoicePresence {
                    channel_id,
                    user_id: user_id.to_string(),
                    username: runtime_username,
                    avatar_url: runtime_avatar_url,
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
            if !state
                .channel_manager
                .voice_channel_has_pair(&channel_id, user_id, &to_user_id)
                .await
            {
                let _ = send_error(
                    socket,
                    "rtc offer requires both users to be in the same voice channel",
                )
                .await;
                return Ok(());
            }

            let Some(target_connection_id) = state
                .channel_manager
                .voice_connection_for_user(&channel_id, &to_user_id)
                .await
            else {
                let _ = send_error(socket, "target user has no active voice connection").await;
                return Ok(());
            };

            state
                .channel_manager
                .send_to_user_connection(
                    &to_user_id,
                    &target_connection_id,
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
            if !state
                .channel_manager
                .voice_channel_has_pair(&channel_id, user_id, &to_user_id)
                .await
            {
                let _ = send_error(
                    socket,
                    "rtc answer requires both users to be in the same voice channel",
                )
                .await;
                return Ok(());
            }

            let Some(target_connection_id) = state
                .channel_manager
                .voice_connection_for_user(&channel_id, &to_user_id)
                .await
            else {
                let _ = send_error(socket, "target user has no active voice connection").await;
                return Ok(());
            };

            state
                .channel_manager
                .send_to_user_connection(
                    &to_user_id,
                    &target_connection_id,
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
            if !state
                .channel_manager
                .voice_channel_has_pair(&channel_id, user_id, &to_user_id)
                .await
            {
                let _ = send_error(
                    socket,
                    "rtc ice requires both users to be in the same voice channel",
                )
                .await;
                return Ok(());
            }

            let Some(target_connection_id) = state
                .channel_manager
                .voice_connection_for_user(&channel_id, &to_user_id)
                .await
            else {
                let _ = send_error(socket, "target user has no active voice connection").await;
                return Ok(());
            };

            state
                .channel_manager
                .send_to_user_connection(
                    &to_user_id,
                    &target_connection_id,
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
            let (runtime_username, runtime_avatar_url) =
                resolve_runtime_profile(state, user_id, username, avatar_url).await;
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
                &runtime_username,
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
                    avatar_url: runtime_avatar_url,
                    content: row.content,
                    attachments: vec![],
                    created_ts: row.created_ts,
                    sort_seq: row.sort_seq,
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

/// The single source of truth for the "private == admin-only" visibility rule,
/// matching the HTTP handler (`channels/handlers.rs::list_channels`). A channel is
/// visible to a socket when it is public, or when the socket's role is admin.
fn channel_visible_to_role(is_private: bool, role: &str) -> bool {
    !is_private || role == "admin"
}

/// Decides whether a fanned-out event may be delivered to a socket with the given
/// role. Admins see everything. Non-admins are denied any event scoped to a channel
/// currently flagged private; events not tied to a single channel are always allowed.
async fn is_event_visible_to_role(state: &AppState, role: &str, event: &ChannelEvent) -> bool {
    if role == "admin" {
        return true;
    }
    match event.channel_id() {
        Some(channel_id) => !state.channel_manager.is_channel_private(channel_id).await,
        None => true,
    }
}

async fn build_hello(state: &AppState, user_id: &str, role: &str) -> ChannelEvent {
    let all_channels = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .unwrap_or_default();

    // Refresh the manager's in-memory private-channel set from the authoritative
    // channel list. This keeps the per-socket broadcast filter accurate (it is also
    // updated incrementally on channel create/update/delete) and runs on every
    // connect, so a missed sync self-heals on the next socket.
    state
        .channel_manager
        .set_private_channels(
            all_channels
                .iter()
                .filter(|c| c.is_private)
                .map(|c| c.id.clone()),
        )
        .await;

    // Mirror the HTTP handler rule (channels/handlers.rs `list_channels`):
    // non-admins must never see private (admin-only) channels in the sidebar.
    let visible_channels: Vec<&rustfin_db::repo::channels::ChannelRow> = all_channels
        .iter()
        .filter(|c| channel_visible_to_role(c.is_private, role))
        .collect();

    // Per-user unread counts for exactly the channels this socket can see, in one query.
    // Computed after the role filter so we never count (or even reference) private channels
    // a non-admin cannot access.
    let visible_ids: Vec<String> = visible_channels.iter().map(|c| c.id.clone()).collect();
    let unread_by_channel =
        rustfin_db::repo::channels::channel_unread_counts(&state.db, user_id, &visible_ids)
            .await
            .unwrap_or_default();

    let channels = visible_channels
        .iter()
        .map(|c| super::protocol::ChannelInfo {
            id: c.id.clone(),
            name: c.name.clone(),
            kind: c.kind.clone(),
            position: c.position,
            is_private: c.is_private,
            unread_count: unread_by_channel.get(&c.id).copied().unwrap_or(0),
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
                let is_finalizing = finalizing_transcription_session_id(&session.channel_id)
                    .as_deref()
                    == Some(session.id.as_str());
                (
                    session.channel_id.clone(),
                    VoiceTranscriptionStateInfo {
                        status: if is_finalizing {
                            "finalizing".to_string()
                        } else {
                            session.status
                        },
                        session_id: Some(session.id),
                        started_by_username: Some(session.started_by_username),
                        started_ts: Some(session.started_ts),
                        ended_ts: if is_finalizing {
                            None
                        } else {
                            session.ended_ts
                        },
                        output_available: if is_finalizing {
                            false
                        } else {
                            session.output_path.is_some()
                        },
                        message: if is_finalizing {
                            Some("finalizing transcript".to_string())
                        } else {
                            session.failure_reason
                        },
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

#[cfg(test)]
mod tests {
    use super::super::protocol::ChannelInfo;
    use super::*;

    /// Mirrors the channel-list filter `build_hello` applies: a non-admin must never
    /// see private channels in their sidebar, while public channels reach everyone and
    /// admins see all. This is the TXT-1 / TXT-5 fix at the predicate level.
    #[test]
    fn build_hello_filter_excludes_private_channels_for_non_admin() {
        let channels = [
            ChannelInfo {
                id: "pub".to_string(),
                name: "general".to_string(),
                kind: "text".to_string(),
                position: 0,
                is_private: false,
                unread_count: 0,
            },
            ChannelInfo {
                id: "priv".to_string(),
                name: "admins".to_string(),
                kind: "text".to_string(),
                position: 1,
                is_private: true,
                unread_count: 0,
            },
        ];

        let visible_to_user: Vec<&str> = channels
            .iter()
            .filter(|c| channel_visible_to_role(c.is_private, "user"))
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(visible_to_user, vec!["pub"]);

        let visible_to_admin: Vec<&str> = channels
            .iter()
            .filter(|c| channel_visible_to_role(c.is_private, "admin"))
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(visible_to_admin, vec!["pub", "priv"]);
    }

    /// The per-socket broadcast filter relies on `ChannelEvent::channel_id()` to know
    /// which channel an event belongs to; channel-scoped events must expose their id
    /// and non-scoped events must return `None` (always delivered).
    #[test]
    fn channel_event_channel_id_is_exposed_for_scoped_events() {
        let new_message = ChannelEvent::NewMessage {
            msg: MessageInfo {
                id: "m-1".to_string(),
                channel_id: "ch-priv".to_string(),
                user_id: "u-1".to_string(),
                username: "alice".to_string(),
                avatar_url: None,
                content: "secret".to_string(),
                attachments: vec![],
                created_ts: 0,
                sort_seq: 0,
            },
        };
        assert_eq!(new_message.channel_id(), Some("ch-priv"));

        let presence = ChannelEvent::VoicePresence {
            channel_id: "ch-priv".to_string(),
            user_id: "u-1".to_string(),
            username: "alice".to_string(),
            avatar_url: None,
            joined: true,
            active_since_ts: None,
        };
        assert_eq!(presence.channel_id(), Some("ch-priv"));

        assert_eq!(ChannelEvent::Pong.channel_id(), None);
        assert_eq!(
            ChannelEvent::Error {
                message: "x".to_string()
            }
            .channel_id(),
            None
        );
    }
}
