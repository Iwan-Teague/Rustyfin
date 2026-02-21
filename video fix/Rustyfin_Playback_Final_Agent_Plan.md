# Rustyfin Playback: Final AI-Agent Implementation Plan (build 8024266)

This is the **single source of truth** plan an AI coding agent should follow to fix Rustyfin so that:
- Libraries reliably detect media in configured directories.
- Selecting a media item actually **plays video** (Direct Play and/or HLS Transcode).
- Playback is implemented in a **modern, secure** way (no bearer-by-URL surprises).

This plan is derived from (and should be cross-checked against) these two documents in the same directory:
- `Rustyfin_Playback_Deep_Dive_8024266.md` (functional diagnosis & baseline pipeline)
- `Rustyfin_Playback_Deep_Dive_Critique_8024266.md` (verified corrections + security hardening)

## Non-negotiable outcomes (Definition of Done)
**Playback works**
- Clicking **Direct Play** starts playback in-browser for web-playable formats.
- Clicking **Transcode (HLS)** starts playback for *any* common format via HLS.
- Player UI shows actionable errors instead of silently doing nothing.

**Security is not optional**
- `/stream/hls/*` endpoints **require authentication/authorization** (no unauthenticated playlist/segment access).
- Long-lived auth JWTs are **not placed in URLs**. If URLs need a token, it must be a **short-lived stream token** scoped to a single session/file.

**Libraries scan reliably**
- Library creation rejects invalid/unreadable paths immediately.
- Scans detect common video container extensions and record the correct DB mappings.
- The API exposes enough data for the player to map `item -> playable file`.

**Proof**
- There are automated tests (unit/integration/E2E) that:
  - confirm the player’s stream URLs are requested
  - confirm HLS endpoints reject unauthenticated access
  - confirm a fixture `.mp4` is detected and playable

---

## Stage 0 — Establish baseline + guardrails

### Goal
Reproduce the failure and capture the evidence. Then lock in safety checks so you don't regress.

### Steps
1) **Run the system locally** (backend + UI) and reproduce:
   - Create library → scan → open item → click Direct Play / HLS → confirm “nothing happens”.
2) Confirm the **root cause** described in the docs:
   - Player expects `file_id` from `/api/v1/items/:id`.
   - Items API does not return it.
3) Verify external dependencies:
   - Confirm `ffmpeg` and `ffprobe` availability.
   - Confirm the transcode/cache directories are writable.

### Done looks like
- You have captured:
  - browser Network trace showing no requests occur on click
  - server logs around playback attempt
- You can point to the exact mismatch in the Player/API contract.

---

## Stage 1 — Fix the API ↔ Player contract (unblock playback)

### Where is the problem?
- UI: `ui/src/app/player/[id]/page.tsx` assumes `/api/v1/items/{id}` returns `file_id`.
- Server: `crates/server/src/routes.rs` does not include `file_id` in item responses.

### What is the problem?
Without a `file_id`, the Player cannot build stream URLs, so both Direct Play and HLS start functions become no-ops.

### Fix (preferred design)
Implement a dedicated endpoint: **Playback Descriptor**
- `GET /api/v1/items/{id}/playback`

#### Response shape (minimum)
```json
{
  "item_id": "…",
  "file_id": "…",
  "direct_url": "/stream/file/<file_id>",
  "hls_start_url": "/api/v1/playback/sessions",
  "media_info_url": "/api/v1/playback/info/<file_id>"
}
```

#### Server implementation requirements
- Load the item.
- Enforce library access (existing access checks).
- Lookup the mapped file via `rustfin_db::repo::items::get_item_file_id()`.
- If `file_id` is missing:
  - return a 409/422 style error (validation-like) with a clear message:
    “No playable file mapped to this item; rescan library.”

#### UI changes
- Update Player to call `/items/{id}/playback` instead of `/items/{id}`.
- If `file_id` is missing:
  - show error text prominently
  - disable Direct Play + HLS buttons
- Remove any silent early returns (`if (!fileId) return`) without user feedback.

### Done looks like
- Clicking either button causes actual network requests:
  - Direct Play issues `GET /stream/file/{file_id}`
  - HLS issues `POST /api/v1/playback/sessions`
- Player shows a meaningful error if a file mapping is absent.

---

## Stage 2 — Fix the biggest security bug: HLS endpoints are unauthenticated

### Where is the problem?
- Server: `crates/server/src/routes.rs`
  - `hls_master(...)` and `hls_segment(...)` accept only `State` + `Path`, no `AuthUser`.

### What is the problem?
Anyone who knows a `session_id` can fetch:
- `/stream/hls/{sid}/master.m3u8`
- `/stream/hls/{sid}/seg_*.ts`

Session IDs become bearer credentials (“bearer-by-URL”), which is not acceptable.

### Fix
Enforce authentication + authorization on **every** HLS request.

#### Required changes
1) Add `AuthUser` to `hls_master` and `hls_segment` handlers.
2) Bind each transcode session to:
   - `user_id`
   - `file_id`
   when it is created (store this in the session map).
3) On every HLS request:
   - verify the caller’s `user_id` matches the session owner
   - optionally also verify the item/library access again if your model requires it.

### Done looks like
- Requests to `/stream/hls/...` without auth return **401/403**.
- Authenticated user A cannot fetch user B’s HLS session even if they guess the ID.

---

## Stage 3 — Replace “JWT in URL” with a safe streaming auth mechanism

### Where is the problem?
- UI passes `?token=<jwt>` for direct play.
- Server accepts long-lived tokens from URL query parameters.

### What is the problem?
Long-lived bearer tokens in URLs leak via browser history, logs, and referrers.

### Fix options (choose ONE; prefer A if you can change auth model)
**Option A (best): Same-origin HttpOnly cookies for auth**
- Store auth in HttpOnly cookie with SameSite policy.
- `<video>` and Safari HLS automatically send cookies to same-origin.
- Server reads cookie and authenticates streams without URL tokens.

**Option B (minimum-change): Mint a short-lived scoped “stream token”**
- Add endpoint or extend playback descriptor to mint:
  - TTL: 30–120 seconds
  - Scope: `file_id` and/or `session_id`
  - Audience: `"stream"`
- Use `?st=<stream_token>` in:
  - Direct file URL
  - HLS playlist URL and segment URLs (or validate per request based on session ownership)
- Reject the primary JWT in query string (keep Authorization header support for XHR/fetch).
- Add response headers to reduce leakage:
  - `Cache-Control: no-store`
  - `Referrer-Policy: strict-origin-when-cross-origin` (or stricter)

### Done looks like
- Stream URLs never include the primary auth JWT.
- Any token in a URL is short-lived and scoped to a single playback.
- Logs and browser history no longer contain long-lived credentials.

---

## Stage 4 — Make Direct Play reliable where possible (MIME + Range + browser capability)

### Where is the problem?
- Some scanned extensions may be served as `application/octet-stream` if MIME mapping is missing.
- Browser support varies by container+codec; Direct Play cannot be universal.

### What is the problem?
Even after `file_id` is fixed, Direct Play will fail for many formats unless:
- the browser supports decoding, and
- the server sends appropriate headers.

### Fix
1) **Align Content-Type mapping** with the scanner allowlist.
   - Update `crates/server/src/streaming.rs::content_type_for_path()`
   - Note: `m4v` is already mapped; only add truly missing types.
   - Add mappings at least for:
     - `m2ts`, `mts` → `video/mp2t`
     - `ogv` → `video/ogg`
     - other common containers you actively claim to support
2) **Improve decision logic in Player**
   - Prefer **Media Capabilities API** when available.
   - Fall back to `canPlayType()` and finally to a pragmatic runtime fallback:
     - attempt Direct Play
     - if `video.onerror` fires quickly, switch to HLS automatically
3) Ensure direct stream endpoint continues to support HTTP Range requests (already present).

### Done looks like
- `.mp4` files Direct Play in major browsers.
- Unsupported containers/codecs fall back to HLS quickly and automatically.
- Network shows `206 Partial Content` for range requests during seeking.

---

## Stage 5 — Make HLS transcode robust and observable

### Where is the problem?
- `ffprobe` path is hardcoded (`Path::new("ffprobe")`)
- Transcode concurrency gating is weakened by dropping semaphore permit early
- Errors are not always surfaced cleanly to the Player

### What is the problem?
Deployment fragility and unstable behavior under load:
- PATH differences break ffprobe
- “max concurrent transcodes” is not deterministically enforced
- user sees “nothing happened” or generic errors without guidance

### Fix
1) Make ffmpeg/ffprobe paths configurable via env (with sensible defaults).
2) Validate dependencies at startup (or via `/health` detail endpoint):
   - if missing, report clearly.
3) Enforce concurrency properly:
   - store an `OwnedSemaphorePermit` inside the session struct so it lives as long as the session.
4) Improve error surfaces:
   - API errors should include user-actionable hints:
     - “ffmpeg missing”
     - “input file not readable”
     - “transcode directory not writable”
   - Player should display those messages.

### Done looks like
- Under load, transcodes cap at the configured maximum.
- Missing ffmpeg/ffprobe shows a clear health error and a clear UI error.

---

## Stage 6 — Libraries: ensure scanning and mapping are correct + fail-fast path validation

### Goal
“Library detects media in directories” is already partly working in your report, but this stage makes it reliable and debuggable.

### Fix
1) Validate library paths on creation/update:
   - must be absolute, exist, be a directory, and be readable by the server process.
   - reject invalid paths with field-level errors pointing to `paths[i]`.
2) Ensure scanner supports a strong extension allowlist for common containers.
3) Ensure scanner always writes the `episode_file_map` mapping for playable items:
   - movie item → file_id mapped
   - episode item → file_id mapped
4) Add a “Rescan” button behavior that:
   - shows last scan time and last scan status
   - surfaces count of files found and items written (even just in logs initially)

### Done looks like
- You cannot create a library pointing to a missing/unreadable path.
- After scan, every playable item returns a file_id via `/items/:id/playback`.
- A fixture `.mp4` in the directory appears and can be played.

---

## Stage 7 — Tests (must ship with the fix)

### Required tests
**Backend**
- Unit/integration tests for `GET /items/:id/playback`:
  - returns file_id for a known mapped item
  - returns appropriate error if mapping missing
- HLS auth tests:
  - unauthenticated request to `/stream/hls/...` returns 401/403
  - authenticated request succeeds
  - authenticated user A cannot read session of user B

**Frontend (E2E)**
- Create library from fixture directory
- Scan
- Open item
- Click Direct Play → assert `GET /stream/file/...` occurs
- Click HLS → assert playlist and segments requested
- Assert UI displays meaningful errors when appropriate

### Done looks like
- A single test runner can prove playback works and that HLS is not public.
- Tests fail on regression (e.g., if someone removes auth from HLS again).

---

## Implementation order (recommended)
1) Stage 1 (playback descriptor) — unblock functionality
2) Stage 2 (auth HLS) — fix critical security
3) Stage 3 (stream token or cookie auth) — remove JWT-in-URL risk
4) Stage 4 (Direct Play reliability + fallback)
5) Stage 5 (transcoder robustness)
6) Stage 6 (library validation + scan observability)
7) Stage 7 (tests)

---

## Reference notes for the agent (do not skip)
Use these docs in this directory:
- `Rustyfin_Playback_Deep_Dive_8024266.md` (functional flow + endpoints)
- `Rustyfin_Playback_Deep_Dive_Critique_8024266.md` (verified corrections + P0 security issues)

External security rationale (URLs in code block so they don’t get pasted into prose):
```text
RFC 6750 (Bearer tokens; protect from disclosure): https://datatracker.ietf.org/doc/html/rfc6750
OWASP: Sensitive data in query strings (token leakage): https://owasp.org/www-community/vulnerabilities/Information_exposure_through_query_strings_in_url
Referrer-Policy header (reduce leakage): https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Referrer-Policy
web.dev referrer best practices: https://web.dev/articles/referrer-best-practices
MDN Media Capabilities API: https://developer.mozilla.org/en-US/docs/Web/API/Media_Capabilities_API
```

---
## Final acceptance checklist (must pass)
- [ ] Player uses `/api/v1/items/{id}/playback` and always gets `file_id` for mapped items.
- [ ] Clicking Direct Play triggers a stream request and video starts when playable.
- [ ] Clicking HLS triggers session creation and then playlist/segments; video starts.
- [ ] `/stream/hls/*` rejects unauthenticated requests.
- [ ] No long-lived auth JWTs appear in any stream URLs.
- [ ] Library creation rejects invalid paths; scanning finds fixture media.
- [ ] Automated tests cover the contract + HLS auth.
