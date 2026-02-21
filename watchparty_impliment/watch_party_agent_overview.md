# Watch Party — Agent Execution Plan (Rustyfin)

Date: 2026-02-21

This document is the “do-this-next” runbook for an automated coding agent working inside the Rustyfin repo.

## Inputs the agent must read first

1. **Feature spec / baseline plan**: `watch_party_implementation_report.md`
2. **Security + correctness critique**: `watch_party_critique_report.md`
3. **This execution plan**: `watch_party_agent_overview.md`

> Note on file availability: chat uploads and search indices can expire. To avoid losing context, copy these three MD files into the repo under `docs/reports/` and treat them as source-of-truth going forward.

## How this plan maps onto the other two docs

Use this as a “where to look” index:

- Implementation report:
  - DB design → sections **2.x**
  - Backend REST + WS modules → sections **3.x–4.x**
  - Frontend pages/components → sections **5.x**
  - Testing notes → last sections / appendix

- Critique report:
  - WebSocket hardening checklist → section **3.1** (and onward)
  - Token-in-URL risk discussion → section **3.2**
  - Repo-specific verification deltas → section **2.x**

Practical workflow for the agent:
1) Follow *this* plan’s stages in order.
2) When implementing a stage, open the corresponding section in the implementation report for API shapes and file lists.
3) Before merging a stage, open the critique report for that same area and apply every “must implement” hardening item.


---

## Repo reality check (verified against `Rustyfin-9fddab6` snapshot)

Before changing anything, the agent should confirm these facts locally:

- Backend is **Axum** with a single router in `crates/server/src/routes.rs`.
- Auth is a **Bearer JWT in the `Authorization` header**, validated by `crates/server/src/auth.rs`.
- UI stores JWT in **`localStorage`** and attaches it in `ui/src/lib/api.ts`.
- SSE exists at `GET /api/v1/events` (one-way), so Watch Party needs a bidirectional channel.
- DB is SQLite via `sqlx`, migrations live under `crates/db/migrations/`.

These facts shape the safest implementation choices.

---

## Security/modernity decision that must be made up front

### WebSocket authentication strategy (pick one, then implement consistently)

**Recommended for this repo (lowest risk + minimal leakage):**

**Authenticate as the first WebSocket message** rather than putting tokens in the URL query.

Why:
- OWASP warns that **query strings can leak sensitive tokens** into logs, browser history, and intermediary systems.
- OWASP’s WebSocket guidance emphasizes authentication, message validation, rate limiting, size limits, and safe logging.
- Browser WebSocket API does not expose a standard way to set arbitrary headers like `Authorization`; you can only pass the URL + optional subprotocols. (This limitation is widely documented; see the ongoing WHATWG discussion and the standard constructor docs.)

**Implementation shape for the recommended strategy:**
- Client connects to `wss://…/api/v1/watch-party/rooms/{room_id}/ws`
- Immediately sends `{"type":"auth","token":"<existing JWT>"}`
- Server holds connection in `Unauthed` state for a short deadline (e.g. 3 seconds) and closes if not authenticated

This avoids introducing a new `/ws-token` endpoint and avoids query-string secrets.

> Optional alternative: If you must authenticate during the handshake, consider `Sec-WebSocket-Protocol` as a carrier, but document the tradeoffs and never treat it as “more secure by default.”

---

## Staged execution plan

The agent should implement in small, compiling stages. Each stage has:
- **Goal**
- **Files**
- **Steps**
- **Acceptance criteria**

### Stage 0 — Prep & guardrails

**Goal:** create a safe workspace and establish baselines.

**Steps**
1. Create a feature branch.
2. Run unit/integration tests once to establish baseline.
3. Build server + UI once to ensure toolchain is healthy.
4. Create a working checklist in the PR description (copy the acceptance checklist at the end of this doc).

**Acceptance criteria**
- Clean baseline build.

---

### Stage 1 — Database schema (migration)

**Goal:** persist rooms, memberships, and invites.

**Files**
- Add: `crates/db/migrations/006_watch_party.sql`

**Steps**
1. Create tables:
   - `watch_party_room`
   - `watch_party_member`
2. Add **CHECK constraints** for enums and add indexes used by inbox queries.
3. Keep room passwords stored as **Argon2 hashes** (no plaintext). Rustyfin already uses Argon2 for user passwords in `crates/db/src/repo/users.rs`.

**Implementation notes**
- Include:
  - `PRIMARY KEY(room_id, user_id)` for membership uniqueness
  - Index on `(user_id, status)` to serve “inbox” queries
  - `CHECK(json_valid(policy_json))` if SQLite build supports JSON1; otherwise validate in Rust.

**Acceptance criteria**
- Migration runs successfully.
- DB constraints prevent invalid role/status values.

---

### Stage 2 — DB repository layer

**Goal:** define a typed access layer for watch party DB operations.

**Files**
- Add: `crates/db/src/repo/watch_party.rs`
- Modify: `crates/db/src/repo/mod.rs`

**Steps**
1. Define row structs:
   - `WatchPartyRoomRow`
   - `WatchPartyMemberRow`
   - `WatchPartyInviteSummary`
2. Implement functions used by server handlers:
   - `create_room(...) -> room_id`
   - `get_room(room_id)`
   - `list_members(room_id)`
   - `upsert_member(room_id, user_id, role, status, …)`
   - `list_invites_for_user(user_id)`
   - `set_member_status(room_id, user_id, status)`
3. Make room creation atomic:
   - room insert + membership inserts inside a transaction

**Acceptance criteria**
- `cargo test` builds DB crate.

---

### Stage 3 — Server module scaffolding

**Goal:** add a new cohesive `watch_party` module without touching existing playback.

**Files**
- Add directory:
  - `crates/server/src/watch_party/`
    - `mod.rs`
    - `router.rs`
    - `handlers.rs`
    - `manager.rs`
    - `protocol.rs`
    - `permissions.rs`
    - `ws.rs`
- Modify:
  - `crates/server/src/lib.rs`
  - `crates/server/src/state.rs`
  - `crates/server/src/main.rs`
  - `crates/server/src/routes.rs`
  - root `Cargo.toml` (Axum `ws` feature)

**Steps**
1. Enable Axum WebSockets feature flag:
   - `axum = { version = "0.8", features = ["macros", "ws"] }`
2. Add `WatchPartyManager` to `AppState` in `state.rs`.
3. Instantiate it in `main.rs` when building `AppState`.
4. Nest router under `/api/v1/watch-party` in `api_router()` in `routes.rs`.

**Acceptance criteria**
- Server compiles with empty watch party router mounted.

---

### Stage 4 — REST API endpoints (room + inbox)

**Goal:** build all non-real-time primitives first.

**Files**
- Implement in: `crates/server/src/watch_party/handlers.rs`
- Route wiring in: `crates/server/src/watch_party/router.rs`

**Endpoints** (all under `/api/v1/watch-party`)

1) `GET /users` — inviteable users
- Auth required.
- Return **minimal** fields: `{ id, username }`.
- Exclude password hashes, roles, library IDs.

2) `POST /eligible-libraries`
- Input: list of selected invitee IDs
- Output: intersection of library IDs
- Treat admins as “all libraries” (because server access checks do).

3) `POST /rooms` — create
- Validations:
  - item exists
  - item.kind ∈ {`movie`,`episode`}
  - host has access to item.library_id
  - all invitees exist
  - all invitees have access to item.library_id
- Insert:
  - room row (status=lobby)
  - host membership row (role=host, status=joined)
  - invite rows (status=invited)

4) `GET /rooms/{room_id}` — details
- Returns:
  - room metadata, member summaries
  - `password_required: bool`

5) `POST /rooms/{room_id}/join`
- Validations:
  - room exists and not ended
  - user has access to item.library_id
  - password (if set) matches Argon2 hash
- Effects:
  - membership: invited→joined OR create new joined membership with default role from policy

6) `POST /rooms/{room_id}/leave`
- sets membership status=left

7) `POST /rooms/{room_id}/end` (host only)
- sets room status=ended

8) `GET /invites`
- lists `watch_party_member` where `user_id = me` and `status = invited`

9) `POST /invites/{room_id}/decline`
- sets status=declined

**Rate limiting (REST)**
- Apply rate limiting to:
  - `POST /rooms` (spam/DoS)
  - `POST /rooms/{id}/join` (password brute force)
- Rustyfin already has an in-memory `RateLimiter` used for setup routes: `crates/server/src/setup/rate_limit.rs`. Reuse or generalize it.

**Acceptance criteria**
- You can create a room and see invites via REST alone.
- Join rejects users lacking access.

---

### Stage 5 — Runtime manager (in-memory state)

**Goal:** maintain authoritative playback state and connection registry.

**Files**
- Implement in: `crates/server/src/watch_party/manager.rs`

**Responsibilities**
- Map `room_id -> RoomRuntime`
- `RoomRuntime` contains:
  - authoritative playback state: `{playing, position_ms, updated_ts_ms}`
  - `broadcast::Sender<ServerMsg>` for fanout
  - connected member set (user IDs)
- Provide functions used by WS handler:
  - `get_or_create_runtime(room_id) -> Arc<RoomRuntime>`
  - `apply_action(user_id, action) -> updated_state`
  - `broadcast_state()`

**Resource control**
- Basic guards:
  - max rooms in memory (LRU or TTL-based cleanup)
  - cleanup task for ended/idle rooms

**Acceptance criteria**
- Unit tests for manager state update logic.

---

### Stage 6 — WebSocket protocol + handler

**Goal:** real-time sync control plane.

**Files**
- Protocol types: `crates/server/src/watch_party/protocol.rs`
- Handler: `crates/server/src/watch_party/ws.rs`

**Protocol (JSON, serde tagged)**
Client → server:
- `auth { token }`
- `play { position_ms }`
- `pause { position_ms }`
- `seek { position_ms }`

Server → client:
- `state { playing, position_ms, updated_ts_ms, server_ts_ms, members[] }`
- `presence { user_id, connected }`
- `error { message }`

**Security hardening (must implement)**
Implement at minimum:
1. **Origin validation** on upgrade
2. **Authentication deadline** (close if no `auth` message quickly)
3. **Authorization checks** (JWT + library access + role policy)
4. **Message size limit**
5. **Message rate limiting**
6. **Idle timeouts**
7. **Logging redaction**

**Acceptance criteria**
- Two browsers in the same room converge to the same play/pause/seek state.
- Viewers cannot invoke restricted actions.

---

### Stage 7 — UI: Watch Party pages

**Goal:** deliver the two-page UX (create page + room/lobby page) while reusing existing player logic.

**Files**
- Modify: `ui/src/app/NavBar.tsx`
- Add:
  - `ui/src/app/watch-party/page.tsx`
  - `ui/src/app/watch-party/components/*`
  - `ui/src/app/watch-party/rooms/[roomId]/page.tsx`

**Refactor (recommended)**
- Extract shared player into `ui/src/app/player/VideoPlayer.tsx` and have both the existing player page and the watch party room page use it.

**Create page behavior**
- Fetch libraries (`GET /libraries`).
- Invite list from `GET /watch-party/users`.
- As invitees change, call `POST /watch-party/eligible-libraries`.
- Create room via `POST /watch-party/rooms`.

**Room page behavior**
- Load room details.
- Prompt for password if needed.
- Open WS and send `auth` message with the existing JWT.
- Apply sync convergence.

**Acceptance criteria**
- Host can create room and see lobby.
- Invitees see invites and can join.

---

### Stage 8 — Testing

**Server tests** (extend `crates/server/tests/integration.rs`)
- Creation rejects invalid access.
- Join rejects wrong password.
- Invite inbox works.
- WS rejects unauthenticated + unauthorized actions.

**Acceptance criteria**
- Tests pass locally.

---

### Stage 9 — Update docs to match implementation

**Goal:** the docs remain executable for future agents.

**Steps**
- Update `watch_party_implementation_report.md` to reflect the chosen WS auth strategy and the mandatory hardening controls.

---

## Final acceptance checklist (definition of done)

Functional:
- [ ] “Watch Party” in top nav.
- [ ] Create page shows media + invite users on one screen.
- [ ] Media list restricts to the intersection of all selected users’ library access.
- [ ] Invited users receive an inbox entry.
- [ ] Join-by-link works for eligible users.
- [ ] Playback sync works with correct permission enforcement.

Security/hardening:
- [ ] No secrets (JWT, passwords) in URLs.
- [ ] WS origin validation, size limit, rate limit, idle timeout.
- [ ] REST rate limiting for create/join.
- [ ] No logging of tokens/passwords/full messages.
- [ ] Server-side checks enforce library access.

Maintainability:
- [ ] Playback logic reused via shared `VideoPlayer`.
- [ ] New code isolated under `watch_party` modules and DB repo file.
- [ ] Tests cover key authorization cases.

---


## References (copy/paste for quick lookup)

```text
OWASP WebSocket Security Cheat Sheet:
https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html

OWASP: Information exposure through query strings in URL:
https://owasp.org/www-community/vulnerabilities/Information_exposure_through_query_strings_in_url

Axum WebSocket docs (extract::ws):
https://docs.rs/axum/latest/axum/extract/ws/

MDN WebSocket API:
https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API

MDN WebSocket constructor (subprotocols / Sec-WebSocket-Protocol):
https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket
```
