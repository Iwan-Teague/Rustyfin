# Backend Efficiency Audit Pass 3 - 2026-03-12

## Scope
This pass continued the runtime hardening work by removing additional production `unwrap()` / `expect()` usage from backend and backend-adjacent crates where those calls could still surface as avoidable panics.

## Findings

### 1. Calendar event creation still assumed the follow-up read must exist
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/calendar.rs`

After inserting a calendar event, the repo reloaded the row and used `expect(...)` on the result.

Fix implemented:
- replaced the `expect(...)` with `sqlx::Error::RowNotFound`
- this preserves failure signaling without panicking

### 2. TV scan cleanup regex was being rebuilt and unwrapped during scans
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs`

The TV parsing path was compiling a regex inline and unwrapping it while normal scan work was happening.

Impact:
- unnecessary work in a repeated scan path
- avoidable unwrap in a production code path

Fix implemented:
- promoted the regex to a shared static `LazyLock`
- removed the runtime `unwrap()` from the scan flow itself

### 3. Servers-host HTTP client initialization still used `expect(...)`
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-host/src/lib.rs`

The native Minecraft-host helper used a fallible client builder with `expect(...)` in static initialization.

Fix implemented:
- switched to a non-fallible shared `reqwest::Client::new()`
- preserved the Rustyfin user-agent by attaching it to the specific outbound requests

### 4. Route hex encoding helper still used `write!(...).unwrap()`
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`

A helper used for subtitle-path encoding still relied on infallible formatting plus `unwrap()`.

Fix implemented:
- replaced it with direct nibble-to-hex encoding with preallocated output

### 5. Main server startup still used `expect(...)` to install SIGTERM handling
File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs`

If SIGTERM handler installation failed, the process would panic on startup.

Fix implemented:
- replaced the panic with structured warning logs
- fallback behavior is now Ctrl-C-only shutdown handling instead of crash

## Implemented Changes
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/calendar.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-host/src/lib.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs`

## Result
This pass further reduces avoidable production panic edges and removes a small amount of repeated scan-time overhead, without changing the intended product behavior.
