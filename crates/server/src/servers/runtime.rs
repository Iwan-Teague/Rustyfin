pub use rustfin_core::servers_agent::{
    DiscoveryCandidate, ImportProvisionSpec, ManagedProvisionSpec, ProvisioningResult,
    ServerLifecycleAction, ServerLogLine, ServersAgentDiscoveryScanResponse,
    ServersAgentLogsResponse, SystemdUnitStatus,
};

use crate::state::AppState;

fn use_servers_agent(state: &AppState) -> bool {
    state
        .servers_agent_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

pub async fn run_lifecycle_action(
    state: &AppState,
    unit_name: &str,
    action: ServerLifecycleAction,
) -> Result<(), String> {
    if use_servers_agent(state) {
        super::agent_client::run_lifecycle_action(state, unit_name, action)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::run_lifecycle_action(unit_name, action).await
    }
}

pub async fn query_unit_status(
    state: &AppState,
    unit_name: &str,
) -> Result<SystemdUnitStatus, String> {
    if use_servers_agent(state) {
        super::agent_client::query_unit_status(state, unit_name)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::query_unit_status(unit_name).await
    }
}

pub async fn provision_managed_instance(
    state: &AppState,
    spec: &ManagedProvisionSpec,
) -> Result<ProvisioningResult, String> {
    if use_servers_agent(state) {
        super::agent_client::provision_managed_instance(state, spec)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::provision_managed_instance(spec).await
    }
}

pub async fn import_existing_instance(
    state: &AppState,
    spec: &ImportProvisionSpec,
) -> Result<ProvisioningResult, String> {
    if use_servers_agent(state) {
        super::agent_client::import_existing_instance(state, spec)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::import_existing_instance(spec).await
    }
}

pub async fn query_unit_logs(
    state: &AppState,
    unit_name: &str,
    limit: u32,
) -> Result<ServersAgentLogsResponse, String> {
    if use_servers_agent(state) {
        super::agent_client::query_unit_logs(state, unit_name, limit)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::query_unit_logs(unit_name, limit).await
    }
}

pub async fn scan_discovery_candidates(
    state: &AppState,
    root_path: Option<String>,
    limit: u32,
) -> Result<ServersAgentDiscoveryScanResponse, String> {
    if use_servers_agent(state) {
        super::agent_client::scan_discovery_candidates(state, root_path, limit)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::scan_discovery_candidates(root_path, limit).await
    }
}
