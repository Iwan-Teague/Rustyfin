use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use async_stream::stream;
use futures::Stream;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use tokio::sync::mpsc;
use tracing::warn;

use crate::backend::shared_backend;
use crate::error::AiError;
use crate::types::{ChatChunk, ChatMessage};

#[derive(Debug, Clone)]
pub struct LlamaEngineParams {
    pub n_gpu_layers: i32,
    pub tensor_split: Vec<f32>,
    pub n_ctx: u32,
    pub n_threads: u32,
}

impl Default for LlamaEngineParams {
    fn default() -> Self {
        Self {
            n_gpu_layers: -1,
            tensor_split: vec![],
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
        if params.n_gpu_layers >= 0 {
            model_params = model_params.with_n_gpu_layers(params.n_gpu_layers as u32);
        }

        let model = LlamaModel::load_from_file(backend, gguf_path, &model_params)
            .map_err(|error| AiError::InferenceError(format!("failed to load model: {error}")))?;

        Ok(Self {
            model: Arc::new(model),
            params,
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
}

fn run_decode_loop(
    model: &LlamaModel,
    engine_params: LlamaEngineParams,
    messages: Vec<ChatMessage>,
    sampling: SamplingParams,
    tx: &mpsc::UnboundedSender<Result<ChatChunk, AiError>>,
) -> Result<(), AiError> {
    let template = match model.chat_template(None) {
        Ok(template) => template,
        Err(_) => LlamaChatTemplate::new("chatml").map_err(|error| {
            AiError::ContextError(format!("failed to create chat template: {error}"))
        })?,
    };

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

    ctx.decode(&mut prompt_batch).map_err(|error| {
        AiError::InferenceError(format!("llama decode failed on prompt: {error}"))
    })?;

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
        total_duration_ms,
        tokens_per_second,
    }));
    let _ = tx.send(Ok(ChatChunk::Done));

    Ok(())
}
