use std::convert::Infallible;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use crate::ai_audit::{AiAssistantAuditEventResponse, parse_audit_event_row};
use crate::ai_storage::{
    AI_MODEL_DIR_SETTING_KEY, AiModelDirectoryState, AiSchedulerState, ModelPullChunk,
    current_model_dir, delete_model_file, download_model_from_url, list_models_with_storage_status,
    resolve_model_dir, resolve_runtime_model_dir, set_model_dir, validate_model_dir,
};
use crate::auth::AdminUser;
use crate::error::AppError;
use crate::state::AppState;

#[cfg(feature = "ai")]
use crate::ai_storage::{AiModelBenchmarkSummary, AiModelProfileSummary, AiRemoteBackendState};

pub const AI_REMOTE_BACKEND_SETTING_KEY: &str = "ai_remote_backend_config";

#[derive(Deserialize)]
pub struct UpdateAiAdminConfigRequest {
    pub model_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiRemoteBackendConfig {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default)]
    pub supports_prompt_cache: bool,
    #[serde(default)]
    pub supports_structured_output: bool,
    #[serde(default)]
    pub max_parallel_requests: u32,
    #[serde(default)]
    pub overload_fallback: bool,
    #[serde(default)]
    pub route_roles: Vec<String>,
}

impl AiRemoteBackendConfig {
    pub fn normalized(mut self) -> Self {
        self.base_url = self.base_url.trim().to_string();
        self.model = self.model.trim().to_string();
        self.route_roles = self
            .route_roles
            .into_iter()
            .map(|role| role.trim().to_ascii_lowercase())
            .filter(|role| !role.is_empty())
            .collect::<Vec<_>>();
        self.route_roles.sort();
        self.route_roles.dedup();
        if self.timeout_secs == 0 {
            self.timeout_secs = 120;
        }
        if self.max_parallel_requests == 0 {
            self.max_parallel_requests = 1;
        }
        self
    }

    pub fn should_route_planner_remote(&self) -> bool {
        self.enabled && !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }
}

#[cfg(feature = "ai")]
impl From<&AiRemoteBackendConfig> for rustfin_ai_agent::RemoteBackendConfig {
    fn from(value: &AiRemoteBackendConfig) -> Self {
        Self {
            base_url: value.base_url.clone(),
            model: value.model.clone(),
            api_key_env: value.api_key_env.clone(),
            timeout_secs: if value.timeout_secs == 0 {
                120
            } else {
                value.timeout_secs
            },
            supports_prompt_cache: value.supports_prompt_cache,
            supports_structured_output: value.supports_structured_output,
            max_parallel_requests: if value.max_parallel_requests == 0 {
                1
            } else {
                value.max_parallel_requests
            },
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateAiRemoteBackendRequest {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default)]
    pub supports_prompt_cache: bool,
    #[serde(default)]
    pub supports_structured_output: bool,
    #[serde(default)]
    pub max_parallel_requests: u32,
    #[serde(default)]
    pub overload_fallback: bool,
    #[serde(default)]
    pub route_roles: Vec<String>,
}

#[derive(Deserialize)]
pub struct RunAiBenchmarkRequest {
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub benchmark_label: Option<String>,
}

#[derive(Deserialize)]
pub struct PullAiModelRequest {
    pub url: String,
}

#[derive(Deserialize)]
pub struct ListAiAuditQuery {
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct ListAiTurnJournalQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiTurnJournalSummary {
    pub id: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_turn_index: Option<i64>,
    pub trace_id: String,
    pub request_message: String,
    pub model_name: String,
    pub response_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_mode: Option<String>,
    pub status: String,
    pub current_phase: String,
    pub history_len: i64,
    pub planner_debug: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_debug: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overload_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub compact_boundary_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_verification: Option<serde_json::Value>,
    pub created_ts: i64,
    pub updated_ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiCompactBoundarySummary {
    pub id: String,
    pub conversation_id: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub from_turn_index: i64,
    pub to_turn_index: i64,
    pub summarized_turn_count: i64,
    pub memory_state_json: String,
    pub created_ts: i64,
}

fn parse_turn_journal_row(
    row: rustfin_db::repo::ai_assistant_turn_journals::AiAssistantTurnJournalRow,
) -> AiTurnJournalSummary {
    AiTurnJournalSummary {
        id: row.id,
        user_id: row.user_id,
        conversation_id: row.conversation_id,
        request_turn_id: row.request_turn_id,
        request_turn_index: row.request_turn_index,
        trace_id: row.trace_id,
        request_message: row.request_message,
        model_name: row.model_name,
        response_mode: row.response_mode,
        planner_mode: row.planner_mode,
        status: row.status,
        current_phase: row.current_phase,
        history_len: row.history_len,
        planner_debug: serde_json::from_str(&row.planner_debug_json).unwrap_or_else(|_| json!({})),
        prompt_debug: row
            .prompt_debug_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        stats: row
            .metrics_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        overload_reason: row.overload_reason,
        error_message: row.error_message,
        compact_boundary_count: row.compact_boundary_count,
        artifact_verification: row
            .artifact_verification_json
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
        finished_ts: row.finished_ts,
    }
}

fn parse_compact_boundary_row(
    row: rustfin_db::repo::ai_compact_boundaries::AiConversationCompactBoundaryRow,
) -> AiCompactBoundarySummary {
    AiCompactBoundarySummary {
        id: row.id,
        conversation_id: row.conversation_id,
        user_id: row.user_id,
        trace_id: row.trace_id,
        from_turn_index: row.from_turn_index,
        to_turn_index: row.to_turn_index,
        summarized_turn_count: row.summarized_turn_count,
        memory_state_json: row.memory_state_json,
        created_ts: row.created_ts,
    }
}

pub async fn get_ai_admin_state(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    Ok(Json(build_ai_admin_state(&state).await?))
}

pub async fn update_ai_admin_config(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateAiAdminConfigRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    let trimmed = body.model_dir.trim();
    if trimmed.is_empty() {
        let _ = rustfin_db::repo::settings::delete(&state.db, AI_MODEL_DIR_SETTING_KEY)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let (resolved, _, _) = resolve_runtime_model_dir(&state.db).await?;
        set_model_dir(&state, resolved).await;
    } else {
        let validated = validate_model_dir(&PathBuf::from(trimmed))?;
        rustfin_db::repo::settings::set(
            &state.db,
            AI_MODEL_DIR_SETTING_KEY,
            validated.to_string_lossy().as_ref(),
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        set_model_dir(&state, validated).await;
    }

    crate::ai::clear_loaded_model_state(&state).await;
    Ok(Json(build_ai_admin_state(&state).await?))
}

#[cfg(feature = "ai")]
pub async fn update_ai_remote_backend(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateAiRemoteBackendRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    let config = AiRemoteBackendConfig {
        enabled: body.enabled,
        base_url: body.base_url,
        model: body.model,
        api_key_env: body.api_key_env,
        timeout_secs: body.timeout_secs,
        supports_prompt_cache: body.supports_prompt_cache,
        supports_structured_output: body.supports_structured_output,
        max_parallel_requests: body.max_parallel_requests,
        overload_fallback: body.overload_fallback,
        route_roles: body.route_roles,
    }
    .normalized();

    if config.should_route_planner_remote() {
        let remote = rustfin_ai_agent::RemoteBackendConfig::from(&config);
        remote
            .validate()
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        let serialized = serde_json::to_string(&config).map_err(|error| {
            ApiError::Internal(format!(
                "failed to serialize remote backend config: {error}"
            ))
        })?;
        rustfin_db::repo::settings::set(&state.db, AI_REMOTE_BACKEND_SETTING_KEY, &serialized)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let scheduler = {
            let guard = state.engine.lock().await;
            guard.scheduler.clone()
        };
        scheduler.set_remote_backend(Some(config));
    } else {
        let _ = rustfin_db::repo::settings::delete(&state.db, AI_REMOTE_BACKEND_SETTING_KEY)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let scheduler = {
            let guard = state.engine.lock().await;
            guard.scheduler.clone()
        };
        scheduler.set_remote_backend(None);
    }

    Ok(Json(build_ai_admin_state(&state).await?))
}

#[cfg(feature = "ai")]
pub async fn run_ai_model_benchmark(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<RunAiBenchmarkRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    crate::ai_benchmark::run_model_benchmarks(
        &state,
        body.model_name.as_deref(),
        body.benchmark_label.as_deref(),
    )
    .await?;
    Ok(Json(build_ai_admin_state(&state).await?))
}

#[cfg(not(feature = "ai"))]
pub async fn update_ai_remote_backend(
    _admin: AdminUser,
    _state: State<AppState>,
    _body: Json<UpdateAiRemoteBackendRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    Err(AppError::from(ApiError::Forbidden(
        "AI is unavailable on this build".into(),
    )))
}

#[cfg(not(feature = "ai"))]
pub async fn run_ai_model_benchmark(
    _admin: AdminUser,
    _state: State<AppState>,
    _body: Json<RunAiBenchmarkRequest>,
) -> Result<Json<AiModelDirectoryState>, AppError> {
    Err(AppError::from(ApiError::Forbidden(
        "AI is unavailable on this build".into(),
    )))
}

pub async fn pull_ai_model(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<PullAiModelRequest>,
) -> Response {
    let model_dir = current_model_dir(&state).await;
    let raw = download_model_from_url(body.url, model_dir, state.http.clone());
    let sse = raw.map(|chunk| {
        let event = match chunk {
            ModelPullChunk::Progress {
                status,
                bytes_done,
                bytes_total,
                percent,
            } => Event::default().event("progress").data(
                json!({
                    "status": status,
                    "bytes_done": bytes_done,
                    "bytes_total": bytes_total,
                    "percent": percent,
                })
                .to_string(),
            ),
            ModelPullChunk::Done => Event::default().event("done").data("{}"),
            ModelPullChunk::Error(message) => Event::default()
                .event("error")
                .data(json!({ "message": message }).to_string()),
        };
        Ok::<Event, Infallible>(event)
    });

    Sse::new(Box::pin(sse))
        .keep_alive(KeepAlive::default())
        .into_response()
}

pub async fn delete_ai_model(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = delete_model_file(&state, &name).await?;
    if deleted {
        crate::ai::clear_loaded_model_if_matching(&state, &name).await;
        return Ok(StatusCode::NO_CONTENT);
    }

    warn!(model = %name, "admin requested delete for missing AI model");
    Ok(StatusCode::NOT_FOUND)
}

pub async fn list_ai_audit_events(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiAuditQuery>,
) -> Result<Json<Vec<AiAssistantAuditEventResponse>>, AppError> {
    let rows = rustfin_db::repo::ai_assistant_audit::list_audit_events(
        &state.db,
        query.limit.unwrap_or(40),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_audit_event_row)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_ai_turn_journals(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiTurnJournalQuery>,
) -> Result<Json<Vec<AiTurnJournalSummary>>, AppError> {
    let rows = rustfin_db::repo::ai_assistant_turn_journals::list_recent_journals(
        &state.db,
        query.limit.unwrap_or(30),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_turn_journal_row)
            .collect::<Vec<_>>(),
    ))
}

pub async fn list_ai_compact_boundaries(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<ListAiTurnJournalQuery>,
) -> Result<Json<Vec<AiCompactBoundarySummary>>, AppError> {
    let rows = rustfin_db::repo::ai_compact_boundaries::list_recent_compact_boundaries(
        &state.db,
        query.limit.unwrap_or(20),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(
        rows.into_iter()
            .map(parse_compact_boundary_row)
            .collect::<Vec<_>>(),
    ))
}

#[cfg(feature = "ai")]
async fn build_ai_admin_state(state: &AppState) -> Result<AiModelDirectoryState, AppError> {
    let (configured_model_dir, configured_source) = resolve_model_dir(&state.db).await?;
    let model_dir = current_model_dir(state).await;
    let source = if configured_source == "default" && model_dir != configured_model_dir {
        "default_fallback".to_string()
    } else {
        configured_source
    };
    let (models, model_storage_available, model_storage_error) =
        list_models_with_storage_status(state).await;
    let scheduler = {
        let guard = state.engine.lock().await;
        guard.scheduler.clone()
    };
    let role_routing = {
        let guard = state.engine.lock().await;
        guard.role_routing.clone()
    };
    if let Some(remote) = load_remote_backend_config(state).await? {
        scheduler.set_remote_backend(Some(remote.clone()));
    }
    let scheduler_snapshot = scheduler.snapshot();
    let remote_backend = scheduler
        .remote_backend()
        .map(|config| AiRemoteBackendState {
            enabled: config.enabled,
            base_url: if config.enabled && !config.base_url.trim().is_empty() {
                Some(config.base_url)
            } else {
                None
            },
            model: if config.enabled && !config.model.trim().is_empty() {
                Some(config.model)
            } else {
                None
            },
            api_key_env: config.api_key_env,
            timeout_secs: config.timeout_secs,
            supports_prompt_cache: config.supports_prompt_cache,
            supports_structured_output: config.supports_structured_output,
            max_parallel_requests: config.max_parallel_requests,
            overload_fallback: config.overload_fallback,
            route_roles: config.route_roles,
        });
    let host_fingerprint = host_fingerprint();
    let model_benchmarks = list_model_benchmarks(&state.db, &host_fingerprint).await?;
    let model_profiles = list_model_profiles(&state.db, &host_fingerprint).await?;

    Ok(AiModelDirectoryState {
        available: crate::ai::inference_available(),
        model_dir: model_dir.to_string_lossy().to_string(),
        default_model_dir: crate::ai_storage::DEFAULT_AI_MODEL_DIR.to_string(),
        model_dir_source: source,
        model_storage_available,
        model_storage_error,
        audit_retention_days: crate::ai_audit::audit_retention_days(),
        audit_prune_interval_seconds: crate::ai_audit::AI_AUDIT_PRUNE_INTERVAL_SECS,
        models,
        remote_backend,
        scheduler: scheduler_state_from_snapshot(&scheduler_snapshot),
        model_benchmarks,
        model_profiles,
        role_routing,
    })
}

#[cfg(not(feature = "ai"))]
async fn build_ai_admin_state(state: &AppState) -> Result<AiModelDirectoryState, AppError> {
    let (configured_model_dir, configured_source) = resolve_model_dir(&state.db).await?;
    let model_dir = current_model_dir(state).await;
    let source = if configured_source == "default" && model_dir != configured_model_dir {
        "default_fallback".to_string()
    } else {
        configured_source
    };
    let (models, model_storage_available, model_storage_error) =
        list_models_with_storage_status(state).await;

    Ok(AiModelDirectoryState {
        available: crate::ai::inference_available(),
        model_dir: model_dir.to_string_lossy().to_string(),
        default_model_dir: crate::ai_storage::DEFAULT_AI_MODEL_DIR.to_string(),
        model_dir_source: source,
        model_storage_available,
        model_storage_error,
        audit_retention_days: crate::ai_audit::audit_retention_days(),
        audit_prune_interval_seconds: crate::ai_audit::AI_AUDIT_PRUNE_INTERVAL_SECS,
        models,
        remote_backend: None,
        scheduler: empty_scheduler_state(),
        model_benchmarks: Vec::new(),
        model_profiles: Vec::new(),
        role_routing: Vec::new(),
    })
}

#[cfg(feature = "ai")]
pub(crate) async fn load_remote_backend_config(
    state: &AppState,
) -> Result<Option<AiRemoteBackendConfig>, AppError> {
    let stored = rustfin_db::repo::settings::get(&state.db, AI_REMOTE_BACKEND_SETTING_KEY)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if let Some(raw) = stored {
        let parsed = serde_json::from_str::<AiRemoteBackendConfig>(&raw).map_err(|error| {
            ApiError::BadRequest(format!("invalid remote backend config: {error}"))
        })?;
        return Ok(Some(parsed.normalized()));
    }
    Ok(None)
}

#[cfg(feature = "ai")]
async fn list_model_benchmarks(
    pool: &rustfin_db::DbPool,
    host_fingerprint: &str,
) -> Result<Vec<AiModelBenchmarkSummary>, AppError> {
    let rows =
        rustfin_db::repo::ai_models::list_model_benchmarks_for_host(pool, host_fingerprint, 40)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|row| AiModelBenchmarkSummary {
            id: row.id,
            model_name: row.model_name,
            model_checksum: row.model_checksum,
            benchmark_label: row.benchmark_label,
            backend_kind: row.backend_kind,
            n_threads: row.n_threads,
            n_gpu_layers: row.n_gpu_layers,
            split_mode: row.split_mode,
            main_gpu: row.main_gpu,
            load_duration_ms: row.load_duration_ms,
            prefill_tokens: row.prefill_tokens,
            prefill_duration_ms: row.prefill_duration_ms,
            decode_tokens: row.decode_tokens,
            decode_duration_ms: row.decode_duration_ms,
            first_token_ms: row.first_token_ms,
            total_duration_ms: row.total_duration_ms,
            tokens_per_second: row.tokens_per_second,
            failure_message: row.failure_message,
            created_ts: row.created_ts,
            updated_ts: row.updated_ts,
        })
        .collect())
}

#[cfg(feature = "ai")]
async fn list_model_profiles(
    pool: &rustfin_db::DbPool,
    host_fingerprint: &str,
) -> Result<Vec<AiModelProfileSummary>, AppError> {
    let rows =
        rustfin_db::repo::ai_models::list_model_profiles_for_host(pool, host_fingerprint, 40)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(rows
        .into_iter()
        .map(|row| AiModelProfileSummary {
            id: row.id,
            model_name: row.model_name,
            model_checksum: row.model_checksum,
            context_window: row.context_window,
            preferred_completion_tokens: row.preferred_completion_tokens,
            planner_max_output: row.planner_max_output,
            summary_max_output: row.summary_max_output,
            safety_headroom: row.safety_headroom,
            warmup_cost_class: row.warmup_cost_class,
            supports_structured_output: row.supports_structured_output,
            supports_prompt_cache: row.supports_prompt_cache,
            recommended_n_threads: row.recommended_n_threads,
            recommended_n_gpu_layers: row.recommended_n_gpu_layers,
            recommended_split_mode: row.recommended_split_mode,
            recommended_main_gpu: row.recommended_main_gpu,
            estimated_model_bytes: row.estimated_model_bytes,
            last_benchmark_label: row.last_benchmark_label,
            last_load_duration_ms: row.last_load_duration_ms,
            last_tokens_per_second: row.last_tokens_per_second,
            benchmark_count: row.benchmark_count,
            created_ts: row.created_ts,
            updated_ts: row.updated_ts,
        })
        .collect())
}

#[cfg(feature = "ai")]
fn scheduler_state_from_snapshot(
    snapshot: &crate::ai_assistant::scheduler::SchedulerSnapshot,
) -> AiSchedulerState {
    AiSchedulerState {
        max_concurrent_turns: snapshot.max_concurrent_turns,
        queue_limit: snapshot.queue_limit,
        active_turns: snapshot.active_turns,
        queued_turns: snapshot.queued_turns,
        overload_state: snapshot.overload_state.clone(),
        warm_pool_bytes: snapshot.warm_pool_bytes,
        warm_pool_budget_bytes: snapshot.warm_pool_budget_bytes,
        active_by_priority: snapshot
            .active_by_priority
            .iter()
            .map(|count| crate::ai_storage::AiSchedulerPriorityCount {
                priority: count.priority.clone(),
                count: count.count,
            })
            .collect(),
        queued_by_priority: snapshot
            .queued_by_priority
            .iter()
            .map(|count| crate::ai_storage::AiSchedulerPriorityCount {
                priority: count.priority.clone(),
                count: count.count,
            })
            .collect(),
        warm_models: snapshot
            .warm_models
            .iter()
            .map(|model| crate::ai_storage::AiSchedulerWarmModel {
                model_name: model.model_name.clone(),
                estimated_bytes: model.estimated_bytes,
                loaded_ts_ms: model.loaded_ts_ms,
                last_used_ts_ms: model.last_used_ts_ms,
                load_count: model.load_count,
            })
            .collect(),
        rejected_turns_total: snapshot.rejected_turns_total,
        degraded_turns_total: snapshot.degraded_turns_total,
    }
}

#[cfg(not(feature = "ai"))]
fn empty_scheduler_state() -> AiSchedulerState {
    AiSchedulerState {
        max_concurrent_turns: 0,
        queue_limit: 0,
        active_turns: 0,
        queued_turns: 0,
        overload_state: "disabled".to_string(),
        warm_pool_bytes: 0,
        warm_pool_budget_bytes: 0,
        active_by_priority: Vec::new(),
        queued_by_priority: Vec::new(),
        warm_models: Vec::new(),
        rejected_turns_total: 0,
        degraded_turns_total: 0,
    }
}

#[cfg(feature = "ai")]
pub(crate) fn host_fingerprint() -> String {
    use sha2::{Digest, Sha256};

    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            std::fs::read_to_string("/var/lib/dbus/machine-id")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "unknown-machine-id".to_string())
        });
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".to_string());
    let mut hasher = Sha256::new();
    hasher.update(machine_id.as_bytes());
    hasher.update(b"|");
    hasher.update(hostname.as_bytes());
    format!("{:x}", hasher.finalize())
}
