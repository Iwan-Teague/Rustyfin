use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::{Duration, sleep};

use super::runtime::{
    ImportProvisionSpec, ManagedProvisionSpec, MinecraftRuntimeCapabilities, ProvisioningResult,
    ServerLifecycleAction, ServersAgentDiscoveryScanResponse, ServersAgentLogsResponse,
    SystemdUnitStatus, delete_managed_instance, import_existing_instance, probe_minecraft_server,
    provision_managed_instance, query_unit_logs, query_unit_status, run_lifecycle_action,
    runtime_capabilities, scan_discovery_candidates,
};
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
    pub java_path: String,
    pub world_name: String,
    pub gamemode: String,
    pub difficulty: String,
    pub hardcore: bool,
    pub motd: String,
    pub min_memory_mb: i64,
    pub max_memory_mb: i64,
    pub online_mode: bool,
    pub pvp: bool,
    pub allow_flight: bool,
    pub enable_command_block: bool,
    pub white_list_enabled: bool,
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

#[derive(Debug, Serialize)]
pub struct MinecraftServerActionResponse {
    pub job_id: String,
    pub requested_action: String,
    pub message: String,
    pub instance: MinecraftServerResponse,
}

#[derive(Debug, Serialize)]
pub struct MinecraftServerOperationResponse {
    pub job_id: String,
    pub message: String,
    pub instance: MinecraftServerResponse,
}

#[derive(Debug, Serialize)]
pub struct MinecraftServerDeleteResponse {
    pub deleted_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct MinecraftRuntimeCapabilitiesResponse {
    pub host_mode: String,
    pub status_supported: bool,
    pub lifecycle_supported: bool,
    pub provision_supported: bool,
    pub import_supported: bool,
    pub delete_supported: bool,
    pub reason: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct ListServerLogsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryScanQuery {
    pub root_path: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ImportMinecraftServerRequest {
    pub source_path: String,
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

#[derive(Debug)]
struct RuntimeProjection {
    install_mode: Option<String>,
    desired_state: String,
    observed_state: String,
    health_state: String,
    current_player_count: i64,
    max_player_count: Option<i64>,
    last_ready_ts: Option<i64>,
    last_started_ts: Option<i64>,
    last_stopped_ts: Option<i64>,
    last_exit_code: Option<i64>,
    last_error_summary: Option<String>,
}

fn can_control_server(
    auth: &AuthUser,
    server: &rustfin_db::repo::servers::MinecraftServerRow,
) -> bool {
    auth.role == "admin"
        || auth.user_id == server.owner_user_id
        || matches!(server.current_user_role.as_deref(), Some("manager"))
}

fn requires_provisioning_before_lifecycle(
    server: &rustfin_db::repo::servers::MinecraftServerRow,
) -> bool {
    matches!(
        server.observed_state.as_str(),
        "draft" | "unprovisioned" | "provisioning" | "importing"
    )
}

fn can_auto_provision_before_start(
    server: &rustfin_db::repo::servers::MinecraftServerRow,
    action: ServerLifecycleAction,
) -> bool {
    action == ServerLifecycleAction::Start
        && server.install_mode == "managed"
        && matches!(server.observed_state.as_str(), "draft" | "unprovisioned")
}

fn capabilities_to_response(
    capabilities: MinecraftRuntimeCapabilities,
) -> MinecraftRuntimeCapabilitiesResponse {
    MinecraftRuntimeCapabilitiesResponse {
        host_mode: capabilities.host_mode.to_string(),
        status_supported: capabilities.status_supported,
        lifecycle_supported: capabilities.lifecycle_supported,
        provision_supported: capabilities.provision_supported,
        import_supported: capabilities.import_supported,
        delete_supported: capabilities.delete_supported,
        reason: capabilities.reason,
    }
}

fn build_managed_provision_spec(
    server: &rustfin_db::repo::servers::MinecraftServerRow,
) -> ManagedProvisionSpec {
    ManagedProvisionSpec {
        instance_id: server.id.clone(),
        display_name: server.display_name.clone(),
        install_mode: server.install_mode.clone(),
        instance_root: server.instance_root.clone(),
        server_work_dir: server.server_work_dir.clone(),
        systemd_unit_name: server.systemd_unit_name.clone(),
        listen_host: server.listen_host.clone(),
        listen_port: server.listen_port,
        autostart: server.autostart,
        server_distribution: server.server_distribution.clone(),
        minecraft_version: server.minecraft_version.clone(),
        java_path: server.java_path.clone(),
        world_name: server.world_name.clone(),
        gamemode: server.gamemode.clone(),
        difficulty: server.difficulty.clone(),
        hardcore: server.hardcore,
        motd: server.motd.clone(),
        min_memory_mb: server.min_memory_mb,
        max_memory_mb: server.max_memory_mb,
        online_mode: server.online_mode,
        pvp: server.pvp,
        allow_flight: server.allow_flight,
        enable_command_block: server.enable_command_block,
        white_list_enabled: server.white_list_enabled,
    }
}

fn apply_runtime_status_projection(
    current: &rustfin_db::repo::servers::MinecraftServerRow,
    status: &SystemdUnitStatus,
    probe: Option<&super::runtime::MinecraftServerProbe>,
) -> RuntimeProjection {
    let now = chrono::Utc::now().timestamp();
    let mut projection = RuntimeProjection {
        install_mode: None,
        desired_state: current.desired_state.clone(),
        observed_state: current.observed_state.clone(),
        health_state: current.health_state.clone(),
        current_player_count: 0,
        max_player_count: current.max_player_count,
        last_ready_ts: current.last_ready_ts,
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: status.exec_main_status.or(current.last_exit_code),
        last_error_summary: current.last_error_summary.clone(),
    };

    if status.load_state == "not-found" {
        projection.observed_state = "unprovisioned".to_string();
        projection.health_state = "unknown".to_string();
        projection.current_player_count = 0;
        projection.last_error_summary = Some(format!(
            "Native systemd unit {} was not found on the host",
            current.systemd_unit_name
        ));
        return projection;
    }

    match status.active_state.as_str() {
        "active" if status.sub_state == "running" => {
            projection.observed_state = "running".to_string();
            projection.last_started_ts = Some(
                current
                    .last_started_ts
                    .filter(|_| current.observed_state == "running")
                    .unwrap_or(now),
            );
            if let Some(probe) = probe {
                projection.health_state = "healthy".to_string();
                projection.current_player_count = probe.online_players.max(0);
                projection.max_player_count = probe.max_players.or(current.max_player_count);
                projection.last_ready_ts = Some(now);
                projection.last_error_summary = None;
            } else {
                projection.health_state = "pending".to_string();
                projection.current_player_count = 0;
                projection.last_error_summary = Some(
                    "Minecraft server process is running but the status probe is not ready yet"
                        .to_string(),
                );
            }
        }
        "activating" => {
            projection.observed_state = "starting".to_string();
            projection.health_state = "pending".to_string();
            projection.current_player_count = 0;
            projection.last_error_summary = None;
        }
        "deactivating" => {
            projection.observed_state = "stopping".to_string();
            projection.health_state = "pending".to_string();
            projection.current_player_count = 0;
            projection.last_error_summary = None;
        }
        "reloading" => {
            projection.observed_state = "restarting".to_string();
            projection.health_state = "pending".to_string();
            projection.current_player_count = 0;
            projection.last_error_summary = None;
        }
        "inactive" => {
            projection.observed_state = "stopped".to_string();
            projection.health_state = "idle".to_string();
            projection.current_player_count = 0;
            projection.last_stopped_ts = Some(
                current
                    .last_stopped_ts
                    .filter(|_| current.observed_state == "stopped")
                    .unwrap_or(now),
            );
            projection.last_error_summary = None;
        }
        "failed" => {
            projection.observed_state = "failed".to_string();
            projection.health_state = "error".to_string();
            projection.current_player_count = 0;
            projection.last_stopped_ts = Some(now);
            projection.last_error_summary = Some(format!(
                "systemd reported result={} sub_state={} unit_state={}",
                status.result, status.sub_state, status.unit_file_state
            ));
        }
        other => {
            projection.observed_state = other.to_string();
            projection.health_state = "unknown".to_string();
            projection.current_player_count = 0;
            projection.last_error_summary = Some(format!(
                "systemd reported active_state={} sub_state={} unit_state={}",
                status.active_state, status.sub_state, status.unit_file_state
            ));
        }
    }

    projection
}

fn apply_runtime_error_projection(
    current: &rustfin_db::repo::servers::MinecraftServerRow,
    error: &str,
) -> RuntimeProjection {
    RuntimeProjection {
        install_mode: None,
        desired_state: current.desired_state.clone(),
        observed_state: current.observed_state.clone(),
        health_state: "error".to_string(),
        current_player_count: 0,
        max_player_count: current.max_player_count,
        last_ready_ts: current.last_ready_ts,
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: current.last_exit_code,
        last_error_summary: Some(error.to_string()),
    }
}

async fn persist_runtime_projection(
    state: &AppState,
    current: &rustfin_db::repo::servers::MinecraftServerRow,
    projection: &RuntimeProjection,
) -> Result<rustfin_db::repo::servers::MinecraftServerRow, AppError> {
    rustfin_db::repo::servers::update_minecraft_server_runtime(
        &state.db,
        &current.id,
        rustfin_db::repo::servers::UpdateMinecraftServerRuntimeParams {
            install_mode: projection.install_mode.as_deref(),
            desired_state: &projection.desired_state,
            observed_state: &projection.observed_state,
            health_state: &projection.health_state,
            current_player_count: projection.current_player_count,
            max_player_count: projection.max_player_count,
            last_ready_ts: projection.last_ready_ts,
            last_started_ts: projection.last_started_ts,
            last_stopped_ts: projection.last_stopped_ts,
            last_exit_code: projection.last_exit_code,
            last_error_summary: projection.last_error_summary.as_deref(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    rustfin_db::repo::servers::get_minecraft_server_by_id(&state.db, &current.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("server instance not found".into()).into())
}

async fn refresh_runtime_status(
    state: &AppState,
    current: &rustfin_db::repo::servers::MinecraftServerRow,
) -> Result<rustfin_db::repo::servers::MinecraftServerRow, AppError> {
    let projection = match query_unit_status(state, &current.systemd_unit_name).await {
        Ok(status) => {
            let probe = if status.active_state == "active" && status.sub_state == "running" {
                let port = u16::try_from(current.listen_port)
                    .map_err(|_| ApiError::Internal("server listen port is out of range".into()))?;
                probe_minecraft_server(state, &current.listen_host, port)
                    .await
                    .ok()
            } else {
                None
            };
            apply_runtime_status_projection(current, &status, probe.as_ref())
        }
        Err(error) => apply_runtime_error_projection(current, &error),
    };

    persist_runtime_projection(state, current, &projection).await
}

async fn record_server_event(
    state: &AppState,
    instance_id: &str,
    job_id: Option<&str>,
    actor_user_id: Option<&str>,
    level: &str,
    event_kind: &str,
    message: &str,
) {
    let _ = rustfin_db::repo::servers::create_server_instance_event(
        &state.db,
        rustfin_db::repo::servers::CreateServerInstanceEventParams {
            instance_id,
            job_id,
            actor_user_id,
            level,
            event_kind,
            message,
            details_json: None,
        },
    )
    .await;
}

async fn run_server_lifecycle_job(
    state: AppState,
    instance_id: String,
    unit_name: String,
    job_id: String,
    action: ServerLifecycleAction,
    actor_user_id: String,
) {
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job_id, "running", 0.2, None).await;
    record_server_event(
        &state,
        &instance_id,
        Some(&job_id),
        Some(&actor_user_id),
        "info",
        "lifecycle_action_started",
        &format!("{} requested for unit {}.", action.as_str(), unit_name),
    )
    .await;

    let command_result = run_lifecycle_action(&state, &unit_name, action).await;
    sleep(Duration::from_millis(350)).await;

    let refreshed = match rustfin_db::repo::servers::get_minecraft_server_by_id(
        &state.db,
        &instance_id,
    )
    .await
    {
        Ok(Some(current)) => refresh_runtime_status(&state, &current).await.ok(),
        _ => None,
    };

    match command_result {
        Ok(()) => {
            let message = if let Some(server) = refreshed.as_ref() {
                format!(
                    "{} completed. Observed state is now {}.",
                    action.as_str(),
                    server.observed_state
                )
            } else {
                format!("{} completed.", action.as_str())
            };
            record_server_event(
                &state,
                &instance_id,
                Some(&job_id),
                Some(&actor_user_id),
                "info",
                "lifecycle_action_completed",
                &message,
            )
            .await;
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "completed",
                1.0,
                None,
            )
            .await;
        }
        Err(error) => {
            if let Ok(Some(current)) =
                rustfin_db::repo::servers::get_minecraft_server_by_id(&state.db, &instance_id).await
            {
                let projection = apply_runtime_error_projection(&current, &error);
                let _ = persist_runtime_projection(&state, &current, &projection).await;
            }

            record_server_event(
                &state,
                &instance_id,
                Some(&job_id),
                Some(&actor_user_id),
                "error",
                "lifecycle_action_failed",
                &format!("{} failed: {}", action.as_str(), error),
            )
            .await;
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&error),
            )
            .await;
        }
    }
}

async fn apply_provisioning_success(
    state: &AppState,
    current: &rustfin_db::repo::servers::MinecraftServerRow,
    result: &ProvisioningResult,
) -> Result<rustfin_db::repo::servers::MinecraftServerRow, AppError> {
    let projection = RuntimeProjection {
        install_mode: Some(result.install_mode.clone()),
        desired_state: "stopped".to_string(),
        observed_state: "stopped".to_string(),
        health_state: "ready".to_string(),
        current_player_count: 0,
        max_player_count: current.max_player_count,
        last_ready_ts: Some(chrono::Utc::now().timestamp()),
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: current.last_exit_code,
        last_error_summary: None,
    };
    persist_runtime_projection(state, current, &projection).await
}

async fn run_managed_provision_job(
    state: AppState,
    instance_id: String,
    job_id: String,
    actor_user_id: String,
) {
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job_id, "running", 0.1, None).await;
    record_server_event(
        &state,
        &instance_id,
        Some(&job_id),
        Some(&actor_user_id),
        "info",
        "provision_started",
        "Managed Minecraft provisioning started.",
    )
    .await;

    let current = match rustfin_db::repo::servers::get_minecraft_server_by_id(
        &state.db,
        &instance_id,
    )
    .await
    {
        Ok(Some(server)) => server,
        Ok(None) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some("server instance not found"),
            )
            .await;
            return;
        }
        Err(error) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&format!("db error: {error}")),
            )
            .await;
            return;
        }
    };

    let spec = build_managed_provision_spec(&current);
    let result = provision_managed_instance(&state, &spec).await;

    match result {
        Ok(result) => match apply_provisioning_success(&state, &current, &result).await {
            Ok(updated) => {
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "info",
                    "provision_completed",
                    &format!(
                        "Managed server files and unit were provisioned at {}.",
                        result.work_dir
                    ),
                )
                .await;
                crate::audit_log::record_event(
                    &state,
                    "servers.minecraft.provision.complete",
                    json!({
                        "instance_id": updated.id,
                        "display_name": updated.display_name,
                        "systemd_unit_name": updated.systemd_unit_name,
                        "server_work_dir": result.work_dir,
                    }),
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "completed",
                    1.0,
                    None,
                )
                .await;
            }
            Err(error) => {
                let message =
                    format!("provision succeeded but state persistence failed: {error:?}");
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "error",
                    "provision_persist_failed",
                    &message,
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "failed",
                    1.0,
                    Some(&message),
                )
                .await;
            }
        },
        Err(error) => {
            let projection = RuntimeProjection {
                install_mode: None,
                desired_state: current.desired_state.clone(),
                observed_state: "failed".to_string(),
                health_state: "error".to_string(),
                current_player_count: current.current_player_count,
                max_player_count: current.max_player_count,
                last_ready_ts: current.last_ready_ts,
                last_started_ts: current.last_started_ts,
                last_stopped_ts: current.last_stopped_ts,
                last_exit_code: current.last_exit_code,
                last_error_summary: Some(error.clone()),
            };
            let _ = persist_runtime_projection(&state, &current, &projection).await;
            record_server_event(
                &state,
                &instance_id,
                Some(&job_id),
                Some(&actor_user_id),
                "error",
                "provision_failed",
                &format!("Managed provisioning failed: {error}"),
            )
            .await;
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&error),
            )
            .await;
        }
    }
}

async fn run_managed_provision_then_start_job(
    state: AppState,
    instance_id: String,
    job_id: String,
    actor_user_id: String,
) {
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job_id, "running", 0.05, None).await;
    record_server_event(
        &state,
        &instance_id,
        Some(&job_id),
        Some(&actor_user_id),
        "info",
        "provision_started",
        "Managed Minecraft provisioning started before launch.",
    )
    .await;

    let current = match rustfin_db::repo::servers::get_minecraft_server_by_id(
        &state.db,
        &instance_id,
    )
    .await
    {
        Ok(Some(server)) => server,
        Ok(None) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some("server instance not found"),
            )
            .await;
            return;
        }
        Err(error) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&format!("db error: {error}")),
            )
            .await;
            return;
        }
    };

    let spec = build_managed_provision_spec(&current);
    let result = provision_managed_instance(&state, &spec).await;

    match result {
        Ok(result) => match apply_provisioning_success(&state, &current, &result).await {
            Ok(updated) => {
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "info",
                    "provision_completed",
                    &format!(
                        "Managed server files and unit were provisioned at {}.",
                        result.work_dir
                    ),
                )
                .await;
                crate::audit_log::record_event(
                    &state,
                    "servers.minecraft.provision.complete",
                    json!({
                        "instance_id": updated.id,
                        "display_name": updated.display_name,
                        "systemd_unit_name": updated.systemd_unit_name,
                        "server_work_dir": result.work_dir,
                        "requested_by": actor_user_id,
                    }),
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "running",
                    0.55,
                    Some("Provisioning complete; launching native Minecraft service"),
                )
                .await;
                run_server_lifecycle_job(
                    state,
                    instance_id,
                    updated.systemd_unit_name,
                    job_id,
                    ServerLifecycleAction::Start,
                    actor_user_id,
                )
                .await;
            }
            Err(error) => {
                let message =
                    format!("provision succeeded but state persistence failed: {error:?}");
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "error",
                    "provision_persist_failed",
                    &message,
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "failed",
                    1.0,
                    Some(&message),
                )
                .await;
            }
        },
        Err(error) => {
            let projection = RuntimeProjection {
                install_mode: None,
                desired_state: "running".to_string(),
                observed_state: "failed".to_string(),
                health_state: "error".to_string(),
                current_player_count: current.current_player_count,
                max_player_count: current.max_player_count,
                last_ready_ts: current.last_ready_ts,
                last_started_ts: current.last_started_ts,
                last_stopped_ts: current.last_stopped_ts,
                last_exit_code: current.last_exit_code,
                last_error_summary: Some(error.clone()),
            };
            let _ = persist_runtime_projection(&state, &current, &projection).await;
            record_server_event(
                &state,
                &instance_id,
                Some(&job_id),
                Some(&actor_user_id),
                "error",
                "provision_failed",
                &format!("Managed provisioning failed before launch: {error}"),
            )
            .await;
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&error),
            )
            .await;
        }
    }
}

async fn run_import_job(
    state: AppState,
    instance_id: String,
    job_id: String,
    actor_user_id: String,
    source_path: String,
) {
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job_id, "running", 0.1, None).await;
    record_server_event(
        &state,
        &instance_id,
        Some(&job_id),
        Some(&actor_user_id),
        "info",
        "import_started",
        &format!("Importing existing Minecraft server from {}.", source_path),
    )
    .await;

    let current = match rustfin_db::repo::servers::get_minecraft_server_by_id(
        &state.db,
        &instance_id,
    )
    .await
    {
        Ok(Some(server)) => server,
        Ok(None) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some("server instance not found"),
            )
            .await;
            return;
        }
        Err(error) => {
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&format!("db error: {error}")),
            )
            .await;
            return;
        }
    };

    let managed = build_managed_provision_spec(&current);
    let spec = ImportProvisionSpec {
        managed,
        source_path: source_path.clone(),
    };
    let result = import_existing_instance(&state, &spec).await;

    match result {
        Ok(result) => match apply_provisioning_success(&state, &current, &result).await {
            Ok(updated) => {
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "info",
                    "import_completed",
                    &format!(
                        "Existing Minecraft server imported from {} into {}.",
                        source_path, result.work_dir
                    ),
                )
                .await;
                crate::audit_log::record_event(
                    &state,
                    "servers.minecraft.import.complete",
                    json!({
                        "instance_id": updated.id,
                        "display_name": updated.display_name,
                        "systemd_unit_name": updated.systemd_unit_name,
                        "import_source_path": source_path,
                        "server_work_dir": result.work_dir,
                    }),
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "completed",
                    1.0,
                    None,
                )
                .await;
            }
            Err(error) => {
                let message = format!("import succeeded but state persistence failed: {error:?}");
                record_server_event(
                    &state,
                    &instance_id,
                    Some(&job_id),
                    Some(&actor_user_id),
                    "error",
                    "import_persist_failed",
                    &message,
                )
                .await;
                let _ = rustfin_db::repo::jobs::update_job_status(
                    &state.db,
                    &job_id,
                    "failed",
                    1.0,
                    Some(&message),
                )
                .await;
            }
        },
        Err(error) => {
            let projection = RuntimeProjection {
                install_mode: None,
                desired_state: current.desired_state.clone(),
                observed_state: "failed".to_string(),
                health_state: "error".to_string(),
                current_player_count: current.current_player_count,
                max_player_count: current.max_player_count,
                last_ready_ts: current.last_ready_ts,
                last_started_ts: current.last_started_ts,
                last_stopped_ts: current.last_stopped_ts,
                last_exit_code: current.last_exit_code,
                last_error_summary: Some(error.clone()),
            };
            let _ = persist_runtime_projection(&state, &current, &projection).await;
            record_server_event(
                &state,
                &instance_id,
                Some(&job_id),
                Some(&actor_user_id),
                "error",
                "import_failed",
                &format!("Minecraft import failed: {error}"),
            )
            .await;
            let _ = rustfin_db::repo::jobs::update_job_status(
                &state.db,
                &job_id,
                "failed",
                1.0,
                Some(&error),
            )
            .await;
        }
    }
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
        java_path: row.java_path,
        world_name: row.world_name,
        gamemode: row.gamemode,
        difficulty: row.difficulty,
        hardcore: row.hardcore,
        motd: row.motd,
        min_memory_mb: row.min_memory_mb,
        max_memory_mb: row.max_memory_mb,
        online_mode: row.online_mode,
        pvp: row.pvp,
        allow_flight: row.allow_flight,
        enable_command_block: row.enable_command_block,
        white_list_enabled: row.white_list_enabled,
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

pub async fn get_minecraft_runtime_capabilities(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<MinecraftRuntimeCapabilitiesResponse>, AppError> {
    Ok(Json(capabilities_to_response(runtime_capabilities(&state))))
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

pub async fn refresh_minecraft_server_status(
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

    let refreshed = refresh_runtime_status(&state, &row).await?;
    Ok(Json(row_to_response(refreshed)))
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

pub async fn list_minecraft_server_logs(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Query(query): Query<ListServerLogsQuery>,
) -> Result<Json<ServersAgentLogsResponse>, AppError> {
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

    let limit = query.limit.unwrap_or(80).clamp(1, 500) as u32;
    let logs = query_unit_logs(&state, &row.systemd_unit_name, limit)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(logs))
}

pub async fn scan_minecraft_discovery_candidates(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(query): Query<DiscoveryScanQuery>,
) -> Result<Json<ServersAgentDiscoveryScanResponse>, AppError> {
    let root_path = query.root_path.and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let limit = query.limit.unwrap_or(64).clamp(1, 200) as u32;
    let response = scan_discovery_candidates(&state, root_path, limit)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(response))
}

pub async fn request_minecraft_server_action(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, action_name)): Path<(String, String)>,
) -> Result<(StatusCode, Json<MinecraftServerActionResponse>), AppError> {
    let action = ServerLifecycleAction::parse(&action_name).ok_or_else(|| {
        ApiError::BadRequest("action must be one of: start, stop, restart".into())
    })?;

    let Some(current) = rustfin_db::repo::servers::get_accessible_minecraft_server(
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

    if !can_control_server(&auth, &current) {
        return Err(ApiError::Forbidden(
            "you do not have permission to control this server".into(),
        )
        .into());
    }

    let auto_provision_before_start = can_auto_provision_before_start(&current, action);

    if requires_provisioning_before_lifecycle(&current) && !auto_provision_before_start {
        let message = match current.observed_state.as_str() {
            "provisioning" => {
                "this Minecraft server is still provisioning. Wait for provisioning to complete before starting it."
            }
            "importing" => {
                "this Minecraft server is still importing. Wait for import to complete before starting it."
            }
            _ => {
                "this Minecraft server has not been provisioned yet. Use Provision Managed Server or import an existing server first."
            }
        };
        return Err(ApiError::BadRequest(message.into()).into());
    }

    let job_payload = json!({
        "instance_id": current.id,
        "display_name": current.display_name,
        "requested_action": action.as_str(),
        "systemd_unit_name": current.systemd_unit_name,
    });
    let job = rustfin_db::repo::jobs::create_job(
        &state.db,
        &format!("servers.minecraft.{}", action.as_str()),
        Some(&job_payload.to_string()),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let transitional = RuntimeProjection {
        install_mode: None,
        desired_state: action.desired_state().to_string(),
        observed_state: if auto_provision_before_start {
            "provisioning".to_string()
        } else {
            action.transitional_observed_state().to_string()
        },
        health_state: "pending".to_string(),
        current_player_count: current.current_player_count,
        max_player_count: current.max_player_count,
        last_ready_ts: current.last_ready_ts,
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: current.last_exit_code,
        last_error_summary: None,
    };
    let updated = persist_runtime_projection(&state, &current, &transitional).await?;

    record_server_event(
        &state,
        &updated.id,
        Some(&job.id),
        Some(&auth.user_id),
        "info",
        "lifecycle_action_queued",
        &format!(
            "{} queued for unit {}{}",
            action.as_str(),
            updated.systemd_unit_name,
            if auto_provision_before_start {
                " after managed provisioning"
            } else {
                "."
            }
        ),
    )
    .await;

    crate::audit_log::record_event(
        &state,
        &format!("servers.minecraft.{}", action.as_str()),
        json!({
            "instance_id": updated.id,
            "display_name": updated.display_name,
            "requested_action": action.as_str(),
            "requested_by": auth.username,
        }),
    )
    .await;

    let state_clone = state.clone();
    let instance_id = updated.id.clone();
    let unit_name = updated.systemd_unit_name.clone();
    let job_id = job.id.clone();
    let actor_user_id = auth.user_id.clone();
    tokio::spawn(async move {
        if auto_provision_before_start {
            run_managed_provision_then_start_job(state_clone, instance_id, job_id, actor_user_id)
                .await;
        } else {
            run_server_lifecycle_job(
                state_clone,
                instance_id,
                unit_name,
                job_id,
                action,
                actor_user_id,
            )
            .await;
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(MinecraftServerActionResponse {
            job_id: job.id,
            requested_action: action.as_str().to_string(),
            message: if auto_provision_before_start {
                "Start requested. Rustyfin is provisioning the managed server and launching it now."
                    .to_string()
            } else {
                format!(
                    "{} requested. Rustyfin is reconciling the native systemd unit now.",
                    action.as_str()
                )
            },
            instance: row_to_response(updated),
        }),
    ))
}

pub async fn provision_minecraft_server(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<MinecraftServerOperationResponse>), AppError> {
    let Some(current) = rustfin_db::repo::servers::get_accessible_minecraft_server(
        &state.db,
        &admin.user_id,
        true,
        &id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    };

    let job_payload = json!({
        "instance_id": current.id,
        "display_name": current.display_name,
        "operation": "provision",
        "systemd_unit_name": current.systemd_unit_name,
    });
    let job = rustfin_db::repo::jobs::create_job(
        &state.db,
        "servers.minecraft.provision",
        Some(&job_payload.to_string()),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let transitional = RuntimeProjection {
        install_mode: None,
        desired_state: current.desired_state.clone(),
        observed_state: "provisioning".to_string(),
        health_state: "pending".to_string(),
        current_player_count: current.current_player_count,
        max_player_count: current.max_player_count,
        last_ready_ts: current.last_ready_ts,
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: current.last_exit_code,
        last_error_summary: None,
    };
    let updated = persist_runtime_projection(&state, &current, &transitional).await?;

    record_server_event(
        &state,
        &updated.id,
        Some(&job.id),
        Some(&admin.user_id),
        "info",
        "provision_queued",
        "Managed Minecraft provisioning queued.",
    )
    .await;

    crate::audit_log::record_event(
        &state,
        "servers.minecraft.provision",
        json!({
            "instance_id": updated.id,
            "display_name": updated.display_name,
            "requested_by": admin.username,
        }),
    )
    .await;

    let state_clone = state.clone();
    let instance_id = updated.id.clone();
    let job_id = job.id.clone();
    let actor_user_id = admin.user_id.clone();
    tokio::spawn(async move {
        run_managed_provision_job(state_clone, instance_id, job_id, actor_user_id).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(MinecraftServerOperationResponse {
            job_id: job.id,
            message: "Managed provisioning queued. Rustyfin is creating the server files and systemd unit now.".to_string(),
            instance: row_to_response(updated),
        }),
    ))
}

pub async fn import_minecraft_server(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(req): Json<ImportMinecraftServerRequest>,
) -> Result<(StatusCode, Json<MinecraftServerOperationResponse>), AppError> {
    let source_path = req.source_path.trim();
    if source_path.is_empty() {
        return Err(ApiError::BadRequest("source_path is required".into()).into());
    }

    let Some(current) = rustfin_db::repo::servers::get_accessible_minecraft_server(
        &state.db,
        &admin.user_id,
        true,
        &id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    };

    let job_payload = json!({
        "instance_id": current.id,
        "display_name": current.display_name,
        "operation": "import",
        "source_path": source_path,
        "systemd_unit_name": current.systemd_unit_name,
    });
    let job = rustfin_db::repo::jobs::create_job(
        &state.db,
        "servers.minecraft.import",
        Some(&job_payload.to_string()),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let transitional = RuntimeProjection {
        install_mode: None,
        desired_state: current.desired_state.clone(),
        observed_state: "importing".to_string(),
        health_state: "pending".to_string(),
        current_player_count: current.current_player_count,
        max_player_count: current.max_player_count,
        last_ready_ts: current.last_ready_ts,
        last_started_ts: current.last_started_ts,
        last_stopped_ts: current.last_stopped_ts,
        last_exit_code: current.last_exit_code,
        last_error_summary: None,
    };
    let updated = persist_runtime_projection(&state, &current, &transitional).await?;

    record_server_event(
        &state,
        &updated.id,
        Some(&job.id),
        Some(&admin.user_id),
        "info",
        "import_queued",
        &format!("Import queued for source path {}.", source_path),
    )
    .await;

    crate::audit_log::record_event(
        &state,
        "servers.minecraft.import",
        json!({
            "instance_id": updated.id,
            "display_name": updated.display_name,
            "requested_by": admin.username,
            "source_path": source_path,
        }),
    )
    .await;

    let state_clone = state.clone();
    let instance_id = updated.id.clone();
    let job_id = job.id.clone();
    let actor_user_id = admin.user_id.clone();
    let source_path_owned = source_path.to_string();
    tokio::spawn(async move {
        run_import_job(
            state_clone,
            instance_id,
            job_id,
            actor_user_id,
            source_path_owned,
        )
        .await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(MinecraftServerOperationResponse {
            job_id: job.id,
            message: "Import queued. Rustyfin is copying the existing server into its managed instance now.".to_string(),
            instance: row_to_response(updated),
        }),
    ))
}

pub async fn delete_minecraft_server(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<MinecraftServerDeleteResponse>, AppError> {
    let Some(current) = rustfin_db::repo::servers::get_accessible_minecraft_server(
        &state.db,
        &admin.user_id,
        true,
        &id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    else {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    };

    delete_managed_instance(&state, &current.systemd_unit_name, &current.instance_root)
        .await
        .map_err(ApiError::BadRequest)?;

    let deleted = rustfin_db::repo::servers::delete_minecraft_server(&state.db, &current.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !deleted {
        return Err(ApiError::NotFound("server instance not found".into()).into());
    }

    crate::audit_log::record_event(
        &state,
        "servers.minecraft.delete",
        json!({
            "instance_id": current.id,
            "display_name": current.display_name,
            "systemd_unit_name": current.systemd_unit_name,
            "deleted_by": admin.username,
        }),
    )
    .await;

    Ok(Json(MinecraftServerDeleteResponse {
        deleted_id: current.id,
        message: format!(
            "Deleted Minecraft server {} and removed its managed host files.",
            current.display_name
        ),
    }))
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
