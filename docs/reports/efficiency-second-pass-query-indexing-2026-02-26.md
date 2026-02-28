# Rustyfin Efficiency Second Pass (Query Indexing)

Date: 2026-02-26  
Scope: targeted backend DB indexing + query plan verification for recently optimized hot paths.

## Summary

Implemented a focused index migration to remove remaining full scans / temp sort trees on high-traffic queries introduced in the first optimization pass.

### Added migration

- `crates/db/migrations/018_query_performance_indexes.sql`

### Migration wired into runner

- `crates/db/src/migrate.rs`

## Indexes Added

1. `idx_channel_attachment_message_created`
- Table: `channel_message_attachment`
- Columns: `(message_id, created_ts, id)`
- Supports:
  - batched message attachment fetch ordered by `(message_id, created_ts, id)`.

2. `idx_library_path_library_created`
- Table: `library_path`
- Columns: `(library_id, created_ts, id)`
- Supports:
  - batched library path fetch ordered by `(library_id, created_ts, id)`.

3. `idx_item_library_parent`
- Table: `item`
- Columns: `(library_id, parent_id)`
- Supports:
  - top-level item counting by library (`parent_id IS NULL`) with grouping.

4. `idx_watch_party_online_audio_track_room_created_updated`
- Table: `watch_party_online_audio_track`
- Columns: `(room_id, created_ts DESC, updated_ts DESC)`
- Supports:
  - online audio track listing ordered by `created_ts DESC, updated_ts DESC`.

## EXPLAIN QUERY PLAN Verification

Executed before/after plan snapshots on fresh PostgreSQL schemas built from migrations (before: `001..017`, after: `001..018`) using representative seed data.

### Before

- `channel_message_attachment`: used `idx_channel_attachment_message`, plus temp B-tree for ORDER BY tail.
- `library_path`: full table scan + temp B-tree for ORDER BY.
- `item` count: used `idx_item_library_kind` only.
- `watch_party_online_audio_track`: used `idx_watch_party_online_audio_track_room`, plus temp B-tree for ORDER BY tail.

### After

- `channel_message_attachment`: direct seek via `idx_channel_attachment_message_created`; no temp sort tree.
- `library_path`: direct seek via `idx_library_path_library_created`; no scan.
- `item` count: covering index seek via `idx_item_library_parent`.
- `watch_party_online_audio_track`: direct seek via `idx_watch_party_online_audio_track_room_created_updated`; no temp sort tree.

## Validation

Commands executed:

- `cargo fmt --all`
- `cargo check -p rustfin-db`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server migrations_are_idempotent -- --nocapture`

All passed.
