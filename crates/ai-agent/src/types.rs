use serde::{Deserialize, Serialize};

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Discovered GGUF model metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Filename stem used as model identifier e.g. "llama3.2-3b-q4"
    pub name: String,
    /// Original .gguf filename
    pub file: String,
    /// File size in GB
    pub size_gb: f64,
    /// Parameter count string from GGUF metadata e.g. "3B"
    pub parameter_size: Option<String>,
    /// Quantization type e.g. "Q4_K_M"
    pub quantization: Option<String>,
    /// Architecture from GGUF metadata e.g. "llama"
    pub architecture: Option<String>,
    /// Max context length from GGUF metadata
    pub context_length: Option<u32>,
}

/// An item yielded from the streaming chat completion.
#[derive(Debug, Clone)]
pub enum ChatChunk {
    Token(String),
    Stats {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_duration_ms: u64,
        tokens_per_second: f64,
    },
    Done,
}

/// An item yielded from a streaming model download.
#[derive(Debug, Clone)]
pub enum PullChunk {
    Progress {
        status: String,
        bytes_done: u64,
        bytes_total: Option<u64>,
        percent: u8,
    },
    Done,
    Error(String),
}
