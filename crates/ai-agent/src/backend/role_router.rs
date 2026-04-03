use std::sync::Arc;

use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::SamplingParams;
use crate::backend::InferenceBackend;
use crate::backends::PromptBackend;
use crate::roles::ModelRole;
use crate::types::{BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptCacheHint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelectionSource {
    ExplicitRequest,
    StoredRecommendation,
    EnvDefault,
    Fallback,
}

impl ModelSelectionSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitRequest => "explicit_request",
            Self::StoredRecommendation => "stored_recommendation",
            Self::EnvDefault => "env_default",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleModelSelection {
    pub model_name: String,
    pub source: ModelSelectionSource,
}

#[derive(Clone)]
pub struct RoleBoundPromptBackend {
    backend: Arc<dyn InferenceBackend>,
    role: ModelRole,
}

impl RoleBoundPromptBackend {
    pub fn new(backend: Arc<dyn InferenceBackend>, role: ModelRole) -> Self {
        Self { backend, role }
    }
}

impl PromptBackend for RoleBoundPromptBackend {
    fn backend_kind(&self) -> BackendKind {
        self.backend.backend_kind()
    }

    fn capabilities(&self) -> BackendCapabilities {
        self.backend.capabilities()
    }

    fn chat_stream_boxed(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
        prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, crate::AiError>> {
        self.backend
            .chat_stream_boxed(self.role, messages, sampling, prompt_cache)
    }
}
