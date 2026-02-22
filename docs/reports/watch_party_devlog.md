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
