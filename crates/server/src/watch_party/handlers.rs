use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::setup::rate_limit::RateLimiter;
use crate::state::AppState;

use super::permissions::RoomPolicy;

const MAX_INVITEES: usize = 100;
const ROOM_PASSWORD_MIN_LEN: usize = 4;
const ROOM_PASSWORD_MAX_LEN: usize = 128;

static CREATE_ROOM_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();
static JOIN_ROOM_RATE_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

#[derive(Debug, Serialize)]
pub struct MinimalUser {
    pub id: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct EligibleLibrariesRequest {
    #[serde(default)]
    pub user_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EligibleLibrariesResponse {
    pub library_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomPolicy {
    #[serde(default = "default_true")]
    pub allow_non_host_play_pause: bool,
    #[serde(default = "default_false")]
    pub allow_non_host_seek: bool,
    #[serde(default = "default_join_role")]
    pub default_join_role: String,
    #[serde(default)]
    pub invite_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomInvite {
    pub user_id: String,
    #[serde(default = "default_join_role")]
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    /// For video rooms: the item to watch.
    pub item_id: Option<String>,
    /// For audio rooms: the music library to use.
    pub audio_library_id: Option<String>,
    /// Explicit room mode override. Use "youtube" for YouTube watch parties.
    pub room_mode: Option<String>,
    #[serde(default)]
    pub invites: Vec<CreateRoomInvite>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub policy: Option<CreateRoomPolicy>,
}

#[derive(Debug, Serialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub join_path: String,
}

#[derive(Debug, Serialize)]
pub struct RoomMemberResponse {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct RoomResponse {
    pub room_id: String,
    pub item_id: String,
    pub host_user_id: String,
    pub status: String,
    pub password_required: bool,
    pub policy: serde_json::Value,
    pub members: Vec<RoomMemberResponse>,
    pub room_mode: String,
    pub audio_library_id: Option<String>,
    pub youtube_video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JoinRoomRequest {
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JoinRoomResponse {
    pub ok: bool,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct InviteResponse {
    pub room_id: String,
    pub item_id: String,
    pub item_title: String,
    pub host_user_id: String,
    pub host_username: String,
    pub created_ts: i64,
    pub password_required: bool,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct AudioTracksQuery {
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AudioTrackResponse {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_art_url: Option<String>,
    pub duration_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_join_role() -> String {
    "viewer".to_string()
}

fn create_room_rate_limiter() -> &'static RateLimiter {
    CREATE_ROOM_RATE_LIMITER.get_or_init(|| RateLimiter::new(20, 60))
}

fn join_room_rate_limiter() -> &'static RateLimiter {
    JOIN_ROOM_RATE_LIMITER.get_or_init(|| RateLimiter::new(25, 60))
}

async fn check_rate_limit(limiter: &RateLimiter, key: &str) -> Result<(), AppError> {
    match limiter.check(key).await {
        Ok(_) => Ok(()),
        Err(retry_after) => Err(ApiError::TooManyRequests {
            retry_after_seconds: retry_after,
        }
        .into()),
    }
}

fn normalize_member_role(role: &str) -> Result<String, AppError> {
    let normalized = role.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "viewer" | "controller" | "host" => Ok(normalized),
        _ => Err(ApiError::BadRequest(
            "invalid member role; expected one of: host, controller, viewer".into(),
        )
        .into()),
    }
}

fn normalize_default_join_role(role: &str) -> Result<String, AppError> {
    let normalized = role.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "viewer" | "controller" => Ok(normalized),
        _ => Err(ApiError::BadRequest(
            "invalid default join role; expected viewer or controller".into(),
        )
        .into()),
    }
}

fn parse_policy(input: Option<CreateRoomPolicy>) -> Result<RoomPolicy, AppError> {
    match input {
        Some(policy) => Ok(RoomPolicy {
            allow_non_host_play_pause: policy.allow_non_host_play_pause,
            allow_non_host_seek: policy.allow_non_host_seek,
            default_join_role: normalize_default_join_role(&policy.default_join_role)?,
            invite_only: policy.invite_only,
        }),
        None => Ok(RoomPolicy::default()),
    }
}

fn normalize_password(password: Option<String>) -> Result<Option<String>, AppError> {
    match password {
        Some(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.len() < ROOM_PASSWORD_MIN_LEN {
                return Err(ApiError::BadRequest(format!(
                    "room password must be at least {ROOM_PASSWORD_MIN_LEN} characters"
                ))
                .into());
            }
            if trimmed.len() > ROOM_PASSWORD_MAX_LEN {
                return Err(ApiError::BadRequest(format!(
                    "room password must be <= {ROOM_PASSWORD_MAX_LEN} characters"
                ))
                .into());
            }
            Ok(Some(trimmed))
        }
        None => Ok(None),
    }
}

fn hash_room_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(format!("password hash error: {e}")))?;
    Ok(hash.to_string())
}

fn verify_room_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| ApiError::Internal(format!("password hash parse error: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

async fn ensure_library_access_for_user(
    state: &AppState,
    user_id: &str,
    user_role: &str,
    library_id: &str,
) -> Result<(), AppError> {
    if user_role == "admin" {
        return Ok(());
    }

    let allowed = rustfin_db::repo::users::is_library_allowed(&state.db, user_id, library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !allowed {
        return Err(ApiError::Forbidden("library access denied".into()).into());
    }
    Ok(())
}

fn room_member_username<'a>(
    usernames: &'a std::collections::HashMap<String, String>,
    user_id: &str,
) -> &'a str {
    usernames
        .get(user_id)
        .map(String::as_str)
        .unwrap_or("unknown")
}

fn is_password_required(hash: Option<&str>) -> bool {
    hash.is_some_and(|value| !value.trim().is_empty())
}

#[derive(Debug, Serialize)]
pub struct PublicRoomEntry {
    pub room_id: String,
    pub host_username: String,
    pub title: String,
    pub room_mode: String,
    pub password_required: bool,
    pub member_count: i64,
    pub created_ts: i64,
}

pub async fn list_public_rooms(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<PublicRoomEntry>>, AppError> {
    let rooms = rustfin_db::repo::watch_party::list_public_rooms(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let entries = rooms
        .into_iter()
        .map(|r| {
            let title = if r.room_mode == "audio" {
                if r.audio_library_name.is_empty() {
                    "Music Party".to_string()
                } else {
                    format!("🎵 {}", r.audio_library_name)
                }
            } else if r.room_mode == "youtube" {
                "▶ YouTube Party".to_string()
            } else {
                r.item_title
            };
            PublicRoomEntry {
                room_id: r.id,
                host_username: r.host_username,
                title,
                room_mode: r.room_mode,
                password_required: r.password_required,
                member_count: r.member_count,
                created_ts: r.created_ts,
            }
        })
        .collect();

    Ok(Json(entries))
}

pub async fn list_inviteable_users(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<MinimalUser>>, AppError> {
    let users = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")));

    let mut users = users?;
    users.sort_by(|a, b| a.username.cmp(&b.username));

    Ok(Json(
        users
            .into_iter()
            .map(|u| MinimalUser {
                id: u.id,
                username: u.username,
            })
            .collect(),
    ))
}

pub async fn eligible_libraries(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<EligibleLibrariesRequest>,
) -> Result<Json<EligibleLibrariesResponse>, AppError> {
    if body.user_ids.len() > MAX_INVITEES {
        return Err(
            ApiError::BadRequest(format!("too many user IDs; maximum is {MAX_INVITEES}")).into(),
        );
    }

    let mut requested_user_ids = Vec::with_capacity(body.user_ids.len() + 1);
    requested_user_ids.push(auth.user_id.clone());

    let mut seen = HashSet::new();
    seen.insert(auth.user_id.clone());

    for user_id in body.user_ids {
        let trimmed = user_id.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            requested_user_ids.push(trimmed.to_string());
        }
    }

    let all_library_ids: Vec<String> = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .map(|lib| lib.id)
        .collect();

    let mut intersection: Option<HashSet<String>> = None;

    for user_id in requested_user_ids {
        let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

        let current: HashSet<String> = if user.role == "admin" {
            all_library_ids.iter().cloned().collect()
        } else {
            rustfin_db::repo::users::get_library_access(&state.db, &user.id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .into_iter()
                .collect()
        };

        intersection = match intersection {
            Some(existing) => Some(existing.intersection(&current).cloned().collect()),
            None => Some(current),
        };
    }

    let mut library_ids: Vec<String> = intersection.unwrap_or_default().into_iter().collect();
    library_ids.sort();
    Ok(Json(EligibleLibrariesResponse { library_ids }))
}

pub async fn create_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<CreateRoomResponse>), AppError> {
    check_rate_limit(
        create_room_rate_limiter(),
        &format!("create-room:{}", auth.user_id),
    )
    .await?;

    let policy = parse_policy(body.policy)?;
    let password = normalize_password(body.password)?;
    let password_hash = match password {
        Some(value) => Some(hash_room_password(&value)?),
        None => None,
    };

    let mut deduped_invites = Vec::with_capacity(body.invites.len());
    let mut seen = HashSet::new();

    for invite in body.invites {
        let user_id = invite.user_id.trim();
        if user_id.is_empty() || user_id == auth.user_id {
            continue;
        }
        if !seen.insert(user_id.to_string()) {
            continue;
        }

        let role = normalize_member_role(&invite.role)?;
        if role == "host" {
            return Err(ApiError::BadRequest("invite role cannot be host".into()).into());
        }

        deduped_invites.push((user_id.to_string(), role));
    }

    if deduped_invites.len() > MAX_INVITEES {
        return Err(
            ApiError::BadRequest(format!("too many invitees; maximum is {MAX_INVITEES}")).into(),
        );
    }

    // Determine room mode and item_id
    let (room_mode, item_id, audio_library_id, track_ids) =
        if body.room_mode.as_deref() == Some("youtube") {
            // YouTube room — no media item or library required
            ("youtube".to_string(), String::new(), None, None)
        } else if let Some(audio_lib_id) = body.audio_library_id.as_deref().filter(|s| !s.trim().is_empty()) {
            // Audio room
            let library = rustfin_db::repo::libraries::get_library(&state.db, audio_lib_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("audio library not found".into()))?;

            if library.kind != "music" {
                return Err(ApiError::BadRequest(
                    "audio_library_id must refer to a music library".into(),
                )
                .into());
            }

            ensure_library_access_for_user(
                &state,
                &auth.user_id,
                &auth.role,
                audio_lib_id,
            )
            .await?;

            // Get all tracks from the library
            let tracks = rustfin_db::repo::watch_party::get_library_tracks(&state.db, audio_lib_id, None)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

            if tracks.is_empty() {
                return Err(ApiError::BadRequest(
                    "the music library has no tracks; scan it first".into(),
                )
                .into());
            }

            // Shuffle tracks
            let mut track_ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let seed = chrono::Utc::now().timestamp_millis() as u64;
            for i in (1..track_ids.len()).rev() {
                let mut hasher = DefaultHasher::new();
                (seed ^ i as u64).hash(&mut hasher);
                let j = (hasher.finish() as usize) % (i + 1);
                track_ids.swap(i, j);
            }

            let first_track_id = track_ids[0].clone();
            (
                "audio".to_string(),
                first_track_id,
                Some(audio_lib_id.to_string()),
                Some(track_ids),
            )
        } else {
            // Video room
            let item_id = body
                .item_id
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| ApiError::BadRequest("item_id is required for video rooms".into()))?;

            let item = rustfin_db::repo::items::get_item(&state.db, item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("item not found".into()))?;

            if item.kind != "movie" && item.kind != "episode" {
                return Err(ApiError::BadRequest(
                    "watch parties currently support movie and episode items only".into(),
                )
                .into());
            }

            ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id)
                .await?;

            ("video".to_string(), item.id.clone(), None, None)
        };

    let now = chrono::Utc::now().timestamp();

    let mut members = Vec::with_capacity(deduped_invites.len() + 1);
    members.push(rustfin_db::repo::watch_party::NewWatchPartyMember {
        user_id: auth.user_id.clone(),
        role: "host".to_string(),
        status: "joined".to_string(),
        invited_by: Some(auth.user_id.clone()),
        invited_ts: Some(now),
        joined_ts: Some(now),
    });

    // Validate invitees for video rooms (check library access per invitee)
    for (user_id, role) in &deduped_invites {
        let user = rustfin_db::repo::users::find_by_id(&state.db, user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("invited user not found".into()))?;

        if room_mode == "video" {
            // For video rooms, we already have the item's library_id baked in item above.
            // Re-fetch item to get library_id.
            let item = rustfin_db::repo::items::get_item(&state.db, &item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
            ensure_library_access_for_user(&state, &user.id, &user.role, &item.library_id).await?;
        } else if let Some(ref lib_id) = audio_library_id {
            ensure_library_access_for_user(&state, &user.id, &user.role, lib_id).await?;
        }

        members.push(rustfin_db::repo::watch_party::NewWatchPartyMember {
            user_id: user_id.clone(),
            role: role.clone(),
            status: "invited".to_string(),
            invited_by: Some(auth.user_id.clone()),
            invited_ts: Some(now),
            joined_ts: None,
        });
    }

    let policy_json = serde_json::to_string(&policy)
        .map_err(|e| ApiError::Internal(format!("policy serialization error: {e}")))?;

    let created = rustfin_db::repo::watch_party::create_room_with_members(
        &state.db,
        &auth.user_id,
        &item_id,
        &policy_json,
        password_hash.as_deref(),
        &members,
        Some(&room_mode),
        audio_library_id.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // For audio rooms, persist the track queue
    if let Some(ref track_ids) = track_ids {
        let track_ids_json = serde_json::to_string(track_ids)
            .map_err(|e| ApiError::Internal(format!("queue serialization error: {e}")))?;
        rustfin_db::repo::watch_party::upsert_audio_queue(
            &state.db,
            &created.id,
            &track_ids_json,
            0,
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    Ok((
        StatusCode::CREATED,
        Json(CreateRoomResponse {
            room_id: created.id.clone(),
            join_path: format!("/watch-party/rooms/{}", created.id),
        }),
    ))
}

pub async fn get_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<RoomResponse>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    // For video rooms only: verify item and library access
    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let policy: RoomPolicy = serde_json::from_str(&room.policy_json)
        .map_err(|e| ApiError::Internal(format!("invalid room policy JSON: {e}")))?;

    let me = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if policy.invite_only && me.is_none() {
        return Err(ApiError::Forbidden(
            "room is invite-only; this account must be invited by the host".into(),
        )
        .into());
    }

    let members = rustfin_db::repo::watch_party::list_members(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let usernames: std::collections::HashMap<String, String> =
        rustfin_db::repo::users::list_users(&state.db)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect();

    let members = members
        .into_iter()
        .map(|member| RoomMemberResponse {
            username: room_member_username(&usernames, &member.user_id).to_string(),
            user_id: member.user_id,
            role: member.role,
            status: member.status,
        })
        .collect();

    // For YouTube rooms, reflect the live runtime state of the video ID if available
    let youtube_video_id = if room.room_mode == "youtube" {
        if let Some(runtime) = state.watch_party.get_runtime(&room_id).await {
            runtime.get_youtube_video_id().await.or(room.youtube_video_id)
        } else {
            room.youtube_video_id
        }
    } else {
        None
    };

    Ok(Json(RoomResponse {
        room_id: room.id,
        item_id: room.item_id,
        host_user_id: room.host_user_id,
        status: room.status,
        password_required: is_password_required(room.join_password_hash.as_deref()),
        policy: serde_json::to_value(policy)
            .map_err(|e| ApiError::Internal(format!("policy serialization error: {e}")))?,
        members,
        room_mode: room.room_mode,
        audio_library_id: room.audio_library_id,
        youtube_video_id,
    }))
}

pub async fn join_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, AppError> {
    check_rate_limit(
        join_room_rate_limiter(),
        &format!("join-room:{}:{}", room_id, auth.user_id),
    )
    .await?;

    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.status != "lobby" {
        return Err(ApiError::Conflict("room is not accepting new joins".into()).into());
    }

    let policy: RoomPolicy = serde_json::from_str(&room.policy_json)
        .map_err(|e| ApiError::Internal(format!("invalid room policy JSON: {e}")))?;

    // For video rooms only: verify item and library access
    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let existing_member =
        rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if policy.invite_only && existing_member.is_none() {
        return Err(ApiError::Forbidden(
            "room is invite-only; password is not enough without an invite".into(),
        )
        .into());
    }

    if auth.user_id != room.host_user_id {
        if let Some(hash) = room.join_password_hash.as_deref() {
            let required = is_password_required(Some(hash));
            if required {
                let provided = body.password.unwrap_or_default();
                let valid = verify_room_password(provided.trim(), hash)?;
                if !valid {
                    return Err(ApiError::Forbidden("invalid room password".into()).into());
                }
            }
        }
    }

    let role = existing_member
        .as_ref()
        .map(|member| member.role.clone())
        .unwrap_or_else(|| policy.default_join_role.clone());

    let now = chrono::Utc::now().timestamp();
    rustfin_db::repo::watch_party::upsert_member(
        &state.db,
        &room_id,
        &rustfin_db::repo::watch_party::NewWatchPartyMember {
            user_id: auth.user_id.clone(),
            role: role.clone(),
            status: "joined".to_string(),
            invited_by: Some(room.host_user_id.clone()),
            invited_ts: existing_member.as_ref().and_then(|m| m.invited_ts),
            joined_ts: Some(now),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(JoinRoomResponse { ok: true, role }))
}

pub async fn leave_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.room_mode == "video" {
        let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("watch party item not found".into()))?;
        ensure_library_access_for_user(&state, &auth.user_id, &auth.role, &item.library_id).await?;
    }

    let updated = rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &room_id,
        &auth.user_id,
        "left",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !updated {
        return Err(ApiError::NotFound("room membership not found".into()).into());
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn end_room(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.host_user_id != auth.user_id {
        return Err(ApiError::Forbidden("only host can end the room".into()).into());
    }

    rustfin_db::repo::watch_party::set_room_status(&state.db, &room_id, "ended")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if let Some(runtime) = state.watch_party.get_runtime(&room_id).await {
        let _ = runtime.tx.send(super::protocol::ServerMessage::RoomEnded);
    }

    state.watch_party.remove_runtime(&room_id).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn list_invites(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<InviteResponse>>, AppError> {
    let invites = rustfin_db::repo::watch_party::list_invites_for_user(&state.db, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(
        invites
            .into_iter()
            .map(|row| InviteResponse {
                room_id: row.room_id,
                item_id: row.item_id,
                item_title: row.item_title,
                host_user_id: row.host_user_id,
                host_username: row.host_username,
                created_ts: row.created_ts,
                password_required: row.password_required,
                role: row.role,
                status: row.status,
            })
            .collect(),
    ))
}

pub async fn decline_invite(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let member = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("invite not found".into()))?;

    if member.status != "invited" {
        return Err(ApiError::BadRequest("invite is not pending".into()).into());
    }

    rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &room_id,
        &auth.user_id,
        "declined",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct InviteMembersRequest {
    invites: Vec<InviteInput>,
}

#[derive(Deserialize)]
pub struct InviteInput {
    user_id: String,
    role: String,
}

pub async fn invite_members(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<InviteMembersRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    if room.status != "lobby" {
        return Err(ApiError::BadRequest("room is not active".into()).into());
    }

    let caller = rustfin_db::repo::watch_party::get_member(&state.db, &room_id, &auth.user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::Forbidden("you are not in this room".into()))?;

    if caller.status != "joined" {
        return Err(ApiError::Forbidden("you must be joined to invite others".into()).into());
    }

    let now = chrono::Utc::now().timestamp();
    let mut count: u32 = 0;

    for invite in body.invites {
        let user_id = invite.user_id.trim().to_string();
        if user_id.is_empty() || user_id == auth.user_id {
            continue;
        }

        let role = normalize_member_role(&invite.role)?;
        if role == "host" {
            return Err(ApiError::BadRequest("invite role cannot be host".into()).into());
        }

        let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("invited user not found".into()))?;

        if room.room_mode == "video" {
            let item = rustfin_db::repo::items::get_item(&state.db, &room.item_id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .ok_or_else(|| ApiError::NotFound("item not found".into()))?;
            ensure_library_access_for_user(&state, &user.id, &user.role, &item.library_id).await?;
        } else if let Some(ref lib_id) = room.audio_library_id {
            ensure_library_access_for_user(&state, &user.id, &user.role, lib_id).await?;
        }

        rustfin_db::repo::watch_party::upsert_member(
            &state.db,
            &room_id,
            &rustfin_db::repo::watch_party::NewWatchPartyMember {
                user_id: user_id.clone(),
                role,
                status: "invited".to_string(),
                invited_by: Some(auth.user_id.clone()),
                invited_ts: Some(now),
                joined_ts: None,
            },
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        count += 1;
    }

    Ok(Json(serde_json::json!({ "ok": true, "invited": count })))
}

pub async fn list_audio_tracks(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(params): Query<AudioTracksQuery>,
) -> Result<Json<Vec<AudioTrackResponse>>, AppError> {
    let room = rustfin_db::repo::watch_party::get_room(&state.db, &room_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("watch party room not found".into()))?;

    let audio_lib_id = room
        .audio_library_id
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("this room is not an audio room".into()))?;

    ensure_library_access_for_user(&state, &auth.user_id, &auth.role, audio_lib_id).await?;

    let tracks = rustfin_db::repo::watch_party::get_library_tracks(
        &state.db,
        audio_lib_id,
        params.q.as_deref(),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // Convert local poster paths to API URLs
    let responses = tracks
        .into_iter()
        .map(|t| {
            let album_art_url = t.album_art_url.map(|url| {
                if url.starts_with("http://") || url.starts_with("https://") {
                    url
                } else {
                    // It's a local path — find the album item to get the proper API URL.
                    // We'll return a placeholder that can be constructed by the client.
                    // For now return the path as-is so the image API can serve it.
                    url
                }
            });
            AudioTrackResponse {
                id: t.id,
                title: t.title,
                artist: t.artist,
                album: t.album,
                album_art_url,
                duration_ms: t.duration_ms,
            }
        })
        .collect();

    Ok(Json(responses))
}
