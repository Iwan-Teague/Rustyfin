# Rustyfin Comprehensive Project Audit — 2026-06-08

**Scope:** Whole-project examination across security, vault cryptography, video playback, text channels, audio/voice channels, and architecture/performance/CI.
**Method:** Six parallel specialist code audits over the live `main` tree, each grounded in the actual source (no speculation). The highest-stakes findings (video direct-play, backup privilege escalation, private-channel leak) were independently re-verified with targeted greps. File:line references are to the tree at commit `4effba6`.

---

## 1. Executive Summary

Rustyfin is a **mature, well-engineered codebase** with genuinely strong fundamentals: Argon2id password hashing, parameterized SQL everywhere (no injection vectors found), a real zero-knowledge vault design, clean crate boundaries, and near-zero `unwrap()`/`panic!` in request paths. The core social-continuation flow works.

However, this audit found **2 Critical and 10 High** issues that materially affect the owner's stated priorities. The three that matter most:

1. **Video always transcodes** — the server has a complete direct-play/remux decision engine and a working byte-range file server, but the video player never uses either. Every play (even a stock H.264/AAC MP4 the browser could play natively) is force-fed through a full `libx264` re-encode into classic 4-second HLS. This is the single biggest hit to the "low-latency, snappy, efficient decoding" goal.
2. **Private text channels leak to all users** — the WebSocket channel list and live events ignore the `is_private`/role filter that the HTTP API correctly applies. Since the UI builds its sidebar from the WebSocket path, every user sees (and receives live messages for) private/admin-only channels.
3. **System backup & restore is reachable by any authenticated user** — all `/system/backups` handlers authenticate with `AuthUser` instead of `AdminUser`, so a regular account can overwrite the entire database.

The vault encryption is **secure and genuinely zero-knowledge at rest** (the server stores only ciphertext, wrapped key, salt/params, and nonces — verified the server crate contains no decryption code at all). The one real vault defect is in the browser extension: the cross-origin-iframe block that prevents credential theft is **not enforced on the fill path**, contradicting the documented hard boundary.

No committed secrets, no secret logging, and no SQL injection were found. The biggest process gap is that **the main workspace has no CI build/test/lint gate** — only the AI-judge workflows run — even though a comprehensive gate script already exists and is just not wired into GitHub Actions.

### Posture by domain

| Domain | Verdict | Worst severity |
|---|---|---|
| Security & secrets | Sound core; concentrated access-control + browser-token gaps | High |
| RustyVault encryption | Secure & zero-knowledge at rest; one extension fill-path hole | High |
| Video playback | Functional but fundamentally inefficient for low-latency goal | **Critical** |
| Text channels | Work, but private channels are over-exposed | High |
| Audio/voice channels | Work for 2–5 people with TURN; no failure recovery | High |
| Architecture / perf / CI | Healthy; main-workspace CI gate missing; AI code disproportionate | High |

### Are the owner's secrets safe? Is the vault secure?
**Yes on both, with caveats.** No plaintext secrets are committed or logged; the JWT secret falls back to a random per-boot value (never a hardcoded default); the vault is true zero-knowledge at rest. Caveats: the TMDB *third-party* API key is stored unencrypted in the settings table (masked in API responses), and the browser session JWT is held in `localStorage` + a non-HttpOnly/non-Secure cookie, so any XSS is account-takeover-capable.

---

## 1b. Remediation Log — 2026-06-08 (P0 fixes landed)

Immediately after this audit, the four P0 items (including both Criticals) were fixed via four parallel agents and verified: `cargo check` + `clippy` clean on all changed files, targeted unit + integration tests, and UI/extension `tsc` clean.

| ID | Fix | Verification |
|---|---|---|
| **SEC-1** | All 5 `/system/backups` handlers now require `AdminUser` (was `AuthUser`); import updated. | New integration test `system_backup_restore_rejects_non_admin` → **403 PASS** (against local PG). |
| **TXT-1/2/5** | `build_hello` role-filters channels at both call sites; per-socket fan-out drops private-channel events for non-admins via an in-memory private-id set kept in sync on channel create/update/delete; added `ChannelEvent::channel_id()`. | 2 new unit tests + existing channels suite **10/10 PASS**. |
| **VID-1/2** (+ VID-10 container match) | Playback descriptor now returns `play_method` + `direct_play`, computed server-side via `decide()` over a single reused ffprobe (conservative browser caps: H.264/AAC/MP3 in mp4/mov, VP8/9/Opus/Vorbis in webm, else transcode); both video players direct-play compatible content through the byte-range server instead of transcoding, with guarded HLS-only cleanup; container match is now token-based. | transcoder unit **15/15**, playback/stream integration **3/3 PASS**, UI **tsc clean**. |
| **VLT-1** | Extension fill evaluates the cross-origin-iframe policy against the *actual target frame* (browser-supplied `sender.url`/`frameId`, no new permission) and refuses cross-origin fills; `get-inline-state` + `page-context` hardened against frame spoofing. | Extension `tsc --noEmit` **clean (exit 0)**. |

**Scope note (P0):** VID-1/2 eliminates *needless* transcoding (the big latency/CPU win — compatible files now play directly).

### P1 fixes (landed 2026-06-08, second wave)

| ID | Fix | Verification |
|---|---|---|
| **VID-3/4** | HLS sessions now `-c copy` remux H.264/AAC sources whose only problem is the container (e.g. MKV) instead of re-encoding (`+h264_mp4toannexb`, audio copied when already AAC); software/HW encoders tuned for fast start (`-tune zerolatency`, NVENC `-preset p1 -tune ll`, GOP = 1 keyframe/segment); default segment 4s→2s. Forced-quality requests still fully transcode. | transcoder **20/20**, playback/stream integration **3/3**, server check clean. |
| **AUD-1/3/7** | Voice peers recover from ICE/connection failure (`restartIce` → rebuild, disconnect grace timer, debounce, retry-capped), pick up TURN if `/runtime-config` resolves late, and surface per-peer connection state for a reconnecting/failed indicator. | UI **tsc clean**. (Real WebRTC needs a manual smoke test.) |
| **SEC-5/6** | Security headers (CSP, `frame-ancestors 'none'`, `nosniff`, Referrer/Permissions-Policy) now apply app-wide, not just `/vault`; vault CSP drops `'unsafe-eval'` (keeps `'wasm-unsafe-eval'` for Argon2). Main-app `script-src` keeps `'unsafe-inline'` for Next hydration (documented tradeoff). | UI **tsc clean**. CSP needs a runtime smoke test. |
| **ARC-1** | Added `.github/workflows/ci.yml` running `scripts/ci/debian_native_gates.sh` (fmt/clippy/tests/UI build) with a Postgres service on PR + push to `main`. | YAML validated; full run only verifiable on GitHub. |

**P2 fixes (landed 2026-06-08, third wave):** ARC-3 — the DB pool now sets `acquire_timeout(10s)` + `min_connections(2)`, so requests fail fast instead of waiting indefinitely on pool exhaustion and stay warm after idle. SEC-2 — `serve_subtitle` now requires `AuthUser`; it was an unauthenticated file-read endpoint and is unused by the current UI (subtitles load client-side), so gating it breaks no live flow.

**Investigated, deferred with reason:** VID-6 (idle-session timeout) is *not* a safe one-line lower — the player buffers up to 600s, so segment fetches (which reset the idle timer) pause during normal playback; a short server timeout would reap actively-playing sessions. It needs a client heartbeat first. ARC-4 (continue-watching N+1) is bounded to 12 rows, self-healing after the first load, and dominated by the per-movie ffprobe (not the query count) — low value versus a moderate query refactor.

**Still open (notable):** SEC-3 (HttpOnly/Secure cookie auth — an auth-transport refactor that also needs CSRF handling); AUD-2 (full-mesh → SFU, only if larger rooms are wanted); the rest of the Medium/Low backlog (VID-5/7, TXT-4, ARC-2). The P0, P1, and P2 fix sets are all committed to `main`.

> **Runtime caveat:** the app-wide CSP (SEC-5) and the voice-recovery changes (AUD-1) can only be *fully* validated at runtime. Smoke-test video/HLS playback, channels voice, watch-party, and vault unlock before relying on them. The transcode remux path (VID-3) should be checked against a real H.264-in-MKV file.

---

## 2. Master Findings Table

Severity reflects impact on the relevant axis (security, user-experience/latency, reliability, maintainability).

| ID | Sev | Area | Finding | Location |
|---|---|---|---|---|
| VID-1 | **Critical** | Video | Player never direct-plays — always transcodes to HLS | `ui/src/app/player/[id]/page.tsx:499-702` |
| VID-2 | **Critical** | Video | `decision.rs` is dead code — no server-side play decision is ever made | `crates/transcoder/src/decision.rs` |
| SEC-1 | High | Security | Backup/restore reachable by any authenticated user (DB overwrite) | `crates/server/src/backups/handlers.rs:13-58` |
| VLT-1 | High | Vault | Cross-origin iframe block not enforced on credential fill | `extensions/rustyvault-webext/src/background/index.ts:640-668` |
| VID-3 | High | Video | No stream-copy/remux path — container-only mismatches re-encode | `crates/transcoder/src/session.rs:707-753` |
| VID-4 | High | Video | Encoder preset/GOP not tuned; classic 4s HLS, no LL-HLS | `crates/transcoder/src/session.rs:743-789` |
| TXT-1 | High | Channels | WebSocket `Hello` leaks private channels to non-admins | `crates/server/src/channels/ws.rs:688-700` |
| TXT-2 | High | Channels | Global broadcast delivers private-channel messages to non-members | `crates/server/src/channels/manager.rs:13-14`, `ws.rs:281-296` |
| TXT-3 | High | Channels | No membership model — "private" is binary admin-only | `crates/db/migrations_pg/008_channels.sql` |
| AUD-1 | High | Voice | No ICE/connection-failure recovery or reconnection | `ui/src/app/channels/components/VoiceEngine.tsx:553-579` |
| AUD-2 | High | Voice | Full-mesh O(n²) topology does not scale past a handful of peers | `VoiceEngine.tsx:581-611`, `channels/ws.rs:495-619` |
| AUD-3 | High | Voice | STUN-only default strands symmetric-NAT peers, no fallback | `ui/src/lib/channelsContext.tsx:214-216`, `ui/src/app/runtime-config/route.ts:136-169` |
| ARC-1 | High | CI | No CI gate for the main workspace (only AI-judge workflows) | `.github/workflows/` |
| ARC-2 | High | Arch | AI subsystem disproportionate; `orchestrator.rs`/`tools.rs` God-files | `crates/server/src/ai_assistant/orchestrator.rs` (15k LOC) |
| SEC-2 | Medium | Security | Unauthenticated subtitle file read | `crates/server/src/routes.rs:4627`, `:288` |
| SEC-3 | Medium | Security | JWT in `localStorage` + non-HttpOnly cookie (XSS → takeover) | `ui/src/lib/browserAuth.ts:19-20` |
| SEC-4 | Medium | Security | Auth cookie missing `Secure` flag | `ui/src/lib/browserAuth.ts:5` |
| SEC-5 | Medium | Security | CSP / clickjacking headers applied only to `/vault`, not the app | `ui/src/middleware.ts:95-128` |
| VID-5 | Medium | Video | Seek beyond buffer kills + respawns ffmpeg (no session reuse) | `ui/src/app/player/[id]/page.tsx:754-765` |
| VID-6 | Medium | Video | `idle_timeout_secs` defaults to 30 min; abandoned tabs hold permits | `crates/server/src/main.rs:335-351` |
| VID-7 | Medium | Video | HLS segments served without byte-range, read fully into memory | `crates/server/src/routes.rs:4103-4178` |
| TXT-4 | Medium | Channels | No unread / last-read state per user per channel | (no `channel_read_state` table) |
| AUD-4 | Medium | Voice | No perfect-negotiation; duplicate/renegotiation offers destroy peer | `VoiceEngine.tsx:923-943` |
| AUD-5 | Medium | Voice | Mid-call mic grant can't reach already-connected peers | `VoiceEngine.tsx:378-400` |
| ARC-3 | Medium | Perf | DB pool lacks `acquire_timeout` / `min_connections` | `crates/db/src/lib.rs:75-78` |
| ARC-4 | Medium | Perf | N+1 lazy duration backfill on continue-watching hot path | `crates/server/src/routes.rs:3204-3210` |
| SEC-6 | Low | Security | `/vault` CSP allows `'unsafe-inline'`/`'unsafe-eval'` | `ui/src/middleware.ts:68` |
| SEC-7 | Low | Security | Minimum password length is 6 | `crates/server/src/user_pipeline.rs:10` |
| VLT-2 | Low | Vault | Client doesn't enforce a KDF floor on unlock (trusts server params) | `ui/src/features/rustyvault/crypto.ts:555-571` |
| VLT-3 | Low | Vault | Protected-action step-up uses account password (doc clarity) | `crates/server/src/rustyvault_host/handlers.rs:651-668` |
| VID-8 | Low | Video | Single rendition only — no ABR ladder / bandwidth adaptation | `crates/transcoder/src/session.rs:771-789` |
| VID-9 | Low | Video | Audio always re-encoded; multi-track audio dropped on transcode | `crates/transcoder/src/session.rs:675-682` |
| TXT-5 | Low | Channels | `Hello` rebuilt unfiltered on broadcast lag (re-leaks) | `crates/server/src/channels/ws.rs:288-293` |
| AUD-6 | Low | Voice | Mute is local-only; no remote muted indicator | `ui/src/lib/channelsContext.tsx:808-816` |
| AUD-7 | Low | Voice | ICE-server change doesn't refresh already-built peers | `VoiceEngine.tsx:559` |
| ARC-5 | Low | Supply chain | No `deny.toml` / `cargo audit` gate | repo root |
| ARC-6 | Low | Arch | Workspace lints claimed in `clippy.toml` but not configured | `clippy.toml`, root `Cargo.toml` |
| ARC-7 | Low | Perf | AI per-library scans materialize full libraries in memory | `crates/server/src/ai_assistant/tools.rs:7886`, `:7967` |
| SEC-8 | Info | Security | Login rate-limit identity collapses behind reverse proxy | `crates/server/src/routes.rs:113-142` |
| SEC-9 | Info | Security | TMDB API key stored plaintext in settings table | `crates/server/src/routes.rs:5124-5127` |
| VLT-4 | Info | Vault | Re-key re-encrypts every item rather than pure envelope re-wrap | `ui/.../RustyVaultPage.tsx:1201-1239` |
| VLT-5 | Info | Vault | `/vault` HTTPS enforcement is deployment-layer, not in app code | `crates/server/src` (no app-layer guard) |
| AUD-8 | Info | Voice | Transcript fallback correctly wired; raw audio never persisted | `VoiceEngine.tsx:669-856`, `handlers.rs:2117-2307` |
| ARC-8 | Info | Reliability | `panic!` in DB backend detection (startup-only) | `crates/db/src/lib.rs:38,52` |

---

## 3. Security & Secrets

**Verdict:** The security core is fundamentally sound — Argon2id hashing, parameterized SQL throughout (zero injection found), type-safe `AuthUser`/`AdminUser` authorization, robust path-traversal defense (`canonicalize` + `starts_with`), and well-hardened vault session isolation. The weaknesses cluster in two places: a broken-access-control bug on backups (SEC-1), and browser-side token handling + missing security headers (SEC-3/4/5) that turn any XSS into full account takeover.

### Findings

- **SEC-1 [High] — Backup/restore reachable by any authenticated user.** `crates/server/src/backups/handlers.rs:13-58`. All five `/system/backups` handlers take `_auth: AuthUser` (verified: `use crate::auth::AuthUser;`, never `AdminUser`). `restore_backup` (`:49`) runs a full DB restore from `db.sql` via `service::restore_backup`. A regular "user"-role account can list/create backup policies, trigger backups, and **overwrite the entire Postgres database** (killing the server mid-restore). **Fix:** change the extractor to `AdminUser` in all five handlers.

- **SEC-2 [Medium] — Unauthenticated subtitle file read.** `serve_subtitle` (`routes.rs:4627`, wired at `:288` in `stream_router()`) takes no `AuthUser`/token and the router applies no auth layer — unlike `stream_file_range`/`hls_*`, which require tokens. Any unauthenticated client can read bytes of any file under a library root given its hex-encoded path (predictable from the authenticated subtitles listing). Confined to library roots by `canonicalize`, but no auth and no per-user scoping. **Fix:** require `AuthUser` (or a scoped stream token) and call `validate_path_in_user_libraries`.

- **SEC-3 [Medium] — JWT in `localStorage` + non-HttpOnly cookie.** `ui/src/lib/browserAuth.ts:19-20` sets both `localStorage` and a `document.cookie` (which cannot be HttpOnly). The 24h bearer token is fully readable by JavaScript; combined with SEC-5 (no CSP on the app) any XSS yields exfiltratable takeover. **Fix:** issue the auth cookie server-side as `HttpOnly; Secure; SameSite=Lax` and stop mirroring into `localStorage`.

- **SEC-4 [Medium] — Auth cookie missing `Secure`.** `browserAuth.ts:5` emits `Path=/; Max-Age=…; SameSite=Lax` (no `Secure`). Home servers are frequently accessed over plaintext HTTP on the LAN, so the token is sniffable. **Fix:** add `Secure` (and serve over TLS); ideally fold into the server-side cookie of SEC-3.

- **SEC-5 [Medium] — Security headers only on `/vault`.** `ui/src/middleware.ts:95-128` applies CSP, `X-Frame-Options: DENY`, etc. only when `isVaultRoute(pathname)`; every other route returns a bare `NextResponse.next()`, and `next.config.js` sets no `headers()`. The main app ships with **no CSP and no clickjacking protection**. **Fix:** apply the security-header set to all routes; tighten `script-src` away from `'unsafe-inline'`/`'unsafe-eval'` for non-vault routes.

- **SEC-6 [Low] — `/vault` CSP allows `'unsafe-inline'`/`'unsafe-eval'`.** `middleware.ts:68`. On the most sensitive UI this weakens XSS containment. `'wasm-unsafe-eval'` is justified for Argon2 Wasm; the broad `'unsafe-eval'`/`'unsafe-inline'` are not. **Fix:** drop `'unsafe-eval'` (keep `'wasm-unsafe-eval'`), replace `'unsafe-inline'` with nonces/hashes.

- **SEC-7 [Low] — Minimum password length 6.** `user_pipeline.rs:10`. Permits weak credentials; online guessing is only loosely bounded by the per-account login limiter. **Fix:** raise to ≥10–12 and/or add a breached-password (zxcvbn/HIBP) check.

- **SEC-8 [Info] — Login rate-limit identity collapses behind the proxy.** `routes.rs:113-142` prefers the TCP peer over `X-Forwarded-For`; behind Caddy the peer is always localhost, so the bucket is effectively per-username-global. Adequate against per-account stuffing, but ignores true client IP. **Fix:** derive client IP from `X-Forwarded-For` (trusting only the proxy hop) when a trusted proxy is configured.

- **SEC-9 [Info] — TMDB API key plaintext in settings.** `routes.rs:5124-5127` / `artwork.rs:37`. A third-party key sits unencrypted in the DB. Mitigations are good: the admin GET returns only a masked `key_preview`, and the route is `AdminUser`-gated. **Fix (optional):** encrypt at rest or load only from env.

### What's solid
- Argon2id password hashing with per-password `OsRng` salt; verification via `Argon2::default().verify_password` (`crates/db/src/repo/users.rs:351-364`).
- HS256 JWT via `Validation::default()` (validates `exp`, rejects `alg:none`); stream tokens pin `aud:"stream"` and are scoped to a specific `file_id`. Legacy query-string token explicitly rejected.
- JWT secret from `RUSTFIN_JWT_SECRET`; absent → random per-boot UUID with a warning (fails safe, never a hardcoded default) (`main.rs:272-282`). All `"test-secret"` usages confined to `#[cfg(test)]`.
- **No SQL injection found** — every dynamic `IN (...)` uses the centralized `dollar_placeholders` helper with `$N` + `.bind()`.
- Type-safe authorization extractors; user management, AI admin, host directories, TMDB config, setup reset all `AdminUser`-gated; per-object access via `ensure_library_access`/`get_accessible_media_file` (no IDOR in items/playback/downloads/avatar).
- WebSocket security: strict same-origin check + mandatory first-message JWT auth + connect rate limiting (`watch_party/ws.rs`, mirrored in `channels/ws.rs`).
- Path traversal closed via `canonicalize()` + `starts_with(lib_root)` on streaming, subtitles, avatars.
- Setup flow: owner token with **constant-time** comparison, blocks post-`setup_completed`, sliding expiry.
- Secrets hygiene: no committed/hardcoded secrets, none in `tracing`/`println` (repo-wide grep), only masked previews in responses; `.gitignore` excludes `.env*` and `.rustyfin.runtime.env`; installer generates per-deploy random secrets.
- Caddy terminates TLS with HSTS; no permissive CORS layer (safe default).

---

## 4. RustyVault Encryption

**Verdict:** The encryption design is genuinely client-side and **effectively zero-knowledge for data at rest**: the master password and unwrapped vault key never leave the browser, and the server only ever receives/stores the wrapped vault key, KDF salt+params, AES-GCM nonces, and ciphertext+tags. The server crate contains **zero** AES/GCM/decryption code — only algorithm-name constants and hex decoding. The KDF is sane and server-floor-enforced; AES-256-GCM uses fresh 96-bit random nonces with verified tags and AAD domain separation; envelope re-wrap on password change is correct. The one real defect is in the browser extension (VLT-1).

### Crypto design (verified)

- **master password** + random 16-byte salt → **Argon2id** (m=65536 KiB, t=3, p=4, 32-byte out) → `masterMaterial` (native WebCrypto when available, else shipped portable `argon2.wasm`/`argon2.js`).
- `masterMaterial` → **HKDF-SHA-256**, split by distinct `info`:
  - `info="rustyvault-wrap-v1"` → **wrap_key** (AES-256-GCM).
  - `info="rustyvault-index-v1"` → **index_key** (HMAC-SHA-256, for blinded URI lookup hashes so the server can match by domain without seeing URLs).
- A random **data_key** (AES-256-GCM) is generated client-side, **wrapped** with `wrap_key` under a fresh 96-bit nonce → only the wrapped form + salt + params + nonce reach the server.
- Each item has summary + payload blobs, each AES-256-GCM with a fresh 96-bit random nonce, 128-bit tag, and **AAD** = `rustyvault:{userId}:{itemId}:{summary|payload}:v{version}`. Stored server-side as opaque `BYTEA`.
- Unlock reverses this fully locally; a wrong password makes GCM unwrap throw. The server never sees the password, masterMaterial, wrap_key, index_key, or data_key.

### Findings

- **VLT-1 [High] — Cross-origin iframe block not enforced on credential fill.** `extensions/rustyvault-webext/src/background/index.ts:640-668`. The `fill-item` handler computes its gate via `resolvePagePolicy(tabId)` with **no frame override**; `policyInputForTab` (`:254`) sets both `url` and `topLevelUrl` to `tab.url` and `isTopFrame: true`, so `evaluatePagePolicy` always derives `crossOriginIframe = false`. The decrypted secret is then delivered to a specific subframe via `preferredFrameId` while the content script runs in **all frames** (`allFrames: true`). A login form inside a cross-origin iframe — the exact case `policy.ts` was written to block (`crossOriginIframe → 'untrusted_iframe'`) — can receive the plaintext credentials. This contradicts the AGENTS.md hard-boundary claim. **Fix:** evaluate the policy against the **actual target frame's** URL/origin and `isTopFrame` (use `sender.frameId` + `chrome.webNavigation.getFrame`), and refuse to fill when the resolved frame is cross-origin to the top document.

- **VLT-2 [Low] — Client doesn't enforce a KDF floor on unlock.** `crypto.ts:555-571` checks only algorithm *names* and feeds server-provided `kdf_memory_kib/iterations/parallelism` straight into derivation; `normalizeArgon2Params` only floors missing/non-positive values, not values *below* policy. A hostile server (the threat zero-knowledge cares about) could serve weakened params. Practical risk is low (a weakened record can't decrypt the existing vault), and the server *does* enforce a floor on writes (`handlers.rs:64-71`) — but not on reads. **Fix:** reject unlock params below the floor (m≥65536, t≥3, p≥4) client-side before deriving.

- **VLT-3 [Low] — Protected-action step-up uses the account password.** `rustyvault_host/handlers.rs:651-668`. `challenge_protected_action` verifies `body.current_password` against the Rustyfin login hash, *not* the vault master password. This does not break vault zero-knowledge (the vault key isn't derivable from it), but reviewers may confuse the two secrets. **Fix:** document that these are distinct; no crypto change needed.

- **VLT-4 [Info] — Re-key re-encrypts every item.** `RustyVaultPage.tsx:1201-1239`. Password change correctly re-wraps the same `data_key` (envelope), but also re-encrypts every item. Since the data key is unchanged, item re-encryption isn't cryptographically required; it's slower and can fail mid-loop leaving mixed revisions (safe — fresh nonces — but unnecessary). **Fix:** re-wrap only unless rotating the data key.

- **VLT-5 [Info] — `/vault` HTTPS enforcement is deployment-layer.** No app-layer `x-forwarded-proto`/HTTPS guard exists; the guarantee is asserted via Caddy edge config and enforced client-side by `getRustyVaultCryptoReadiness` refusing non-secure contexts (`crypto.ts:474-482`). If an operator misconfigures the edge, the API would serve over HTTP and only the browser check protects users. **Fix (hardening):** add an app-layer reject of non-HTTPS forwarded requests on `/vault`.

### What's solid
- True zero-knowledge at rest: DB columns are salt, KDF params, wrap nonce, wrapped key, and ciphertext+nonce only — no plaintext, key, or password column anywhere. Server crate has no decryption code.
- KDF exceeds OWASP minimums and is floor-enforced on every wrapped-key write.
- Fresh `crypto.getRandomValues` 96-bit nonces per blob and per wrap → no GCM nonce-reuse exposure; tags verified (fails closed on tamper).
- Correct HKDF domain separation (distinct `info`; index uses a different algorithm).
- Correct envelope re-wrap on password change; no recovery/escrow backdoor (lost master password is unrecoverable by design).
- Strong surrounding session hygiene: refresh-token rotation with replay detection + family revocation, short-lived `rustyvault_session` JWTs, single-use step-up tokens, strict cross-user token-mixing rejection, `no-store`/`nosniff`/`no-referrer` on all vault responses, per-action rate limiting.
- Web UI hard-refuses to operate outside a secure context.

---

## 5. Video Playback Pipeline

**Verdict:** Playback is functionally solid but **fundamentally inefficient for a low-latency goal.** The server has a well-written direct-play/remux decision engine (`decision.rs`) and a complete HTTP byte-range file server (`streaming.rs`) — but the video player **never uses either**. It unconditionally creates an HLS transcode session for every play, every seek-out-of-buffer, and every quality change, even for a stock H.264/AAC MP4 the browser could play byte-for-byte. There is **no `-c copy` remux path anywhere**, and the HLS encode always re-encodes video with `libx264 -preset veryfast` (no `zerolatency`/GOP tuning) into 4s classic (non-LL) HLS. Realistic time-to-first-frame is ~1–3s+ of pure server CPU for content that should start near-instantly. Hardware accel is real (probed, with genuine software fallback) and resource hygiene is good — but the core "avoid needless transcode" requirement is unmet.

### Findings

- **VID-1 [Critical] — Video player never direct-plays.** `ui/src/app/player/[id]/page.tsx:499-702`; rooms player identical (`useRoomPlayback.ts:383,500-503`). Every path calls `startHls()` → `create_playback_session` → ffmpeg. The descriptor exposes `direct_url`, but it is consumed **only** by the audio/music player (verified: `direct_url` appears in `rooms/components/AudioPlayer.tsx`, never in the video players, which only `loadSource(data.hls_url)` / `video.src = data.hls_url`). A compatible MP4 that the browser can play natively (instant start, byte-range seek, zero server CPU) is instead re-encoded + segmented — the single biggest latency/CPU cost, scaling per concurrent viewer. **Fix:** on play, run the existing `decide()`; if `DirectPlay`, set `video.src = direct_url`; only true incompatibility should hit HLS.

- **VID-2 [Critical] — `decision.rs` is dead code.** `crates/transcoder/src/decision.rs`. Verified: `decide(` has callers **only** inside the module's own `#[cfg(test)]` tests (`:170-216`); zero production callers. `create_playback_session` never probes codecs or calls `decide()` — it unconditionally spawns a transcode. The "is a transcode needed?" brain is disconnected from the body. **Fix:** wire `decide()` into `create_playback_session` (or a `/playback/decision` endpoint) returning the play method.

- **VID-3 [High] — No stream-copy / remux path.** `crates/transcoder/src/session.rs:707-753`. Verified: no `-c copy`/`c:v copy`/`c:a copy` anywhere in transcoder or server. `spawn_ffmpeg` always selects a real video encoder and `-c:a aac`. Container-only mismatches (H.264+AAC in MKV needing MP4/TS packaging) burn a full re-encode instead of `ffmpeg -c copy`. **Fix:** add a remux mode emitting `-c:v copy -c:a copy` (or copy video, transcode only incompatible audio) into fMP4/TS HLS.

- **VID-4 [High] — Encoder preset/GOP not tuned; classic 4s HLS, no LL-HLS.** `session.rs:743-789`. Software path uses `-preset veryfast -crf 23` with no `-tune zerolatency`, no GOP cap, no `-sc_threshold 0`; HW path sets bitrate but no low-latency preset; HLS is classic `event` with 4s segments, no `+low_latency`/partial segments; client `lowLatencyMode:false`. Time-to-first-frame ≈ time to fully encode the first 4s GOP before the playlist appears (often 1–3s+ on a busy CPU). **Fix:** when transcode is genuinely needed, use 2s (or LL-HLS partial) segments, `-tune zerolatency` / nvenc low-latency preset, cap GOP to one keyframe per segment.

- **VID-5 [Medium] — Seek beyond buffer kills + respawns ffmpeg.** `player page:754-765` → `create_session` with `start_time_secs`; server applies fast input `-ss` before `-i` (good) but `stop_sessions_for_owner_file` kills the old session and wipes its output. Every out-of-buffer seek pays the full TTFF again. Mitigated by a large client buffer (`maxBufferLength:600`). **Fix:** pair input-seek restart with shorter segments (VID-4); optionally briefly cache sessions for re-seek.

- **VID-6 [Medium] — `idle_timeout_secs` defaults to 30 min.** `main.rs:335-351`. Prod forces ≥60 but defaults to **1800s** (overriding the `lib.rs` default of 60). `last_ping` is bumped only on segment/master fetches, so a hard tab-close/network-drop holds the ffmpeg process + concurrency permit (default 4 total) for up to 30 minutes — a few abandoned tabs can starve real viewers (`MaxTranscodesReached`). **Fix:** lower the idle window to ~60–120s.

- **VID-7 [Medium] — HLS segments served without byte-range, read fully into memory.** `routes.rs:4103-4178` does `tokio::fs::read` of each multi-MB segment with `no-store` and no `Accept-Ranges`, unlike the direct-file route. Adds memory pressure and redundant transfers under concurrency. **Fix:** stream with `ReaderStream` and allow short-lived caching of immutable `.ts` segments.

- **VID-8 [Low] — Single rendition; no ABR.** `session.rs:771-789` emits one variant; quality is a manual `target_height` that restarts ffmpeg. A bandwidth drop stalls rather than down-shifts. Acceptable on a LAN. **Fix (optional):** emit a small 1080/720/480 ladder.

- **VID-9 [Low] — Audio always re-encoded; multi-track audio dropped.** `session.rs:675-682` hardcodes `-map 0:a:0? ... -c:a aac -ac 2` — first audio only, 5.1 downmixed, already-AAC re-encoded, and no track-selection plumbed through despite `ffprobe.rs` parsing all tracks. **Fix:** add `audio_index`/`audio_lang` to the session request and `-c:a copy` when compatible.

### What's solid
- Real, probed hardware-accel selection (a `testsrc2` encode probe per accelerator, not "assumed if listed") with a genuine software fallback that resets output and retries.
- Concurrency correctly bounded by an `OwnedSemaphorePermit` held for the session lifetime; same-user/same-file sessions replaced to prevent slot leaks.
- Lifecycle hygiene on the happy path: client `stopSession` on unmount, server reaps exited processes, `Drop` kills orphans, idle eviction wipes output dirs.
- The direct-file Range server is correct and security-conscious (token scoping, path confinement, proper `206`/`416`).
- ffprobe parsing is robust (duration fallbacks, fractional frame rates, full track disposition) — the metadata to *do* direct-play already exists.
- HW encode edge-cases handled (NVENC `yuv420p` for Main10 sources; VAAPI/QSV decode-in-sw-then-upload for 10-bit HDR safety).

---

## 6. Text Channels

**Verdict:** **Partial — channels are visible to all users, but too visible.** The core flow works: messages persist with correct monotonic ordering, broadcast reaches every connected client, and create/rename/delete propagate live. But **private channels leak**: the WebSocket `Hello` and live `ChannelCreated`/`ChannelUpdated` events send *every* channel (including `is_private`) to *every* user with no role filter, while the HTTP `GET /channels` endpoint correctly filters them. Since the UI builds its sidebar entirely from the WebSocket path, non-admins see private channels they shouldn't, and the global broadcast bus delivers private-channel message bodies to non-admins too. For *public* channels — the common case — visibility is correct.

### How visibility actually works (the two paths disagree)
- **HTTP `GET /api/v1/channels`** (`handlers.rs:561-575`): unfiltered query, then keeps rows where `!c.is_private || auth.role == "admin"` (verified at `:571`). Correct.
- **WebSocket `Hello`** (`ws.rs:688-700`): runs the *same unfiltered* `list_channels` and returns **all** rows with no role filter (verified: `:689` calls `list_channels`, `:698` maps `is_private` straight through). The browser sidebar is populated only from this path → every user sees every channel.

### Findings

- **TXT-1 [High] — WS `Hello` leaks private channels to non-admins.** `channels/ws.rs:688-700`. `build_hello` calls unfiltered `list_channels` and maps every row including `is_private`. The UI feeds the sidebar exclusively from this payload with no `is_private` re-check. Non-admins see the names/existence of all private channels (the create modal advertises private as "admins only"). **Fix:** filter by the authenticated role inside `build_hello` (`.filter(|c| !c.is_private || role == "admin")`), mirroring the HTTP handler.

- **TXT-2 [High] — Global broadcast delivers private-channel messages to non-members.** `manager.rs:13-14`, `ws.rs:281-296`. `ChannelManager` has a single process-wide `broadcast::Sender` with no per-channel subscriber set; every socket subscribes to the one bus and forwards every event. `NewMessage`/`ChannelCreated`/`MessageDeleted` for private channels reach non-admin clients, which only filter by `channel_id === activeChannel.id`, not by access. **Fix:** gate fan-out by access — key subscribers per channel, or drop private-channel events at the per-socket send path when `role != "admin"`.

- **TXT-3 [High] — No membership model; "private" is binary admin-only.** `008_channels.sql` (no `channel_member` table anywhere). A private channel is visible/postable only to admins; a regular user can never be granted one. This both answers the visibility question and flags a likely product gap versus the "watch rooms / audio channels you can join together" intent. **Fix (product decision):** add `channel_member(channel_id, user_id)` and switch every `is_private && role != admin` check (plus Hello/broadcast) to membership-based — or document the admin-only model as intended.

- **TXT-4 [Medium] — No unread / last-read state.** No `channel_read_state`/`last_read` table exists; the AI tool `channels_list_unread_activity` is documented as not providing true unread counts. The design's "resume / see new activity" goal has no backing data. **Fix:** add `channel_read_state(user_id, channel_id, last_read_sort_seq)` and compute unread as `count(sort_seq > last_read)`.

- **TXT-5 [Low] — `Hello` rebuilt unfiltered on broadcast lag.** `ws.rs:288-293` resends a full (unfiltered) `Hello` on `RecvError::Lagged`, re-applying TXT-1's leak and re-triggering client voice rejoin. **Fix:** covered once TXT-1 filters `build_hello` centrally.

### What's solid
- Public text channels work as intended; all users receive them and all messages.
- Message persistence + ordering is robust: monotonic unique-indexed `BIGINT sort_seq`, correct ASC render, keyset pagination tie-breaking on `sort_seq` (same-timestamp messages page correctly).
- No lost-message window: DB commit precedes WS broadcast on both paths; missed live events are recoverable via `get_messages`.
- Connection registry is sound for fan-out: per-user/per-connection `mpsc` map supports multi-device and self-evicts dead connections; reconnect with backoff + `Hello` resync.
- Write/read authz is enforced within the admin-gated model (`get_accessible_channel`, WS `SendMessage`, `get_messages`); message delete checks ownership-or-admin with no IDOR.
- Live propagation of create/rename/delete + presence is correct, including on-disk attachment cleanup via FK cascade.

> Note: TXT-1 and TXT-2 share one root cause (the WS layer ignores `is_private`/role). A single role filter in `build_hello` + the per-socket send path fixes both.

---

## 7. Audio / Voice Channels

**Verdict:** **Partial — works for the common case (2–4 people, same network or with a real TURN server), not reliable beyond it.** The signaling relay is correct and the full-mesh offer/answer/ICE exchange is sound, with deterministic initiator selection (`localeCompare`) that cleanly avoids initial glare. Media acquisition, mute, speaking detection, per-peer volume, and remote playback are all properly implemented. Two real gaps: **no connection-failure recovery at all** (any ICE failure or network blip permanently kills that peer link), and **full-mesh O(n²)** caps practical channel size at a handful of users. TURN is correctly surfaced when configured, but the STUN-only default silently fails for symmetric-NAT peers. Transcripts are correct and raw audio is never persisted.

### Voice flow (verified)
Full-mesh P2P; the Rust server is a pure WebSocket signaling relay (`channels/ws.rs` forwards `rtc_offer`/`rtc_answer`/`rtc_ice` 1:1, gated by `voice_channel_has_pair`). No SFU; no media touches the server. For N users: N(N−1)/2 connections. Initiator chosen by `localeCompare` so exactly one side offers. ICE/TURN comes from `/runtime-config`. Transcript: browser SpeechRecognition primary, MediaRecorder audio-upload fallback on stop, server decodes audio in-memory → text only, finalized as one server-authored markdown merged by timestamp.

### Findings

- **AUD-1 [High] — No ICE/connection-failure recovery.** `VoiceEngine.tsx:553-579`. `createPeer` sets only `onicecandidate`/`ontrack`; no `onconnectionstatechange`/`oniceconnectionstatechange`/`restartIce`/`onnegotiationneeded` anywhere. The WS auto-reconnect only revives signaling, not the peer connections. A transient drop or Wi-Fi handoff leaves the peer `failed`/`disconnected` forever with dead audio until the user fully leaves and rejoins. **Fix:** add `onconnectionstatechange`; on `failed` call `restartIce()` or recreate + re-offer.

- **AUD-2 [High] — Full-mesh doesn't scale.** `VoiceEngine.tsx:581-611`, `ws.rs:495-619`. Each client opens one PC per other member and encodes/uploads N−1 streams. Fine for 2–5 people (the stated use), but a 10-person channel = 45 connections and 9 upstream encodes per browser. **Fix:** acceptable for small N — document the cap, or introduce an SFU (mediasoup/LiveKit) if larger rooms are ever needed.

- **AUD-3 [High] — STUN-only default strands symmetric-NAT peers.** `channelsContext.tsx:214-216`, `runtime-config/route.ts:136-169`. Default `voiceIceServers` is Google STUN only; host-derived TURN URLs are appended only when *both* `TURN_USERNAME` and `TURN_CREDENTIAL` are set. Two users behind symmetric NAT/CGNAT gather only `srflx`/`host` candidates, never connect, and (per AUD-1) never recover or surface an error. **Fix:** operationally deploy a TURN server + set the env vars; in-app, surface a "couldn't connect" indicator when a peer stays non-connected.

- **AUD-4 [Medium] — No perfect-negotiation; duplicate offers destroy the peer.** `VoiceEngine.tsx:923-943`. On `rtc_offer` the handler unconditionally `createPeer` → `existing.close()`s the live PC; no `signalingState` check, no rollback. Initial glare is avoided only because exactly one side offers. Any second/renegotiation offer tears down working audio. **Fix:** implement the standard perfect-negotiation pattern.

- **AUD-5 [Medium] — Mid-call mic grant can't reach connected peers.** `VoiceEngine.tsx:378-400`. A listen-only user who later grants a mic does `addTrack` (firing `negotiationneeded`), but nothing handles it and no new offer is sent — so peers already connected never hear them. **Fix:** trigger renegotiation when a track is added to an established connection (with AUD-4's pattern).

- **AUD-6 [Low] — Mute is local-only.** `channelsContext.tsx:808-816` flips `track.enabled` (correct — stops transmission), but there's no signaling so peers get no muted indicator. **Fix:** add a mute presence event to the protocol.

- **AUD-7 [Low] — ICE-server change doesn't refresh built peers.** `VoiceEngine.tsx:559` reads `iceServers` only at construction. If `/runtime-config` resolves *after* early peers are built, those peers keep STUN-only config. **Fix:** recreate peers (or `setConfiguration`) when `iceServers` changes.

### What's solid
- Signaling relay correctness: pair-membership validation, per-connection targeting (not broadcast), symmetric join/leave so joiners mesh with existing members and leavers are cleaned up (unit-tested in `manager.rs`).
- Deterministic initiator selection cleanly prevents initial offer glare.
- ICE candidate buffering: candidates arriving before `remoteDescription` are queued and flushed (avoids a classic race).
- Robust playback: hidden autoplay elements with autoplay-unblock nudges, AudioContext resume, per-peer volume, deafen, `setSinkId` output routing.
- Runtime-configurable ICE with sane sanitization, dedup, host-derivation, localhost exclusion.
- **AUD-8 [Info]** Transcript fallback correctly wired on client and server; raw mic audio leaves the browser only when SpeechRecognition is unavailable, is processed in-memory, and is **never written to disk** — only text is persisted. Good privacy posture.

---

## 8. Architecture, Performance & CI

**Verdict:** A well-architected, disciplined codebase — clean crate boundaries, an exemplary repo layer (parameterized `IN (...)` batching, 154 indexes across 58 migrations), and near-zero `unwrap()`/`panic!` in request paths. The single biggest lever is **wiring the existing `debian_native_gates.sh` (fmt/clippy -D warnings/tests/UI build) into GitHub Actions** — that gate already exists but the only CI workflows are `ai-judge-*`, so the main workspace has no automated build/test/lint gate on PRs. Second concern: **proportionality** — ~65% of the server crate is AI-assistant code, with a 15k-line `orchestrator.rs`.

### Findings

- **ARC-1 [High] — No CI gate for the main workspace.** `.github/workflows/` has only `ai-judge-smoke.yml`/`ai-judge-release.yml`, both running the AI eval harness. The real gate `scripts/ci/debian_native_gates.sh:516-548` (`cargo fmt --check`, `clippy -D warnings`, crate tests, `npm run lint`/`build`) is invoked nowhere in Actions. A 160k-LOC monorepo can merge to `main` with compile errors, clippy regressions, failed tests, or a broken UI build. **Fix:** add a `ci.yml` on `pull_request`/`push` running `debian_native_gates.sh` (already CI-ready).

- **ARC-2 [High] — AI subsystem disproportionate; God-files.** `ai_assistant/orchestrator.rs` is 15,169 LOC, `tools.rs` 12,668, `replies.rs` ~8,000. AI server code is ~77k LOC = 65.6% of the 118k-LOC server crate, while the actual inference crate `ai-agent` is 1,863 LOC. A 15k-line file can't be reviewed or refactored safely. Structural debt, not a correctness bug. **Fix:** split `orchestrator.rs` by phase and `tools.rs` by tool domain into submodules.

- **ARC-3 [Medium] — DB pool lacks `acquire_timeout`/`min_connections`.** `crates/db/src/lib.rs:75-78` sets only `.max_connections(15)`. No `acquire_timeout` means requests can wait indefinitely under pool exhaustion; no `min_connections` means cold-start latency after idle (contrast the HTTP client at `main.rs:285-286`, which sets timeouts). **Fix:** add `.acquire_timeout(5–10s)`, `.min_connections(2–5)`.

- **ARC-4 [Medium] — N+1 duration backfill on continue-watching.** `routes.rs:3204-3210`. Inside `for row in rows`, when `row.duration_ms.is_none()` it calls `get_item_file_id` + `resolve_and_persist_media_duration_ms` per row. The base query resolves duration via subquery only for **episodes**, so **movies always fall through** to per-row round-trips — on *the* primary social-continuation endpoint. Bounded by `limit`. **Fix:** resolve movie duration in the base query, or batch the missing IDs into one `IN (...)` lookup (the function already batches library settings the same way).

- **ARC-5 [Low] — No `deny.toml` / supply-chain gate.** `Cargo.lock` is committed (good) but there's no `cargo-deny`/`cargo-audit` config or CI step, so a new RUSTSEC advisory in the network-facing dep tree raises no alert. **Fix:** add `deny.toml` + a CI job (cheap once ARC-1 adds Actions).

- **ARC-6 [Low] — Workspace lints claimed but not configured.** `clippy.toml` is empty with a comment saying lint levels are set in the workspace `Cargo.toml`, but no `[workspace.lints]` exists. Clippy is `-D warnings` only inside the unwired gate, and only for 4 crates. **Fix:** add `[workspace.lints.clippy]`/`[workspace.lints.rust]` + `lints.workspace = true` per crate.

- **ARC-7 [Low] — AI per-library scans materialize libraries in memory.** `tools.rs:7886`, `:7967` (`libraries_find_duplicate_titles`, `libraries_list_missing_metadata`) load every item of every accessible library into RAM then truncate to 12. On a large server this is an unbounded fetch per AI invocation. **Fix:** push dedup/missing-field logic into SQL with `LIMIT`.

- **ARC-8 [Info] — `panic!` in DB backend detection.** `crates/db/src/lib.rs:38,52` panic on a non-Postgres URL. Startup/config-time only (not request-reachable) — acceptable fail-fast, but a `Result` would be cleaner.

### Quick wins
- Add `ci.yml` running `scripts/ci/debian_native_gates.sh` on PRs (ARC-1) — highest value-per-effort change in the repo.
- Add `.acquire_timeout(...)` + `.min_connections(...)` to `db/lib.rs:75` (ARC-3) — 2 lines.
- Batch continue-watching duration lookups (ARC-4) — mirror the existing batching in the same function.
- Drop in `deny.toml` + `cargo audit` (ARC-5).
- Add `[workspace.lints]` to make `clippy.toml`'s claim real (ARC-6).

### What's solid
- Repo/query layer is exemplary: every multi-value query uses `dollar_placeholders` + `IN (...)` binding; 154 indexes across 58 migrations with dedicated performance-index migrations.
- Hot-path error handling is clean: zero `unwrap()` in non-test request-handler code; the bulk of `unwrap`/`expect`/`panic` are in tests/startup/tool-registration.
- Blocking work handled correctly (`spawn_blocking` for the synchronous artwork scan; shared `reqwest` client with timeouts; no `thread::sleep` on async paths; no unbounded channels).
- Clean crate boundaries; RustyVault feature-gated and runtime-disablable; GPU link crates excluded from the default build.
- Frontend core flow correct: homepage fetches continue-watching + rooms + calendar via a single `Promise.all` (no waterfall).
- Tests where it counts: server has 69 test files + integration + AI eval/judge harness; transcoder/db/scanner covered. (Thin: `youtube-agent`, `transcription-agent`, `servers-agent`, `ai-evals` have no in-crate tests — `servers-agent`, which drives systemd, would most benefit.)

---

## 9. Prioritized Remediation Roadmap

### P0 — fix now (security + the headline UX defect)
1. **SEC-1** — gate `/system/backups` handlers with `AdminUser` (one-word change × 5; prevents DB-overwrite by any user).
2. **TXT-1 + TXT-2** — role-filter `build_hello` and the per-socket event send path (one central fix stops private-channel name *and* message leakage).
3. **VID-1 + VID-2** — wire `decide()` into the playback flow and use `direct_url` for compatible content (the biggest latency/CPU win; the engine already exists and is tested).
4. **VLT-1** — evaluate the extension fill policy against the real target frame; refuse cross-origin-iframe fills.

### P1 — soon (hardening + reliability)
5. **SEC-3/4/5** — server-side `HttpOnly; Secure; SameSite` auth cookie; stop using `localStorage`; apply security headers app-wide.
6. **VID-3/VID-4** — add a `-c copy` remux path and tune the encoder/segments for fast startup when a transcode is genuinely needed.
7. **AUD-1 + AUD-3** — add connection-state recovery (`restartIce`) and deploy/surface TURN; together they fix most "voice randomly stops working" reports.
8. **ARC-1** — add the main-workspace CI gate.
9. **VID-6** — drop the idle-session timeout so abandoned tabs don't hold transcode permits.

### P2 — improvement backlog
10. **SEC-2** (auth subtitle reads), **VID-5/7** (seek/segment streaming), **TXT-4** (unread state), **AUD-4/5** (perfect-negotiation), **ARC-3/4** (pool config, continue-watching N+1), **ARC-5/6** (deny.toml, workspace lints).
11. **TXT-3** — decide whether private channels should support per-user membership (product call); **AUD-2** — SFU only if larger voice rooms become a goal; **ARC-2** — split the AI God-files.
12. Low/Info: SEC-6/7/9, VLT-2/3/4/5, VID-8/9, AUD-6/7, ARC-7/8.

---

## 10. Direct Answers to the Owner's Questions

- **Can it be improved?** Yes — the architecture is healthy, but the biggest single improvement is making video direct-play work (VID-1/2): the code to do it already exists and is simply not called. After that, wiring CI (ARC-1) and closing the channel/backup access-control gaps.
- **Is it secure enough?** The core is strong (Argon2id, parameterized SQL, zero-knowledge vault, no committed/logged secrets). It is **not** there yet because of SEC-1 (backup privesc), VLT-1 (extension fill), and the browser-token/CSP gaps (SEC-3/4/5). Fix the P0/P1 security items and it is in good shape.
- **Is video playback proper, efficient, low-latency, snappy?** Proper and correct, but **not efficient or low-latency**: it transcodes everything, even content the browser could play natively, into classic 4s HLS with an untuned encoder. This is the most impactful thing to fix for the stated experience goal.
- **Are text channels correct, showing for all users?** Public channels: yes. But private channels are over-exposed — every user currently sees them and their live messages via the WebSocket path (TXT-1/2). There is also no per-user membership model (TXT-3).
- **Do audio channels work?** Yes for 2–5 people on the same network or with a TURN server configured. They are unreliable beyond that: no failure recovery (AUD-1), full-mesh scaling limits (AUD-2), and STUN-only-default NAT failures (AUD-3).
- **Is the vault encryption secure?** Yes — genuinely client-side / zero-knowledge at rest, with sound primitives (Argon2id → HKDF → AES-256-GCM, fresh nonces, verified tags, AAD binding). The one real hole is the extension fill path (VLT-1), not the cryptography itself.
- **Are any secrets left in plaintext?** No committed secrets, no secret logging, no hardcoded JWT default. Two items of note: the **TMDB third-party API key** is stored unencrypted in the settings table (masked in responses), and the **session JWT** lives in `localStorage` + a non-HttpOnly/non-Secure cookie (readable by JS, sniffable over HTTP).

---

*Generated by a six-agent parallel audit; Critical/top-High findings independently re-verified against source at `4effba6`.*
