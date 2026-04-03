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
        prefill_duration_ms: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Local,
    Remote,
}

impl BackendKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub kind: BackendKind,
    pub supports_streaming: bool,
    pub supports_prompt_cache: bool,
    pub supports_structured_output: bool,
    pub can_degrade: bool,
    pub max_parallel_requests: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCacheHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_scope: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmupCostClass {
    Low,
    Medium,
    High,
    Extreme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfileRecommendation {
    pub model_name: String,
    pub model_checksum: String,
    pub host_fingerprint: String,
    pub context_window: u32,
    pub preferred_completion_tokens: u32,
    pub planner_max_output: u32,
    pub summary_max_output: u32,
    pub safety_headroom: u32,
    pub warmup_cost_class: WarmupCostClass,
    pub supports_structured_output: bool,
    pub supports_prompt_cache: bool,
    pub recommended_n_threads: u32,
    pub recommended_n_gpu_layers: i32,
    pub recommended_split_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_main_gpu: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommended_device_indices: Vec<usize>,
    pub estimated_model_bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBenchmarkResult {
    pub benchmark_label: String,
    pub model_name: String,
    pub model_checksum: String,
    pub host_fingerprint: String,
    pub load_duration_ms: u64,
    pub prefill_tokens: u64,
    pub prefill_duration_ms: u64,
    pub decode_tokens: u64,
    pub decode_duration_ms: u64,
    pub first_token_ms: u64,
    pub total_duration_ms: u64,
    pub tokens_per_second: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_before_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_after_load_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rss_peak_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
    pub recommendation: ModelProfileRecommendation,
}
