# Backend Efficiency Audit - 2026-03-12

## Scope
This pass focused on long-running backend paths that matter on a native Debian 12 home server: process lifecycle management, periodic scheduler query shape, and panic resistance in setup flows.

## Findings

### 1. Transcode session cleanup held the global session mutex across slow work
File: `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder/src/session.rs`

The HLS session manager was holding the shared `sessions` mutex while awaiting ffmpeg child shutdown and while deleting transcode directories from disk. That creates avoidable lock contention and can serialize unrelated session operations behind slow process exit or filesystem cleanup.

Impact:
- worse tail latency on create/stop/ping paths under load
- higher risk of awkward contention during repeated seeks, playback switches, or cleanup bursts
- poor lifecycle isolation for a service intended to run continuously

Fix implemented:
- remove matching sessions from the map under lock
- perform child termination and directory cleanup after the lock is released
- switch directory existence checks to async `tokio::fs::try_exists`

### 2. TMDB auto-sync scheduler had a periodic N+1 query pattern
File: `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs`

The scheduler tick listed all libraries and then fetched settings one library at a time. That is a repeated N+1 pattern inside a background loop.

Impact:
- unnecessary DB round-trips every scheduler tick
- background churn that scales poorly with library count
- avoidable wakeups on a server meant to stay on indefinitely

Fix implemented:
- batch-fetch settings using `get_library_settings_for_libraries(...)`
- build an in-memory map keyed by `library_id`
- reuse that map inside the tick loop

### 3. Setup library validation had a panic edge on malformed values
File: `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/handlers.rs`

The setup library creation handler assumed the validation error payload was always a JSON object and used `unwrap()` on that assumption.

Impact:
- low-probability but unnecessary panic on a user-facing setup route
- poor failure isolation for a path that should always return structured validation errors

Fix implemented:
- replaced the `unwrap()` with safe object matching
- added a generic field error fallback if the payload shape is ever not an object

## Implemented Changes
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder/src/session.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/handlers.rs`

## Remaining Good Candidates

### A. Continue reducing runtime `unwrap` / `expect` usage on user-facing paths
There are still runtime call sites where panics are unlikely but not impossible. The next pass should target the non-test, non-obviously-internal cases only.

### B. Review websocket backpressure and queue sizing again after more live usage
The current bounded-queue approach is materially better than the original unbounded shape, but audio/channel realtime behavior should still be watched under multi-user sessions.

### C. Consider more explicit metrics or lightweight health counters for background jobs
Library scan, TMDB sync, transcription, and Minecraft server actions would be easier to reason about operationally with cheap in-process counters exposed through diagnostics.

## Result
This pass improves backend efficiency and durability without changing product behavior:
- less lock contention in transcoding lifecycle management
- fewer database round-trips in recurring background work
- fewer panic edges on setup validation
