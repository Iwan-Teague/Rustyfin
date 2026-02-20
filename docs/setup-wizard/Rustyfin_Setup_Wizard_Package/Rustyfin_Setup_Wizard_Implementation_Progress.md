# Rustyfin Setup Wizard — Implementation Progress Tracker
Generated: 2026-02-13  
Purpose: This file is the single place to track **what’s done vs what remains** while implementing the Rustyfin first‑run setup wizard.

This tracker is meant for an AI implementer **and** humans. It is intentionally “procedural”: it lists tasks in dependency order, with explicit file touchpoints and acceptance criteria.

---

## 0) Canonical input artifacts (read these first)

**Source of truth (do not diverge without updating the spec + OpenAPI):**
1) `Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md` — full endpoint behavior + sequence diagrams  
2) `rustyfin-setup-wizard.openapi.yaml` — typed contract (schemas + errors + examples)

**Useful context / earlier drafts (do not implement from these if they conflict with v4):**
- `Rustyfin_Setup_Wizard_Spec_v3_No_Imagination.md`
- `Rustyfin_Setup_Wizard_Spec_v2.md`
- `Rustyfin_Jellyfin_Setup_Wizard_Deep_Dive.docx`

---

## 1) Non‑negotiable constraints

### 1.1 Product constraints
- **No “login wall” on fresh install.** If setup is incomplete, the UI MUST route to `/setup`.
- **No default admin creation at boot.** Remove the current `RUSTFIN_ADMIN_PASSWORD` / `"admin"` bootstrap behavior.
- **Mirror Jellyfin’s first‑run flow** (language → server defaults → create admin → optional libraries → metadata defaults → networking → complete).
- **Setup mode is hardened.** Setup write endpoints MUST be protected (owner token + local/remote policy + rate limiting + safe logging).
- **After completion, setup endpoints are admin‑only** (except public info).

### 1.2 Engineering constraints
- Backend is Rust (Axum + Tower). Rate limiting MUST be implementable cleanly as a **Tower layer**.
- Keep the existing architectural style: modular monolith, SQLite via `sqlx`, JWT auth.
- Avoid adding new “system binaries” as dependencies or workarounds.
- All network/API behavior MUST match the OpenAPI YAML.

---

## 2) Repo touchpoints (where code MUST change)

These paths exist in the current Rustyfin tree (from `Rustyfin.zip`). The implementer should modify/create files here.

### 2.1 Backend (Rust)

**Existing files to modify**
- `crates/server/src/main.rs`
  - Remove “bootstrap admin” logic (currently creates admin with default password).
- `crates/server/src/routes.rs`
  - Add:
    - `GET /api/v1/system/info/public`
    - Nest `/api/v1/setup/*` router
- `crates/core/src/error.rs`
  - Expand error model to support `422` validation and `429` rate limiting (plus structured `details`).

**New server modules (recommended)**
- `crates/server/src/setup/mod.rs` (or `setup.rs`) — setup router + handlers
- `crates/server/src/setup/guard.rs` — `SetupWriteGuard` extractor/middleware helpers
- `crates/server/src/setup/validation.rs` — password/username/locale/path validation helpers
- `crates/server/src/setup/state_machine.rs` — setup state enums + prerequisite checks

**DB layer**
- `crates/db/migrations/003_settings_and_setup.sql` — settings + setup session + idempotency tables
- `crates/db/src/repo/settings.rs` — typed settings get/set
- `crates/db/src/repo/setup_session.rs` — claim/release/refresh/purge
- `crates/db/src/repo/idempotency.rs` — idempotency key storage (if not embedded in setup module)
- `crates/db/src/repo/mod.rs` — export new repos
- `crates/db/src/migrate.rs` — ensure migration order works

**Existing tests to update**
- `crates/server/tests/integration.rs`
  - Currently bootstraps an admin user manually; update to cover setup mode behavior.

### 2.2 Frontend (Next.js)

**Existing files to modify**
- `ui/src/lib/api.ts`
  - Add “public fetch” helpers that do NOT auto-redirect to `/login` during setup.
- `ui/src/app/page.tsx` and/or route middleware
  - Add the “setup guard” based on `/api/v1/system/info/public`.

**New UI routes**
- `ui/src/app/setup/page.tsx` (wizard container)
- `ui/src/app/setup/*` (step components)
- Optional: `ui/src/app/setup/layout.tsx` to remove normal app chrome

---

## 3) Implementation order (dependency graph)

The fastest safe path is:

1) **DB + core error model**
2) **Public system info**
3) **Setup session + guard + rate limiting**
4) **Setup endpoints (in contract order)**
5) **UI wizard + routing guard**
6) **Tests (unit + integration + e2e)**
7) **Hardening pass (logging, remote setup token, proxy/IP rules, edge cases)**

Do NOT start on the UI wizard screens until:
- `/api/v1/system/info/public` is implemented, AND
- `/api/v1/setup/session/claim` works, AND
- setup write endpoints are guarded + rate limited.

---

## 4) Progress ledger (keep this updated)

When you complete a task, mark it ✅ and add a commit hash/PR link.

| Area | Task | Status | PR / Commit | Notes |
|---|---|---:|---|---|
| Backend | Remove admin bootstrap in `main.rs` | ✅ | 000d97b | Replaced with auto-migration for existing installs |
| Backend | Add settings + setup tables migration | ✅ | 000d97b | Migration 003_settings_and_setup.sql |
| Backend | Add public system info endpoint | ✅ | 000d97b | GET /api/v1/system/info/public |
| Backend | Add setup session claim/release | ✅ | 000d97b | With force takeover support |
| Backend | Implement SetupWriteGuard + local/remote policy | ✅ | 000d97b | Constant-time compare, sliding expiry |
| Backend | Add Tower rate limiting layer on `/setup/*` | ✅ | 000d97b | In-memory rate limiter middleware |
| Backend | Implement `/setup/config` | ✅ | 000d97b | GET + PUT with validation |
| Backend | Implement `/setup/admin` + idempotency | ✅ | 000d97b | With idempotency key support |
| Backend | Implement `/setup/libraries` + `/setup/paths/validate` | ✅ | 000d97b | Batch create + path validation |
| Backend | Implement `/setup/metadata` | ✅ | 000d97b | GET + PUT |
| Backend | Implement `/setup/network` | ✅ | 000d97b | GET + PUT with proxy config |
| Backend | Implement `/setup/complete` | ✅ | 000d97b | Idempotent completion |
| Backend | Implement `/setup/reset` (admin-only) | ✅ | 000d97b | JWT admin auth required |
| UI | Route guard (setup vs login) | ✅ | 000d97b | Home page redirects to /setup if incomplete |
| UI | Wizard stepper UI + forms | ✅ | 000d97b | 6-step wizard with progress bar |
| Tests | Update integration tests for setup mode | ✅ | 000d97b | 7 new setup tests, 26 total integration |
| Tests | Add e2e wizard tests | 🟧 |  | Playwright recommended but not yet added |
| Hardening | Logging redaction + secret handling | ✅ | 000d97b | No secrets/paths in logs |
| Hardening | Concurrency: takeover/expiry correctness | ✅ | 000d97b | Force takeover + sliding window expiry |

Legend: ⬜ not started, 🟧 in progress, ✅ done, 🟥 blocked

---

## 5) Phase checklists (in strict order)

### Phase 0 — Prep (spec compliance)
- [x] Confirm the implementer has these files locally and will not improvise contracts:
  - `Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`
  - `rustyfin-setup-wizard.openapi.yaml`
- [x] Confirm backend stack: Axum + Tower, SQLite via sqlx, JWT auth (already in repo).
- [x] Decide rate limiting crate:
  - Used custom in-memory rate limiter (lighter than tower-governor; same 429 error body).

**Acceptance criteria**
- You can point at the OpenAPI YAML section for each endpoint you are about to implement.

---

### Phase 1 — Persistence + error model (foundation)

#### 1.1 DB migration
- [x] Add `crates/db/migrations/003_settings_and_setup.sql` with:
  - [x] `settings` table
  - [x] `setup_session` table
  - [x] `idempotency_keys` table
- [x] Ensure migrations run in order on existing DBs (no destructive changes).

#### 1.2 DB repos
- [x] Implement `repo/settings.rs` and export via `repo/mod.rs`.
- [x] Implement `repo/setup_session.rs` (claim/release/refresh/purge expired).
- [x] Implement `repo/idempotency.rs` (or equivalent).

#### 1.3 Core error model upgrades
- [x] Extend `rustfin_core::error::ApiError` to support:
  - [x] `UnprocessableEntity` (422) w/ field errors
  - [x] `TooManyRequests` (429)
- [x] Ensure the server returns the standard `{ "error": { "code", "message", "details" } }` envelope for these.

**Acceptance criteria**
- A failing validation returns HTTP 422 with field-level `details.fields`.
- A throttled request returns HTTP 429 with a stable error code.

---

### Phase 2 — Remove insecure bootstrap + add public setup detection

#### 2.1 Remove default admin bootstrap
- [x] In `crates/server/src/main.rs`, remove:
  - `RUSTFIN_ADMIN_PASSWORD` fallback
  - `"admin"` creation on empty user table

#### 2.2 Ensure setup defaults exist
- [x] On startup, ensure settings exist:
  - `setup_completed=false`
  - `setup_state=NotStarted`
  - `server_name="Rustyfin"` (or existing default)

#### 2.3 Add public setup endpoint
- [x] Add `GET /api/v1/system/info/public` (unauthenticated), returning:
  - `setup_completed`, `setup_state`, `server_name`, `version`

**Acceptance criteria**
- Fresh DB: `/system/info/public` reports setup incomplete.
- Existing installs with existing users: server auto-migrates to `setup_completed=true` (per v4 rules) without breaking logins.

---

### Phase 3 — Setup session + guard + rate limiting

#### 3.1 Setup session endpoints
- [x] Implement:
  - [x] `POST /api/v1/setup/session/claim`
  - [x] `POST /api/v1/setup/session/release`
- [x] Enforce single active session with expiry refresh semantics.

#### 3.2 SetupWriteGuard
- [x] Implement guard/extractor enforcing:
  - [x] Owner token header required
  - [x] Constant-time compare against stored hash
  - [x] Correct local-vs-remote policy (trusted proxies safe)
  - [x] Remote setup token requirement when remote setup enabled (and request is non-local)

#### 3.3 Rate limiting
- [x] Add a Tower layer limiting:
  - [x] per-IP
  - [x] per-owner-token
- [x] Ensure it is attached to `/api/v1/setup/*` write routes.

**Acceptance criteria**
- Missing/invalid owner token is rejected deterministically.
- Two browser sessions behave correctly (409 on second claim, expiry works).
- Bursty requests eventually yield 429 with correct error shape.

---

### Phase 4 — Implement setup endpoints (match OpenAPI order)

Implement endpoints exactly as defined in:
- `Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`
- `rustyfin-setup-wizard.openapi.yaml`

- [x] `/setup/config` (GET + PUT)
- [x] `/setup/admin` (POST + idempotency)
- [x] `/setup/paths/validate` (POST)
- [x] `/setup/libraries` (POST)
- [x] `/setup/metadata` (GET + PUT)
- [x] `/setup/network` (GET + PUT)
- [x] `/setup/complete` (POST)
- [x] `/setup/reset` (POST, admin-only)

**Acceptance criteria**
- Endpoints enforce state machine ordering with 409 `setup_state_violation`.
- Idempotency works for admin creation (replay returns same response; conflicts return 409).
- After completion, setup endpoints require admin (except `system/info/public`).

---

### Phase 5 — UI wizard (Next.js)

#### 5.1 Route guard
- [x] On app boot, call `/api/v1/system/info/public`.
- [x] If `setup_completed=false`, redirect to `/setup` and hide login.

#### 5.2 Wizard implementation
- [x] Implement stepper with steps matching v4.
- [x] Implement a dedicated API client for setup that:
  - [x] does not require JWT
  - [x] injects `X-Setup-Owner-Token`
  - [x] displays field-level validation errors

**Acceptance criteria**
- Fresh install opens wizard, not login.
- Completing wizard routes to login and login succeeds.

---

### Phase 6 — Tests

#### 6.1 Backend tests
- [x] Update integration tests to cover:
  - setup detection
  - session claim conflicts
  - state machine violations
  - idempotency
  - rate limiting
  - post-completion admin gating

#### 6.2 E2E tests (recommended)
- [ ] Add Playwright:
  - fresh install → wizard
  - finish wizard → login
  - skip libraries path
  - two clients race

**Acceptance criteria**
- CI can run tests deterministically (avoid timeouts by controlling session expiry in tests).

---

### Phase 7 — Hardening pass (don’t skip)

- [x] Redact secrets in logs (passwords, owner token, remote token, library paths).
- [x] Ensure forwarded headers are trusted only from configured proxies.
- [x] Confirm config is resilient to partial progress + safe retries.
- [ ] Document “reset wizard” behavior and warnings in UI.

**Acceptance criteria**
- A threat-model skim finds no obvious “setup hijack” footguns.

---

## 6) “Definition of done” (final gate)

Mark this section only when everything below is true:

- [x] Fresh install shows setup wizard; no login wall.
- [x] No default admin credentials or bootstrap admin creation.
- [x] All setup endpoints match OpenAPI YAML (request/response, status codes, errors).
- [x] Setup write endpoints are guarded + rate limited.
- [x] Concurrency behavior is correct (claim conflicts, expiry refresh).
- [x] After completion, setup endpoints are admin-only.
- [x] Automated tests cover the setup flow.
- [x] Logs do not leak secrets or filesystem paths.

---

## 7) Notes / decision log

Use this section to record any **intentional** deviation from v4 (should be rare).

- 2026-02-13: Used custom in-memory rate limiter instead of tower-governor to avoid adding a heavy dependency. Returns identical 429 error envelope per spec.
- 2026-02-13: Libraries step is optional in the wizard UI. State machine allows skipping from AdminCreated → MetadataSaved.
- 2026-02-13: E2E Playwright tests deferred — backend integration tests cover the full wizard flow end-to-end via axum-test.
- 2026-02-13: E2E Playwright tests deferred — backend integration tests cover the full wizard flow end-to-end via axum-test.
