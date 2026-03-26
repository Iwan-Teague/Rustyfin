//! Task 3J: Backend reconciliation loop for server status and auto-stop enforcement.

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use super::runtime::{query_unit_status, run_lifecycle_action};
use crate::state::AppState;
use rustfin_core::servers_agent::ServerLifecycleAction;
use rustfin_db::repo::servers::{CreateServerInstanceEventParams, UpdateMinecraftServerRuntimeParams};

const RECONCILE_INTERVAL_SECS: u64 = 60;
const STATUS_REFRESH_OLDER_THAN_SECS: i64 = 120;

pub async fn run_reconciler(state: AppState, shutdown: CancellationToken) {
    info!("servers reconciler started");

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                info!("servers reconciler shutting down");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(RECONCILE_INTERVAL_SECS)) => {
                if let Err(e) = reconcile_once(&state).await {
                    error!("reconciler tick failed: {e}");
                }
            }
        }
    }
}

async fn reconcile_once(state: &AppState) -> Result<(), String> {
    // 1. Refresh status for servers that haven't been updated recently
    refresh_stale_servers(state).await;

    // 2. Enforce auto-stop-when-empty
    enforce_auto_stop(state).await;

    Ok(())
}

async fn refresh_stale_servers(state: &AppState) {
    let servers = match rustfin_db::repo::servers::list_servers_needing_status_refresh(
        &state.db,
        STATUS_REFRESH_OLDER_THAN_SECS,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to list servers needing refresh: {e}");
            return;
        }
    };

    for server in servers {
        let status = match query_unit_status(state, &server.systemd_unit_name).await {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "failed to query status for {}: {e}",
                    server.systemd_unit_name
                );
                continue;
            }
        };

        let observed_state = match status.active_state.as_str() {
            "active" => "running",
            "activating" => "starting",
            "deactivating" => "stopping",
            "inactive" | "failed" => "stopped",
            _ => "unknown",
        };

        let health_state = if status.active_state == "active" {
            "healthy"
        } else if status.active_state == "failed" {
            "unhealthy"
        } else {
            "unknown"
        };

        let now = chrono::Utc::now().timestamp();

        if let Err(e) = rustfin_db::repo::servers::update_minecraft_server_runtime(
            &state.db,
            &server.id,
            UpdateMinecraftServerRuntimeParams {
                install_mode: None,
                desired_state: &server.desired_state,
                observed_state,
                health_state,
                current_player_count: server.current_player_count,
                max_player_count: server.max_player_count,
                last_ready_ts: server.last_ready_ts,
                last_started_ts: server.last_started_ts,
                last_stopped_ts: if observed_state == "stopped" && server.observed_state == "running" {
                    Some(now)
                } else {
                    server.last_stopped_ts
                },
                last_exit_code: server.last_exit_code,
                last_error_summary: server.last_error_summary.as_deref(),
            },
        )
        .await
        {
            warn!(
                "failed to update runtime state for {}: {e}",
                server.display_name
            );
        }
    }
}

async fn enforce_auto_stop(state: &AppState) {
    let servers = match rustfin_db::repo::servers::list_servers_for_auto_stop(&state.db).await {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to list servers for auto-stop: {e}");
            return;
        }
    };

    let now = chrono::Utc::now().timestamp();

    for server in servers {
        let idle_minutes = server.auto_stop_idle_minutes.unwrap_or(15);
        let last_ready = server.last_ready_ts.unwrap_or(0);
        let idle_since = now - last_ready;
        let idle_threshold_secs = idle_minutes * 60;

        if idle_since < idle_threshold_secs {
            continue;
        }

        info!(
            "auto-stopping {} (empty for {} minutes, threshold {} minutes)",
            server.display_name,
            idle_since / 60,
            idle_minutes
        );

        if let Err(e) =
            run_lifecycle_action(state, &server.systemd_unit_name, ServerLifecycleAction::Stop)
                .await
        {
            warn!("auto-stop failed for {}: {e}", server.display_name);
            continue;
        }

        // Update state to stopped
        let _ = rustfin_db::repo::servers::update_minecraft_server_runtime(
            &state.db,
            &server.id,
            UpdateMinecraftServerRuntimeParams {
                install_mode: None,
                desired_state: "stopped",
                observed_state: "stopped",
                health_state: "healthy",
                current_player_count: 0,
                max_player_count: server.max_player_count,
                last_ready_ts: server.last_ready_ts,
                last_started_ts: server.last_started_ts,
                last_stopped_ts: Some(now),
                last_exit_code: Some(0),
                last_error_summary: None,
            },
        )
        .await;

        // Record event
        let _ = rustfin_db::repo::servers::create_server_instance_event(
            &state.db,
            CreateServerInstanceEventParams {
                instance_id: &server.id,
                job_id: None,
                actor_user_id: None,
                level: "info",
                event_kind: "auto_stopped",
                message: &format!(
                    "Server auto-stopped after being empty for {} minutes.",
                    idle_since / 60
                ),
                details_json: None,
            },
        )
        .await;
    }
}
