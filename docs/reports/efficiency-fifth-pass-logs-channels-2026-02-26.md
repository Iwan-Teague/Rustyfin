# Rustyfin Efficiency Deep Dive - Fifth Pass (Logs + Channels DB)

Date: 2026-02-26

## Scope

This pass focused on high-frequency admin log reads and channel message paging:

- Admin logs endpoint (`GET /jobs`) previously returned the full table and relied on client-side filtering.
- Channel message pagination relied on timestamp-only cursor ordering.
- Existing indexes did not fully cover current sort/filter patterns.

## Implemented Changes

### 1) Server-side logs filtering + pagination

Files:

- `crates/server/src/routes.rs`
- `crates/db/src/repo/jobs.rs`

Details:

- Added query support for `/jobs`:
  - `status` (`all`, `complete`, `failed`, `in_progress`, `queued`, `running`, `cancelled`, `error`)
  - `kind`
  - `limit`
  - `offset`
- Added `list_jobs_filtered(...)` in DB repo with SQL-level filtering and ordering.
- Kept `list_jobs(...)` as wrapper for backward compatibility.

### 2) Admin UI now uses filtered jobs endpoint

File:

- `ui/src/app/admin/page.tsx`

Details:

- Replaced full `/jobs` fetch with targeted requests:
  - logs panel: `/jobs?status=<tab>&limit=300`
  - TMDB panel status source: `/jobs?kind=library_tmdb_sync&limit=1000`
  - active jobs polling signal: `/jobs?status=in_progress&limit=100`
- Removed client-side log filtering loop over full jobs list.
- TMDB sync status computation now uses TMDB-specific job list.

### 3) Channel message pagination stability

Files:

- `crates/server/src/channels/handlers.rs`
- `crates/db/src/repo/channels.rs`

Details:

- Added optional `before_id` to message query input.
- DB query now supports stable pagination tie-break:
  - `(created_ts < ? OR (created_ts = ? AND id < ?))`
  - `ORDER BY created_ts DESC, id DESC`
- Backward compatible: if `before_id` absent, timestamp-only behavior still works with improved ordering.

### 4) New DB indexes for logs/channels patterns

Files:

- `crates/db/migrations/021_logs_channels_query_indexes.sql`
- `crates/db/src/migrate.rs`

Indexes added:

- `idx_job_created_ts` on `job(created_ts DESC)`
- `idx_job_status_created_ts` on `job(status, created_ts DESC)`
- `idx_job_kind_created_ts` on `job(kind, created_ts DESC)`
- `idx_channel_position_created` on `channel(position, created_ts)`
- `idx_channel_message_channel_ts_id` on `channel_message(channel_id, created_ts DESC, id DESC)`

## Query Plan Verification

Validated with temporary SQLite DBs and `EXPLAIN QUERY PLAN`.

Key deltas:

- Jobs status filter (`status IN (...) ORDER BY created_ts DESC`) moved from full table scan to index-backed status search.
- Jobs kind filter (`kind = ? ORDER BY created_ts DESC`) uses `idx_job_kind_created_ts`.
- Channel message page query moved from older index + temp sort to `idx_channel_message_channel_ts_id`.
- Channel list ordering now uses `idx_channel_position_created`.

## Validation Run

Passed:

- `cargo fmt --all`
- `cargo check -p rustfin-db`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server migrations_are_idempotent -- --nocapture`
- `cd ui && npx tsc --noEmit`

## Outcome

This pass reduced avoidable DB and payload overhead by:

- moving log filtering/paging to SQL (instead of client-side full-list filtering),
- narrowing admin polling payloads to purpose-specific subsets,
- and making channel pagination more index-aligned and stable under equal timestamps.
