# Agent Prompt: Rustyfin AI Page — Phase 1 Native Inference Implementation

You are implementing Phase 1 of the Rustyfin AI assistant: replacing the temporary OllamaClient HTTP shim with a native llama-cpp-2 in-process inference engine. Read the full design spec first, then implement everything described below.

---

## Step 0: Read Context

Read these files before writing any code:
- /home/server/docker/Rustyfin/docs/plans/2026-03-14-ai-assistant-design.md  (full design spec)
- /home/server/docker/Rustyfin/crates/ai-agent/src/client.rs                 (the OllamaClient shim — to be replaced)
- /home/server/docker/Rustyfin/crates/ai-agent/src/types.rs                  (existing types)
- /home/server/docker/Rustyfin/crates/ai-agent/src/lib.rs                    (crate entry)
- /home/server/docker/Rustyfin/crates/ai-agent/Cargo.toml                    (current deps)
- /home/server/docker/Rustyfin/crates/server/src/ai.rs                       (Axum route handlers)
- /home/server/docker/Rustyfin/crates/server/src/state.rs                    (AppState)
- /home/server/docker/Rustyfin/crates/server/src/main.rs                     (server startup)
- /home/server/docker/Rustyfin/ui/src/app/ai/page.tsx                        (frontend AI page)
- /home/server/docker/Rustyfin/ui/src/lib/aiApi.ts                           (frontend API client)

---

## Step 1: Update crates/ai-agent/Cargo.toml

Replace the entire Cargo.toml for the ai-agent crate. The crate now depends on llama-cpp-2 for in-process inference, reqwest for GGUF downloads, and tokio-util for streaming download progress. Remove the reqwest dependency that was used for Ollama HTTP calls — it is still needed for GGUF downloads from HuggingFace.

```toml
[package]
name = "rustfin-ai-agent"
version.workspace = true
edition.workspace = true

[features]
default = ["cuda"]
cuda    = ["llama-cpp-2/cuda"]
rocm    = ["llama-cpp-2/hipblas"]
vulkan  = ["llama-cpp-2/vulkan"]
cpu     = []

[dependencies]
llama-cpp-2 = { version = "0.1" }
tokio       = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
reqwest     = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
futures     = "0.3"
tokio-util  = { version = "0.7", features = ["io"] }
tracing     = { workspace = true }
thiserror   = { workspace = true }
async-stream = "0.3"
```

---

## Step 2: Write crates/ai-agent/build.rs

This build script propagates Cargo feature flags through to the llama.cpp cmake configuration. The llama-cpp-2 crate reads environment variables set by build.rs to determine which GPU backend to compile:

```rust
fn main() {
    // llama-cpp-2 reads these env vars during its build.rs to select backends
    if cfg!(feature = "cuda") {
        println!("cargo:rustc-env=LLAMA_CUDA=1");
    }
    if cfg!(feature = "rocm") {
        println!("cargo:rustc-env=LLAMA_HIPBLAS=1");
    }
    if cfg!(feature = "vulkan") {
        println!("cargo:rustc-env=LLAMA_VULKAN=1");
    }
}
```

---

## Step 3: Write crates/ai-agent/src/error.rs

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("model directory error: {0}")]
    ModelDirError(String),
    #[error("inference error: {0}")]
    InferenceError(String),
    #[error("download error: {0}")]
    DownloadError(String),
    #[error("context build error: {0}")]
    ContextError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## Step 4: Write crates/ai-agent/src/types.rs

Keep the existing ChatMessage, ChatChunk, PullChunk types. Remove all OllamaXxx wire types (OllamaTagsResponse, OllamaTagModel, OllamaChatChunk, OllamaPsResponse, etc.) — they are no longer needed once OllamaClient is removed. Add a new ModelInfo type for GGUF discovery results:

```rust
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
```

---

## Step 5: Write crates/ai-agent/src/model_store.rs

The ModelStore scans a directory for `*.gguf` files, extracts metadata using llama-cpp-2's model metadata API, and handles GGUF downloads from HuggingFace.

Key responsibilities:
1. `discover(model_dir)` — walk the model directory, call `LlamaModel::load_from_file()` briefly just to read metadata headers, return `Vec<ModelInfo>`
2. `download(model_name, model_dir, http_client, on_chunk)` — fetch a GGUF URL via reqwest with streaming byte progress, write to a `.part` file, atomically rename on completion
3. `delete(name, model_dir)` — remove a .gguf file by stem name

For the curated model catalog, include these HuggingFace direct GGUF URLs:

| name | HuggingFace URL |
|---|---|
| llama3.2:3b | https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf |
| llama3.1:8b | https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF/resolve/main/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf |
| qwen2.5:7b | https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf |
| deepseek-r1:8b | https://huggingface.co/bartowski/DeepSeek-R1-Distill-Llama-8B-GGUF/resolve/main/DeepSeek-R1-Distill-Llama-8B-Q4_K_M.gguf |
| mistral:7b | https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf |

The `download(model_name, http_client)` function should:
- Look up the URL from the curated catalog if the name matches (e.g. "llama3.2:3b")
- Accept a raw HTTPS URL as fallback if no catalog match
- Stream download using `reqwest` with `bytes_stream()`, yield `PullChunk::Progress` per chunk
- Write to `{model_dir}/{stem}.gguf.part`, rename to `{model_dir}/{stem}.gguf` on success

---

## Step 6: Write crates/ai-agent/src/engine.rs

The LlamaEngine wraps a loaded llama-cpp-2 model and provides a streaming `chat_stream()` method.

```rust
use std::path::Path;
use std::sync::Arc;
use futures::Stream;
use crate::types::{ChatMessage, ChatChunk};
use crate::error::AiError;

pub struct LlamaEngineParams {
    pub n_gpu_layers: i32,       // -1 = all layers on GPU
    pub tensor_split: Vec<f32>,  // empty = automatic multi-GPU split
    pub n_ctx: u32,              // context size in tokens
    pub n_threads: u32,          // inference threads
}

impl Default for LlamaEngineParams {
    fn default() -> Self {
        Self { n_gpu_layers: -1, tensor_split: vec![], n_ctx: 4096, n_threads: 8 }
    }
}

pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repeat_penalty: f32,
    pub max_tokens: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self { temperature: 0.8, top_p: 0.95, top_k: 40, repeat_penalty: 1.1, max_tokens: 2048 }
    }
}

pub struct LlamaEngine { ... }

impl LlamaEngine {
    pub fn load(gguf_path: &Path, params: LlamaEngineParams) -> Result<Self, AiError>;
    pub fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
    ) -> impl Stream<Item = Result<ChatChunk, AiError>> + Send + 'static + use<>;
}
```

The `chat_stream` implementation should:
1. Apply the model's chat template to the messages Vec using llama-cpp-2's `apply_chat_template`
2. Tokenize the result
3. Run the decode loop: create a `LlamaContext` with `n_ctx`, call `llama_decode()` with the token batch
4. Sample one token per step using the `LlamaSampler` with the provided SamplingParams
5. Yield `ChatChunk::Token(text)` for each decoded piece (using the tokenizer to decode each token piece)
6. On EOS or max_tokens reached: yield `ChatChunk::Stats { ... }` then `ChatChunk::Done`
7. Use `async_stream::stream!` to bridge the sync llama.cpp calls into an async Stream

Calculate stats:
- `prompt_tokens` = input token count
- `completion_tokens` = generated token count
- `total_duration_ms` = wall clock via `std::time::Instant`
- `tokens_per_second` = `completion_tokens` / (`total_duration_ms` / 1000.0)

Use `tokio::task::spawn_blocking` around the decode loop since llama.cpp is blocking/synchronous.

---

## Step 7: Write crates/ai-agent/src/lib.rs

```rust
pub mod engine;
pub mod error;
pub mod model_store;
pub mod types;

pub use engine::{LlamaEngine, LlamaEngineParams, SamplingParams};
pub use error::AiError;
pub use model_store::ModelStore;
pub use types::*;
```

Delete the old `src/client.rs` file entirely.

---

## Step 8: Rewrite crates/server/src/ai.rs

Replace all OllamaClient usage with LlamaEngine + ModelStore. The AppState will now hold an `Arc<Mutex<EngineState>>` where:

```rust
pub struct EngineState {
    pub loaded_model: Option<String>,
    pub engine: Option<rustfin_ai_agent::LlamaEngine>,
}
```

**GET /api/v1/ai/models** — call `ModelStore::discover(config.model_dir)` and return results. Replace `ollama_available` field with `inference_available: true` (llama-cpp-2 is always compiled in).

**POST /api/v1/ai/models/pull** (SSE) — call `ModelStore::download()`, stream PullChunk events. The request body: `{"model": "llama3.2:3b"}` or `{"url": "https://..."}`. Map `PullChunk::Progress` to SSE progress events.

**DELETE /api/v1/ai/models/:name** — call `ModelStore::delete(name, model_dir)`.

**GET /api/v1/ai/running** — return name + VRAM estimate of the currently loaded LlamaEngine model (if any). VRAM estimate: read from nvidia-smi, or return 0.0 if unavailable.

**GET /api/v1/ai/gpus** — unchanged, still uses nvidia-smi.

**POST /api/v1/ai/chat** — acquire the engine mutex; if `engine` is None or `loaded_model` != requested model, load the new model from `{model_dir}/{name}.gguf`, replace the engine. Then call `engine.chat_stream(messages, SamplingParams::default())`.

---

## Step 9: Update crates/server/src/state.rs

Replace `pub ollama_url: String` with:
```rust
pub model_dir: std::path::PathBuf,
pub engine: std::sync::Arc<tokio::sync::Mutex<crate::ai::EngineState>>,
```

---

## Step 10: Update crates/server/src/main.rs

Replace `RUSTFIN_OLLAMA_URL` env var reading with:
```rust
let model_dir = std::env::var("RUSTFIN_AI_MODEL_DIR")
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|_| std::path::PathBuf::from("/var/lib/rustyfin/models"));
let _ = std::fs::create_dir_all(&model_dir);
```

---

## Step 11: Update ui/src/lib/aiApi.ts

In the `ModelsResponse` interface, rename `ollama_available` to `inference_available`. Update `fetchModels()` to read `res.inference_available`.

---

## Step 12: Update ui/src/app/ai/page.tsx

1. Rename all references to `ollamaAvailable` state variable → `inferenceAvailable`
2. Replace the `OllamaOffline` component with an `InferenceUnavailable` component:
   - Title: "No model loaded"
   - Body: "Open the Models panel and pull a model to get started."
   - Recommended pill: "Recommended: llama3.2:3b (~2 GB)"
   - No ollama install command block
3. Replace "Connecting to Ollama…" loading text with "Loading…"
4. In `PullSection`, replace the link to `ollama.com/library` with a link to `huggingface.co/models?library=gguf` with text "Browse GGUF models on Hugging Face"
5. In `GpuSection`, replace "Ollama" references with "llama.cpp" / "rustfin-server":
   - CUDA_VISIBLE_DEVICES note: "Set this env var before starting `rustfin-server` to restrict GPU access."
   - AMD note: "AMD ROCm GPUs are managed automatically by llama.cpp when present."
6. The `online` status chip in the header should say "ready" when `inferenceAvailable && selectedModel` or "no model" when `inferenceAvailable && !selectedModel`

---

## Step 13: Run the immaculate skill

After all code changes above are complete, invoke the `/immaculate` skill to perform a final frontend quality pass on `ui/src/app/ai/page.tsx`. This will review and polish the page for visual hierarchy, consistency with Rustyfin's cinematic dark aesthetic (orange→pink→purple accent gradient), spacing, accessibility, and component detail quality.

---

## Constraints and Notes

- **Rust edition is 2024.** Any `impl Trait` return types that capture state need `+ use<>` to satisfy the edition's capture rules.
- **AppState is Clone** (Arc-based). The new `Arc<Mutex<EngineState>>` fits this — wrap in Arc.
- **Compilation target**: Debian, NVIDIA CUDA present. Default feature = `"cuda"`.
- **llama-cpp-2 crate version**: check crates.io for the latest 0.1.x version before writing Cargo.toml.
- **GGUF metadata API**: llama-cpp-2 exposes `model.metadata()` returning a `HashMap<String, MetadataValue>`. Key names follow GGUF spec: `general.architecture`, `general.parameter_count`, `llama.context_length`. Parse these defensively.
- **Do not** implement conversation history persistence (Phase 4). In-memory message history passed in the request body is sufficient for now.
- **Do not** implement tool calling (Phase 2). Basic streaming chat only.
- The system prompt in `crates/server/src/ai.rs` already says "Rustyfin assistant" and does not mention Ollama — keep it as-is.
- After completing all steps, confirm what compiled successfully and note that deployment requires: `sudo systemctl stop rustyfin-native.service && sudo ./scripts/deploy-native.sh --skip-git-pull`
