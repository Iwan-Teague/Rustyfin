use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::state::AppState;

use super::protocol::{ChannelEvent, ChannelInfo, MessageInfo};

// ── Request / response types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub is_private: bool,
}

#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub position: i64,
    pub is_private: bool,
    pub created_by: String,
    pub created_ts: i64,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    pub before: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    pub created_ts: i64,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn channel_to_response(row: &rustfin_db::repo::channels::ChannelRow) -> ChannelResponse {
    ChannelResponse {
        id: row.id.clone(),
        name: row.name.clone(),
        kind: row.kind.clone(),
        position: row.position,
        is_private: row.is_private,
        created_by: row.created_by.clone(),
        created_ts: row.created_ts,
    }
}

fn channel_to_info(row: &rustfin_db::repo::channels::ChannelRow) -> ChannelInfo {
    ChannelInfo {
        id: row.id.clone(),
        name: row.name.clone(),
        kind: row.kind.clone(),
        position: row.position,
        is_private: row.is_private,
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /channels
pub async fn list_channels(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ChannelResponse>>, AppError> {
    let all = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let visible: Vec<ChannelResponse> = all
        .iter()
        .filter(|c| !c.is_private || auth.role == "admin")
        .map(channel_to_response)
        .collect();

    Ok(Json(visible))
}

/// POST /channels  (admin only)
pub async fn create_channel(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), AppError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("channel name cannot be empty".into()).into());
    }
    if body.kind != "text" && body.kind != "voice" {
        return Err(ApiError::BadRequest("kind must be 'text' or 'voice'".into()).into());
    }

    // Position = current max + 1
    let all = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let position = all.iter().map(|c| c.position).max().unwrap_or(-1) + 1;

    let row = rustfin_db::repo::channels::create_channel(
        &state.db,
        body.name.trim(),
        &body.kind,
        body.is_private,
        &admin.user_id,
        position,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::ChannelCreated {
            channel: channel_to_info(&row),
        });

    Ok((StatusCode::CREATED, Json(channel_to_response(&row))))
}

/// PATCH /channels/:id  (admin only)
pub async fn update_channel(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateChannelRequest>,
) -> Result<Json<ChannelResponse>, AppError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("channel name cannot be empty".into()).into());
    }

    let existing = rustfin_db::repo::channels::get_channel(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;

    rustfin_db::repo::channels::rename_channel(&state.db, &id, &name)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let updated = rustfin_db::repo::channels::get_channel(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Internal("channel disappeared after update".into()))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::ChannelUpdated {
            channel: channel_to_info(&updated),
        });

    let _ = existing;
    Ok(Json(channel_to_response(&updated)))
}

/// DELETE /channels/:id  (admin only)
pub async fn delete_channel(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let existing = rustfin_db::repo::channels::get_channel(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;

    rustfin_db::repo::channels::delete_channel(&state.db, &existing.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::ChannelDeleted {
            channel_id: existing.id,
        });

    Ok(StatusCode::NO_CONTENT)
}

/// GET /channels/:id/messages
pub async fn get_messages(
    _auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, AppError> {
    // Ensure channel exists
    rustfin_db::repo::channels::get_channel(&state.db, &channel_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;

    let limit = params.limit.unwrap_or(50).min(200);
    let before_ts = params
        .before
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + 1);

    let messages = rustfin_db::repo::channels::list_messages(
        &state.db,
        &channel_id,
        limit,
        before_ts,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let response: Vec<MessageResponse> = messages
        .into_iter()
        .map(|m| MessageResponse {
            id: m.id,
            channel_id: m.channel_id,
            user_id: m.user_id,
            username: m.username,
            content: m.content,
            created_ts: m.created_ts,
        })
        .collect();

    Ok(Json(response))
}

/// DELETE /channels/:channel_id/messages/:message_id
pub async fn delete_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let msg = rustfin_db::repo::channels::get_message(&state.db, &message_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("message not found".into()))?;

    if msg.channel_id != channel_id {
        return Err(ApiError::NotFound("message not found".into()).into());
    }

    if msg.user_id != auth.user_id && auth.role != "admin" {
        return Err(ApiError::Forbidden("cannot delete another user's message".into()).into());
    }

    rustfin_db::repo::channels::delete_message(&state.db, &message_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::MessageDeleted {
            message_id,
            channel_id,
        });

    Ok(StatusCode::NO_CONTENT)
}

/// POST /channels/:id/messages
pub async fn send_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let content = body.content.trim().to_string();
    if content.is_empty() {
        return Err(ApiError::BadRequest("message content cannot be empty".into()).into());
    }
    if content.len() > 2000 {
        return Err(ApiError::BadRequest("message content too long (max 2000 chars)".into()).into());
    }

    // Ensure channel exists
    rustfin_db::repo::channels::get_channel(&state.db, &channel_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;

    let row = rustfin_db::repo::channels::create_message(
        &state.db,
        &channel_id,
        &auth.user_id,
        &auth.username,
        &content,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::NewMessage {
            msg: MessageInfo {
                id: row.id.clone(),
                channel_id: row.channel_id.clone(),
                user_id: row.user_id.clone(),
                username: row.username.clone(),
                content: row.content.clone(),
                created_ts: row.created_ts,
            },
        });

    Ok((
        StatusCode::CREATED,
        Json(MessageResponse {
            id: row.id,
            channel_id: row.channel_id,
            user_id: row.user_id,
            username: row.username,
            content: row.content,
            created_ts: row.created_ts,
        }),
    ))
}
