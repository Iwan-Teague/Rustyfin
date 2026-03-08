# Rustyfin PostgreSQL Cutover Runbook

Date: 2026-03-07  
Scope: PostgreSQL-only runtime cutover/rollback operations for Rustyfin.

## 1. Preconditions

- PostgreSQL is healthy and reachable for both source and target DSNs.
- Rustfin runtime is configured with PostgreSQL DSNs only:
  - `RUSTFIN_DATABASE_URL=postgresql://...`
- You have a verified PostgreSQL backup/restore path (`pg_dump`/`pg_restore` or physical snapshot tooling).
- Migration authority is enabled only on backend:
  - `rustfin`: `RUSTFIN_RUN_MIGRATIONS=true`
  - side services: `RUSTFIN_RUN_MIGRATIONS=false`

## 2. Dry-run (staging rehearsal)

1. Restore a recent backup into a staging PostgreSQL database/instance.
2. Point Rustyfin at staging:

```bash
RUSTFIN_DATABASE_URL=postgresql://<user>:<pass>@<host>:5432/<staging_db> ./scripts/start.sh --no-build
```

3. Execute smoke checks on staging:
  - auth/login
  - libraries/items listing
  - channel message send/list
  - room creation/join/invite
  - queue/transcript/calendar read/write

## 3. Production cutover

1. Freeze writes (maintenance window).
2. Take immutable backup/snapshot of current production PostgreSQL.
3. Apply your rehearsed PostgreSQL migration/copy procedure to target production DB.
4. Start Rustyfin against target DSN:

```bash
RUSTFIN_DATABASE_URL=postgresql://<user>:<pass>@<host>:5432/<prod_db> ./scripts/start.sh
```

5. Re-run smoke checks listed above.

## 4. Rollback

1. Stop stack:

```bash
./scripts/stop.sh
```

2. Point runtime back to the last known-good PostgreSQL DSN/snapshot target:

```bash
RUSTFIN_DATABASE_URL=postgresql://<user>:<pass>@<host>:5432/<last_known_good_db> ./scripts/start.sh
```

3. Preserve failed-cutover DB state for forensic diffing.

## 5. Test strategy notes

- Integration tests:
  - `RUSTFIN_TEST_DATABASE_URL=postgresql://...`
- Rust integration guardrails:
  - non-test-looking PostgreSQL URLs are rejected unless `RUSTFIN_TEST_DB_ALLOW_ANY=1`
- E2E harness resolution order:
  - `RUSTFIN_TEST_DATABASE_URL` (preferred)
  - then `RUSTFIN_DATABASE_URL`
  - then harness-provided default PostgreSQL URL
