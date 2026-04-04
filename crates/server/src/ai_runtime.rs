use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::ai_assistant::types::AssistantRuntimePhase;
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct AiRuntimeResponse {
    pub model: AiRuntimeModelSummary,
    pub turn: AiRuntimeTurnSummary,
    pub scheduler: AiRuntimeSchedulerSummary,
    pub resources: AiRuntimeResourcesSummary,
    pub gpus: Vec<AiRuntimeGpuSummary>,
    pub role_routing: Vec<crate::ai_model_routing::RoleRoutingDecision>,
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
    pub last_execution: Option<AiRuntimeExecutionSummary>,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeExecutionSummary {
    pub stop_reason: String,
    pub final_answer_path: String,
    pub tool_step_count: u32,
    pub alternate_tool_count: u32,
    pub recovery_step_count: u32,
    pub clarification_count: u32,
    pub conflict_count: u32,
    pub attempt_count: u32,
    pub deterministic_answer_used: bool,
    pub synthesis_used: bool,
    pub outcome_counts: Vec<AiRuntimeExecutionOutcomeCount>,
    pub role_backend_usage: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeExecutionOutcomeCount {
    pub outcome_kind: String,
    pub count: u32,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeSchedulerSummary {
    pub max_concurrent_turns: u64,
    pub queue_limit: u64,
    pub active_turns: u64,
    pub queued_turns: u64,
    pub overload_state: String,
    pub warm_pool_bytes: u64,
    pub warm_pool_budget_bytes: u64,
    pub active_by_priority: Vec<AiRuntimeSchedulerPriorityCount>,
    pub queued_by_priority: Vec<AiRuntimeSchedulerPriorityCount>,
    pub warm_models: Vec<AiRuntimeWarmModel>,
    pub rejected_turns_total: u64,
    pub degraded_turns_total: u64,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeSchedulerPriorityCount {
    pub priority: String,
    pub count: u64,
}

#[derive(Debug, Serialize)]
pub struct AiRuntimeWarmModel {
    pub model_name: String,
    pub estimated_bytes: u64,
    pub loaded_ts_ms: i64,
    pub last_used_ts_ms: i64,
    pub load_count: u64,
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

pub async fn collect_ai_runtime_response(state: &AppState) -> AiRuntimeResponse {
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
        backend,
        role_routing,
        scheduler_snapshot,
        last_execution_trace,
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
            guard
                .role_models
                .get(&rustfin_ai_agent::ModelRole::Answer)
                .map(|loaded| loaded.backend_kind.as_str().to_string())
                .unwrap_or_else(|| inferred_backend(&gpus)),
            guard.role_routing.clone(),
            guard.scheduler.snapshot(),
            guard.last_execution_trace.clone(),
        )
    };

    let process_rss_bytes = current_process_rss_bytes();
    let active_request_count = runtime.assistant.chats.calls_in_flight;
    let queue_depth = if phase == AssistantRuntimePhase::Idle {
        0
    } else {
        active_request_count.saturating_sub(1)
    };

    AiRuntimeResponse {
        model: AiRuntimeModelSummary {
            name: loaded_model,
            backend,
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
            last_execution: last_execution_trace.map(|trace| AiRuntimeExecutionSummary {
                stop_reason: trace.stop_reason.as_str().to_string(),
                final_answer_path: trace.final_answer_path.as_str().to_string(),
                tool_step_count: trace.tool_step_count,
                alternate_tool_count: trace.alternate_tool_count,
                recovery_step_count: trace.recovery_step_count,
                clarification_count: trace.clarification_count,
                conflict_count: trace.conflict_count,
                attempt_count: trace.attempts.len() as u32,
                deterministic_answer_used: trace.deterministic_answer_used,
                synthesis_used: trace.synthesis_used,
                outcome_counts: trace
                    .outcome_counts
                    .into_iter()
                    .map(|(outcome_kind, count)| AiRuntimeExecutionOutcomeCount {
                        outcome_kind,
                        count,
                    })
                    .collect(),
                role_backend_usage: trace.used_role_backends,
            }),
        },
        scheduler: AiRuntimeSchedulerSummary {
            max_concurrent_turns: scheduler_snapshot.max_concurrent_turns,
            queue_limit: scheduler_snapshot.queue_limit,
            active_turns: scheduler_snapshot.active_turns,
            queued_turns: scheduler_snapshot.queued_turns,
            overload_state: scheduler_snapshot.overload_state,
            warm_pool_bytes: scheduler_snapshot.warm_pool_bytes,
            warm_pool_budget_bytes: scheduler_snapshot.warm_pool_budget_bytes,
            active_by_priority: scheduler_snapshot
                .active_by_priority
                .into_iter()
                .map(|entry| AiRuntimeSchedulerPriorityCount {
                    priority: entry.priority,
                    count: entry.count,
                })
                .collect(),
            queued_by_priority: scheduler_snapshot
                .queued_by_priority
                .into_iter()
                .map(|entry| AiRuntimeSchedulerPriorityCount {
                    priority: entry.priority,
                    count: entry.count,
                })
                .collect(),
            warm_models: scheduler_snapshot
                .warm_models
                .into_iter()
                .map(|entry| AiRuntimeWarmModel {
                    model_name: entry.model_name,
                    estimated_bytes: entry.estimated_bytes,
                    loaded_ts_ms: entry.loaded_ts_ms,
                    last_used_ts_ms: entry.last_used_ts_ms,
                    load_count: entry.load_count,
                })
                .collect(),
            rejected_turns_total: scheduler_snapshot.rejected_turns_total,
            degraded_turns_total: scheduler_snapshot.degraded_turns_total,
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
        role_routing,
    }
}

pub async fn get_ai_runtime(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<AiRuntimeResponse>, AppError> {
    Ok(Json(collect_ai_runtime_response(&state).await))
}

fn inferred_backend(gpus: &[AiRuntimeGpuSummary]) -> String {
    let requested = std::env::var("RUSTFIN_AI_GPU_BACKEND")
        .unwrap_or_else(|_| "auto".to_string())
        .trim()
        .to_ascii_lowercase();
    match requested.as_str() {
        "auto" => {
            if !gpus.is_empty() {
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
                index: columns.first().and_then(|value| value.parse::<u32>().ok()),
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
    use crate::ai_assistant::types::{
        AssistantExecutionStopReason, AssistantExecutionTrace, AssistantResponseMode,
        AssistantSynthesisMode,
    };
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
        assert!(body["resources"].is_object());
        #[cfg(target_os = "linux")]
        {
            assert!(body["resources"].get("host_ram_used_human").is_some());
            assert!(body["resources"].get("host_ram_total_human").is_some());
        }
        assert!(body["gpus"].is_array());
    }

    #[tokio::test]
    async fn runtime_route_reports_last_execution_summary() {
        let state = test_state();
        {
            let mut engine = state.engine.lock().await;
            engine.last_execution_trace = Some(AssistantExecutionTrace {
                response_mode: AssistantResponseMode::Thinking,
                budget: crate::ai_assistant::types::AssistantExecutionBudget::for_mode(
                    AssistantResponseMode::Thinking,
                ),
                planner_mode: Some("deterministic_fallback".to_string()),
                attempts: vec![
                    crate::ai_assistant::types::AssistantExecutionAttempt {
                        step_index: 1,
                        tool: "system_get_host_runtime_summary".to_string(),
                        label: "Host runtime".to_string(),
                        domain_family: crate::ai_assistant::types::AssistantDomainFamily::System,
                        status: "ok".to_string(),
                        outcome_kind: crate::ai_assistant::types::AssistantToolOutcomeKind::Partial,
                        latency_ms: 12,
                        args_hash: "abc".to_string(),
                        result_signature: "sig1".to_string(),
                        evidence_ids: vec!["ev:1".to_string()],
                        ambiguity_keys: Vec::new(),
                        fallback_edge: None,
                        used_alternate: false,
                        recovery_depth: 0,
                        message: None,
                    },
                    crate::ai_assistant::types::AssistantExecutionAttempt {
                        step_index: 2,
                        tool: "system_get_ai_runtime_summary".to_string(),
                        label: "AI runtime".to_string(),
                        domain_family: crate::ai_assistant::types::AssistantDomainFamily::AiRuntime,
                        status: "ok".to_string(),
                        outcome_kind: crate::ai_assistant::types::AssistantToolOutcomeKind::Answer,
                        latency_ms: 18,
                        args_hash: "def".to_string(),
                        result_signature: "sig2".to_string(),
                        evidence_ids: vec!["ev:2".to_string()],
                        ambiguity_keys: Vec::new(),
                        fallback_edge: Some("system.intent_to_ai_runtime".to_string()),
                        used_alternate: true,
                        recovery_depth: 1,
                        message: None,
                    },
                ],
                retained_evidence: Vec::new(),
                stop_reason: AssistantExecutionStopReason::SufficientAnswer,
                final_outcome_kind: Some(
                    crate::ai_assistant::types::AssistantToolOutcomeKind::Answer,
                ),
                final_answer_path: AssistantSynthesisMode::DeterministicReply,
                planner_pass_count: 1,
                tool_step_count: 2,
                alternate_tool_count: 1,
                recovery_step_count: 1,
                clarification_count: 0,
                conflict_count: 0,
                deterministic_answer_used: true,
                synthesis_used: false,
                used_role_backends: vec!["planner:local".to_string(), "answer:local".to_string()],
                outcome_counts: std::collections::BTreeMap::from([
                    ("partial".to_string(), 1_u32),
                    ("answer".to_string(), 1_u32),
                ]),
            });
        }

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

        assert_eq!(
            body["turn"]["last_execution"]["stop_reason"].as_str(),
            Some("sufficient_answer")
        );
        assert_eq!(
            body["turn"]["last_execution"]["attempt_count"].as_u64(),
            Some(2)
        );
        assert_eq!(
            body["turn"]["last_execution"]["alternate_tool_count"].as_u64(),
            Some(1)
        );
        assert_eq!(
            body["turn"]["last_execution"]["recovery_step_count"].as_u64(),
            Some(1)
        );
    }

    #[test]
    fn human_bytes_uses_binary_units() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
    }
}
