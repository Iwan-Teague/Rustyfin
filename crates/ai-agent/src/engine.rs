use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use async_stream::stream;
use futures::Stream;
use futures::stream::BoxStream;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::{LlamaModelParams, LlamaSplitMode};
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{LlamaBackendDeviceType, list_llama_ggml_backend_devices};
use tokio::sync::mpsc;
use tracing::warn;

use crate::backend::shared_backend;
use crate::error::AiError;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

#[derive(Debug, Clone)]
pub struct LlamaEngineParams {
    pub n_gpu_layers: i32,
    pub tensor_split: Vec<f32>,
    pub split_mode: LlamaGpuSplitMode,
    pub main_gpu: Option<i32>,
    pub device_indices: Vec<usize>,
    pub n_ctx: u32,
    pub n_threads: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlamaGpuSplitMode {
    None,
    Layer,
    Row,
}

impl LlamaGpuSplitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Layer => "layer",
            Self::Row => "row",
        }
    }
}

impl From<LlamaGpuSplitMode> for LlamaSplitMode {
    fn from(value: LlamaGpuSplitMode) -> Self {
        match value {
            LlamaGpuSplitMode::None => LlamaSplitMode::None,
            LlamaGpuSplitMode::Layer => LlamaSplitMode::Layer,
            LlamaGpuSplitMode::Row => LlamaSplitMode::Row,
        }
    }
}

impl Default for LlamaEngineParams {
    fn default() -> Self {
        Self {
            n_gpu_layers: -1,
            tensor_split: vec![],
            split_mode: LlamaGpuSplitMode::Layer,
            main_gpu: None,
            device_indices: vec![],
            n_ctx: 4096,
            n_threads: 8,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repeat_penalty: f32,
    pub max_tokens: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.8,
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            max_tokens: 2048,
        }
    }
}

#[derive(Clone)]
pub struct LlamaEngine {
    model: Arc<LlamaModel>,
    params: LlamaEngineParams,
}

impl LlamaEngine {
    pub fn load(gguf_path: &Path, params: LlamaEngineParams) -> Result<Self, AiError> {
        if !gguf_path.exists() {
            return Err(AiError::ModelNotFound(gguf_path.display().to_string()));
        }

        if !params.tensor_split.is_empty() {
            warn!(
                tensor_split = ?params.tensor_split,
                "llama-cpp-2 currently does not expose tensor_split in safe model params; using automatic split"
            );
        }

        let backend = shared_backend()?;
        let mut model_params = LlamaModelParams::default();
        let resolved_device_indices = resolve_device_indices(&params);
        if params.n_gpu_layers >= 0 {
            model_params = model_params.with_n_gpu_layers(params.n_gpu_layers as u32);
        }
        if should_apply_split_mode_override(&params, &resolved_device_indices) {
            model_params = model_params.with_split_mode(params.split_mode.into());
        }
        if !resolved_device_indices.is_empty() {
            model_params = model_params
                .with_devices(&resolved_device_indices)
                .map_err(|error| {
                    AiError::ContextError(format!(
                        "failed to select llama backend devices {:?}: {error}",
                        resolved_device_indices
                    ))
                })?;
        }

        if let Some(main_gpu) = params.main_gpu {
            model_params = model_params.with_main_gpu(main_gpu);
        }

        let model = LlamaModel::load_from_file(backend, gguf_path, &model_params)
            .map_err(|error| AiError::InferenceError(format!("failed to load model: {error}")))?;

        let mut resolved_params = params;
        resolved_params.device_indices = resolved_device_indices;

        Ok(Self {
            model: Arc::new(model),
            params: resolved_params,
        })
    }

    pub fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
    ) -> impl Stream<Item = Result<ChatChunk, AiError>> + Send + 'static + use<> {
        let model = self.model.clone();
        let engine_params = self.params.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<Result<ChatChunk, AiError>>();

        tokio::task::spawn_blocking(move || {
            if let Err(error) = run_decode_loop(&model, engine_params, messages, sampling, &tx) {
                let _ = tx.send(Err(error));
            }
        });

        stream! {
            while let Some(item) = rx.recv().await {
                yield item;
            }
        }
    }

    pub fn params(&self) -> &LlamaEngineParams {
        &self.params
    }

    pub fn backend_kind(&self) -> BackendKind {
        BackendKind::Local
    }

    pub fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: self.backend_kind(),
            supports_streaming: true,
            supports_prompt_cache: false,
            supports_structured_output: true,
            can_degrade: true,
            max_parallel_requests: 1,
        }
    }
}

impl crate::backends::PromptBackend for LlamaEngine {
    fn backend_kind(&self) -> BackendKind {
        self.backend_kind()
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.capabilities()
    }

    fn chat_stream_boxed(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        _prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, AiError>> {
        Box::pin(self.chat_stream(messages, sampling))
    }
}

fn resolve_device_indices(params: &LlamaEngineParams) -> Vec<usize> {
    let device_indices = if params.device_indices.is_empty() {
        default_gpu_backend_device_indices()
    } else {
        params.device_indices.clone()
    };

    normalize_device_indices(device_indices, params.split_mode, params.main_gpu)
}

fn should_apply_split_mode_override(
    params: &LlamaEngineParams,
    resolved_device_indices: &[usize],
) -> bool {
    if params.split_mode != LlamaGpuSplitMode::None {
        return true;
    }

    !resolved_device_indices.is_empty() || params.main_gpu.is_some()
}

fn default_gpu_backend_device_indices() -> Vec<usize> {
    list_llama_ggml_backend_devices()
        .into_iter()
        .filter(|device| {
            matches!(
                device.device_type,
                LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
            )
        })
        .map(|device| device.index)
        .collect()
}

fn normalize_device_indices(
    mut device_indices: Vec<usize>,
    split_mode: LlamaGpuSplitMode,
    main_gpu: Option<i32>,
) -> Vec<usize> {
    if split_mode == LlamaGpuSplitMode::None {
        if let Some(main_gpu) = main_gpu.and_then(|value| usize::try_from(value).ok()) {
            device_indices.retain(|index| *index == main_gpu);
            if device_indices.is_empty() {
                device_indices.push(main_gpu);
            }
        } else {
            device_indices.truncate(1);
        }
    }

    device_indices.sort_unstable();
    device_indices.dedup();
    device_indices
}

fn builtin_chat_template_name_for_architecture(architecture: &str) -> Option<&'static str> {
    let normalized = architecture.trim().to_ascii_lowercase();
    if normalized.starts_with("gemma") {
        Some("gemma")
    } else {
        None
    }
}

fn resolve_chat_template(model: &LlamaModel) -> Result<LlamaChatTemplate, AiError> {
    if let Ok(architecture) = model.meta_val_str("general.architecture") {
        if let Some(template_name) = builtin_chat_template_name_for_architecture(&architecture) {
            return LlamaChatTemplate::new(template_name).map_err(|error| {
                AiError::ContextError(format!(
                    "failed to create {template_name} chat template for {architecture}: {error}"
                ))
            });
        }
    }

    match model.chat_template(None) {
        Ok(template) => Ok(template),
        Err(_) => LlamaChatTemplate::new("chatml").map_err(|error| {
            AiError::ContextError(format!("failed to create chat template: {error}"))
        }),
    }
}

fn run_decode_loop(
    model: &LlamaModel,
    engine_params: LlamaEngineParams,
    messages: Vec<ChatMessage>,
    sampling: SamplingParams,
    tx: &mpsc::UnboundedSender<Result<ChatChunk, AiError>>,
) -> Result<(), AiError> {
    let template = resolve_chat_template(model)?;

    let chat_messages = messages
        .into_iter()
        .map(|message| {
            LlamaChatMessage::new(message.role, message.content)
                .map_err(|error| AiError::ContextError(format!("invalid chat message: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let prompt = model
        .apply_chat_template(&template, &chat_messages, true)
        .map_err(|error| {
            AiError::ContextError(format!("failed to apply chat template: {error}"))
        })?;

    let prompt_tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|error| AiError::ContextError(format!("failed to tokenize prompt: {error}")))?;

    if prompt_tokens.is_empty() {
        return Err(AiError::ContextError(
            "prompt produced no tokens".to_string(),
        ));
    }

    let backend = shared_backend()?;
    let n_ctx = NonZeroU32::new(engine_params.n_ctx.max(1))
        .ok_or_else(|| AiError::ContextError("invalid n_ctx (must be > 0)".to_string()))?;
    let threads = i32::try_from(engine_params.n_threads)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(8);

    let mut ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    ctx_params = ctx_params.with_n_threads(threads);
    ctx_params = ctx_params.with_n_threads_batch(threads);

    let mut ctx = model.new_context(backend, ctx_params).map_err(|error| {
        AiError::ContextError(format!("failed to create llama context: {error}"))
    })?;

    if prompt_tokens.len() as u32 >= ctx.n_ctx() {
        return Err(AiError::ContextError(format!(
            "prompt token count ({}) exceeds context ({})",
            prompt_tokens.len(),
            ctx.n_ctx()
        )));
    }

    let mut prompt_batch = LlamaBatch::new(prompt_tokens.len().max(1), 1);
    for (index, token) in prompt_tokens.iter().enumerate() {
        let position = i32::try_from(index).map_err(|_| {
            AiError::ContextError("prompt too large to fit decode positions".to_string())
        })?;
        let is_last = index + 1 == prompt_tokens.len();
        prompt_batch
            .add(*token, position, &[0], is_last)
            .map_err(|error| {
                AiError::ContextError(format!("failed to build prompt batch: {error}"))
            })?;
    }

    let prefill_started = Instant::now();
    ctx.decode(&mut prompt_batch).map_err(|error| {
        AiError::InferenceError(format!("llama decode failed on prompt: {error}"))
    })?;
    let prefill_duration_ms = prefill_started.elapsed().as_millis() as u64;

    let temperature = if sampling.temperature.is_finite() {
        sampling.temperature.max(0.0)
    } else {
        0.8
    };
    let top_p = if sampling.top_p.is_finite() {
        sampling.top_p.clamp(0.0, 1.0)
    } else {
        0.95
    };
    let top_k = sampling.top_k.max(1);
    let repeat_penalty = if sampling.repeat_penalty.is_finite() {
        sampling.repeat_penalty.max(0.0)
    } else {
        1.1
    };

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::penalties(-1, repeat_penalty, 0.0, 0.0),
        LlamaSampler::top_k(top_k),
        LlamaSampler::top_p(top_p, 1),
        LlamaSampler::temp(temperature),
        LlamaSampler::dist(42),
    ]);
    sampler.accept_many(prompt_tokens.iter());

    let started = Instant::now();
    let prompt_token_count = prompt_tokens.len() as u64;
    let mut completion_tokens = 0_u64;
    let mut last_batch_tokens = prompt_batch.n_tokens();
    let mut next_position = i32::try_from(prompt_tokens.len()).unwrap_or(i32::MAX);
    let mut token_decoder = encoding_rs::UTF_8.new_decoder();
    let mut decode_batch = LlamaBatch::new(1, 1);

    for _ in 0..sampling.max_tokens {
        let token = sampler.sample(&ctx, last_batch_tokens - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        let piece = model
            .token_to_piece(token, &mut token_decoder, true, None)
            .map_err(|error| {
                AiError::InferenceError(format!("failed to decode token piece: {error}"))
            })?;
        if !piece.is_empty() {
            let _ = tx.send(Ok(ChatChunk::Token(piece)));
        }
        completion_tokens = completion_tokens.saturating_add(1);

        decode_batch.clear();
        decode_batch
            .add(token, next_position, &[0], true)
            .map_err(|error| {
                AiError::InferenceError(format!("failed to add generation token to batch: {error}"))
            })?;
        next_position = next_position.saturating_add(1);

        ctx.decode(&mut decode_batch).map_err(|error| {
            AiError::InferenceError(format!("llama decode failed during generation: {error}"))
        })?;
        last_batch_tokens = decode_batch.n_tokens();
    }

    let total_duration_ms = started.elapsed().as_millis() as u64;
    let seconds = total_duration_ms as f64 / 1000.0;
    let tokens_per_second = if completion_tokens > 0 && seconds > 0.0 {
        completion_tokens as f64 / seconds
    } else {
        0.0
    };

    let _ = tx.send(Ok(ChatChunk::Stats {
        prompt_tokens: prompt_token_count,
        completion_tokens,
        prefill_duration_ms,
        total_duration_ms,
        tokens_per_second,
    }));
    let _ = tx.send(Ok(ChatChunk::Done));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        LlamaEngineParams, LlamaGpuSplitMode, builtin_chat_template_name_for_architecture,
        normalize_device_indices, should_apply_split_mode_override,
    };

    #[test]
    fn normalize_device_indices_keeps_all_unique_devices_for_split_modes() {
        assert_eq!(
            normalize_device_indices(vec![2, 0, 2, 1], LlamaGpuSplitMode::Layer, None),
            vec![0, 1, 2]
        );
        assert_eq!(
            normalize_device_indices(vec![1, 0, 1], LlamaGpuSplitMode::Row, None),
            vec![0, 1]
        );
    }

    #[test]
    fn normalize_device_indices_uses_single_device_when_split_disabled() {
        assert_eq!(
            normalize_device_indices(vec![3, 1, 2], LlamaGpuSplitMode::None, None),
            vec![3]
        );
        assert_eq!(
            normalize_device_indices(vec![3, 1, 2], LlamaGpuSplitMode::None, Some(2)),
            vec![2]
        );
    }

    #[test]
    fn normalize_device_indices_allows_explicit_main_gpu_even_without_visible_devices() {
        assert_eq!(
            normalize_device_indices(vec![], LlamaGpuSplitMode::None, Some(1)),
            vec![1]
        );
    }

    #[test]
    fn skip_split_mode_none_override_without_visible_gpu_state() {
        let mut params = LlamaEngineParams::default();
        params.split_mode = LlamaGpuSplitMode::None;
        params.main_gpu = None;

        assert!(!should_apply_split_mode_override(&params, &[]));
        assert!(should_apply_split_mode_override(&params, &[0]));
    }

    #[test]
    fn keep_split_mode_override_when_main_gpu_is_explicit() {
        let mut params = LlamaEngineParams::default();
        params.split_mode = LlamaGpuSplitMode::None;
        params.main_gpu = Some(0);

        assert!(should_apply_split_mode_override(&params, &[]));
    }

    #[test]
    fn prefer_builtin_gemma_template_for_gemma_architectures() {
        assert_eq!(
            builtin_chat_template_name_for_architecture("gemma"),
            Some("gemma")
        );
        assert_eq!(
            builtin_chat_template_name_for_architecture("gemma2"),
            Some("gemma")
        );
        assert_eq!(
            builtin_chat_template_name_for_architecture("gemma4"),
            Some("gemma")
        );
    }

    #[test]
    fn keep_embedded_template_path_for_non_gemma_architectures() {
        assert_eq!(builtin_chat_template_name_for_architecture("qwen2"), None);
        assert_eq!(builtin_chat_template_name_for_architecture("llama"), None);
    }
}
