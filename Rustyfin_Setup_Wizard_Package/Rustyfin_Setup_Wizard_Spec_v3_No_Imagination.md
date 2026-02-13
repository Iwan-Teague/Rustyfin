# Rustyfin Setup Wizard Specification — v3 (Nothing Left to the Imagination)

**Status:** Implementation-ready spec (backend + UI + tests)  
**Target:** Replace “login wall + default admin” first-run behavior with a secure, Jellyfin-mirroring setup wizard.  
**Audience:** A developer new to this repo should be able to implement without inventing behavior.

> This v3 spec is intentionally “over-specified.” When in doubt, it chooses **deterministic, testable rules** over “whatever feels right.”

---

## 0) Ground truth: what the current repo does today

This spec is written to fit the actual Rustyfin workspace structure and dependencies:

- Backend: **Axum 0.8 + Tower + tower-http** (CORS/trace available)
- DB: **sqlx + SQLite**
- Auth: **JWT** + **Argon2** password hashing
- Current first-run behavior (must be removed):
  - `crates/server/src/main.rs` **auto-creates an `admin` user** on empty DB, with password from `RUSTFIN_ADMIN_PASSWORD` or default `"admin"`.
  - UI is forced into login immediately because a user exists (or because endpoints require auth).

This v3 spec replaces that admin bootstrap with a wizard-driven bootstrap.

---

## 1) Normative language

This document uses RFC-style requirement keywords:

- **MUST / MUST NOT**: required for correctness/security
- **SHOULD / SHOULD NOT**: strong recommendation; deviation needs an explicit reason
- **MAY**: optional

---

## 2) Goals and non-goals

### 2.1 Goals

1) **No first-run login wall**
- If setup is incomplete, the UI MUST show setup wizard instead of login.

2) **No default credentials**
- The server MUST NOT create a default admin user automatically.

3) **Mirror Jellyfin’s conceptual wizard**
- Steps MUST include:
  - (client) wizard display language
  - (server) server name + locale defaults
  - create initial admin user
  - (optional) media libraries
  - metadata language + region
  - networking defaults (remote access toggle + port mapping toggle)
  - complete + redirect to normal login

4) **Setup mode is secure-by-default**
- Setup endpoints are a high-risk surface. The design MUST harden:
  - ownership gating
  - local-network vs remote restrictions
  - rate limiting
  - concurrency (two clients racing)
  - secrets handling in logs

5) **Implementation-grade**
- Every endpoint MUST have concrete request/response shapes, validation rules, status codes, and idempotency semantics.

### 2.2 Non-goals

- OAuth/SSO onboarding
- Hosted SaaS multi-tenant setup
- Full admin dashboard configuration (GPU, providers, transcoding tuning, etc.)
- Solving every reverse proxy nuance in the wizard UI (we provide safe defaults + “Advanced”)

---

## 3) Terminology

- **Setup mode**: `setup_completed = false`.
- **Wizard**: the UI workflow that takes server from setup mode to normal mode.
- **Setup write endpoints**: endpoints allowed before an admin exists (but still guarded).
- **Setup session**: concurrency guard preventing multiple clients from writing setup simultaneously.
- **Owner token**: opaque token proving “this client owns the setup session.”
- **Remote setup token**: optional one-time token used to authorize setup from non-local networks (headless/VPS scenario).
- **Local request**: request originating from loopback or private LAN ranges (precisely defined later).

---

## 4) Product behavior overview (user-facing)

### 4.1 Startup routing (the “no-login-wall” rule)

**Backend MUST expose** `GET /api/v1/system/info/public` unauthenticated.

Response MUST include:
- `setup_completed` (bool)
- `setup_state` (enum)
- `server_name` (string, or default)
- `version` (string)

**Frontend MUST call** this endpoint before showing login.

Routing rules:
- If `setup_completed=false`: UI MUST route to `/setup` and MUST NOT show login.
- If `setup_completed=true`: normal auth flow.

### 4.2 Wizard steps (Jellyfin-mirroring)

Wizard is a stepper. All steps MUST:
- show progress (“Step X of Y”)
- support Back
- preserve entered values (client memory; server persistence when specified)
- show inline validation
- show clear system status during network calls

#### Step 0 — Claim setup session (concurrency guard)
Purpose: prevent two browsers from racing; give a predictable “owner”.

- UI calls `POST /api/v1/setup/session/claim`
- If already claimed, UI shows:
  - who claimed (client name)
  - when it expires
  - options: wait / refresh / force takeover (strictly defined later)

#### Step 1 — Wizard display language (client-scoped)
- This affects wizard UI only.
- Stored in local storage (or equivalent) and later copied into user preferences after login.

#### Step 2 — Server identity & locale defaults (server-scoped)
- `server_name`
- `default_ui_locale` (BCP-47 string like `en-IE`)
- `default_region` (ISO 3166-1 alpha-2 like `IE`)
- `default_time_zone` (IANA tz name like `Europe/Dublin`) — OPTIONAL field; can be hidden behind “Advanced”.

#### Step 3 — Create initial admin user
- `username`
- `password`
- `confirm_password`

Wizard MUST not complete without a valid admin.

#### Step 4 — Libraries (optional, skippable)
- Add 0+ libraries (name, kind, paths, read-only)
- Support “Validate path” for each path.

#### Step 5 — Preferred metadata language + region (server-scoped defaults)
- `metadata_language` (e.g. `en`, `fr`, optionally BCP-47)
- `metadata_region` (ISO 3166-1 alpha-2)

#### Step 6 — Networking defaults
- `allow_remote_access`
- `enable_automatic_port_mapping` (UPnP/NAT-PMP): default **false**
- `trusted_proxies` (advanced)

#### Step 7 — Finish
- `POST /api/v1/setup/complete`
- UI then routes to login. (Auto-login is optional and must be secured by one-time token if implemented.)

---

## 5) Wizard state machine (server-side)

### 5.1 Persisted state

Server MUST persist these keys (in DB settings table, see §9):

- `setup_completed` (bool)
- `setup_state` (enum string)

`SetupState` enum MUST be:

- `NotStarted`
- `SessionClaimed`
- `ServerConfigSaved`
- `AdminCreated`
- `LibrariesSaved` *(optional; may be skipped)*
- `MetadataSaved`
- `NetworkSaved`
- `Completed`

### 5.2 Monotonic transitions

State MUST be monotonic (only forward) except via explicit reset.

Transition prerequisites:

- Claim session → `SessionClaimed`
- Save server config requires `SessionClaimed`
- Create admin requires `ServerConfigSaved`
- Libraries step requires `AdminCreated` (because libraries should be created as admin or as a special setup-only path; we choose admin because it’s safer and matches existing auth direction)
- Save metadata requires `AdminCreated` (libraries optional)
- Save network requires `AdminCreated`
- Complete requires `AdminCreated` AND `MetadataSaved` AND `NetworkSaved`
  - If libraries were skipped, treat `LibrariesSaved` as satisfied.

### 5.3 Ordering enforcement

If an endpoint is called out-of-order, server MUST return:

- HTTP **409**
- error code: `setup_state_violation`
- details: include `expected_min_state` and `current_state`

### 5.4 Idempotency rules

- Config-like steps MUST be idempotent and use `PUT`:
  - `PUT /setup/config`
  - `PUT /setup/metadata`
  - `PUT /setup/network`

- Create-only steps:
  - `POST /setup/admin` MUST be idempotent via `Idempotency-Key` header.
  - If admin already exists (and the same Idempotency-Key was not used), respond 409 `admin_already_exists`.

- `POST /setup/complete` MUST be idempotent:
  - If already completed, return 200 with `{ setup_completed: true }` and do not mutate anything.

---

## 6) Security model (setup mode hardening)

### 6.1 Setup gating policy

- If `setup_completed=false`:
  - Setup **read** endpoints MAY be unauthenticated (but must not leak secrets)
  - Setup **write** endpoints MUST pass `SetupWriteGuard` (defined below)

- If `setup_completed=true`:
  - Setup endpoints MUST require authenticated **admin** (same as Jellyfin behavior).
  - Public info endpoint stays public.

### 6.2 SetupWriteGuard (the non-negotiable gate)

A request to a setup write endpoint MUST satisfy ALL:

1) **Setup session ownership**
- Request MUST include:
  - `X-Setup-Owner-Token: <token>`
- Server MUST verify token matches the active setup session (constant-time comparison of hashes).

2) **Rate limiting**
- Per-IP and per-owner-token throttling MUST be applied.

3) **Origin safety**
- If auth is cookie-based, CSRF MUST be enforced.
- If auth is bearer-header-only (current Rustyfin), server MUST enforce strict CORS for browser origins.

4) **Network safety**
- If request is non-local:
  - MUST also include `X-Setup-Remote-Token: <token>` unless remote setup is explicitly enabled.
- Default posture: remote setup is **denied** unless the operator opts in.

### 6.3 Local-network definition (precise)

A request is “local” if the effective client IP is in:

- IPv4 loopback: `127.0.0.0/8`
- IPv4 private: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- IPv4 link-local: `169.254.0.0/16` (optional; recommended to treat as local)
- IPv6 loopback: `::1`
- IPv6 unique local: `fc00::/7`
- IPv6 link-local: `fe80::/10` (optional; recommended to treat as local)

**Trusted proxy handling rule:**
- Server MUST NOT trust `X-Forwarded-For` / `Forwarded` headers unless the direct peer IP is in a configured trusted proxy range.
- If you don’t have trusted proxies configured, ignore forwarded headers entirely.

### 6.4 Remote setup token (headless/VPS installs)

Remote setup is a common UX need, but a giant security hole if done lazily.

Policy:
- Remote setup MUST be disabled by default.
- Enabling remote setup MUST require an explicit operator action:
  - env var `RUSTFIN_REMOTE_SETUP=1` OR config file `remote_setup_enabled=true`

Token generation:
- On startup, if `setup_completed=false` and `remote_setup_enabled=true`:
  - Generate a random token (32 bytes, base64url) if one does not exist
  - Store only a hash of it on disk (or in DB settings)
  - Print a one-time “Remote setup token: …” message to logs ONCE per boot (never again)
  - Provide a file location operator can read (e.g. `data_dir/remote-setup-token.txt`) — optional, but extremely useful

Token usage:
- Non-local setup write requests MUST present `X-Setup-Remote-Token`.
- Token MUST be rate limited separately.
- Token MUST be invalidated automatically when setup completes.

### 6.5 Logging rules

- MUST NOT log:
  - passwords
  - owner tokens
  - remote setup tokens
  - library filesystem paths (paths can leak local system structure)
- SHOULD log:
  - setup lifecycle events (claimed, config saved, admin created, completed)
  - request IDs (from tracing)
  - minimal metadata (client_name, state transitions)

---

## 7) Engineering architecture (how this fits into the existing Rustyfin crates)

### 7.1 Where the new code goes (backend)

**crates/db**
- Add migrations:
  - `003_settings_and_setup.sql`
- Add `repo/settings.rs`
- Add `repo/setup_session.rs` (or combine with settings; recommended separate)

**crates/server**
- Add module: `setup.rs` (router + handlers + middleware)
- Update `routes.rs` to nest `/api/v1/setup` routes
- Update `main.rs`:
  - remove default-admin bootstrap
  - load setup settings at boot (or lazy-load on request)
  - configure CORS + rate limiting layers
  - decide bind address semantics (see §12.4)

**crates/core**
- Extend `ApiError` to support:
  - `UnprocessableEntity` (422)
  - `TooManyRequests` (429)
  - optionally, structured “details” fields per error
- This is necessary because setup needs “validation” errors and rate limiting.

### 7.2 Single source of truth: SettingsService

Implement a `SettingsService` in server crate that wraps DB operations:

Responsibilities:
- read/write `settings` keys
- parse JSON values into typed structs
- enforce that setup state transitions are atomic (use DB transactions)
- expose cached read for `system/info/public` (short TTL in memory is OK)

---

## 8) Backend API specification (OpenAPI-style, exact)

All endpoints are under `/api/v1`.

### 8.1 Error envelope

Use the existing Rustyfin envelope shape, but expand codes/statuses.

Response MUST be:

```json
{
  "error": {
    "code": "string",
    "message": "string",
    "details": {}
  }
}
```

Status code mapping MUST be:
- 400: malformed JSON / missing required fields (parsing-level)
- 401: missing/invalid auth token (post-setup) OR missing setup owner token (during setup)
- 403: forbidden (e.g. non-admin after setup; remote setup denied)
- 409: conflict (state violation, session claimed, admin already exists)
- 422: validation errors
- 429: rate limit triggered
- 500: unexpected error

Validation errors (422) MUST include `details` with field-level errors:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "validation failed",
    "details": {
      "fields": {
        "password": ["must be at least 12 characters"],
        "username": ["must match ^[a-zA-Z0-9._-]{3,32}$"]
      }
    }
  }
}
```

### 8.2 Public system info

#### GET /system/info/public

**Auth:** none

Response 200:

```json
{
  "server_name": "Rustyfin",
  "version": "0.1.0",
  "setup_completed": false,
  "setup_state": "NotStarted"
}
```

Rules:
- MUST not include secrets (jwt secret, paths, user counts, etc.)
- MUST be cheap; should not do heavy DB work beyond simple settings lookup.

---

## 8.3 Setup session (concurrency guard)

#### POST /setup/session/claim

Request:

```json
{ "client_name": "WebUI (Chrome on iPad)", "force": false }
```

Response 200:

```json
{
  "owner_token": "opaque-setup-owner-token",
  "expires_at": "2026-02-13T17:10:00Z",
  "claimed_by": "WebUI (Chrome on iPad)",
  "setup_state": "SessionClaimed"
}
```

Response 409 (already claimed):

```json
{
  "error": {
    "code": "setup_claimed",
    "message": "Setup is currently being configured.",
    "details": {
      "claimed_by": "WebUI (Firefox)",
      "expires_at": "2026-02-13T17:10:00Z"
    }
  }
}
```

Rules:
- Claim MUST create or refresh a row in `setup_session`.
- `expires_at` MUST be now + 30 minutes.
- Any successful write endpoint call MUST refresh the expiry to extend by 30 minutes.
- If `force=true`:
  - If setup not completed: MUST be local request AND UI must send `confirm_takeover=true` to avoid accidental clicks.
  - If setup completed: MUST require admin auth (and should be used only for “reset wizard”).

#### POST /setup/session/release

Headers:
- `X-Setup-Owner-Token`

Response 200:

```json
{ "released": true }
```

Rules:
- If session already expired or missing, return 200 `{ released: true }` (idempotent).

---

## 8.4 Server config

#### GET /setup/config

Headers:
- `X-Setup-Owner-Token` (optional; recommended because it reduces information leak)

Response 200:

```json
{
  "server_name": "Rustyfin (Basement NAS)",
  "default_ui_locale": "en-IE",
  "default_region": "IE",
  "default_time_zone": "Europe/Dublin"
}
```

#### PUT /setup/config

Headers:
- `X-Setup-Owner-Token` (required)

Request:

```json
{
  "server_name": "Rustyfin (Basement NAS)",
  "default_ui_locale": "en-IE",
  "default_region": "IE",
  "default_time_zone": "Europe/Dublin"
}
```

Response 200:

```json
{ "ok": true, "setup_state": "ServerConfigSaved" }
```

Validation:
- `server_name` length 1–64, trimmed, must not contain control characters
- `default_ui_locale` must match BCP-47 syntax (basic validator acceptable)
- `default_region` must be `[A-Z]{2}`
- `default_time_zone` optional; if present must match `Area/City` pattern and be in allowlist (see §11.3)

State prerequisites:
- requires `SessionClaimed`

---

## 8.5 Admin creation

#### POST /setup/admin

Headers:
- `X-Setup-Owner-Token` (required)
- `Idempotency-Key` (required; UUID strongly recommended)

Request:

```json
{ "username": "iwan", "password": "a-very-long-passphrase" }
```

Response 201:

```json
{ "user_id": "c0a8012e-...", "setup_state": "AdminCreated" }
```

Errors:
- 409 `admin_already_exists` if any admin user exists
- 409 `idempotency_conflict` if the same Idempotency-Key was used with a different payload
- 422 validation errors

Validation:
- username:
  - trimmed
  - regex `^[a-zA-Z0-9._-]{3,32}$`
  - case-insensitive uniqueness recommended (enforce in DB if possible)
- password:
  - minimum length 12
  - maximum length MUST be at least 128
  - MUST reject blank/whitespace-only
  - SHOULD reject a small denylist of known terrible passwords (`password`, `admin`, `123456`, etc.)
- Password MUST be hashed with Argon2 (reuse existing `rustfin_db::repo::users::create_user`).

State prerequisites:
- requires `ServerConfigSaved`

Side effects:
- MUST create user with role `"admin"`.
- SHOULD also create default preferences row (already done by db layer).
- SHOULD write `setup_state=AdminCreated` in same DB transaction.

---

## 8.6 Libraries (optional)

You have two viable designs:

**Design A (recommended for v1):** Reuse existing authenticated `/libraries` endpoints, but after admin exists.
- Setup wizard creates admin first, then logs in, then uses standard library API.
- Pros: no duplicate “library create” logic
- Cons: wizard becomes “create admin → login → continue wizard” (slightly less smooth)

**Design B (recommended for best UX):** Provide setup-only batch endpoint `POST /setup/libraries`, guarded by SetupWriteGuard.
- Pros: smoother wizard, mirrors Jellyfin better
- Cons: duplicates some creation logic

This v3 spec assumes Design B (max clarity, fewer UI transitions).

#### POST /setup/libraries

Headers:
- `X-Setup-Owner-Token` (required)

Request:

```json
{
  "libraries": [
    { "name": "Movies", "kind": "movie", "paths": ["/media/movies"], "is_read_only": true },
    { "name": "Shows", "kind": "show", "paths": ["/media/shows"], "is_read_only": true }
  ]
}
```

Response 200:

```json
{
  "created": 2,
  "libraries": [
    { "id": "lib_...", "name": "Movies" },
    { "id": "lib_...", "name": "Shows" }
  ],
  "setup_state": "LibrariesSaved"
}
```

Validation:
- name: 1–64
- kind: enum (`movie`, `show`, `music`, `mixed`)
- paths:
  - 1+ paths
  - canonicalize
  - reject traversal and empty
  - reject dangerous pseudo roots by default: `/proc`, `/sys`, `/dev`, etc.
  - existence/readability check is REQUIRED (use OS checks)
- is_read_only defaults true

State prerequisites:
- requires `AdminCreated` (admin exists; you still use setup guard, not JWT)

#### POST /setup/paths/validate

Request:

```json
{ "path": "/media/movies" }
```

Response 200:

```json
{ "path": "/media/movies", "exists": true, "readable": true, "writable": false }
```

Rules:
- MUST canonicalize and reject invalid paths.
- MUST return actionable details:
  - if not readable, include hint: “If using Docker, mount host folder into container (e.g. -v /host/movies:/media/movies).”

---

## 8.7 Metadata defaults

#### GET /setup/metadata
#### PUT /setup/metadata

Request:

```json
{ "metadata_language": "en", "metadata_region": "IE" }
```

Response 200:

```json
{ "ok": true, "setup_state": "MetadataSaved" }
```

Validation:
- metadata_language: `^[a-zA-Z]{2,8}(-[a-zA-Z0-9]{1,8})*$` (relaxed BCP-47 ok)
- metadata_region: `[A-Z]{2}`

State prerequisites:
- requires `AdminCreated`

---

## 8.8 Networking

#### GET /setup/network
#### PUT /setup/network

Request:

```json
{
  "allow_remote_access": true,
  "enable_automatic_port_mapping": false,
  "trusted_proxies": []
}
```

Response 200:

```json
{ "ok": true, "setup_state": "NetworkSaved" }
```

Rules:
- `enable_automatic_port_mapping` MUST default false.
- If `allow_remote_access=true`:
  - UI MUST show warning about WAN exposure
  - server MUST store this setting; effect on bind address is defined in §12.4

Trusted proxies:
- list of CIDR strings
- MUST be validated strictly (reject invalid CIDR)
- default empty

State prerequisites:
- requires `AdminCreated`

---

## 8.9 Completion

#### POST /setup/complete

Request:

```json
{ "confirm": true }
```

Response 200:

```json
{ "setup_completed": true, "setup_state": "Completed" }
```

Rules:
- MUST set `setup_completed=true` and `setup_state=Completed` in one transaction
- MUST invalidate remote setup token if one exists
- MUST delete active setup sessions

---

## 8.10 Reset (admin-only)

#### POST /setup/reset

**Auth:** Admin JWT required  
Request:

```json
{ "confirm": "RESET", "delete_users": false, "delete_settings": true }
```

Rules:
- MUST require admin auth.
- MUST require exact confirmation string `"RESET"`.
- MUST NOT delete libraries/media/items unless explicitly requested.
- Default behavior SHOULD be:
  - reset wizard settings only
  - keep users (so you don’t lock yourself out)

---

## 9) Persistence model (SQLite + sqlx)

### 9.1 Migration: `003_settings_and_setup.sql`

Add tables:

```sql
-- Global settings (typed via JSON).
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_ts INTEGER NOT NULL
);

-- Concurrency control: one active setup session owner at a time.
CREATE TABLE IF NOT EXISTS setup_session (
  id TEXT PRIMARY KEY,
  owner_token_hash TEXT NOT NULL,
  client_name TEXT,
  created_ts INTEGER NOT NULL,
  expires_ts INTEGER NOT NULL,
  updated_ts INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_setup_session_expires ON setup_session(expires_ts);

-- Idempotency tracking (for admin create and other create-only setup endpoints).
CREATE TABLE IF NOT EXISTS idempotency_keys (
  key TEXT PRIMARY KEY,
  endpoint TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  response_json TEXT NOT NULL,
  created_ts INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_idempotency_endpoint ON idempotency_keys(endpoint);
```

### 9.2 Settings keys (exact)

Required keys:

- `setup_completed` → JSON boolean
- `setup_state` → JSON string enum
- `server_name` → JSON string
- `default_ui_locale` → JSON string
- `default_region` → JSON string
- `default_time_zone` → JSON string or null
- `metadata_language` → JSON string
- `metadata_region` → JSON string
- `allow_remote_access` → JSON boolean
- `enable_automatic_port_mapping` → JSON boolean
- `trusted_proxies` → JSON array of strings
- `remote_setup_enabled` → JSON boolean (optional)
- `remote_setup_token_hash` → JSON string (optional)

### 9.3 Repository layer (db crate)

Add `repo/settings.rs`:

- `get_setting(pool, key) -> Option<String>`
- `set_setting(pool, key, value_json)`
- `get_bool/get_string/get_json<T>()` convenience helpers (optional)

Add `repo/setup_session.rs`:

- `get_active_session() -> Option<SessionRow>` (must ignore expired rows)
- `claim_session(client_name, force) -> (owner_token, expires_ts)` (transaction + uniqueness)
- `refresh_session(owner_token_hash)`
- `release_session(owner_token_hash)`
- `purge_expired_sessions()`

### 9.4 Transactions (critical detail)

These operations MUST be atomic:

- `PUT /setup/config`: update settings + state in one txn
- `POST /setup/admin`: create user + record idempotency + update state in one txn
- `POST /setup/complete`: set completed + delete sessions + invalidate remote token in one txn

Use `sqlx::Transaction<'_, sqlx::Sqlite>`.

---

## 10) Backend implementation blueprint (exact code-level plan)

### 10.1 Add router under `/api/v1/setup`

In `crates/server/src/routes.rs`, add:

- `.nest("/api/v1/setup", crate::setup::router())`

And add `mod setup;` in `lib.rs`.

### 10.2 Add `setup.rs` (layout)

`crates/server/src/setup.rs` MUST contain:

- `pub fn router() -> Router<AppState>`
- request/response DTO structs (serde)
- handler fns
- guard extractor(s) for SetupWriteGuard
- helper functions:
  - state prerequisite check
  - validation helpers (username/password/locale/path)

Recommended internal structure:

```rust
// setup.rs
pub fn router() -> Router<AppState> {
    Router::new()
      .route("/session/claim", post(claim_session))
      .route("/session/release", post(release_session))
      .route("/config", get(get_config).put(put_config))
      .route("/admin", post(post_admin))
      .route("/paths/validate", post(validate_path))
      .route("/libraries", post(post_libraries))
      .route("/metadata", get(get_metadata).put(put_metadata))
      .route("/network", get(get_network).put(put_network))
      .route("/complete", post(post_complete))
      .route("/reset", post(post_reset))
}
```

### 10.3 SetupWriteGuard extractor (exact behavior)

Implement as a custom extractor similar to `AuthUser`/`AdminUser`.

Inputs:
- `X-Setup-Owner-Token`
- optional: `X-Setup-Remote-Token`
- `ConnectInfo<SocketAddr>` for peer IP (Axum supports this when using `into_make_service_with_connect_info`)
- request headers for origin/CORS enforcement (see §12.2)

Behavior:
1) Read setup_completed from settings:
   - If true: reject unless AdminUser exists (let admin route handle reset etc.)
2) Verify active session exists and not expired
3) Hash provided owner token and compare to stored hash
4) Determine effective client IP (respect trusted proxies only)
5) If request is non-local:
   - require remote setup enabled + remote setup token header, or reject 403
6) Apply rate limiting:
   - Per IP: e.g. 20/min
   - Per owner token: e.g. 60/min
7) Refresh session expiry on success

Output:
- struct `SetupGuard { owner_session_id: String, is_local: bool }` that handlers can use.

### 10.4 Rate limiting implementation (Tower layer)

Implement rate limiting as middleware layered onto `/api/v1/setup` router.

You can:
- Use `tower_governor` (recommended) OR implement a simple in-memory token bucket.
- The spec requires determinism and testability:
  - rate-limits MUST be configurable via env
  - tests MUST assert 429 behavior

### 10.5 Remove default admin bootstrap (main.rs)

Delete the entire block:

```rust
// Bootstrap admin if no users exist
if user_count == 0 { ... }
```

Replace with:
- ensure settings table has defaults:
  - `setup_completed=false`
  - `setup_state=NotStarted`
  - `server_name="Rustyfin"` (default)
- do NOT create any users.

Migration policy for existing installs:
- If users exist AND settings missing:
  - set `setup_completed=true` and `setup_state=Completed` on first boot (one-time migration logic).
  - This prevents breaking existing installs.

---

## 11) Validation reference (no guesswork)

### 11.1 Username validation

- Normalize:
  - trim whitespace
  - reject leading/trailing spaces (after trim, must be identical to input)
- Regex: `^[a-zA-Z0-9._-]{3,32}$`
- MUST enforce uniqueness:
  - recommended: store usernames lowercased in separate column or enforce uniqueness with `COLLATE NOCASE` in SQLite.

### 11.2 Password policy

- min length: 12
- max length: MUST be >= 128
- MUST reject passwords that are all whitespace
- SHOULD reject denylist:
  - `admin`, `password`, `123456`, `qwerty`, `letmein`, `rustyfin`, `jellyfin`
- SHOULD support passphrases (spaces allowed)
- MUST hash with Argon2 (already done in db repo)

### 11.3 Locale/region/timezone validation

- locale: relaxed BCP-47 validator (don’t implement the full standard; just reject obvious nonsense)
- region: `^[A-Z]{2}$`
- timezone:
  - safest approach: ship an allowlist JSON file of common IANA zones
  - minimal approach: validate format `^[A-Za-z_]+/[A-Za-z_]+$` and accept (risk: garbage values)
  - recommended: allowlist + “Other…” field for power users

### 11.4 Path validation

Algorithm:
1) Reject empty string
2) Reject any path containing `\0`
3) Convert to `PathBuf`
4) Canonicalize (if exists)
5) Reject if root is dangerous:
   - `/proc`, `/sys`, `/dev` by default
6) Check existence
7) Check readability:
   - attempt `std::fs::read_dir` or metadata open
8) Return boolean flags

Return hints:
- If not exists: “Path not found in container. Did you mount it?”
- If permission denied: “Server user lacks permission; adjust chmod/chown or Docker user.”

---

## 12) CORS, client IP, and bind semantics

### 12.1 Client IP extraction (Axum)

To use `ConnectInfo<SocketAddr>`, the server must be started with:

- `app.into_make_service_with_connect_info::<SocketAddr>()`

If you don’t do this, `ConnectInfo` won’t work and your “local vs remote” policy becomes guesswork.

### 12.2 CORS policy (browser safety)

During setup, browsers can be tricked into calling local services via malicious websites if CORS is permissive.

Rules:
- If a web UI is served from the same origin, set CORS to `SameOrigin` (strict).
- If UI is served separately (different port/domain), set `allow_origin` to the exact known UI origin(s).

Do NOT use `allow_origin(Any)`.

### 12.3 Trusted proxies

If the server is behind a reverse proxy, the “direct peer IP” is the proxy, not the client.

Rules:
- Only trust forwarded headers if the direct peer IP matches one of `trusted_proxies`.
- Otherwise ignore forwarded headers.

### 12.4 `allow_remote_access` vs bind address (match current Rustyfin)

Current Rustyfin uses `RUSTFIN_BIND` (default `0.0.0.0:8096`) for binding.

This spec defines a sane, explicit behavior:

- During setup (`setup_completed=false`):
  - default bind stays as currently: `0.0.0.0:8096` (LAN-accessible)
  - BUT setup write endpoints are protected by SetupWriteGuard (so LAN exposure isn’t catastrophic)

- After setup:
  - `allow_remote_access=false` SHOULD set default bind to `127.0.0.1:8096` **only if** `RUSTFIN_BIND` is not explicitly set.
  - `allow_remote_access=true` SHOULD set default bind to `0.0.0.0:8096` **only if** `RUSTFIN_BIND` is not explicitly set.

This preserves operator control: env var always wins.

---

## 13) Frontend implementation plan (UI-agnostic but complete)

> The repo’s `ui/` directory may be empty in your snapshot; therefore this section describes *requirements*, plus two reference implementations (Next.js or Vite SPA). Pick one and follow it.

### 13.1 Route guard (required)

On app boot:
1) call `GET /api/v1/system/info/public`
2) if `setup_completed=false`:
   - route to `/setup`
   - hide login UI completely
3) if `setup_completed=true`:
   - normal auth flow

### 13.2 Wizard skeleton (state + API client)

Wizard MUST have:
- `WizardContext` storing:
  - `owner_token`
  - cached `system_info`
  - step form values
  - last server error
- API client wrapper that automatically injects:
  - `X-Setup-Owner-Token` header on setup write calls

### 13.3 “Session claimed” UX

When claim returns 409 `setup_claimed`, show:
- “Setup is in progress on: {claimed_by}”
- “Expires at: {expires_at}”
- CTA buttons:
  - “Refresh” (retry claim)
  - “Take over” (only shown if local, requires extra confirmation)

### 13.4 Exact UI copy (suggested)

- Welcome: “Rustyfin needs a one-time setup before you can log in.”
- Admin step warning: “This account controls the server. Use a long passphrase.”
- Networking warning: “Remote access can expose your server to the internet. Prefer a reverse proxy with TLS.”

### 13.5 Accessibility requirements (must be testable)

- Full keyboard navigation
- Visible focus indicators
- Error text announced via ARIA live region
- Inputs have labels and `aria-describedby` for errors
- Tap targets ≥ 44px
- Supports `prefers-reduced-motion`

---

## 14) Testing plan (backend + UI)

### 14.1 Unit tests

- Setup state machine transitions:
  - valid and invalid transitions
- Validation helpers:
  - username regex edge cases
  - password rules
  - locale/region/timezone parsing
  - path validator behavior (mock filesystem when possible)
- Token hashing + constant-time compare helpers
- Trusted proxy header parsing

### 14.2 Integration tests (Axum + sqlx)

Using `crates/server/tests/integration.rs` style:

- Fresh DB:
  - `GET /system/info/public` returns `setup_completed=false`
  - `POST /setup/admin` before config returns 409
- Session:
  - claim works
  - second claim without force returns 409
  - release is idempotent
- Owner token gating:
  - missing token → 401
  - wrong token → 401
- Rate limiting:
  - exceed limit returns 429
- Completion:
  - sets setup_completed true
  - setup endpoints now require admin JWT
- Migration compatibility:
  - if users exist and no settings, server treats as already setup

### 14.3 E2E tests (Playwright/Cypress)

- Fresh install: app shows wizard, not login
- Complete wizard: routes to login; admin can log in
- Skip libraries: still completes
- Two browser sessions:
  - second sees “setup in progress”
- Remote setup:
  - non-local write blocked without remote token
  - succeeds with remote token when enabled

---

## 15) Definition of done checklist (zero ambiguity)

A build is “wizard-complete” only if ALL are true:

- [ ] Fresh install shows setup wizard; login is not shown.
- [ ] Server does NOT create default `admin` user on boot.
- [ ] `GET /api/v1/system/info/public` returns setup flags and nothing sensitive.
- [ ] Setup steps enforce ordering with 409 + `setup_state_violation`.
- [ ] Setup steps are idempotent where specified.
- [ ] Concurrency: only one setup owner token can write at a time.
- [ ] Local-vs-remote policy works (trusted proxies safe).
- [ ] Rate limiting returns 429.
- [ ] Passwords/tokens/paths never logged.
- [ ] After completion, setup endpoints require admin auth.
- [ ] Automated tests cover the above (unit + integration + e2e).
- [ ] Wizard works on mobile and with keyboard only.

---

## Appendix A) Implementation snippets (copy/paste grade)

### A.1 Hashing owner tokens

- Use SHA-256 or BLAKE3; store hex.
- Compare using constant-time equality.

### A.2 Idempotency key storage

- `request_hash = sha256(canonical_json(request_body))`
- If key exists:
  - if hashes match: return stored response_json
  - else 409 idempotency_conflict

### A.3 SQLite uniqueness (case-insensitive username)

Recommended schema tweak:

```sql
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_username_nocase
ON user(username COLLATE NOCASE);
```

---

## Appendix B) External references (URLs in code blocks)

Jellyfin setup wizard overview:
```text
https://jellyfin.org/docs/general/post-install/setup-wizard/
```

RFC requirement keywords:
```text
https://www.rfc-editor.org/rfc/rfc2119
```
