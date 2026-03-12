# Rustyfin User Account And Activity Insights Design

Date: 2026-03-12

## Goal

Add a first-class user account surface to Rustyfin that:

- makes the username in the top navigation clickable
- gives every authenticated user a dedicated account page
- keeps the existing channels self-settings experience useful
- allows self-service display name changes, password changes, and profile picture management
- shows simple, meaningful personal usage stats
- tracks those stats with Rust-owned backend logic wherever possible

The feature should stay consistent with Rustyfin's current architecture:

- browser-only logic for device enumeration and route-presence timing
- Rust backend ownership for auth, storage, activity session lifecycles, aggregation, and authorization
- PostgreSQL as the durable source of truth

## Current State

Relevant existing pieces already in the repo:

- Top nav username chip exists in `ui/src/app/NavBar.tsx`, but it is not a link.
- Channels has a self-settings modal in `ui/src/app/channels/components/ChannelUserSettings.tsx`.
- Profile, avatar, and preferences endpoints already exist:
  - `GET/PATCH /users/me/profile`
  - `POST/DELETE /users/me/avatar`
  - `GET/PATCH /users/me/preferences`
  - implemented in `crates/server/src/routes.rs`
  - consumed in `ui/src/lib/userProfileApi.ts`
- Playback progress already persists through `POST /playback/progress`, with client snapshots every 10 seconds in `ui/src/app/player/[id]/page.tsx`.
- Watch party room membership already stores `status`, `joined_ts`, and `last_seen_ts` in `watch_party_member`, but that table is membership state, not a historical session ledger.
- Voice channel presence is runtime-only in `crates/server/src/channels/manager.rs`; there is no durable per-user voice session history.

Missing pieces:

- no dedicated `/account` page
- no self-service password change endpoint
- no durable user activity session tables
- no activity summary API
- no consistent plan for section-level time spent across Channels, Rooms, Servers, Calendar, Libraries, Admin, and Account

## Industry Patterns And Gaps

The current document covers the core Rustyfin-specific need, but mature web platforms usually split account management into a few stable areas:

- `Profile`: name, avatar, short bio/about, pronouns, time zone, status, profile visibility
- `Preferences`: notifications, do-not-disturb schedule, appearance, accessibility, device defaults
- `Security`: password, MFA/passkeys, active sessions/devices, security events, recovery methods
- `Data & Privacy`: activity controls, personalization toggles, profile discoverability, export/download, deletion/retention
- `Activity`: recent activity, usage summaries, history, top items, and sometimes recommendations

Compared with GitHub, Google, Discord, and Slack, the biggest gaps in the current Rustyfin plan are:

- no `Data & Privacy` section, even though the feature introduces new personal activity tracking
- no long-term plan for `active sessions/devices` and `revoke other sessions`
- no typed Rust model for user preferences; everything still relies on a loose JSON blob
- no place for high-value profile metadata like `time zone`, which is especially useful for Calendar
- no explicit path for future accessibility, notification schedule, or status/availability preferences
- no self-service data export or activity-history clearing plan

For Rustyfin, the right move is not to implement every feature immediately. The right move is to structure the account system the way mature platforms do, then phase it in with Rust-owned storage and logic.

## Product Scope

### v1 user-facing behavior

Add a new account destination at `/account`.

Entry points:

- clicking the username in the top nav opens `/account`
- the channels sidebar settings button remains, but it should either:
  - open a compact version of the same account controls, or
  - include a clear `Open account page` action
- profile edits made from `/account` or the channels settings shortcut must update the other surface immediately for the same signed-in user

Recommended account page sections:

1. Profile
- avatar upload/remove
- display name edit
- login username shown read-only
- role shown read-only
- optional time zone field
- leave room for future `about me`, `pronouns`, and status fields without redesigning the page

2. Preferences & Devices
- audio input selection
- audio output selection
- reuse the current preferences schema under `prefs.audio`
- reserve account space for future notification, appearance, and accessibility settings

3. Security
- change password form
- current password
- new password
- confirm new password
- show a placeholder/summary for active sessions once session-backed auth exists

4. Data & Privacy
- explain what activity data Rustyfin stores for personal insights
- allow users to disable personal activity insight persistence
- allow users to clear their stored activity history
- leave room for future data export and account-data download flows

5. Activity
- total time by top-level product area
- time spent in watch rooms
- time spent in voice channels
- watch time for library media
- simple top lists such as most-used rooms, most-used voice channels, and most-watched items
- a small recent activity feed

### v1 profile and preference fields

Recommended v1 fields:

- `display_name`
- `avatar`
- `time_zone` as an IANA zone ID such as `Europe/Dublin`
- audio input and output device preferences

Recommended v2 profile fields:

- `about_me` with a short length cap
- `pronouns`
- `status_text`
- `status_expires_ts`
- `availability_mode` such as `available`, `busy`, `away`, `do_not_disturb`

### v1 stats to show

Keep the stats intentionally small and understandable.

Recommended cards:

- `Time on Rustyfin` over last 7 days / 30 days / all time
- `Rooms time`
- `Voice time`
- `Media watch time`
- `Recent activity`
- `Most used sections`
- `Top rooms`
- `Top voice channels`
- `Top watched media`
- `Sessions started` counts for rooms, voice, and playback

Recommended time windows:

- `7 days`
- `30 days`
- `All time`

### What not to include in v1

- public user profiles
- profile comments / social features
- per-message or per-click analytics
- admin inspection of another user's private activity page
- cross-device sign-out everywhere
- complex charts that require a separate analytics stack

Note on `device history` and `active sessions`:

- active sessions are common and valuable, but Rustyfin cannot implement them correctly on the current pure stateless JWT model
- the document should plan for them now, but not promise them in the first page release unless auth sessions are added

## Key Product Decisions

### 1. Make `/account` the canonical self-service surface

Do not create a second independent settings system. The current channels modal should be refactored to reuse the same account sections or hook layer used by `/account`.

Recommended structure:

- new route: `ui/src/app/account/page.tsx`
- extract shared UI/state from `ChannelUserSettings.tsx` into reusable account components/hooks
- keep channels settings as a shortcut, not a fork

Recommended information architecture:

- `Profile`
- `Preferences`
- `Security`
- `Data & Privacy`
- `Activity`

### 1a. Keep current-user profile state single-sourced and live-synced

The channels footer profile card, the top-nav username/avatar, and the `/account` page must all read from one canonical current-user state.

Recommended behavior:

- if the user changes display name or avatar in `/account`, the channels page footer updates immediately
- if the user changes display name or avatar from channels settings, `/account` updates immediately
- the top nav updates at the same time
- if the same user has multiple tabs open, other tabs update within the same session without a full reload

Recommended Rustyfin implementation:

- keep `AuthContext` as the canonical current-user source
- extend it with a mutation helper such as `setMe` or `applyMeProfileUpdate`
- have both `/account` and `ChannelUserSettings` call the same shared mutation hook
- after a successful profile or avatar mutation, write the updated `me` payload into:
  - React context state
  - the existing localStorage cache
  - a `BroadcastChannel`, for example `rustfin-me`, for cross-tab sync
- add a `storage` event fallback for browsers without `BroadcastChannel`

This avoids polling and keeps the sync mostly client-local. The backend only needs to return the updated profile payload it already owns.

### 2. Track activity with coarse sessions, not noisy event logs

Do not log every click. Store bounded activity sessions with start, heartbeat, and stop timestamps, then roll them up into daily aggregates.

This keeps:

- the data understandable
- the storage size under control
- the queries simple
- the implementation mostly in Rust

### 3. Use server-authoritative tracking where Rustyfin already owns the lifecycle

Use server-side tracking for:

- voice channel time
- watch room time
- media watch time

Use client-side timing only for:

- generic top-level section presence such as `channels`, `rooms`, `servers`, `calendar`, `libraries`, `admin`, `account`

### 4. Treat section time as approximate, and voice/room/playback time as authoritative

Section presence depends on browser tabs, visibility, and unload behavior. It is still useful, but it should be described as approximate browser time.

Voice, room, and playback timing can be much more trustworthy because Rustyfin already owns those session boundaries.

### 5. Use typed Rust preference models, not unbounded ad-hoc JSON

The current `user_pref.json` storage is flexible, but the backend should stop treating it as an untyped blob once `/account` becomes a real product surface.

Recommended direction:

- keep the existing JSON column
- decode it into versioned Rust structs
- validate and normalize on the server
- expose typed DTOs to the frontend instead of `Record<string, unknown>`

This preserves migration simplicity while keeping the product Rust-first.

### 6. Separate `Activity` from `Data & Privacy`

Platforms like Google and Discord make a clear distinction between:

- data controls
- personalization or improvement toggles
- the data the user can view later

Rustyfin should do the same.

The account page should not mix:

- analytics summaries the user wants to see
- controls about whether Rustyfin is allowed to persist those insights

Those should be adjacent sections, not one combined panel.

### 7. Plan now for session-backed auth, even if it ships after `/account`

GitHub and Google both expose active devices or sessions, and users now expect that from any serious account area.

Rustyfin does not need to implement this in the first `/account` release, but the document should explicitly plan for:

- active session listing
- revoke session
- revoke other sessions
- forced re-auth for sensitive changes

without pretending the current 24-hour stateless JWT model can provide that cleanly.

## Proposed Architecture

### Frontend

### New route

- `ui/src/app/account/page.tsx`

Recommended page layout:

- header card with avatar, display name, login name, role
- two-column grid on desktop
- sections:
  - Profile
  - Preferences
  - Security
  - Data & Privacy
  - Activity

### Shared account UI extraction

Refactor `ui/src/app/channels/components/ChannelUserSettings.tsx` into reusable building blocks:

- `ui/src/app/account/components/AccountProfileSection.tsx`
- `ui/src/app/account/components/AccountPreferencesSection.tsx`
- `ui/src/app/account/components/AccountSecuritySection.tsx`
- `ui/src/app/account/components/AccountDataPrivacySection.tsx`
- `ui/src/app/account/components/AccountActivitySection.tsx`
- `ui/src/app/account/hooks/useMyAccount.ts`

Then:

- `/account` composes all sections
- channels settings either uses the same sections inside a modal, or becomes a smaller shortcut modal focused on audio + profile + link to full account
- audio device controls can remain embedded in channels for fast live-call access, but should reuse the same preferences hook and typed DTOs

### Current-user sync layer

Add a small shared client layer so profile mutations propagate immediately across Rustyfin surfaces.

Recommended pieces:

- extend `ui/src/lib/auth.tsx` with:
  - `updateMe(next: Partial<Me>)`
  - `replaceMe(next: Me)`
- add a shared account mutation hook in `ui/src/app/account/hooks/useMyAccount.ts`
- have `ChannelUserSettings` and `/account` both use that hook for:
  - display name save
  - avatar upload
  - avatar delete

Recommended mutation flow:

1. submit to `/users/me/profile` or `/users/me/avatar`
2. receive the updated profile from the Rust backend
3. normalize it into the shared `Me` shape
4. update `AuthContext`
5. update localStorage cache
6. publish the change over `BroadcastChannel`

This is the simplest way to guarantee that:

- channels footer profile state
- `/account` header/profile state
- top navigation username/avatar

all stay in sync without page refreshes.

### Top navigation change

Update `ui/src/app/NavBar.tsx` so the displayed username becomes a `Link` to `/account` on both desktop and mobile layouts.

The nav should also subscribe to the same current-user state so display name and avatar updates appear there immediately after either account-surface mutation succeeds.

### Section presence tracking

Add a lightweight client provider in `ui/src/app/providers.tsx`:

- observes pathname changes
- maps pathname to a top-level section key
- starts a browser-presence session
- sends heartbeats only while the tab is visible
- closes the session on route change, `pagehide`, and sign-out

Recommended section mapping:

- `/channels` -> `channels`
- `/rooms` -> `rooms`
- `/servers` -> `servers`
- `/calendar` -> `calendar`
- `/libraries` -> `libraries`
- `/admin` -> `admin`
- `/account` -> `account`
- `/player/*` -> `libraries`
- everything else authenticated -> `home`

Use:

- a generated `tab_id`
- a generated `client_session_id`
- `navigator.sendBeacon()` for unload where available
- `fetch(..., { keepalive: true })` fallback

### Backend

### Keep Rust authoritative

All of the following should live in Rust:

- password change validation and persistence
- activity session lifecycle storage
- activity rollups
- summary queries for account stats
- voice, room, and playback activity tracking

Recommended backend additions:

- new module: `crates/server/src/user_activity.rs`
- new repo module: `crates/db/src/repo/user_activity.rs`
- new migration for activity tables
- route handlers added in `crates/server/src/routes.rs`
- typed preferences models, for example in `crates/server/src/account_prefs.rs`

### Rust-first preferences model

Recommended server-side shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UserPreferences {
    pub version: u32,
    pub audio: AudioPreferences,
    pub activity: ActivityPreferences,
    pub privacy: PrivacyPreferences,
    pub notifications: NotificationPreferences,
    pub accessibility: AccessibilityPreferences,
    pub appearance: AppearancePreferences,
}
```

Initial v1 sections would only actively use a subset of this model, but defining the typed container early prevents the account page from growing into unstructured JSON over time.

## Data Model

### 1. Account and auth additions

### Self-service password change

Add a new endpoint:

- `POST /users/me/password`

Request:

```json
{
  "current_password": "old-secret",
  "new_password": "new-secret",
  "confirm_password": "new-secret"
}
```

Rules:

- current password must verify against the stored hash
- new password must pass the existing shared password rules from `crates/server/src/user_pipeline.rs`
- confirm must match
- new password cannot equal current password

Repository change needed:

- add `update_password(pool, user_id, new_password)` to `crates/db/src/repo/users.rs`

### JWT note

Rustyfin currently uses stateless 24-hour JWTs from `crates/server/src/auth.rs`. That means a password change cannot instantly revoke every already-issued token without a broader auth redesign.

Recommended v1 behavior:

- update the password hash
- clear the current browser token and force re-login after success
- document that previously issued tokens may remain valid until expiry

Recommended v2 improvement:

- add token versioning or server-backed auth sessions if immediate global revocation is required

### Recommended phase-2 auth session model

If Rustyfin wants a serious `Security` section, it should introduce session-backed auth instead of relying only on long-lived stateless JWTs.

Recommended shape:

- short-lived access JWT, for example 15 to 30 minutes
- server-backed refresh or session token stored hashed in PostgreSQL
- one row per signed-in browser or device

Recommended table:

- `auth_session`
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE`
  - `session_token_hash TEXT NOT NULL`
  - `created_ts BIGINT NOT NULL`
  - `last_seen_ts BIGINT NOT NULL`
  - `expires_ts BIGINT NOT NULL`
  - `revoked_ts BIGINT`
  - `user_agent TEXT`
  - `ip_address_hash TEXT`
  - `device_label TEXT`
  - `is_current BOOLEAN` computed in responses, not stored

This unlocks:

- `GET /users/me/sessions`
- `DELETE /users/me/sessions/{id}`
- `POST /users/me/sessions/revoke-others`
- forced re-auth before password change or future sensitive actions
- a recent security events panel

Rust-first implementation notes:

- hashing session tokens keeps server storage safer if the database leaks
- storing an IP hash instead of raw IP reduces retained personal data
- the auth/session logic should stay inside Rust route handlers and repo functions, not in browser-only state

### Optional profile metadata

Common platforms expose more than just a name and avatar. For Rustyfin, the most justifiable additions are:

- `time_zone`
- `about_me`
- `pronouns`
- `status_text`
- `status_expires_ts`

Recommended storage:

- identity-like fields used broadly across the product should live in first-class Rust types and database columns or a dedicated `user_profile` table
- purely local client defaults should stay in typed preferences

For Rustyfin specifically:

- `time_zone` is high-value because Calendar already exists
- `status_text` is potentially useful in Channels and Rooms
- `about_me` and `pronouns` are optional and should remain lightweight if added

### 2. Activity storage

Do not overload `watch_party_member` or `user_pref` for analytics.

Add two new tables:

### `user_activity_session`

Purpose:

- raw bounded sessions
- supports debugging, recent activity, and precise rollup generation

Recommended columns:

- `id TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE`
- `activity_kind TEXT NOT NULL`
- `top_section TEXT NOT NULL`
- `subject_id TEXT`
- `subject_label TEXT`
- `source TEXT NOT NULL`
- `client_session_id TEXT`
- `tab_id TEXT`
- `started_ts BIGINT NOT NULL`
- `last_seen_ts BIGINT NOT NULL`
- `ended_ts BIGINT`
- `duration_secs BIGINT NOT NULL DEFAULT 0`
- `metadata_json TEXT NOT NULL DEFAULT '{}'`

Recommended `activity_kind` values:

- `section_presence`
- `voice_channel`
- `watch_room`
- `media_playback`

Recommended `source` values:

- `browser`
- `channels_ws`
- `watch_party_ws`
- `playback`

### `user_activity_daily`

Purpose:

- fast account-page queries
- long-term retention without keeping every raw session forever

Recommended columns:

- `user_id TEXT NOT NULL`
- `activity_day DATE NOT NULL`
- `activity_kind TEXT NOT NULL`
- `top_section TEXT NOT NULL`
- `subject_id TEXT`
- `total_duration_secs BIGINT NOT NULL DEFAULT 0`
- `session_count BIGINT NOT NULL DEFAULT 0`
- `last_activity_ts BIGINT`
- `metadata_json TEXT NOT NULL DEFAULT '{}'`
- primary key across the identifying dimensions

Retention policy:

- keep `user_activity_session` for 90 days
- keep `user_activity_daily` indefinitely

### 3. Data and privacy controls

Since Rustyfin will now persist personal activity insights, add a small typed privacy model as part of account preferences.

Recommended fields:

- `activity_insights_enabled: bool`
- `allow_browser_section_presence: bool`
- `allow_service_usage_rollups: bool`
- `activity_retention_days_override: Option<u32>` only if per-user retention is ever supported

Recommended v1 behavior:

- default personal activity insights to `enabled`
- show a clear explanation of what is stored
- let the user disable future persistence
- let the user delete their existing stored activity summaries and sessions

If a user disables activity insights:

- service-critical ephemeral runtime signals still exist so Rooms, Channels, and Playback work
- persistent writes to `user_activity_session` and `user_activity_daily` stop for that user
- the Activity page shows a disabled-state explanation instead of blank charts

This matches the pattern used by platforms that separate product operation from personalization and analytics storage.

## Tracking Rules

### 1. Section presence

Ownership: browser plus Rust endpoint.

Flow:

1. client starts section session on route entry
2. client heartbeats every 30 seconds while visible
3. client stops session on route change or unload
4. backend closes stale browser sessions after a timeout, for example 90 seconds without heartbeat

Counting rule:

- duration is wall-clock time between `started_ts` and `ended_ts`
- clamp stale sessions to the last heartbeat window

Known limitation:

- multiple open tabs can inflate section totals

That is acceptable for v1 if the UI labels these metrics as approximate browser time.

Privacy rule:

- if `allow_browser_section_presence` is `false`, do not persist section presence sessions at all

### 2. Voice channel time

Ownership: Rust channels websocket.

Track in:

- `crates/server/src/channels/ws.rs`
- `crates/server/src/channels/manager.rs`

Flow:

1. open a `voice_channel` activity session when `join_voice` succeeds
2. close it on `leave_voice`
3. close it on websocket disconnect / `leave_all_voice`
4. if the same user switches channels, close the prior session before opening the new one

Metadata to store:

- `channel_id`
- channel name snapshot

### 3. Watch room time

Ownership: Rust watch party handlers and websocket.

Track in:

- `crates/server/src/watch_party/handlers.rs`
- `crates/server/src/watch_party/ws.rs`

Flow:

1. open a `watch_room` activity session when room join succeeds
2. update heartbeats from the existing room websocket activity / member `last_seen_ts`
3. close on `leave_room`
4. close on websocket disconnect or idle timeout

Metadata to store:

- `room_id`
- room name snapshot
- `room_mode`

Important detail:

Do not try to derive multi-session history from `watch_party_member.joined_ts`; that field is membership state, not a proper session ledger.

### 4. Media watch time

Ownership: Rust playback handlers, using the client progress reports as inputs.

Track in:

- `crates/server/src/routes.rs`
- `crates/db/src/repo/playstate.rs`
- new `user_activity` repo logic

Flow:

1. open a `media_playback` session when `/playback/sessions` is created
2. update it when `/playback/progress` arrives
3. close it when `/playback/sessions/{sid}/stop` runs, or when a cleanup task expires it

Counting rule:

- count watched seconds from forward playback progress deltas
- do not count seek jumps as watch time
- cap credited watch time by wall-clock elapsed time between updates

This prevents:

- scrubbing to the end from looking like real watch time
- accidental inflation from noisy progress updates

Metadata to store:

- `item_id`
- title snapshot
- `library_id`
- optionally `file_id`

## Common Account Features And Rustyfin Recommendations

The table below reflects common account patterns across GitHub, Google, Discord, and Slack, and the recommended Rustyfin posture.

| Feature area | Common on other platforms | Rustyfin recommendation |
| --- | --- | --- |
| Profile identity | Avatar, name, bio/about, pronouns, location or time zone, status | Ship avatar, display name, and time zone first; keep bio/pronouns/status as phase-2 fields |
| Preferences | Notifications, DND/schedules, appearance, accessibility, device defaults | Ship audio device preferences first; define typed preference sections now for notifications, accessibility, and appearance later |
| Security | Password change, MFA/passkeys, sessions/devices, recent security activity, recovery methods | Ship password change now; plan session-backed auth next; passkeys/TOTP after that |
| Privacy | Visibility controls, discoverability, blocking, personalization toggles | Add a `Data & Privacy` section now for activity-insight controls and retention explanation |
| Data control | Export/download data, request copy, clear history, delete account | Add personal data export and clear-activity as follow-on features once activity storage ships |
| Activity | View history, delete history, top activity summaries | Ship simple time-and-top-list summaries with recent activity and clear-history |

## Security Roadmap Beyond v1

Common account systems usually keep improving after the first account page release. For Rustyfin, the highest-value future items are:

### MFA and passkeys

Recommended Rust-first path:

- WebAuthn/passkeys with [`webauthn-rs`](https://github.com/kanidm/webauthn-rs)
- optional TOTP with a Rust library such as `totp-rs`
- hashed recovery codes stored in PostgreSQL

This would align Rustyfin with the security direction used by GitHub, Google, and Discord.

### Recent security events

Recommended future table:

- `account_security_event`
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL`
  - `event_kind TEXT NOT NULL`
  - `created_ts BIGINT NOT NULL`
  - `session_id TEXT`
  - `metadata_json TEXT NOT NULL DEFAULT '{}'`

Example events:

- `password_changed`
- `session_revoked`
- `new_login`
- `profile_updated`
- `avatar_removed`

This would support a compact `Recent security activity` panel in `/account`.

## API Plan

Reuse existing endpoints where possible.

### Existing endpoints to keep

- `GET /users/me`
- `GET/PATCH /users/me/profile`
- `POST/DELETE /users/me/avatar`
- `GET/PATCH /users/me/preferences`

### New endpoints

- `POST /users/me/password`
- `POST /users/me/activity/browser`
  - generic start / heartbeat / stop for section presence
- `GET /users/me/activity/summary?range=7d|30d|all`
- `GET /users/me/activity/top?kind=section|voice|room|media&range=...&limit=...`
- `DELETE /users/me/activity`
  - clears persisted personal activity sessions and daily rollups for that user

Recommended future endpoints once session-backed auth exists:

- `GET /users/me/sessions`
- `DELETE /users/me/sessions/{id}`
- `POST /users/me/sessions/revoke-others`

Recommended future endpoint once data export is added:

- `POST /users/me/data-export`

Recommended browser activity request body:

```json
{
  "action": "start",
  "activity_kind": "section_presence",
  "top_section": "rooms",
  "pathname": "/rooms/abc",
  "client_session_id": "uuid",
  "tab_id": "uuid"
}
```

The summary response should be shaped for the account page directly, for example:

```json
{
  "range": "30d",
  "totals": {
    "time_on_rustyfin_secs": 12345,
    "rooms_secs": 3210,
    "voice_secs": 1880,
    "media_watch_secs": 5420
  },
  "recent_activity": [
    { "kind": "watch_room", "label": "Movie Night", "started_ts": 1760000000, "duration_secs": 5400 }
  ],
  "section_totals": [
    { "section": "rooms", "seconds": 3210 },
    { "section": "channels", "seconds": 2100 }
  ],
  "top_rooms": [
    { "room_id": "r1", "room_name": "Movie Night", "seconds": 2400 }
  ],
  "top_voice_channels": [
    { "channel_id": "c1", "channel_name": "General", "seconds": 1700 }
  ],
  "top_media": [
    { "item_id": "i1", "title": "Blade Runner", "seconds": 3600 }
  ]
}
```

## UI Plan

### `/account` page

Recommended sections:

### Header

- large avatar
- display name
- login username
- role badge
- optional time zone summary

Recommended navigation pattern:

- a compact local tab bar or left rail with:
  - `Profile`
  - `Preferences`
  - `Security`
  - `Data & Privacy`
  - `Activity`

### Profile section

- edit display name
- upload / remove avatar
- optional time zone field
- reserve room for future `about me`, `pronouns`, and `status`

### Preferences section

- move the existing audio input/output controls here
- keep the current browser capability messaging, but keep it compact
- leave room for future:
  - notification schedule
  - do not disturb
  - accessibility toggles
  - appearance/theme preferences

### Security section

- current password
- new password
- confirm new password
- success state should clear fields immediately
- if session-backed auth exists, add `Active sessions` and `Sign out other sessions`

### Data & Privacy section

- explain what activity Rustyfin stores
- toggle personal activity insights on or off
- show retention period
- clear stored activity history
- later: request account data export

### Activity section

- range selector: `7d`, `30d`, `all`
- summary cards
- one compact section chart/list
- three top lists:
  - rooms
  - voice channels
  - media
- a recent activity list

### Channels sidebar settings behavior

Recommended v1:

- keep the bottom-left settings entry in channels
- reduce it to a quick-settings shortcut
- add `Open account page`
- let it focus on live-call relevant controls, not the full account surface

This avoids user confusion and preserves a fast path for audio device changes during a live voice session.

## Implementation Order

### Phase 1: account route and shared settings refactor

1. Create `/account`
2. Make nav username clickable
3. Extract shared profile/avatar/audio settings logic from `ChannelUserSettings.tsx`
4. Keep channels settings working through the shared implementation
5. Introduce typed Rust preference models behind the existing preferences endpoint

### Phase 2: password change

1. Add `POST /users/me/password`
2. Add shared server-side validation using the existing password rules
3. Force current-browser re-login after a successful change
4. Add rate limits and fresh-auth checks for sensitive account writes where practical

### Phase 3: data and privacy foundations

1. Add new migration for `user_activity_session` and `user_activity_daily`
2. Add repo helpers in `crates/db/src/repo/user_activity.rs`
3. Add privacy preference flags for activity persistence
4. Add `DELETE /users/me/activity`
5. Add stale-session cleanup / rollup maintenance in Rust

### Phase 4: activity tracking and insights UI

1. Add browser section presence endpoint
2. Add server-authoritative voice tracking
3. Add server-authoritative watch room tracking
4. Add media playback session accounting
5. Add summary endpoints
6. Render stats on `/account`
7. Add range selector, top lists, and recent activity

### Phase 5: session-backed auth and active devices

1. Add `auth_session` storage and refresh/session token flow
2. Add active session listing and revoke actions
3. Add recent security events

### Phase 6: advanced account security

1. Add passkeys or WebAuthn
2. Optionally add TOTP and recovery codes
3. Add forced re-auth for sensitive actions using the new auth session model

## Recommended File Targets

Frontend:

- `ui/src/app/NavBar.tsx`
- `ui/src/app/providers.tsx`
- `ui/src/app/account/page.tsx`
- `ui/src/app/account/components/*`
- `ui/src/app/channels/components/ChannelUserSettings.tsx`
- `ui/src/app/channels/page.tsx`
- `ui/src/lib/userProfileApi.ts`
- `ui/src/lib/auth.tsx`

Backend:

- `crates/server/src/routes.rs`
- `crates/server/src/auth.rs`
- `crates/server/src/account_prefs.rs` (new)
- `crates/server/src/channels/ws.rs`
- `crates/server/src/watch_party/handlers.rs`
- `crates/server/src/watch_party/ws.rs`
- `crates/db/src/repo/users.rs`
- `crates/db/src/repo/playstate.rs`
- `crates/db/src/repo/user_activity.rs` (new)
- `crates/db/migrations_pg/<new_migration>.sql`

Future security files if session-backed auth is added:

- `crates/db/src/repo/auth_sessions.rs` (new)
- `crates/db/migrations_pg/<auth_session_migration>.sql`

## Testing Plan

### Backend tests

- password change success
- wrong current password rejected
- invalid new password rejected
- time zone validation and persistence
- typed preference round-trip tests
- browser activity start / heartbeat / stop lifecycle
- stale browser session cleanup
- voice session opens on join and closes on leave/disconnect
- watch room session opens on join and closes on leave/disconnect
- playback watch time ignores large seek jumps
- daily rollups aggregate correctly
- users can only read their own account activity
- users can disable activity persistence and no new activity rows are written after that
- activity clear endpoint removes only the caller's activity data

Future security tests:

- active sessions list excludes revoked sessions
- revoke other sessions keeps current session alive
- passkey registration and assertion verification if WebAuthn is added

### Frontend tests

- nav username links to `/account`
- account page loads existing profile and preferences
- shared sections save correctly
- password form validation and success state
- time zone input save and reload
- data and privacy toggles save and reflect disabled activity state
- section presence hook starts/stops when pathname changes
- channels quick settings still works after refactor
- saving display name in `/account` updates the channels footer and nav immediately
- saving display name in channels settings updates `/account` and nav immediately
- uploading or removing avatar in `/account` updates channels footer and nav immediately
- uploading or removing avatar in channels settings updates `/account` and nav immediately
- cross-tab current-user updates propagate through `BroadcastChannel` or storage fallback

### Manual checks

- changing display name in `/account` updates the channels footer immediately
- changing display name in channels settings updates `/account` immediately
- avatar updates appear immediately in nav, channels, and `/account`
- if `/channels` and `/account` are open in separate tabs, profile updates sync across tabs without reload
- audio device preferences persist across reloads
- leaving a room or voice channel stops that timer
- media watch time rises during real playback but not from seeking around
- disabling activity insights stops new personal activity summaries from appearing
- clearing activity history removes existing summaries for that user only

## Risks And Mitigations

### Risk: duplicated account logic between `/account` and channels modal

Mitigation:

- extract shared hooks/components first
- keep one canonical current-user state in `AuthContext`
- route both profile mutation surfaces through the same shared account hook

### Risk: `/account` and channels settings drift out of sync

Mitigation:

- update `AuthContext` immediately from successful mutation responses
- publish profile changes over `BroadcastChannel`
- use localStorage `storage` event as a compatibility fallback
- avoid duplicating local profile state in isolated components unless it is transient form state

### Risk: section-time inflation from multiple tabs

Mitigation:

- label section time as approximate
- keep authoritative voice/room/playback timers separate

### Risk: watch room data double-counted from join API plus websocket reconnects

Mitigation:

- activity sessions should be idempotent per active room/user/source
- reopening the websocket should heartbeat the same active session when possible

### Risk: password change feels incomplete because old tokens still work

Mitigation:

- explicitly document current 24-hour JWT limitation
- force current-browser re-login
- optionally plan token versioning later

### Risk: users perceive the new activity insights as hidden surveillance

Mitigation:

- add a visible `Data & Privacy` section
- explain exactly what is stored
- allow disabling future persistence
- allow clearing stored activity history

### Risk: the loose `user_pref` JSON becomes unmaintainable as account features grow

Mitigation:

- introduce typed Rust preference models with serde defaults and versioning
- keep validation in Rust handlers rather than the browser

### Risk: active sessions are requested before the auth model can support them cleanly

Mitigation:

- mark session/device management as phase 5
- do not fake a device list from stateless JWTs
- add `auth_session` storage first

### Risk: media watch time inflated by seeks

Mitigation:

- compute watch time from bounded forward deltas, not absolute progress alone

### Risk: unbounded analytics storage

Mitigation:

- keep raw sessions for 90 days
- maintain daily rollups for long-term history

## Recommended Final Shape

The best fit for Rustyfin is:

- `/account` as the canonical user account page
- the existing channels settings entry preserved as a quick shortcut
- a `Data & Privacy` section next to `Activity`
- typed Rust preference models instead of ad-hoc JSON
- Rust-owned session tracking for voice, rooms, and media
- browser-assisted, approximate section timing for top-level navigation areas
- a planned path to active sessions/devices through session-backed auth
- simple summaries and top lists, not a heavyweight analytics subsystem

This gives users a clear personal account hub without fighting the current architecture, and it keeps the important state transitions inside Rust where the repo already has strong control.

## Shipped Implementation Notes

The implemented slice now includes:

- `/account` as a first-class account page
- top-nav username links to `/account`
- shared profile/account mutation logic between `/account` and the channels settings surface
- live current-user sync through the canonical `AuthContext`, localStorage cache updates, `BroadcastChannel`, and `storage` fallback
- self-service display name, avatar, time zone, audio preferences, password change, privacy controls, activity clear, and activity summaries
- typed Rust preference DTOs backed by the existing `user_pref` JSON row
- Rust-owned `user_activity_session` and `user_activity_daily` storage with a Rust maintenance loop for stale browser-session cleanup and daily rollups
- browser section presence tracking from the UI plus Rust-owned room, voice, and playback activity hooks
- bounded media watch-time accumulation so seek jumps do not inflate watch-time totals

Intentional v1 deferral:

- active sessions/devices, revoke-session flows, and recent security events remain deferred
- the current repo still uses stateless 24-hour JWTs, so this pass does not fake a session-management UI on top of an auth model that cannot support it correctly

Implementation detail:

- activity summaries currently read from the durable session ledger for live account views while the daily rollup table is maintained in parallel for retention and future optimization
- the channels settings surface remains a compact shortcut, but it now uses the same shared account hook and update path as `/account`

## External Product References

These references informed the improvements above:

- [GitHub account settings](https://docs.github.com/en/account-and-profile/how-tos/account-settings)
- [GitHub profile customization](https://docs.github.com/en/account-and-profile/tutorials/personalize-your-profile)
- [GitHub sessions](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/viewing-and-managing-your-sessions)
- [GitHub data export](https://docs.github.com/en/get-started/archiving-your-github-personal-account-and-public-repositories/requesting-an-archive-of-your-personal-accounts-data)
- [GitHub passkeys](https://docs.github.com/en/authentication/authenticating-with-a-passkey/signing-in-with-a-passkey)
- [Google strong passwords and recovery info](https://support.google.com/accounts/answer/32040)
- [Google devices with account access](https://support.google.com/accounts/answer/3067630?hl=en)
- [Google profile visibility controls](https://support.google.com/accounts/answer/6304920?co=GENIE.Platform%3DDesktop&hl=en)
- [Google activity controls](https://support.google.com/accounts/answer/6139018?co=GENIE.Platform%3DAndroid&hl=en)
- [Google My Activity controls](https://support.google.com/accounts/answer/9784401?hl=en)
- [Discord custom profiles](https://support.discord.com/hc/en-us/articles/4403147417623-Custom-Profiles-)
- [Discord accessibility settings](https://support.discord.com/hc/en-us/articles/1500010454681-Accessibility-Settings-Tab)
- [Discord blocking and privacy](https://support.discord.com/hc/en-us/articles/217916488-Blocking-Privacy-Settings)
- [Discord data requests](https://support.discord.com/hc/en-us/articles/360004027692-Requesting-a-Copy-of-your-Data)
- [Discord data-use controls](https://support.discord.com/hc/en-us/articles/21864805694999-Data-Used-to-Improve-Discord)
- [Discord personalization controls](https://support.discord.com/hc/en-us/articles/21865322754327-Data-Used-to-Personalize-Discord)
- [Discord MFA](https://support.discord.com/hc/en-us/articles/219576828-Setting-up-Two-Factor-Authentication)
- [Slack profile editing](https://slack.com/help/articles/204092246-edit-your-profile)
- [Slack custom member profiles](https://slack.com/help/articles/212281478-Customize-member-profiles)
- [Slack notifications](https://slack.com/help/articles/201355156-Configure-your-Slack-notifications)
- [Slack do not disturb and notification schedule](https://slack.com/help/articles/214908388-Pause-your-Slack-notifications)
- [Slack discoverability preference](https://slack.com/help/articles/5535411189267-Manage-your-Slack-Connect-discoverability-preference)
