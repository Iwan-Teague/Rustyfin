# Rustyfin Test Harness

This directory contains the practical test harness for Rustyfin:

- Rust unit/integration tests
- UI build checks
- E2E suites (Playwright)
- API contract checks

## Run

From repo root:

```bash
./tests/bootstrap.sh
./tests/test-all.sh
```

Run a single suite:

```bash
./tests/run-suite.sh 00_smoke
./tests/run-suite.sh 06_accessibility
```

Debian-host browser smoke:

```bash
./scripts/ci/debian_browser_smoke.sh
```

This host-specific smoke path reuses the Playwright harness but runs it against an isolated PostgreSQL schema on the configured Debian runtime database, so it does not mutate the live runtime state.

## Ports Used By Test Harness

The harness uses isolated defaults to avoid colliding with local dev instances:

- Backend: `127.0.0.1:18096`
- UI: `127.0.0.1:13000`

Override with:

- `RUSTFIN_TEST_BACKEND_PORT`
- `RUSTFIN_TEST_UI_PORT`

## Output

Each run writes to:

- `tests/_runs/<timestamp>/`
  - `logs/`
  - `playwright/`
  - `summary.txt`

## Directory Picker Behavior During E2E

E2E suites set `RUSTFIN_DIRECTORY_PICKER_PATH` to `tests/fixtures/media` so tests can exercise browse/create flows without interactive OS dialogs.

## Scope Reminder

These suites verify real behavior across setup, auth, libraries/scanning, playback, channels/rooms, accessibility, and API contract surfaces.
