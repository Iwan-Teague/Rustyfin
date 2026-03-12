# Backend Efficiency Audit Pass 2 - 2026-03-12

## Scope
This follow-up pass focused on production panic edges in hot media-serving routes and playback-related handlers.

## Findings

### 1. Media response construction still used `unwrap()` in hot routes
Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/streaming.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`

The project still had several `Response::builder(...).body(...).unwrap()` call sites in playback download, direct media streaming, and watch-party audio serving. These are user-facing hot paths.

Impact:
- low-probability but avoidable request panics in media-heavy flows
- poor resilience in exactly the paths users hit most during regular viewing/listening
- unnecessary crash risk for a long-running home server

Fix implemented:
- replaced `unwrap()` with structured internal error mapping
- kept the response behavior the same when construction succeeds
- preserved the existing security headers and range semantics

### 2. Playback download target-height validation still relied on `expect(...)`
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`

The transcode download path validated a requested height and then used `expect(...)` to assume the validated value existed.

Impact:
- small but unnecessary panic edge in a production playback path

Fix implemented:
- replaced `expect(...)` with explicit error conversion

### 3. Media-info serialization still used `unwrap()`
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`

The ffprobe/media-info endpoint serialized response payloads with `serde_json::to_value(...).unwrap()`.

Impact:
- avoidable panic edge on a user-facing endpoint

Fix implemented:
- replaced the unwrap with explicit serialization error handling

## Implemented Changes
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/streaming.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`

## Result
This pass does not change product behavior when everything is healthy. It reduces crash risk in hot playback/streaming paths by preferring structured request failure over panic.
