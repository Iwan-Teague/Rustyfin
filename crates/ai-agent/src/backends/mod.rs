mod remote;

use futures::stream::BoxStream;

use crate::SamplingParams;
use crate::backend::RemoteBackendConfig;
use crate::error::AiError;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

pub use remote::RemotePromptBackend;
pub type RemotePromptBackendConfig = RemoteBackendConfig;

pub trait PromptBackend: Send + Sync {
    fn backend_kind(&self) -> BackendKind;

    fn capabilities(&self) -> BackendCapabilities;

    fn chat_stream_boxed(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, AiError>>;
}
