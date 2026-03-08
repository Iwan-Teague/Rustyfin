# Rustyfin Minor Consistency and Weakness Review

Date: 2026-03-07
Scope: quick whole-project pass for low-risk, high-value cleanup opportunities.

## Summary
The project is in a strong state functionally, but there are a few easy wins around hygiene, consistency, and operational safety. None of these require architectural changes.

## Findings and simple improvements

Status legend:
- `DONE` implemented and validated.
- `PENDING` identified improvement not yet implemented in this pass.

1. Tracked TypeScript build artifact in git (`DONE`)
- Evidence:
  - `ui/tsconfig.tsbuildinfo` is tracked (`git ls-files`).
  - `.gitignore` does not currently ignore `*.tsbuildinfo`.
- Improvement:
  - Add `*.tsbuildinfo` to `.gitignore`.
  - Remove tracked artifact from index (`git rm --cached ui/tsconfig.tsbuildinfo`).
- Why this matters:
  - Reduces noisy diffs and avoids accidental commits of local incremental build state.

2. `.gitignore` duplication and drift (`DONE`)
- Evidence:
  - Duplicate entries for `node_modules/`, `ui/node_modules/`, and `ui/.next/` in `.gitignore`.
- Improvement:
  - Consolidate duplicate lines into a single canonical section.
- Why this matters:
  - Cleaner repo hygiene and less confusion for contributors.

3. UI lint gate is effectively disabled (`DONE`)
- Evidence:
  - `npm --prefix ui run lint` prints: `Skipping UI lint: eslint or config missing`.
  - Script: `ui/scripts/lint-if-config.cjs`.
- Improvement implemented:
  - `ui/scripts/lint-if-config.cjs` now fails hard when config or eslint dependency is missing.
  - Script resolves config/dependencies from `ui` package root and runs `next lint` with explicit `cwd`.
  - Added `eslint-config-next` and aligned `eslint` major version for compatibility.
- Why this matters:
  - Prevents silent frontend regressions and style drift.

4. Postgres runbook has stale/mismatched instructions (`DONE`)
- Evidence:
  - `docs/reports/postgres-cutover-runbook.md` references non-existent scripts:
    - `scripts/db/migrate_postgres_snapshot.sh`
    - `scripts/db/validate_postgres_counts.sh`
  - Same doc has rollback guidance that sets `RUSTFIN_DATABASE_URL=/path/to/postgres-snapshot.db`, which conflicts with postgres-only runtime.
- Improvement:
  - Update/remove stale commands and rewrite rollback section to DSN-only PostgreSQL semantics.
- Why this matters:
  - Prevents operator mistakes during incident response.

5. Internal path disclosure in client-facing error (`DONE`)
- Evidence:
  - `crates/server/src/watch_party/handlers.rs` currently returns canonical file/cache paths in the forbidden error payload from `validate_room_audio_scope`.
- Improvement:
  - Return a generic client error message.
  - Log full path details server-side only (structured warn log).
- Why this matters:
  - Reduces filesystem/path information leakage.

6. Clippy baseline is not clean under strict warnings (`PARTIAL`)
- Evidence:
  - `cargo clippy -p rustfin-server --lib -- -D warnings` fails.
  - Reported categories:
    - `too_many_arguments` (`crates/transcoder/src/session.rs`, `crates/db/src/repo/libraries.rs`)
    - `type_complexity` (`crates/server/src/channels/manager.rs`)
    - `redundant_guards` (`crates/server/src/watch_party/handlers.rs`)
    - `field_reassign_with_default` (multiple in `crates/server/src/watch_party/manager.rs`)
    - `large_enum_variant` (`crates/server/src/watch_party/protocol.rs`)
- Improvement implemented in this pass:
  - Fixed redundant guard in watch-party handlers.
  - Removed repeated `field_reassign_with_default` patterns in watch-party manager by introducing constructor helpers.
  - Verified targeted strict clippy pass:
    - `cargo clippy -p rustfin-server --lib -- -D warnings -A clippy::too_many_arguments -A clippy::type_complexity -A clippy::large_enum_variant`
- Remaining:
  - Workspace-wide strict clippy still has known debt categories outside this narrow pass.
- Why this matters:
  - Improves maintainability and catches regressions earlier.

7. Very large source files in watch-party domain (`PARTIAL`)
- Evidence (line counts):
  - `crates/server/src/watch_party/ws.rs`: 3949
  - `crates/server/src/watch_party/handlers.rs`: 3345
  - `crates/server/src/watch_party/manager.rs`: 2592
  - `scripts/start.sh`: 1574
- Improvement implemented in this pass:
  - Extracted presence cache/mapping logic from `watch_party/ws.rs` into `watch_party/presence.rs` with tests moved accordingly.
- Remaining:
  - Further modular splits still beneficial for `handlers.rs` and `manager.rs`.
- Why this matters:
  - Easier code review, lower merge conflict frequency, better testability.

8. Native binary cache can bloat local build contexts (`PENDING`)
- Evidence:
  - Local workspace size: ~10G.
  - `.native-bins`: ~704M.
- Improvement:
  - Add optional `start.sh` maintenance step to prune stale target/profile output in `.native-bins`.
  - Keep only active target/profile unless explicitly preserving others.
- Why this matters:
  - Faster local Docker context handling and less disk churn.

9. Minor policy/implementation inconsistency in start helper (`PENDING`)
- Evidence:
  - Repo policy is shell-first and Debian-12-centered, but `scripts/start.sh` embeds a Windows PowerShell folder picker branch in Python helper logic.
- Improvement:
  - Either explicitly document this cross-platform helper support, or remove/de-scope Windows path if project support matrix is Linux/macOS only.
- Why this matters:
  - Keeps support expectations and operational code aligned.

## Validation snapshot (this pass)
- `cargo fmt --all -- --check` passed.
- `cargo check -p rustfin-server` passed.
- `cargo test -p rustfin-server --lib` passed.
- Targeted strict clippy command above passed.
- `npm --prefix ui run lint` now runs real lint and fails on existing repo lint violations (expected until lint debt is addressed).

## Suggested next order
1. Continue small clippy cleanup in non-watch-party hotspots.
2. Incrementally split `watch_party/handlers.rs` and `watch_party/manager.rs`.
3. Add optional `.native-bins` prune path to `start.sh`.
4. Decide whether to keep or remove Windows helper branch in startup helper.
