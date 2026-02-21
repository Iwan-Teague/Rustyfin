# Watch Party Feature — Implementation Report (Rustyfin)

> Scope: Add a “Watch Party” feature with (1) a top-nav entry, (2) a create-party page that lets the host pick playable media and invite users on the same screen, (3) a lobby/room page with synchronized playback controls and join-by-link, and (4) an invitation inbox.

This report is written to match Rustyfin’s existing architecture:
- Rust backend: Axum REST API + SQLite (sqlx) + in-process state
- Next.js UI (TypeScript)
- Keep everything “single-binary server” friendly; avoid external services.

---

## 0) High-level product behavior

### 0.1 UX flow: Host (create)
1. User clicks **Watch Party** in top nav.
2. Page shows a split “pre-lobby” screen:
   - **Left panel**: media picker (only media the host can access; hierarchical browsing for TV).
   - **Right panel**: invite picker (list of local server users), per-user role, and room options.
3. As invitees are toggled, the media picker dynamically restricts to **media accessible to everyone selected**.
4. Host clicks **Create Watch Party**.
5. Host is navigated to `/watch-party/rooms/{room_id}`.

### 0.2 UX flow: Invitee (inbox → join)
1. Invitee sees a badge/count on Watch Party page (“Invites”).
2. Invitee opens Watch Party page and sees “Invites” list.
3. Clicking an invite opens the room link.
4. If the room requires a password, invitee enters it.
5. Lobby loads and the video syncs.

### 0.3 UX flow: Join-by-link
- Any authenticated user can open `/watch-party/rooms/{room_id}`.
- Join is allowed only if:
  - The user has access to the item’s library.
  - The room password (if configured) is correct.
- If the user wasn’t explicitly invited, their membership is created on join with a default role (viewer).

---

## 1) Architectural approach

### 1.1 Why WebSockets (not SSE)
Watch parties need **bidirectional** real-time messages (play/pause/seek) and room state fanout.
SSE is server→client only.

### 1.2 “Authoritative room state” model
- The server stores the authoritative playback state per room:
  - `playing: bool`
  - `position_ms: u64`
  - `updated_ts_ms: u64` (server time when state last changed)
  - `playback_rate: f32` (default 1.0)
- Clients compute desired position from server state and converge via:
  - immediate seek if drift > threshold
  - play/pause changes

### 1.3 Persistence vs runtime state
- **Persisted (SQLite)**: room metadata, invites/members, roles, policy toggles, password hash.
- **In-memory (server)**: live playback state, connected roster, ready flags.

If the server restarts, rooms remain in DB but live state resets to “paused at 0” unless you implement optional state persistence (outlined later).

---

## 2) Database model

### 2.1 New migration
Create a new migration file:

- `crates/db/migrations/006_watch_party.sql`

Schema:
- `watch_party_room`
- `watch_party_member`

#### `watch_party_room`
| Column | Type | Notes |
|---|---|---|
| id | TEXT PK | UUID |
| host_user_id | TEXT FK user(id) | ON DELETE CASCADE |
| item_id | TEXT FK item(id) | ON DELETE CASCADE |
| status | TEXT | `lobby` \| `ended` |
| policy_json | TEXT | JSON blob (permissions toggles) |
| join_password_hash | TEXT NULL | Argon2 hash of room password |
| created_ts | INTEGER | unix ts |
| updated_ts | INTEGER | unix ts |

#### `watch_party_member`
| Column | Type | Notes |
|---|---|---|
| room_id | TEXT FK watch_party_room(id) | ON DELETE CASCADE |
| user_id | TEXT FK user(id) | ON DELETE CASCADE |
| role | TEXT | `host` \| `controller` \| `viewer` |
| status | TEXT | `invited` \| `joined` \| `declined` \| `left` |
| invited_by | TEXT NULL FK user(id) | host id typically |
| invited_ts | INTEGER NULL | unix ts |
| joined_ts | INTEGER NULL | unix ts |
| last_seen_ts | INTEGER NULL | unix ts (optional) |
| PRIMARY KEY | (room_id, user_id) |  |

Indexes:
- `idx_watch_party_member_user` on `(user_id, status)` for inbox queries.

### 2.2 Policy JSON shape (server-validated)
Example:
```json
{
  "allow_non_host_play_pause": true,
  "allow_non_host_seek": false,
  "default_join_role": "viewer"
}
```

---

## 3) Backend (Rust) implementation

### 3.1 Files to add / modify (server)

**Modify**
- `crates/server/src/lib.rs` — `pub mod watch_party;`
- `crates/server/src/state.rs`
  - add `watch_party: Arc<WatchPartyManager>` to `AppState`
  - optionally extend `ServerEvent` later (not required for MVP)
- `crates/server/src/main.rs` — instantiate `WatchPartyManager` and pass to `AppState`
- `crates/server/src/routes.rs` — nest watch party router

**Add** (new module)
- `crates/server/src/watch_party/mod.rs`
- `crates/server/src/watch_party/router.rs` — `watch_party_router()`
- `crates/server/src/watch_party/handlers.rs` — REST endpoints
- `crates/server/src/watch_party/ws.rs` — WebSocket upgrade + loop
- `crates/server/src/watch_party/protocol.rs` — serde message types
- `crates/server/src/watch_party/manager.rs` — runtime rooms + state
- `crates/server/src/watch_party/permissions.rs` — centralized policy checks

### 3.2 Files to add / modify (db)

**Add**
- `crates/db/migrations/006_watch_party.sql`
- `crates/db/src/repo/watch_party.rs`

**Modify**
- `crates/db/src/repo/mod.rs` — `pub mod watch_party;`

### 3.3 Cargo changes
Enable Axum WebSocket feature:
- Update workspace dependency in root `Cargo.toml`:
  - `axum = { version = "0.8", features = ["macros", "ws"] }`

### 3.4 REST API design (v1)
All routes below live under `/api/v1/watch-party`.

#### 3.4.1 Media eligibility for a selected invite set
- `POST /eligible-libraries`

Request:
```json
{ "user_ids": ["uuid1", "uuid2"] }
```
Response:
```json
{ "library_ids": ["lib1", "lib2"] }
```
Notes:
- The backend computes intersection between:
  - current user’s accessible libraries
  - each selected user’s accessible libraries

#### 3.4.2 Create room
- `POST /rooms`

Request:
```json
{
  "item_id": "...",
  "invites": [
    {"user_id":"...","role":"viewer"},
    {"user_id":"...","role":"controller"}
  ],
  "password": "optional",
  "policy": {
    "allow_non_host_play_pause": true,
    "allow_non_host_seek": false,
    "default_join_role": "viewer"
  }
}
```
Response:
```json
{ "room_id": "...", "join_path": "/watch-party/rooms/..." }
```
Validation:
- `item_id` exists and is playable (`movie` or `episode`).
- host has access to item’s library.
- each invited user exists.
- each invited user has access to item’s library.

#### 3.4.3 Room details
- `GET /rooms/{room_id}`

Response:
```json
{
  "room_id":"...",
  "item_id":"...",
  "host_user_id":"...",
  "status":"lobby",
  "password_required": true,
  "policy": { ... },
  "members": [
    {"user_id":"...","username":"...","role":"host","status":"joined"}
  ]
}
```

#### 3.4.4 Join room
- `POST /rooms/{room_id}/join`

Request:
```json
{ "password": "optional" }
```
Response:
```json
{ "ok": true, "role": "viewer" }
```
Rules:
- deny if room ended
- deny if user lacks library access
- if password is set, require it (except host)
- create membership row if missing

#### 3.4.5 Leave / end room
- `POST /rooms/{room_id}/leave`
- `POST /rooms/{room_id}/end` (host only)

#### 3.4.6 Inbox
- `GET /invites`

Response:
```json
[
  {
    "room_id":"...",
    "item_id":"...",
    "item_title":"...",
    "host_username":"...",
    "created_ts":123,
    "password_required":true,
    "role":"viewer"
  }
]
```

- `POST /invites/{room_id}/decline`

### 3.5 WebSocket design

#### 3.5.1 Auth problem + solution (short-lived room WS token)
Browsers can’t set custom headers in the WebSocket handshake. The safe pattern is:
1) fetch a short-lived token using the normal REST auth header,
2) open the WS with that short-lived token.

Add:
- `POST /rooms/{room_id}/ws-token` → returns `{ token }`

Token contains:
- `aud = "watch_party"`
- `sub = user_id`
- `room_id`
- short expiration (e.g. 60s)

Client then connects:
- `wss://{host}/api/v1/watch-party/rooms/{room_id}/ws?t={token}`

#### 3.5.2 Protocol messages
All JSON with `type` discriminator.

Server → Client:
- `state`
```json
{
  "type":"state",
  "room_id":"...",
  "item_id":"...",
  "playing":false,
  "position_ms":0,
  "updated_ts_ms":1700000000000,
  "server_ts_ms":1700000000000,
  "members":[{"user_id":"...","username":"...","role":"host","connected":true}]
}
```

- `presence`
```json
{ "type":"presence", "user_id":"...", "connected":true }
```

Client → Server:
- `play`
```json
{ "type":"play", "position_ms": 123456 }
```
- `pause`
```json
{ "type":"pause", "position_ms": 123456 }
```
- `seek`
```json
{ "type":"seek", "position_ms": 123456 }
```

#### 3.5.3 Server-side enforcement
Server accepts commands only if:
- sender is host, OR
- sender is controller AND room policy allows the action.

If not allowed, server replies with an `error` message and immediately re-broadcasts the current authoritative state (so the client snaps back).

### 3.6 Runtime manager details

Add `WatchPartyManager` to `AppState`. Responsibilities:
- Load room metadata from DB on demand.
- Maintain in-memory playback state.
- Track connected users.
- Provide a broadcast channel per room for fanout.
- Optional cleanup task to evict idle rooms.

Pseudo-structure:
```rust
pub struct WatchPartyManager {
  rooms: tokio::sync::RwLock<HashMap<String, Arc<RoomRuntime>>>,
}

pub struct RoomRuntime {
  meta: RoomMeta,
  state: tokio::sync::RwLock<PlaybackState>,
  tx: tokio::sync::broadcast::Sender<ServerMsg>,
  connected: tokio::sync::RwLock<HashSet<String>>,
}
```

---

## 4) Frontend (Next.js) implementation

### 4.1 Files to add / modify

**Modify**
- `ui/src/app/NavBar.tsx` — add **Watch Party** button
- `ui/src/lib/api.ts` — no changes required

**Add**
- `ui/src/app/watch-party/page.tsx` — create + invites screen
- `ui/src/app/watch-party/rooms/[roomId]/page.tsx` — lobby + playback
- `ui/src/lib/watchPartyApi.ts` — API wrappers + types
- `ui/src/app/watch-party/components/MediaPicker.tsx`
- `ui/src/app/watch-party/components/UserInvitePicker.tsx`
- `ui/src/app/watch-party/components/RoomOptions.tsx`
- `ui/src/app/watch-party/components/InvitesPanel.tsx`
- `ui/src/app/watch-party/hooks/useWatchPartySocket.ts`

### 4.2 Create page layout
Two-column responsive grid:
- Left: media
  - libraries list
  - grid of items
  - hierarchical drilldown for series→seasons→episodes
  - search input
  - selected item “pill”
- Right: invites
  - list of users with toggle
  - role dropdown per user (viewer/controller)
  - room options: password, allow play/pause, allow seek
  - create button

Eligibility logic:
- On initial load: fetch host-visible libraries.
- On invitee change: call `POST /eligible-libraries` and disable non-common libraries.

### 4.3 Room page
Panels:
- Video player (reuse logic from `/player/[id]` but wrap in a component)
- Roster panel
- Copy link button
- If password required and not joined: show password prompt

Playback sync:
- Open WS after join succeeds.
- Apply `state` updates to the HTML5 video element.
- Emit play/pause/seek messages when local user controls (if allowed).

---

## 5) Step-by-step implementation plan (incremental)

### Step 1 — DB migration + repo
1. Add `006_watch_party.sql`.
2. Add `repo/watch_party.rs` with CRUD.
3. Add unit tests in db crate (optional, but recommended).

### Step 2 — REST endpoints (no WebSockets yet)
1. Add router nest `/watch-party`.
2. Implement:
   - eligible libraries
   - create room
   - get room
   - join room
   - invites inbox
3. Add integration tests in `crates/server/tests/integration.rs`.

### Step 3 — WebSocket plumbing
1. Enable axum `ws` feature.
2. Implement WS token issuance endpoint.
3. Add WS endpoint and in-memory room runtime.
4. Minimal sync: broadcast play/pause/seek and apply state.

### Step 4 — UI create page
1. Add navbar link.
2. Implement create page with media picker and invite picker.
3. Call create endpoint and navigate to room.

### Step 5 — UI room page with sync
1. Add password join prompt.
2. Refactor `/player/[id]` logic into a reusable component.
3. Implement WS hook and state application.

### Step 6 — Inbox + “notifications”
MVP:
- Poll `GET /invites` every 20–30s and show a badge.

Upgrade path:
- Add a small authenticated WebSocket “notification channel” using the same short-lived token pattern.

---

## 6) Edge cases / risk register

- **TV libraries**: top-level items are series; must drill down to episodes.
- **Large libraries**: current API returns all items; consider pagination later.
- **Auth limitations**: browser WS can’t send Authorization headers, so use short-lived WS tokens.
- **Password storage**: always store an Argon2 hash (never plaintext).
- **Invite privacy**: inbox lists only invitations for the current user.
- **Room takeover**: if host disconnects, room continues; consider “host reassignment” later.

---

## 7) Optional “v2” enhancements

- Persist room playback state every N seconds for crash recovery.
- Add chat.
- Add per-user fine-grained permissions beyond role.
- Add ready-state gating (“start when everyone ready”).
- Add drift smoothing using measured RTT / ping.

