# Rustyfin Efficiency Deep Dive - Fourth Pass (Database Focus)

Date: 2026-02-26

## Scope

This pass targeted high-frequency room/audio query paths that still had avoidable DB overhead:

- Online listen-together search (`watch_party_online_audio_track`).
- Audio state websocket payload assembly (queue metadata lookups).
- Presence/member resolution in websocket state builders.
- Audio track list endpoint load size control (DB-level pagination).

## Implemented Changes

## 1) Online audio FTS search index

Files:

- `crates/db/migrations/020_online_audio_search_fts.sql`
- `crates/db/src/migrate.rs`
- `crates/db/src/repo/watch_party.rs`

Details:

- Added `watch_party_online_audio_track_fts` (FTS5 virtual table).
- Migration rebuilds FTS rows from `watch_party_online_audio_track`.
- `list_online_audio_tracks` now uses FTS `MATCH` for query mode with prefix terms.
- Added pagination support (`LIMIT/OFFSET`) to online list queries.
- Kept ordered result semantics (`created_ts DESC, updated_ts DESC`).
- FTS rows are maintained by repo writes:
  - upsert path updates FTS row.
  - room clear/delete paths purge FTS rows.

## 2) Queue-ID scoped metadata fetches for audio websocket state

Files:

- `crates/db/src/repo/watch_party.rs`
- `crates/server/src/watch_party/ws.rs`

Details:

- Added repo methods:
  - `get_library_tracks_by_item_ids(...)`
  - `list_online_audio_tracks_by_ids(...)`
- `build_audio_state_message` no longer loads all local library tracks and all online tracks for the room.
- It now:
  - parses queue IDs,
  - fetches only required online track rows by queued IDs,
  - resolves legacy IDs,
  - fetches only required local track rows by queued IDs.

This removes full-table reads on frequent websocket state broadcasts.

## 3) Removed `list_users()` full scans from websocket state builders

File:

- `crates/server/src/watch_party/ws.rs`

Details:

- Added shared helper `build_presence_members(...)`.
- Switched room state builders (`video`, `audio`, `youtube`, `web`, `create`) to `list_members_with_usernames(...)`.
- Eliminated repeated "list all users then map by id" pattern on every state build.

## 4) DB-level pagination for audio tracks endpoint

Files:

- `crates/server/src/watch_party/handlers.rs`
- `crates/db/src/repo/watch_party.rs`
- `ui/src/lib/watchPartyApi.ts`

Details:

- Added `limit` and `offset` to `AudioTracksQuery`.
- Added bounded defaults and max caps in handler (`AUDIO_TRACKS_DEFAULT_LIMIT`, `AUDIO_TRACKS_MAX_LIMIT`, `AUDIO_TRACKS_MAX_OFFSET`).
- Propagated pagination to both local and online DB queries.
- Frontend helper now sends `limit` and `offset` query params.

## Query Plan Verification

Validated with temporary PostgreSQL DBs using `EXPLAIN QUERY PLAN`.

Key observations:

- Pre-FTS online search (LIKE path): scanned within `room_id` index path and filtered title/channel text.
- Post-FTS online search:
  - uses room ordering index + FTS virtual table match path.
- Online by-IDs fetch:
  - uses primary key lookup on the track table primary index.
- Local duration subquery continues using `idx_episode_file_map_episode_created` from prior pass.

## Validation Run

Passed:

- `cargo fmt --all`
- `cargo check -p rustfin-db`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server migrations_are_idempotent -- --nocapture`

Not fully validated in this environment:

- `npm --prefix ui run build` failed due missing Next.js SWC binary (`@next/swc-darwin-arm64`) in local environment, not due application type errors.

## Net Outcome

This fourth pass reduces DB and CPU load on the most active room paths by:

- removing broad repeated scans during websocket state generation,
- adding full-text indexed online audio search,
- and enforcing paged DB reads for audio track listing endpoints.
