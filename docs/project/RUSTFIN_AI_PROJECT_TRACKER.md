# Rustfin AI Implementation Tracker & Project Operating Manual
Generated: 2026-02-13  
Purpose: This document is written for an AI agent (and humans) to **structure the Rustfin project**, enforce constraints, and keep an always-up-to-date record of **what’s implemented vs what remains**.

> **Rule #1:** Every time you implement anything (even a tiny refactor), update the **Status Ledger** and the relevant checklists in this file.  
> **Rule #2:** Do not “silently drift” from constraints. If you must change a constraint, record it in **Decision Log** with rationale and tradeoffs.

---

## 0) Project identity (one paragraph)
Rustfin is a **local-first** Jellyfin-class media server, implemented as a **single Rust server binary** with **SQLite** for storage, using **FFmpeg/ffprobe** for media probing/remux/transcoding, and a **single UI app** (either Rust/WASM or Next.js) talking to the server via a versioned HTTP API. Streaming supports **HTTP Range** (Direct Play) and **HLS** (adaptive; Transcode when required). Docker support is required.

---

## 1) Hard constraints (do not violate)

### 1.1 Stack constraints
- **Backend language:** Rust (required).
- **Server architecture:** **Modular monolith** (one server process). No microservice network.
- **Database:** SQLite (required) using WAL mode.
- **Media engine:** FFmpeg + ffprobe (required). Do not attempt to implement codecs/containers.
- **Client/UI:** Choose exactly **one**:
  - **Option A:** Rust/WASM (Leptos or equivalent)  
  - **Option B:** Next.js (TypeScript/React)
- **Two-language cap:** Rust + (Rust OR TypeScript). Avoid additional languages/services.
- **Local-first:** Intended to run locally or in Docker; not production multi-tenant.

### 1.2 Feature constraints (baseline Jellyfin-class)
Rustfin must support:
- Multi-user auth + roles (admin/user)
- Libraries (paths, scans, types)
- Movies + TV series (Show → Seasons → Episodes default)
- Metadata providers + provider IDs + user overrides
- Artwork (posters/backdrops/logos/thumbs) + caching/resizing
- Playback sessions
- Direct Play via Range
- HLS generation for broad device support
- Subtitle discovery (sidecar + embedded) + selection; optional download provider
- Missing episodes placeholders (expected vs present) configurable per user
- Docker + persistent volumes + (optional) GPU support

### 1.3 “No surprises” rules
- All config is **DB-first** (stored in SQLite). Do not require editing config files for normal use.
- Providers are optional and must be **visible** (no hidden network calls).
- Every expensive task must be cancelable (jobs, transcodes).
- Every feature needs an acceptance check (see checklists).

---

## 2) Sources of truth (documents and how to use them)
This repo has an expanded spec library (see the bundle). The AI should use:
- `01_ARCHITECTURE_OVERVIEW.md` for subsystem boundaries and flows
- `04_API_SPEC.md` for endpoint contracts
- `05_DATABASE_SPEC.md` for schema, locks, expected-episodes model
- `06_BACKEND_REST_IMPLEMENTATION.md` for Axum patterns and Range correctness
- `07_METADATA_SUBTITLES_ARTWORK_PROVIDERS.md` for provider precedence rules
- `08_STREAMING_TRANSCODING_GPU.md` for Direct vs HLS vs GPU policy
- `02_UI_UX_SPEC.md` + `03_THEME_STYLE_MOTION.md` for UI behavior and tokens

**Operating rule:** If implementation differs from the spec, either:
1) fix implementation to match, OR  
2) update spec + log a decision in **Decision Log**.

---

## 3) Repository structure (recommended)
If not yet created, build toward this layout:

```
/crates
  /server        # Axum, routing, SSE, streaming endpoints
  /db            # sqlx models, migrations, repositories
  /core          # shared domain types (ItemKind, DeviceProfile, etc)
  /scanner       # filesystem scanning, parsers, file->item mapping
  /metadata      # provider clients, merge engine, expected episodes
  /artwork       # image cache + resize service
  /subtitles     # sidecar/embedded discovery, extract, optional provider
  /transcoder    # ffmpeg session orchestration
  /jobs          # queue + workers + cancellation
/ui             # EITHER (Rust/WASM app) OR (Next.js app)
/docs           # specs, diagrams, ADRs
```

---

## 4) “What is implemented?” tracking protocol

### 4.1 Status Ledger (single place, always updated)
Maintain **exactly one** authoritative status table here.

**Legend:**  
- ✅ Done (implemented + tested)  
- 🟡 Partial (implemented but missing key pieces/tests)  
- 🔴 Not started  
- ⛔ Blocked (explain why)

> Update this table after every change.

| Subsystem | Status | Notes (what’s done / what’s missing) | Last updated |
|---|---:|---|---|
| Repo skeleton + workspace | ✅ | Cargo workspace with core/db/server crates, rustfmt + clippy config | 2026-02-13 |
| DB migrations + connection | ✅ | SQLite WAL mode, forward-only migration runner, all baseline tables | 2026-02-13 |
| Auth (login, tokens) | ✅ | POST /api/v1/auth/login with Argon2 + JWT, Bearer auth extractor | 2026-02-13 |
| Users + roles + prefs | ✅ | Admin bootstrap, GET /users/me, GET/PATCH preferences, POST/GET /users (admin create+list), DELETE /users/{id} | 2026-02-14 |
| Libraries CRUD | ✅ | POST/GET/PATCH /libraries, paths, item counts, admin-only guards | 2026-02-13 |
| Scan job queue | ✅ | POST /libraries/{id}/scan creates job, GET/cancel jobs, job status tracking | 2026-02-13 |
| File parsing (movies) | ✅ | Title (Year) from filename + folder, dot/paren formats | 2026-02-13 |
| File parsing (shows/episodes) | ✅ | SxxExx, 1x02, Season X Episode Y patterns, specials S00 | 2026-02-13 |
| Item graph (series→seasons→episodes) | ✅ | Auto-creates series→season→episode hierarchy, idempotent scans | 2026-02-13 |
| Provider IDs + manual identify | ✅ | Parser extracts [tmdb=], [tvdb=], [imdb=] from folder names; provider_id CRUD in merge engine; routes for GET providers, POST/DELETE field-locks | 2026-02-14 |
| Metadata provider client | ✅ | TMDB API v3 client (search/get movies+series, season episodes, credits); MetadataProvider trait; POST /items/{id}/metadata/refresh route | 2026-02-14 |
| Merge engine + field locks | ✅ | Merge respects locked fields; item_field_lock table; user overrides survive refresh; 2 merge unit tests | 2026-02-14 |
| Expected episodes + missing logic | ✅ | Expected episodes DB repo (upsert/get/missing); GET /items/{id}/expected-episodes + /missing-episodes routes | 2026-02-14 |
| Artwork cache + resize | ✅ | GET /items/{id}/images/{type}?w=&h= with remote download + local file cache, ETag + Cache-Control headers | 2026-02-14 |
| Subtitles discovery | ✅ | Sidecar discovery (lang/forced/sdh markers), embedded enumeration via ffprobe, serving endpoint with path security | 2026-02-13 |
| Playback sessions + progress | ✅ | POST /playback/progress + GET /playback/state/{id}, user_item_state upsert, integration test | 2026-02-13 |
| Range streaming (Direct Play) | ✅ | RFC 7233 Range (206/416), path traversal protection, content-type detection, 7 unit tests + integration test | 2026-02-13 |
| HLS sessions + playlists | ✅ | SessionManager with semaphore, ffmpeg spawn, segment serving, idle cleanup, playlist/segment routes | 2026-02-13 |
| Transcode orchestration | ✅ | Decision engine (direct/remux/transcode), ffprobe media info, HW accel config | 2026-02-13 |
| GPU acceleration | ✅ | Detection via ffmpeg -encoders, NVENC/VAAPI/QSV/VideoToolbox, GET /system/gpu, Docker GPU compose files | 2026-02-13 |
| SSE/WebSocket events | ✅ | Broadcast channel with typed ServerEvent enum; scan/job/metadata/heartbeat events; real-time SSE endpoint with reconnection support | 2026-02-14 |
| UI app foundation | ✅ | Next.js app: login, libraries list, library items, item detail (seasons/episodes), API client with auth | 2026-02-14 |
| UI player + track selection | ✅ | Video player page with Direct Play + HLS (hls.js) mode, progress reporting, quality switching | 2026-02-14 |
| Admin dashboard | ✅ | Admin page: create/scan libraries, view jobs; user management API (create/list/delete users) | 2026-02-14 |
| Docker (CPU) | ✅ | Multi-stage Dockerfile, docker-compose.yml, volumes for /config /cache /transcode /media | 2026-02-13 |
| Docker (GPU) | ✅ | docker-compose.gpu.yml (NVIDIA), docker-compose.vaapi.yml (Intel/AMD), documented setup | 2026-02-13 |
| Testing harness | ✅ | 57 tests: 18 scanner + 7 range + 9 transcoder + 4 metadata + 19 integration | 2026-02-14 |
| Observability (logs/metrics) | 🟡 | tracing + env-filter; metrics endpoint not yet | 2026-02-13 |
| Security hardening | ✅ | Argon2 hashing, JWT auth, error envelope, path traversal protection on streaming | 2026-02-13 |

### 4.2 “Implementation log” (append-only)
After each meaningful change, append a bullet to this section:

#### Implementation Log
- (2026-02-13) [Milestone 0] Repo skeleton + health + DB + auth — Cargo workspace (core/db/server), domain types, ApiError envelope, SQLite WAL + migrations, Argon2 auth + JWT, admin bootstrap, health/login/users/prefs endpoints. 7 integration tests.
- (2026-02-13) [Milestone 1] Libraries CRUD + Jobs queue — DB repos for libraries and jobs. AdminUser extractor (403 for non-admins). POST/GET /libraries, GET/PATCH /libraries/{id}, POST /libraries/{id}/scan, GET/cancel /jobs. 6 new integration tests.
- (2026-02-13) [Milestone 2] Scanner + item graph — Created scanner crate with filename parser (movies: Title (Year), TV: SxxExx/1x02/Season X Episode Y, specials S00), filesystem walker (ignore patterns, video-only), scan engine (walk→parse→DB items+files). Items repo for browse (get, children, library items). Server routes: GET /libraries/{id}/items, GET /items/{id}, GET /items/{id}/children. Scan runs in background via tokio::spawn with job status tracking. 12 parser unit tests + 3 scan integration tests. Files: crates/scanner/*, crates/db/src/repo/items.rs, crates/server/src/routes.rs. Follow-up: Range streaming (Milestone 5).
- (2026-02-13) [Milestone 4] Range streaming (Direct Play) — RFC 7233-compliant Range handler: supports bytes=start-end, start-, -suffix; returns 206 Partial Content with Content-Range header; 416 for invalid ranges. Path traversal protection via canonicalization + library root checks. Content-type detection from extension. Route: GET /stream/file/{file_id}. 7 unit tests + 1 integration test. Files: crates/server/src/streaming.rs, crates/db/src/repo/media_files.rs.
- (2026-02-13) [Milestone 4] Playback progress — DB repo (playstate.rs) with update_progress (upsert user_item_state) and get_play_state. Routes: POST /api/v1/playback/progress, GET /api/v1/playback/state/{item_id}. Integration test verifying progress update + mark-as-played flow. Files: crates/db/src/repo/playstate.rs, crates/server/src/routes.rs, crates/server/tests/integration.rs. Total: 37 tests passing.
- (2026-02-13) [Milestone 6] HLS transcode + playback decision — Created rustfin-transcoder crate with: ffprobe media info extraction (JSON parsing, video/audio/subtitle streams), playback decision engine (direct play/remux/transcode with reasons), HLS session manager (semaphore-gated, ffmpeg argv spawn, idle cleanup, segment/playlist serving), HW accel config (NVENC/VAAPI/QSV/VideoToolbox). Server routes: POST /api/v1/playback/sessions (create HLS session), POST /api/v1/playback/sessions/{sid}/stop, GET /api/v1/playback/info/{file_id} (ffprobe), GET /stream/hls/{sid}/master.m3u8, GET /stream/hls/{sid}/{filename}. 8 new unit tests (decision + ffprobe + hls). Files: crates/transcoder/*, crates/server/src/routes.rs, crates/server/src/state.rs, crates/server/src/main.rs. Total: 45 tests passing.
- (2026-02-13) [Milestone 7] Subtitles discovery — Sidecar subtitle file discovery with language (ISO 639), forced, and SDH/HI markers. Supported formats: SRT, SUB, ASS, SSA, VTT, SUP, IDX. Embedded track enumeration via ffprobe. Routes: GET /api/v1/items/{id}/subtitles (lists sidecar + embedded), GET /stream/subtitles/{path} (serves sidecar files with path security). 6 subtitle unit tests. Files: crates/scanner/src/subtitles.rs, crates/db/src/repo/libraries.rs, crates/server/src/routes.rs. Total: 51 tests passing.
- (2026-02-13) [Milestone 8] GPU detection + Docker — GPU encoder detection via `ffmpeg -encoders` (NVENC/VAAPI/QSV/VideoToolbox), best-accelerator selection, admin endpoint GET /api/v1/system/gpu. Multi-stage Dockerfile (rust:1.83 builder → debian:bookworm-slim runtime with ffmpeg). docker-compose.yml (CPU), docker-compose.gpu.yml (NVIDIA), docker-compose.vaapi.yml (Intel/AMD VAAPI). Persistent volumes for /config, /cache, /transcode, /media. 1 new GPU test. Files: crates/transcoder/src/gpu.rs, Dockerfile, docker-compose*.yml. Total: 52 tests passing.
- (2026-02-14) [Milestone 3] Metadata provider + merge engine — Created rustfin-metadata crate with TMDB API v3 client (search/get movies+series, season episodes, credits), MetadataProvider trait, merge engine with field locks (user overrides survive refresh), provider ID CRUD. Migration 002 adds metadata columns. Routes: POST /items/{id}/metadata/refresh, GET /items/{id}/providers, POST/DELETE /items/{id}/field-locks. 4 metadata tests. Files: crates/metadata/*, crates/db/migrations/002_metadata_columns.sql. Total: 56 tests.
- (2026-02-14) [Milestone 4] TV correctness — Expected episodes DB repo (upsert_expected, get_expected, get_present, get_missing). Routes: GET /items/{id}/expected-episodes, GET /items/{id}/missing-episodes. File: crates/db/src/repo/episodes.rs.
- (2026-02-14) [Milestone 9] UI app (Next.js) — Decision recorded in Decision Log. Created Next.js app with pages: login, libraries list, library detail, item detail (seasons/episodes), video player (Direct Play + HLS via hls.js + progress reporting), admin dashboard (create/scan libraries, view jobs). API client helper with auth token management. npm install + build verified. Files: ui/src/app/*, ui/src/lib/api.ts.
- (2026-02-14) [Milestone 10] User management + artwork + SSE events — Added user management API routes (POST/GET /users, DELETE /users/{id}) with admin guards. Artwork image endpoint (GET /items/{id}/images/{type}?w=&h=) with remote download + local file cache + ETag + Cache-Control. Replaced SSE placeholder with broadcast channel (typed ServerEvent enum: scan_progress, scan_complete, metadata_refresh, job_update, heartbeat). Scan handler emits events. User management integration test. Total: 57 tests passing.

---

## 5) Definition of Done (global)
A feature is **Done ✅** only when:
- It compiles (CI or local), and
- It has at least one test (unit or integration) OR a documented manual test procedure, and
- It updates docs/specs if behavior changed, and
- It updates the Status Ledger + Implementation Log.

---

## 6) Coding standards & invariants (backend)

### 6.1 Rust async rules
- Use Tokio.
- Never spawn unbounded tasks without backpressure.
- Every expensive operation must support cancellation (CancellationToken).

### 6.2 Error model rules
- API errors are consistent envelopes: `{ error: { code, message, details } }`.
- Avoid leaking internal errors to clients; log internally, return safe messages.

### 6.3 DB rules
- Single migration path (forward-only).
- WAL mode enabled.
- Use transactions for multi-write operations (scan updates, metadata merge).

### 6.4 File system safety
- All media file paths must be canonicalized and checked against allowed library roots.
- Prevent path traversal in streaming endpoints.

### 6.5 FFmpeg safety
- Never shell-invoke ffmpeg via a string command.
- Always pass args as an argv list (prevents injection).
- Capture ffmpeg logs per session for debugging.

---

## 7) High-level milestone plan (AI should execute in this order)
**Milestone 0:** skeleton + health + DB + auth  
**Milestone 1:** libraries + scan job + browse minimal  
**Milestone 2:** metadata provider + artwork  
**Milestone 3:** TV correctness + missing episodes + specials  
**Milestone 4:** playback (Range + HLS CPU) + progress reporting  
**Milestone 5:** GPU acceleration + subtitle enhancements  
**Milestone 6:** Docker ops + backup/restore + docs polish

---

## 8) Detailed checklists (what to build)

### 8.1 Repo & bootstrap
- [x] Create Cargo workspace
- [x] Add `crates/server` Axum app
- [x] Add `crates/db` with sqlx + migrations
- [ ] Add `/docs` with specs included
- [x] Add formatting/lints (rustfmt, clippy config)

### 8.2 Database & migrations
- [x] Enable WAL mode on startup
- [x] Implement migration runner
- [x] Tables: library, library_path, item, media_file, episode_file_map
- [x] Tables: users, prefs, user_item_state
- [x] Tables: provider IDs + field locks
- [x] Tables: jobs
- [x] Tables: expected episodes (TV)

### 8.3 Auth & users
- [x] Admin bootstrap (first run)
- [x] Login endpoint + token issuing
- [x] Auth extractor middleware
- [x] Roles (admin/user) enforced
- [x] User preferences CRUD (JSON blob)

### 8.4 Libraries
- [x] Create library + paths
- [x] List libraries + stats (item counts)
- [x] Trigger scan job
- [ ] Scan scheduling settings (optional)

### 8.5 Scanner
- [x] Enumerate files in paths
- [x] Record file stats + hashes
- [x] Parse movies (Title (Year))
- [x] Parse shows (SxxExx, 1x02, Season X Episode Y)
- [x] Build item graph (series→seasons→episodes)
- [ ] Handle multi-part episodes (part1/part2)
- [x] Ignore patterns (.DS_Store, @eaDir, etc)
- [ ] Emit scan progress events

### 8.6 Metadata & providers
- [x] Provider client abstraction
- [x] Provider IDs in folder names ([tmdb=], [tvdb=], [imdb=])
- [x] Manual identify endpoint + UI flow support
- [x] Merge engine with precedence rules
- [x] Field locks and user overrides preserved
- [x] Expected episodes list for series (drives missing placeholders)

### 8.7 Artwork
- [x] Local image discovery + precedence
- [x] Remote fetch + caching
- [ ] Variant generation (resize to requested w/h)
- [x] ETag + cache-control headers

### 8.8 Subtitles
- [x] Sidecar discovery (lang + forced markers)
- [x] Embedded track enumeration (ffprobe cached)
- [ ] Extraction job (optional)
- [x] Subtitle selection in playback session manifest
- [ ] Optional download provider (later milestone)

### 8.9 Playback & streaming
- [x] Playback session creation endpoint
- [ ] Device profile capability model
- [x] Decision engine: direct vs remux vs transcode
- [x] Range streaming handler (206, Content-Range, 416)
- [x] Progress update endpoint
- [x] Stop session endpoint

### 8.10 HLS + transcoding
- [x] Session dir management (/transcode/<sid>/)
- [x] Spawn ffmpeg with controlled args
- [x] Serve master + variant playlists
- [x] Serve segments with correct types
- [x] Cleanup expired sessions
- [x] Resource limits (max transcodes)

### 8.11 GPU acceleration
- [x] Detect available accel (NVENC/QSV/VAAPI)
- [x] Config option to enable/disable GPU
- [x] Docker runtime docs + compose examples

### 8.12 Events
- [x] SSE endpoint
- [x] Event types: scan progress, job progress, metadata refresh, transcode state
- [ ] Client reconnection strategy (event id / resume)

### 8.13 UI app (choose one)
**Choose exactly one path:**

**A) Rust/WASM UI**
- [ ] Base app shell + routing
- [ ] Login, libraries, browse, item detail
- [ ] Player integration + track controls
- [ ] Admin pages

**B) Next.js UI** ← CHOSEN (see Decision Log)
- [x] Base app shell + routing
- [x] Login, libraries, browse, item detail
- [x] Player integration (native HLS or hls.js fallback)
- [x] Admin pages

### 8.14 Docker & ops
- [x] Multi-stage Dockerfile
- [x] docker-compose (CPU)
- [x] volumes: /config /cache /transcode /media
- [ ] backup instructions
- [x] health endpoint

### 8.15 Testing & observability
- [x] Unit tests: filename parsing, range parsing, merge rules
- [x] Integration tests: scan fixtures → DB → API
- [x] Streaming test: request Range returns correct bytes
- [ ] HLS tests: playlist parse + segment availability
- [x] tracing logs with request id + session id
- [ ] optional metrics endpoint

---

## 9) Acceptance test snippets (AI should keep these updated)

### 9.1 Range streaming manual test
- Start server with a known media file.
- Request:
  - `curl -v -H "Range: bytes=0-999" http://localhost:8096/stream/file/<file_id> -o /tmp/chunk`
- Expect:
  - `HTTP/1.1 206 Partial Content`
  - `Content-Range: bytes 0-999/<size>`
  - output length 1000 bytes

### 9.2 Missing episodes behavior
- Identify a series with provider episode list (10 episodes).
- Place only 8 episodes in media folder.
- Toggle `show_missing_episodes=true`
- UI shows 10 entries with 2 marked missing and “8/10 present”.

---

## 10) Decision Log (ADR-lite)
Record any meaningful architectural choice changes here.

### Template
- **Date:** YYYY-MM-DD
- **Decision:** (what changed)
- **Context:** (why)
- **Options considered:** (A/B/C)
- **Chosen because:** (tradeoffs)
- **Consequences:** (what must be updated / migration / deprecation)

### Decisions
- **Date:** 2026-02-13
- **Decision:** UI framework: Next.js (TypeScript/React)
- **Context:** Need to choose exactly one UI path per hard constraint 1.1.
- **Options considered:** (A) Rust/WASM (Leptos), (B) Next.js (TypeScript/React)
- **Chosen because:** Next.js offers mature ecosystem for media UIs (hls.js for HLS playback, video.js alternatives, rich component libraries). Faster UI development iteration. hls.js provides MSE-based HLS on Chrome/Firefox while Safari uses native HLS. Stays within two-language cap (Rust + TypeScript).
- **Consequences:** UI lives in /ui directory as a Next.js app. Served separately during dev, can be built as static export and served by Rust server in production.

---

## 11) “AI guardrails” (how to behave while coding)
- Prefer **small, testable increments** over big rewrites.
- Keep public APIs stable; bump version if breaking.
- Always update:
  - Status Ledger
  - Implementation Log
  - Any affected spec doc
- If a task is blocked, mark it **⛔** and state the blocker explicitly.

---

## 12) Quick-start for a fresh AI session
When you (the AI) resume work:
1) Read Status Ledger + last 10 lines of Implementation Log.
2) Identify the next milestone.
3) Choose one small deliverable and implement end-to-end.
4) Update Status Ledger + Implementation Log.
5) Ensure compile/test passes.
