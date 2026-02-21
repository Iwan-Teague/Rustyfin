# Rustyfin Playback Deep Dive (build: 8024266)

This document explains **why video playback currently does nothing** (both *Direct Play* and *Transcode (HLS)*) and provides a concrete, file-by-file plan to fix playback end‑to‑end—from **library scanning → item selection → stream delivery → browser playback**—including how to expand supported formats responsibly.

It is based on the attached codebase `Rustyfin-8024266.zip` (repo root `Rustyfin/`) plus a small amount of web‑sourced standards/reference material for browser/media behavior.

---

## 1) The playback pipeline (what *should* happen)

### 1.1 Scan and ingest (server-side)
1. A **Library** is created (kind: movies/tv, and one or more filesystem paths).
2. A scan walks the library paths, detects video files by extension, and writes DB rows:
   - `media_file` row per file (path, size, timestamps, etc.)
   - `item` row(s) representing movie/series/season/episode hierarchy
   - `episode_file_map` row linking an item to its “primary” media file

In this repo, that mapping is explicit:
- Movie ingest writes to `episode_file_map` (reused for movies) in  
  `crates/scanner/src/scan.rs::create_movie_item()`  
- Episode ingest writes to `episode_file_map` in  
  `crates/scanner/src/scan.rs::create_episode_item()`

### 1.2 Browse and select (frontend)
1. UI lists libraries (`/libraries`)
2. UI shows item detail (`/items/:id`)
3. If `item.kind` is `movie` or `episode`, the UI renders **Play Now** and routes to:
   - `/player/:id`

### 1.3 Player behavior (frontend)
The player page needs the **media file ID** for the selected item to build a stream URL.

- Direct Play should set `<video src="/stream/file/{file_id}?token=...">`
- HLS Transcode should POST `/api/v1/playback/sessions` with `{ file_id }`, receive `hls_url`, then play it with:
  - Safari: native HLS
  - Chrome/Firefox: `hls.js` (Media Source Extensions)

References:
- `HTMLMediaElement.canPlayType()` is how the browser reports support for a given MIME type and optional codec string. citeturn0search0turn0search17  
- hls.js plays HLS in browsers via MSE and transmuxing. citeturn0search8

### 1.4 Stream delivery (backend)
Rustyfin provides:
- **Direct file range streaming**: `GET /stream/file/{file_id}` with RFC-style Range support and `206 Partial Content`, `Content-Range`, `Accept-Ranges`. The Range header semantics are standard. citeturn0search4turn0search1  
- **HLS**: `GET /stream/hls/{sid}/master.m3u8` and segment URLs beneath it. Apple’s documentation describes playlists/segments and normal web delivery considerations. citeturn0search15turn0search16turn0search2  
- HLS playlist MIME type is typically `application/vnd.apple.mpegurl`. citeturn0search15turn0search5  

---

## 2) What’s actually happening (symptom: “clicking does nothing”)

### 2.1 The Player never obtains `file_id`, so it never sets a `video.src`
**Frontend player implementation**
- File: `ui/src/app/player/[id]/page.tsx`

It tries to fetch:
```ts
apiJson<{ file_id?: string }>(`/items/${id}`)
  .then((item) => { if (item.file_id) setFileId(item.file_id); })
```

Then:
- Direct Play only sets `videoRef.current.src` if `fileId` is present.
- HLS start function returns immediately if `!fileId`:

```ts
async function startHls() {
  if (!fileId) return;
  ...
}
```

So when `fileId === null`, both buttons are effectively no-ops:
- Clicking **Direct Play** just sets `mode` to `'direct'` (already default), but **doesn’t set a source**
- Clicking **Transcode (HLS)** returns immediately

✅ This matches your report exactly: both buttons “do nothing”.

### 2.2 Why `file_id` is always missing: the server never returns it
**Backend item response type**
- File: `crates/server/src/routes.rs`
- `struct ItemResponse` contains:
  - `id, library_id, kind, parent_id, title, sort_title, year, overview, created_ts, updated_ts`
  - **No `file_id` field**

**Backend item endpoint**
- `GET /api/v1/items/{id}` returns `ItemResponse` via `item_to_response(item)` which also has no `file_id`.

Therefore, the Player’s assumption—“Items have a `file_id` field”—is false for this build.

### 2.3 The DB *does* have the mapping—just not surfaced to the client
There is a DB helper:
- File: `crates/db/src/repo/items.rs`
- Function: `get_item_file_id(pool, item_id)`  
  It selects the file via:
  ```sql
  SELECT file_id FROM episode_file_map WHERE episode_item_id = ? LIMIT 1
  ```

And the scanner writes that mapping for both movies and episodes:
- File: `crates/scanner/src/scan.rs`
- Functions:
  - `create_movie_item()` inserts into `episode_file_map`
  - `create_episode_item()` inserts into `episode_file_map`

So: **the correct data exists**, but the API omits it.

---

## 3) Secondary playback blockers you’ll hit next (even after `file_id` is fixed)

After you add `file_id`, Direct Play and HLS will start making network requests. At that point, these issues become “next failures” you should fix now to avoid a whack-a-mole cycle.

### 3.1 Direct Play “format support” is mostly a browser capability problem
Direct Play streams the file bytes as-is. The browser can only decode certain container+codec combinations.

The robust solution is:
- Use Direct Play when the browser supports the format
- Otherwise fall back to HLS transcode (H.264 + AAC is the most universal baseline for web playback)

The browser signal is `video.canPlayType(mime; codecs=...)`. citeturn0search0turn0search17

### 3.2 Your server’s `Content-Type` mapping is incomplete vs your scanner extension list
Scanner supports many extensions:
- `crates/scanner/src/parser.rs` includes `m4v`, `m2ts`, `mts`, `wmv`, `asf`, etc.

But Direct Play’s MIME mapping in:
- `crates/server/src/streaming.rs::content_type_for_path()`

…only maps a smaller subset (notably it misses `m4v`, `m2ts`, `mts`, `wmv`, `asf`, `flv`, etc.).

For unknown extensions it returns `application/octet-stream`, which can cause browsers to refuse playback or behave inconsistently (especially with strict MIME handling).

### 3.3 HLS sessions depend on ffmpeg working (and errors need to surface)
The transcode session spawns ffmpeg:
- File: `crates/transcoder/src/session.rs::spawn_ffmpeg()`

If ffmpeg is missing / not executable / fails to read the input file, the API will return an error from `create_session`.

Your player code *does* display a UI error if the POST fails (because it catches and sets `error`), but only if `startHls()` actually runs—meaning `fileId` must be fixed first.

### 3.4 HLS MIME types: mostly correct, but tighten for compatibility
The HLS playlist content-type is set to `application/vnd.apple.mpegurl` in `crates/transcoder/src/hls.rs`, matching common recommendations. citeturn0search15turn0search5  
The segment content type is `video/MP2T` (uppercase). MIME types are case-insensitive in practice, but using the canonical lowercase `video/mp2t` is safer for picky clients and proxies. citeturn0search5  

---

## 4) The primary fix (make the Player able to stream *anything at all*)

### 4.1 Backend: expose the `file_id` for playable items
There are two good designs. I recommend **Option B** because it scales to multiple files, subtitles, and “decision logic” cleanly.

#### Option A (minimal): add `file_id` to `ItemResponse`
**Files to change**
- `crates/server/src/routes.rs`

**Implementation sketch**
1. Update `ItemResponse`:
   - Add `file_id: Option<String>`
2. In `get_item`, after fetching `item`, if `kind` is `movie` or `episode`:
   - call `rustfin_db::repo::items::get_item_file_id(&state.db, &id)`
   - include it in response

**Concrete change shape**
```rust
struct ItemResponse {
  ...
  file_id: Option<String>,
}

async fn get_item(...) -> Result<Json<ItemResponse>, AppError> {
  ...
  let file_id = if item.kind == "movie" || item.kind == "episode" {
    rustfin_db::repo::items::get_item_file_id(&state.db, &id).await?
  } else { None };

  Ok(Json(item_to_response(item, file_id)))
}
```

> If you add `file_id` to list endpoints too, avoid N+1 queries by doing a SQL LEFT JOIN from `item` to `episode_file_map` for `map_kind='primary'`.

#### Option B (recommended): add a dedicated playback endpoint
Add a new endpoint that returns the precise fields the player needs without bloating ItemResponse:

**New route**
- `GET /api/v1/items/{id}/playback`

**Response**
```json
{
  "item_id": "...",
  "file_id": "...",
  "direct_url": "/stream/file/<file_id>?token=<...>",
  "hls_start_url": "/api/v1/playback/sessions",
  "media_info_url": "/api/v1/playback/info/<file_id>"
}
```

**Benefits**
- Works even if items later map to multiple files
- Lets you evolve “Direct vs Transcode” decision logic cleanly
- Keeps general item browsing responses small and stable

**Files to change**
- `crates/server/src/routes.rs`:
  - add handler `get_item_playback()`
  - add route `.route("/items/{id}/playback", get(get_item_playback))`

**Required server logic**
- Load item
- Ensure library access
- Lookup `file_id` via `get_item_file_id`
- Return URLs

> For `direct_url`, you can either include the token query param server-side (by reading Authorization header and re-issuing a short-lived one-time token) or keep the current approach where the UI appends `?token=...`. The latter is simplest, but document the security tradeoffs.

### 4.2 Frontend: stop assuming `file_id` exists on `/items/{id}`
**File to change**
- `ui/src/app/player/[id]/page.tsx`

**Minimal update (Option A)**
- Keep `GET /items/{id}` but now it actually returns `file_id`
- Add a guard: if the response lacks `file_id`, show an error and disable buttons

**Recommended update (Option B)**
- Call `/items/{id}/playback` and use that response
- If file_id missing, show:
  - “This item has no playable media file mapping. Rescan or check ingest.”

**Also add UX protection**
Right now `startHls()` does nothing when `fileId` is null. Make it loud:

```ts
if (!fileId) {
  setError('No media file is attached to this item (missing file_id mapping).');
  return;
}
```

### 4.3 What “fixed” looks like (observable signals)
After implementing the above:

**Direct Play**
- Clicking “Direct Play” causes the `<video>` to request:
  - `GET /stream/file/<file_id>?token=...`
- The server returns:
  - `200 OK` or `206 Partial Content`
  - `Accept-Ranges: bytes`
  - valid `Content-Type` (e.g. `video/mp4`)
- The video begins playback

**HLS**
- Clicking “Transcode (HLS)” causes:
  1) `POST /api/v1/playback/sessions` with `{ file_id }`
  2) Response includes `hls_url="/stream/hls/<sid>/master.m3u8"`
  3) Browser requests:
     - `/stream/hls/<sid>/master.m3u8`
     - `/stream/hls/<sid>/seg_00000.ts`, etc.
- You see playback start after the first segments arrive

---

## 5) Format support strategy (how to support “a wide range” without lying to yourself)

You effectively have two playback products:

1) **Direct Play**: works only for what the browser can decode.
2) **HLS Transcode**: makes almost everything playable by converting to a web-friendly set.

### 5.1 Define a “Web Baseline” encode for HLS
The most compatible baseline for web is:
- Video: H.264 (AVC)  
- Audio: AAC-LC
- Container: HLS with TS segments (or fMP4 segments)

Your current ffmpeg command outputs:
- `-c:v libx264` (or HW equivalent)
- `-c:a aac`
- HLS `.ts` segments

That matches the hls.js model (TS + AAC) described in hls.js docs. citeturn0search8

### 5.2 Decide when to Direct Play vs Transcode
There are two robust approaches:

#### Approach 1 (pragmatic): attempt Direct Play; fall back on `video.onerror`
1. Set `video.src = direct_url`
2. Attach:
   - `video.addEventListener('error', ...)`
3. If error fires quickly (or `canPlayType` returns `""`), switch to HLS mode automatically.

This avoids building a huge codec string mapping up front, and it reflects the truth: even if the container is “supported”, profiles/levels can break playback.

#### Approach 2 (explicit): use ffprobe + canPlayType for a pre-check
You already have:
- `GET /api/v1/playback/info/{file_id}` → runs ffprobe

Use that server response to compute a MIME type and codec string and call:
- `video.canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"')` citeturn0search0turn0search17  

If it returns empty, start HLS transcode.

> Doing this properly requires mapping ffprobe codec fields (like H.264 profile/level) to RFC codec strings (`avc1.*`). That’s doable, but work. If you want a modern, “works everywhere” product sooner, Approach 1 is often better.

### 5.3 Expand Direct Play Content-Types (minimum bar)
Update `crates/server/src/streaming.rs::content_type_for_path()` so that your scanner’s recognized containers are served with appropriate `Content-Type`.

Suggested mapping (containers, not codecs):
- `mp4`, `m4v` → `video/mp4`
- `mov` → `video/quicktime`
- `mkv` → `video/x-matroska` (browser support limited; expect HLS fallback)
- `webm` → `video/webm`
- `avi` → `video/x-msvideo`
- `ts`, `m2ts`, `mts` → `video/mp2t`
- `mpg`, `mpeg`, `mpe`, `mpv` → `video/mpeg`
- `wmv`/`asf` → `video/x-ms-wmv` or `application/vnd.ms-asf` (direct play unlikely; transcode)
- `ogv` → `video/ogg`

Then rely on HLS for everything else.

### 5.4 Supported “input formats” vs “playable outputs”
It helps to be explicit in the UI:

- “Rustyfin can **ingest** many formats (containers).”
- “Rustyfin can **direct play** some of them depending on your browser.”
- “Rustyfin can **transcode** most formats to a web baseline (HLS).”

This avoids the common media-server trap of implying that “supported format” means “direct-play in all browsers”.

---

## 6) Where exactly the bugs live (file-by-file)

### Primary playback bug
**Frontend**
- `ui/src/app/player/[id]/page.tsx`
  - Assumes `/api/v1/items/{id}` returns `file_id`
  - If it doesn’t, both modes do nothing (silent early return / no src)

**Backend**
- `crates/server/src/routes.rs`
  - `ItemResponse` / `item_to_response` omit `file_id`
  - `GET /api/v1/items/{id}` cannot satisfy the player’s needs

### Playback-enabling data exists but is unused
**DB**
- `crates/db/src/repo/items.rs`
  - `get_item_file_id()` reads mapping from `episode_file_map`

**Scanner**
- `crates/scanner/src/scan.rs`
  - Writes mapping for movies and episodes into `episode_file_map`

### “Next failures” after fixing file_id
- `crates/server/src/streaming.rs::content_type_for_path()` is missing content-type mappings for several scanned extensions.
- `crates/transcoder/src/session.rs` requires ffmpeg availability; errors become visible only after player triggers the session.

---

## 7) Concrete fix plan (recommended sequence)

### Step 1 — Fix the API/Player contract (must-do)
Do **Option B** (dedicated playback endpoint) unless you are certain you want `file_id` on ItemResponse permanently.

**Backend**
1. Add route:
   - `GET /api/v1/items/{id}/playback`
2. Implement handler:
   - Ensure item exists
   - Ensure library access
   - Load file_id via `get_item_file_id`
   - Return { file_id, direct_url_base, hls_start_url, media_info_url }

**Frontend**
1. Player page calls `/items/{id}/playback`
2. If file_id missing:
   - show error message
   - disable playback buttons
3. If present:
   - enable Direct Play and HLS

### Step 2 — Make Direct Play succeed for your scanned container list
Update `content_type_for_path()` to cover:
- `m4v`, `m2ts`, `mts`, `ogv`, etc.
(As outlined in §5.3.)

### Step 3 — Make playback selection “smart” and user-friendly
Add:
- `video.onerror` fallback from Direct Play → HLS
- show meaningful errors:
  - 401 token invalid
  - 404 file missing
  - 500 ffmpeg failure (include hint: “is ffmpeg installed?”)

### Step 4 — Validate HLS correctness
Ensure the HLS playlist/segments are served with correct MIME types:
- `.m3u8`: `application/vnd.apple.mpegurl` citeturn0search15turn0search5  
- `.ts`: `video/mp2t` citeturn0search5  

Your playlist type is correct already; segment MIME casing should be normalized.

### Step 5 — Define and document the supported matrix
Ship a clear matrix in your README/UI:
- Input containers ingestable
- Direct-play likely on major browsers
- Transcode fallback for the rest

---

## 8) Verification (how to prove it’s fixed)

### 8.1 Network-level proof (Direct Play)
Open DevTools → Network
1. Click Direct Play
2. You should see requests to:
   - `/stream/file/<file_id>?token=...`
3. Responses should include:
   - `206 Partial Content` for Range requests
   - `Content-Range: bytes ...`
   - `Accept-Ranges: bytes`
This matches the standard Range header behavior. citeturn0search4turn0search1  

### 8.2 Network-level proof (HLS)
1. Click HLS Transcode
2. You should see:
   - `POST /api/v1/playback/sessions`
   - `GET /stream/hls/<sid>/master.m3u8`
   - segment GETs like `/stream/hls/<sid>/seg_00000.ts`
3. Playlist responses should have HLS MIME type. citeturn0search15turn0search5  

### 8.3 Server logs
- Backend should log stream requests and ffmpeg spawn lines (if tracing is enabled).
- For HLS, transcode dir should contain:
  - `master.m3u8`
  - `seg_00000.ts`, …

---

## 9) Summary (the “why it fails” in one sentence)

Playback fails because **the player page requires a media `file_id`** to build stream URLs, but **the backend item API never returns a `file_id`**, so the player never sets `video.src` and both playback buttons become no-ops.

Fix the contract (expose `file_id` via item response or dedicated playback endpoint), then tighten MIME mappings and add Direct→HLS fallback to support wide formats cleanly.
