# Rustyfin AI Assistant

Date: 2026-03-14
Last revised: 2026-03-15
Status: current implementation baseline

## Purpose

Rustyfin AI is a local assistant surface for authenticated users at `/ai`.

Its current job is narrow and explicit:

- let signed-in users chat with a locally hosted model
- keep model installation and storage management out of the end-user UI
- degrade cleanly when the host cannot support inference

## Implemented Product Boundary

### End-user surface

- `/ai`
  - authenticated chat UI
  - model selection from already installed models
  - streaming SSE responses
  - initial grounded read-only answers for selected Rustyfin domains
  - no install, delete, or storage-folder controls

### Admin surface

- `Admin` -> `AI`
  - inspect the current model directory
  - see whether the path came from database, environment, or default
  - download a `.gguf` model from a direct `http` or `https` URL
  - delete installed models
  - change the AI model directory

### Server routes

- user-facing:
  - `GET /api/v1/ai/models`
  - `POST /api/v1/ai/chat`
- admin-only:
  - `GET /api/v1/system/ai`
  - `PUT /api/v1/system/ai`
  - `POST /api/v1/system/ai/models/pull`
  - `DELETE /api/v1/system/ai/models/{name}`

Removed legacy surface:

- `GET /api/v1/ai/running`
- `GET /api/v1/ai/gpus`
- end-user model install/delete endpoints on `/api/v1/ai/*`

## Backend Architecture

Current ownership is split like this:

- `crates/ai-agent`
  - inference engine and chat primitives
- `crates/server/src/ai.rs`
  - feature-gated AI entrypoint and disabled-host fallback
- `crates/server/src/ai_enabled.rs`
  - authenticated `/api/v1/ai` routes
- `crates/server/src/ai_assistant`
  - grounded assistant context, tool registry, planner, and read-only product tools
- `crates/server/src/ai_audit.rs`
  - durable assistant audit-event shaping and admin-facing parsing
- `crates/server/src/ai_admin.rs`
  - admin-only model-management routes plus recent assistant audit history
- `crates/server/src/ai_storage.rs`
  - model directory resolution, validation, discovery, download, and delete behavior

The server loads models from disk on demand and keeps one active loaded model in process at a time through the shared engine state in `AppState`.

Grounded assistant requests now also persist a compact audit record containing:

- trace ID
- user and role snapshot
- model name
- normalized prompt preview
- planned tools
- executed tool summaries
- grounded source summaries
- terminal response kind and any terminal error message

Assistant audit retention now defaults to 30 days with hourly cleanup, and can be overridden with:

- `RUSTFIN_AI_AUDIT_RETENTION_DAYS`

The current grounded read-only tool slice can summarize:

- signed-in account context
- visible upcoming calendar events
- visible upcoming birthdays
- tighter visible calendar-event details used after direct questions or grounded follow-ups
- named birthday lookups narrowed to visible matching people instead of a broad birthday list
- recent visible channel activity, while explicitly reporting that exact unread tracking is not available yet
- transcript-based summaries of accessible completed voice calls
- authenticated host-published downloads and planned Rustyfin artifacts
- host-visible network topology and saved Rustyfin network settings
- accessible libraries
- accessible library title matches for a user-provided query
- recently added accessible library items
- tighter single-item library summaries used after grounded entity-reference follow-ups
- active public rooms
- joinable rooms, including public lobbies and direct invites
- tighter active-room summaries used after grounded entity-reference follow-ups
- authenticated public weather through a fixed provider path
- admin-only host runtime stats such as RAM, memory usage, CPU thread count, CPU usage, load, uptime, and compact Rustyfin runtime counters
- admin-only backup capability summaries
- admin-only internal service-health summaries
- admin-only transcoding summaries
- admin-only storage summaries
- admin-only recent-error summaries
- admin-only constrained public web search results when `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`
- admin-only constrained public page summaries for explicit public URLs when `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`
- accessible Minecraft server status
- tighter single-server summaries used after grounded entity-reference follow-ups

Current query-understanding support includes:

- calendar windows such as `today`, `tomorrow`, `this week`, `next week`, `this month`, `next month`
- direct calendar-event detail prompts such as `Tell me more about the Team Meeting event`
- named birthday queries such as `When is Rachel's birthday?`, with backend-side narrowing against visible birthday entries
- network prompts such as `What network interfaces are active right now?`, `What IP address is this server on?`, and `Is remote access enabled?`
- recent channel-activity prompts such as `Any unread activity in general chat?`
- numbered windows such as `next 10 days` and `next 2 weeks`
- ISO-date targeting such as `2026-03-22`
- safer library-search intent detection so generic questions like library access are not misclassified as title search
- recently added library prompts such as `What was recently added to my library?`
- transcript-summary prompts such as `What was the call in General Voice about?` and `What did they talk about in that call?`
- ambiguous calendar questions such as “What’s on my calendar?” now trigger a clarification prompt instead of a guessed default window
- room-mode filtering such as YouTube rooms, watch rooms, audio rooms, web rooms, screen-share rooms, create rooms, and play rooms
- joinable-room prompts such as `What rooms can I join right now?`
- Minecraft server filtering by availability such as `online`, `offline`, `healthy`, or `failed`
- named Minecraft server matching, including prompts such as `Is the Minecraft server called Survival online?`
- ambiguous singular server questions such as “Is the server online?” now trigger a clarification prompt instead of guessing which server was meant
- authenticated weather prompts such as `What is the temperature in Dublin right now?` and `What is the forecast for Cork tomorrow?`
- ambiguous weather questions without a location now trigger a clarification prompt instead of guessing a place
- admin-only host runtime-stat prompts such as `How much RAM is the server using right now?` and `How many CPU threads does the server have?`
- admin-only service-health prompts such as `What services are down right now?`
- admin-only storage prompts such as `How much free space is left on disk?`
- admin-only transcode prompts such as `Are there any active transcodes?`
- admin-only recent-error prompts such as `What failed recently?`
- explicit public URLs can now trigger a constrained public page fetch, summary, and source attribution path
- `/api/v1/ai/chat` now streams lightweight assistant status events before token output so the `/ai` page can show progress such as checking calendar, rooms, or server state without exposing chain-of-thought
- `/api/v1/ai/chat` now uses a model-assisted structured planner for grounded tool selection, while the backend keeps deterministic fallback and deterministic entity-follow-up resolution as the safe normalization path
- short follow-up prompts can now inherit the last grounded domain as a planner hint, so prompts like `What about next week?` or `Which ones are healthy?` can stay grounded without restating the domain
- simple entity-reference follow-ups such as `the second one`, `that server`, or `the first room` can now resolve against the last grounded result set and rerun a fresh scoped detail tool for that entity
- grounded assistant requests now emit traceable server logs and assistant chat/tool counters into the admin runtime diagnostics surface

## Runtime Model

AI backend selection is host-safe and explicitly controlled by:

- `RUSTFIN_AI_GPU_BACKEND=auto|disabled|cpu|cuda|rocm|vulkan`

Expected behavior:

- supported hosts use the selected backend
- unsupported hosts can fall back to `disabled`
- Rustyfin keeps running even when AI is unavailable

When AI is disabled at build/runtime, `/api/v1/ai/*` returns a controlled `503` response with `inference_available: false` instead of crashing the host.

## Model Storage

The active model directory resolves in this order:

1. database setting `ai_model_dir`
2. environment variable `RUSTFIN_AI_MODEL_DIR`
3. default path `/var/lib/rustyfin/ai/models`

Behavioral rules:

- model directories must be absolute paths
- directories are validated and created if needed
- model downloads are written to a `.part` file and renamed on completion
- changing the model directory clears the currently loaded model state
- deleting the currently loaded model clears the in-memory engine state

## Security Rules

- AI chat requires a normal authenticated Rustyfin session
- grounded tool execution stays server-side and read-only
- curated fixed-provider public-data tools can be available to normal authenticated users when they do not expose arbitrary browsing
- constrained public web tools stay admin-only and are disabled by default unless `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`
- model installation, deletion, and path changes require `AdminUser`
- no cloud inference provider is part of the current design
- only local GGUF files are used
- do not reintroduce end-user model management to `/ai`

## Frontend Ownership

- `ui/src/app/ai/page.tsx`
  - chat-focused user surface
- `ui/src/lib/aiApi.ts`
  - user-facing AI API helpers
- `ui/src/app/admin/page.tsx`
  - admin AI management tab
- `ui/src/lib/aiAdminApi.ts`
  - admin AI management API helpers

## Directional Constraints

Future AI work should preserve these constraints:

- keep the end-user page chat-focused
- keep model lifecycle management admin-only
- keep the host-safe backend selection path
- keep AI optional so unsupported hosts can still run the rest of Rustyfin

## Planned Next Step

The next major AI architecture step is grounded server-side tool calling so the assistant can answer with live Rustyfin data instead of prompt-only guesses.

That design is captured in:

- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
