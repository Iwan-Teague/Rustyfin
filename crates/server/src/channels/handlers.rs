use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use chrono::{DateTime, Utc};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tracing::warn;

use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;
use crate::transcription_agent::{self, AgentTranscribeChunkRequest};

use super::protocol::{
    ChannelEvent, ChannelInfo, MessageAttachmentInfo, MessageInfo, VoiceTranscriptionStateInfo,
};

const MAX_MESSAGE_CHARS: usize = 2000;
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
const CHANNEL_UPLOADS_DIR: &str = "channel_uploads";
const TRANSCRIPTION_FINALIZE_MIN_GRACE_MS: u64 = 900;
const TRANSCRIPTION_FINALIZE_QUIET_WINDOW_MS: u64 = 250;
const TRANSCRIPTION_FINALIZE_MAX_WAIT_MS: u64 = 25_000;

fn text_message_rate_limiter() -> &'static RateLimiter {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| RateLimiter::new(120, 60))
}

fn attachment_upload_rate_limiter() -> &'static RateLimiter {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| RateLimiter::new(12, 60))
}

fn transcription_in_flight_chunks() -> &'static Mutex<HashMap<String, usize>> {
    static COUNTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mark_transcription_chunk_started(session_id: &str) {
    if let Ok(mut guard) = transcription_in_flight_chunks().lock() {
        let counter = guard.entry(session_id.to_string()).or_insert(0);
        *counter += 1;
    }
}

fn mark_transcription_chunk_finished(session_id: &str) {
    if let Ok(mut guard) = transcription_in_flight_chunks().lock()
        && let Some(counter) = guard.get_mut(session_id)
    {
        if *counter <= 1 {
            guard.remove(session_id);
        } else {
            *counter -= 1;
        }
    }
}

fn transcription_in_flight_count(session_id: &str) -> usize {
    transcription_in_flight_chunks()
        .lock()
        .ok()
        .and_then(|guard| guard.get(session_id).copied())
        .unwrap_or(0)
}

struct TranscriptionChunkInFlightGuard {
    session_id: String,
}

impl TranscriptionChunkInFlightGuard {
    fn new(session_id: &str) -> Self {
        mark_transcription_chunk_started(session_id);
        Self {
            session_id: session_id.to_string(),
        }
    }
}

impl Drop for TranscriptionChunkInFlightGuard {
    fn drop(&mut self) {
        mark_transcription_chunk_finished(&self.session_id);
    }
}

async fn wait_for_transcription_chunks_to_settle(session_id: &str) {
    let started_at = Instant::now();
    let min_grace = Duration::from_millis(TRANSCRIPTION_FINALIZE_MIN_GRACE_MS);
    let quiet_window = Duration::from_millis(TRANSCRIPTION_FINALIZE_QUIET_WINDOW_MS);
    let max_wait = Duration::from_millis(TRANSCRIPTION_FINALIZE_MAX_WAIT_MS);
    let mut last_non_zero_at: Option<Instant> = None;

    loop {
        let in_flight = transcription_in_flight_count(session_id);
        if in_flight > 0 {
            last_non_zero_at = Some(Instant::now());
        }

        let grace_elapsed = started_at.elapsed() >= min_grace;
        let quiet_elapsed = last_non_zero_at
            .map(|at| at.elapsed() >= quiet_window)
            .unwrap_or(grace_elapsed);
        if grace_elapsed && in_flight == 0 && quiet_elapsed {
            break;
        }

        if started_at.elapsed() >= max_wait {
            warn!(
                session_id = %session_id,
                in_flight,
                waited_ms = started_at.elapsed().as_millis() as u64,
                "timed out waiting for in-flight transcription chunks; finalizing transcript anyway"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

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
    pub before_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub id: String,
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
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

#[derive(Debug, Clone)]
struct SenderProfile {
    display_name: String,
    avatar_url: Option<String>,
}

fn avatar_url_for_user(user_id: &str, avatar_path: Option<&str>) -> Option<String> {
    avatar_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|_| format!("/api/v1/users/avatar/{user_id}"))
}

async fn resolve_sender_profile(
    state: &AppState,
    auth: &AuthUser,
) -> Result<SenderProfile, AppError> {
    let Some(user) = rustfin_db::repo::users::find_by_id(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Ok(SenderProfile {
            display_name: auth.username.clone(),
            avatar_url: None,
        });
    };
    Ok(SenderProfile {
        display_name: user.display_name,
        avatar_url: avatar_url_for_user(&auth.user_id, user.avatar_path.as_deref()),
    })
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

async fn enforce_channel_rate_limit(
    limiter: &RateLimiter,
    action: &str,
    auth: &AuthUser,
    channel_id: &str,
) -> Result<(), AppError> {
    let key = format!("channels:{action}:{channel_id}:{}", auth.user_id);
    limiter
        .check(&key)
        .await
        .map(|_| ())
        .map_err(|retry_after_seconds| {
            ApiError::TooManyRequests {
                retry_after_seconds,
            }
            .into()
        })
}

async fn remove_uploaded_file_if_present(path: Option<&PathBuf>) {
    let Some(path) = path else {
        return;
    };
    if let Err(err) = fs::remove_file(path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        warn!(path = %path.display(), error = %err, "failed removing staged channel upload");
    }
}

async fn stream_attachment_field_to_path(
    mut field: axum::extract::multipart::Field<'_>,
    target_path: &StdPath,
) -> Result<i64, AppError> {
    let mut file = fs::File::create(target_path)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to create upload target: {e}")))?;
    let mut total_bytes: usize = 0;

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid file field: {e}")))?
    {
        total_bytes = total_bytes.saturating_add(chunk.len());
        if total_bytes > MAX_ATTACHMENT_BYTES {
            drop(file);
            let _ = fs::remove_file(target_path).await;
            return Err(ApiError::BadRequest(format!(
                "file too large (max {} MB)",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            ))
            .into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| ApiError::Internal(format!("failed saving uploaded file: {e}")))?;
    }

    if total_bytes == 0 {
        drop(file);
        let _ = fs::remove_file(target_path).await;
        return Err(ApiError::BadRequest("uploaded file is empty".into()).into());
    }

    file.flush()
        .await
        .map_err(|e| ApiError::Internal(format!("failed finalizing uploaded file: {e}")))?;
    Ok(total_bytes as i64)
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

    let messages = rustfin_db::repo::channels::list_messages(
        &state.db,
        &channel_id,
        limit,
        before_ts,
        params.before_id.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let message_ids: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
    let attachment_rows =
        rustfin_db::repo::channels::list_message_attachments_for_messages(&state.db, &message_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let mut attachments_by_message: HashMap<String, Vec<MessageAttachmentResponse>> =
        HashMap::new();
    for attachment in attachment_rows {
        attachments_by_message
            .entry(attachment.message_id.clone())
            .or_default()
            .push(attachment_to_response(&attachment));
    }

    let mut response: Vec<MessageResponse> = Vec::with_capacity(messages.len());
    for m in messages {
        let attachments = attachments_by_message.remove(&m.id).unwrap_or_default();
        let avatar_url = avatar_url_for_user(&m.user_id, m.avatar_path.as_deref());
        response.push(MessageResponse {
            id: m.id,
            channel_id: m.channel_id,
            user_id: m.user_id,
            username: m.username,
            avatar_url,
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
    enforce_channel_rate_limit(text_message_rate_limiter(), "send", &auth, &channel_id).await?;
    let sender = resolve_sender_profile(&state, &auth).await?;

    let row = rustfin_db::repo::channels::create_message(
        &state.db,
        &channel_id,
        &auth.user_id,
        &sender.display_name,
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
            avatar_url: sender.avatar_url.clone(),
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
            avatar_url: sender.avatar_url,
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
    enforce_channel_rate_limit(
        attachment_upload_rate_limiter(),
        "upload",
        &auth,
        &channel_id,
    )
    .await?;
    let sender = resolve_sender_profile(&state, &auth).await?;

    let mut maybe_content: Option<String> = None;
    let mut maybe_filename: Option<String> = None;
    let mut maybe_content_type: Option<String> = None;
    let mut maybe_stored_path: Option<PathBuf> = None;
    let mut maybe_file_size_bytes: Option<i64> = None;
    let upload_dir = state.cache_dir.join(CHANNEL_UPLOADS_DIR).join(&channel_id);

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

        if field_name == "file" && maybe_stored_path.is_none() {
            let file_name = sanitize_file_name(field.file_name().unwrap_or("upload.bin"));
            let content_type = infer_content_type(&file_name, field.content_type());

            fs::create_dir_all(&upload_dir).await.map_err(|e| {
                ApiError::Internal(format!("failed to create upload directory: {e}"))
            })?;

            let attachment_id = uuid::Uuid::new_v4().to_string();
            let ext = StdPath::new(&file_name)
                .extension()
                .and_then(|v| v.to_str())
                .and_then(sanitize_extension);
            let stored_file_name = match ext {
                Some(ext) => format!("{attachment_id}.{ext}"),
                None => attachment_id,
            };
            let stored_path = upload_dir.join(stored_file_name);
            let size_bytes = stream_attachment_field_to_path(field, &stored_path).await?;

            maybe_filename = Some(file_name);
            maybe_content_type = Some(content_type);
            maybe_file_size_bytes = Some(size_bytes);
            maybe_stored_path = Some(stored_path);
        }
    }

    if maybe_stored_path.is_none() {
        return Err(ApiError::BadRequest("multipart form requires a file field".into()).into());
    }
    let file_name = maybe_filename.unwrap_or_else(|| "upload.bin".to_string());
    let content_type = maybe_content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let stored_path = maybe_stored_path.clone();
    let size_bytes = maybe_file_size_bytes.unwrap_or(0);
    let mut message_content = maybe_content.unwrap_or_default().trim().to_string();
    if message_content.len() > MAX_MESSAGE_CHARS {
        remove_uploaded_file_if_present(stored_path.as_ref()).await;
        return Err(ApiError::BadRequest(format!(
            "message content too long (max {MAX_MESSAGE_CHARS} chars)"
        ))
        .into());
    }
    if message_content.is_empty() {
        message_content = format!("Shared file: {file_name}");
    }
    let Some(stored_path) = stored_path else {
        return Err(ApiError::BadRequest("multipart form requires a file field".into()).into());
    };

    let row = match rustfin_db::repo::channels::create_message(
        &state.db,
        &channel_id,
        &auth.user_id,
        &sender.display_name,
        &message_content,
    )
    .await
    {
        Ok(row) => row,
        Err(err) => {
            remove_uploaded_file_if_present(Some(&stored_path)).await;
            return Err(ApiError::Internal(format!("db error: {err}")).into());
        }
    };

    let attachment = match rustfin_db::repo::channels::create_message_attachment(
        &state.db,
        &row.id,
        &channel_id,
        &file_name,
        &content_type,
        size_bytes,
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
            avatar_url: sender.avatar_url.clone(),
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
            avatar_url: sender.avatar_url,
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

    let file = fs::File::open(&attachment.storage_path)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ApiError::NotFound("attachment file not found".into())
            } else {
                ApiError::Internal(format!("failed reading attachment file: {e}"))
            }
        })?;
    let stream = ReaderStream::new(file);

    let mut response = Response::new(Body::from_stream(stream));
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

#[derive(Debug, Serialize)]
pub struct VoiceTranscriptionStatusResponse {
    pub channel_id: String,
    pub status: String,
    pub session_id: Option<String>,
    pub started_by_username: Option<String>,
    pub started_ts: Option<i64>,
    pub ended_ts: Option<i64>,
    pub output_available: bool,
    pub output_download_path: Option<String>,
    pub message: Option<String>,
    pub entry_count: i64,
}

#[derive(Debug, Serialize)]
pub struct VoiceTranscriptionSessionSummary {
    pub session_id: String,
    pub status: String,
    pub started_by_username: String,
    pub started_ts: i64,
    pub ended_ts: Option<i64>,
    pub output_available: bool,
    pub output_download_path: Option<String>,
    pub message: Option<String>,
    pub entry_count: i64,
}

#[derive(Debug, Serialize)]
pub struct VoiceTranscriptionSessionsResponse {
    pub channel_id: String,
    pub sessions: Vec<VoiceTranscriptionSessionSummary>,
}

#[derive(Debug, Deserialize)]
pub struct TranscribeChunkRequest {
    pub session_id: String,
    pub sample_rate_hz: u32,
    pub started_ts_ms: i64,
    pub ended_ts_ms: i64,
    pub pcm_s16le_base64: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranscribeChunkResponse {
    pub accepted: bool,
    pub persisted_segments: i64,
}

fn status_info_from_session(
    session: Option<&rustfin_db::repo::channel_transcripts::TranscriptSessionRow>,
    message: Option<String>,
) -> VoiceTranscriptionStateInfo {
    match session {
        Some(s) => VoiceTranscriptionStateInfo {
            status: s.status.clone(),
            session_id: Some(s.id.clone()),
            started_by_username: Some(s.started_by_username.clone()),
            started_ts: Some(s.started_ts),
            ended_ts: s.ended_ts,
            output_available: s.output_path.is_some(),
            message,
        },
        None => VoiceTranscriptionStateInfo {
            status: "idle".to_string(),
            session_id: None,
            started_by_username: None,
            started_ts: None,
            ended_ts: None,
            output_available: false,
            message,
        },
    }
}

async fn is_user_in_voice_channel(state: &AppState, channel_id: &str, user_id: &str) -> bool {
    let snapshot = state.channel_manager.voice_snapshot().await;
    snapshot
        .get(channel_id)
        .map(|members| members.iter().any(|u| u.user_id == user_id))
        .unwrap_or(false)
}

fn format_timestamp_compact(ts: i64) -> String {
    let Some(dt) = DateTime::<Utc>::from_timestamp(ts, 0) else {
        return ts.to_string();
    };
    dt.to_rfc3339()
}

fn format_ms(ms: i64) -> String {
    let total_ms = ms.max(0);
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn build_transcript_markdown(
    channel: &rustfin_db::repo::channels::ChannelRow,
    session: &rustfin_db::repo::channel_transcripts::TranscriptSessionRow,
    entries: &[rustfin_db::repo::channel_transcripts::TranscriptEntryRow],
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Voice Channel Transcript");
    let _ = writeln!(out);
    let _ = writeln!(out, "- Channel: {}", channel.name);
    let _ = writeln!(out, "- Channel ID: {}", channel.id);
    let _ = writeln!(out, "- Session ID: {}", session.id);
    let _ = writeln!(
        out,
        "- Started: {} (by {})",
        format_timestamp_compact(session.started_ts),
        session.started_by_username
    );
    if let Some(ended) = session.ended_ts {
        let _ = writeln!(out, "- Ended: {}", format_timestamp_compact(ended));
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## Timeline");
    let _ = writeln!(out);

    if entries.is_empty() {
        let _ = writeln!(out, "_No transcript lines captured._");
        return out;
    }

    let session_started_ms = session.started_ts.saturating_mul(1000);
    for entry in entries {
        let relative_start_ms = entry.started_ts_ms.saturating_sub(session_started_ms);
        let relative_end_ms = entry
            .ended_ts_ms
            .max(entry.started_ts_ms)
            .saturating_sub(session_started_ms);
        let _ = writeln!(
            out,
            "[{} - {}] {}: {}",
            format_ms(relative_start_ms),
            format_ms(relative_end_ms),
            entry.username,
            entry.text.trim()
        );
    }
    out
}

pub async fn get_transcription_status(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<VoiceTranscriptionStatusResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }

    let running = rustfin_db::repo::channel_transcripts::get_running_session_for_channel(
        &state.db,
        &channel_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let session = if running.is_some() {
        running
    } else {
        rustfin_db::repo::channel_transcripts::get_latest_session_for_channel(
            &state.db,
            &channel_id,
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    };

    let entry_count = if let Some(ref s) = session {
        let counts = rustfin_db::repo::channel_transcripts::count_entries_for_sessions(
            &state.db,
            std::slice::from_ref(&s.id),
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        counts
            .into_iter()
            .find_map(|(session_id, count)| {
                if session_id == s.id {
                    Some(count)
                } else {
                    None
                }
            })
            .unwrap_or(0)
    } else {
        0
    };

    Ok(Json(VoiceTranscriptionStatusResponse {
        channel_id,
        status: session
            .as_ref()
            .map(|s| s.status.clone())
            .unwrap_or_else(|| "idle".to_string()),
        session_id: session.as_ref().map(|s| s.id.clone()),
        started_by_username: session.as_ref().map(|s| s.started_by_username.clone()),
        started_ts: session.as_ref().map(|s| s.started_ts),
        ended_ts: session.as_ref().and_then(|s| s.ended_ts),
        output_available: session
            .as_ref()
            .map(|s| s.output_path.is_some())
            .unwrap_or(false),
        output_download_path: session.as_ref().and_then(|s| {
            if s.output_path.is_some() {
                Some(format!(
                    "/api/v1/channels/{}/transcription/sessions/{}/download",
                    channel.id, s.id
                ))
            } else {
                None
            }
        }),
        message: session.as_ref().and_then(|s| s.failure_reason.clone()),
        entry_count,
    }))
}

pub async fn list_transcription_sessions(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<VoiceTranscriptionSessionsResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }

    let sessions = rustfin_db::repo::channel_transcripts::list_sessions_for_channel(
        &state.db,
        &channel_id,
        100,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let session_ids: Vec<String> = sessions.iter().map(|session| session.id.clone()).collect();
    let counts =
        rustfin_db::repo::channel_transcripts::count_entries_for_sessions(&state.db, &session_ids)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let counts_by_session_id: HashMap<String, i64> = counts.into_iter().collect();

    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        let entry_count = *counts_by_session_id.get(&session.id).unwrap_or(&0);
        let output_download_path = if session.output_path.is_some() {
            Some(format!(
                "/api/v1/channels/{}/transcription/sessions/{}/download",
                channel.id, session.id
            ))
        } else {
            None
        };
        out.push(VoiceTranscriptionSessionSummary {
            session_id: session.id,
            status: session.status,
            started_by_username: session.started_by_username,
            started_ts: session.started_ts,
            ended_ts: session.ended_ts,
            output_available: output_download_path.is_some(),
            output_download_path,
            message: session.failure_reason,
            entry_count,
        });
    }

    Ok(Json(VoiceTranscriptionSessionsResponse {
        channel_id,
        sessions: out,
    }))
}

pub async fn start_transcription(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<VoiceTranscriptionStatusResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }
    if !is_user_in_voice_channel(&state, &channel_id, &auth.user_id).await {
        return Err(ApiError::Forbidden(
            "join the voice channel before starting transcription".into(),
        )
        .into());
    }

    if let Some(existing) = rustfin_db::repo::channel_transcripts::get_running_session_for_channel(
        &state.db,
        &channel_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    {
        return Ok(Json(VoiceTranscriptionStatusResponse {
            channel_id: channel_id.clone(),
            status: existing.status.clone(),
            session_id: Some(existing.id.clone()),
            started_by_username: Some(existing.started_by_username.clone()),
            started_ts: Some(existing.started_ts),
            ended_ts: existing.ended_ts,
            output_available: existing.output_path.is_some(),
            output_download_path: None,
            message: Some("transcription is already running".to_string()),
            entry_count: rustfin_db::repo::channel_transcripts::count_entries_for_sessions(
                &state.db,
                std::slice::from_ref(&existing.id),
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .find_map(|(session_id, count)| {
                if session_id == existing.id {
                    Some(count)
                } else {
                    None
                }
            })
            .unwrap_or(0),
        }));
    }

    let session = rustfin_db::repo::channel_transcripts::create_running_session(
        &state.db,
        &channel_id,
        &auth.user_id,
        &auth.username,
    )
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.is_unique_violation() {
                return ApiError::BadRequest("a transcription is already running".into());
            }
        }
        ApiError::Internal(format!("db error: {e}"))
    })?;

    state
        .channel_manager
        .broadcast(ChannelEvent::VoiceTranscriptionState {
            channel_id: channel_id.clone(),
            state: VoiceTranscriptionStateInfo {
                status: "running".to_string(),
                session_id: Some(session.id.clone()),
                started_by_username: Some(session.started_by_username.clone()),
                started_ts: Some(session.started_ts),
                ended_ts: None,
                output_available: false,
                message: Some("starting transcription model...".to_string()),
            },
        });

    if let Err(err) = transcription_agent::start_session(&state, &session.id).await {
        let _ = rustfin_db::repo::channel_transcripts::fail_session(
            &state.db,
            &session.id,
            &err.to_string(),
        )
        .await;
        state
            .channel_manager
            .broadcast(ChannelEvent::VoiceTranscriptionState {
                channel_id: channel_id.clone(),
                state: VoiceTranscriptionStateInfo {
                    status: "failed".to_string(),
                    session_id: Some(session.id.clone()),
                    started_by_username: Some(session.started_by_username.clone()),
                    started_ts: Some(session.started_ts),
                    ended_ts: Some(chrono::Utc::now().timestamp()),
                    output_available: false,
                    message: Some(err.to_string()),
                },
            });
        return Err(err.into());
    }

    state
        .channel_manager
        .broadcast(ChannelEvent::VoiceTranscriptionState {
            channel_id: channel_id.clone(),
            state: status_info_from_session(Some(&session), None),
        });

    Ok(Json(VoiceTranscriptionStatusResponse {
        channel_id,
        status: session.status,
        session_id: Some(session.id),
        started_by_username: Some(session.started_by_username),
        started_ts: Some(session.started_ts),
        ended_ts: session.ended_ts,
        output_available: false,
        output_download_path: None,
        message: None,
        entry_count: 0,
    }))
}

pub async fn transcribe_chunk(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    Json(body): Json<TranscribeChunkRequest>,
) -> Result<Json<TranscribeChunkResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }
    if !is_user_in_voice_channel(&state, &channel_id, &auth.user_id).await {
        return Err(ApiError::Forbidden(
            "join the voice channel before uploading transcript audio".into(),
        )
        .into());
    }
    if body.session_id.trim().is_empty() {
        return Err(ApiError::BadRequest("session_id is required".into()).into());
    }
    if body.pcm_s16le_base64.trim().is_empty() {
        return Ok(Json(TranscribeChunkResponse {
            accepted: true,
            persisted_segments: 0,
        }));
    }

    let running = rustfin_db::repo::channel_transcripts::get_running_session_for_channel(
        &state.db,
        &channel_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    .ok_or_else(|| {
        ApiError::BadRequest("no active transcription session for this channel".into())
    })?;
    if body.session_id != running.id {
        return Err(ApiError::BadRequest(
            "chunk does not match the active transcription session".into(),
        )
        .into());
    }
    let _in_flight_guard = TranscriptionChunkInFlightGuard::new(&running.id);

    let segments = transcription_agent::transcribe_chunk(
        &state,
        &AgentTranscribeChunkRequest {
            session_id: running.id.clone(),
            user_id: auth.user_id.clone(),
            username: auth.username.clone(),
            sample_rate_hz: body.sample_rate_hz,
            started_ts_ms: body.started_ts_ms,
            ended_ts_ms: body.ended_ts_ms,
            pcm_s16le_base64: body.pcm_s16le_base64,
            language: body.language.clone(),
        },
    )
    .await?;
    if segments.is_empty() {
        warn!(
            channel_id = %channel_id,
            session_id = %running.id,
            user_id = %auth.user_id,
            sample_rate_hz = body.sample_rate_hz,
            started_ts_ms = body.started_ts_ms,
            ended_ts_ms = body.ended_ts_ms,
            "transcription agent returned no segments for uploaded audio chunk"
        );
    }

    let mut persisted_segments = 0_i64;
    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        rustfin_db::repo::channel_transcripts::append_entry(
            &state.db,
            rustfin_db::repo::channel_transcripts::NewTranscriptEntry {
                session_id: &running.id,
                channel_id: &channel_id,
                user_id: &auth.user_id,
                username: &auth.username,
                started_ts_ms: segment.started_ts_ms,
                ended_ts_ms: segment.ended_ts_ms,
                text,
            },
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        persisted_segments += 1;
    }
    if persisted_segments == 0 {
        warn!(
            channel_id = %channel_id,
            session_id = %running.id,
            user_id = %auth.user_id,
            sample_rate_hz = body.sample_rate_hz,
            started_ts_ms = body.started_ts_ms,
            ended_ts_ms = body.ended_ts_ms,
            "transcription chunk produced zero persisted transcript lines"
        );
    }

    Ok(Json(TranscribeChunkResponse {
        accepted: true,
        persisted_segments,
    }))
}

pub async fn stop_transcription(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<VoiceTranscriptionStatusResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }
    if !is_user_in_voice_channel(&state, &channel_id, &auth.user_id).await {
        return Err(ApiError::Forbidden(
            "join the voice channel before stopping transcription".into(),
        )
        .into());
    }

    let running = rustfin_db::repo::channel_transcripts::get_running_session_for_channel(
        &state.db,
        &channel_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    .ok_or_else(|| {
        ApiError::BadRequest("no active transcription session for this channel".into())
    })?;

    state
        .channel_manager
        .broadcast(ChannelEvent::VoiceTranscriptionState {
            channel_id: channel_id.clone(),
            state: VoiceTranscriptionStateInfo {
                status: "finalizing".to_string(),
                session_id: Some(running.id.clone()),
                started_by_username: Some(running.started_by_username.clone()),
                started_ts: Some(running.started_ts),
                ended_ts: None,
                output_available: false,
                message: Some("finalizing transcript".to_string()),
            },
        });

    // Give clients time to flush their final buffered audio chunks and let
    // in-flight transcriptions finish before we seal the transcript output.
    wait_for_transcription_chunks_to_settle(&running.id).await;

    if let Err(err) = transcription_agent::stop_session(&state, &running.id).await {
        let _ = rustfin_db::repo::channel_transcripts::fail_session(
            &state.db,
            &running.id,
            &err.to_string(),
        )
        .await;
        state
            .channel_manager
            .broadcast(ChannelEvent::VoiceTranscriptionState {
                channel_id: channel_id.clone(),
                state: VoiceTranscriptionStateInfo {
                    status: "failed".to_string(),
                    session_id: Some(running.id.clone()),
                    started_by_username: Some(running.started_by_username.clone()),
                    started_ts: Some(running.started_ts),
                    ended_ts: Some(chrono::Utc::now().timestamp()),
                    output_available: false,
                    message: Some(err.to_string()),
                },
            });
        return Err(err.into());
    }

    let entries =
        rustfin_db::repo::channel_transcripts::list_entries_for_session(&state.db, &running.id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut finished = rustfin_db::repo::channel_transcripts::get_session(&state.db, &running.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Internal("transcript session disappeared".into()))?;
    finished.ended_ts = Some(chrono::Utc::now().timestamp());

    let transcript_dir = state
        .cache_dir
        .join("channel_transcripts")
        .join(&channel_id);
    fs::create_dir_all(&transcript_dir)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to create transcript output dir: {e}")))?;
    let output_path = transcript_dir.join(format!("{}.md", running.id));
    let markdown = build_transcript_markdown(&channel, &finished, &entries);
    fs::write(&output_path, markdown)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to write transcript file: {e}")))?;

    rustfin_db::repo::channel_transcripts::complete_session(
        &state.db,
        &running.id,
        &output_path.to_string_lossy(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let completed = rustfin_db::repo::channel_transcripts::get_session(&state.db, &running.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Internal("completed transcript session disappeared".into()))?;
    let entry_count = entries.len() as i64;

    state
        .channel_manager
        .broadcast(ChannelEvent::VoiceTranscriptionState {
            channel_id: channel_id.clone(),
            state: status_info_from_session(Some(&completed), None),
        });

    Ok(Json(VoiceTranscriptionStatusResponse {
        channel_id,
        status: completed.status,
        session_id: Some(completed.id.clone()),
        started_by_username: Some(completed.started_by_username),
        started_ts: Some(completed.started_ts),
        ended_ts: completed.ended_ts,
        output_available: completed.output_path.is_some(),
        output_download_path: Some(format!(
            "/api/v1/channels/{}/transcription/sessions/{}/download",
            channel.id, completed.id
        )),
        message: None,
        entry_count,
    }))
}

pub async fn cancel_transcription(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> Result<Json<VoiceTranscriptionStatusResponse>, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }
    if !is_user_in_voice_channel(&state, &channel_id, &auth.user_id).await {
        return Err(ApiError::Forbidden(
            "join the voice channel before cancelling transcription".into(),
        )
        .into());
    }

    let running = rustfin_db::repo::channel_transcripts::get_running_session_for_channel(
        &state.db,
        &channel_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    .ok_or_else(|| {
        ApiError::BadRequest("no active transcription session for this channel".into())
    })?;

    let _ = transcription_agent::cancel_session(&state, &running.id).await;
    rustfin_db::repo::channel_transcripts::cancel_session(&state.db, &running.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    state
        .channel_manager
        .broadcast(ChannelEvent::VoiceTranscriptionState {
            channel_id: channel_id.clone(),
            state: VoiceTranscriptionStateInfo {
                status: "cancelled".to_string(),
                session_id: Some(running.id),
                started_by_username: Some(running.started_by_username),
                started_ts: Some(running.started_ts),
                ended_ts: Some(chrono::Utc::now().timestamp()),
                output_available: false,
                message: Some("transcription cancelled".to_string()),
            },
        });

    Ok(Json(VoiceTranscriptionStatusResponse {
        channel_id,
        status: "cancelled".to_string(),
        session_id: None,
        started_by_username: None,
        started_ts: None,
        ended_ts: None,
        output_available: false,
        output_download_path: None,
        message: Some("transcription cancelled".to_string()),
        entry_count: 0,
    }))
}

pub async fn download_transcription(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((channel_id, session_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let _channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    let session = rustfin_db::repo::channel_transcripts::get_session(&state.db, &session_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("transcript session not found".into()))?;
    if session.channel_id != channel_id {
        return Err(ApiError::NotFound("transcript session not found for channel".into()).into());
    }
    let output_path = session
        .output_path
        .ok_or_else(|| ApiError::BadRequest("transcript output is not available".into()))?;
    let bytes = fs::read(&output_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::NotFound("transcript file not found".into())
        } else {
            ApiError::Internal(format!("failed reading transcript file: {e}"))
        }
    })?;

    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    let file_name = format!("voice-transcript-{}.md", session_id);
    let content_disposition = format!(
        "attachment; filename=\"{}\"",
        sanitize_content_disposition_filename(&file_name)
    );
    if let Ok(value) = HeaderValue::from_str(&content_disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

pub async fn delete_transcription_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((channel_id, session_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let channel = get_accessible_channel(&state, &auth, &channel_id).await?;
    if channel.kind != "voice" {
        return Err(ApiError::BadRequest(
            "transcription is only available for voice channels".into(),
        )
        .into());
    }

    let session = rustfin_db::repo::channel_transcripts::get_session(&state.db, &session_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("transcript session not found".into()))?;
    if session.channel_id != channel_id {
        return Err(ApiError::NotFound("transcript session not found for channel".into()).into());
    }
    if session.status == "running" {
        return Err(ApiError::BadRequest(
            "stop or cancel the active transcript before deleting it".into(),
        )
        .into());
    }

    if let Some(path) = &session.output_path {
        if let Err(err) = fs::remove_file(path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    session_id = %session.id,
                    output_path = %path,
                    error = %err,
                    "failed deleting transcript output file"
                );
            }
        }
    }

    rustfin_db::repo::channel_transcripts::delete_session(&state.db, &session.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_chunk_guard_tracks_in_flight_counts() {
        let session_id = "session-test";
        assert_eq!(transcription_in_flight_count(session_id), 0);
        {
            let _guard = TranscriptionChunkInFlightGuard::new(session_id);
            assert_eq!(transcription_in_flight_count(session_id), 1);
        }
        assert_eq!(transcription_in_flight_count(session_id), 0);
    }

    #[test]
    fn transcript_markdown_uses_session_relative_timestamps() {
        let channel = rustfin_db::repo::channels::ChannelRow {
            id: "ch-1".to_string(),
            name: "Voice 1".to_string(),
            kind: "voice".to_string(),
            position: 0,
            is_private: false,
            created_by: "u-admin".to_string(),
            created_ts: 1_700_000_000,
        };
        let session = rustfin_db::repo::channel_transcripts::TranscriptSessionRow {
            id: "s-1".to_string(),
            channel_id: channel.id.clone(),
            status: "completed".to_string(),
            started_by_user_id: "u-admin".to_string(),
            started_by_username: "admin".to_string(),
            started_ts: 1_700_000_000,
            ended_ts: Some(1_700_000_120),
            output_path: Some("/tmp/t.md".to_string()),
            failure_reason: None,
        };
        let entries = vec![rustfin_db::repo::channel_transcripts::TranscriptEntryRow {
            id: "e-1".to_string(),
            session_id: session.id.clone(),
            channel_id: channel.id.clone(),
            user_id: "u-2".to_string(),
            username: "alice".to_string(),
            started_ts_ms: 1_700_000_001_500,
            ended_ts_ms: 1_700_000_002_000,
            text: "hello there".to_string(),
            created_ts: 1_700_000_002,
        }];

        let markdown = build_transcript_markdown(&channel, &session, &entries);
        assert!(markdown.contains("[00:00:01.500 - 00:00:02.000] alice: hello there"));
    }
}
