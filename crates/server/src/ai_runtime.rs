use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::ai_assistant::types::{AssistantRuntimePhase, AssistantTurnStats};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct AiRuntimeResponse {
    pub model: AiRuntimeModelSummary,
    pub turn: AiRuntimeTurnSummary,
    pub resources: AiRuntimeResourcesSummary,
    pub gpus: Vec<AiRuntimeGpuSummary>,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeModelSummary {
    pub name: Option<String>,
    pub backend: String,
    pub context_length: u32,
    pub n_threads: u32,
    pub n_gpu_layers: i32,
    pub split_mode: String,
    pub device_indices: Vec<usize>,
    pub loaded: bool,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeTurnSummary {
    pub phase: AssistantRuntimePhase,
    pub queue_depth: u64,
    pub active_request_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<AiRuntimePromptSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stats: Option<AssistantTurnStats>,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimePromptSummary {
    pub context_length: u32,
    pub prompt_budget_tokens: u32,
    pub reserved_completion_tokens: u32,
    pub prompt_tokens_estimate: u32,
    pub loaded_history_turns: u32,
    pub retained_raw_turns: u32,
    pub summarized_turns: u32,
    pub recent_grounded_context_count: u32,
    pub used_memory_summary: bool,
    pub memory_turn_index: i64,
    pub memory_summary_chars: usize,
    pub compact_boundary_count: u32,
    pub recovered_from_compact_boundary: bool,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeResourcesSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_rss_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_rss_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ram_used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ram_total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ram_used_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ram_total_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ram_used_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeGpuSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_used_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_total_human: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_celsius: Option<f64>,
}

pub async fn get_ai_runtime(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<AiRuntimeResponse>, AppError> {
    let host = crate::runtime_diagnostics::collect_host_runtime_snapshot().await;
    let runtime = state.runtime_metrics.snapshot();
    let gpus = collect_gpu_metrics().await;

    let (
        loaded_model,
        loaded,
        phase,
        context_length,
        n_threads,
        n_gpu_layers,
        split_mode,
        device_indices,
        prompt_debug,
        last_turn_stats,
    ) = {
        let guard = state.engine.lock().await;
        let loaded_model = guard.loaded_model.clone();
        let loaded = guard.engine.is_some();
        let params = guard
            .engine
            .as_ref()
            .map(|engine| engine.params().clone())
            .unwrap_or_default();
        (
            loaded_model,
            loaded,
            guard.active_phase,
            params.n_ctx,
            params.n_threads,
            params.n_gpu_layers,
            params.split_mode.as_str().to_string(),
            params.device_indices,
            guard.last_prompt_debug.clone(),
            guard.last_turn_stats.clone(),
        )
    };

    let process_rss_bytes = current_process_rss_bytes();
    let active_request_count = runtime.assistant.chats.calls_in_flight;
    let queue_depth = if phase == AssistantRuntimePhase::Idle {
        0
    } else {
        active_request_count.saturating_sub(1)
    };

    Ok(Json(AiRuntimeResponse {
        model: AiRuntimeModelSummary {
            name: loaded_model,
            backend: inferred_backend(loaded, n_gpu_layers, &device_indices, &gpus),
            context_length,
            n_threads,
            n_gpu_layers,
            split_mode,
            device_indices,
            loaded,
        },
        turn: AiRuntimeTurnSummary {
            phase,
            queue_depth,
            active_request_count,
            prompt: prompt_debug.map(|value| AiRuntimePromptSummary {
                context_length: value.context_length,
                prompt_budget_tokens: value.prompt_budget_tokens,
                reserved_completion_tokens: value.reserved_completion_tokens,
                prompt_tokens_estimate: value.prompt_tokens_estimate,
                loaded_history_turns: value.loaded_history_turns,
                retained_raw_turns: value.retained_raw_turns,
                summarized_turns: value.summarized_turns,
                recent_grounded_context_count: value.recent_grounded_context_count,
                used_memory_summary: value.used_memory_summary,
                memory_turn_index: value.memory_turn_index,
                memory_summary_chars: value.memory_summary_chars,
                compact_boundary_count: value.compact_boundary_count,
                recovered_from_compact_boundary: value.recovered_from_compact_boundary,
            }),
            last_stats: last_turn_stats,
        },
        resources: AiRuntimeResourcesSummary {
            process_rss_human: process_rss_bytes.map(human_bytes),
            process_rss_bytes,
            host_cpu_percent: host.cpu_usage_percent,
            host_ram_used_bytes: host.used_memory_bytes,
            host_ram_total_bytes: host.total_memory_bytes,
            host_ram_used_human: host.used_memory_bytes.map(human_bytes),
            host_ram_total_human: host.total_memory_bytes.map(human_bytes),
            host_ram_used_percent: host.memory_used_percent,
        },
        gpus,
    }))
}

fn inferred_backend(
    loaded: bool,
    n_gpu_layers: i32,
    device_indices: &[usize],
    gpus: &[AiRuntimeGpuSummary],
) -> String {
    let requested = configured_backend_request();
    resolve_backend_label(
        &requested,
        compiled_backend_label(),
        loaded,
        n_gpu_layers,
        device_indices,
        gpus.len(),
    )
}

fn configured_backend_request() -> String {
    std::env::var("RUSTFIN_AI_GPU_BACKEND")
        .unwrap_or_else(|_| "auto".to_string())
        .trim()
        .to_ascii_lowercase()
}

fn compiled_backend_label() -> Option<&'static str> {
    if cfg!(feature = "ai-cuda") {
        Some("cuda")
    } else if cfg!(feature = "ai-rocm") {
        Some("rocm")
    } else if cfg!(feature = "ai-vulkan") {
        Some("vulkan")
    } else if cfg!(feature = "ai-cpu") || cfg!(feature = "ai") {
        Some("cpu")
    } else {
        None
    }
}

fn resolve_backend_label(
    requested: &str,
    compiled_backend: Option<&str>,
    loaded: bool,
    n_gpu_layers: i32,
    device_indices: &[usize],
    gpu_metric_count: usize,
) -> String {
    match requested {
        "disabled" | "none" | "off" => "disabled".to_string(),
        "cpu" => "cpu".to_string(),
        "cuda" | "rocm" | "vulkan" => requested.to_string(),
        "auto" | "" => {
            if let Some(label) = compiled_backend {
                if label == "cpu" {
                    return "cpu".to_string();
                }
                if loaded && n_gpu_layers == 0 && device_indices.is_empty() {
                    return "cpu".to_string();
                }
                return label.to_string();
            }
            if loaded && (n_gpu_layers > 0 || !device_indices.is_empty()) {
                return if gpu_metric_count > 0 {
                    "cuda".to_string()
                } else {
                    "cpu".to_string()
                };
            }
            if gpu_metric_count > 0 {
                "cuda".to_string()
            } else {
                "cpu".to_string()
            }
        }
        other => other.to_string(),
    }
}

async fn collect_gpu_metrics() -> Vec<AiRuntimeGpuSummary> {
    let output = tokio::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let columns = line
                .split(',')
                .map(|value| value.trim().to_string())
                .collect::<Vec<_>>();
            if columns.is_empty() {
                return None;
            }
            let used_bytes = columns
                .get(3)
                .and_then(|value| value.parse::<u64>().ok())
                .map(|value| value.saturating_mul(1024 * 1024));
            let total_bytes = columns
                .get(4)
                .and_then(|value| value.parse::<u64>().ok())
                .map(|value| value.saturating_mul(1024 * 1024));
            Some(AiRuntimeGpuSummary {
                index: columns.get(0).and_then(|value| value.parse::<u32>().ok()),
                name: columns.get(1).cloned().unwrap_or_else(|| "GPU".to_string()),
                utilization_percent: columns.get(2).and_then(|value| value.parse::<f64>().ok()),
                vram_used_human: used_bytes.map(human_bytes),
                vram_used_bytes: used_bytes,
                vram_total_human: total_bytes.map(human_bytes),
                vram_total_bytes: total_bytes,
                temperature_celsius: columns.get(5).and_then(|value| value.parse::<f64>().ok()),
            })
        })
        .collect()
}

fn current_process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in raw.lines() {
            let Some(rest) = line.strip_prefix("VmRSS:") else {
                continue;
            };
            let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kib.saturating_mul(1024));
        }
        None
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_index = 0_usize;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use axum_test::TestServer;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;

    fn test_state() -> AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig {
            transcode_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-runtime-test-{}", uuid::Uuid::new_v4())),
            max_concurrent: 1,
            ..Default::default()
        };
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder = Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(8);

        AppState {
            db: pool,
            rustyvault: crate::state::RustyVaultRuntimeState::available(),
            jwt_secret: "test-secret".to_string(),
            http: reqwest::Client::builder().build().unwrap(),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url: "http://127.0.0.1:8102".to_string(),
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            model_dir: Arc::new(tokio::sync::RwLock::new(
                std::env::temp_dir().join("rustyfin-ai-models-test"),
            )),
            engine: Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-runtime-cache-{}",
                uuid::Uuid::new_v4()
            )),
            watch_party_audio_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-runtime-watch-audio-{}",
                uuid::Uuid::new_v4()
            )),
            events: events_tx,
            watch_party: Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    fn auth_header_value(secret: &str) -> axum::http::HeaderValue {
        let token = crate::auth::issue_token("user-1", "tester", "user", secret).unwrap();
        axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap()
    }

    #[tokio::test]
    async fn runtime_route_reports_phase_and_queue_depth() {
        let state = test_state();
        {
            let mut engine = state.engine.lock().await;
            engine.loaded_model = Some("tiny.gguf".to_string());
            engine.active_phase = AssistantRuntimePhase::Grounding;
            engine.last_prompt_debug = Some(crate::ai_assistant::memory::ConversationPromptDebug {
                context_length: 4096,
                prompt_budget_tokens: 3072,
                reserved_completion_tokens: 1024,
                prompt_tokens_estimate: 1200,
                loaded_history_turns: 12,
                retained_raw_turns: 4,
                summarized_turns: 8,
                recent_grounded_context_count: 2,
                used_memory_summary: true,
                memory_turn_index: 7,
                memory_summary_chars: 240,
                compact_boundary_count: 1,
                recovered_from_compact_boundary: true,
            });
        }
        let _chat_1 = state.runtime_metrics.start_ai_chat_request();
        let _chat_2 = state.runtime_metrics.start_ai_chat_request();

        let app = Router::new()
            .route("/runtime", get(get_ai_runtime))
            .with_state(state.clone());
        let server = TestServer::new(app).unwrap();

        let response = server
            .get("/runtime")
            .add_header(
                axum::http::header::AUTHORIZATION,
                auth_header_value(&state.jwt_secret),
            )
            .await;
        response.assert_status_ok();
        let body: serde_json::Value = response.json();

        assert_eq!(body["model"]["name"].as_str(), Some("tiny.gguf"));
        assert_eq!(body["model"]["loaded"].as_bool(), Some(false));
        assert_eq!(body["model"]["split_mode"].as_str(), Some("layer"));
        assert_eq!(
            body["model"]["device_indices"].as_array().map(Vec::len),
            Some(0)
        );
        assert!(
            !body["model"]["backend"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        );
        assert_eq!(body["turn"]["phase"].as_str(), Some("grounding"));
        assert_eq!(body["turn"]["active_request_count"].as_u64(), Some(2));
        assert_eq!(body["turn"]["queue_depth"].as_u64(), Some(1));
        assert_eq!(
            body["turn"]["prompt"]["loaded_history_turns"].as_u64(),
            Some(12)
        );
        assert_eq!(body["turn"]["prompt"]["summarized_turns"].as_u64(), Some(8));
        assert_eq!(
            body["turn"]["prompt"]["used_memory_summary"].as_bool(),
            Some(true)
        );
        assert!(body["resources"].is_object());
        #[cfg(target_os = "linux")]
        {
            assert!(body["resources"].get("host_ram_used_human").is_some());
            assert!(body["resources"].get("host_ram_total_human").is_some());
        }
        assert!(body["gpus"].is_array());
    }

    #[test]
    fn human_bytes_uses_binary_units() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn resolve_backend_label_prefers_explicit_request() {
        assert_eq!(
            resolve_backend_label("rocm", Some("cpu"), false, 0, &[], 0),
            "rocm"
        );
        assert_eq!(
            resolve_backend_label("disabled", Some("cuda"), true, 99, &[0], 1),
            "disabled"
        );
    }

    #[test]
    fn resolve_backend_label_reports_cpu_when_auto_falls_back_to_cpu() {
        assert_eq!(
            resolve_backend_label("auto", Some("cuda"), true, 0, &[], 1),
            "cpu"
        );
        assert_eq!(
            resolve_backend_label("auto", Some("cpu"), false, 0, &[], 0),
            "cpu"
        );
    }

    #[test]
    fn resolve_backend_label_uses_compiled_gpu_backend_for_auto() {
        assert_eq!(
            resolve_backend_label("auto", Some("vulkan"), true, 32, &[0, 1], 0),
            "vulkan"
        );
        assert_eq!(
            resolve_backend_label("auto", Some("rocm"), false, 0, &[], 0),
            "rocm"
        );
    }
}
