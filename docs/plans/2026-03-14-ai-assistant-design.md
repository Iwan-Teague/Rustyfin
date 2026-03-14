# Rustyfin AI Assistant — Design Specification
Date: 2026-03-14
Last revised: 2026-03-14
Status: **Approved for implementation**
Author: Design session with project lead

---

## 0. Decision Log

| Date | Decision | Rationale |
|---|---|---|
| 2026-03-14 | Initial design drafted using Ollama as inference backend | Fastest path to a working UI |
| 2026-03-14 | **Replaced Ollama requirement with `llama-cpp-2` (direct llama.cpp Rust bindings)** | Self-contained binary, no external daemon dependency, fully in-process, same inference quality, aligns with Rust-first constraint |

> **The current `OllamaClient` in `crates/ai-agent/src/client.rs` is a temporary bridge implementation only.** It exists to allow UI development to proceed before the native inference engine is complete. It must be replaced with a `LlamaEngine` backed by `llama-cpp-2` before the AI feature is considered production-ready.

---

## 1. Overview

### 1.1 Purpose
The AI assistant embedded in Rustyfin is a **local-first, tool-augmented chat agent** that knows about your server. Its value is not general knowledge — it is **situational awareness of the Rustyfin instance**: who is online, what events are coming up, what rooms exist, what media is in the library, and how to act on your behalf (create events, spin up rooms, start Minecraft servers).

It is not a replacement for interfaces. It is a low-friction shortcut: _"Hey, when is Jake's birthday?"_ or _"Create a movie night room and invite everyone"_ should be answerable without navigating three pages.

### 1.2 Hard Requirements

- **Native Rust inference via `llama-cpp-2`**: inference runs in-process inside `rustfin-server` via Rust bindings to llama.cpp. No external daemon. No HTTP roundtrip to a sidecar process.
- **No Ollama dependency in production**: Ollama must not be a runtime requirement. The `OllamaClient` is a temporary shim and must be removed before the feature ships.
- **Rust-heavy end to end**: all inference, tool execution, context assembly, model management, and GPU configuration is Rust. The frontend is purely a rendering surface.
- **Local models only**: no cloud API calls. All inference runs on the host machine.
- **No external data leakage**: the system prompt and all tool data stay within the LAN.
- **Tool-calling, not RAG (v1)**: structured function calls against live Rustyfin data, not embeddings. Keeps latency low and results fresh.
- **Streaming responses**: the UI renders tokens as they arrive via SSE.
- **GGUF model format**: all models are loaded from GGUF files stored in a configurable local directory managed by Rustyfin.

---

## 2. Why llama-cpp-2 (not Ollama)

### 2.1 What Ollama actually is
Ollama is a Go HTTP server that wraps **llama.cpp** — a highly optimised C/C++ inference library with hand-tuned CUDA/ROCm/Vulkan kernels, GGUF model parsing, KV-cache management, batching, and quantisation dequantisation. Ollama adds model management, an OpenAI-compatible REST API, and multi-GPU layer distribution on top of llama.cpp.

Inference quality is identical whether you use Ollama or call llama.cpp directly — it is the same engine.

### 2.2 Why direct bindings are better for Rustyfin

| Concern | Ollama | llama-cpp-2 (direct) |
|---|---|---|
| External daemon required | Yes — `ollama serve` must be running | No — inference runs inside `rustfin-server` |
| Binary self-containment | No | Yes |
| HTTP overhead per token | Yes (~1ms/chunk roundtrip) | None — direct memory call |
| Dependency count | Ollama binary + systemd service | One Rust crate + llama.cpp compiled in |
| GPU support | CUDA, ROCm, Metal via llama.cpp | CUDA, ROCm, Metal, Vulkan — identical (same llama.cpp) |
| Multi-GPU | Automatic layer distribution | Configurable via `n_gpu_layers` + `tensor_split` |
| Model format | GGUF (llama.cpp's format) | GGUF — identical |
| Alignment with project constraints | Violates Rust-first, introduces Go daemon | Fully Rust, in-process, no new processes |

### 2.3 What is NOT feasible
Writing an inference engine from scratch in Rust is not feasible. LLM inference requires hand-tuned GEMM kernels, CUDA/ROCm device code, attention mechanisms, KV-cache management, quantisation routines, and tokeniser implementations (SentencePiece, BPE, tiktoken). This is years of specialist engineering. `llama-cpp-2` gives us all of that for free while keeping the calling code pure Rust.

---

## 3. System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  Browser (Next.js)                                                   │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  /ai page                                                     │   │
│  │  ModelSelector ──→ ChatThread ──→ ToolCallBadge              │   │
│  │  MessageInput (textarea + send)  StatsBar (tokens, t/s)      │   │
│  │  ManagementPanel (models, GPU info, download progress)        │   │
│  └──────────────────────────────────────────────────────────────┘   │
│              │  POST /api/v1/ai/chat   (SSE stream)                  │
│              │  GET  /api/v1/ai/models                               │
│              │  POST /api/v1/ai/models/pull                          │
│              │  DELETE /api/v1/ai/models/:name                       │
│              │  GET  /api/v1/ai/gpus                                 │
│              │  GET  /api/v1/ai/running                              │
└──────────────┼──────────────────────────────────────────────────────┘
               │
┌──────────────▼──────────────────────────────────────────────────────┐
│  rustfin-server  (Axum, port 8096)                                  │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  AI Router  (src/ai/)                                          │ │
│  │  ├── POST /chat     → streams tokens + tool-call events       │ │
│  │  ├── GET  /models   → lists installed GGUF models             │ │
│  │  ├── POST /models/pull → download GGUF (SSE progress)        │ │
│  │  ├── DELETE /models/:name → delete model file                │ │
│  │  ├── GET  /gpus     → nvidia-smi GPU info                    │ │
│  │  └── GET  /running  → currently loaded model + VRAM usage    │ │
│  └────────────────────────────────────────────────────────────────┘ │
│              │  calls                                                │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  crates/ai-agent/  (library crate, linked into server)       │   │
│  │  ├── LlamaEngine        — in-process llama-cpp-2 inference   │   │
│  │  │   ├── ModelStore     — GGUF file discovery + metadata     │   │
│  │  │   ├── SessionPool    — concurrent context management      │   │
│  │  │   └── GpuConfig      — layer split, tensor_split params   │   │
│  │  ├── ToolRegistry       — registered tool fn pointers        │   │
│  │  ├── AgentLoop          — turn loop: infer → tools → resume  │   │
│  │  └── ContextBuilder     — system prompt + tool schema        │   │
│  └──────────────────────────────────────────────────────────────┘   │
│              │  calls                                                │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  Rustyfin internal services                                  │   │
│  │  ├── db (sqlx repo layer)                                    │   │
│  │  ├── calendar service (port 8099)                            │   │
│  │  └── servers-agent (port 8103 — Minecraft)                   │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
         ↑
         │  in-process FFI calls (no HTTP, no sockets)
         │
┌────────┴────────────────────────────────────────────────────────────┐
│  llama.cpp  (compiled into the binary via llama-cpp-2 crate)        │
│  CUDA / ROCm / Vulkan / CPU backends                                │
│  GGUF model files  →  /var/lib/rustyfin/models/*.gguf               │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.1 Crate Layout

```
crates/ai-agent/
  Cargo.toml          — depends on llama-cpp-2, async-stream, tokio, serde_json
  build.rs            — passes GPU feature flags to llama.cpp cmake
  src/
    lib.rs            — public API re-exports
    engine.rs         — LlamaEngine: model loading, session pool, streaming
    model_store.rs    — GGUF file discovery, metadata, download management
    gpu.rs            — GPU layer configuration (n_gpu_layers, tensor_split)
    agent.rs          — AgentLoop: orchestrates turn + tool execution
    context.rs        — ContextBuilder: system prompt, tool schema serialisation
    tools/
      mod.rs          — ToolRegistry, ToolDef, ToolResult types
      calendar.rs     — list_events, get_birthdays, create_event
      rooms.rs        — list_rooms, create_room, invite_user
      users.rs        — list_users, get_user
      servers.rs      — list_servers, start_server, stop_server
      media.rs        — search_media, get_continue_watching
    conv.rs           — ConversationStore (DB-backed history, optional)
    error.rs          — AiError type
```

> **Temporary file (to be removed):**
> `src/client.rs` — `OllamaClient` HTTP shim. Exists only for current UI development.
> Remove when `LlamaEngine` is implemented.

---

## 4. Inference Engine (`LlamaEngine`)

### 4.1 Cargo Dependency

```toml
# crates/ai-agent/Cargo.toml
[dependencies]
llama-cpp-2 = { version = "0.1", features = [] }   # GPU features via build.rs

[features]
default = ["cuda"]
cuda    = ["llama-cpp-2/cuda"]
rocm    = ["llama-cpp-2/hipblas"]
vulkan  = ["llama-cpp-2/vulkan"]
cpu     = []   # CPU-only fallback, no GPU features
```

GPU backend is selected at compile time via Cargo features. The default for the Debian host is `cuda`. The `build.rs` script reads `RUSTFIN_AI_GPU_BACKEND` and sets the appropriate feature.

### 4.2 `LlamaEngine` API

```rust
// crates/ai-agent/src/engine.rs

pub struct LlamaEngine {
    model: Arc<LlamaModel>,       // llama-cpp-2::LlamaModel — loaded once at startup
    params: LlamaEngineParams,
}

pub struct LlamaEngineParams {
    /// Number of model layers to offload to GPU. -1 = all layers.
    pub n_gpu_layers: i32,
    /// For multi-GPU: fraction of layers per GPU. Empty = automatic.
    /// Example: [0.5, 0.5] splits evenly across 2 GPUs.
    pub tensor_split: Vec<f32>,
    /// Maximum context tokens.
    pub n_ctx: u32,
    /// Inference threads.
    pub n_threads: u32,
}

impl LlamaEngine {
    /// Load a GGUF model file from disk. Called once at server startup or model switch.
    pub fn load(gguf_path: &Path, params: LlamaEngineParams) -> Result<Self, AiError> { ... }

    /// Stream a chat completion token by token.
    /// Yields Token chunks, then a Stats chunk, then Done.
    pub fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        sampling: SamplingParams,
    ) -> impl Stream<Item = Result<ChatChunk, AiError>> + Send + 'static { ... }
}

pub struct SamplingParams {
    pub temperature: f32,    // default 0.8
    pub top_p: f32,          // default 0.95
    pub top_k: i32,          // default 40
    pub repeat_penalty: f32, // default 1.1
    pub max_tokens: u32,     // default 2048
}
```

### 4.3 Multi-GPU Configuration

llama.cpp distributes model layers across GPUs via two parameters:

- **`n_gpu_layers`**: total number of transformer layers to move to GPU. Set to `-1` (all layers) when VRAM is sufficient. Reduce to partially offload when VRAM is limited.
- **`tensor_split`**: a slice of floats (one per GPU) expressing the fraction of layers each GPU receives. `[0.5, 0.5]` splits evenly across two GPUs. Empty slice = automatic distribution based on VRAM size.

```rust
// Example: 2x GPU, automatic split
LlamaEngineParams {
    n_gpu_layers: -1,
    tensor_split: vec![],   // llama.cpp auto-balances by VRAM
    ..Default::default()
}

// Example: 2x GPU, manual 60/40 split
LlamaEngineParams {
    n_gpu_layers: -1,
    tensor_split: vec![0.6, 0.4],
    ..Default::default()
}
```

The GPU configuration is exposed in the AI settings panel and stored per-model in the DB settings table.

**There is no per-request GPU selection** — GPU assignment is fixed when the model is loaded. Changing GPU split requires unloading and reloading the model (a few seconds). This is a fundamental constraint of how llama.cpp works, not a Rustyfin limitation.

### 4.4 Tool Calling

llama.cpp supports two approaches to structured tool calling:

1. **Grammar-constrained generation (GBNF)**: force the model to output valid JSON matching a provided grammar. Reliable for any model. Used as fallback.
2. **Native tool call format**: for models trained with a tool-calling format (Llama 3.1, Qwen 2.5, Mistral v0.3+), parse the model's native `<tool_call>` or `[TOOL_CALLS]` block from generated text.

`AgentLoop` uses approach 2 for known models and falls back to approach 1 for unknown models.

---

## 5. Model Store

### 5.1 Storage Location
GGUF model files are stored at `$RUSTFIN_AI_MODEL_DIR` (default: `/var/lib/rustyfin/models/`). Rustyfin discovers all `*.gguf` files in this directory at startup and on refresh.

### 5.2 Model Metadata
When a GGUF file is loaded, llama.cpp exposes rich metadata embedded in the file header:

- Architecture (llama, mistral, qwen2, etc.)
- Parameter count, context length, quantisation type (Q4_K_M, Q8_0, F16, etc.)
- Embedding dimensions, attention head count
- Tokeniser type and vocabulary size

This metadata is read at discovery time and returned in `GET /api/v1/ai/models`.

### 5.3 Model Download

Models are downloaded as GGUF files from Hugging Face Hub. Rustyfin maintains a curated catalog of recommended model URLs. Users can also provide a direct GGUF URL.

```rust
// Curated catalog entry
pub struct CatalogEntry {
    pub name: &'static str,         // "llama3.2:3b"
    pub display_name: &'static str, // "Llama 3.2 3B"
    pub url: &'static str,          // HuggingFace direct GGUF URL
    pub size_gb: f32,
    pub min_vram_gb: f32,
    pub description: &'static str,
}
```

Downloads stream via `reqwest` with byte progress reported as SSE events on `POST /api/v1/ai/models/pull`. Files are written to a `.part` temp file and atomically renamed on completion.

### 5.4 Recommended Starter Models

| Name | Params | Quant | VRAM | Use case |
|---|---|---|---|---|
| `llama3.2:3b-q4` | 3B | Q4_K_M | ~2 GB | Fast general assistant, low VRAM |
| `llama3.1:8b-q4` | 8B | Q4_K_M | ~5 GB | Strong tool-calling, daily driver |
| `qwen2.5:7b-q4` | 7B | Q4_K_M | ~4.5 GB | Excellent reasoning + tool-calling |
| `deepseek-r1:8b-q4` | 8B | Q4_K_M | ~5 GB | Chain-of-thought, emits `<think>` blocks |
| `mistral:7b-q4` | 7B | Q4_K_M | ~4.5 GB | Fast, good instruction following |

---

## 6. Tool Definitions

Each tool is a Rust async function registered in `ToolRegistry`. The registry serialises `ToolDef` structs to JSON Schema for inclusion in the model's context.

### 6.1 Tool Definition Type

```rust
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,  // JSON Schema object
}

pub type ToolFn = Arc<
    dyn Fn(serde_json::Value, Arc<ToolContext>) -> BoxFuture<'static, Result<serde_json::Value, AiError>>
    + Send + Sync
>;

pub struct ToolContext {
    pub db: DbPool,
    pub caller_user_id: i64,
    pub http: reqwest::Client,
    pub config: AiConfig,
}
```

### 6.2 Calendar Tools

#### `calendar_list_events`
```
List upcoming calendar events. Optionally filter by date range.
Parameters: { from: date_string?, to: date_string?, limit: int? }
Returns: array of { id, title, type, date, recurrence, created_by }
```

#### `calendar_get_birthdays`
```
List all user birthdays, returning name and next occurrence date.
Parameters: {}
Returns: array of { user_id, display_name, birthday_date, next_occurrence, age_turning }
```

#### `calendar_create_event`
```
Create a new calendar event.
Parameters: { title: string, date: ISO8601, type: "event"|"birthday",
              recurrence: "none"|"yearly", scope: "global"|"personal", notes: string? }
Returns: { id, title, date }
```

#### `calendar_delete_event`
```
Delete a calendar event by ID. Caller must be creator or admin.
Parameters: { event_id: int }
Returns: { deleted: bool }
```

### 6.3 Room Tools

#### `rooms_list_active`
```
List currently active watch-party rooms.
Parameters: {}
Returns: array of { id, name, mode, member_count, host_name, has_media }
```

#### `rooms_create`
```
Create a new watch-party room.
Parameters: { name: string, mode: "video"|"audio"|"youtube",
              password: string?, invite_user_ids: int[]? }
Returns: { room_id, invite_link }
```

#### `rooms_invite_user`
```
Invite a specific user to an existing room.
Parameters: { room_id: string, user_id: int }
Returns: { invite_id, sent: bool }
```

### 6.4 User Tools

#### `users_list`
```
List all Rustyfin users.
Parameters: {}
Returns: array of { id, username, display_name, role, is_online }
```

#### `users_get`
```
Look up a user by name or ID.
Parameters: { query: string }
Returns: { id, username, display_name, role }
```

### 6.5 Minecraft Server Tools

#### `servers_list`
```
List all Minecraft server instances and their current status.
Parameters: {}
Returns: array of { id, name, status, player_count, version }
```

#### `servers_start`
```
Start a Minecraft server. Admin-only.
Parameters: { server_id: string }
Returns: { started: bool, message: string }
```

#### `servers_stop`
```
Stop a running Minecraft server. Admin-only.
Parameters: { server_id: string }
Returns: { stopped: bool }
```

### 6.6 Media Tools

#### `media_search`
```
Search the Rustyfin media library.
Parameters: { query: string, type: "movie"|"series"|"any"?, limit: int? }
Returns: array of { id, title, year, type, overview }
```

#### `media_continue_watching`
```
Get the continue-watching list for a user.
Parameters: { user_id: int? }   — defaults to caller
Returns: array of { title, progress_pct, last_watched }
```

---

## 7. Agent Loop Design

```rust
// crates/ai-agent/src/agent.rs

pub struct AgentLoop {
    engine: Arc<LlamaEngine>,
    tools: ToolRegistry,
    context: ContextBuilder,
}

impl AgentLoop {
    /// Run one user turn. Yields SSE-compatible events.
    pub fn run(
        &self,
        history: Vec<ChatMessage>,
        user_message: String,
        sampling: SamplingParams,
        ctx: Arc<ToolContext>,
    ) -> impl Stream<Item = Result<AgentEvent, AiError>> {
        // 1. Build messages: system + history + new user message
        // 2. Call engine.chat_stream() with tool schemas in system prompt
        // 3. Stream tokens → yield AgentEvent::Token
        // 4. On tool_call detected in output:
        //    → yield AgentEvent::ToolStart(name, args)
        //    → execute tool async via ToolRegistry
        //    → yield AgentEvent::ToolResult(name, result)
        //    → append tool result message, loop back to step 2
        // 5. On generation complete → yield AgentEvent::Done
    }
}

pub enum AgentEvent {
    Token(String),                              // stream to browser
    ToolStart { name: String, args: String },   // show badge in UI
    ToolResult { name: String, result: String },
    Stats {
        prompt_tokens: u64,
        completion_tokens: u64,
        total_duration_ms: u64,
        tokens_per_second: f64,
    },
    Done,
    Error(String),
}
```

Hard cap of **10 tool-call rounds** per turn to prevent runaway execution.

---

## 8. HTTP API

### 8.1 POST `/api/v1/ai/chat`
**Auth:** Bearer required
**Body:**
```json
{
  "model": "llama3.2-3b-q4",
  "message": "When is Jake's birthday?",
  "conversation_id": "uuid (optional)"
}
```
**Response:** `text/event-stream` (SSE)

```
event: token
data: {"text": "Jake"}

event: tool_start
data: {"name": "calendar_get_birthdays", "args": "{}"}

event: tool_result
data: {"name": "calendar_get_birthdays", "result": "[{\"display_name\":\"Jake\",...}]"}

event: token
data: {"text": "'s birthday is March 22nd."}

event: stats
data: {"prompt_tokens": 312, "completion_tokens": 48, "tokens_per_second": 31.4, "total_duration_ms": 1530}

event: done
data: {"conversation_id": "uuid"}
```

### 8.2 GET `/api/v1/ai/models`
Returns GGUF models discovered in the model directory.
```json
{
  "models": [
    {
      "name": "llama3.2-3b-q4",
      "file": "llama3.2-3b-q4_k_m.gguf",
      "size_gb": 2.0,
      "parameter_size": "3B",
      "quantization": "Q4_K_M",
      "architecture": "llama",
      "context_length": 131072
    }
  ],
  "inference_available": true,
  "model_dir": "/var/lib/rustyfin/models"
}
```

### 8.3 POST `/api/v1/ai/models/pull`
SSE stream downloading a GGUF file. Body: `{"model": "llama3.2:3b", "url": null}` (null = use curated catalog URL).

### 8.4 DELETE `/api/v1/ai/models/:name`
Deletes the GGUF file from disk. Returns 204 No Content.

### 8.5 GET `/api/v1/ai/gpus`
Returns per-GPU VRAM info from `nvidia-smi`, current GPU layer config, and the multi-GPU explanation.

### 8.6 GET `/api/v1/ai/running`
Returns the currently loaded model name and its VRAM footprint.

### 8.7 GET `/api/v1/ai/conversations` *(Phase 4)*
Returns paginated conversation history for the current user.

---

## 9. System Prompt

```
You are the Rustyfin assistant — a helpful AI built into a personal home media server.
You have access to live tools that let you read and act on real server data.

Current user: {display_name} (id: {user_id})
Server time: {iso_datetime}

Available tools:
- Calendar: view upcoming events and birthdays, create or delete events
- Rooms: list active watch-party rooms, create a room, invite users
- Users: look up who is on the server
- Minecraft servers: check status, start or stop servers (admin only)
- Media: search the library, check continue-watching progress

Rules:
- Always call tools when you need live data. Do not guess dates, names, or statuses.
- When creating something (event, room), confirm the details back to the user first
  unless they've made the intent completely unambiguous.
- Keep responses concise. This is a utility assistant, not a conversationalist.
- Never reveal raw user IDs or database internals in your response.
```

---

## 10. Database Schema

### 10.1 AI Settings (stored in existing `settings` table)

| Key | Default | Description |
|---|---|---|
| `ai_model_dir` | `/var/lib/rustyfin/models` | Path where GGUF files are stored |
| `ai_default_model` | `""` | Filename of default model |
| `ai_n_gpu_layers` | `-1` | GPU layers (-1 = all) |
| `ai_tensor_split` | `""` | Comma-separated GPU fractions e.g. `0.5,0.5` |
| `ai_n_ctx` | `4096` | Context window size |
| `ai_temperature` | `0.8` | Sampling temperature |
| `ai_max_tokens` | `2048` | Max generated tokens per turn |

### 10.2 Conversation History *(Phase 4)*

Migration: `041_ai_conversations.sql`

```sql
CREATE TABLE ai_conversations (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  model        TEXT NOT NULL,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ai_messages (
  id              BIGSERIAL PRIMARY KEY,
  conversation_id UUID NOT NULL REFERENCES ai_conversations(id) ON DELETE CASCADE,
  role            TEXT NOT NULL CHECK (role IN ('user','assistant','tool')),
  content         TEXT NOT NULL,
  tool_name       TEXT,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ai_messages_conv_idx ON ai_messages(conversation_id, created_at);
```

---

## 11. AiConfig (server-side)

```rust
pub struct AiConfig {
    /// Directory containing GGUF model files.
    pub model_dir: PathBuf,
    /// Currently loaded model filename (None = no model loaded).
    pub active_model: Option<String>,
    /// GPU layers to offload. -1 = all.
    pub n_gpu_layers: i32,
    /// Per-GPU layer fractions for multi-GPU split. Empty = auto.
    pub tensor_split: Vec<f32>,
    /// Context window size in tokens.
    pub n_ctx: u32,
    /// Inference thread count.
    pub n_threads: u32,
    /// Max tool-call rounds per turn.
    pub max_tool_rounds: u32,
    /// Messages to include as context per turn.
    pub conversation_history_limit: usize,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from("/var/lib/rustyfin/models"),
            active_model: None,
            n_gpu_layers: -1,
            tensor_split: vec![],
            n_ctx: 4096,
            n_threads: 8,
            max_tool_rounds: 10,
            conversation_history_limit: 20,
        }
    }
}
```

Config values are read from the `settings` DB table on startup and can be changed at runtime (model reload required when `n_gpu_layers` or `tensor_split` change).

---

## 12. Frontend Design

### 12.1 Page Layout (`/ai`)
```
┌────────────────────────────────────────────────────────┬────────────┐
│  HEADER                                                │            │
│  "AI Assistant"  [Model selector]  [online]  [Models] │  MGMT      │
├────────────────────────────────────────────────────────┤  PANEL     │
│  CHAT THREAD  (scrollable)                             │            │
│  [User bubble]  "When is Jake's birthday?"             │  Pull model│
│  [Tool badge]   calendar_get_birthdays  ✓              │  + progress│
│  [AI bubble]    "Jake's birthday is March 22nd..."     │  ─────     │
│  [Stats]  Prompt 312tok · Output 48tok · 31.4 t/s     │  Installed │
├────────────────────────────────────────────────────────┤  models    │
│  INPUT BAR                                             │  ─────     │
│  [textarea — auto-resize]           [Send ▶]           │  GPU info  │
└────────────────────────────────────────────────────────┴────────────┘
```

### 12.2 No Model Loaded State
When `inference_available` is true but no model is selected/loaded, show:
```
No model loaded.
Open the Models panel → pull a model to get started.
Recommended: llama3.2:3b-q4 (2 GB)
```

### 12.3 Inference Not Available State
If the server cannot load llama.cpp (e.g. missing CUDA libraries at compile time), show:
```
AI inference is not available on this build.
Rebuild rustfin-server with GPU or CPU inference features enabled.
```

### 12.4 Tool Call Badge
```
⚙ calendar_get_birthdays  [checking…]  →  ✓ done
```

### 12.5 Stats Bar
Shown under each completed AI message:
```
Prompt 312 tok  ·  Output 48 tok  ·  31.4 t/s  ·  1.5s
```

### 12.6 Thinking Blocks
Models like `deepseek-r1` emit `<think>…</think>` blocks before their visible response. These are extracted and shown in a collapsible "Thinking…" panel with purple accent styling.

---

## 13. Implementation Phases

### Phase 0 — Temporary Ollama Shim *(COMPLETE — to be removed)*
The existing `OllamaClient` in `crates/ai-agent/src/client.rs` provides a working chat
UI backed by an Ollama HTTP call. This exists to allow frontend development to proceed in
parallel. **This code must be deleted when Phase 1 is complete.**

### Phase 1 — Native Inference Engine *(replaces Phase 0)*
Goal: working chat UI backed by `llama-cpp-2` in-process inference with no Ollama dependency.

1. Add `llama-cpp-2` to `crates/ai-agent/Cargo.toml` with CUDA feature.
2. Write `build.rs` to pass GPU backend flags to llama.cpp's cmake build.
3. Implement `LlamaEngine` with `load()` and `chat_stream()`.
4. Implement `ModelStore`: GGUF discovery, metadata extraction, file download with progress.
5. Replace `OllamaClient` usage in `crates/server/src/ai.rs` with `LlamaEngine`.
6. Remove `src/client.rs` (OllamaClient).
7. Update `GET /ai/models` to read from `ModelStore` instead of Ollama `/api/tags`.
8. Update `GET /ai/running` to reflect in-process loaded model.
9. Update the UI "offline" state copy from "Ollama not running" to "No model loaded".

**Done when:** User can pull a GGUF model via the UI, select it, send a message, and receive a streamed response — all without Ollama installed.

### Phase 2 — Tool Calling (read-only tools)
Goal: AI can answer live questions about the server.

1. Implement `ToolRegistry`, `ToolDef`, `AgentLoop` with tool-call detection cycle.
2. Implement read tools: `calendar_list_events`, `calendar_get_birthdays`, `users_list`, `rooms_list_active`, `media_continue_watching`, `servers_list`.
3. Add tool-call SSE events (`tool_start`, `tool_result`); add `ToolCallBadge` to frontend.
4. Integrate dynamic system prompt with current user context.

**Done when:** "When is Jake's birthday?" triggers a live calendar tool call and returns accurate data.

### Phase 3 — Write Tools
Goal: AI can take actions on the user's behalf.

1. Implement write tools: `calendar_create_event`, `rooms_create`, `rooms_invite_user`, `servers_start`, `servers_stop`.
2. Confirmation pattern: AI states the intended action, user replies "yes"/"no".
3. Permission guard in `ToolContext`: non-admins cannot start/stop servers.

**Done when:** "Create a movie night room and invite everyone" works end-to-end.

### Phase 4 — Conversation History
Goal: persist chat history per user across sessions.

1. Add DB migration `041_ai_conversations.sql`.
2. Implement `ConversationStore` in `crates/db/src/repo/ai.rs`.
3. Update `POST /ai/chat` to persist and reload conversation history.
4. Add conversation list sidebar to the `/ai` page.

---

## 14. Security Considerations

- **Tool permissions**: write tools check `caller_user_id` role. `servers_start`/`servers_stop` are admin-only. All read tools are user-accessible.
- **Input sanitisation**: tool arguments are deserialised via `serde_json` into typed Rust structs — no string interpolation into SQL or shell commands.
- **Prompt injection**: the system prompt is constructed in Rust from trusted data only. User messages occupy the `user` role. Tool results are always `tool` role and cannot overwrite the system prompt.
- **No external inference calls**: all inference runs in-process. The AI routes make no external network requests for inference.
- **Model file safety**: GGUF files are stored in a dedicated directory. The model store does not execute model code outside of llama.cpp's controlled FFI boundary.
- **Rate limiting (future)**: add a per-user token budget counter to prevent one user monopolising the inference engine.
- **GPU memory exhaustion**: `LlamaEngine::load()` validates that the requested `n_gpu_layers` fits within available VRAM before loading, returning a clear error if not.

---

## 15. Build Notes — llama.cpp GPU Backends

llama.cpp ships with its own cmake build system. The `llama-cpp-2` crate handles compilation but requires the appropriate GPU SDK to be present on the build host.

```bash
# NVIDIA CUDA (Debian)
sudo apt install nvidia-cuda-toolkit

# Build with CUDA
cargo build -p rustfin-server --features "rustfin-ai-agent/cuda"

# CPU-only (no GPU required)
cargo build -p rustfin-server --features "rustfin-ai-agent/cpu"
```

The Debian native install script (`install_native_debian.sh`) must be updated to detect CUDA availability and set the appropriate feature flag when building.

---

## 16. Future Directions *(out of scope for current phases)*

- **Voice input**: pipe mic audio through `rustfin-transcription-agent` (Whisper) and send the transcript to the AI chat endpoint.
- **Embedding-based recommendations**: use a local embedding model (e.g. `nomic-embed-text`) via llama.cpp to find "movies similar to what I've been watching."
- **RAG over media metadata**: vector search over plot summaries and credits for richer "find me a movie about…" queries.
- **Scheduled automations**: "remind me about the watch party an hour before" — agent registers a job in the scheduler.
- **Multi-model routing**: send long-context tasks to a bigger model, quick lookups to a smaller one.
- **Per-user GPU quota**: allocate VRAM budget per user when multiple concurrent sessions are active.
