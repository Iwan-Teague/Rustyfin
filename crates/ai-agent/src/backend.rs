use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::engine::{LlamaEngine, SamplingParams};
use crate::error::AiError;
use crate::roles::ModelRole;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

pub mod local_llama;
pub mod role_router;

pub use local_llama::LocalLlamaBackend;
pub use role_router::{ModelSelectionSource, RoleBoundPromptBackend, RoleModelSelection};

static LLAMA_BACKEND: OnceLock<llama_cpp_2::llama_backend::LlamaBackend> = OnceLock::new();
static LLAMA_BACKEND_INIT: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteBackendConfig {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default)]
    pub supports_prompt_cache: bool,
    #[serde(default)]
    pub supports_structured_output: bool,
    #[serde(default)]
    pub max_parallel_requests: u32,
}

impl RemoteBackendConfig {
    pub fn validate(&self) -> Result<(), AiError> {
        let base_url = self.base_url.trim();
        if base_url.is_empty() {
            return Err(AiError::ContextError(
                "remote backend base URL is required".to_string(),
            ));
        }
        reqwest::Url::parse(base_url).map_err(|error| {
            AiError::ContextError(format!("invalid remote backend base URL: {error}"))
        })?;
        if self.model.trim().is_empty() {
            return Err(AiError::ContextError(
                "remote backend model is required".to_string(),
            ));
        }
        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.max(1))
    }

    pub fn backend_kind(&self) -> crate::types::BackendKind {
        crate::types::BackendKind::Remote
    }

    pub fn capabilities(&self) -> crate::types::BackendCapabilities {
        crate::types::BackendCapabilities {
            kind: crate::types::BackendKind::Remote,
            supports_streaming: true,
            supports_prompt_cache: self.supports_prompt_cache,
            supports_structured_output: self.supports_structured_output,
            can_degrade: true,
            max_parallel_requests: self.max_parallel_requests.max(1),
        }
    }

    pub fn api_key(&self) -> Option<String> {
        let env_name = self.api_key_env.as_ref()?.trim();
        if env_name.is_empty() {
            return None;
        }
        std::env::var(env_name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
}

pub trait InferenceBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn backend_kind(&self) -> BackendKind;
    fn model_name(&self) -> Option<&str>;
    fn capabilities(&self) -> BackendCapabilities;
    fn local_engine(&self) -> Option<&LlamaEngine> {
        None
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
        role: ModelRole,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, AiError>>;
}

pub(crate) fn shared_backend() -> Result<&'static llama_cpp_2::llama_backend::LlamaBackend, AiError>
{
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend);
    }

    let _guard = LLAMA_BACKEND_INIT
        .lock()
        .map_err(|_| AiError::ContextError("failed to acquire backend init lock".to_string()))?;

    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend);
    }

    let backend = llama_cpp_2::llama_backend::LlamaBackend::init().map_err(|error| {
        AiError::ContextError(format!("failed to initialize llama backend: {error}"))
    })?;
    let _ = LLAMA_BACKEND.set(backend);

    LLAMA_BACKEND
        .get()
        .ok_or_else(|| AiError::ContextError("llama backend initialization failed".to_string()))
}

pub fn estimate_chat_tokens(messages: &[ChatMessage]) -> u32 {
    let mut total: usize = 0;
    for message in messages {
        let role_cost = message.role.len().saturating_div(4).saturating_add(4);
        let content_cost = message.content.len().saturating_div(4).saturating_add(1);
        total = total.saturating_add(role_cost).saturating_add(content_cost);
    }

    u32::try_from(total).unwrap_or(u32::MAX).max(1)
}
