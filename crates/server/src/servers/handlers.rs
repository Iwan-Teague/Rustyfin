use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::state::AppState;

const ALLOWED_SERVER_DISTRIBUTIONS: &[&str] = &["vanilla", "paper"];
const ALLOWED_GAMEMODES: &[&str] = &["survival", "creative", "adventure", "spectator"];
const ALLOWED_DIFFICULTIES: &[&str] = &["peaceful", "easy", "normal", "hard"];

#[derive(Debug, Serialize)]
pub struct MinecraftServerResponse {
    pub id: String,
    pub display_name: String,
    pub slug: String,
    pub description: Option<String>,
    pub owner_user_id: String,
    pub owner_display_name: String,
    pub install_mode: String,
    pub runtime_mode: String,
    pub desired_state: String,
    pub observed_state: String,
    pub health_state: String,
    pub instance_root: String,
    pub server_work_dir: String,
    pub systemd_unit_name: String,
    pub listen_host: String,
    pub listen_port: i64,
    pub advertised_host: Option<String>,
    pub advertised_port: Option<i64>,
    pub autostart: bool,
    pub auto_stop_when_empty: bool,
    pub auto_stop_idle_minutes: Option<i64>,
    pub current_player_count: i64,
    pub max_player_count: Option<i64>,
    pub last_ready_ts: Option<i64>,
    pub last_started_ts: Option<i64>,
    pub last_stopped_ts: Option<i64>,
    pub last_exit_code: Option<i64>,
    pub last_error_summary: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub server_distribution: String,
    pub minecraft_version: String,
    pub world_name: String,
    pub gamemode: String,
    pub difficulty: String,
    pub max_memory_mb: i64,
    pub current_user_role: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServerInstanceEventResponse {
    pub id: String,
    pub instance_id: String,
    pub job_id: Option<String>,
    pub actor_user_id: Option<String>,
    pub level: String,
    pub event_kind: String,
    pub message: String,
    pub created_ts: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateMinecraftServerRequest {
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub server_distribution: String,
    pub minecraft_version: String,
    pub world_name: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: i64,
    #[serde(default = "default_gamemode")]
    pub gamemode: String,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
    #[serde(default)]
    pub hardcore: bool,
    #[serde(default)]
    pub motd: String,
    #[serde(default = "default_max_players")]
    pub max_player_count: i64,
    #[serde(default = "default_min_memory_mb")]
    pub min_memory_mb: i64,
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: i64,
    #[serde(default = "default_true")]
    pub online_mode: bool,
    #[serde(default = "default_true")]
    pub pvp: bool,
    #[serde(default)]
    pub allow_flight: bool,
    #[serde(default)]
    pub enable_command_block: bool,
    #[serde(default)]
    pub white_list_enabled: bool,
    #[serde(default)]
    pub autostart: bool,
    pub eula_accepted: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListServerEventsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug)]
struct ValidatedCreateMinecraftServer {
    display_name: String,
    description: String,
    server_distribution: String,
    minecraft_version: String,
    world_name: String,
    listen_port: i64,
    gamemode: String,
    difficulty: String,
    hardcore: bool,
    motd: String,
    max_player_count: i64,
    min_memory_mb: i64,
    max_memory_mb: i64,
    online_mode: bool,
    pvp: bool,
    allow_flight: bool,
    enable_command_block: bool,
    white_list_enabled: bool,
    autostart: bool,
    eula_accepted: bool,
}

fn default_listen_port() -> i64 {
    25565
}

fn default_gamemode() -> String {
    "survival".to_string()
}

fn default_difficulty() -> String {
    "normal".to_string()
}

fn default_max_players() -> i64 {
    20
}

fn default_min_memory_mb() -> i64 {
    1024
}

fn default_max_memory_mb() -> i64 {
    4096
}

fn default_true() -> bool {
    true
}

fn normalize_non_empty(value: &str, field: &str, max_len: usize) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest(format!("{field} is required")).into());
    }
    if trimmed.chars().count() > max_len {
        return Err(
            ApiError::BadRequest(format!("{field} must be {max_len} characters or fewer")).into(),
        );
    }
    Ok(trimmed.to_string())
}

fn normalize_choice(value: &str, field: &str, allowed: &[&str]) -> Result<String, AppError> {
    let normalized = value.trim().to_ascii_lowercase();
    if allowed.iter().any(|candidate| *candidate == normalized) {
        Ok(normalized)
    } else {
        Err(ApiError::BadRequest(format!("{field} must be one of: {}", allowed.join(", "))).into())
    }
}

fn normalize_version(value: &str) -> Result<String, AppError> {
    let normalized = normalize_non_empty(value, "minecraft_version", 32)?;
    if !normalized
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(ApiError::BadRequest(
            "minecraft_version contains unsupported characters".into(),
        )
        .into());
    }
    Ok(normalized)
}

fn validate_create_request(
    req: &CreateMinecraftServerRequest,
) -> Result<ValidatedCreateMinecraftServer, AppError> {
    let display_name = normalize_non_empty(&req.display_name, "display_name", 120)?;
    let description = req.description.trim().to_string();
    let server_distribution = normalize_choice(
        &req.server_distribution,
        "server_distribution",
        ALLOWED_SERVER_DISTRIBUTIONS,
    )?;
    let minecraft_version = normalize_version(&req.minecraft_version)?;
    let world_name = normalize_non_empty(&req.world_name, "world_name", 80)?;
    let gamemode = normalize_choice(&req.gamemode, "gamemode", ALLOWED_GAMEMODES)?;
    let difficulty = normalize_choice(&req.difficulty, "difficulty", ALLOWED_DIFFICULTIES)?;

    if req.listen_port < 1 || req.listen_port > 65_535 {
        return Err(ApiError::BadRequest("listen_port must be between 1 and 65535".into()).into());
    }
    if req.max_player_count < 1 || req.max_player_count > 500 {
        return Err(
            ApiError::BadRequest("max_player_count must be between 1 and 500".into()).into(),
        );
    }
    if req.min_memory_mb < 512 || req.min_memory_mb > 131_072 {
        return Err(
            ApiError::BadRequest("min_memory_mb must be between 512 and 131072".into()).into(),
        );
    }
    if req.max_memory_mb < 512 || req.max_memory_mb > 131_072 {
        return Err(
            ApiError::BadRequest("max_memory_mb must be between 512 and 131072".into()).into(),
        );
    }
    if req.max_memory_mb < req.min_memory_mb {
        return Err(ApiError::BadRequest(
            "max_memory_mb must be greater than or equal to min_memory_mb".into(),
        )
        .into());
    }
    if !req.eula_accepted {
        return Err(ApiError::BadRequest("Minecraft EULA acceptance is required".into()).into());
    }

    Ok(ValidatedCreateMinecraftServer {
        display_name,
        description,
        server_distribution,
        minecraft_version,
        world_name,
        listen_port: req.listen_port,
        gamemode,
        difficulty,
        hardcore: req.hardcore,
        motd: if req.motd.trim().is_empty() {
            req.display_name.trim().to_string()
        } else {
            normalize_non_empty(&req.motd, "motd", 180)?
        },
        max_player_count: req.max_player_count,
        min_memory_mb: req.min_memory_mb,
        max_memory_mb: req.max_memory_mb,
        online_mode: req.online_mode,
        pvp: req.pvp,
        allow_flight: req.allow_flight,
        enable_command_block: req.enable_command_block,
        white_list_enabled: req.white_list_enabled,
        autostart: req.autostart,
        eula_accepted: req.eula_accepted,
    })
}

fn row_to_response(row: rustfin_db::repo::servers::MinecraftServerRow) -> MinecraftServerResponse {
    MinecraftServerResponse {
        id: row.id,
        display_name: row.display_name,
        slug: row.slug,
        description: row.description,
        owner_user_id: row.owner_user_id,
        owner_display_name: row.owner_display_name,
        install_mode: row.install_mode,
        runtime_mode: row.runtime_mode,
        desired_state: row.desired_state,
        observed_state: row.observed_state,
        health_state: row.health_state,
        instance_root: row.instance_root,
        server_work_dir: row.server_work_dir,
        systemd_unit_name: row.systemd_unit_name,
        listen_host: row.listen_host,
        listen_port: row.listen_port,
        advertised_host: row.advertised_host,
        advertised_port: row.advertised_port,
        autostart: row.autostart,
        auto_stop_when_empty: row.auto_stop_when_empty,
        auto_stop_idle_minutes: row.auto_stop_idle_minutes,
        current_player_count: row.current_player_count,
        max_player_count: row.max_player_count,
        last_ready_ts: row.last_ready_ts,
        last_started_ts: row.last_started_ts,
        last_stopped_ts: row.last_stopped_ts,
        last_exit_code: row.last_exit_code,
        last_error_summary: row.last_error_summary,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
        server_distribution: row.server_distribution,
        minecraft_version: row.minecraft_version,
        world_name: row.world_name,
        gamemode: row.gamemode,
        difficulty: row.difficulty,
        max_memory_mb: row.max_memory_mb,
        current_user_role: row.current_user_role,
    }
}

pub async fn list_minecraft_servers(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<MinecraftServerResponse>>, AppError> {
    let rows = rustfin_db::repo::servers::list_accessible_minecraft_servers(
        &state.db,
        &auth.user_id,
        auth.role == "admin",
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(Json(rows.into_iter().map(row_to_response).collect()))
}

pub async fn get_minecraft_server(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<MinecraftServerResponse>, AppError> {
    let Some(row) = rustfin_db::repo::servers::get_accessible_minecraft_server(
        &state.db,
        &auth.user_id,
        auth.role == "admin",
        &id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    };
    Ok(Json(row_to_response(row)))
}

pub async fn list_minecraft_server_events(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(query): Query<ListServerEventsQuery>,
) -> Result<Json<Vec<ServerInstanceEventResponse>>, AppError> {
    let Some(_) = rustfin_db::repo::servers::get_accessible_minecraft_server(
        &state.db,
        &auth.user_id,
        auth.role == "admin",
        &id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    };

    let rows = rustfin_db::repo::servers::list_server_instance_events(
        &state.db,
        &id,
        query.limit.unwrap_or(20),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(|row| ServerInstanceEventResponse {
                id: row.id,
                instance_id: row.instance_id,
                job_id: row.job_id,
                actor_user_id: row.actor_user_id,
                level: row.level,
                event_kind: row.event_kind,
                message: row.message,
                created_ts: row.created_ts,
            })
            .collect(),
    ))
}

pub async fn create_minecraft_server(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateMinecraftServerRequest>,
) -> Result<(StatusCode, Json<MinecraftServerResponse>), AppError> {
    let validated = validate_create_request(&req)?;
    let row = rustfin_db::repo::servers::create_minecraft_server(
        &state.db,
        rustfin_db::repo::servers::CreateMinecraftServerParams {
            owner_user_id: &admin.user_id,
            created_by_user_id: &admin.user_id,
            display_name: &validated.display_name,
            description: if validated.description.trim().is_empty() {
                None
            } else {
                Some(validated.description.as_str())
            },
            server_distribution: &validated.server_distribution,
            minecraft_version: &validated.minecraft_version,
            world_name: &validated.world_name,
            listen_port: validated.listen_port,
            gamemode: &validated.gamemode,
            difficulty: &validated.difficulty,
            hardcore: validated.hardcore,
            motd: &validated.motd,
            max_player_count: Some(validated.max_player_count),
            min_memory_mb: validated.min_memory_mb,
            max_memory_mb: validated.max_memory_mb,
            online_mode: validated.online_mode,
            pvp: validated.pvp,
            allow_flight: validated.allow_flight,
            enable_command_block: validated.enable_command_block,
            white_list_enabled: validated.white_list_enabled,
            autostart: validated.autostart,
            eula_accepted: validated.eula_accepted,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let response = row_to_response(row.clone());

    crate::audit_log::record_event(
        &state,
        "servers.minecraft.create",
        json!({
            "instance_id": row.id,
            "display_name": row.display_name,
            "distribution": row.server_distribution,
            "minecraft_version": row.minecraft_version,
            "created_by": admin.username,
        }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(response)))
}

#[cfg(test)]
mod tests {
    use super::{
        CreateMinecraftServerRequest, default_difficulty, default_gamemode, default_listen_port,
        default_max_memory_mb, default_max_players, default_min_memory_mb, validate_create_request,
    };

    fn valid_request() -> CreateMinecraftServerRequest {
        CreateMinecraftServerRequest {
            display_name: "Family SMP".to_string(),
            description: "Main household server".to_string(),
            server_distribution: "paper".to_string(),
            minecraft_version: "1.21.1".to_string(),
            world_name: "family-world".to_string(),
            listen_port: default_listen_port(),
            gamemode: default_gamemode(),
            difficulty: default_difficulty(),
            hardcore: false,
            motd: "Welcome home".to_string(),
            max_player_count: default_max_players(),
            min_memory_mb: default_min_memory_mb(),
            max_memory_mb: default_max_memory_mb(),
            online_mode: true,
            pvp: true,
            allow_flight: false,
            enable_command_block: false,
            white_list_enabled: false,
            autostart: false,
            eula_accepted: true,
        }
    }

    #[test]
    fn validate_create_request_accepts_valid_payload() {
        let request = valid_request();
        let validated = validate_create_request(&request).expect("request should validate");
        assert_eq!(validated.display_name, "Family SMP");
        assert_eq!(validated.server_distribution, "paper");
        assert_eq!(validated.minecraft_version, "1.21.1");
        assert_eq!(validated.world_name, "family-world");
        assert_eq!(validated.listen_port, 25565);
        assert!(validated.eula_accepted);
    }

    #[test]
    fn validate_create_request_rejects_invalid_memory_ranges() {
        let mut request = valid_request();
        request.min_memory_mb = 8192;
        request.max_memory_mb = 4096;
        let error = validate_create_request(&request).expect_err("memory range should fail");
        let message = format!("{error:?}");
        assert!(message.contains("max_memory_mb"));
    }

    #[test]
    fn validate_create_request_requires_eula_acceptance() {
        let mut request = valid_request();
        request.eula_accepted = false;
        let error = validate_create_request(&request).expect_err("missing eula should fail");
        let message = format!("{error:?}");
        assert!(message.contains("EULA"));
    }
}
