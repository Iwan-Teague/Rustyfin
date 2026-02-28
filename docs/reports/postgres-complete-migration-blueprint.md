# Rustyfin PostgreSQL -> PostgreSQL Complete Migration Blueprint

Date: 2026-02-26  
Owner: Platform / Backend

## 1) Goal

Migrate Rustyfin from PostgreSQL to PostgreSQL as the primary and only production database, including:

- Backend API (`crates/server`)
- Calendar service (`crates/calendar`)
- TMDB agent (`crates/tmdb-agent`)
- Shared DB layer (`crates/db`)
- Supporting crates that run SQL directly (`crates/scanner`, `crates/metadata`)
- Runtime scripts, compose topology, tests, and documentation

This is a full migration plan, not a partial compatibility patch.

## 2) Current State (Repo Inventory)

### Verified coupling points

- Workspace SQLx is PostgreSQL-only today:
  - `Cargo.toml` has `sqlx = { ..., features = ["runtime-tokio", "postgres"] }`
- Core pool type is PostgreSQL:
  - `crates/db/src/lib.rs` returns `AnyPool`
  - `crates/server/src/state.rs` stores `AnyPool`
  - `crates/calendar/src/main.rs` and `crates/tmdb-agent/src/main.rs` use `AnyPool`
- Migration runner is custom + PostgreSQL-specific:
  - `crates/db/src/migrate.rs`
  - Runs split SQL statements and relies on PRAGMA behavior
- PostgreSQL-specific SQL exists in schema/migrations:
  - `PRAGMA ...`
  - `INSERT OR IGNORE`
  - `json_extract(...)`
  - FTS5 virtual table (`CREATE VIRTUAL TABLE ... USING fts5`)
- Runtime wiring uses file DB path:
  - `RUSTFIN_DATABASE_URL=/config/rustfin.db` in compose/runtime
- Cleanup scripts delete `rustfin.db`, `rustfin.db-wal`, `rustfin.db-shm`.

### Important pre-existing issue to fix first

- `crates/db/migrations/023_watch_party_invite_only_column.sql` exists but is **not included** in the hardcoded migration list in `crates/db/src/migrate.rs`.
- This is a migration drift risk and should be corrected before/during migration framework refactor.

## 3) Recommended Migration Strategy

Use a **phased cutover**:

1. Build Postgres-capable code path.
2. Add Postgres schema/migrations.
3. Bulk copy PostgreSQL data -> Postgres.
4. Freeze writes, run delta sync, cut over all services to Postgres.
5. Keep PostgreSQL snapshot for rollback window.

Avoid long-lived dual-write unless absolutely needed; it adds complexity and drift risk.

## 4) Target Architecture

- Add PostgreSQL service in compose (`postgres:16` or `15`).
- Replace `RUSTFIN_DATABASE_URL` (file path) with DSN-based config:
  - `RUSTFIN_DATABASE_URL=postgres://...`
- Single migration authority:
  - One service or startup phase runs migrations.
  - Other services start with migration disabled flag.
- Optional pgBouncer later (not required for initial cutover).

## 5) Concrete Workstreams

## 5.1 Workspace dependencies and DB abstraction

### Changes

1. Update SQLx features in root `Cargo.toml`:
   - Add `postgres`, `macros`, and migrate support as needed.
2. In `crates/db/src/lib.rs`:
   - Replace PostgreSQL connect options with `PgConnectOptions`/`PgPoolOptions`.
   - Rename input config from `db_path` to `database_url`.
3. Introduce central DB type aliases in `crates/db`:
   - `pub type DbPool = sqlx::PgPool;`
   - Use this in all crates to reduce future churn.

### Notes

- Do this early to force compiler-guided rewrite across all call sites.

## 5.2 Migration framework refactor

### Changes

1. Replace manual migration runner in `crates/db/src/migrate.rs` with SQLx migrator:
   - `sqlx::migrate!` against a Postgres migration directory.
2. Convert existing PostgreSQL migrations into Postgres-safe migrations.
3. Include missing migration 023 logic in the new migration set.
4. Add migration locking strategy:
   - Prefer one migration runner service.
   - Or use PG advisory lock in migrator wrapper.

### Why this matters

- Current runner depends on `PRAGMA` and split-on-semicolon behavior.
- Multi-service startup currently tries migrations from multiple processes.

## 5.3 Schema conversion (PostgreSQL -> PostgreSQL)

Map and update schema patterns:

- `TEXT PRIMARY KEY` -> keep `TEXT` (or move to `UUID` later; not required now)
- `INTEGER` booleans (0/1) -> `BOOLEAN`
- `BLOB` -> `BYTEA`
- JSON blobs currently stored as TEXT:
  - Keep as `TEXT` for fast migration, or move selected columns to `JSONB`
- `INSERT OR IGNORE` -> `INSERT ... ON CONFLICT DO NOTHING`
- `ON CONFLICT ... excluded` remains valid in PG with minor syntax updates
- Replace any `json_extract(...)` with either:
  - real column (preferred for hot paths), or
  - `jsonb ->> ...` if column becomes JSONB

## 5.4 SQL query rewrite across repo

Current SQL is PostgreSQL-style with `?` bind placeholders. PostgreSQL requires `$1`, `$2`, ...

### Required rewrite scope

- `crates/db/src/repo/*.rs` (all repos)
- Direct SQL in:
  - `crates/scanner/src/scan.rs`
  - `crates/metadata/src/merge.rs`
  - `crates/server/src/routes.rs`, `crates/server/src/tmdb_sync.rs`, and any direct `sqlx::query(...)`
  - `crates/calendar/src/main.rs` direct queries
  - `crates/tmdb-agent/src/main.rs` direct queries

### High-friction cases

- Dynamic `IN (...)` placeholder builders (currently build `?, ?, ?` strings):
  - `libraries.rs`, `users.rs`, `channels.rs`, `jobs.rs`, `watch_party.rs`, `channel_transcripts.rs`
  - Replace with either:
    - `WHERE id = ANY($1)` using PG arrays, or
    - SQLx QueryBuilder with generated `$n` placeholders.

## 5.5 FTS migration (critical)

Current online-audio search relies on PostgreSQL FTS5:

- Migration file: `crates/db/migrations/020_online_audio_search_fts.sql`
- Table: `watch_party_online_audio_track_fts`
- Query usage: `crates/db/src/repo/watch_party.rs` (`MATCH`)

### PostgreSQL replacement

Option A (recommended):
- Add searchable `tsvector` expression / generated column
- GIN index on search vector
- Query with `to_tsquery` / `websearch_to_tsquery`

Option B:
- Use `pg_trgm` with `ILIKE` + GIN trigram indexes (simpler ranking, great fuzzy behavior)

### Required decision

- If prefix semantics (`term* AND term*`) must stay exact, implement `to_tsquery` logic accordingly.

## 5.6 Runtime/compose/scripts

### docker-compose

1. Add `postgres` service + persistent volume.
2. Inject `RUSTFIN_DATABASE_URL` into:
   - `rustfin`
   - `rustfin-calendar`
   - `rustfin-tmdb-agent`
3. Remove PostgreSQL file-path assumptions.

### scripts

Update:
- `scripts/start.sh`
- `scripts/stop.sh`
- `scripts/clean_install.sh`
- `scripts/clean_install.ps1`

Concrete updates:
- Add PG health wait (`pg_isready` or TCP + SQL probe).
- Remove direct PostgreSQL file deletion logic.
- On clean install, drop/reset PG volume/schema instead.

## 5.7 Service startup and migration ordering

Current services run migrations on startup. For PG this can race.

Recommended:

- Add env flag `RUSTFIN_RUN_MIGRATIONS=false` for non-authoritative services.
- Let backend run migrations first, or introduce a dedicated migrator container.

## 5.8 Test strategy migration

Current tests depend on PostgreSQL behavior:

- `crates/server/tests/integration.rs` uses `:memory:` DB.
- `tests/lib/harness.sh` exports file-based `RUSTFIN_DATABASE_URL`.

### Replace with

- Ephemeral Postgres for tests (testcontainers or compose test DB).
- Per-test schema namespace or per-test database.
- CI gate:
  - migrations up/down or forward-only validation
  - integration tests against Postgres

## 5.9 Data migration (PostgreSQL -> PG)

## Step-by-step

1. Freeze version and take PostgreSQL snapshot.
2. Create PG schema via new migrations.
3. Export PostgreSQL data in deterministic table order.
4. Import into PG with FK-safe ordering.
5. Run post-import fixups:
   - sequence alignment (if sequences used)
   - materialized search columns/vectors
6. Run row-count and checksum validation per table.
7. Cutover with write freeze + delta sync.

### Validation checklist

- Row counts match per table.
- Critical query snapshots match:
  - users/auth
  - room membership
  - queue state
  - transcripts
  - calendar events
- FTS/search behavior verified.
- App health and smoke flows pass.

## 5.10 Rollback plan

Rollback must be scripted before cutover:

1. Keep PostgreSQL DB snapshot immutable.
2. During cutover, maintain change log of writes applied to PG.
3. If rollback triggered:
   - Stop all services
   - Restore old runtime config to PostgreSQL mode
   - Restart on snapshot

Define hard rollback window (for example 24-72h).

## 6) SQL Dialect Differences You Must Handle

1. Bind placeholders: `?` -> `$n`
2. `INSERT OR IGNORE` -> `ON CONFLICT DO NOTHING`
3. `PRAGMA` statements: remove/replace with PG settings
4. FTS5 + `MATCH`: replace with PG text search
5. Boolean handling: integer flags -> real booleans
6. `json_extract`: convert to column or JSONB operators

## 7) Proposed Execution Order (Concrete)

1. **Prep**
   - Fix migration registry drift (include 023) as a baseline patch.
   - Add migration ADR/doc note.
2. **DB foundation**
   - Switch SQLx workspace features to include Postgres.
   - Introduce `DbPool` alias and swap pool types.
3. **Migrations**
   - Implement PG migration tree and runner.
4. **Repository rewrite**
   - Convert `crates/db/src/repo/*` queries.
5. **Direct SQL rewrite**
   - scanner/metadata/server/calendar/tmdb-agent direct SQL.
6. **FTS replacement**
   - implement PG search indexes + query behavior.
7. **Runtime**
   - compose + scripts + env vars.
8. **Tests**
   - migrate tests to ephemeral PG.
9. **Data migration tooling**
   - export/import/validate scripts.
10. **Cutover**
   - dry run, production run, post-cutover checks.

## 8) Acceptance Criteria (Done Definition)

- No crate imports `sqlx::AnyPool`.
- No PostgreSQL-specific SQL remains in runtime path.
- All migrations are Postgres and applied via unified migrator.
- Compose defaults to Postgres.
- Start/stop/clean scripts support Postgres lifecycle.
- Integration tests run against Postgres and pass.
- Data migration runbook executes successfully end-to-end.

## 9) Risks and Mitigations

1. **Hidden PostgreSQL syntax left behind**
   - Mitigation: grep gates in CI for `AnyPool`, `PRAGMA`, `INSERT OR IGNORE`, `?` placeholders in SQL strings.
2. **Migration race on startup**
   - Mitigation: single migration authority + health dependency ordering.
3. **Search regressions from FTS migration**
   - Mitigation: behavior tests for online track search before cutover.
4. **Data drift during cutover**
   - Mitigation: write freeze + deterministic delta replay.
5. **Performance regressions**
   - Mitigation: baseline and compare p95 for hot endpoints; add missing indexes.

## 10) File-Level Impact Checklist

### Must change

- `/Users/iwanteague/Desktop/Rustyfin/Cargo.toml`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/lib.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/migrate.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations/*` (new PG migration set)
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/*.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/state.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs` (direct SQL)
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs` (direct SQL)
- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/tmdb-agent/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner/src/scan.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/metadata/src/merge.rs`
- `/Users/iwanteague/Desktop/Rustyfin/docker-compose.yml`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/start.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/clean_install.sh`
- `/Users/iwanteague/Desktop/Rustyfin/scripts/clean_install.ps1`
- `/Users/iwanteague/Desktop/Rustyfin/tests/lib/harness.sh`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/tests/integration.rs`
- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`

### Strongly recommended

- Add a dedicated migration utility crate or binary.
- Add SQL lint/check scripts for PG syntax during CI.
- Add migration verification CI job that starts Postgres and runs full app smoke tests.

## 11) Practical Recommendation

Given current coupling, this should be done as a dedicated migration epic branch with strict staging:

- Stage A: DB foundation + migrations + compile
- Stage B: repo/direct-query rewrites + tests
- Stage C: data migration tooling + cutover rehearsal
- Stage D: production cutover

Do not attempt this as opportunistic small PRs; schema and query semantics are too interconnected.

