# Critique & Verification Report — Watch Party Implementation Report (Rustyfin)

Date: 2026-02-21
Inputs reviewed:
- `watch_party_implementation_report.md` (the document you asked me to critique)
- `Rustyfin-9fddab6.zip` (repo snapshot extracted under `Rustyfin/`)

> Note on file availability: I was able to open both files above from this chat’s uploaded artifacts. Some file-search indices can expire over time; if you later ask me to cross-check *other* artifacts that aren’t currently present, you may need to re-upload them.

---

## 1) Executive verdict (with hard truth, lovingly delivered)

Your implementation report is **directionally solid**: it fits the repo’s current Axum/sqlx/Next architecture, uses sane primitives (SQLite tables + in-memory runtime state + WebSocket control plane), and correctly emphasizes server-side authorization.

However, for “modern and secure” you’ll want to tighten a few things:

- **WebSocket security hardening is under-specified** (Origin checks, rate limits, message size limits, idle timeouts, token leakage, logging redaction). OWASP explicitly calls out these controls for WS endpoints. citeturn0search0turn1search2turn1search6  
- The statement “Browsers can’t set custom headers in the WebSocket handshake” is **mostly** true but needs nuance: you can’t set arbitrary headers via the browser WebSocket API, but **cookies and subprotocols** are available, and you can also authenticate in the **first message** after upgrade. citeturn0search13turn1search6  
- Using a **JWT in the query string** works, but it is a known exposure vector (logs, reverse proxies, analytics). If you keep it, you must treat it as sensitive and harden accordingly. citeturn0search10turn0search0  
- The repo currently stores the login JWT in **localStorage** (`ui/src/lib/auth.tsx`), which is more XSS-sensitive than HttpOnly cookies. If you ever want to claim “best practice secure,” you should at least document that tradeoff and consider a migration path. citeturn1search1turn1search4  

The rest of this critique verifies the document claim-by-claim against the repo, then proposes concrete improvements with file-level instructions.

---

## 2) Verification ledger — “does this match the repo?”

This section checks each meaningful claim in the implementation report against the actual project snapshot.

### 2.1 Repo architecture claims

| Document claim | Repo reality | Verdict |
|---|---|---|
| “Rust backend: Axum REST API + SQLite (sqlx) + in-process state” | `crates/server` uses Axum; `crates/db` uses sqlx sqlite; `AppState` holds broadcast events and managers | ✅ Correct |
| “Next.js UI (TypeScript)” | UI is under `ui/`, Next app router, TS (`.tsx`) | ✅ Correct |
| “Single-binary server friendly; avoid external services” | Repo is a single server binary; no external services required for watch party | ✅ Compatible |

### 2.2 WebSocket justification claims

| Document claim | Repo reality | Verdict |
|---|---|---|
| “SSE is server→client only” | `/api/v1/events` is implemented as Axum SSE using a broadcast channel; it’s one-way | ✅ Correct |
| “Need WebSockets for bidirectional play/pause/seek” | True for real-time control plane | ✅ Correct and modern citeturn0search0turn0search3 |

### 2.3 Cargo / Axum claims

| Document claim | Repo reality | Verdict |
|---|---|---|
| “Enable Axum WebSocket feature: add `ws` feature” | Workspace `Cargo.toml` currently has `axum = { version = "0.8", features = ["macros"] }` | ✅ Correct and required citeturn0search3 |

### 2.4 File-path and module claims (server)

| Document claim | Repo reality | Verdict |
|---|---|---|
| Modify `crates/server/src/lib.rs` to add `pub mod watch_party;` | `lib.rs` exists and lists modules; adding is consistent | ✅ Correct |
| Modify `crates/server/src/state.rs` to add `watch_party: Arc<WatchPartyManager>` | `AppState` exists and is constructed in `main.rs`; adding a field requires updating construction sites | ✅ Correct |
| Modify `crates/server/src/main.rs` to instantiate manager | `main.rs` constructs `AppState` directly | ✅ Correct |
| Modify `crates/server/src/routes.rs` to nest router | `routes.rs` builds router via `api_router()`; nesting is feasible | ✅ Correct |
| Add `crates/server/src/watch_party/*` modules | Folder doesn’t exist yet; adding matches current module style | ✅ Correct |

### 2.5 File-path and module claims (db)

| Document claim | Repo reality | Verdict |
|---|---|---|
| Add migration `crates/db/migrations/006_watch_party.sql` | Existing migrations are `001`..`005`; adding `006` matches numbering | ✅ Correct |
| Add `crates/db/src/repo/watch_party.rs`, and export in `repo/mod.rs` | Repo uses one file per area and exports in `mod.rs` | ✅ Correct |

### 2.6 Frontend path claims

| Document claim | Repo reality | Verdict |
|---|---|---|
| Modify `ui/src/app/NavBar.tsx` | File exists and contains “Libraries/Admin” links | ✅ Correct |
| Add `ui/src/app/watch-party/page.tsx` etc | Routes folder doesn’t exist yet; Next app router supports it | ✅ Correct |
| “No changes required to `ui/src/lib/api.ts`” | True: wrappers already exist; you can create new helpers separately | ✅ Reasonable |

### 2.7 Claims that need tightening / correction

1) **“Browsers can’t set custom headers in the WebSocket handshake.”**  
Browser WebSocket API doesn’t allow arbitrary header injection (you can’t do `Authorization: Bearer ...`), but **cookies are automatically sent** to same-origin WS endpoints, and you can pass data via the **subprotocol list** or authenticate immediately after connect with a first message. So the *core point* is correct, but the statement should be more precise. citeturn0search13turn1search6  

2) **“The safe pattern is short-lived token, then connect with token in URL query.”**  
This is *a* pattern, but the document should explicitly acknowledge query-string exposure and provide mitigations (don’t log it, short TTL, one-time use, origin check, rate limit). OWASP warns about sensitive data in query strings. citeturn0search10turn0search0  

3) **Playback_rate field in authoritative state.**  
No existing UI uses playbackRate; adding it is harmless but introduces more surface area. For MVP, keep the state minimal unless you explicitly support speed changes.

---

## 3) Security review — modern, secure, and “things that will bite you later”

This section is the meat: what’s secure, what’s risky, and what upgrades to do.

### 3.1 WebSocket endpoint hardening (missing details in doc)

OWASP’s WebSocket Security Cheat Sheet recommends (among other things): TLS (`wss://`), origin validation, authentication/authorization, message validation, rate limiting, message size limits, timeouts/idle connection handling, and careful logging that avoids secrets. citeturn0search0turn1search2turn1search6  

Your document mentions auth and permission checks, but it should explicitly add **all** of the following implementation constraints:

#### Add to the document: mandatory controls
1) **Origin validation** during handshake  
- Why: prevents Cross-Site WebSocket Hijacking (CSWSH) when cookie-based auth is used; still useful as defense-in-depth. RFC 6455’s security model references origin-based security. citeturn1search6turn0search0  
- How in Axum: inspect `headers.get("origin")` in the WS handler before upgrade and reject unexpected origins.

2) **Rate limiting**
- Per-IP and per-user for:
  - `POST /rooms`
  - `POST /rooms/{id}/join` (password brute force)
  - `POST /rooms/{id}/ws-token`
  - WS message rate (per connection)
- Why: prevents “free DoS” and brute force. citeturn0search0turn1search10  

3) **Message size limits**
- Reject messages above a small cap (e.g., 8–32 KB for control messages).
- Why: memory pressure and parsing attacks. citeturn0search0turn1search10  

4) **Idle timeouts**
- Close connections that:
  - never authenticate (if you allow unauthenticated upgrade)
  - stay idle (no ping/pong, no messages) beyond N minutes
- Why: resource leak prevention. citeturn1search10turn0search0  

5) **Logging redaction**
- Never log: WS tokens, join passwords, full message bodies.
- Why: OWASP explicitly warns against logging sensitive WS data. citeturn0search0turn0search10  

### 3.2 Token-in-URL risk (and better alternatives)

Your doc proposes: `.../ws?t=token`.

That works, but OWASP flags sensitive data in query strings as a common information exposure risk. citeturn0search10  

**Recommended upgrade:** prefer one of these patterns (in order):

#### Option A (best for your current UI auth): “Authenticate in the first WS message”
- Connect: `wss://.../ws` (no token in URL).
- Client immediately sends `{ "type":"auth", "token":"<jwt>" }` using the same JWT already in localStorage.
- Server:
  - starts connection in `Unauthed` state
  - enforces a short deadline (e.g. 3s) for receiving auth message
  - closes if missing/invalid
- Pros:
  - no token in URL
  - no extra token minting endpoint
  - matches the fact your UI already has access to JWT in JS
- Cons:
  - handshake isn’t authenticated; must be careful about DoS.

This approach is common because the browser API is constrained. citeturn0search13turn0search0  

#### Option B: use `Sec-WebSocket-Protocol` to carry auth material
- `new WebSocket(url, ["bearer", token])`
- Server reads `sec-websocket-protocol` header.
- Pros: no query string
- Cons: token may still be logged by some proxies; awkward parsing; still sensitive.

#### Option C: keep your query token, but make it **one-time-use and opaque**
- `POST /ws-token` returns a random UUID (not a JWT) stored server-side for 60s.
- WS handshake includes `?t=uuid`.
- Server checks and deletes it (single-use).
- Pros: leaked token is short-lived AND single-use
- Cons: more server state, but it’s small.

If you keep the JWT-in-query approach anyway, the doc must explicitly mandate:
- TTL ≤ 60s
- include `room_id` and `sub`
- include `jti` and store “used JTIs” to prevent replay within TTL (or switch to opaque single-use)

### 3.3 Session/auth storage in the UI (existing repo risk)

Repo reality: the UI stores the bearer token in `localStorage` (`ui/src/lib/auth.tsx`). That’s not automatically “bad,” but it’s more exposed to XSS than HttpOnly cookies, and OWASP materials call out that local/session storage lacks HttpOnly protection. citeturn1search1turn1search4  

**What the doc should say:**
- Current system uses localStorage for JWT; therefore:
  - invest in CSP + XSS hygiene
  - keep JWT short-lived or rotate
  - avoid putting additional long-lived secrets in URLs

**Optional future hardening:** migrate to cookie-based session tokens with `Secure`, `HttpOnly`, `SameSite` attributes (requires CSRF strategy). citeturn1search1turn1search13  

### 3.4 Password handling (room password)

Document says “store an Argon2 hash.” That’s modern and correct.

Improvements to document:
- Define **minimum** and **maximum** room password length.
- Add brute-force controls:
  - rate limit join attempts per IP/user/room
  - exponential backoff after N failures
- Never log passwords; never return password validation details beyond “invalid password.”

### 3.5 Authorization model (intersection rule)

Your “everyone must have access to the media” rule is correct.

But the document should specify details that match Rustyfin’s current authorization semantics:
- In Rustyfin, `admin` bypasses library restrictions (`ensure_library_access()` in `crates/server/src/routes.rs`).  
So intersection logic should treat admins as “all libraries.” Otherwise, inviting an admin could accidentally reduce eligibility.

Also, enforcing intersection only in UI is not security; ensure creation/join endpoints enforce access server-side (your doc already does—good).

### 3.6 Data integrity: DB constraints

Your schema is fine, but you should add constraints for integrity and “future you” sanity:

- `CHECK(status IN ('lobby','ended'))`
- `CHECK(role IN ('host','controller','viewer'))`
- `CHECK(status IN ('invited','joined','declined','left'))`
- optional `CHECK(json_valid(policy_json))`

Also add indexes you’ll actually use:
- `watch_party_member(room_id)`
- `watch_party_member(user_id, status)`
- `watch_party_room(host_user_id, created_ts)`

### 3.7 DoS / resource exhaustion risks in runtime manager

Your manager uses `broadcast` and `HashMap<room_id, runtime>` (good). Missing details to add:

- Cap maximum rooms in memory (LRU eviction).
- Cap max connections per room (e.g., 50) and per user (e.g., 3).
- Handle broadcast lag: when a client lags, don’t spam; just send the latest `state` snapshot.

OWASP suggests message rate limiting and careful resource controls. citeturn0search0turn1search10  

---

## 4) Clarity / “how to make the change” improvements (the document should be more explicit)

The report is already readable, but for “agent-ready,” add the following explicit wiring snippets.

### 4.1 Exact router wiring (because `routes.rs` is monolithic)

Add to the implementation report:

```rust
// crates/server/src/routes.rs
fn api_router() -> Router<AppState> {
    Router::new()
        // existing routes...
        .nest("/watch-party", crate::watch_party::router::watch_party_router())
}
```

### 4.2 Exact AppState wiring (because it’s constructed manually)

Add to the implementation report:

```rust
// crates/server/src/state.rs
#[derive(Clone)]
pub struct AppState {
  pub db: SqlitePool,
  pub jwt_secret: String,
  pub transcoder: Arc<rustfin_transcoder::session::SessionManager>,
  pub cache_dir: PathBuf,
  pub events: tokio::sync::broadcast::Sender<ServerEvent>,
  pub watch_party: Arc<crate::watch_party::manager::WatchPartyManager>,
}
```

…and the corresponding construction in `crates/server/src/main.rs`.

### 4.3 WS protocol versioning

Add `protocol_version: 1` in the WS `state` message or implement a `hello` handshake.

### 4.4 Transaction boundaries (room creation + invites)

Document should explicitly require:
- Room create + member inserts happen in **one** DB transaction.
- If any insert fails, the entire operation rolls back.

### 4.5 Explicit error mapping

Rustyfin uses `ApiError::{BadRequest, Forbidden, NotFound, Conflict, Internal}`.

Document should say:
- use `BadRequest` for validation errors
- `Forbidden` for access violations
- `Conflict` for “item has no playable file mapped” (mirrors existing playback descriptor behavior)

---

## 5) Feature-level gaps vs your requirements

Your original requirement includes:
- host can copy URL and send it ✅ addressed
- invited users get a notification inbox ✅ addressed
- invite roles: viewer/controller ✅ addressed
- blanket toggles for what non-host can do ✅ addressed
- media list restricted to intersection ✅ addressed

Two gaps worth adding to the implementation report:

1) **Invite-only vs open-by-link toggle**  
Add a per-room toggle:
- `invite_only: bool`  
If true, join-by-link requires an existing member row (no “uninvited join”).

2) **Host reassignment policy**
Define what happens when the host disconnects long-term.

---

## 6) Concrete “patch list” — edits to apply to the implementation report itself

### 6.1 Amend WS auth section to include safer options
Add:
- “Preferred: authenticate in first message (no URL token)”
- “Acceptable: opaque one-time token in query string”
- “If using JWT in query: MUST redact logs, MUST short TTL, MUST prevent replay” citeturn0search10turn0search0  

### 6.2 Add a “WebSocket security checklist” section
Include:
- wss only in production
- origin check
- per-connection message rate limit
- per-connection message size cap
- idle timeout
- audit logging without secrets citeturn0search0turn1search2turn1search10  

### 6.3 Add DB CHECK constraints + indexes
Add CHECK constraints for role/status values and practical indexes.

### 6.4 Add “transactional create_room” guidance
Require a DB transaction for room + members.

### 6.5 Add “admin intersection semantics” note
Define how admins interact with library restrictions.

### 6.6 Add “input validation rules”
Specify:
- max invitees per room
- password length constraints
- reject duplicate invitees

### 6.7 Add “room cleanup strategy”
Define:
- cleanup ended rooms from memory immediately
- cleanup idle rooms after N minutes

---

## 7) Optional improvements (nice-to-have)

### 7.1 Hls.js header injection as a non-Safari-only alternative
Hls.js supports setting headers via `xhrSetup`, which can carry Authorization for many browsers. citeturn1search3  
(But native HLS clients, especially Safari, won’t let you add custom headers — so signed URLs/cookies remain necessary for broad compatibility.)

### 7.2 Improve JWT hygiene over time
Current `validate_token` uses default validation without issuer/audience. If you want “secure by default,” plan for:
- `iss`, `aud`
- key rotation
- shorter auth token TTL with refresh flow  
OWASP JWT guidance is a good baseline. citeturn0search2turn0search18  

---

## 8) Bottom line

- **Modern approach:** yes (WS + authoritative timeline). citeturn0search3turn1search6  
- **Secure as written:** almost, but needs explicit WS hardening, query-token mitigations, and DB constraints. citeturn0search0turn0search10turn1search10  
- **Clear “why/how”:** good, but add concrete wiring snippets and transactional guidance.

---

## 9) Suggested “agent rules” header to prepend to the implementation report

> **Agent rules:**  
> - Do not trust UI-side filtering for security; all access control must be enforced server-side.  
> - WebSocket endpoints MUST implement: origin validation, auth (handshake or first message), message size cap, message rate cap, idle timeout, and never log secrets.  
> - Room creation MUST be transactional (room + members).  
> - Room join/password attempts MUST be rate-limited to prevent brute force.  
> - Never store plaintext room passwords; hash with Argon2.

