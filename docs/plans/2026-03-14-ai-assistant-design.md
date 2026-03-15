# Rustyfin AI Assistant

Date: 2026-03-14
Last revised: 2026-03-14
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
- `crates/server/src/ai_admin.rs`
  - admin-only model-management routes
- `crates/server/src/ai_storage.rs`
  - model directory resolution, validation, discovery, download, and delete behavior

The server loads models from disk on demand and keeps one active loaded model in process at a time through the shared engine state in `AppState`.

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
