# Rustyfin Test Suite (macOS-oriented)

This `tests/` directory adds a practical, **user-style** end-to-end (E2E) test suite for Rustyfin, plus smoke/API checks,
Rust/unit checks, and a top-level runner.

It is designed for **macOS (Darwin)** and assumes you run Rustyfin locally from source:
- Backend: `cargo run -p rustfin-server` (listens on `localhost:8096`)
- UI: `npm --prefix ui run dev` (listens on `localhost:3000`)

The test harness itself uses isolated ports by default so it does not collide with your local dev server:
- Test backend: `127.0.0.1:18096`
- Test UI: `127.0.0.1:13000`

You can override these with:
- `RUSTFIN_TEST_BACKEND_PORT`
- `RUSTFIN_TEST_UI_PORT`

## Quick Start

From repo root:

```bash
./tests/bootstrap.sh
./tests/test-all.sh
```

Or run a single suite:

```bash
./tests/run-suite.sh 02_auth
```

## Where results go

Each run creates a timestamped folder in:

- `tests/_runs/<timestamp>/`
  - `logs/` (backend + ui logs)
  - `playwright/` (HTML report, JSON, JUnit)
  - `summary.txt`

## Why the directory-picker tests won't pop UI dialogs

Rustyfin's server supports a non-interactive override:
`RUSTFIN_DIRECTORY_PICKER_PATH=/absolute/path`.

All E2E suites set this to `tests/fixtures/media` so Playwright can click "Browse" without macOS dialogs.

## Notes

- Some tests assert the *desired* behavior described in `docs/reports/Rustyfin_Fixes_Report.md`.
  If the fixes are not implemented yet, you'll see failures — that's the point.
- If the configured test port is already in use, suites fail fast with a clear message.
