use axum::Json;
use axum::extract::{Path, State};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

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

pub async fn list_inviteable_users(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<MinimalUser>>, AppError> {
    let users = rustfin_db::repo::users::list_users(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
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

#[derive(Debug, Deserialize)]
pub struct EligibleLibrariesRequest {
    #[serde(default)]
    pub user_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EligibleLibrariesResponse {
    pub library_ids: Vec<String>,
}

pub async fn eligible_libraries(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<EligibleLibrariesRequest>,
) -> Result<Json<EligibleLibrariesResponse>, AppError> {
    let mut all_user_ids = Vec::with_capacity(body.user_ids.len() + 1);
    all_user_ids.push(auth.user_id.clone());
    for user_id in body.user_ids {
        if user_id != auth.user_id {
            all_user_ids.push(user_id);
        }
    }

    let mut intersection: Option<std::collections::HashSet<String>> = None;
    for user_id in all_user_ids {
        let user = rustfin_db::repo::users::find_by_id(&state.db, &user_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::NotFound("user not found".into()))?;

        let current: std::collections::HashSet<String> = if user.role == "admin" {
            rustfin_db::repo::libraries::list_libraries(&state.db)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .into_iter()
                .map(|lib| lib.id)
                .collect()
        } else {
            rustfin_db::repo::users::get_library_access(&state.db, &user_id)
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

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_join_role() -> String {
    "viewer".to_string()
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomInvite {
    pub user_id: String,
    #[serde(default = "default_join_role")]
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub item_id: String,
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

pub async fn create_room(
    _auth: AuthUser,
    _state: State<AppState>,
    _body: Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, AppError> {
    Err(
        ApiError::BadRequest("watch party create endpoint is not fully configured yet".into())
            .into(),
    )
}

pub async fn get_room(
    _auth: AuthUser,
    _state: State<AppState>,
    _path: Path<String>,
) -> Result<Json<RoomResponse>, AppError> {
    Err(ApiError::NotFound("watch party room not found".into()).into())
}

pub async fn join_room(
    _auth: AuthUser,
    _state: State<AppState>,
    _path: Path<String>,
    _body: Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, AppError> {
    Err(ApiError::BadRequest("watch party join endpoint is not fully configured yet".into()).into())
}

pub async fn leave_room(
    _auth: AuthUser,
    _state: State<AppState>,
    _path: Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(
        ApiError::BadRequest("watch party leave endpoint is not fully configured yet".into())
            .into(),
    )
}

pub async fn end_room(
    _auth: AuthUser,
    _state: State<AppState>,
    _path: Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(ApiError::BadRequest("watch party end endpoint is not fully configured yet".into()).into())
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
    let updated = rustfin_db::repo::watch_party::set_member_status(
        &state.db,
        &room_id,
        &auth.user_id,
        "declined",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !updated {
        return Err(ApiError::NotFound("invite not found".into()).into());
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}
