use std::path::{Path, PathBuf};

use async_stream::stream;
use futures::Stream;
use rustfin_core::error::ApiError;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

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
pub struct AiModelDirectoryState {
    pub available: bool,
    pub model_dir: String,
    pub default_model_dir: String,
    pub model_dir_source: String,
    pub models: Vec<AiModelSummary>,
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
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).last())
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
