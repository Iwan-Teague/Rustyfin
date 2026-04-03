# AI Assistant Delta Phase 0-2 Execution Report

Date: 2026-04-01
Local branch: `ai-assistant-delta-phase0-2`
Implemented commit: `aa113f9` (`Implement AI assistant delta phases 0-2`)
Deployed live branch tip: `4345beb` (`Cover native TLS certs with host aliases`)
Implementation source of truth: `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`

## Scope Requested

Execute Phases 0, 1, and 2 of the AI assistant delta plan end-to-end:

- Phase 0: correctness, honesty, and timing
- Phase 1: conversation persistence and navigation
- Phase 2: assistant activity view and observability

Then deploy to the live Ubuntu server at `server@192.168.0.36`, verify the running system there, and report what is complete, partial, and deferred.

## Completion Status

### Complete

#### Phase 0

- Added richer assistant turn timing with backward-compatible `total_duration_ms` plus:
  - `planner_duration_ms`
  - `tool_duration_ms`
  - `generation_duration_ms`
  - `end_to_end_duration_ms`
  - `queue_duration_ms`
  - `model_load_duration_ms`
- Added human-readable host runtime memory summaries from grounded tooling:
  - `used_memory_human`
  - `total_memory_human`
  - `used_memory_gib`
  - `total_memory_gib`
  - `memory_summary`
- Added deterministic unsupported-write refusal for unsupported create/update/delete prompts so the assistant cannot imply success for read-only operations.
- Added deterministic `calendar_get_next_event` support and server-side routing for next-event prompts.
- Added deterministic host-runtime follow-up routing for common unit-conversion prompts such as `what is that in gigabytes`.

#### Phase 1

- Added dedicated PostgreSQL conversation storage:
  - `ai_conversation`
  - `ai_conversation_turn`
- Added conversation repo helpers for create/list/get/update/delete plus turn persistence.
- Added authenticated conversation API routes:
  - `GET /api/v1/ai/conversations`
  - `POST /api/v1/ai/conversations`
  - `GET /api/v1/ai/conversations/:id`
  - `PATCH /api/v1/ai/conversations/:id`
  - `DELETE /api/v1/ai/conversations/:id`
  - `POST /api/v1/ai/conversations/:id/messages/stream`
- Kept legacy `POST /api/v1/ai/chat` working during migration.
- Rebuilt `/ai` into a persisted conversation shell with:
  - desktop left rail
  - mobile hamburger + left drawer
  - real `New chat`
  - stored history
  - rename/archive/delete flows
  - conversation switching while another conversation is streaming

#### Phase 2

- Added structured assistant activity persistence and streaming:
  - `phase` SSE events
  - `tool` SSE events
  - stored `activity_trace_json` per assistant turn
- Reworked assistant turn presentation into:
  - `Thinking...`
  - ordered tool rows
  - final answer
  - turn stats below
- Added Codex-style `Thinking...` sweep animation and activity row styling.
- Removed source-chip-first answer presentation from the main assistant answer flow.
- Kept exact tool names available as secondary detail beneath readable labels.
- Added deterministic short-circuits for obvious unsupported-write and next-event paths before planner overuse.

### Partial

- Local integration tests for DB-backed server behavior were compiled but not executed because `RUSTFIN_TEST_DATABASE_URL` and `RUSTFIN_DATABASE_URL` were not set locally.
- Live authenticated conversation CRUD and live SSE turn execution were not exercised on the running host because:
  - no application credentials were provided for a Rustyfin account
  - the live runtime did not expose a persisted `RUSTFIN_JWT_SECRET` in accessible env files for non-invasive JWT minting
- Browser or E2E coverage for mobile drawer behavior and live conversation switching was not added in this slice; the frontend behavior was validated through static build plus code-path review rather than end-to-end browser automation.

### Deferred

- Phase 3 safe calendar writes and confirmation-gated birthday creation
- Phase 4 voice input
- Phase 5 live AI runtime and GPU telemetry panel

These remain deferred exactly as planned and were not part of the requested execution slice.

## Local Verification

### Passed

- `cargo fmt --all`
- `cargo check`
- `cargo check -p rustfin-server --features ai`
- `cargo test -p rustfin-server --features ai --lib`
- `cargo test -p rustfin-db next_occurrence_`
- `cargo test -p rustfin-server --features ai --test integration --no-run`
- `npm --prefix ui run build`

### Key Added Coverage

- planner routing for next-event prompts
- unsupported-write refusal regression
- calendar next-occurrence repo logic
- conversation ownership and ordering
- conversation stream persistence path

### Not Run

- full DB-backed integration execution locally, because test database environment variables were unavailable

## Live Deployment

### Active Host Discovery

Verified the active live runtime before deployment:

- systemd unit: `rustyfin-native.service`
- active working directory: `/home/server/docker/Rustyfin-main-33b3d14`
- service user: `server`
- pre-deploy live checkout matched `main` at local baseline commit `6604208`

### Deployment Result

Switched the active live checkout from `main` to branch `ai-assistant-delta-phase0-2`.

The live host ultimately deployed branch tip `4345beb`, which contains:

- `aa113f9` `Implement AI assistant delta phases 0-2`
- `cc44732` `Add AI assistant delta execution docs`
- `4345beb` `Cover native TLS certs with host aliases`

The first deploy attempt failed during the CUDA-backed `rustfin-server` rebuild with:

- `could not find native static library cudart_static`

To recover, a host-side CUDA linker-path fix was applied on the Ubuntu server:

- wrote `/etc/environment.d/50-rustyfin-cuda.conf`
- exported `RUSTFLAGS=-L/usr/lib/x86_64-linux-gnu -L/usr/lib/cuda/lib64` for the deployment shell

After that remediation, `./scripts/deploy-native.sh` completed successfully and restarted the live stack.

## Live Verification

### Service Health

Verified on the live Ubuntu host after deploy:

- deployed branch: `ai-assistant-delta-phase0-2`
- deployed commit: `4345beb`
- `rustyfin-native.service`: active
- `rustfin-servers-agent.service`: active
- `rustyfin-post-healthcheck.service`: exited successfully

### Deployed Artifacts

Verified rebuilt live backend artifact:

- `/home/server/docker/Rustyfin-main-33b3d14/.native-bins/x86_64-unknown-linux-gnu/dev/rustfin-server`
- rebuilt timestamp: `2026-04-01 15:23:45.843071902 +0100`

### UI And API Surface

Verified on the live host:

- `https://127.0.0.1:3008/login` returned `200`
- `https://127.0.0.1:3008/ai` returned `200`
- the `/ai` HTML shell references `/_next/static/chunks/app/ai/page-b74bf5a8fff19eff.js`
- `https://127.0.0.1:3008/api/v1/ai/chat` returned authenticated `401` when called without a token
- `https://127.0.0.1:3008/api/v1/ai/models` returned authenticated `401` when called without a token
- `https://127.0.0.1:3008/api/v1/ai/conversations` returned authenticated `401` when called without a token
- `https://127.0.0.1:3008/api/v1/ai/conversations/test/messages/stream` returned authenticated `401` when called without a token

This confirms both the legacy and conversation-backed AI route surfaces are mounted behind the running live edge and protected by auth.

### Database Verification

Verified the new live PostgreSQL tables exist:

- `ai_conversation`
- `ai_conversation_turn`

## Changed Files In Scope

- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/044_ai_conversations.sql`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/migrate.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/ai_conversations.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/calendar.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/mod.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/mod.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_audit.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_conversations.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/lib.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/tests/integration.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/globals.css`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-assistant/components/AiAssistantActivity.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-assistant/components/AiConversationRail.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts`

## Final Assessment

Phases 0, 1, and 2 are implemented and deployed on the live Ubuntu server.

What is still incomplete is not the code slice itself, but the final layer of live authenticated product verification: without a Rustyfin account credential or a persisted JWT secret on the host, I could not non-invasively execute real authenticated conversation CRUD and SSE turns against the running service.

Within those constraints, the strongest available live checks were completed:

- exact commit deployed
- services healthy
- post-start healthcheck passed
- new DB schema present
- new `/ai` UI bundle live
- new protected AI API routes live
