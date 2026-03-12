# Runtime Diagnostics Counters

This pass adds lightweight in-process runtime diagnostics for the native Debian 12 Rustyfin runtime.

## Goal

Add cheap counters that help answer:

- are background jobs piling up or failing?
- are HLS transcode sessions being created and cleaned up normally?
- are websocket session counts drifting or growing unexpectedly?
- are internal agent calls succeeding, failing, or hanging in flight?

The implementation is intentionally simple:

- lock-free atomics for counters
- no new database writes
- no extra polling loops
- one admin-only read endpoint

## Implemented Counters

### Jobs

Tracked in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/runtime_metrics.rs`.

Per family:

- `library_scan`
- `tmdb_sync`
- `server_operations`
- `admin_audit`
- `other`

Per family fields:

- `enqueued_total`
- `running_total`
- `active_running`
- `completed_total`
- `failed_total`

These are incremented from the actual enqueue and lifecycle paths in:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/library_scan.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/audit_log.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`

### Transcoding

Tracked in `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder/src/session.rs`.

Fields:

- `active_sessions`
- `created_total`
- `create_failures_total`
- `cleaned_total`

These cover the most important HLS lifecycle signals without adding more periodic work.

### Websocket Sessions

Tracked in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/runtime_metrics.rs`.

Fields:

- `channels.active`
- `channels.connections_total`
- `watch_party.active`
- `watch_party.connections_total`

Instrumentation points:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/ws.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/ws.rs`

### Agent Calls

Tracked in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/runtime_metrics.rs`.

Per agent:

- `servers`
- `tmdb`
- `transcription`
- `youtube`

Per agent fields:

- `calls_total`
- `calls_succeeded_total`
- `calls_failed_total`
- `calls_in_flight`

Instrumentation points:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/agent_client.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/transcription_agent.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`

## Endpoint

Admin-only endpoint:

- `GET /api/v1/system/runtime-diagnostics`

Implemented in:

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`

The response contains:

- uptime
- job counters
- websocket counters
- agent call counters
- transcode counters

## Why This Is Safe

- counters are in-memory only
- all updates use relaxed atomics
- no hot-path locking was added
- no database schema changes were required
- websocket active counts are guarded by drop-based lifecycle tracking, so they unwind even on early disconnects

## Operational Use

This endpoint is meant for:

- debugging long-lived host behavior
- spotting growing websocket counts
- confirming transcode sessions are being cleaned up
- checking whether agent requests are hanging in flight
- verifying background job churn without querying the database directly

## Remaining Gaps

The next useful pass would be:

1. surface these counters in the admin UI
2. add bounded per-minute rolling rates for agent failures and job failures
3. add runtime memory/queue depth signals for the channel manager and watch-party manager
