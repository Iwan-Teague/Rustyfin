# Music Library Design

**Date:** 2026-02-23
**Approach:** A — Enhance existing pages

---

## Overview

Add music library support to Rustyfin. The scanner already handles mp3, flac, aac, m4a, ogg, opus, wav, wma, aiff, alac and builds an artist → album → track hierarchy. What's missing is the playback UI and browsing experience.

Music libraries integrate into the existing library/item browser pages rather than getting a separate route. A persistent mini-player bar at the bottom of the viewport plays audio and survives page navigations.

---

## Section 1 — Backend

### Allow `music` library kind
`crates/server/src/routes.rs` — the `create_library` handler currently rejects any kind that isn't `movies` or `tv_shows`. Add `music` to the accepted set.

### Add `duration_ms` to item responses
Add `duration_ms: Option<i64>` to `ItemResponse`. Populate by LEFT JOINing `episode_file_map` + `media_file` in the `get_children` and `get_library_items` DB queries (`crates/db/src/repo/items.rs`). Tracks will have a value; all other kinds get `null`. Backwards compatible — no migration needed.

### Admin dropdown
`ui/src/app/admin/page.tsx` — add `<option value="music">Music</option>` to the library kind `<select>`.

---

## Section 2 — Library Browser (`/libraries/[id]`)

Fetch `GET /libraries/{id}` (in addition to the existing `/libraries/{id}/items` call) to get the library `kind`.

**If `kind === 'music'`:**
- Renders a music-specific layout
- Top-level items are artists (no parent_id) — shown as a 1:1 square art grid
- Music note placeholder if no cover art
- Artist name label below each tile
- Clicking an artist navigates to `/items/{artistId}`

**Otherwise:** renders exactly as today (2:3 poster grid). No regressions.

---

## Section 3 — Item Page (`/items/[id]`)

Branch on `item.kind`:

**`artist`**
- Heading: artist name
- Fetch children (albums) → square art grid
- No play button on the artist itself

**`album`**
- Left: square album art + album title + artist name (fetched from parent item) + year
- "Play Album" button → loads all tracks into queue, starts at index 0
- Children (tracks) rendered as a numbered list:
  ```
  #   Title            Duration   ▶
  1   One More Time    5:45       ▶
  2   Aerodynamic      3:27       ▶
  ```
- Clicking ▶ on a track loads the full album queue and starts at that track's index

**`track`**
- Title + album name + "Play" button
- Starts a single-track queue

**All other kinds:** render exactly as today. No regressions.

---

## Section 4 — Music Player

### `MusicPlayerContext` (`ui/src/lib/musicPlayerContext.tsx`)

React context wrapping the whole app. State:
- `queue: Track[]` — current track list
- `currentIndex: number`
- `playing: boolean`
- `progress: number` — seconds (driven by `<audio>` `ontimeupdate`)
- `duration: number` — seconds
- `volume: number` — 0–1

Actions:
- `playQueue(tracks, startIndex)` — load new queue and start
- `playPause()`
- `seek(seconds)`
- `next()` / `prev()`
- `setVolume(v)`

The `<audio>` element lives inside this context (hidden). On current track change it calls `GET /items/{id}/playback` to get the `direct_url`, sets `audio.src`. On `ended` event it calls `next()`.

### `MiniPlayer` (`ui/src/app/components/MiniPlayer.tsx`)

Fixed bottom bar, only rendered when `queue.length > 0`.

- **Left:** small square album art + track title + artist name
- **Center:** ⏮ prev · ⏸/▶ play-pause · ⏭ next · seek bar · elapsed / duration
- **Right:** volume slider

Wired into `ui/src/app/layout.tsx`. Renders once, persists across navigations. Uses a z-index that stacks below `VoiceBar` if both are visible.

### Track type

```ts
interface Track {
  id: string;
  title: string;
  artist: string;
  albumTitle: string;
  albumArtUrl?: string;
  durationMs?: number;
}
```

---

## Files Touched

| File | Change |
|------|--------|
| `crates/server/src/routes.rs` | Allow `music` kind; add `duration_ms` to `ItemResponse` |
| `crates/db/src/repo/items.rs` | JOIN media_file in get_children + get_library_items |
| `ui/src/app/admin/page.tsx` | Add Music option to kind dropdown |
| `ui/src/app/libraries/[id]/page.tsx` | Branch on music kind → square art grid |
| `ui/src/app/items/[id]/page.tsx` | Handle artist / album / track kinds |
| `ui/src/app/layout.tsx` | Mount MusicPlayerContext + MiniPlayer |
| `ui/src/lib/musicPlayerContext.tsx` | **New** — audio context + hidden `<audio>` |
| `ui/src/app/components/MiniPlayer.tsx` | **New** — fixed bottom player bar |
