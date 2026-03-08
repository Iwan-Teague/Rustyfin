use rustfin_core::{
    error::ApiError,
    servers_agent::{
        ServersAgentAckResponse, ServersAgentDiscoveryScanRequest,
        ServersAgentDiscoveryScanResponse, ServersAgentImportRequest, ServersAgentLifecycleRequest,
        ServersAgentLogsRequest, ServersAgentLogsResponse, ServersAgentProvisionRequest,
        ServersAgentStatusRequest, SystemdUnitStatus,
    },
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Clone, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiErrorBody {
    message: String,
}

fn map_agent_error(prefix: &str, status: reqwest::StatusCode, body: &[u8]) -> ApiError {
    let parsed = serde_json::from_slice::<ApiErrorEnvelope>(body).ok();
    let message = parsed
        .as_ref()
        .map(|v| v.error.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string());
    let message = if message.is_empty() {
        format!("{prefix}: status {status}")
    } else {
        format!("{prefix}: {message}")
    };
    if status == reqwest::StatusCode::BAD_REQUEST {
        ApiError::BadRequest(message)
    } else if status == reqwest::StatusCode::UNAUTHORIZED {
        ApiError::Unauthorized(message)
    } else if status == reqwest::StatusCode::FORBIDDEN {
        ApiError::Forbidden(message)
    } else {
        ApiError::Internal(message)
    }
}

async fn post_json<TReq: Serialize, TRes: for<'de> Deserialize<'de>>(
    state: &AppState,
    path: &str,
    body: &TReq,
    error_prefix: &str,
) -> Result<TRes, ApiError> {
    let base = state
        .servers_agent_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::BadRequest("servers agent URL is not configured".into()))?
        .trim_end_matches('/');
    let url = format!("{base}{path}");
    let client = reqwest::Client::new();
    let mut request = client.post(url).json(body);
    if let Some(token) = state
        .servers_agent_token
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        request = request.header("x-agent-token", token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("{error_prefix}: request failed: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.bytes().await.unwrap_or_default();
        return Err(map_agent_error(error_prefix, status, &body));
    }
    response
        .json::<TRes>()
        .await
        .map_err(|e| ApiError::Internal(format!("{error_prefix}: invalid response body: {e}")))
}

pub async fn query_unit_status(
    state: &AppState,
    unit_name: &str,
) -> Result<SystemdUnitStatus, ApiError> {
    post_json(
        state,
        "/v1/minecraft/status",
        &ServersAgentStatusRequest {
            unit_name: unit_name.to_string(),
        },
        "failed to query servers agent unit status",
    )
    .await
}

pub async fn run_lifecycle_action(
    state: &AppState,
    unit_name: &str,
    action: rustfin_core::servers_agent::ServerLifecycleAction,
) -> Result<(), ApiError> {
    let _: ServersAgentAckResponse = post_json(
        state,
        "/v1/minecraft/lifecycle",
        &ServersAgentLifecycleRequest {
            unit_name: unit_name.to_string(),
            action,
        },
        "failed to request servers agent lifecycle action",
    )
    .await?;
    Ok(())
}

pub async fn provision_managed_instance(
    state: &AppState,
    spec: &rustfin_core::servers_agent::ManagedProvisionSpec,
) -> Result<rustfin_core::servers_agent::ProvisioningResult, ApiError> {
    post_json(
        state,
        "/v1/minecraft/provision",
        &ServersAgentProvisionRequest { spec: spec.clone() },
        "failed to provision Minecraft server via servers agent",
    )
    .await
}

pub async fn import_existing_instance(
    state: &AppState,
    spec: &rustfin_core::servers_agent::ImportProvisionSpec,
) -> Result<rustfin_core::servers_agent::ProvisioningResult, ApiError> {
    post_json(
        state,
        "/v1/minecraft/import",
        &ServersAgentImportRequest { spec: spec.clone() },
        "failed to import Minecraft server via servers agent",
    )
    .await
}

pub async fn query_unit_logs(
    state: &AppState,
    unit_name: &str,
    limit: u32,
) -> Result<ServersAgentLogsResponse, ApiError> {
    post_json(
        state,
        "/v1/minecraft/logs",
        &ServersAgentLogsRequest {
            unit_name: unit_name.to_string(),
            limit: Some(limit),
        },
        "failed to load Minecraft server logs via servers agent",
    )
    .await
}

pub async fn scan_discovery_candidates(
    state: &AppState,
    root_path: Option<String>,
    limit: u32,
) -> Result<ServersAgentDiscoveryScanResponse, ApiError> {
    post_json(
        state,
        "/v1/minecraft/discovery/scan",
        &ServersAgentDiscoveryScanRequest {
            root_path,
            limit: Some(limit),
        },
        "failed to scan Minecraft discovery roots via servers agent",
    )
    .await
}
