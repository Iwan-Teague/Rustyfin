# Rustyfin Efficiency Deep Dive
Date: 2026-02-26  
Author: Rustyfin Engineering Audit

## Scope
This pass focused on:
- Redundant code patterns across Rust services and high-traffic UI room components.
- Low-risk DRY opportunities with no product behavior regressions.
- Security consistency for inter-service auth handling.
- Practical wins that reduce future maintenance cost.

## Methodology
1. Codebase inventory and hotspot scan:
   - Counted source files by language.
   - Ranked largest files by LOC to identify complexity hotspots.
2. Duplicate-pattern analysis:
   - Exact-block duplicate scan on project source (`.rs`, `.ts`, `.tsx`, `.sh`) excluding build/runtime artifacts.
   - Manual inspection of known hot files (`watch_party`, room UI components, start scripts).
3. Refactor selection criteria:
   - Safe behavior-preserving changes.
   - Shared utility extraction with immediate reuse.
   - Prefer centralized logic for auth/error paths.

## Baseline Snapshot
- Source files scanned (code-related): `148`
- Rust files: `72`
- UI TS/TSX files: `56`
- Largest Rust hotspots:
  - `crates/server/src/watch_party/handlers.rs` (~3775 LOC)
  - `crates/server/src/routes.rs` (~2711 LOC)
  - `crates/server/src/watch_party/ws.rs` (~2316 LOC)
- Largest UI hotspots:
  - `ui/src/app/admin/page.tsx` (~2110 LOC)
  - `ui/src/app/rooms/[roomId]/page.tsx` (~1946 LOC)
  - `ui/src/app/rooms/components/CreateTogetherEditor.tsx` (~1703 LOC)

## Key Findings
## 1) Repeated Axum API error wrappers across services
Same `AppError`/`IntoResponse` glue was redefined in multiple service binaries and in server-local wrapper code.

Risk:
- Drift in error envelope behavior across services.
- More maintenance and copy-paste overhead.

## 2) Repeated agent-token verification and secret normalization logic
`tmdb-agent`, `youtube-agent`, and `transcription-agent` each carried near-identical `normalized_secret` and `verify_agent_token` implementations.

Risk:
- Security logic divergence.
- Inconsistent error text and future bugfix duplication.

## 3) Repeated watch-source tab/header rendering in room page
`Local Media / YouTube / Web` header + right-aligned role/control chips were repeated across multiple watch-room branches.

Risk:
- UI inconsistency over time.
- High-cost edits for simple style/behavior updates.

## 4) Repeated clear-search button markup in room search UIs
Same clear icon button existed in both YouTube and audio room search inputs.

Risk:
- Inconsistent interaction styling when modified.
- Repeated code in high-change areas.

## Implemented Improvements
## A) Centralized Axum error wrappers in `rustfin-core`
- Added shared module:
  - `crates/core/src/axum_error.rs`
- Exposes:
  - `AppError`
  - `AppErrorWithCode`
- Added module export:
  - `crates/core/src/lib.rs`
- Enabled `axum` dependency in core:
  - `crates/core/Cargo.toml`

Applied usage:
- `crates/server/src/error.rs` now re-exports shared types.
- Removed duplicated wrapper implementations in:
  - `crates/calendar/src/main.rs`
  - `crates/tmdb-agent/src/main.rs`
  - `crates/transcription-agent/src/main.rs`
  - `crates/youtube-agent/src/main.rs`

Impact:
- Unified API envelope/status behavior across services.
- Fewer duplicate implementations.

## B) Centralized agent auth utilities in `rustfin-core`
- Added:
  - `crates/core/src/agent_auth.rs`
- Provides:
  - `normalize_secret(...)`
  - `verify_agent_token(...)`
- Exported via:
  - `crates/core/src/lib.rs`

Applied usage:
- `crates/tmdb-agent/src/main.rs`
- `crates/transcription-agent/src/main.rs`
- `crates/youtube-agent/src/main.rs`

Impact:
- Single source of truth for token verification semantics.
- Reduced security-related copy-paste.

## C) Reusable room watch-source tab bar component
- Added:
  - `ui/src/app/rooms/components/WatchSourceTabsBar.tsx`
- Replaced repeated header/tab rendering in:
  - `ui/src/app/rooms/[roomId]/page.tsx`

Impact:
- Single implementation for watch-source tabs + right-aligned badge row.
- Lower UI maintenance overhead and easier consistency fixes.

## D) Reusable clear-search button component
- Added:
  - `ui/src/app/rooms/components/ClearSearchButton.tsx`
- Reused in:
  - `ui/src/app/rooms/components/AudioPlayer.tsx`
  - `ui/src/app/rooms/components/YouTubePlayer.tsx`

Impact:
- Consistent search-clear UX.
- Lower repeated JSX noise in room components.

## Validation Performed
- Rust formatting:
  - `cargo fmt --all`
- Rust compile checks:
  - `cargo check -p rustfin-core -p rustfin-calendar -p rustfin-tmdb-agent -p rustfin-transcription-agent -p rustfin-youtube-agent -p rustfin-server`
- UI TypeScript checks:
  - `./ui/node_modules/.bin/tsc -p ui/tsconfig.json --noEmit`

All checks passed.

## Additional High-Value Opportunities (Next Pass)
These are strong candidates for further improvements:

1. Break up monolithic backend files:
   - `crates/server/src/watch_party/handlers.rs`
   - `crates/server/src/watch_party/ws.rs`
   - `crates/server/src/routes.rs`
   - Refactor into focused modules by domain (`auth`, `queue`, `youtube`, `web`, `create`, `invites`).

2. Extract common room control primitives in UI:
   - Playback action row patterns in `AudioPlayer` and room-level player controls.
   - Reusable role/control chip sections.

3. Reduce render pressure in `AudioPlayer`:
   - Isolate queue/search result list rendering into memoized child components.
   - Avoid full-list re-render on timeline tick updates.

4. Consolidate repeated script bootstrap logic:
   - Shared compose/env parsing across:
     - `scripts/start.sh`
     - `scripts/stop.sh`
     - `scripts/clean_install.sh`

5. Add lightweight perf guards:
   - UI: React profiler run around rooms/channels heavy views.
   - Backend: tracing spans around hot watch-party handlers/ws paths with timing histograms.

## Summary
This pass removed clear structural duplication with low regression risk and improved consistency in:
- service error handling,
- service token auth validation,
- watch room source-header UI,
- room search clear-control UI.

The codebase is now better positioned for a larger modularization pass in the two biggest hotspots: watch-party backend handlers/ws and room/admin UI monolith pages.
