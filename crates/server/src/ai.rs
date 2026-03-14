use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            loaded_model: None,
            engine: None,
        }
    }
}

pub fn ai_router() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/models/pull", post(pull_model))
        .route("/models/{name}", delete(delete_model))
        .route("/running", get(list_running))
        .route("/gpus", get(list_gpus))
        .route("/chat", post(chat))
}

fn sse_error(message: impl Into<String>) -> Response {
    let event = Event::default()
        .event("error")
        .data(json!({ "message": message.into() }).to_string());
    let stream = stream::once(async move { Ok::<Event, Infallible>(event) });
    Sse::new(Box::pin(stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---------- GET /api/v1/ai/models -------------------------------------------

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<rustfin_ai_agent::ModelInfo>,
    inference_available: bool,
}

async fn list_models(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ModelsResponse>, AppError> {
    let models = rustfin_ai_agent::ModelStore::discover(&state.model_dir)
        .map_err(|error| rustfin_core::error::ApiError::Internal(error.to_string()))?;

    Ok(Json(ModelsResponse {
        models,
        inference_available: true,
    }))
}

// ---------- POST /api/v1/ai/models/pull (SSE) --------------------------------

#[derive(Deserialize)]
struct PullRequest {
    model: Option<String>,
    url: Option<String>,
}

async fn pull_model(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<PullRequest>,
) -> Response {
    let target = req
        .model
        .or(req.url)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(target) = target else {
        return sse_error("missing model or url");
    };

    let model_dir = state.model_dir.clone();
    let http_client = state.http.clone();
    let raw = rustfin_ai_agent::ModelStore::download(target, model_dir, http_client);
    let sse = raw.map(|chunk| {
        let event = match chunk {
            rustfin_ai_agent::PullChunk::Progress {
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
            rustfin_ai_agent::PullChunk::Done => Event::default().event("done").data("{}"),
            rustfin_ai_agent::PullChunk::Error(message) => Event::default()
                .event("error")
                .data(json!({ "message": message }).to_string()),
        };
        Ok::<Event, Infallible>(event)
    });

    Sse::new(Box::pin(sse))
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---------- DELETE /api/v1/ai/models/:name ----------------------------------

async fn delete_model(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    match rustfin_ai_agent::ModelStore::delete(&name, &state.model_dir) {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(rustfin_ai_agent::AiError::ModelNotFound(_)) => Ok(StatusCode::NOT_FOUND),
        Err(error) => {
            warn!(error = %error, model = %name, "failed to delete gguf model");
            Err(rustfin_core::error::ApiError::Internal(error.to_string()).into())
        }
    }
}

// ---------- GET /api/v1/ai/running ------------------------------------------

#[derive(Serialize)]
struct RunningModelsResponse {
    models: Vec<RunningModelInfo>,
}

#[derive(Serialize)]
struct RunningModelInfo {
    name: String,
    size_vram_gb: f64,
    parameter_size: Option<String>,
    expires_at: Option<String>,
}

async fn list_running(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<RunningModelsResponse>, AppError> {
    let loaded_model = {
        let guard = state.engine.lock().await;
        guard.loaded_model.clone()
    };

    let models = if let Some(name) = loaded_model {
        vec![RunningModelInfo {
            name,
            size_vram_gb: estimate_loaded_model_vram_gb().await,
            parameter_size: None,
            expires_at: None,
        }]
    } else {
        vec![]
    };

    Ok(Json(RunningModelsResponse { models }))
}

async fn estimate_loaded_model_vram_gb() -> f64 {
    let gpus = enumerate_nvidia_gpus().await;
    if gpus.is_empty() {
        return 0.0;
    }

    let used_mb: u64 = gpus
        .into_iter()
        .map(|gpu| gpu.vram_used_mb)
        .fold(0_u64, |acc, used| acc.saturating_add(used));
    used_mb as f64 / 1024.0
}

// ---------- GET /api/v1/ai/gpus ---------------------------------------------

#[derive(Serialize)]
struct GpusResponse {
    gpus: Vec<GpuInfo>,
    multi_gpu_note: &'static str,
    cuda_visible_devices: Option<String>,
}

#[derive(Serialize)]
struct GpuInfo {
    index: u32,
    name: String,
    vram_total_mb: u64,
    vram_used_mb: u64,
    utilization_pct: Option<u8>,
}

/// Enumerate NVIDIA GPUs via nvidia-smi.
/// Returns an empty vec if nvidia-smi is unavailable (no NVIDIA GPU / AMD host).
async fn enumerate_nvidia_gpus() -> Vec<GpuInfo> {
    let output = tokio::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total,memory.used,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;

    let out = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => return vec![],
    };

    let text = String::from_utf8_lossy(&out);
    let mut gpus = Vec::new();

    for line in text.lines() {
        let parts: Vec<&str> = line.split(',').map(str::trim).collect();
        if parts.len() < 4 {
            continue;
        }
        let index: u32 = parts[0].parse().unwrap_or(0);
        let name = parts[1].to_string();
        let vram_total_mb: u64 = parts[2].parse().unwrap_or(0);
        let vram_used_mb: u64 = parts[3].parse().unwrap_or(0);
        let utilization_pct: Option<u8> = parts.get(4).and_then(|s| s.parse().ok());

        gpus.push(GpuInfo {
            index,
            name,
            vram_total_mb,
            vram_used_mb,
            utilization_pct,
        });
    }

    gpus
}

async fn list_gpus(
    _user: AuthUser,
    State(_state): State<AppState>,
) -> Result<Json<GpusResponse>, AppError> {
    let gpus = enumerate_nvidia_gpus().await;
    let cuda_env = std::env::var("CUDA_VISIBLE_DEVICES").ok();

    Ok(Json(GpusResponse {
        gpus,
        multi_gpu_note: "llama.cpp automatically distributes model layers across visible GPUs. \
            Set CUDA_VISIBLE_DEVICES before starting rustfin-server to restrict GPU access.",
        cuda_visible_devices: cuda_env,
    }))
}

// ---------- POST /api/v1/ai/chat --------------------------------------------

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    message: String,
    #[serde(default)]
    history: Vec<HistoryMessage>,
}

#[derive(Deserialize)]
struct HistoryMessage {
    role: String,
    content: String,
}

async fn chat(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let system_prompt =
        "You are the Rustyfin assistant — a helpful AI built into a personal home media server. \
         Be concise and genuinely helpful. Respond in plain text unless code or markdown lists \
         add real clarity to the answer."
            .to_string();

    let mut messages = vec![rustfin_ai_agent::ChatMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];

    for history in req.history {
        messages.push(rustfin_ai_agent::ChatMessage {
            role: history.role,
            content: history.content,
        });
    }

    messages.push(rustfin_ai_agent::ChatMessage {
        role: "user".to_string(),
        content: req.message,
    });

    let gguf_path = state.model_dir.join(format!("{}.gguf", req.model));

    let engine = {
        let mut guard = state.engine.lock().await;
        let needs_reload =
            guard.engine.is_none() || guard.loaded_model.as_deref() != Some(req.model.as_str());

        if needs_reload {
            match rustfin_ai_agent::LlamaEngine::load(
                &gguf_path,
                rustfin_ai_agent::LlamaEngineParams::default(),
            ) {
                Ok(engine) => {
                    guard.loaded_model = Some(req.model.clone());
                    guard.engine = Some(engine);
                }
                Err(error) => {
                    return sse_error(format!(
                        "failed to load model {}: {error}",
                        gguf_path.display()
                    ));
                }
            }
        }

        guard.engine.clone()
    };

    let Some(engine) = engine else {
        return sse_error("no inference engine loaded");
    };

    let raw_stream = engine.chat_stream(messages, rustfin_ai_agent::SamplingParams::default());
    let sse_stream = raw_stream.map(|chunk| {
        let event = match chunk {
            Ok(rustfin_ai_agent::ChatChunk::Token(text)) => Event::default()
                .event("token")
                .data(json!({ "text": text }).to_string()),
            Ok(rustfin_ai_agent::ChatChunk::Stats {
                prompt_tokens,
                completion_tokens,
                total_duration_ms,
                tokens_per_second,
            }) => Event::default().event("stats").data(
                json!({
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_duration_ms": total_duration_ms,
                    "tokens_per_second": tokens_per_second,
                })
                .to_string(),
            ),
            Ok(rustfin_ai_agent::ChatChunk::Done) => Event::default().event("done").data("{}"),
            Err(error) => Event::default()
                .event("error")
                .data(json!({ "message": error.to_string() }).to_string()),
        };
        Ok::<Event, Infallible>(event)
    });

    Sse::new(Box::pin(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
}
