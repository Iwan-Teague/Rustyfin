use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::HeaderMap,
    routing::{get, post},
};
use base64::Engine as _;
use rustfin_core::agent_auth::{normalize_secret, verify_agent_token};
use rustfin_core::axum_error::AppError;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const MAX_REQUEST_BYTES: usize = 1_000_000;
const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;
const MIN_MODEL_BYTES: usize = 8 * 1024 * 1024;
const MAX_TRANSCRIBE_SECONDS: u64 = 120;

#[derive(Debug, Clone, Copy)]
enum TranscriptionGpuMode {
    OpenCl,
    Cuda,
    Hip,
}

impl TranscriptionGpuMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::OpenCl => "opencl",
            Self::Cuda => "cuda",
            Self::Hip => "hip",
        }
    }
}

fn parse_gpu_mode(raw: &str) -> anyhow::Result<TranscriptionGpuMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "opencl" | "auto" | "off" => Ok(TranscriptionGpuMode::OpenCl),
        "cuda" => Ok(TranscriptionGpuMode::Cuda),
        "hip" | "hipblas" => Ok(TranscriptionGpuMode::Hip),
        other => Err(anyhow::anyhow!(
            "invalid RUSTFIN_TRANSCRIPTION_GPU_MODE '{other}' (expected opencl, cuda, hip, or auto)"
        )),
    }
}

#[derive(Clone)]
struct AppState {
    model_path: PathBuf,
    model_url: Option<String>,
    model_init_lock: Arc<tokio::sync::Mutex<()>>,
    http: reqwest::Client,
    agent_token: Option<String>,
    gpu_mode: TranscriptionGpuMode,
    gpu_required: bool,
    gpu_ready: bool,
    gpu_error_message: Option<String>,
    workers: WorkerRegistry,
    inference_permits: Arc<tokio::sync::Semaphore>,
    inference_acquire_timeout: Duration,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionControlRequest {
    session_id: String,
}

#[derive(Debug, Serialize)]
struct SessionControlResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscribeChunkRequest {
    session_id: String,
    user_id: String,
    username: String,
    sample_rate_hz: u32,
    started_ts_ms: i64,
    ended_ts_ms: i64,
    pcm_s16le_base64: String,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Debug, Serialize)]
struct TranscribeChunkResponse {
    segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptSegment {
    started_ts_ms: i64,
    ended_ts_ms: i64,
    text: String,
}

#[derive(Debug, Clone)]
struct SegmentOffset {
    start_offset_ms: i64,
    end_offset_ms: i64,
    text: String,
}

#[derive(Clone)]
struct WorkerRegistry {
    inner: Arc<Mutex<HashMap<String, WorkerHandle>>>,
    max_workers: usize,
    max_workers_per_session: usize,
    max_threads_per_worker: i32,
}

#[derive(Clone)]
struct WorkerHandle {
    tx: mpsc::Sender<WorkerCommand>,
}

enum WorkerCommand {
    Transcribe {
        audio_f32: Vec<f32>,
        language: Option<String>,
        respond_to: tokio::sync::oneshot::Sender<Result<Vec<SegmentOffset>, String>>,
    },
    Shutdown,
}

fn normalize_identifier(raw: &str, label: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(ApiError::BadRequest(format!("{label} is invalid")).into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::BadRequest(format!("{label} is invalid")).into());
    }
    Ok(trimmed.to_string())
}

impl WorkerRegistry {
    fn new(
        max_workers: usize,
        max_workers_per_session: usize,
        max_threads_per_worker: i32,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_workers,
            max_workers_per_session,
            max_threads_per_worker: max_threads_per_worker.clamp(1, 8),
        }
    }

    fn key(session_id: &str, user_id: &str) -> String {
        format!("{session_id}::{user_id}")
    }

    fn get_or_spawn(
        &self,
        session_id: &str,
        user_id: &str,
        model_path: &Path,
    ) -> Result<WorkerHandle, AppError> {
        let key = Self::key(session_id, user_id);
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| ApiError::Internal("failed to lock worker registry".into()))?;
        if let Some(handle) = guard.get(&key).cloned() {
            return Ok(handle);
        }

        let session_prefix = format!("{session_id}::");
        let session_worker_count = guard
            .keys()
            .filter(|existing| existing.starts_with(&session_prefix))
            .count();
        if session_worker_count >= self.max_workers_per_session {
            return Err(ApiError::BadRequest(format!(
                "transcription capacity reached for this session (max {} dedicated speaker workers); try again",
                self.max_workers_per_session
            ))
            .into());
        }
        if guard.len() >= self.max_workers {
            return Err(ApiError::TooManyRequests {
                retry_after_seconds: 2,
            }
            .into());
        }

        let (tx, rx) = mpsc::channel::<WorkerCommand>();
        let worker_model_path = model_path.to_path_buf();
        let max_threads_per_worker = self.max_threads_per_worker;
        let worker_name = format!("whisper-{session_id}-{user_id}");
        std::thread::Builder::new()
            .name(worker_name)
            .spawn(move || worker_loop(worker_model_path, rx, max_threads_per_worker))
            .map_err(|e| ApiError::Internal(format!("failed to spawn whisper worker: {e}")))?;

        let handle = WorkerHandle { tx };
        guard.insert(key, handle.clone());
        Ok(handle)
    }

    fn shutdown_session(&self, session_id: &str) {
        let prefix = format!("{session_id}::");
        let mut removed: Vec<WorkerHandle> = Vec::new();
        if let Ok(mut guard) = self.inner.lock() {
            let keys: Vec<String> = guard
                .keys()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            for key in keys {
                if let Some(handle) = guard.remove(&key) {
                    removed.push(handle);
                }
            }
        }
        for worker in removed {
            let _ = worker.tx.send(WorkerCommand::Shutdown);
        }
    }
}

fn worker_loop(model_path: PathBuf, rx: mpsc::Receiver<WorkerCommand>, max_threads: i32) {
    let Some(model_path_str) = model_path.to_str() else {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                WorkerCommand::Transcribe { respond_to, .. } => {
                    let _ = respond_to.send(Err("whisper model path is not valid utf-8".into()));
                }
                WorkerCommand::Shutdown => break,
            }
        }
        return;
    };

    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(true);
    let context = match WhisperContext::new_with_params(model_path_str, context_params) {
        Ok(ctx) => ctx,
        Err(err) => {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    WorkerCommand::Transcribe { respond_to, .. } => {
                        let _ =
                            respond_to.send(Err(format!("failed to load whisper model: {err}")));
                    }
                    WorkerCommand::Shutdown => break,
                }
            }
            return;
        }
    };

    while let Ok(cmd) = rx.recv() {
        match cmd {
            WorkerCommand::Shutdown => break,
            WorkerCommand::Transcribe {
                audio_f32,
                language,
                respond_to,
            } => {
                let result =
                    transcribe_audio_chunk(&context, &audio_f32, language.as_deref(), max_threads);
                let _ = respond_to.send(result);
            }
        }
    }
}

fn transcribe_audio_chunk(
    context: &WhisperContext,
    audio_f32: &[f32],
    language: Option<&str>,
    max_threads: i32,
) -> Result<Vec<SegmentOffset>, String> {
    if audio_f32.is_empty() {
        return Ok(Vec::new());
    }

    let mut state = context
        .create_state()
        .map_err(|e| format!("failed to create whisper state: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_translate(false);
    params.set_n_threads(max_threads.clamp(1, 8));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    // Keep per-user worker context between chunks to improve recognition continuity.
    params.set_no_context(false);
    if let Some(lang) = language {
        let trimmed = lang.trim();
        if !trimmed.is_empty() {
            params.set_language(Some(trimmed));
        }
    }

    state
        .full(params, audio_f32)
        .map_err(|e| format!("whisper transcription failed: {e}"))?;

    let segment_count = state
        .full_n_segments()
        .map_err(|e| format!("failed to read whisper segments: {e}"))?;
    let mut segments = Vec::new();
    for idx in 0..segment_count {
        let text = state
            .full_get_segment_text(idx)
            .map_err(|e| format!("failed to read whisper segment text: {e}"))?
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let start_offset_ms = state
            .full_get_segment_t0(idx)
            .map_err(|e| format!("failed to read whisper segment start: {e}"))?
            * 10;
        let end_offset_ms = state
            .full_get_segment_t1(idx)
            .map_err(|e| format!("failed to read whisper segment end: {e}"))?
            * 10;
        segments.push(SegmentOffset {
            start_offset_ms,
            end_offset_ms,
            text,
        });
    }

    Ok(segments)
}

fn decode_pcm_s16le(base64_value: &str) -> Result<Vec<i16>, AppError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(base64_value.as_bytes())
        .map_err(|_| ApiError::BadRequest("pcm_s16le_base64 is not valid base64".into()))?;

    if decoded.is_empty() {
        return Ok(Vec::new());
    }
    if decoded.len() % 2 != 0 {
        return Err(ApiError::BadRequest("pcm_s16le payload has odd byte length".into()).into());
    }

    let mut out = Vec::with_capacity(decoded.len() / 2);
    for chunk in decoded.chunks_exact(2) {
        out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(out)
}

fn pcm_i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|s| f32::from(*s) / 32768.0).collect()
}

fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if input.is_empty() || from_hz == to_hz {
        return input.to_vec();
    }
    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let source_pos = (i as f64) / ratio;
        let idx = source_pos.floor() as usize;
        let frac = source_pos - idx as f64;
        let s0 = *input.get(idx).unwrap_or(&0.0);
        let s1 = *input.get(idx + 1).unwrap_or(&s0);
        output.push((s0 as f64 + (s1 as f64 - s0 as f64) * frac) as f32);
    }
    output
}

fn parse_env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    let Some(raw) = std::env::var(name).ok() else {
        return default.clamp(min, max);
    };
    match raw.trim().parse::<usize>() {
        Ok(value) => value.clamp(min, max),
        Err(_) => {
            warn!(env = %name, value = %raw, "invalid integer env value; using default");
            default.clamp(min, max)
        }
    }
}

fn parse_env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    let Some(raw) = std::env::var(name).ok() else {
        return default.clamp(min, max);
    };
    match raw.trim().parse::<u64>() {
        Ok(value) => value.clamp(min, max),
        Err(_) => {
            warn!(env = %name, value = %raw, "invalid integer env value; using default");
            default.clamp(min, max)
        }
    }
}

fn parse_env_bool(name: &str, default: bool) -> bool {
    let Some(raw) = std::env::var(name).ok() else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => {
            warn!(env = %name, value = %raw, "invalid boolean env value; using default");
            default
        }
    }
}

fn discover_gpu_device_nodes() -> Vec<String> {
    let mut paths = Vec::new();
    for pattern in ["/dev/dri/renderD*", "/dev/nvidia[0-9]*"] {
        let Ok(entries) = glob::glob(pattern) else {
            continue;
        };
        for entry in entries.flatten() {
            paths.push(entry.display().to_string());
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn run_command_capture(binary: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {binary}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    } else if stderr.is_empty() {
        Err(format!(
            "{binary} exited with status {}; output: {}",
            output.status, stdout
        ))
    } else {
        Err(format!("{binary} failed: {stderr}"))
    }
}

fn detect_gpu_backend_ready(mode: TranscriptionGpuMode) -> Result<(), String> {
    let compiled = match mode {
        TranscriptionGpuMode::OpenCl => cfg!(feature = "gpu-opencl"),
        TranscriptionGpuMode::Cuda => cfg!(feature = "gpu-cuda"),
        TranscriptionGpuMode::Hip => cfg!(feature = "gpu-hip"),
    };
    if !compiled {
        return Err(format!(
            "transcription-agent is not compiled with {} GPU backend support",
            mode.as_str()
        ));
    }

    let gpu_nodes = discover_gpu_device_nodes();
    if gpu_nodes.is_empty() {
        return Err(
            "no GPU device nodes found on the host (/dev/dri/renderD* or /dev/nvidia[0-9]*)"
                .to_string(),
        );
    }

    match mode {
        TranscriptionGpuMode::OpenCl => {
            let output = run_command_capture("clinfo", &["-l"])?;
            if !output.to_ascii_lowercase().contains("gpu") {
                return Err(
                    "OpenCL runtime is present but no GPU-class OpenCL device is reported"
                        .to_string(),
                );
            }
        }
        TranscriptionGpuMode::Cuda => {
            let _ = run_command_capture("nvidia-smi", &["-L"])?;
        }
        TranscriptionGpuMode::Hip => {
            let _ = run_command_capture("rocminfo", &[])?;
        }
    }

    Ok(())
}

fn ensure_gpu_ready(state: &AppState) -> Result<(), AppError> {
    if !state.gpu_required {
        return Ok(());
    }
    if state.gpu_ready {
        return Ok(());
    }
    let detail = state
        .gpu_error_message
        .clone()
        .unwrap_or_else(|| "no usable GPU backend is available for transcription".to_string());
    Err(ApiError::BadRequest(format!(
        "transcription requires a GPU ({}) and cannot run on CPU fallback: {detail}",
        state.gpu_mode.as_str()
    ))
    .into())
}

async fn ensure_model_available(state: &AppState) -> Result<(), AppError> {
    if state.model_path.exists() {
        return Ok(());
    }

    let _guard = state.model_init_lock.lock().await;
    if state.model_path.exists() {
        return Ok(());
    }

    let model_url = state.model_url.clone().ok_or_else(|| {
        ApiError::BadRequest(
            "whisper model file is missing; configure RUSTFIN_WHISPER_MODEL_URL".into(),
        )
    })?;

    if let Some(parent) = state.model_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Internal(format!("failed to create model directory: {e}")))?;
    }

    info!(url = %model_url, path = %state.model_path.display(), "downloading whisper model");
    let response = state
        .http
        .get(&model_url)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("failed to download whisper model: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::Internal(format!(
            "failed to download whisper model: HTTP {}",
            response.status()
        ))
        .into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(format!("failed to read whisper model bytes: {e}")))?;
    if bytes.len() < MIN_MODEL_BYTES {
        return Err(ApiError::Internal(
            "downloaded whisper model is unexpectedly small; aborting".into(),
        )
        .into());
    }

    let tmp_path = state.model_path.with_extension("tmp");
    tokio::fs::write(&tmp_path, &bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to write whisper model: {e}")))?;
    tokio::fs::rename(&tmp_path, &state.model_path)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to finalize whisper model: {e}")))?;
    info!(path = %state.model_path.display(), bytes = bytes.len(), "whisper model ready");
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn start_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionControlRequest>,
) -> Result<Json<SessionControlResponse>, AppError> {
    verify_agent_token(
        &headers,
        state.agent_token.as_deref(),
        "transcription-agent",
    )?;
    normalize_identifier(&body.session_id, "session_id")?;
    ensure_gpu_ready(&state)?;
    ensure_model_available(&state).await?;
    Ok(Json(SessionControlResponse { ok: true }))
}

async fn stop_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionControlRequest>,
) -> Result<Json<SessionControlResponse>, AppError> {
    verify_agent_token(
        &headers,
        state.agent_token.as_deref(),
        "transcription-agent",
    )?;
    let session_id = normalize_identifier(&body.session_id, "session_id")?;
    state.workers.shutdown_session(&session_id);
    Ok(Json(SessionControlResponse { ok: true }))
}

async fn cancel_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SessionControlRequest>,
) -> Result<Json<SessionControlResponse>, AppError> {
    verify_agent_token(
        &headers,
        state.agent_token.as_deref(),
        "transcription-agent",
    )?;
    let session_id = normalize_identifier(&body.session_id, "session_id")?;
    state.workers.shutdown_session(&session_id);
    Ok(Json(SessionControlResponse { ok: true }))
}

async fn transcribe_chunk(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<TranscribeChunkRequest>,
) -> Result<Json<TranscribeChunkResponse>, AppError> {
    verify_agent_token(
        &headers,
        state.agent_token.as_deref(),
        "transcription-agent",
    )?;

    let session_id = normalize_identifier(&body.session_id, "session_id")?;
    let user_id = normalize_identifier(&body.user_id, "user_id")?;
    let _username = body.username.trim();
    if _username.is_empty() {
        return Err(ApiError::BadRequest("username cannot be empty".into()).into());
    }
    if body.started_ts_ms <= 0 || body.ended_ts_ms <= body.started_ts_ms {
        return Err(ApiError::BadRequest("invalid chunk timestamps".into()).into());
    }
    if !(8_000..=96_000).contains(&body.sample_rate_hz) {
        return Err(ApiError::BadRequest("sample_rate_hz is out of supported range".into()).into());
    }

    ensure_gpu_ready(&state)?;
    ensure_model_available(&state).await?;

    let pcm_i16 = decode_pcm_s16le(&body.pcm_s16le_base64)?;
    if pcm_i16.is_empty() {
        return Ok(Json(TranscribeChunkResponse {
            segments: Vec::new(),
        }));
    }

    let chunk_duration_ms = ((pcm_i16.len() as f64 / body.sample_rate_hz as f64) * 1000.0) as i64;
    if chunk_duration_ms <= 0 || chunk_duration_ms > 20_000 {
        return Err(ApiError::BadRequest(
            "audio chunk duration must be between 1ms and 20000ms".into(),
        )
        .into());
    }

    let mut pcm_f32 = pcm_i16_to_f32(&pcm_i16);
    if body.sample_rate_hz != TARGET_SAMPLE_RATE_HZ {
        pcm_f32 = resample_linear(&pcm_f32, body.sample_rate_hz, TARGET_SAMPLE_RATE_HZ);
    }

    let _permit = tokio::time::timeout(
        state.inference_acquire_timeout,
        state.inference_permits.clone().acquire_owned(),
    )
    .await
    .map_err(|_| ApiError::TooManyRequests {
        retry_after_seconds: 2,
    })?
    .map_err(|_| ApiError::Internal("transcription capacity limiter is unavailable".into()))?;

    let worker = state
        .workers
        .get_or_spawn(&session_id, &user_id, &state.model_path)?;
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    worker
        .tx
        .send(WorkerCommand::Transcribe {
            audio_f32: pcm_f32,
            language: body.language.clone(),
            respond_to: reply_tx,
        })
        .map_err(|_| ApiError::Internal("whisper worker is unavailable".into()))?;

    let offsets = tokio::time::timeout(Duration::from_secs(MAX_TRANSCRIBE_SECONDS), reply_rx)
        .await
        .map_err(|_| ApiError::Internal("whisper transcription timed out".into()))?
        .map_err(|_| ApiError::Internal("whisper worker failed to respond".into()))?
        .map_err(|e| ApiError::BadRequest(format!("whisper transcription failed: {e}")))?;

    let mut segments = Vec::new();
    let offset_count = offsets.len();
    let fallback_segment_ms = (chunk_duration_ms / (offset_count.max(1) as i64)).clamp(250, 3_000);
    for seg in offsets {
        let mut started_ts_ms = body.started_ts_ms + seg.start_offset_ms.max(0);
        let mut ended_ts_ms = body.started_ts_ms + seg.end_offset_ms.max(seg.start_offset_ms);
        if started_ts_ms > body.ended_ts_ms {
            started_ts_ms = body.ended_ts_ms;
        }
        if ended_ts_ms > body.ended_ts_ms {
            ended_ts_ms = body.ended_ts_ms;
        }
        if ended_ts_ms <= started_ts_ms {
            let min_end = (started_ts_ms + fallback_segment_ms).min(body.ended_ts_ms);
            if min_end <= started_ts_ms {
                if body.ended_ts_ms > body.started_ts_ms {
                    started_ts_ms = body.started_ts_ms;
                    ended_ts_ms = body.ended_ts_ms;
                } else {
                    continue;
                }
            } else {
                ended_ts_ms = min_end;
            }
        }
        segments.push(TranscriptSegment {
            started_ts_ms,
            ended_ts_ms,
            text: seg.text,
        });
    }

    Ok(Json(TranscribeChunkResponse { segments }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind =
        std::env::var("RUSTFIN_TRANSCRIPTION_AGENT_BIND").unwrap_or_else(|_| "0.0.0.0:8102".into());
    let cache_dir = std::env::var("RUSTFIN_CACHE_DIR").unwrap_or_else(|_| "/cache".into());
    let requested_gpu_mode_raw =
        std::env::var("RUSTFIN_TRANSCRIPTION_GPU_MODE").unwrap_or_else(|_| "opencl".into());
    let gpu_mode = parse_gpu_mode(&requested_gpu_mode_raw)?;
    let gpu_required = parse_env_bool("RUSTFIN_TRANSCRIPTION_REQUIRE_GPU", true);
    let compiled_gpu_backends = format!(
        "opencl={},cuda={},hip={}",
        cfg!(feature = "gpu-opencl"),
        cfg!(feature = "gpu-cuda"),
        cfg!(feature = "gpu-hip")
    );
    let gpu_probe = detect_gpu_backend_ready(gpu_mode);
    let gpu_ready = gpu_probe.is_ok();
    let gpu_error_message = gpu_probe.err();
    let cpu_count = num_cpus::get().max(1);
    let default_parallel_inferences = (cpu_count / 2).clamp(1, 4);
    let max_parallel_inferences = parse_env_usize(
        "RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES",
        default_parallel_inferences,
        1,
        16,
    );
    let max_workers = parse_env_usize(
        "RUSTFIN_TRANSCRIPTION_MAX_WORKERS",
        (max_parallel_inferences * 2).clamp(2, 24),
        1,
        64,
    );
    let max_workers_per_session =
        parse_env_usize("RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION", 8, 1, 64);
    let max_threads_per_worker =
        parse_env_usize("RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER", 2, 1, 8) as i32;
    let acquire_timeout_ms = parse_env_u64(
        "RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS",
        2_500,
        100,
        30_000,
    );

    let model_path = std::env::var("RUSTFIN_WHISPER_MODEL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(cache_dir).join("whisper/ggml-small.en.bin"));
    let model_url = std::env::var("RUSTFIN_WHISPER_MODEL_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            Some(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
                    .to_string(),
            )
        });
    let agent_token = normalize_secret(std::env::var("RUSTFIN_TRANSCRIPTION_AGENT_TOKEN").ok());

    if !model_path.exists() {
        warn!(
            path = %model_path.display(),
            "whisper model is not present at startup; it will be downloaded lazily on first use"
        );
    }

    info!(
        max_parallel_inferences,
        max_workers,
        max_workers_per_session,
        max_threads_per_worker,
        acquire_timeout_ms,
        requested_gpu_mode = gpu_mode.as_str(),
        gpu_required,
        gpu_ready,
        compiled_gpu_backends,
        "transcription-agent resource limits configured"
    );
    if let Some(reason) = gpu_error_message.as_ref() {
        if gpu_required {
            warn!(
                requested_gpu_mode = gpu_mode.as_str(),
                %reason,
                "transcription GPU requirement is not satisfied; transcription start/chunk requests will be rejected"
            );
        } else {
            warn!(
                requested_gpu_mode = gpu_mode.as_str(),
                %reason,
                "transcription GPU probe failed; CPU fallback is currently allowed by configuration"
            );
        }
    }

    let state = AppState {
        model_path,
        model_url,
        model_init_lock: Arc::new(tokio::sync::Mutex::new(())),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("failed to build http client")?,
        agent_token,
        gpu_mode,
        gpu_required,
        gpu_ready,
        gpu_error_message,
        workers: WorkerRegistry::new(max_workers, max_workers_per_session, max_threads_per_worker),
        inference_permits: Arc::new(tokio::sync::Semaphore::new(max_parallel_inferences)),
        inference_acquire_timeout: Duration::from_millis(acquire_timeout_ms),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/sessions/start", post(start_session))
        .route("/v1/sessions/stop", post(stop_session))
        .route("/v1/sessions/cancel", post(cancel_session))
        .route("/v1/transcribe/chunk", post(transcribe_chunk))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind transcription-agent on {bind}"))?;
    info!(addr = %bind, "transcription-agent listening");
    axum::serve(listener, app).await?;
    Ok(())
}
