use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::path::Path as StdPath;
use tokio::fs;
use tracing::warn;

use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::state::AppState;

use super::protocol::{ChannelEvent, ChannelInfo, MessageAttachmentInfo, MessageInfo};

const MAX_MESSAGE_CHARS: usize = 2000;
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
const CHANNEL_UPLOADS_DIR: &str = "channel_uploads";

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
    #[serde(default)]
    pub attachments: Vec<MessageAttachmentResponse>,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageAttachmentResponse {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub download_path: String,
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: String,
    #[serde(default)]
    pub is_private: Option<bool>,
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

async fn get_accessible_channel(
    state: &AppState,
    auth: &AuthUser,
    channel_id: &str,
) -> Result<rustfin_db::repo::channels::ChannelRow, AppError> {
    let channel = rustfin_db::repo::channels::get_channel(&state.db, channel_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;

    if channel.is_private && auth.role != "admin" {
        return Err(ApiError::Forbidden("channel access denied".into()).into());
    }

    Ok(channel)
}

fn attachment_to_response(
    attachment: &rustfin_db::repo::channels::MessageAttachmentRow,
) -> MessageAttachmentResponse {
    MessageAttachmentResponse {
        id: attachment.id.clone(),
        filename: attachment.filename.clone(),
        content_type: attachment.content_type.clone(),
        size_bytes: attachment.size_bytes,
        download_path: format!("/api/v1/channels/attachments/{}", attachment.id),
    }
}

fn attachment_to_info(
    attachment: &rustfin_db::repo::channels::MessageAttachmentRow,
) -> MessageAttachmentInfo {
    MessageAttachmentInfo {
        id: attachment.id.clone(),
        filename: attachment.filename.clone(),
        content_type: attachment.content_type.clone(),
        size_bytes: attachment.size_bytes,
        download_path: format!("/api/v1/channels/attachments/{}", attachment.id),
    }
}

async fn list_message_attachment_responses(
    state: &AppState,
    message_id: &str,
) -> Result<Vec<MessageAttachmentResponse>, AppError> {
    let attachments = rustfin_db::repo::channels::list_message_attachments(&state.db, message_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(attachments.iter().map(attachment_to_response).collect())
}

fn sanitize_file_name(raw: &str) -> String {
    let from_path = StdPath::new(raw)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("upload.bin");
    let cleaned: String = from_path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "upload.bin".to_string()
    } else {
        trimmed
    }
}

fn sanitize_extension(raw_ext: &str) -> Option<String> {
    let cleaned: String = raw_ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn infer_content_type(filename: &str, provided: Option<&str>) -> String {
    if let Some(content_type) = provided.map(str::trim).filter(|v| !v.is_empty()) {
        return content_type.to_string();
    }

    let ext = StdPath::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "gif" => "image/gif".to_string(),
        "webp" => "image/webp".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "pdf" => "application/pdf".to_string(),
        "txt" => "text/plain".to_string(),
        "md" => "text/markdown".to_string(),
        "csv" => "text/csv".to_string(),
        "doc" => "application/msword".to_string(),
        "docx" => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string()
        }
        "xls" => "application/vnd.ms-excel".to_string(),
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        "ppt" => "application/vnd.ms-powerpoint".to_string(),
        "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string()
        }
        _ => "application/octet-stream".to_string(),
    }
}

fn sanitize_content_disposition_filename(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                if matches!(ch, '"' | '\\') { '_' } else { ch }
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim().is_empty() {
        "file".to_string()
    } else {
        sanitized
    }
}

async fn log_admin_channel_action(
    state: &AppState,
    admin_user_id: &str,
    action: &str,
    channel_id: &str,
    payload: serde_json::Value,
) {
    let payload = serde_json::json!({
        "scope": "channels",
        "action": action,
        "admin_user_id": admin_user_id,
        "channel_id": channel_id,
        "data": payload,
    });
    let payload_json = serde_json::to_string(&payload).ok();
    let Ok(job) = rustfin_db::repo::jobs::create_job(
        &state.db,
        &format!("admin.channels.{action}"),
        payload_json.as_deref(),
    )
    .await
    else {
        return;
    };
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job.id, "completed", 1.0, None).await;
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

    log_admin_channel_action(
        &state,
        &admin.user_id,
        "create",
        &row.id,
        serde_json::json!({
            "name": row.name,
            "kind": row.kind,
            "is_private": row.is_private,
        }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(channel_to_response(&row))))
}

/// PATCH /channels/:id  (admin only)
pub async fn update_channel(
    admin: AdminUser,
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

    let next_private = body.is_private.unwrap_or(existing.is_private);

    rustfin_db::repo::channels::update_channel(&state.db, &id, &name, next_private)
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

    log_admin_channel_action(
        &state,
        &admin.user_id,
        "update",
        &updated.id,
        serde_json::json!({
            "previous_name": existing.name,
            "next_name": updated.name,
            "previous_private": existing.is_private,
            "next_private": updated.is_private,
        }),
    )
    .await;

    Ok(Json(channel_to_response(&updated)))
}

/// DELETE /channels/:id  (admin only)
pub async fn delete_channel(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let existing = rustfin_db::repo::channels::get_channel(&state.db, &id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("channel not found".into()))?;
    let deleted_channel_id = existing.id.clone();

    let attachments =
        rustfin_db::repo::channels::list_channel_attachments(&state.db, &deleted_channel_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    rustfin_db::repo::channels::delete_channel(&state.db, &deleted_channel_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    for attachment in attachments {
        if let Err(err) = fs::remove_file(&attachment.storage_path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    attachment_id = %attachment.id,
                    path = %attachment.storage_path,
                    error = %err,
                    "failed deleting channel attachment file after channel delete"
                );
            }
        }
    }

    state
        .channel_manager
        .broadcast(ChannelEvent::ChannelDeleted {
            channel_id: deleted_channel_id.clone(),
        });

    log_admin_channel_action(
        &state,
        &admin.user_id,
        "delete",
        &deleted_channel_id,
        serde_json::json!({
            "name": existing.name,
            "kind": existing.kind,
            "is_private": existing.is_private,
        }),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /channels/:id/messages
pub async fn get_messages(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "text" {
        return Err(
            ApiError::BadRequest("messages are only supported in text channels".into()).into(),
        );
    }

    let limit = params.limit.unwrap_or(50).min(200);
    let before_ts = params
        .before
        .unwrap_or_else(|| chrono::Utc::now().timestamp() + 1);

    let messages =
        rustfin_db::repo::channels::list_messages(&state.db, &channel_id, limit, before_ts)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut response: Vec<MessageResponse> = Vec::with_capacity(messages.len());
    for m in messages {
        let attachments = list_message_attachment_responses(&state, &m.id).await?;
        response.push(MessageResponse {
            id: m.id,
            channel_id: m.channel_id,
            user_id: m.user_id,
            username: m.username,
            content: m.content,
            attachments,
            created_ts: m.created_ts,
        });
    }

    Ok(Json(response))
}

/// DELETE /channels/:channel_id/messages/:message_id
pub async fn delete_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((channel_id, message_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "text" {
        return Err(
            ApiError::BadRequest("messages are only supported in text channels".into()).into(),
        );
    }

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

    let attachments = rustfin_db::repo::channels::list_message_attachments(&state.db, &message_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    rustfin_db::repo::channels::delete_message(&state.db, &message_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    for attachment in attachments {
        if let Err(err) = fs::remove_file(&attachment.storage_path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    attachment_id = %attachment.id,
                    path = %attachment.storage_path,
                    error = %err,
                    "failed deleting channel attachment file after message delete"
                );
            }
        }
    }

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
    if content.len() > MAX_MESSAGE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "message content too long (max {MAX_MESSAGE_CHARS} chars)"
        ))
        .into());
    }

    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "text" {
        return Err(
            ApiError::BadRequest("messages are only supported in text channels".into()).into(),
        );
    }

    let row = rustfin_db::repo::channels::create_message(
        &state.db,
        &channel_id,
        &auth.user_id,
        &auth.username,
        &content,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state.channel_manager.broadcast(ChannelEvent::NewMessage {
        msg: MessageInfo {
            id: row.id.clone(),
            channel_id: row.channel_id.clone(),
            user_id: row.user_id.clone(),
            username: row.username.clone(),
            content: row.content.clone(),
            attachments: vec![],
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
            attachments: vec![],
            created_ts: row.created_ts,
        }),
    ))
}

/// POST /channels/:id/attachments (multipart form fields: file, optional content)
pub async fn upload_attachment_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "text" {
        return Err(
            ApiError::BadRequest("attachments are only supported in text channels".into()).into(),
        );
    }

    let mut maybe_content: Option<String> = None;
    let mut maybe_filename: Option<String> = None;
    let mut maybe_content_type: Option<String> = None;
    let mut maybe_file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid multipart form: {e}")))?
    {
        let field_name = field.name().unwrap_or_default().to_string();
        if field_name == "content" && maybe_content.is_none() {
            let text = field
                .text()
                .await
                .map_err(|e| ApiError::BadRequest(format!("invalid content field: {e}")))?;
            maybe_content = Some(text);
            continue;
        }

        if field_name == "file" && maybe_file_bytes.is_none() {
            maybe_filename = field.file_name().map(|v| v.to_string());
            maybe_content_type = field.content_type().map(|v| v.to_string());
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::BadRequest(format!("invalid file field: {e}")))?;
            if bytes.is_empty() {
                return Err(ApiError::BadRequest("uploaded file is empty".into()).into());
            }
            if bytes.len() > MAX_ATTACHMENT_BYTES {
                return Err(ApiError::BadRequest(format!(
                    "file too large (max {} MB)",
                    MAX_ATTACHMENT_BYTES / (1024 * 1024)
                ))
                .into());
            }
            maybe_file_bytes = Some(bytes.to_vec());
        }
    }

    let file_bytes = maybe_file_bytes
        .ok_or_else(|| ApiError::BadRequest("multipart form requires a file field".into()))?;
    let file_name = sanitize_file_name(maybe_filename.as_deref().unwrap_or("upload.bin"));
    let content_type = infer_content_type(&file_name, maybe_content_type.as_deref());
    let mut message_content = maybe_content.unwrap_or_default().trim().to_string();
    if message_content.len() > MAX_MESSAGE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "message content too long (max {MAX_MESSAGE_CHARS} chars)"
        ))
        .into());
    }
    if message_content.is_empty() {
        message_content = format!("Shared file: {file_name}");
    }

    let upload_dir = state.cache_dir.join(CHANNEL_UPLOADS_DIR).join(&channel_id);
    fs::create_dir_all(&upload_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to create upload directory: {e}")))?;

    let attachment_id = uuid::Uuid::new_v4().to_string();
    let ext = StdPath::new(&file_name)
        .extension()
        .and_then(|v| v.to_str())
        .and_then(sanitize_extension);
    let stored_file_name = match ext {
        Some(ext) => format!("{attachment_id}.{ext}"),
        None => attachment_id.clone(),
    };
    let stored_path = upload_dir.join(stored_file_name);

    fs::write(&stored_path, &file_bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("failed saving uploaded file: {e}")))?;

    let row = rustfin_db::repo::channels::create_message(
        &state.db,
        &channel_id,
        &auth.user_id,
        &auth.username,
        &message_content,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let attachment = match rustfin_db::repo::channels::create_message_attachment(
        &state.db,
        &row.id,
        &channel_id,
        &file_name,
        &content_type,
        file_bytes.len() as i64,
        &stored_path.to_string_lossy(),
    )
    .await
    {
        Ok(attachment) => attachment,
        Err(err) => {
            let _ = rustfin_db::repo::channels::delete_message(&state.db, &row.id).await;
            let _ = fs::remove_file(&stored_path).await;
            return Err(ApiError::Internal(format!("db error: {err}")).into());
        }
    };

    let attachment_info = attachment_to_info(&attachment);
    let attachment_response = attachment_to_response(&attachment);

    state.channel_manager.broadcast(ChannelEvent::NewMessage {
        msg: MessageInfo {
            id: row.id.clone(),
            channel_id: row.channel_id.clone(),
            user_id: row.user_id.clone(),
            username: row.username.clone(),
            content: row.content.clone(),
            attachments: vec![attachment_info],
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
            attachments: vec![attachment_response],
            created_ts: row.created_ts,
        }),
    ))
}

/// GET /channels/attachments/:attachment_id
pub async fn download_attachment(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(attachment_id): Path<String>,
) -> Result<Response, AppError> {
    let attachment = rustfin_db::repo::channels::get_message_attachment(&state.db, &attachment_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("attachment not found".into()))?;

    let _channel = get_accessible_channel(&state, &auth, &attachment.channel_id).await?;

    let bytes = fs::read(&attachment.storage_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::NotFound("attachment file not found".into())
        } else {
            ApiError::Internal(format!("failed reading attachment file: {e}"))
        }
    })?;

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();

    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    let content_type = HeaderValue::from_str(&attachment.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, content_type);

    let safe_name = sanitize_content_disposition_filename(&attachment.filename);
    let disposition_mode = if attachment.content_type.starts_with("image/")
        || attachment.content_type.starts_with("text/")
        || attachment.content_type == "application/pdf"
    {
        "inline"
    } else {
        "attachment"
    };
    if let Ok(disposition) =
        HeaderValue::from_str(&format!("{disposition_mode}; filename=\"{safe_name}\""))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    if let Ok(content_length) = HeaderValue::from_str(&attachment.size_bytes.to_string()) {
        headers.insert(header::CONTENT_LENGTH, content_length);
    }

    Ok(response)
}
