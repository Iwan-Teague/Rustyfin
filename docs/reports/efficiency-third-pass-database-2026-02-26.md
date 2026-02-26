# Rustyfin Efficiency Third Pass (Database)

Date: 2026-02-26  
Scope: deeper DB-side optimization for room/channel paths, with query-plan verification.

## What Was Optimized

1. Removed room/member/user N+1 query patterns in watch-room handlers.
2. Replaced expensive local-audio duration join with indexed correlated subquery.
3. Added targeted migration-backed indexes for room member listing and local audio lookup.

## Code Changes

### New batched user lookup API
- `crates/db/src/repo/users.rs`
  - Added `list_users_by_ids(pool, user_ids)`.

### New member+username query
- `crates/db/src/repo/watch_party.rs`
  - Added `WatchPartyMemberWithUsernameRow`.
  - Added `list_members_with_usernames(pool, room_id)`.

### Handler query reductions
- `crates/server/src/watch_party/handlers.rs`
  - Added shared `load_users_by_ids(...)` helper.
  - `eligible_libraries`: switched from per-user `find_by_id` + `get_library_access` calls to batched user + access queries.
  - `create_room`: batched invitee user fetch and one-time video-item library lookup.
  - `reconfigure_room`: replaced per-member user fetch loop with batched user lookup.
  - `get_room`: now uses `list_members_with_usernames` directly (removed global users list scan).
  - `invite_members`: loads existing members once, loads invite users in batch, and does one-time video-item library lookup.

### SQL rewrite for local audio tracks
- `crates/db/src/repo/watch_party.rs`
  - `get_library_tracks(...)` now resolves `duration_ms` via correlated subquery:
    - `episode_file_map` + `media_file` lookup per track, ordered by `efm.created_ts`, `LIMIT 1`.
  - This removes broad left-join expansion over `episode_file_map` for the full track set.

## New Migration

- `crates/db/migrations/019_database_query_optimizations.sql`
  - `idx_watch_party_member_room_invited_user` on `(room_id, invited_ts, user_id)`
  - `idx_episode_file_map_episode_created` on `(episode_item_id, created_ts)`
- Registered in `crates/db/src/migrate.rs` as:
  - `"019_database_query_optimizations"`

## EXPLAIN QUERY PLAN Results

Comparison run on migration baseline `001..018` (before) vs `001..019` (after), with representative seed data.

### Improved plans

1. Room member listing
- Before:
  - index seek by `room_id` plus temp B-tree sort for `ORDER BY invited_ts, user_id`.
- After:
  - direct indexed plan using `idx_watch_party_member_room_invited_user`.
  - temp sort removed.

2. Local audio duration lookup subquery
- Before:
  - correlated subquery scanned `episode_file_map` and used temp B-tree sort for `ORDER BY created_ts`.
- After:
  - indexed seek on `episode_file_map` via `idx_episode_file_map_episode_created`.
  - subquery scan/sort removed.

## Validation

Executed and passed:
- `cargo fmt --all`
- `cargo check -p rustfin-db`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server migrations_are_idempotent -- --nocapture`

## Remaining High-Value DB Work (Optional Next Pass)

1. Introduce FTS-backed search for local/online audio (`LIKE '%...%'` currently cannot use B-tree indexes).
2. Add DB-level pagination for large local track catalogs in `list_audio_tracks`.
3. Add lightweight SQL timing instrumentation around high-frequency repo calls for production profiling.
