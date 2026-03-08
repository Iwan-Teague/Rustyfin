pub use rustfin_core::servers_agent::{
    DiscoveryCandidate, ImportProvisionSpec, ManagedProvisionSpec, MinecraftServerProbe,
    ProvisioningResult, ServerLifecycleAction, ServerLogLine, ServersAgentDiscoveryScanResponse,
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

#[derive(Debug, Clone)]
pub struct MinecraftRuntimeCapabilities {
    pub host_mode: &'static str,
    pub status_supported: bool,
    pub lifecycle_supported: bool,
    pub provision_supported: bool,
    pub import_supported: bool,
    pub delete_supported: bool,
    pub reason: Option<String>,
}

pub fn runtime_capabilities(state: &AppState) -> MinecraftRuntimeCapabilities {
    if use_servers_agent(state) {
        MinecraftRuntimeCapabilities {
            host_mode: "agent",
            status_supported: true,
            lifecycle_supported: true,
            provision_supported: true,
            import_supported: true,
            delete_supported: true,
            reason: None,
        }
    } else {
        let caps = rustfin_servers_host::detect_native_runtime_capabilities();
        MinecraftRuntimeCapabilities {
            host_mode: "local",
            status_supported: caps.status_supported,
            lifecycle_supported: caps.lifecycle_supported,
            provision_supported: caps.provision_supported,
            import_supported: caps.import_supported,
            delete_supported: caps.delete_supported,
            reason: caps.reason,
        }
    }
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

pub async fn probe_minecraft_server(
    state: &AppState,
    host: &str,
    port: u16,
) -> Result<MinecraftServerProbe, String> {
    if use_servers_agent(state) {
        super::agent_client::probe_minecraft_server(state, host, port)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::probe_minecraft_server(host, port).await
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

pub async fn delete_managed_instance(
    state: &AppState,
    unit_name: &str,
    instance_root: &str,
) -> Result<(), String> {
    if use_servers_agent(state) {
        super::agent_client::delete_managed_instance(state, unit_name, instance_root)
            .await
            .map_err(|error| error.to_string())
    } else {
        rustfin_servers_host::delete_managed_instance(unit_name, instance_root).await
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
