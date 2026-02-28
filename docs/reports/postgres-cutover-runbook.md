# Rustyfin PostgreSQL Cutover Runbook

Date: 2026-02-27  
Scope: PostgreSQL -> PostgreSQL migration/cutover for Rustyfin runtime.

## 1. Preconditions

- PostgreSQL stack is running and healthy (`postgres` service in compose).
- Rustfin services are configured via `RUSTFIN_DATABASE_URL=postgresql://...`.
- Existing PostgreSQL source DB snapshot is available (for example `/config/rustfin.db` copy).
- Migration authority is enabled for backend (`RUSTFIN_RUN_MIGRATIONS=true`) and disabled for side services by default.

## 2. Dry-run migration

1. Stop app writes (or run on clone/snapshot for rehearsal).
2. Run bulk migration with pgloader wrapper:

```bash
./scripts/db/migrate_postgres_snapshot.sh \
  --postgres /path/to/rustfin.db \
  --postgres-url postgresql://rustfin:rustfin@localhost:5432/rustfin
```

3. Validate row counts:

```bash
./scripts/db/validate_postgres_counts.sh \
  --postgres /path/to/rustfin.db \
  --postgres-url postgresql://rustfin:rustfin@localhost:5432/rustfin
```

## 3. Cutover execution

1. Freeze writes (maintenance window).
2. Take immutable PostgreSQL backup snapshot.
3. Run migration + validation (commands above).
4. Start Rustyfin with Postgres URL:
   - `RUSTFIN_DATABASE_URL=postgresql://... ./scripts/start.sh`
5. Smoke test critical flows:
   - auth/login
   - libraries/items listing
   - channel message send/list
   - room creation/join/invite
   - queue/transcript/calendar read/write

## 4. Rollback

1. Stop stack: `./scripts/stop.sh`
2. Reconfigure runtime to PostgreSQL target:
   - `RUSTFIN_DATABASE_URL` unset
   - set `RUSTFIN_DATABASE_URL=/path/to/postgres-snapshot.db`
3. Restart: `./scripts/start.sh`
4. Keep Postgres snapshot for forensic comparison.

## 5. Test strategy notes

- Integration tests can target PostgreSQL by setting:
  - `RUSTFIN_TEST_DATABASE_URL=postgresql://...`
- For safety, the Rust integration suite rejects non-test-looking PostgreSQL URLs unless:
  - `RUSTFIN_TEST_DB_ALLOW_ANY=1`
- E2E harness respects:
  - `RUSTFIN_TEST_DATABASE_URL` (preferred)
  - then `RUSTFIN_DATABASE_URL`
  - then local PostgreSQL file fallback.
