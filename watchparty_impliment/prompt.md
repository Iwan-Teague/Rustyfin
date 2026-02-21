You are an automated coding agent working inside the Rustyfin repo. Your job is to IMPLEMENT the Watch Party feature end-to-end by following three in-repo documents, in the correct order, while keeping the project stable, modern, and secure.

========================================================
0) NON-NEGOTIABLE CONSTRAINTS (do not violate)
========================================================
- Keep as much logic as possible in Rust (Axum + existing crates). Do not introduce external services.
- Do NOT add third-party system binaries as dependencies/fixes (project guideline).
- Preserve existing working features: start/stop commands, factory reset script, setup wizard, libraries, users, permissions, TMDB metadata, streaming playback.
- Frontend must follow existing UI theme/components and Next.js App Router style already used.
- All authorization must be enforced server-side. UI checks are only UX.
- Implement modern WS security hardening (origin checks, rate limiting, message validation, size limits, timeouts, safe logging). Follow OWASP WebSocket guidance and avoid placing sensitive tokens in URL query strings unless explicitly required and mitigated.
- Work in small stages with compiling commits. After each stage: format/lint/test.

========================================================
1) READ THESE DOCUMENTS FIRST (in this exact order)
========================================================
Open and read these three markdown docs from:
  ./watchparty implements/

1) watch_party_agent_overview.md  (the runbook / stage plan)
2) watch_party_implementation_report.md (feature design + file list + API shapes)
3) watch_party_critique_report.md (corrections + security hardening checklist)

Treat the agent_overview as the primary execution sequence.
Treat the critique as the authority on security/modernity and repo-specific corrections.
Use the implementation report for API payload shapes, UI structure, and overall feature spec.

If any doc claim conflicts with repo reality, you must: (a) verify in code, (b) follow the critique’s recommendation, (c) update the plan in your PR notes.

========================================================
2) REPO REALITY CHECK (verify before coding)
========================================================
Confirm locally (by opening files) these key facts; if they differ, adapt:
- Backend is Axum router in crates/server/src/routes.rs and server state in crates/server/src/state.rs
- Auth uses Bearer JWT for HTTP fetches; UI stores JWT in localStorage and attaches it in ui/src/lib/api.ts
- DB is SQLite via crates/db; migrations are NOT automatic via sqlx CLI:
  IMPORTANT: migrations are hardcoded in crates/db/src/migrate.rs (MIGRATIONS array). Any new migration file MUST be added there.
- SSE exists at GET /api/v1/events (one-way). Watch Party needs WS for bidirectional control.

Write down the exact file paths you verified in your working notes.

========================================================
3) IMPLEMENTATION STRATEGY (must match agent_overview)
========================================================
Implement strictly in stages. Each stage must:
- Compile and run basic tests
- Include a clear commit message
- Include “why” and “what changed” in a running changelog (PR description or /docs/reports/watch_party_devlog.md)

Stages (high-level) — follow details from watch_party_agent_overview.md:
Stage 0: Prep/guardrails and baseline build
Stage 1: DB migration(s) for rooms/members/invites
Stage 2: DB repository layer (typed functions)
Stage 3: Server module scaffolding + router mounting
Stage 4: REST endpoints (create room, eligible libraries intersection, room details, join/leave/end, invites inbox/decline, list users for invites)
Stage 5: WebSocket control plane (protocol, manager/runtime state, room presence, playback state)
Stage 6: UI: NavBar link + Watch Party creation page (media picker + invite panel + room options + inbox)
Stage 7: UI: Room/lobby page + player integration + synchronization logic
Stage 8: Tests (server + UI sanity) + documentation polish

========================================================
4) SECURITY & MODERNITY REQUIREMENTS (apply everywhere)
========================================================
A) WebSocket hardening checklist (must implement):
- Only allow secure deployment (wss behind TLS in production); document expectation.
- Validate Origin header on WS upgrade (allow same-origin + configured allowed origins).
- Enforce authentication for WS: DO NOT rely on tokens in URL query strings.
  Preferred (per agent_overview): connect WS, then require first message:
    {"type":"auth","token":"<existing JWT>"} within a short deadline (e.g., 3 seconds).
  Keep connection in Unauthed state until validated, then upgrade to Authed.
- Rate limit:
  - REST endpoints: create/join/ws handshake routes; protect password brute force.
  - WS messages: per-connection message rate; drop/close on abuse.
- Message size limits (hard cap for control messages, e.g. 8–32KB).
- Strict message validation (serde enum + reject unknown fields).
- Idle timeouts + ping/pong strategy; close dead sockets.
- Safe logging: never log passwords, tokens, or full WS frames. Redact.
- Authorization: enforce per-room roles for play/pause/seek; host always allowed.
- Avoid compression negotiation unless explicitly needed (OWASP guidance).

B) Token handling rule:
- Do NOT put JWT or secrets in query parameters. If any mechanism would leak via logs/history, choose a safer alternative (first-message auth or subprotocol carrier).
- If you must use a one-time ticket, keep TTL extremely short, single-use, and ensure it never hits logs.

C) Database hardening:
- Use CHECK constraints or validated enums for role/status.
- Index for inbox queries (user_id + status).
- Store room passwords as strong hashes (reuse existing Argon2 approach used for users).
- Transactions for room creation (room row + member rows atomically).

========================================================
5) FEATURE-SPEC REQUIREMENTS (must match spec)
========================================================
- Add “Watch Party” tab to header bar (NavBar) next to Libraries/Admin.
- Watch Party page: single screen “pre-lobby”:
  - Media list restricted to what the current user can access.
  - Invite list shows all server user accounts; toggle invitees.
  - Media availability must be intersection-based: chosen media must be accessible to ALL selected invitees.
  - Options: optional room password; blanket permissions for non-host (viewer/controller; allow seek; allow play/pause).
  - Create room produces a shareable URL; invited users also get an “inbox” notification.
- Lobby/room page:
  - Shows roster + ready/connected state.
  - Host can start/stop/seek. Non-host actions allowed only by policy/role.
  - Users can join via URL; if password required, must enter it.
  - Invited users can join from inbox.

========================================================
6) FILE MAPPING (use docs, but also verify repo patterns)
========================================================
Backend likely areas (confirm in repo and adapt):
- crates/server/src/routes.rs (mount new router)
- crates/server/src/state.rs and main.rs (add WatchPartyManager and init)
- crates/server/src/watch_party/ (new module tree: mod.rs, router.rs, handlers.rs, ws.rs, protocol.rs, manager.rs)
- crates/db/migrations/006_watch_party.sql (new migration)
- crates/db/src/migrate.rs (MIGRATIONS array MUST include 006)
- crates/db/src/repo/watch_party.rs and repo/mod.rs export

Frontend likely areas:
- ui/src/app/NavBar.tsx (add link)
- ui/src/app/watch-party/page.tsx (create page)
- ui/src/app/watch-party/rooms/[roomId]/page.tsx (room page)
- ui/src/app/watch-party/components/* (media picker, invite picker, options, inbox panel)
- Consider refactoring existing player page logic into a reusable component if needed, but avoid regressions.

========================================================
7) STEP-BY-STEP EXECUTION (what you must do)
========================================================
(1) Create a working branch and a running dev log.
(2) Stage 0: Build backend + UI as-is. Record commands and results.
(3) Stage 1: Add DB migration file AND update crates/db/src/migrate.rs MIGRATIONS list.
    - Add tables: watch_party_room, watch_party_member (invites/inbox via status).
    - Add constraints/indexes.
(4) Stage 2: Implement DB repo functions and structs in crates/db/src/repo/watch_party.rs
    - Ensure transactions for create_room + invited members.
(5) Stage 3: Create server watch_party module scaffolding and router mounting.
(6) Stage 4: Implement REST endpoints required by UI flow:
    - list inviteable users (minimal fields)
    - eligible libraries intersection
    - create room (validations: all invitees have access to item’s library)
    - get room details
    - join room (password check, membership updates)
    - leave/end room
    - invites inbox list + decline
(7) Stage 5: Implement WebSocket control plane:
    - Protocol enums (auth + play/pause/seek + state broadcast + presence)
    - First-message auth with existing JWT; short deadline.
    - WatchPartyManager runtime map of rooms with broadcast sender + state.
    - Permission enforcement for control messages.
    - Hardening: origin check, message size cap, message rate cap, idle timeout.
(8) Stage 6: UI create page:
    - media picker from accessible libraries
    - invite picker (server users) with roles
    - intersection logic: disable media not shared by all invitees (or filter)
    - room options + create button
    - inbox panel for invites
(9) Stage 7: UI room page:
    - join flow (password prompt if needed)
    - open WS and send auth message immediately
    - apply authoritative state to video element
    - send control messages only if permitted
(10) Stage 8: Tests + polish:
    - Add integration tests for create/join/access control/password brute force protections (as feasible).
    - Ensure lint/format pass:
      - Rust: cargo fmt, cargo clippy, cargo test
      - UI: npm run lint, npm run build
    - Update docs (if you change any API shapes from the plan).

========================================================
8) DEFINITION OF DONE (must satisfy all)
========================================================
- Watch Party appears in NavBar and routes work.
- A user can create a room, pick media, select invitees, set policies/password, and get a shareable link.
- Invited users see invites in inbox and can join from there.
- Join-by-link works; password gating works; library access is enforced.
- Playback synchronization works for play/pause/seek with server-authoritative state.
- Permissions work (viewer vs controller vs host).
- WS endpoint is hardened (origin checks, validation, size/rate limits, timeouts, safe logging).
- No existing features regress; tests/build pass.

========================================================
9) OUTPUT REQUIREMENTS (what you must produce)
========================================================
At the end, output a concise PR-style summary containing:
- List of all new/changed files
- All new endpoints (method + path + purpose)
- WS message protocol summary
- Security controls implemented (checklist)
- How to run server + UI + tests locally
- Any follow-up TODOs explicitly marked (only if unavoidable)

Do the work now, stage by stage, committing after each stage, and keep the repo in a runnable state after each commit.