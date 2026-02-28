# Rustyfin Efficiency Deep Dive - Sixth Pass (Transcripts)

Date: 2026-02-26

## Scope

This pass focused on transcript-heavy paths in voice channels:

- Session list/status query efficiency.
- Entry-count query patterns (remove N+1 counting).
- Running-session lookup ordering/index use.
- Stable text message pagination cursoring for equal timestamps.

## Implemented Changes

### 1) Batched transcript entry counting (remove N+1)

Files:

- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/channel_transcripts.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`

Details:

- Added repo API:
  - `count_entries_for_sessions(pool, session_ids)`
- Updated handlers:
  - `list_transcription_sessions` now fetches counts in one grouped query.
  - `get_transcription_status` uses batched count path for selected session.
  - `start_transcription` (already-running branch) uses batched count path.
  - `stop_transcription` reuses `entries.len()` instead of issuing a second DB count query.

Impact:

- Replaces per-session count loops with one grouped count query.
- Reduces DB round trips and load during transcript history rendering.

### 2) Transcript index tuning

Files:

- `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations/022_transcript_query_indexes.sql`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/migrate.rs`

Indexes added:

- `idx_channel_transcript_session_status_started` on `(status, started_ts DESC)`
  - Supports running-session listings and status checks.
- `idx_channel_transcript_entry_session` on `(session_id)`
  - Supports grouped entry-count queries.

### 3) Stable text-message cursor pagination (same-timestamp safety)

Files:

- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/channels.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/channelsApi.ts`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/components/TextChannelView.tsx`

Details:

- Added optional `before_id` cursor support.
- Query now paginates with:
  - `(created_ts < before_ts) OR (created_ts = before_ts AND id < before_id)`
  - `ORDER BY created_ts DESC, id DESC`
- UI passes oldest message `id` with timestamp when loading older messages.

Impact:

- Avoids duplicates/holes when many messages share same second-level timestamp.

## Query Plan Verification

Validated with temporary PostgreSQL DBs and `EXPLAIN QUERY PLAN`.

Observed improvements:

- Running transcript session list:
  - Before: scan + temp B-tree sort.
  - After: index search via `idx_channel_transcript_session_status_started`.
- Entry counts grouped by `session_id IN (...)`:
  - Uses covering index path on `session_id` (now explicit dedicated index).

## Validation Run

Passed:

- `cargo fmt --all`
- `cargo check -p rustfin-db`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server migrations_are_idempotent -- --nocapture`
- `cd ui && npx tsc --noEmit`

## Outcome

This pass reduces transcript DB overhead by eliminating N+1 count patterns and improving index alignment for active-session and count queries, while also hardening message pagination behavior under timestamp ties.
