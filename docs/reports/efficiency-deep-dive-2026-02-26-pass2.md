# Rustyfin Efficiency Deep Dive (Pass 2)
Date: 2026-02-26  
Author: Rustyfin Engineering Audit

## Executive Summary
Rustyfin is functionally rich and architecture-aligned with your Rust-first direction, but performance and maintainability are now constrained by three patterns:

1. N+1 data access and sequential pipelines in hot backend paths.
2. Large client-side state surfaces with redundant polling in Next.js pages.
3. Build pipeline duplication across Rust service Dockerfiles.

The fastest wins are backend query batching, frontend polling reduction, and bounded parallelism for expensive media/download tasks.

## Scope and Method
This pass focused on real hotspots (high-LOC files + high-frequency request paths) and concrete pipeline opportunities.

Reviewed files included:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/ws.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/watch_party.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/libraries.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/youtube-agent/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/admin/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/rooms/[roomId]/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/channelsContext.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/components/VoiceEngine.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/start.sh`
- `/Users/iwanteague/Desktop/Rustyfin/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/crates/*/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/ui/Dockerfile`

## Current Hotspots (Measured)
- Rust largest files:
  - `crates/server/src/watch_party/handlers.rs` (~3775 LOC)
  - `crates/server/src/routes.rs` (~2711 LOC)
  - `crates/server/src/watch_party/ws.rs` (~2316 LOC)
- UI largest files:
  - `ui/src/app/admin/page.tsx` (~2110 LOC)
  - `ui/src/app/rooms/[roomId]/page.tsx` (~1895 LOC)
  - `ui/src/app/rooms/components/CreateTogetherEditor.tsx` (~1809 LOC)

These large files also contain the heaviest runtime codepaths (admin load/polling, room websocket orchestration, room reconfigure/invite logic).

## Priority Findings

## P0: Backend N+1 and Sequential Data Access
### 1) User and library listing N+1 patterns
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:352`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:767`

`list_users_route` fetches users, then per-user library access.  
`list_libraries` loops libraries and calls `library_row_to_response`, which triggers per-library subqueries (paths, settings, item_count).

Impact:
- Admin and setup endpoints do avoidable DB round trips.
- Latency scales with number of users/libraries.

Recommended fix:
- Add repo-level batch APIs:
  - `list_users_with_library_access(...)`
  - `list_libraries_with_paths_settings_counts(...)`
- Materialize maps once, then assemble response in-memory.

### 2) Watch room reconfigure/invite N+1 patterns
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:2059`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:2748`

`reconfigure_room` and `invite_members` repeatedly call member/user queries inside loops.

Impact:
- Reconfigure and invite cost grows quickly with room size.

Recommended fix:
- Batch load members/users once per request.
- Move access checks to set-based validation where possible.

### 3) Channel messages attachment fetch is N+1
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs:459`

`get_messages` fetches message list, then attachments message-by-message.

Impact:
- Text channel history loads do unnecessary query fan-out.

Recommended fix:
- Add `list_attachments_for_messages(pool, &[message_id])`.
- Group by `message_id` in memory when building response.

### 4) In-memory filtering instead of SQL filtering for audio track search
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/watch_party.rs:728`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/watch_party.rs:960`

`get_library_tracks` and `list_online_audio_tracks` fetch full sets, then filter in Rust.

Impact:
- Expensive with large libraries/track histories.
- Higher memory and slower query-to-first-byte.

Recommended fix:
- Push filtering and pagination into SQL (`WHERE ... LIKE`, `LIMIT`, `OFFSET`).

## P0: YouTube Download Pipeline Can Be Faster and More Predictable
### 5) Sequential fallback chain causes long waits under partial failure
- `/Users/iwanteague/Desktop/Rustyfin/crates/youtube-agent/src/main.rs` (download attempt loop sections around `download_youtube_audio_mp3_for_room`)

Current strategy is robust but mostly sequential; each attempt can consume significant timeout budget.

Impact:
- Slow “time to first playable track” in online listen rooms.

Recommended fix (pipeline):
- Use bounded parallel race for candidate profiles (e.g., 2-3 concurrent attempts).
- Cancel remaining attempts on first valid stream.
- Cache successful request profile per source host for short TTL to bias future requests.

## P1: Frontend Polling + State Fan-out
### 6) Admin page polls full dataset every 1s/5s
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/admin/page.tsx:434`

`loadData()` fetches libraries, jobs, users, tmdb config, channels, rooms together; polling interval tightens to 1s when active jobs exist.

Impact:
- Unnecessary network and render pressure.
- Work scales with all tabs, even when viewing only one.

Recommended fix:
- Poll only jobs aggressively when needed.
- Lazy-load other tab data on tab activation.
- Add stale windows and visibility-aware polling.

### 7) Room page uses websocket and periodic HTTP refresh simultaneously
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/rooms/[roomId]/page.tsx:472`

There is websocket state sync plus 5s `refreshRoom` polling.

Impact:
- Duplicate updates and avoidable room fetch load.

Recommended fix:
- Use websocket as primary source of truth.
- Keep periodic refresh as degraded fallback only (e.g., disconnected ws state).

### 8) Presence updates duplicate state writes across multiple room state slices
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/rooms/[roomId]/page.tsx:645`

A single presence event updates up to five state trees (`roomState`, `audioState`, `youtubeState`, `webState`, `createState`) with near-identical mapping code.

Impact:
- Extra render churn and complexity.

Recommended fix:
- Normalize member presence state into one store and derive child states from it.

## P1: Channels Voice Stack Modernization Opportunity
### 9) Transcription capture still uses `ScriptProcessorNode`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/components/VoiceEngine.tsx:556`

`createScriptProcessor` is deprecated and main-thread heavy.

Impact:
- Potential jitter/glitches under load.

Recommended fix:
- Migrate transcription capture path to `AudioWorkletNode`.
- Keep gain/monitoring compatible with existing WebRTC pipeline.

## P1: Build Pipeline Duplication and Compile Time Cost
### 10) Each Rust service Dockerfile recompiles full workspace context
- `/Users/iwanteague/Desktop/Rustyfin/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/crates/tmdb-agent/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/crates/youtube-agent/Dockerfile`
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/Dockerfile`

All copy full `crates/` and run separate cargo build loops.

Impact:
- Long rebuilds when touching shared crates.

Recommended fix:
- Adopt `cargo-chef` (planner/cook stages) with shared dependency layers.
- Keep per-service final stages minimal; reuse compiled deps aggressively.

### 11) UI Docker build mutates config via `sed`
- `/Users/iwanteague/Desktop/Rustyfin/ui/Dockerfile`

Build modifies `next.config.js` literals at image build time.

Impact:
- Brittle and cache-unfriendly.

Recommended fix:
- Remove `sed` patching; rely on env-driven config (`RUSTYFIN_API_BASE_URL`, `RUSTYFIN_CALENDAR_API_BASE_URL`) consistently.

## P2: Scanner Pipeline and Metadata Throughput
### 12) Library scan path is highly sequential and mixes blocking ffprobe calls
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs:1`
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs:343`

Scan loops file-by-file with multiple DB lookups and optional per-file `ffprobe` shell execution.

Impact:
- Slow large-library scan completion.

Recommended fix:
- Add bounded worker pipeline:
  - stage 1 walk -> candidate queue
  - stage 2 parse/normalize (parallel)
  - stage 3 DB upsert in transaction batches
  - stage 4 metadata probe queue (bounded, async subprocess management)

## P2: Smaller but Valuable Cleanup
### 13) Duplicate YouTube helper logic exists across watch-party modules
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/ws.rs:421`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:324`

Examples: YouTube ID validation and JSON extraction helpers.

Recommended fix:
- Extract shared `watch_party::youtube_utils` module.

### 14) Channels websocket origin allowlist not cached like watch-party path
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs:72`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/ws.rs:76`

Recommended fix:
- Cache parsed allowed origins in channels ws path via `OnceLock<Vec<String>>`.

## “Modern Ways” Gap Assessment
You are using modern building blocks (Axum, Tokio, Next.js 15, React 19), but not fully exploiting current best practices in these areas:

- Data fetching: manual polling/state orchestration instead of query cache primitives.
- State model: broad local state trees where normalized stores/reducers would reduce rerender fan-out.
- Service networking: request client reuse inconsistent across high-churn agent codepaths.
- Build system: Rust multi-service builds not yet optimized with layered dependency cooking.

## Recommended Delivery Plan

## Phase 1 (1-3 days, low-risk)
1. Batch message attachments query (remove channel message N+1).
2. Batch user/library access responses in admin APIs.
3. Switch room page HTTP polling to fallback-only when websocket is healthy.
4. Convert audio track search to SQL filtering + pagination.
5. Cache channels websocket allowed origins.

Expected effect:
- Lower DB call count on common admin/rooms/channels paths.
- Smoother UI under load.

## Phase 2 (3-7 days, medium)
1. Refactor admin page into tab-scoped data loaders and memoized subcomponents.
2. Normalize room presence state in one source of truth.
3. Rework youtube-agent attempts to bounded parallel race with fast cancellation.
4. Introduce shared reqwest clients in youtube-agent hot paths.

Expected effect:
- Faster perceived response in rooms/admin.
- Lower median and tail latency for online audio track staging.

## Phase 3 (1-2 weeks, larger)
1. Build pipeline upgrade with `cargo-chef` and shared dependency layers.
2. Scanner multi-stage bounded pipeline for large libraries.
3. AudioWorklet migration for transcription capture in `VoiceEngine`.
4. Optional auth model upgrade (HTTP-only cookie sessions) to unlock more Server Component usage.

Expected effect:
- Major rebuild time reduction.
- Better scalability for scanning/transcription-heavy sessions.

## Concrete Quick Wins I’d Start With
1. `get_messages` attachment batching (`crates/server/src/channels/handlers.rs` + `crates/db/src/repo/channels.rs`).
2. `list_users_route` + `list_libraries` response batching (`crates/server/src/routes.rs`, `crates/db/src/repo/*`).
3. SQL-side audio search (`crates/db/src/repo/watch_party.rs`).
4. Disable redundant room polling when websocket connected (`ui/src/app/rooms/[roomId]/page.tsx`).
5. Channels ws origin caching (`crates/server/src/channels/ws.rs`).

These five are high ROI and low behavior risk.

## Final Assessment
The project is strong on feature velocity and breadth. The main speed and maintainability gains now come from:
- reducing round trips,
- pipelining expensive operations with bounded concurrency,
- and tightening frontend state/data flow.

The code can be noticeably quicker without architecture changes, starting with the P0/P1 items above.
