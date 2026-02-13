# Rustfin Blueprint — building a “modern Jellyfin, but Rust-first”

> This document is a practical, code-heavy blueprint for building a **local-first**, Jellyfin-class media server in **Rust**, while deliberately keeping the stack small (≈2 languages, 3–4 “projects” total).
>
> **Design goal:** don’t build a fragile quilt of 12 micro-services and 6 languages. Build a *single* Rust server binary, a *single* UI app, and rely on *one* media engine (FFmpeg) for the truly gnarly codec work.

---

## 0) Big picture

### What Jellyfin does (the parts that matter)
Jellyfin’s streaming system makes a per-request decision:

1. **Direct Play**: send the original file as-is (best).
2. **Direct Stream / Remux**: change container without re-encoding (cheap).
3. **Transcode**: re-encode audio/video (expensive; may use GPU).

That decision is driven by:
- **Client capabilities** (“device profile” / codec support tables)
- **Media characteristics** (container, codecs, bitrate, resolution, subtitle formats)
- **User/server policy** (max bitrate, allow transcoding, subtitle burn-in rules, etc.)

Jellyfin implements this logic in its server and uses **FFmpeg** for conversion; Jellyfin also maintains a tuned FFmpeg fork (“jellyfin-ffmpeg”) for better media pipelines and fixes.

---

## 1) Constraints and principles (your rules)

### Local-first
- Runs on your own machine/LAN.
- “Production-grade” isn’t required, but correctness and reliability are.

### Minimize moving parts
Target: **3–4 projects max**
1) `rustfin-server` (Rust) — the only “backend”  
2) `rustfin-ui` (either Rust-WASM OR Next.js) — the only “frontend”  
3) `ffmpeg` + `ffprobe` (binary) — the only “media engine”  
4) Optional: packaging wrapper (Tauri) **only if you want a desktop app**

### Don’t “invent a codec stack”
Implementing a full transcoder is a multi-year effort. Use FFmpeg as the heavy lifter (like Jellyfin does).

---

## 2) Recommended stack (two viable options)

### Option A (recommended for *minimum languages*): Rust full-stack UI
- **Backend:** Rust + Tokio + Axum
- **Frontend:** **Leptos** (Rust → WASM) with SSR/hydration for a “modern app” feel
- **DB:** SQLite (via SQLx)
- **Media:** FFmpeg/ffprobe
- **Packaging (optional):** Tauri (still Rust + WebView)

**Why this option?**
- You can keep the project mostly **one language (Rust)**.
- You still get a modern reactive UI and can do SSR/hydration.
- Single toolchain, fewer dependency ecosystems to babysit.

### Option B (if you want the biggest web ecosystem): Rust backend + Next.js UI
- **Backend:** Rust + Tokio + Axum
- **Frontend:** Next.js (TypeScript/React)
- **DB:** SQLite (via SQLx)
- **Media:** FFmpeg/ffprobe
- **Packaging:** optional

**Why this option?**
- Next.js gives you a mature, batteries-included UI stack with routing, SSR, data fetching patterns, etc.
- Tons of ready-made UI components and auth patterns.
- Tradeoff: you’ve now committed to maintaining a TypeScript front-end.

---

## 3) Component choices and “what problem they prevent”

### 3.1 Axum (HTTP/API)
Axum is built on Tokio + Hyper + Tower, and uses Tower middleware instead of inventing its own middleware system. That matters because:
- You get robust, composable middleware for **timeouts**, **tracing**, **compression**, **auth layers**, **rate limiting**, etc.
- It’s very “Rust standard stack” in 2026.

**Problem prevented:** ad-hoc middleware spaghetti and inconsistent cross-cutting concerns.

### 3.2 SQLx + SQLite (local database)
SQLx provides async SQL with **compile-time checked queries** (when enabled) and supports SQLite.

**Problem prevented:** “it compiles but the query is wrong” and runtime-only SQL failures.

SQLite is perfect for local-first:
- zero admin
- single file
- easy backups
- good performance for this use-case

If you later want multi-machine, swap to Postgres with minimal changes.

### 3.3 FFmpeg / ffprobe (media analysis + transcoding)
FFprobe gives you canonical media stream info; FFmpeg provides direct-stream remuxing and transcoding, including hardware acceleration.

**Problem prevented:** reinventing demuxers/decoders/encoders/subtitle renderers.

### 3.4 Hardware acceleration strategy (GPU)
Jellyfin’s validated HWA methods include:
- Intel QSV, NVIDIA NVENC/NVDEC, AMD AMF, VA-API, VideoToolbox, etc.
Your Rustfin should support the same set by selecting FFmpeg arguments based on capability probing.

**Problem prevented:** “works on my machine” GPU assumptions; broken transcodes on different vendors.

### 3.5 UI choice: Leptos vs Next.js
- **Leptos** prevents “two worlds” complexity: shared types in Rust, fewer serialization edge cases, consistent domain model.
- **Next.js** prevents “homegrown UI framework pain” and gives a huge ecosystem, but costs you a second language and toolchain.

---

## 4) Architecture: one process, a few subsystems

```
┌──────────────────────────────┐
│          rustfin-server       │  (single binary)
│  ┌───────────────┐           │
│  │ HTTP API       │  Axum     │
│  ├───────────────┤           │
│  │ Auth/Users     │  tokens  │
│  ├───────────────┤           │
│  │ Library Index  │  SQLite  │
│  ├───────────────┤           │
│  │ Streaming      │  Range/HLS│
│  ├───────────────┤           │
│  │ Transcoder     │  FFmpeg  │
│  ├───────────────┤           │
│  │ Tasks/Queue    │  Tokio   │
│  └───────────────┘           │
└───────────────┬──────────────┘
                │ HTTP
         ┌──────▼───────┐
         │ rustfin-ui    │ (Leptos or Next.js)
         └──────────────┘
```

**Key idea:** do not prematurely split into microservices. Use internal modules with clean boundaries.

---

## 5) A pragmatic feature roadmap (build working slices)

### Phase 1: “Playable”
- Scan folders → store media items in SQLite
- Web UI lists items
- Direct file streaming with HTTP Range requests (seek works)

### Phase 2: “Transcode”
- ffprobe analysis, stream decision engine (direct/remux/transcode)
- HLS VOD transcoding (single bitrate)
- Subtitle handling (SRT/VTT direct; burn-in when needed)

### Phase 3: “Jellyfin-ish”
- Users + permissions (per-library access)
- Sessions (“now playing”)
- Profile-driven capabilities (web, iOS, Android TV, etc.)
- Multiple quality profiles

### Phase 4: “Nice-to-have”
- DLNA/UPnP (SSDP discovery), Chromecast, etc.
- Plugins (WASM sandbox if you go there)
- Sync-play

---

## 6) Repo layout (keep it boring)

```
rustfin/
  Cargo.toml                 # workspace
  crates/
    rustfin-core/            # domain types + decisions
    rustfin-db/              # SQLx models/migrations
    rustfin-media/           # ffprobe + ffmpeg runners
    rustfin-server/          # axum app + routes
    rustfin-ui/              # leptos app (or separate nextjs/)
  assets/
    device-profiles/         # JSON profiles, optional
    migrations/
```

If you pick Next.js, make it:
```
web/
  package.json
  app/
  ...
```
…and keep the API types generated from OpenAPI.

---

## 7) Core data model (SQLite)

### 7.1 Minimal schema
```sql
-- users
CREATE TABLE users (
  id            TEXT PRIMARY KEY,
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  created_at    INTEGER NOT NULL
);

-- libraries
CREATE TABLE libraries (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  path       TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

-- user ↔ library permissions
CREATE TABLE user_library_access (
  user_id    TEXT NOT NULL,
  library_id TEXT NOT NULL,
  can_read   INTEGER NOT NULL DEFAULT 1,
  can_write  INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, library_id),
  FOREIGN KEY(user_id) REFERENCES users(id),
  FOREIGN KEY(library_id) REFERENCES libraries(id)
);

-- media items
CREATE TABLE media_items (
  id          TEXT PRIMARY KEY,
  library_id  TEXT NOT NULL,
  path        TEXT NOT NULL UNIQUE,
  kind        TEXT NOT NULL,       -- movie, episode, music, etc.
  title       TEXT,
  duration_ms INTEGER,
  size_bytes  INTEGER NOT NULL,
  hash        TEXT,                -- optional content fingerprint
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  FOREIGN KEY(library_id) REFERENCES libraries(id)
);

-- ffprobe JSON cached (avoid re-probing constantly)
CREATE TABLE media_probe (
  media_id    TEXT PRIMARY KEY,
  ffprobe_json TEXT NOT NULL,
  updated_at  INTEGER NOT NULL,
  FOREIGN KEY(media_id) REFERENCES media_items(id)
);

-- playback sessions
CREATE TABLE sessions (
  id           TEXT PRIMARY KEY,
  user_id      TEXT NOT NULL,
  device_name  TEXT,
  client_kind  TEXT,               -- web, ios, androidtv, dlna...
  last_seen_at INTEGER NOT NULL,
  FOREIGN KEY(user_id) REFERENCES users(id)
);
```

### 7.2 SQLx models (Rust)
```rust
// crates/rustfin-db/src/models.rs
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct MediaItemRow {
    pub id: String,
    pub library_id: String,
    pub path: String,
    pub kind: String,
    pub title: Option<String>,
    pub duration_ms: Option<i64>,
    pub size_bytes: i64,
    pub hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
```

---

## 8) Auth: keep it simple and robust

### 8.1 Token approach
For local-only:
- Use **opaque bearer tokens** stored in SQLite (or in-memory with restart invalidation).
- Hash passwords with **Argon2id**.

**Problem prevented:** JWT complexity + key rotation + footguns, when you don’t need it.

### 8.2 Example: login + token issuance
```rust
// crates/rustfin-server/src/routes/auth.rs
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResp {
    pub token: String,
    pub user_id: String,
}

pub async fn login(
    State(app): State<AppState>,
    Json(req): Json<LoginReq>,
) -> Result<Json<LoginResp>, ApiError> {
    let user = app.db.users().find_by_username(&req.username).await?;
    app.auth.verify_password(&req.password, &user.password_hash)?;

    let token = app.auth.issue_token(&user.id).await?;
    Ok(Json(LoginResp { token, user_id: user.id }))
}
```

### 8.3 Middleware: require auth
```rust
// crates/rustfin-server/src/mw/auth.rs
use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
};

pub struct AuthedUser {
    pub user_id: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthedUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        let Some(authz) = parts.headers.get(axum::http::header::AUTHORIZATION) else {
            return Err((StatusCode::UNAUTHORIZED, "missing Authorization header"));
        };
        let authz = authz.to_str().map_err(|_| (StatusCode::UNAUTHORIZED, "bad header"))?;
        let token = authz.strip_prefix("Bearer ").ok_or((StatusCode::UNAUTHORIZED, "bad scheme"))?;

        let user_id = app.auth.verify_token(token).await
            .map_err(|_| (StatusCode::UNAUTHORIZED, "invalid token"))?;

        Ok(AuthedUser { user_id })
    }
}
```

---

## 9) Library scanning (index your media)

### 9.1 Scanning strategy
- On startup: scan configured library paths
- Use a filesystem watcher to catch changes (optional)
- For each file:
  - derive `kind` (movie/episode/music) using naming heuristics
  - store path + size + mtime
  - schedule ffprobe (background) for duration/streams

### 9.2 Spawn a background probe
```rust
// crates/rustfin-media/src/probe.rs
use std::path::Path;
use tokio::process::Command;

pub async fn ffprobe_json(ffprobe: &Path, media_path: &Path) -> anyhow::Result<String> {
    let out = Command::new(ffprobe)
        .args([
            "-v", "error",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(media_path)
        .output()
        .await?;

    if !out.status.success() {
        anyhow::bail!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?)
}
```

**Problem prevented:** parsing dozens of container formats yourself.

---

## 10) Streaming fundamentals: HTTP Range (Direct Play)

### 10.1 Why Range matters
Most clients seek by issuing:
- `Range: bytes=...-...`

If you don’t support Range:
- seeking breaks
- scrubbing breaks
- some players fail entirely

### 10.2 Axum handler for ranged files
```rust
// crates/rustfin-server/src/routes/stream.rs
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    body::Body,
};
use tokio::fs::File;
use tokio_util::io::ReaderStream;

pub async fn direct_stream(
    State(app): State<AppState>,
    _user: AuthedUser,
    Path(media_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let item = app.db.media().get(&media_id).await?;
    let path = std::path::PathBuf::from(&item.path);
    let meta = tokio::fs::metadata(&path).await?;
    let len = meta.len();

    // TODO: parse Range header properly; this is intentionally abbreviated.
    // For production-grade seeking, implement RFC 7233 byte ranges.

    let file = File::open(path).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut resp = Response::new(body);
    *resp.status_mut() = StatusCode::OK;
    resp.headers_mut().insert("Accept-Ranges", "bytes".parse().unwrap());
    resp.headers_mut().insert("Content-Length", len.to_string().parse().unwrap());
    Ok(resp)
}
```

(You’ll implement full Range parsing + partial responses; this is the skeleton.)

---

## 11) The “Jellyfin core”: stream decision engine

### 11.1 Represent the decision
```rust
#[derive(Debug, Clone)]
pub enum Delivery {
    DirectPlay { mime: String },
    Remux { container: String, mime: String },
    Transcode(TranscodePlan),
}

#[derive(Debug, Clone)]
pub struct TranscodePlan {
    pub protocol: StreamProtocol, // HLS, DASH, progressive
    pub video: VideoPlan,
    pub audio: AudioPlan,
    pub subtitles: SubtitlePlan,
    pub accel: Option<HardwareAccel>,
    pub target: QualityTarget,
}

#[derive(Debug, Clone)]
pub enum StreamProtocol { Hls, Dash, Progressive }

#[derive(Debug, Clone)]
pub struct QualityTarget {
    pub max_width: u32,
    pub max_height: u32,
    pub max_video_bitrate_kbps: u32,
    pub max_audio_bitrate_kbps: u32,
}
```

### 11.2 Inputs to the decision
```rust
pub struct ClientCaps {
    pub containers: Vec<String>,    // mp4, mkv, webm
    pub video_codecs: Vec<String>,  // h264, hevc, av1...
    pub audio_codecs: Vec<String>,  // aac, opus, ac3...
    pub subtitle_formats: Vec<String>, // srt, vtt, pgs...
    pub max_level: Option<String>,
    pub max_bitrate_kbps: Option<u32>,
}

pub struct MediaInfo {
    pub container: String,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub subtitles: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bitrate_kbps: Option<u32>,
}
```

### 11.3 Decision algorithm (high-level)
```rust
pub fn decide_stream(media: &MediaInfo, client: &ClientCaps, policy: &Policy) -> Delivery {
    // 1) If everything is compatible, Direct Play.
    if is_direct_play_ok(media, client, policy) {
        return Delivery::DirectPlay { mime: guess_mime(&media.container) };
    }

    // 2) If only the container/subtitle can be changed, Remux.
    if is_remux_ok(media, client, policy) {
        return Delivery::Remux { container: "mp4".into(), mime: "video/mp4".into() };
    }

    // 3) Otherwise, Transcode.
    let accel = pick_hardware_accel(policy);
    let target = pick_quality(policy, client);
    Delivery::Transcode(TranscodePlan {
        protocol: StreamProtocol::Hls,
        video: VideoPlan::h264(accel.clone(), target.clone()),
        audio: AudioPlan::aac(target.clone()),
        subtitles: SubtitlePlan::Auto,
        accel,
        target,
    })
}
```

**You’ll build out `is_direct_play_ok` using real codec tables / profiles.**

---

## 12) HLS transcoding (the “good enough” streaming protocol)

HLS is well-supported across browsers and devices and is explicitly designed to adapt to network conditions. A practical server can start with **single-variant VOD HLS** and expand to ABR ladders later.

### 12.1 Transcode session lifecycle
1) Client requests `master.m3u8`
2) Server creates (or reuses) a `TranscodeSession`
3) Spawn FFmpeg to write segments to a cache dir:
   - `seg_000.ts`, `seg_001.ts`, ...
4) Serve playlist + segments over HTTP
5) Kill FFmpeg when idle; cleanup cache

### 12.2 Cache layout
```
cache/transcodes/{session_id}/
  master.m3u8
  media_000.ts
  media_001.ts
  ...
  ffmpeg.log
```

### 12.3 FFmpeg command builder (hardware-aware)
```rust
pub struct FfmpegArgs {
    pub input: String,
    pub video_args: Vec<String>,
    pub audio_args: Vec<String>,
    pub hls_args: Vec<String>,
}

pub fn build_hls_transcode(input: &str, out_dir: &str, accel: Option<HardwareAccel>, target: &QualityTarget) -> FfmpegArgs {
    let mut video = vec![];

    // decode/encode selection
    match accel {
        Some(HardwareAccel::NvidiaNvenc) => {
            // decode might still be software; advanced: use -hwaccel cuda etc.
            video.extend([
                "-c:v".into(), "h264_nvenc".into(),
                "-b:v".into(), format!("{}k", target.max_video_bitrate_kbps),
            ]);
        }
        Some(HardwareAccel::Vaapi { device }) => {
            video.extend([
                "-vaapi_device".into(), device,
                "-vf".into(), format!("scale_vaapi=w={}:h={}", target.max_width, target.max_height),
                "-c:v".into(), "h264_vaapi".into(),
                "-b:v".into(), format!("{}k", target.max_video_bitrate_kbps),
            ]);
        }
        _ => {
            // software fallback
            video.extend([
                "-c:v".into(), "libx264".into(),
                "-preset".into(), "veryfast".into(),
                "-b:v".into(), format!("{}k", target.max_video_bitrate_kbps),
            ]);
        }
    }

    let audio = vec![
        "-c:a".into(), "aac".into(),
        "-b:a".into(), format!("{}k", target.max_audio_bitrate_kbps),
    ];

    let hls = vec![
        "-f".into(), "hls".into(),
        "-hls_time".into(), "4".into(),
        "-hls_playlist_type".into(), "vod".into(),
        "-hls_segment_filename".into(), format!("{out_dir}/media_%03d.ts"),
        format!("{out_dir}/master.m3u8"),
    ];

    FfmpegArgs {
        input: input.into(),
        video_args: video,
        audio_args: audio,
        hls_args: hls,
    }
}
```

### 12.4 Spawning FFmpeg
```rust
use tokio::process::Command;
use std::path::Path;

pub async fn spawn_ffmpeg(ffmpeg: &Path, args: &FfmpegArgs) -> anyhow::Result<tokio::process::Child> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-y"]);
    cmd.arg("-i").arg(&args.input);
    cmd.args(args.video_args.iter());
    cmd.args(args.audio_args.iter());
    cmd.args(args.hls_args.iter());
    cmd.stderr(std::process::Stdio::piped());

    Ok(cmd.spawn()?)
}
```

---

## 13) Playback speed (client + server)

### 13.1 Direct play
Playback speed is typically handled entirely by the client:
- HTML5 video exposes `playbackRate`.
- Native apps do similar.

### 13.2 Transcode path
If you *must* support speed changes server-side (e.g., for clients that can’t):
- Video: adjust timestamps (`setpts=PTS/1.25`)
- Audio: use `atempo=1.25` (note: atempo limits require chaining for large factors)

Example snippet:
```bash
-vf "setpts=PTS/1.25" -af "atempo=1.25"
```

In Rust you’d build a filter graph when speed != 1.0.

---

## 14) Subtitles (the underestimated source of pain)

Plan for:
- Sidecar `.srt` and `.vtt` (easy)
- Embedded subs in MKV/MP4 (ffprobe tells you)
- Image-based subs (PGS) that often require **burn-in** for web clients

Rule of thumb:
- If the client supports the subtitle format → deliver as separate track.
- Otherwise → burn into video during transcode.

---

## 15) Device support without a zoo of clients

### 15.1 Start with “Web is the universal client”
- A good web UI + HLS covers most devices:
  - desktop browsers
  - mobile browsers
  - many TVs via built-in browser (varies)

### 15.2 Then add “protocol bridges” when needed
- **DLNA/UPnP** helps old TVs without a native app
- Chromecast/AirPlay can be supported later

DLNA requires SSDP discovery on UDP 1900 and UPnP concepts.

---

## 16) DLNA (optional, but doable)
If you implement DLNA:
- SSDP discovery responder (UDP multicast)
- Device profile matching
- Serve DIDL-Lite metadata + SOAP control endpoints
- Use the same stream decision engine underneath

**Important:** DLNA is a whole mini-universe. Put it behind a feature flag.

---

## 17) Reliability patterns (the boring parts that save you)

### 17.1 Transcode process supervision
- Track each FFmpeg child by session id
- Heartbeat: last segment requested time
- Kill after idle timeout (e.g., 60–120s)
- Always clean up temp dirs

### 17.2 Backpressure and limits
- Max concurrent transcodes
- Per-user limits
- Segment cache size limit

### 17.3 Tracing and structured logs
Use `tracing` + `tracing-subscriber`.
Store per-transcode logs to `ffmpeg.log`.

---

## 18) OpenAPI-first (API you can grow)
Jellyfin publishes an OpenAPI spec; doing the same makes it trivial to:
- generate TypeScript clients (Next.js option)
- generate Rust clients for tests/tools
- keep server + UI contract stable

---

## 19) Minimal Cargo.toml (server)
```toml
[package]
name = "rustfin-server"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "fs"] }
tower-http = { version = "0.6", features = ["trace", "compression-gzip", "cors"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros"] }
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4"] }
```

---

## 20) Where you “match Jellyfin” vs “choose sanity”

### Match Jellyfin (high value)
- Direct play/remux/transcode decision
- HLS VOD streaming
- Hardware acceleration
- Users/permissions
- Metadata providers (TMDb etc.) + local NFO scanning

### Choose sanity (initially)
- ABR ladders (start single-bitrate HLS)
- Full device-profile zoo (start web profile + a couple common ones)
- DLNA/Chromecast/AirPlay (later)
- Live TV/DVR (later)

---

## 21) Legal / licensing note (important)
Jellyfin is GPL-licensed. If you copy Jellyfin code into your project, your project inherits GPL obligations.

If your goal is “a Rust Jellyfin-like system,” the clean approach is:
- Treat Jellyfin as behavioral reference (how it behaves, what endpoints do)
- Don’t copy code; re-implement from scratch (clean-room)
- Use standard protocols/specs (HLS/DASH/HTTP Range) and your own structure

---

## 22) “Next action” checklist (practical)
1) Create workspace + crates
2) Implement SQLite schema + migrations
3) Implement library scanning + media_items table population
4) Implement **direct streaming with Range**
5) Implement `ffprobe` caching
6) Implement decision engine (direct/remux/transcode)
7) Implement HLS transcode sessions + segment serving
8) Implement user auth + per-library permissions
9) Build UI: browse library + play item

---

## References / pointers (for deeper reading)
- Jellyfin docs: codec support, direct play/stream/transcode model, metadata providers, DLNA, transcoding, hardware acceleration
- Apple HLS docs / RFC 8216 pointers (authoring + validation tools)
- FFmpeg hardware acceleration docs (VAAPI/NVENC/etc.)
- Axum / SQLx / Leptos / Tauri docs for the Rust-side stack

(Keep these as clickable links in your local copy; update them as you go.)
