use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use rustfin_core::{
    agent_auth::{normalize_secret, verify_agent_token},
    axum_error::AppError,
    error::ApiError,
    servers_agent::{
        MinecraftServerProbe, ServersAgentAckResponse, ServersAgentDeleteRequest,
        ServersAgentDiscoveryScanRequest, ServersAgentDiscoveryScanResponse,
        ServersAgentImportRequest, ServersAgentLifecycleRequest, ServersAgentLogsRequest,
        ServersAgentLogsResponse, ServersAgentProbeRequest, ServersAgentProvisionRequest,
        ServersAgentStatusRequest, SystemdUnitStatus,
    },
};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AgentState {
    token: Option<String>,
}

fn require_agent_auth(headers: &HeaderMap, state: &AgentState) -> Result<(), AppError> {
    verify_agent_token(headers, state.token.as_deref(), "servers agent").map_err(Into::into)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

async fn get_status(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentStatusRequest>,
) -> Result<Json<SystemdUnitStatus>, AppError> {
    require_agent_auth(&headers, &state)?;
    let status = rustfin_servers_host::query_unit_status(req.unit_name.trim())
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(status))
}

async fn run_lifecycle(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentLifecycleRequest>,
) -> Result<Json<ServersAgentAckResponse>, AppError> {
    require_agent_auth(&headers, &state)?;
    rustfin_servers_host::run_lifecycle_action(req.unit_name.trim(), req.action)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(ServersAgentAckResponse { ok: true }))
}

async fn probe(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentProbeRequest>,
) -> Result<Json<MinecraftServerProbe>, AppError> {
    require_agent_auth(&headers, &state)?;
    let result = rustfin_servers_host::probe_minecraft_server(req.host.trim(), req.port)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(result))
}

async fn provision(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentProvisionRequest>,
) -> Result<Json<rustfin_core::servers_agent::ProvisioningResult>, AppError> {
    require_agent_auth(&headers, &state)?;
    let result = rustfin_servers_host::provision_managed_instance(&req.spec)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(result))
}

async fn import(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentImportRequest>,
) -> Result<Json<rustfin_core::servers_agent::ProvisioningResult>, AppError> {
    require_agent_auth(&headers, &state)?;
    let result = rustfin_servers_host::import_existing_instance(&req.spec)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(result))
}

async fn delete_instance(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentDeleteRequest>,
) -> Result<Json<ServersAgentAckResponse>, AppError> {
    require_agent_auth(&headers, &state)?;
    rustfin_servers_host::delete_managed_instance(req.unit_name.trim(), req.instance_root.trim())
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(ServersAgentAckResponse { ok: true }))
}

async fn logs(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentLogsRequest>,
) -> Result<Json<ServersAgentLogsResponse>, AppError> {
    require_agent_auth(&headers, &state)?;
    let result =
        rustfin_servers_host::query_unit_logs(req.unit_name.trim(), req.limit.unwrap_or(80))
            .await
            .map_err(ApiError::BadRequest)?;
    Ok(Json(result))
}

async fn discovery_scan(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Json(req): Json<ServersAgentDiscoveryScanRequest>,
) -> Result<Json<ServersAgentDiscoveryScanResponse>, AppError> {
    require_agent_auth(&headers, &state)?;
    let result =
        rustfin_servers_host::scan_discovery_candidates(req.root_path, req.limit.unwrap_or(64))
            .await
            .map_err(ApiError::BadRequest)?;
    Ok(Json(result))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind =
        std::env::var("RUSTFIN_SERVERS_AGENT_BIND").unwrap_or_else(|_| "0.0.0.0:8103".to_string());
    let token = normalize_secret(std::env::var("RUSTFIN_SERVERS_AGENT_TOKEN").ok());

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/minecraft/status", post(get_status))
        .route("/v1/minecraft/probe", post(probe))
        .route("/v1/minecraft/lifecycle", post(run_lifecycle))
        .route("/v1/minecraft/provision", post(provision))
        .route("/v1/minecraft/import", post(import))
        .route("/v1/minecraft/delete", post(delete_instance))
        .route("/v1/minecraft/logs", post(logs))
        .route("/v1/minecraft/discovery/scan", post(discovery_scan))
        .with_state(AgentState { token });

    let addr: SocketAddr = bind.parse().context("invalid RUSTFIN_SERVERS_AGENT_BIND")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind servers agent")?;
    tracing::info!(%addr, "servers agent listening");
    axum::serve(listener, app)
        .await
        .context("servers agent failed")?;
    Ok(())
}
