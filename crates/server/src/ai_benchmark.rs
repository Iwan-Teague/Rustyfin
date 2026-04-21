use std::path::Path;
use std::process::Command;
use std::time::Instant;

use futures::StreamExt;
use rustfin_ai_agent::engine::LlamaGpuSplitMode;
use rustfin_ai_agent::{
    ChatChunk, ChatMessage, LlamaEngine, LlamaEngineParams, ModelProfileRecommendation,
    SamplingParams, WarmupCostClass,
};
use serde::Serialize;
use tracing::{info, warn};

use crate::ai_enabled::engine_params_from_env;
use crate::ai_storage::{AiModelSummary, current_model_dir, model_file_path};
use crate::error::AppError;
use crate::state::AppState;

const BENCHMARK_SYSTEM_PROMPT: &str =
    "You are benchmarking a local GGUF model for Rustyfin. Reply with one short paragraph.";
const BENCHMARK_USER_PROMPT: &str =
    "Explain why scheduler-aware warm-model pooling improves latency and capacity.";

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkCandidateSummary {
    label: String,
    benchmark_label: String,
    n_threads: u32,
    n_gpu_layers: i32,
    split_mode: String,
    main_gpu: Option<i32>,
    load_duration_ms: u64,
    prefill_tokens: u64,
    prefill_duration_ms: u64,
    decode_tokens: u64,
    decode_duration_ms: u64,
    first_token_ms: u64,
    total_duration_ms: u64,
    tokens_per_second: f64,
    rss_before_bytes: Option<u64>,
    rss_after_load_bytes: Option<u64>,
    rss_peak_bytes: Option<u64>,
    failure_message: Option<String>,
}

#[derive(Debug, Clone)]
struct BenchmarkCandidate {
    label: String,
    params: LlamaEngineParams,
    benchmark_label: String,
    estimated_bytes: u64,
}

#[derive(Debug, Clone)]
struct BenchmarkOutcome {
    candidate: BenchmarkCandidate,
    load_duration_ms: u64,
    prefill_tokens: u64,
    prefill_duration_ms: u64,
    decode_tokens: u64,
    decode_duration_ms: u64,
    first_token_ms: u64,
    total_duration_ms: u64,
    tokens_per_second: f64,
    rss_before_bytes: Option<u64>,
    rss_after_load_bytes: Option<u64>,
    rss_peak_bytes: Option<u64>,
    failure_message: Option<String>,
}

#[derive(Debug, Clone)]
struct ModelProfilePersistence {
    recommendation: ModelProfileRecommendation,
    benchmark_count: i64,
    last_benchmark_label: String,
    last_load_duration_ms: u64,
    last_tokens_per_second: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BenchmarkGpuDevice {
    index: usize,
    total_vram_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiBenchmarkRunSummary {
    pub model_name: String,
    pub model_checksum: String,
    pub host_fingerprint: String,
    pub recommendation: ModelProfileRecommendation,
    pub candidates: Vec<BenchmarkCandidateSummary>,
}

pub async fn run_model_benchmarks(
    state: &AppState,
    model_name: Option<&str>,
    benchmark_label: Option<&str>,
) -> Result<AiBenchmarkRunSummary, AppError> {
    let models = crate::ai_storage::list_models_from_state(state).await?;
    let loaded_model = {
        let guard = state.engine.lock().await;
        guard.loaded_model.clone()
    };
    let target_model = select_target_model(&models, model_name, loaded_model.as_deref())?;
    let model_dir = current_model_dir(state).await;
    let model_path = model_file_path(&model_dir, &target_model.name)?;
    let host_fingerprint = crate::ai_admin::host_fingerprint();
    let model_checksum = rustfin_ai_agent::ModelStore::checksum(&model_path).map_err(|error| {
        AppError::from(rustfin_core::error::ApiError::Internal(format!(
            "failed to checksum model for benchmarking: {error}"
        )))
    })?;
    let model_size_bytes = std::fs::metadata(&model_path)
        .map_err(|error| {
            AppError::from(rustfin_core::error::ApiError::Internal(format!(
                "failed to stat model for benchmarking: {error}"
            )))
        })?
        .len();
    let host_threads = std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(4);
    let benchmark_prefix = benchmark_label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "admin-benchmark".to_string());
    let next_profile_benchmark_count = rustfin_db::repo::ai_models::list_model_profiles_for_host(
        &state.db,
        &host_fingerprint,
        200,
    )
    .await
    .ok()
    .and_then(|rows| {
        rows.into_iter()
            .find(|row| row.model_checksum == model_checksum)
            .map(|row| row.benchmark_count.saturating_add(1))
    })
    .unwrap_or(1);

    let candidates = benchmark_candidates(
        target_model,
        &model_path,
        model_size_bytes,
        host_threads,
        &benchmark_prefix,
    );
    let mut outcomes = Vec::new();
    for candidate in candidates {
        let outcome = benchmark_candidate(&model_path, &candidate).await;
        outcomes.push(outcome.clone());
        let profile_persistence = build_profile_persistence(
            &host_fingerprint,
            target_model,
            &model_checksum,
            model_size_bytes,
            host_threads,
            next_profile_benchmark_count,
            &outcomes,
        );
        persist_benchmark_progress(
            &state.db,
            &host_fingerprint,
            target_model,
            &model_path,
            &model_checksum,
            model_size_bytes,
            &outcome,
            profile_persistence.as_ref(),
        )
        .await?;
        if let Some(message) = outcome.failure_message.as_deref() {
            warn!(
                model = %target_model.name,
                benchmark_label = %outcome.candidate.benchmark_label,
                error = %message,
                "AI benchmark candidate failed"
            );
        } else {
            info!(
                model = %target_model.name,
                benchmark_label = %outcome.candidate.benchmark_label,
                tokens_per_second = outcome.tokens_per_second,
                "AI benchmark candidate completed"
            );
        }
    }

    let recommendation = recommend_model_profile(
        &host_fingerprint,
        target_model,
        &model_checksum,
        model_size_bytes,
        host_threads,
        &outcomes,
    );
    Ok(AiBenchmarkRunSummary {
        model_name: target_model.name.clone(),
        model_checksum,
        host_fingerprint,
        recommendation,
        candidates: outcomes
            .into_iter()
            .map(|outcome| BenchmarkCandidateSummary {
                label: outcome.candidate.label,
                benchmark_label: outcome.candidate.benchmark_label,
                n_threads: outcome.candidate.params.n_threads,
                n_gpu_layers: outcome.candidate.params.n_gpu_layers,
                split_mode: outcome.candidate.params.split_mode.as_str().to_string(),
                main_gpu: outcome.candidate.params.main_gpu,
                load_duration_ms: outcome.load_duration_ms,
                prefill_tokens: outcome.prefill_tokens,
                prefill_duration_ms: outcome.prefill_duration_ms,
                decode_tokens: outcome.decode_tokens,
                decode_duration_ms: outcome.decode_duration_ms,
                first_token_ms: outcome.first_token_ms,
                total_duration_ms: outcome.total_duration_ms,
                tokens_per_second: outcome.tokens_per_second,
                rss_before_bytes: outcome.rss_before_bytes,
                rss_after_load_bytes: outcome.rss_after_load_bytes,
                rss_peak_bytes: outcome.rss_peak_bytes,
                failure_message: outcome.failure_message,
            })
            .collect(),
    })
}

fn select_target_model<'a>(
    models: &'a [AiModelSummary],
    requested_model: Option<&str>,
    loaded_model: Option<&str>,
) -> Result<&'a AiModelSummary, AppError> {
    if let Some(requested_model) = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(model) = models
            .iter()
            .find(|model| model.name == requested_model || model.file == requested_model)
        {
            return Ok(model);
        }
        return Err(AppError::from(rustfin_core::error::ApiError::BadRequest(
            format!("unknown AI model `{requested_model}`"),
        )));
    }

    if let Some(loaded_model) = loaded_model {
        if let Some(model) = models.iter().find(|model| model.name == loaded_model) {
            return Ok(model);
        }
    }

    models.first().ok_or_else(|| {
        AppError::from(rustfin_core::error::ApiError::BadRequest(
            "no GGUF models are available to benchmark".into(),
        ))
    })
}

fn benchmark_candidates(
    _model: &AiModelSummary,
    model_path: &Path,
    model_size_bytes: u64,
    host_threads: u32,
    benchmark_prefix: &str,
) -> Vec<BenchmarkCandidate> {
    let base_params = engine_params_from_env();
    let model_block_count = rustfin_ai_agent::ModelStore::inspect_file(model_path)
        .ok()
        .and_then(|info| info.block_count);
    let visible_gpu_devices = rustfin_ai_agent::engine::available_gpu_backend_device_indices();
    let preferred_gpu = preferred_benchmark_gpu_device(&base_params, &visible_gpu_devices);

    build_benchmark_candidates(
        &base_params,
        model_size_bytes,
        host_threads,
        benchmark_prefix,
        model_block_count,
        preferred_gpu,
        visible_gpu_devices.len(),
    )
}

fn build_benchmark_candidates(
    base_params: &LlamaEngineParams,
    model_size_bytes: u64,
    host_threads: u32,
    benchmark_prefix: &str,
    model_block_count: Option<u32>,
    preferred_gpu: Option<BenchmarkGpuDevice>,
    visible_gpu_count: usize,
) -> Vec<BenchmarkCandidate> {
    let mut candidates = Vec::new();
    let (stable_base_params, current_label) =
        stabilize_benchmark_base_params(base_params, preferred_gpu, visible_gpu_count);

    candidates.push(BenchmarkCandidate {
        label: current_label.to_string(),
        benchmark_label: format!("{benchmark_prefix}/{current_label}"),
        estimated_bytes: estimate_model_bytes(model_size_bytes, &stable_base_params),
        params: stable_base_params.clone(),
    });

    let mut balanced = stable_base_params.clone();
    let balanced_threads = host_threads.saturating_mul(3).saturating_div(4).max(1);
    balanced.n_threads = balanced_threads.min(host_threads.max(1)).max(1);
    if !same_params(&balanced, &stable_base_params) {
        candidates.push(BenchmarkCandidate {
            label: "balanced".to_string(),
            benchmark_label: format!("{benchmark_prefix}/balanced"),
            estimated_bytes: estimate_model_bytes(model_size_bytes, &balanced),
            params: balanced,
        });
    }

    if stable_base_params.n_gpu_layers != 0 {
        if let Some(gpu) = preferred_gpu {
            let single_gpu_full = single_gpu_params(&stable_base_params, gpu, -1);
            if !same_params(&single_gpu_full, &stable_base_params) {
                candidates.push(BenchmarkCandidate {
                    label: "single_gpu_full".to_string(),
                    benchmark_label: format!("{benchmark_prefix}/single_gpu_full"),
                    estimated_bytes: estimate_model_bytes(model_size_bytes, &single_gpu_full),
                    params: single_gpu_full,
                });
            }

            if let Some(block_count) = model_block_count {
                for layers in
                    partial_gpu_layer_targets(block_count, gpu.total_vram_bytes, model_size_bytes)
                {
                    let partial = single_gpu_params(&stable_base_params, gpu, layers as i32);
                    candidates.push(BenchmarkCandidate {
                        label: format!("single_gpu_partial_{layers}"),
                        benchmark_label: format!("{benchmark_prefix}/single_gpu_partial_{layers}"),
                        estimated_bytes: estimate_model_bytes(model_size_bytes, &partial),
                        params: partial,
                    });
                }
            }
        }

        let mut cpu_safe = stable_base_params.clone();
        cpu_safe.n_gpu_layers = 0;
        cpu_safe.split_mode = LlamaGpuSplitMode::None;
        cpu_safe.main_gpu = None;
        cpu_safe.device_indices.clear();
        cpu_safe.n_threads = (host_threads / 2).max(1);
        if !same_params(&cpu_safe, &stable_base_params) {
            candidates.push(BenchmarkCandidate {
                label: "cpu_safe".to_string(),
                benchmark_label: format!("{benchmark_prefix}/cpu_safe"),
                estimated_bytes: estimate_model_bytes(model_size_bytes, &cpu_safe),
                params: cpu_safe,
            });
        }
    }

    dedup_candidates(candidates)
}

fn stabilize_benchmark_base_params(
    base_params: &LlamaEngineParams,
    preferred_gpu: Option<BenchmarkGpuDevice>,
    visible_gpu_count: usize,
) -> (LlamaEngineParams, &'static str) {
    if base_params.n_gpu_layers == 0 {
        return (base_params.clone(), "current");
    }

    let has_explicit_single_gpu = matches!(
        (
            base_params.main_gpu,
            base_params.device_indices.len(),
            base_params.split_mode
        ),
        (Some(_), _, _) | (_, 1, _)
    );
    if has_explicit_single_gpu || visible_gpu_count <= 1 {
        return (base_params.clone(), "current");
    }

    if let Some(gpu) = preferred_gpu {
        return (
            single_gpu_params(base_params, gpu, base_params.n_gpu_layers),
            "current_single_gpu",
        );
    }

    (base_params.clone(), "current")
}

fn single_gpu_params(
    base_params: &LlamaEngineParams,
    gpu: BenchmarkGpuDevice,
    n_gpu_layers: i32,
) -> LlamaEngineParams {
    let mut params = base_params.clone();
    params.n_gpu_layers = n_gpu_layers;
    params.split_mode = LlamaGpuSplitMode::Layer;
    params.main_gpu = None;
    params.device_indices = vec![gpu.index];
    params
}

fn partial_gpu_layer_targets(
    block_count: u32,
    total_vram_bytes: Option<u64>,
    model_size_bytes: u64,
) -> Vec<u32> {
    if block_count == 0 {
        return Vec::new();
    }

    let conservative_cap = total_vram_bytes
        .filter(|bytes| *bytes > 0 && model_size_bytes > 0)
        .map(|bytes| {
            let usable_ratio = (bytes as f64 * 0.72) / model_size_bytes as f64;
            ((block_count as f64 * usable_ratio).floor() as u32).clamp(1, block_count)
        });
    let ratio_targets = [3_u32, 2_u32, 1_u32]
        .into_iter()
        .map(|numerator| block_count.saturating_mul(numerator).saturating_div(4))
        .filter(|layers| *layers > 0);

    let mut targets = std::collections::BTreeSet::new();
    targets.insert((block_count / 3).max(1));
    targets.extend(ratio_targets);
    if let Some(cap) = conservative_cap {
        targets.insert(cap);
        targets.insert((cap.saturating_mul(3) / 4).max(1));
        targets.insert((cap / 2).max(1));
    }
    targets.remove(&block_count);
    targets.into_iter().rev().collect()
}

fn preferred_benchmark_gpu_device(
    base_params: &LlamaEngineParams,
    visible_gpu_devices: &[usize],
) -> Option<BenchmarkGpuDevice> {
    if let Some(main_gpu) = base_params
        .main_gpu
        .and_then(|value| usize::try_from(value).ok())
    {
        return Some(BenchmarkGpuDevice {
            index: main_gpu,
            total_vram_bytes: nvidia_gpu_total_vram_bytes(main_gpu),
        });
    }

    if base_params.split_mode == LlamaGpuSplitMode::None
        && let Some(index) = base_params.device_indices.first().copied()
    {
        return Some(BenchmarkGpuDevice {
            index,
            total_vram_bytes: nvidia_gpu_total_vram_bytes(index),
        });
    }

    let nvidia_preferred = visible_gpu_devices
        .iter()
        .copied()
        .filter_map(|index| {
            nvidia_gpu_total_vram_bytes(index).map(|total_vram_bytes| BenchmarkGpuDevice {
                index,
                total_vram_bytes: Some(total_vram_bytes),
            })
        })
        .max_by_key(|gpu| gpu.total_vram_bytes.unwrap_or_default());
    if nvidia_preferred.is_some() {
        return nvidia_preferred;
    }

    visible_gpu_devices
        .first()
        .copied()
        .map(|index| BenchmarkGpuDevice {
            index,
            total_vram_bytes: None,
        })
}

fn nvidia_gpu_total_vram_bytes(target_index: usize) -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout.lines().find_map(|line| {
        let mut parts = line.split(',').map(str::trim);
        let index = parts.next()?.parse::<usize>().ok()?;
        let memory_mib = parts.next()?.parse::<u64>().ok()?;
        if index == target_index {
            Some(memory_mib.saturating_mul(1024 * 1024))
        } else {
            None
        }
    })
}

fn dedup_candidates(candidates: Vec<BenchmarkCandidate>) -> Vec<BenchmarkCandidate> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        let key = candidate_key(&candidate.params);
        if seen.insert(key) {
            out.push(candidate);
        }
    }
    out
}

fn same_params(left: &LlamaEngineParams, right: &LlamaEngineParams) -> bool {
    candidate_key(left) == candidate_key(right)
}

fn candidate_key(params: &LlamaEngineParams) -> String {
    format!(
        "{}|{}|{}|{:?}|{:?}|{:?}",
        params.n_threads,
        params.n_gpu_layers,
        params.split_mode.as_str(),
        params.main_gpu,
        params.device_indices,
        params.n_ctx
    )
}

fn estimate_model_bytes(model_size_bytes: u64, params: &LlamaEngineParams) -> u64 {
    let base = model_size_bytes.saturating_mul(3).saturating_div(2);
    let thread_overhead = u64::from(params.n_threads.max(1)).saturating_mul(16 * 1024 * 1024);
    let ctx_overhead = u64::from(params.n_ctx.max(1)).saturating_mul(2048);
    base.saturating_add(thread_overhead)
        .saturating_add(ctx_overhead)
}

async fn benchmark_candidate(
    model_path: &Path,
    candidate: &BenchmarkCandidate,
) -> BenchmarkOutcome {
    let sampling = SamplingParams {
        temperature: 0.2,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.05,
        max_tokens: 96,
    };

    let rss_before_bytes = current_process_rss_bytes();
    let load_started = Instant::now();
    let model_path = model_path.to_path_buf();
    let params = candidate.params.clone();
    let load_result = tokio::task::spawn_blocking(move || LlamaEngine::load(&model_path, params))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
    let load_duration_ms = load_started.elapsed().as_millis() as u64;

    let rss_after_load_bytes = current_process_rss_bytes();
    let mut rss_peak_bytes = rss_before_bytes;
    rss_peak_bytes = max_option_u64(rss_peak_bytes, rss_after_load_bytes);

    let engine = match load_result {
        Ok(engine) => engine,
        Err(error) => {
            return BenchmarkOutcome {
                candidate: candidate.clone(),
                load_duration_ms,
                prefill_tokens: 0,
                prefill_duration_ms: 0,
                decode_tokens: 0,
                decode_duration_ms: 0,
                first_token_ms: 0,
                total_duration_ms: 0,
                tokens_per_second: 0.0,
                rss_before_bytes,
                rss_after_load_bytes,
                rss_peak_bytes,
                failure_message: Some(error),
            };
        }
    };

    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: BENCHMARK_SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: BENCHMARK_USER_PROMPT.to_string(),
        },
    ];
    let stream_started = Instant::now();
    let mut first_token_ms = None;
    let mut prefill_tokens = 0_u64;
    let mut prefill_duration_ms = 0_u64;
    let mut decode_tokens = 0_u64;
    let mut decode_duration_ms = 0_u64;
    let mut tokens_per_second = 0.0;
    let mut failure_message = None;
    let raw_stream = engine.chat_stream(messages, sampling);
    futures::pin_mut!(raw_stream);

    while let Some(chunk) = raw_stream.next().await {
        match chunk {
            Ok(ChatChunk::Token(_text)) => {
                decode_tokens = decode_tokens.saturating_add(1);
                if first_token_ms.is_none() {
                    first_token_ms = Some(stream_started.elapsed().as_millis() as u64);
                }
                rss_peak_bytes = max_option_u64(rss_peak_bytes, current_process_rss_bytes());
            }
            Ok(ChatChunk::Stats {
                prompt_tokens,
                completion_tokens,
                prefill_duration_ms: stats_prefill_duration_ms,
                total_duration_ms,
                tokens_per_second: stats_tokens_per_second,
            }) => {
                prefill_tokens = prompt_tokens;
                decode_tokens = completion_tokens;
                prefill_duration_ms = stats_prefill_duration_ms;
                decode_duration_ms = total_duration_ms;
                tokens_per_second = stats_tokens_per_second;
            }
            Ok(ChatChunk::Done) => break,
            Err(error) => {
                failure_message = Some(error.to_string());
                break;
            }
        }
    }

    if first_token_ms.is_none() {
        first_token_ms = Some(decode_duration_ms);
    }
    rss_peak_bytes = max_option_u64(rss_peak_bytes, current_process_rss_bytes());

    BenchmarkOutcome {
        candidate: candidate.clone(),
        load_duration_ms,
        prefill_tokens,
        prefill_duration_ms,
        decode_tokens,
        decode_duration_ms,
        first_token_ms: first_token_ms.unwrap_or_default(),
        total_duration_ms: stream_started.elapsed().as_millis() as u64,
        tokens_per_second,
        rss_before_bytes,
        rss_after_load_bytes,
        rss_peak_bytes,
        failure_message,
    }
}

fn max_option_u64(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

async fn persist_benchmark_progress(
    pool: &rustfin_db::DbPool,
    host_fingerprint: &str,
    model: &AiModelSummary,
    model_path: &Path,
    model_checksum: &str,
    model_size_bytes: u64,
    outcome: &BenchmarkOutcome,
    profile: Option<&ModelProfilePersistence>,
) -> Result<(), AppError> {
    let notes_json = serde_json::json!({
        "label": outcome.candidate.label,
        "benchmark_label": outcome.candidate.benchmark_label,
        "params": {
            "n_threads": outcome.candidate.params.n_threads,
            "n_gpu_layers": outcome.candidate.params.n_gpu_layers,
            "split_mode": outcome.candidate.params.split_mode.as_str(),
            "main_gpu": outcome.candidate.params.main_gpu,
            "device_indices": outcome.candidate.params.device_indices,
            "n_ctx": outcome.candidate.params.n_ctx,
        },
        "estimated_bytes": outcome.candidate.estimated_bytes,
        "rss": {
            "before_bytes": outcome.rss_before_bytes,
            "after_load_bytes": outcome.rss_after_load_bytes,
            "peak_bytes": outcome.rss_peak_bytes,
        },
        "failure": outcome.failure_message,
    })
    .to_string();

    let model_path_string = model_path.to_string_lossy().to_string();
    let benchmark_device_indices_json =
        serde_json::to_string(&outcome.candidate.params.device_indices)
            .unwrap_or_else(|_| "[]".to_string());

    let benchmark_params = rustfin_db::repo::ai_models::UpsertAiModelBenchmarkParams {
        host_fingerprint,
        model_name: &model.name,
        model_checksum,
        model_path: &model_path_string,
        benchmark_label: &outcome.candidate.benchmark_label,
        backend_kind: "local",
        n_threads: i32::try_from(outcome.candidate.params.n_threads).unwrap_or(i32::MAX),
        n_gpu_layers: outcome.candidate.params.n_gpu_layers,
        split_mode: outcome.candidate.params.split_mode.as_str(),
        main_gpu: outcome.candidate.params.main_gpu,
        device_indices_json: &benchmark_device_indices_json,
        load_duration_ms: i64::try_from(outcome.load_duration_ms).unwrap_or(i64::MAX),
        prefill_tokens: i64::try_from(outcome.prefill_tokens).unwrap_or(i64::MAX),
        prefill_duration_ms: i64::try_from(outcome.prefill_duration_ms).unwrap_or(i64::MAX),
        decode_tokens: i64::try_from(outcome.decode_tokens).unwrap_or(i64::MAX),
        decode_duration_ms: i64::try_from(outcome.decode_duration_ms).unwrap_or(i64::MAX),
        first_token_ms: i64::try_from(outcome.first_token_ms).unwrap_or(i64::MAX),
        total_duration_ms: i64::try_from(outcome.total_duration_ms).unwrap_or(i64::MAX),
        tokens_per_second: outcome.tokens_per_second,
        rss_before_bytes: outcome
            .rss_before_bytes
            .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
        rss_after_load_bytes: outcome
            .rss_after_load_bytes
            .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
        rss_peak_bytes: outcome
            .rss_peak_bytes
            .map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
        failure_message: outcome.failure_message.as_deref(),
        notes_json: &notes_json,
    };
    let profile_artifacts = profile.map(|profile| {
        let notes_json = serde_json::json!({
            "model_size_bytes": model_size_bytes,
            "estimated_model_bytes": profile.recommendation.estimated_model_bytes,
            "notes": profile.recommendation.notes,
            "winner": {
                "n_threads": profile.recommendation.recommended_n_threads,
                "n_gpu_layers": profile.recommendation.recommended_n_gpu_layers,
                "split_mode": profile.recommendation.recommended_split_mode,
                "main_gpu": profile.recommendation.recommended_main_gpu,
                "device_indices": profile.recommendation.recommended_device_indices,
            },
        })
        .to_string();
        let device_indices_json =
            serde_json::to_string(&profile.recommendation.recommended_device_indices)
                .unwrap_or_else(|_| "[]".to_string());
        (notes_json, device_indices_json)
    });

    let profile_params = profile
        .zip(profile_artifacts.as_ref())
        .map(
            |(profile, artifacts)| rustfin_db::repo::ai_models::UpsertAiModelProfileParams {
                host_fingerprint,
                model_name: &model.name,
                model_checksum,
                model_path: &model_path_string,
                context_window: i32::try_from(profile.recommendation.context_window)
                    .unwrap_or(i32::MAX),
                preferred_completion_tokens: i32::try_from(
                    profile.recommendation.preferred_completion_tokens,
                )
                .unwrap_or(i32::MAX),
                planner_max_output: i32::try_from(profile.recommendation.planner_max_output)
                    .unwrap_or(i32::MAX),
                summary_max_output: i32::try_from(profile.recommendation.summary_max_output)
                    .unwrap_or(i32::MAX),
                safety_headroom: i32::try_from(profile.recommendation.safety_headroom)
                    .unwrap_or(i32::MAX),
                warmup_cost_class: warmup_cost_class_to_str(
                    profile.recommendation.warmup_cost_class,
                ),
                supports_structured_output: profile.recommendation.supports_structured_output,
                supports_prompt_cache: profile.recommendation.supports_prompt_cache,
                recommended_n_threads: i32::try_from(profile.recommendation.recommended_n_threads)
                    .unwrap_or(i32::MAX),
                recommended_n_gpu_layers: profile.recommendation.recommended_n_gpu_layers,
                recommended_split_mode: &profile.recommendation.recommended_split_mode,
                recommended_main_gpu: profile.recommendation.recommended_main_gpu,
                recommended_device_indices_json: &artifacts.1,
                estimated_model_bytes: i64::try_from(profile.recommendation.estimated_model_bytes)
                    .unwrap_or(i64::MAX),
                notes_json: &artifacts.0,
                last_benchmark_label: &profile.last_benchmark_label,
                last_load_duration_ms: i64::try_from(profile.last_load_duration_ms)
                    .unwrap_or(i64::MAX),
                last_tokens_per_second: profile.last_tokens_per_second,
                benchmark_count: profile.benchmark_count,
            },
        );

    let row = rustfin_db::repo::ai_models::upsert_benchmark_and_profile(
        pool,
        benchmark_params,
        profile_params,
    )
    .await
    .map_err(|error| {
        AppError::from(rustfin_core::error::ApiError::Internal(format!(
            "failed to persist AI benchmark progress: {error}"
        )))
    })?;

    let _ = row;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_profile_persistence(
    host_fingerprint: &str,
    model: &AiModelSummary,
    model_checksum: &str,
    model_size_bytes: u64,
    host_threads: u32,
    benchmark_count: i64,
    outcomes: &[BenchmarkOutcome],
) -> Option<ModelProfilePersistence> {
    if !outcomes
        .iter()
        .any(|outcome| outcome.failure_message.is_none())
    {
        return None;
    }

    let recommendation = recommend_model_profile(
        host_fingerprint,
        model,
        model_checksum,
        model_size_bytes,
        host_threads,
        outcomes,
    );
    let winner = select_preferred_benchmark_outcome(outcomes)
        .filter(|outcome| outcome.failure_message.is_none())?;

    Some(ModelProfilePersistence {
        recommendation,
        benchmark_count,
        last_benchmark_label: winner.candidate.benchmark_label.clone(),
        last_load_duration_ms: winner.load_duration_ms,
        last_tokens_per_second: winner.tokens_per_second,
    })
}

fn warmup_cost_class_to_str(value: WarmupCostClass) -> &'static str {
    match value {
        WarmupCostClass::Low => "low",
        WarmupCostClass::Medium => "medium",
        WarmupCostClass::High => "high",
        WarmupCostClass::Extreme => "extreme",
    }
}

fn select_preferred_benchmark_outcome(outcomes: &[BenchmarkOutcome]) -> Option<&BenchmarkOutcome> {
    let successful = outcomes
        .iter()
        .filter(|outcome| outcome.failure_message.is_none())
        .collect::<Vec<_>>();
    let successful_gpu = successful
        .iter()
        .copied()
        .filter(|outcome| is_gpu_candidate(&outcome.candidate.params))
        .collect::<Vec<_>>();

    if let Some(winner) = select_fastest_benchmark_outcome(&successful_gpu) {
        return Some(winner);
    }

    select_fastest_benchmark_outcome(&successful).or_else(|| outcomes.first())
}

fn select_fastest_benchmark_outcome<'a>(
    outcomes: &[&'a BenchmarkOutcome],
) -> Option<&'a BenchmarkOutcome> {
    outcomes
        .iter()
        .copied()
        .max_by(|left, right| compare_benchmark_outcomes(left, right))
}

fn compare_benchmark_outcomes(
    left: &BenchmarkOutcome,
    right: &BenchmarkOutcome,
) -> std::cmp::Ordering {
    left.tokens_per_second
        .partial_cmp(&right.tokens_per_second)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| right.load_duration_ms.cmp(&left.load_duration_ms))
        .then_with(|| {
            left.rss_peak_bytes
                .unwrap_or_default()
                .cmp(&right.rss_peak_bytes.unwrap_or_default())
                .reverse()
        })
}

fn is_gpu_candidate(params: &LlamaEngineParams) -> bool {
    params.n_gpu_layers != 0
        && (params.split_mode != LlamaGpuSplitMode::None
            || params.main_gpu.is_some()
            || !params.device_indices.is_empty())
}

fn recommend_model_profile(
    host_fingerprint: &str,
    model: &AiModelSummary,
    model_checksum: &str,
    model_size_bytes: u64,
    host_threads: u32,
    outcomes: &[BenchmarkOutcome],
) -> ModelProfileRecommendation {
    let winner = select_preferred_benchmark_outcome(outcomes)
        .expect("benchmark outcomes should not be empty");

    let preferred_completion_tokens: u32 = if winner.tokens_per_second >= 25.0 {
        1024
    } else if winner.tokens_per_second >= 12.0 {
        768
    } else if winner.tokens_per_second >= 6.0 {
        512
    } else {
        256
    };
    let planner_max_output = preferred_completion_tokens
        .saturating_div(2)
        .clamp(128, 512);
    let summary_max_output = preferred_completion_tokens.saturating_div(3).clamp(96, 384);
    let safety_headroom = (model.context_length.unwrap_or(4096) / 6).clamp(128, 1024);
    let warmup_cost_class =
        if winner.load_duration_ms <= 5_000 && model_size_bytes <= 2 * 1024 * 1024 * 1024 {
            WarmupCostClass::Low
        } else if winner.load_duration_ms <= 15_000 || model_size_bytes <= 6 * 1024 * 1024 * 1024 {
            WarmupCostClass::Medium
        } else if winner.load_duration_ms <= 45_000 || model_size_bytes <= 12 * 1024 * 1024 * 1024 {
            WarmupCostClass::High
        } else {
            WarmupCostClass::Extreme
        };
    let estimated_model_bytes = winner
        .rss_peak_bytes
        .unwrap_or(model_size_bytes)
        .max(model_size_bytes);

    let notes = vec![
        format!("winner: {}", winner.candidate.label),
        format!("load: {}ms", winner.load_duration_ms),
        format!("tps: {:.2}", winner.tokens_per_second),
        format!("prefill: {}ms", winner.prefill_duration_ms),
        format!("decode: {}ms", winner.decode_duration_ms),
        format!("host_threads: {host_threads}"),
        format!("host: {host_fingerprint}"),
        format!("model_name: {}", model.name),
        format!("checksum: {model_checksum}"),
        format!("estimated_bytes: {}", winner.candidate.estimated_bytes),
    ];

    ModelProfileRecommendation {
        model_name: model.name.clone(),
        model_checksum: model_checksum.to_string(),
        host_fingerprint: host_fingerprint.to_string(),
        context_window: model.context_length.unwrap_or(4096),
        preferred_completion_tokens,
        planner_max_output,
        summary_max_output,
        safety_headroom,
        warmup_cost_class,
        supports_structured_output: true,
        supports_prompt_cache: false,
        recommended_n_threads: winner.candidate.params.n_threads,
        recommended_n_gpu_layers: winner.candidate.params.n_gpu_layers,
        recommended_split_mode: winner.candidate.params.split_mode.as_str().to_string(),
        recommended_main_gpu: winner.candidate.params.main_gpu,
        recommended_device_indices: winner.candidate.params.device_indices.clone(),
        estimated_model_bytes,
        notes,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_model() -> AiModelSummary {
        AiModelSummary {
            name: "sample".to_string(),
            file: "sample.gguf".to_string(),
            size_gb: 2.0,
            parameter_size: Some("3B".to_string()),
            quantization: Some("Q4_K_M".to_string()),
            architecture: Some("llama".to_string()),
            context_length: Some(4096),
        }
    }

    #[test]
    fn recommend_model_profile_prefers_gpu_candidate_over_faster_cpu_fallback() {
        let model = sample_model();
        let candidates = vec![
            BenchmarkOutcome {
                candidate: BenchmarkCandidate {
                    label: "gpu_partial".to_string(),
                    benchmark_label: "gpu_partial".to_string(),
                    params: LlamaEngineParams {
                        n_gpu_layers: 18,
                        tensor_split: vec![],
                        split_mode: LlamaGpuSplitMode::None,
                        main_gpu: Some(1),
                        device_indices: vec![1],
                        n_ctx: 4096,
                        n_threads: 4,
                    },
                    estimated_bytes: 0,
                },
                load_duration_ms: 20_000,
                prefill_tokens: 32,
                prefill_duration_ms: 250,
                decode_tokens: 64,
                decode_duration_ms: 1_000,
                first_token_ms: 300,
                total_duration_ms: 1_250,
                tokens_per_second: 18.0,
                rss_before_bytes: None,
                rss_after_load_bytes: None,
                rss_peak_bytes: Some(4 * 1024 * 1024 * 1024),
                failure_message: None,
            },
            BenchmarkOutcome {
                candidate: BenchmarkCandidate {
                    label: "cpu_safe".to_string(),
                    benchmark_label: "cpu_safe".to_string(),
                    params: LlamaEngineParams {
                        n_gpu_layers: 0,
                        tensor_split: vec![],
                        split_mode: LlamaGpuSplitMode::None,
                        main_gpu: None,
                        device_indices: vec![],
                        n_ctx: 4096,
                        n_threads: 8,
                    },
                    estimated_bytes: 0,
                },
                load_duration_ms: 10_000,
                prefill_tokens: 32,
                prefill_duration_ms: 200,
                decode_tokens: 64,
                decode_duration_ms: 900,
                first_token_ms: 220,
                total_duration_ms: 1_100,
                tokens_per_second: 28.0,
                rss_before_bytes: None,
                rss_after_load_bytes: None,
                rss_peak_bytes: Some(3 * 1024 * 1024 * 1024),
                failure_message: None,
            },
        ];

        let recommendation = recommend_model_profile(
            "host",
            &model,
            "checksum",
            2 * 1024 * 1024 * 1024,
            8,
            &candidates,
        );

        assert_eq!(recommendation.recommended_n_threads, 4);
        assert_eq!(recommendation.recommended_n_gpu_layers, 18);
        assert_eq!(recommendation.recommended_split_mode, "none");
        assert_eq!(recommendation.recommended_main_gpu, Some(1));
        assert!(matches!(
            recommendation.warmup_cost_class,
            WarmupCostClass::Medium
        ));
        assert!(recommendation.supports_structured_output);
        assert_eq!(recommendation.recommended_device_indices, vec![1]);
    }

    #[test]
    fn benchmark_candidates_include_cpu_safe_variant_when_gpu_layers_are_enabled() {
        let model = sample_model();
        let model_path = std::path::PathBuf::from("/tmp/sample.gguf");
        let candidates = benchmark_candidates(&model, &model_path, 2_000_000_000, 8, "admin");
        assert!(!candidates.is_empty());
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "current")
        );
    }

    #[test]
    fn build_benchmark_candidates_add_partial_single_gpu_variants() {
        let candidates = build_benchmark_candidates(
            &LlamaEngineParams {
                n_gpu_layers: -1,
                tensor_split: vec![],
                split_mode: LlamaGpuSplitMode::Layer,
                main_gpu: None,
                device_indices: vec![],
                n_ctx: 4096,
                n_threads: 8,
            },
            4_000_000_000,
            8,
            "admin",
            Some(36),
            Some(BenchmarkGpuDevice {
                index: 1,
                total_vram_bytes: Some(6 * 1024 * 1024 * 1024),
            }),
            2,
        );

        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "current_single_gpu")
        );
        let current_single_gpu = candidates
            .iter()
            .find(|candidate| candidate.label == "current_single_gpu")
            .expect("current_single_gpu candidate should exist");
        assert_eq!(
            current_single_gpu.params.split_mode,
            LlamaGpuSplitMode::Layer
        );
        assert_eq!(current_single_gpu.params.main_gpu, None);
        assert_eq!(current_single_gpu.params.device_indices, vec![1]);
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label.starts_with("single_gpu_partial_"))
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "cpu_safe")
        );
    }

    #[test]
    fn partial_gpu_layer_targets_stay_below_full_offload() {
        let targets = partial_gpu_layer_targets(36, Some(6 * 1024 * 1024 * 1024), 4_000_000_000);
        assert!(!targets.is_empty());
        assert!(targets.iter().all(|layers| *layers > 0 && *layers < 36));
        assert!(targets.windows(2).all(|window| window[0] >= window[1]));
    }

    #[test]
    fn build_profile_persistence_skips_all_failure_runs() {
        let model = sample_model();
        let outcomes = vec![BenchmarkOutcome {
            candidate: BenchmarkCandidate {
                label: "gpu_partial".to_string(),
                benchmark_label: "bench/gpu_partial".to_string(),
                params: LlamaEngineParams {
                    n_gpu_layers: 18,
                    tensor_split: vec![],
                    split_mode: LlamaGpuSplitMode::Layer,
                    main_gpu: None,
                    device_indices: vec![1],
                    n_ctx: 4096,
                    n_threads: 4,
                },
                estimated_bytes: 0,
            },
            load_duration_ms: 0,
            prefill_tokens: 0,
            prefill_duration_ms: 0,
            decode_tokens: 0,
            decode_duration_ms: 0,
            first_token_ms: 0,
            total_duration_ms: 0,
            tokens_per_second: 0.0,
            rss_before_bytes: None,
            rss_after_load_bytes: None,
            rss_peak_bytes: None,
            failure_message: Some("load failed".to_string()),
        }];

        assert!(
            build_profile_persistence(
                "host",
                &model,
                "checksum",
                2 * 1024 * 1024 * 1024,
                8,
                3,
                &outcomes,
            )
            .is_none()
        );
    }

    #[test]
    fn build_profile_persistence_uses_best_success_so_far() {
        let model = sample_model();
        let outcomes = vec![
            BenchmarkOutcome {
                candidate: BenchmarkCandidate {
                    label: "cpu_safe".to_string(),
                    benchmark_label: "bench/cpu_safe".to_string(),
                    params: LlamaEngineParams {
                        n_gpu_layers: 0,
                        tensor_split: vec![],
                        split_mode: LlamaGpuSplitMode::None,
                        main_gpu: None,
                        device_indices: vec![],
                        n_ctx: 4096,
                        n_threads: 8,
                    },
                    estimated_bytes: 0,
                },
                load_duration_ms: 10_000,
                prefill_tokens: 32,
                prefill_duration_ms: 200,
                decode_tokens: 64,
                decode_duration_ms: 900,
                first_token_ms: 220,
                total_duration_ms: 1_100,
                tokens_per_second: 28.0,
                rss_before_bytes: None,
                rss_after_load_bytes: None,
                rss_peak_bytes: Some(3 * 1024 * 1024 * 1024),
                failure_message: None,
            },
            BenchmarkOutcome {
                candidate: BenchmarkCandidate {
                    label: "gpu_full".to_string(),
                    benchmark_label: "bench/gpu_full".to_string(),
                    params: LlamaEngineParams {
                        n_gpu_layers: -1,
                        tensor_split: vec![],
                        split_mode: LlamaGpuSplitMode::Layer,
                        main_gpu: None,
                        device_indices: vec![1],
                        n_ctx: 4096,
                        n_threads: 8,
                    },
                    estimated_bytes: 0,
                },
                load_duration_ms: 15_000,
                prefill_tokens: 32,
                prefill_duration_ms: 250,
                decode_tokens: 64,
                decode_duration_ms: 1_000,
                first_token_ms: 300,
                total_duration_ms: 1_250,
                tokens_per_second: 24.0,
                rss_before_bytes: None,
                rss_after_load_bytes: None,
                rss_peak_bytes: Some(4 * 1024 * 1024 * 1024),
                failure_message: None,
            },
            BenchmarkOutcome {
                candidate: BenchmarkCandidate {
                    label: "late_failure".to_string(),
                    benchmark_label: "bench/late_failure".to_string(),
                    params: LlamaEngineParams {
                        n_gpu_layers: -1,
                        tensor_split: vec![],
                        split_mode: LlamaGpuSplitMode::Layer,
                        main_gpu: None,
                        device_indices: vec![1],
                        n_ctx: 4096,
                        n_threads: 8,
                    },
                    estimated_bytes: 0,
                },
                load_duration_ms: 0,
                prefill_tokens: 0,
                prefill_duration_ms: 0,
                decode_tokens: 0,
                decode_duration_ms: 0,
                first_token_ms: 0,
                total_duration_ms: 0,
                tokens_per_second: 0.0,
                rss_before_bytes: None,
                rss_after_load_bytes: None,
                rss_peak_bytes: None,
                failure_message: Some("late crash".to_string()),
            },
        ];

        let profile = build_profile_persistence(
            "host",
            &model,
            "checksum",
            2 * 1024 * 1024 * 1024,
            8,
            4,
            &outcomes,
        )
        .expect("profile should persist after at least one successful candidate");

        assert_eq!(profile.benchmark_count, 4);
        assert_eq!(profile.last_benchmark_label, "bench/gpu_full");
        assert_eq!(profile.last_load_duration_ms, 15_000);
        assert_eq!(profile.last_tokens_per_second, 24.0);
        assert_eq!(profile.recommendation.recommended_n_gpu_layers, -1);
        assert_eq!(profile.recommendation.recommended_split_mode, "layer");
        assert_eq!(profile.recommendation.recommended_device_indices, vec![1]);
    }
}
