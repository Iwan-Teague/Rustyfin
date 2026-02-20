# Architecture Overview (Extreme Expansion)

## 0. Non-goals
- No multi-region, multi-tenant production hosting.
- No DRM pipeline.
- No cloud storage required.

Rustfin is **local-first** but should still behave professionally: predictable, testable, recoverable.

---

## 1. Jellyfin-class surface area (what “same functionality” implies)
A Jellyfin-class server generally includes:
- Users and auth (multi-user watch state, admin vs standard)
- Libraries (paths, scan schedules)
- Media modeling (Movies, Series, Seasons, Episodes, Music)
- Metadata + IDs + cast/genres/ratings
- Artwork types (poster/backdrop/logo/thumb) + resizing/cache
- Streaming modes (Direct Play, Remux, Transcode)
- Device profiles and session management
- Subtitles (sidecar, embedded, burn-in, optional provider download)
- Extras/trailers
- Playback controls (speed, quality, track selection)
- Watch state & progress sync
- Admin dashboard (logs, tasks, transcode settings)
- Packaging (Docker, persistent config, GPU support)

---

## 2. Modular monolith (bounded contexts)
1) Identity & Auth  
2) Library & Catalog  
3) Media Intelligence (parsing/identification/providers/merge)  
4) Artwork & Assets  
5) Playback (sessions + decision engine)  
6) Streaming (Range + HLS)  
7) Transcode Orchestrator (FFmpeg lifecycle)  
8) Jobs (queue + workers)  
9) Events (SSE/WebSocket)  
10) Observability (logs/metrics)

Why: stops you from mixing provider calls + ffmpeg + DB writes inside request handlers.

---

## 3. Runtime topology
- One process: `rustfin-server`
- Child processes: `ffprobe` / `ffmpeg` as needed

Storage:
- SQLite DB: `/config/rustfin.db`
- Cache: `/cache`
- Transcode scratch: `/transcode/<sid>/`
- Media mounts: `/media` (read-only preferred)

---

## 4. Conceptual data model
```
Library
  ├─ Movie
  │    ├─ MediaFile versions
  │    └─ Images
  └─ Series
       ├─ Season
       │    └─ Episode → MediaFile(s) + Subtitles
       └─ Images
```

Missing episodes are enabled by storing an **expected episode list** per series.

---

## 5. Core flows

### 5.1 Scan flow
1) Enumerate files
2) Record stats (size/mtime/hash)
3) Parse naming → candidate identity
4) Create/update Items + map files
5) Enqueue identify/metadata jobs
6) Emit progress events

### 5.2 Metadata flow
1) Determine canonical provider (series-level lock)
2) Fetch provider records
3) Merge with precedence (user/local > provider)
4) Fetch/cache artwork
5) Update expected episodes
6) Emit events

### 5.3 Playback flow
1) Create playback session
2) Evaluate device profile
3) Decide direct/remux/transcode
4) If transcode: spawn ffmpeg session
5) Serve Range or HLS URLs
6) Track progress

---

## 6. Concurrency model
- bounded pools for: provider HTTP, ffprobe/ffmpeg, image resize
- cancellation tokens for: jobs + transcodes
- backpressure: limit simultaneous transcodes
