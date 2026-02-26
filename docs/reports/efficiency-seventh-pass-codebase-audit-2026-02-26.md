# Rustyfin Codebase Improvement Audit (Pass 7)

Date: 2026-02-26
Scope: Rust backend/services, Next.js UI, DB repository layer, scripts, test surface.

## Executive Summary

The project has strong momentum and broad functionality, but complexity has concentrated in a few very large files and cross-cutting flows (rooms, watch-party, websocket state). The next quality gains come from modularization, stricter API typing, and reducing high-churn state duplication.

Top opportunities:
1. Split monolithic files in `watch_party`, `routes`, and room UI.
2. Harden API/type boundaries in UI (remove `any`, eliminate direct `res.json()` calls).
3. Reduce network/DB overhead in collaborative flows (delta updates, fewer insert-then-select round trips).
4. Tighten security/operational defaults (`/jobs` exposure, logging gates, CI coverage).

## Evidence Snapshot

- Code scale (selected hotspots):
  - `crates/server/src/watch_party/handlers.rs`: 3875 lines
  - `crates/server/src/routes.rs`: 2851 lines
  - `crates/server/src/watch_party/ws.rs`: 2494 lines
  - `ui/src/app/admin/page.tsx`: 2118 lines
  - `ui/src/app/rooms/[roomId]/page.tsx`: 1940 lines
  - `ui/src/app/rooms/components/CreateTogetherEditor.tsx`: 1797 lines
  - `scripts/start.sh`: 986 lines
- Type looseness:
  - 50 occurrences of `catch(err: any)`, `catch(e: any)`, or `as any` across UI/backend search scope.
- Protocol compatibility debt:
  - UI still handles both `youtube_state` and `you_tube_state` variants.
- Routing concentration:
  - `crates/server/src/routes.rs` contains ~82 `fn`/`async fn` handlers and router setup.
- Realtime page concentration:
  - `ui/src/app/rooms/[roomId]/page.tsx` contains 20 `useState` stores and large websocket + UI orchestration.
- Tests:
  - E2E suite exists, but there are no dedicated room-mode E2E specs for watch/listen/create/play flows.
- Good signs:
  - No active TODO/FIXME markers in primary source paths.
  - Multiple DB performance index migrations already added (018/019/021/022).
  - Smart rebuild logic is present in `scripts/start.sh`.

## Priority Findings and Improvements

## P0 - High Impact, Low Risk

### 1) Remove remaining direct `Response.json()` parsing in UI network paths
Problem:
- Some API code still calls `res.json()` directly and can throw `Unexpected end of JSON input` on empty/204 responses.

Evidence:
- `ui/src/lib/setupApi.ts`
- `ui/src/lib/channelsApi.ts`
- `ui/src/app/login/page.tsx`
- `ui/src/app/admin/page.tsx` (mixed patterns)

Improvement:
- Route all network parsing through shared safe parser logic in `ui/src/lib/api.ts` (`parseResponseBody` + `apiJson`) and remove direct `res.json()` except where strictly required.

Benefit:
- Eliminates a recurring class of runtime parsing errors and reduces duplicated error parsing logic.

### 2) Replace `any`-based error handling with a typed app error utility
Problem:
- Error handling is broad and repetitive, reducing reliability and IDE safety.

Evidence:
- Concentrated in `ui/src/app/admin/page.tsx`, `ui/src/app/rooms/[roomId]/page.tsx`, `ui/src/app/rooms/page.tsx`, `ui/src/app/player/[id]/page.tsx`.

Improvement:
- Introduce a small typed utility:
  - `type AppClientError = { message: string; code?: string; status?: number; details?: unknown }`
  - helper `toClientError(unknown) -> AppClientError`
- Replace `catch (err: any)` with `catch (err: unknown)` + helper.

Benefit:
- Better correctness, fewer silent type escapes, easier refactors.

### 3) Add sane default limits to jobs/log endpoints
Problem:
- `/api/v1/jobs` can return unbounded rows when no `limit` is provided.

Evidence:
- `crates/server/src/routes.rs` in `list_jobs` passes `limit: Option<i64>` directly.
- `crates/db/src/repo/jobs.rs` supports unlimited path when `limit` is `None`.

Improvement:
- Set server default limit (for example 100) when no limit supplied.
- Keep explicit larger limit allowed for admins.

Benefit:
- Better performance and safer UI behavior as log tables grow.

## P1 - Architecture and Maintainability

### 4) Decompose `watch_party` backend by mode and responsibility
Problem:
- `handlers.rs` and `ws.rs` are very large and represent multiple domains in one file.

Evidence:
- `crates/server/src/watch_party/handlers.rs`: 3875 lines
- `crates/server/src/watch_party/ws.rs`: 2494 lines

Improvement:
- Split into modules:
  - `handlers/video.rs`, `handlers/audio.rs`, `handlers/youtube.rs`, `handlers/create.rs`, `handlers/play.rs`
  - `ws/commands/*.rs` with command dispatch table
  - `ws/state_sync.rs` for broadcast + state snapshot patterns

Benefit:
- Easier testing, lower merge conflict rate, safer feature work.

### 5) Decompose room client orchestration into focused hooks/components
Problem:
- `ui/src/app/rooms/[roomId]/page.tsx` mixes websocket lifecycle, transport fallback, mode switching, media control, modal handling, and rendering.

Improvement:
- Extract hooks:
  - `useRoomRealtime(roomId, joinedRole)`
  - `useRoomReconfigure(...)`
  - `useRoomPlayback(...)`
- Keep page as composition/layout container.

Benefit:
- Lower cognitive load and easier defect isolation.

### 6) Consolidate top tab bars into one shared component
Problem:
- Tabs in watch/create rooms share behavior and styling but are implemented separately.

Evidence:
- `ui/src/app/rooms/components/WatchSourceTabsBar.tsx`
- `ui/src/app/rooms/components/CreateToolTabsBar.tsx`

Improvement:
- Replace with a generic `RoomModeTabsBar` (options + active key + badges + disabled policy).

Benefit:
- Less duplicated styling logic and easier UI consistency.

## P2 - Performance and Data Flow

### 7) Replace full-state collaborative updates with incremental operations
Problem:
- Create Together sends full text payload or full canvas stroke arrays, which scales poorly.

Evidence:
- `CreateTogetherEditor` currently sends full `create_set_text` and full `create_set_canvas` payloads.

Improvement:
- Move to operation-based updates:
  - Text: patch/delta operations (OT/CRDT-friendly)
  - Canvas: append-stroke/remove-stroke operations instead of full array replacement

Benefit:
- Lower websocket bandwidth, better responsiveness on long sessions.

### 8) Remove insert-then-select DB round trips where data is already known
Problem:
- Several repo functions insert, then query the inserted row by id.

Evidence:
- `crates/db/src/repo/channel_transcripts.rs`:
  - `create_running_session` inserts then `get_session`
  - `append_entry` inserts then `SELECT ... WHERE id = ?`
- `crates/db/src/repo/watch_party.rs`:
  - online track upsert path inserts/updates then fetches by `video_id`

Improvement:
- Return constructed row in-process when safe.
- Where needed, use `RETURNING` (SQLite >= 3.35) with fallback strategy.

Benefit:
- Fewer DB round trips in high-frequency paths.

### 9) Avoid JSON field filtering in hot room listing queries
Problem:
- Public/admin room listing relies on `json_extract(r.policy_json, '$.invite_only')` filters/computations.

Evidence:
- `crates/db/src/repo/watch_party.rs` list queries.

Improvement:
- Add a dedicated indexed `invite_only` column (or generated column) and backfill from `policy_json`.

Benefit:
- More index-friendly list queries at scale.

## P3 - Build and Ops

### 10) Pin Rust toolchain to stable and make Docker toolchain deterministic
Problem:
- Dockerfiles use `rustlang/rust:nightly-bookworm` without visible nightly-only feature use.

Evidence:
- No `#![feature(...)]` in source tree.

Improvement:
- Move to stable image and add `rust-toolchain.toml` pin.

Benefit:
- More reproducible builds and reduced breakage risk from nightly regressions.

### 11) Improve smart rebuild fingerprint granularity
Problem:
- Service fingerprints include full compose file for each service; unrelated compose edits can invalidate all service fingerprints.

Evidence:
- `scripts/start.sh` `compute_all_service_fingerprints` includes `"$compose_scope_file"` in every service fingerprint.

Improvement:
- Compute per-service compose hash (service-specific section or service build args only).

Benefit:
- Better rebuild selectivity and faster local iteration.

### 12) Add root `.dockerignore`
Problem:
- Root build context filtering is not explicit in repo root.

Improvement:
- Add root `.dockerignore` to exclude `.git`, `target`, `node_modules`, `.next`, test artifacts, and docs bundles not needed at build time.

Benefit:
- Faster context transfer and fewer accidental cache invalidations.

## P4 - Security, Observability, and Test Coverage

### 13) Restrict jobs/logs visibility scope
Problem:
- `/api/v1/jobs` is currently protected by `AuthUser`, not `AdminUser`.

Evidence:
- `crates/server/src/routes.rs` `list_jobs(_auth: AuthUser, ...)`.

Improvement:
- Either:
  - restrict to admins, or
  - expose user-scoped jobs only and redact sensitive payload fields.

Benefit:
- Reduced information leakage risk.

### 14) Add room-specific E2E suites
Problem:
- Existing E2E coverage does not include dedicated room-mode flows.

Improvement:
- Add suites/specs for:
  - Watch Together source switching (local/youtube/web)
  - Listen Together queue control + online download status
  - Create Together document/canvas sync basics
  - Play Together chess turn and permission behavior

Benefit:
- Prevents regressions in the most complex real-time surface.

### 15) Add structured websocket telemetry and event IDs
Problem:
- Debug output is present but not consistently structured for correlation across backend/UI.

Improvement:
- Add event IDs + structured `tracing` fields in backend room/ws paths.
- Gate verbose client logs behind explicit debug flag.

Benefit:
- Faster incident diagnosis and less noisy production logs.

## Recommended Implementation Plan

## Phase 1 (1-3 days)
- P0 items 1-3.
- Add root `.dockerignore`.
- Start with typed error utility and JSON parsing normalization.

## Phase 2 (3-5 days)
- P1 items 4-6.
- Break down `routes.rs` and room page orchestration into modules/hooks.

## Phase 3 (4-7 days)
- P2 items 7-9.
- Introduce operation-based collaborative updates and DB query/index cleanup.

## Phase 4 (2-4 days)
- P3/P4 items 10-15.
- Toolchain pinning, CI hardening, security scope updates, room E2E suites.

## Quick Wins To Start Immediately

1. Standardize on `apiJson` parsing and remove direct `res.json()` call sites.
2. Add default jobs API limit and admin-only scope review.
3. Replace `any` catch blocks in room/admin pages with typed error conversion.
4. Split room page websocket logic into `useRoomRealtime` hook first (lowest-risk extraction).

