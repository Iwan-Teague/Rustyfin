use futures::stream::BoxStream;

use crate::backend::{InferenceBackend, estimate_chat_tokens};
use crate::engine::{LlamaEngine, SamplingParams};
use crate::error::AiError;
use crate::roles::ModelRole;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

#[derive(Clone)]
pub struct LocalLlamaBackend {
    model_name: String,
    engine: LlamaEngine,
}

impl LocalLlamaBackend {
    pub fn new(model_name: impl Into<String>, engine: LlamaEngine) -> Self {
        Self {
            model_name: model_name.into(),
            engine,
        }
    }

    pub fn engine(&self) -> &LlamaEngine {
        &self.engine
    }
}

impl InferenceBackend for LocalLlamaBackend {
    fn backend_id(&self) -> &'static str {
        "local_llama"
    }

    fn backend_kind(&self) -> BackendKind {
        BackendKind::Local
    }

    fn model_name(&self) -> Option<&str> {
        Some(self.model_name.as_str())
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.engine.capabilities()
    }

    fn local_engine(&self) -> Option<&LlamaEngine> {
        Some(&self.engine)
    }

    fn count_chat_tokens(
        &self,
        _role: ModelRole,
        messages: &[ChatMessage],
    ) -> Result<u32, AiError> {
        Ok(estimate_chat_tokens(messages))
    }

    fn chat_stream_boxed(
        &self,
        _role: ModelRole,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        _prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, AiError>> {
        Box::pin(self.engine.chat_stream(messages, sampling))
    }
}
