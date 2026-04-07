use std::path::{Path, PathBuf};

use async_stream::stream;
use futures::Stream;
use rustfin_core::error::ApiError;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::ai_role_routing::AiRoleRoutingDecision;
use crate::error::AppError;
use crate::state::AppState;

pub const AI_MODEL_DIR_SETTING_KEY: &str = "ai_model_dir";
pub const DEFAULT_AI_MODEL_DIR: &str = "/var/lib/rustyfin/ai/models";

#[derive(Debug, Clone, Serialize)]
pub struct AiModelSummary {
    pub name: String,
    pub file: String,
    pub size_gb: f64,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    pub architecture: Option<String>,
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiRemoteBackendState {
    pub enabled: bool,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    pub timeout_secs: u64,
    pub supports_prompt_cache: bool,
    pub supports_structured_output: bool,
    pub max_parallel_requests: u32,
    pub overload_fallback: bool,
    pub route_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSchedulerPriorityCount {
    pub priority: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSchedulerWarmModel {
    pub model_name: String,
    pub estimated_bytes: u64,
    pub loaded_ts_ms: i64,
    pub last_used_ts_ms: i64,
    pub load_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSchedulerState {
    pub max_concurrent_turns: u64,
    pub queue_limit: u64,
    pub active_turns: u64,
    pub queued_turns: u64,
    pub overload_state: String,
    pub warm_pool_bytes: u64,
    pub warm_pool_budget_bytes: u64,
    pub active_by_priority: Vec<AiSchedulerPriorityCount>,
    pub queued_by_priority: Vec<AiSchedulerPriorityCount>,
    pub warm_models: Vec<AiSchedulerWarmModel>,
    pub rejected_turns_total: u64,
    pub degraded_turns_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiModelBenchmarkSummary {
    pub id: String,
    pub model_name: String,
    pub model_checksum: String,
    pub benchmark_label: String,
    pub backend_kind: String,
    pub n_threads: i32,
    pub n_gpu_layers: i32,
    pub split_mode: String,
    pub main_gpu: Option<i32>,
    pub load_duration_ms: i64,
    pub prefill_tokens: i64,
    pub prefill_duration_ms: i64,
    pub decode_tokens: i64,
    pub decode_duration_ms: i64,
    pub first_token_ms: i64,
    pub total_duration_ms: i64,
    pub tokens_per_second: f64,
    pub failure_message: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiModelProfileSummary {
    pub id: String,
    pub model_name: String,
    pub model_checksum: String,
    pub context_window: i32,
    pub preferred_completion_tokens: i32,
    pub planner_max_output: i32,
    pub summary_max_output: i32,
    pub safety_headroom: i32,
    pub warmup_cost_class: String,
    pub supports_structured_output: bool,
    pub supports_prompt_cache: bool,
    pub recommended_n_threads: i32,
    pub recommended_n_gpu_layers: i32,
    pub recommended_split_mode: String,
    pub recommended_main_gpu: Option<i32>,
    pub estimated_model_bytes: i64,
    pub last_benchmark_label: String,
    pub last_load_duration_ms: i64,
    pub last_tokens_per_second: f64,
    pub benchmark_count: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiModelDirectoryState {
    pub available: bool,
    pub model_dir: String,
    pub default_model_dir: String,
    pub model_dir_source: String,
    pub model_storage_available: bool,
    pub model_storage_error: Option<String>,
    pub audit_retention_days: i64,
    pub audit_prune_interval_seconds: u64,
    pub models: Vec<AiModelSummary>,
    pub remote_backend: Option<AiRemoteBackendState>,
    pub scheduler: AiSchedulerState,
    pub model_benchmarks: Vec<AiModelBenchmarkSummary>,
    pub model_profiles: Vec<AiModelProfileSummary>,
    pub role_routing: Vec<AiRoleRoutingDecision>,
}

#[derive(Debug, Clone)]
pub enum ModelPullChunk {
    Progress {
        status: String,
        bytes_done: u64,
        bytes_total: Option<u64>,
        percent: u8,
    },
    Done,
    Error(String),
}

pub async fn resolve_model_dir(pool: &rustfin_db::DbPool) -> Result<(PathBuf, String), AppError> {
    let setting = rustfin_db::repo::settings::get(pool, AI_MODEL_DIR_SETTING_KEY)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if let Some(path) = normalize_model_dir_value(setting.as_deref()) {
        return Ok((path, "database".to_string()));
    }

    let env = normalize_model_dir_value(std::env::var("RUSTFIN_AI_MODEL_DIR").ok().as_deref());
    if let Some(path) = env {
        return Ok((path, "environment".to_string()));
    }

    Ok((PathBuf::from(DEFAULT_AI_MODEL_DIR), "default".to_string()))
}

pub async fn resolve_runtime_model_dir(
    pool: &rustfin_db::DbPool,
) -> Result<(PathBuf, String, Option<String>), AppError> {
    let (configured_path, source) = resolve_model_dir(pool).await?;
    Ok(select_runtime_model_dir(
        configured_path,
        source,
        std::env::var_os("HOME").map(PathBuf::from),
    ))
}

pub async fn current_model_dir(state: &AppState) -> PathBuf {
    state.model_dir.read().await.clone()
}

pub async fn set_model_dir(state: &AppState, model_dir: PathBuf) {
    *state.model_dir.write().await = model_dir;
}

pub fn normalize_model_dir_value(value: Option<&str>) -> Option<PathBuf> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

pub fn model_dir_storage_status_from_result(
    result: Result<Vec<AiModelSummary>, AppError>,
) -> (Vec<AiModelSummary>, bool, Option<String>) {
    match result {
        Ok(models) => (models, true, None),
        Err(error) => (Vec::new(), false, Some(error.0.to_string())),
    }
}

pub fn validate_model_dir(path: &Path) -> Result<PathBuf, ApiError> {
    if path.as_os_str().is_empty() {
        return Err(ApiError::BadRequest("model directory is required".into()));
    }
    if !path.is_absolute() {
        return Err(ApiError::BadRequest(
            "model directory must be an absolute path".into(),
        ));
    }
    std::fs::create_dir_all(path)
        .map_err(|e| ApiError::BadRequest(format!("failed to create model directory: {e}")))?;
    Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

pub async fn list_models_from_state(state: &AppState) -> Result<Vec<AiModelSummary>, AppError> {
    let model_dir = current_model_dir(state).await;
    list_models_in_dir(&model_dir).await.map_err(Into::into)
}

pub async fn list_models_with_storage_status(
    state: &AppState,
) -> (Vec<AiModelSummary>, bool, Option<String>) {
    model_dir_storage_status_from_result(list_models_from_state(state).await)
}

pub async fn list_models_in_dir(model_dir: &Path) -> Result<Vec<AiModelSummary>, ApiError> {
    let model_dir = model_dir.to_path_buf();
    tokio::task::spawn_blocking(move || list_models_in_dir_blocking(&model_dir))
        .await
        .map_err(|e| ApiError::Internal(format!("list models task failed: {e}")))?
}

pub fn model_file_path(model_dir: &Path, name: &str) -> Result<PathBuf, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("model name is required".into()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(ApiError::BadRequest("invalid model name".into()));
    }

    let file_name = if trimmed.ends_with(".gguf") {
        trimmed.to_string()
    } else {
        format!("{trimmed}.gguf")
    };

    Ok(model_dir.join(file_name))
}

pub async fn delete_model_file(state: &AppState, name: &str) -> Result<bool, AppError> {
    let model_dir = current_model_dir(state).await;
    let target = model_file_path(&model_dir, name)?;
    match tokio::fs::remove_file(&target).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(ApiError::Internal(format!("failed to delete model: {e}")).into()),
    }
}

pub fn download_model_from_url(
    url: String,
    model_dir: PathBuf,
    client: reqwest::Client,
) -> impl Stream<Item = ModelPullChunk> + Send + 'static {
    stream! {
        let url = url.trim().to_string();
        if url.is_empty() {
            yield ModelPullChunk::Error("model URL is required".into());
            return;
        }

        let model_dir = match validate_model_dir(&model_dir) {
            Ok(path) => path,
            Err(error) => {
                yield ModelPullChunk::Error(error.to_string());
                return;
            }
        };

        let parsed = match reqwest::Url::parse(&url) {
            Ok(parsed) if parsed.scheme() == "http" || parsed.scheme() == "https" => parsed,
            Ok(_) => {
                yield ModelPullChunk::Error("model URL must use http or https".into());
                return;
            }
            Err(error) => {
                yield ModelPullChunk::Error(format!("invalid model URL: {error}"));
                return;
            }
        };

        let file_name = match derive_download_file_name(&parsed) {
            Ok(file_name) => file_name,
            Err(error) => {
                yield ModelPullChunk::Error(error.to_string());
                return;
            }
        };

        let part_path = model_dir.join(format!("{file_name}.part"));
        let final_path = model_dir.join(&file_name);

        let response = match client.get(parsed.clone()).send().await {
            Ok(response) => response,
            Err(error) => {
                yield ModelPullChunk::Error(format!("failed to request model: {error}"));
                return;
            }
        };

        if !response.status().is_success() {
            yield ModelPullChunk::Error(format!("model download returned HTTP {}", response.status()));
            return;
        }

        let bytes_total = response.content_length();
        let mut bytes_done: u64 = 0;
        let mut response = response;
        let mut file = match tokio::fs::File::create(&part_path).await {
            Ok(file) => file,
            Err(error) => {
                yield ModelPullChunk::Error(format!("failed to create partial model file: {error}"));
                return;
            }
        };

        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&part_path).await;
                    yield ModelPullChunk::Error(format!("model download failed: {error}"));
                    return;
                }
            };

            if let Err(error) = file.write_all(&chunk).await {
                let _ = tokio::fs::remove_file(&part_path).await;
                yield ModelPullChunk::Error(format!("failed to write model file: {error}"));
                return;
            }

            bytes_done = bytes_done.saturating_add(chunk.len() as u64);
            let percent = bytes_total
                .filter(|total| *total > 0)
                .map(|total| ((bytes_done.saturating_mul(100) / total).min(100)) as u8)
                .unwrap_or(0);

            yield ModelPullChunk::Progress {
                status: format!("Downloading {file_name}"),
                bytes_done,
                bytes_total,
                percent,
            };
        }

        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&part_path).await;
            yield ModelPullChunk::Error(format!("failed to finalize model file: {error}"));
            return;
        }

        if let Err(error) = tokio::fs::rename(&part_path, &final_path).await {
            let _ = tokio::fs::remove_file(&part_path).await;
            yield ModelPullChunk::Error(format!("failed to move completed model file: {error}"));
            return;
        }

        yield ModelPullChunk::Done;
    }
}

fn list_models_in_dir_blocking(model_dir: &Path) -> Result<Vec<AiModelSummary>, ApiError> {
    std::fs::create_dir_all(model_dir)
        .map_err(|e| ApiError::BadRequest(format!("failed to create model directory: {e}")))?;

    let mut models = Vec::new();
    let entries = std::fs::read_dir(model_dir)
        .map_err(|e| ApiError::Internal(format!("failed to read model directory: {e}")))?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            ApiError::Internal(format!("failed to read model directory entry: {e}"))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("gguf") {
            continue;
        }

        let metadata = entry
            .metadata()
            .map_err(|e| ApiError::Internal(format!("failed to read model metadata: {e}")))?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&file_name)
            .to_string();
        let size_gb = metadata.len() as f64 / (1024.0 * 1024.0 * 1024.0);

        models.push(AiModelSummary {
            name,
            file: file_name,
            size_gb,
            parameter_size: None,
            quantization: None,
            architecture: None,
            context_length: None,
        });
    }

    models.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(models)
}

fn derive_download_file_name(url: &reqwest::Url) -> Result<String, ApiError> {
    let file_name = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .ok_or_else(|| ApiError::BadRequest("model URL must include a filename".into()))?;

    let file_name = file_name.trim();
    if file_name.is_empty() || file_name.contains('/') || file_name.contains('\\') {
        return Err(ApiError::BadRequest("invalid model filename in URL".into()));
    }
    if !file_name.ends_with(".gguf") {
        return Err(ApiError::BadRequest(
            "model URL must point to a .gguf file".into(),
        ));
    }

    Ok(file_name.to_string())
}

fn select_runtime_model_dir(
    configured_path: PathBuf,
    source: String,
    home_dir: Option<PathBuf>,
) -> (PathBuf, String, Option<String>) {
    match ensure_model_dir_ready(&configured_path) {
        Ok(path) => (path, source, None),
        Err(error) if source == "default" => {
            if let Some(fallback) = fallback_model_dir(home_dir.as_deref()) {
                match ensure_model_dir_ready(&fallback) {
                    Ok(path) => (
                        path,
                        "default_fallback".to_string(),
                        Some(format!(
                            "default model directory {} is not writable ({error}); using fallback {}",
                            configured_path.display(),
                            fallback.display()
                        )),
                    ),
                    Err(fallback_error) => (
                        configured_path.clone(),
                        source,
                        Some(format!(
                            "default model directory {} is not writable ({error}); fallback {} also failed ({fallback_error})",
                            configured_path.display(),
                            fallback.display()
                        )),
                    ),
                }
            } else {
                (
                    configured_path.clone(),
                    source,
                    Some(format!(
                        "default model directory {} is not writable ({error}) and no HOME-based fallback is available",
                        configured_path.display()
                    )),
                )
            }
        }
        Err(error) => (
            configured_path.clone(),
            source,
            Some(format!(
                "configured model directory {} is not writable ({error})",
                configured_path.display()
            )),
        ),
    }
}

fn ensure_model_dir_ready(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn fallback_model_dir(home_dir: Option<&Path>) -> Option<PathBuf> {
    let home_dir = home_dir?;
    Some(home_dir.join(".local/share/rustyfin/ai/models"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn select_runtime_model_dir_prefers_configured_path_when_writable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let configured = temp.path().join("models");
        let (path, source, warning) =
            select_runtime_model_dir(configured.clone(), "default".to_string(), None);
        let expected = configured
            .canonicalize()
            .unwrap_or_else(|_| configured.clone());
        assert_eq!(path, expected);
        assert_eq!(source, "default");
        assert!(warning.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn select_runtime_model_dir_falls_back_for_unwritable_default_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let locked = temp.path().join("locked");
        std::fs::create_dir_all(&locked).expect("create locked dir");
        let mut perms = std::fs::metadata(&locked).expect("metadata").permissions();
        perms.set_mode(0o500);
        std::fs::set_permissions(&locked, perms).expect("set perms");

        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home dir");
        let configured = locked.join("models");

        let (path, source, warning) =
            select_runtime_model_dir(configured, "default".to_string(), Some(home.clone()));

        let expected = home
            .join(".local/share/rustyfin/ai/models")
            .canonicalize()
            .unwrap_or_else(|_| home.join(".local/share/rustyfin/ai/models"));
        assert_eq!(path, expected);
        assert_eq!(source, "default_fallback");
        assert!(warning.is_some());
    }
}
