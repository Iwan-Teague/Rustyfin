# Watch Party Dev Log

## Stage 0 - Prep and baseline
- Read plan docs from `watchparty_impliment/` in required order.
- Verified repo reality:
  - `crates/server/src/routes.rs` (Axum router + `/api/v1/events` SSE)
  - `crates/server/src/state.rs` (AppState)
  - `crates/server/src/auth.rs` (Bearer JWT validation)
  - `ui/src/lib/api.ts` (localStorage token + Authorization header)
  - `crates/db/src/migrate.rs` (hardcoded MIGRATIONS array)
- Branch: `codex/watch-party`
- Baseline commands:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test` ✅
  - `npm --prefix ui run build` ❌ (pre-existing UI dependency issue: invalid/missing eslint/esrecurse type package in local environment)

## Stage 1 - DB migration
- Added migration: `crates/db/migrations/006_watch_party.sql`
  - `watch_party_room` table
  - `watch_party_member` table
  - CHECK constraints for room/member status and role
  - indexes for host queries, room member scans, and invite inbox (`user_id,status`)
- Wired migration into hardcoded migration list:
  - `crates/db/src/migrate.rs` added `006_watch_party`
- Validation commands:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test` ✅

## Stage 2 - DB repository layer
- Added `crates/db/src/repo/watch_party.rs` with typed rows and operations:
  - `WatchPartyRoomRow`, `WatchPartyMemberRow`, `WatchPartyInviteSummary`
  - `NewWatchPartyMember`
  - `create_room_with_members` (transactional room + members insert)
  - `get_room`, `list_members`, `get_member`
  - `upsert_member`, `set_member_status`, `set_room_status`, `touch_member_last_seen`
  - `list_invites_for_user` (inbox projection with item/host info)
- Exported repo module in `crates/db/src/repo/mod.rs`.
- Validation commands:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test` ✅

## Stage 3 - Server scaffolding and router mount
- Enabled Axum WebSocket feature in workspace dependency:
  - `Cargo.toml` (`axum` features now include `ws`)
- Added `watch_party` server module tree:
  - `crates/server/src/watch_party/mod.rs`
  - `crates/server/src/watch_party/router.rs`
  - `crates/server/src/watch_party/handlers.rs`
  - `crates/server/src/watch_party/manager.rs`
  - `crates/server/src/watch_party/protocol.rs`
  - `crates/server/src/watch_party/permissions.rs`
  - `crates/server/src/watch_party/ws.rs`
- Wired module into server:
  - `crates/server/src/lib.rs` exported `watch_party`
  - `crates/server/src/state.rs` added `watch_party: Arc<WatchPartyManager>`
  - `crates/server/src/main.rs` instantiates `WatchPartyManager`
  - `crates/server/src/routes.rs` nests `/api/v1/watch-party`
- Updated test app state constructors in:
  - `crates/server/tests/integration.rs`
- Validation commands:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test` ✅
- Note:
  - Initial `cargo clippy` run needed a one-time dependency fetch (`tokio-tungstenite`) after enabling `axum/ws`.

## Stage 4 - REST endpoints (room + inbox)
- Implemented watch-party REST handlers in:
  - `crates/server/src/watch_party/handlers.rs`
- Endpoints implemented under `/api/v1/watch-party`:
  - `GET /users`
  - `POST /eligible-libraries`
  - `POST /rooms`
  - `GET /rooms/{room_id}`
  - `POST /rooms/{room_id}/join`
  - `POST /rooms/{room_id}/leave`
  - `POST /rooms/{room_id}/end`
  - `GET /invites`
  - `POST /invites/{room_id}/decline`
- Server-side validations and controls added:
  - item must be `movie` or `episode`
  - host and invitees must have access to item library (admins bypass library ACL checks)
  - invite deduplication and max invitee cap
  - policy validation (`default_join_role` restricted to `viewer|controller`)
  - optional room password normalization + Argon2 hashing + join-time verification
  - invite-only room enforcement on room details/join
  - rate limiting for create/join (`ApiError::TooManyRequests`)
  - host-only room end action
- Validation commands:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test` ✅
