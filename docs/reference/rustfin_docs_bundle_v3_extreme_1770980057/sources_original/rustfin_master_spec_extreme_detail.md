# Rustfin Master Spec (Extreme Detail)
*A “Jellyfin-class” media server, but Rust-first, local-first, and not stitched together from 17 languages.*

> This document has two jobs:
> 1) **Explain (in practical terms) how Jellyfin handles libraries, metadata/artwork, and subtitles**—including the *real* “gotchas” that make media servers annoying.
> 2) **Turn that into a concrete build plan** for a Rust implementation (“Rustfin”), with a *small* stack and lots of code skeletons you can actually start from.

---

## Table of contents

- [0. Scope, principles, and the “small stack” rule](#0-scope-principles-and-the-small-stack-rule)
- [1. Jellyfin feature parity checklist (what you’re copying)](#1-jellyfin-feature-parity-checklist-what-youre-copying)
- [2. Jellyfin’s metadata & artwork pipeline (how it really gets posters/backdrops/etc)](#2-jellyfins-metadata--artwork-pipeline-how-it-really-gets-postersbackdropsetc)
- [3. Media identification: naming, provider IDs, and why your files must be boring](#3-media-identification-naming-provider-ids-and-why-your-files-must-be-boring)
- [4. Local metadata: NFO, sidecar images, and “prefer local” rules](#4-local-metadata-nfo-sidecar-images-and-prefer-local-rules)
- [5. Online metadata sources: what to use, what to avoid, and why](#5-online-metadata-sources-what-to-use-what-to-avoid-and-why)
- [6. Subtitles: sidecar rules, embedded tracks, extraction, and downloading](#6-subtitles-sidecar-rules-embedded-tracks-extraction-and-downloading)
- [7. Theme songs, backdrop videos, and other “bells & whistles”](#7-theme-songs-backdrop-videos-and-other-bells--whistles)
- [8. Chapter images, trickplay, and intro/segment metadata](#8-chapter-images-trickplay-and-introsegment-metadata)
- [9. Users, personalization, and per-library permissions](#9-users-personalization-and-per-library-permissions)
- [10. Streaming & transcoding (short version) and where GPU comes in](#10-streaming--transcoding-short-version-and-where-gpu-comes-in)
- [11. Rustfin architecture: modules, traits, data model, and background jobs](#11-rustfin-architecture-modules-traits-data-model-and-background-jobs)
- [12. Code-heavy skeletons (metadata, artwork, subtitles, and scanning)](#12-code-heavy-skeletons-metadata-artwork-subtitles-and-scanning)
- [13. Roadmap: from “first scan works” to Jellyfin-class polish](#13-roadmap-from-first-scan-works-to-jellyfin-class-polish)
- [Appendix A: sane defaults (config + caching + rate limits)](#appendix-a-sane-defaults-config--caching--rate-limits)
- [Appendix B: provider capability matrix (movies/shows/music/books)](#appendix-b-provider-capability-matrix-moviesshowsmusicbooks)

---


## Primary sources used (Jellyfin side)
These are the “ground truth” references for how Jellyfin behaves/configures things (useful for verifying parity):

- Jellyfin docs — Libraries: https://jellyfin.org/docs/general/server/libraries/
- Jellyfin docs — Movies naming/structure: https://jellyfin.org/docs/general/server/media/movies/
- Jellyfin docs — TV Shows naming/structure + provider IDs: https://jellyfin.org/docs/general/server/media/shows/
- Jellyfin docs — Metadata providers overview: https://jellyfin.org/docs/general/server/media/metadata/
- Jellyfin docs — Provider identifiers: https://jellyfin.org/docs/general/server/media/identifiers/
- Jellyfin docs — NFO metadata: https://jellyfin.org/docs/general/server/media/nfo/
- Jellyfin docs — Chapter images: https://jellyfin.org/docs/general/server/media/chapter-images/
- Jellyfin docs — Plugins index (subtitle providers, etc.): https://jellyfin.org/docs/general/server/plugins/index.html
- Jellyfin docs — OpenSubtitles plugin: https://jellyfin.org/docs/general/server/plugins/open-subtitles/
- Jellyfin docs — Storage guidance (DB local, SSD recommended): https://jellyfin.org/docs/general/administration/storage/
- Jellyfin API (OpenAPI, stable): https://api.jellyfin.org/openapi/jellyfin-openapi-stable.json

---

## 0. Scope, principles, and the “small stack” rule

### The constraints you set (and how we honor them)
- **Local-only**: you’re not running a SaaS; but reliability still matters (so: structured logs, crash-safe DB writes, good defaults).
- **Feature parity**: users, libraries, metadata (incl. artwork), subtitles, streaming quality controls, playback speed, device support, GPU transcoding.
- **Don’t build a language lasagna**: aim for *one* main language (Rust) plus at most one “UI language” (either *also* Rust via WASM, or TypeScript via Next.js).

### Recommended “small stack” (2 languages, 4 projects max)
Pick one UI path:

**Option A (minimum languages):**
1) **Rust backend**: `axum` + `tokio` + `sqlx` (SQLite)  
2) **Rust web UI**: `leptos` (SSR + WASM)  
3) **Media engine**: FFmpeg (binary or libav)  
4) **Optional**: a small “provider SDK” crate for plugins

**Option B (best UI ecosystem):**
1) Rust backend (`axum`)  
2) **Next.js UI** (TypeScript/React)  
3) FFmpeg  
4) Optional provider SDK crate

Why pick A vs B?
- A avoids context-switching and keeps your “project brain” in Rust.
- B gets you a mature UI ecosystem (auth flows, PWA polish, mobile UX, accessibility, video player plumbing) faster.

For “mobile-focused” polish, B has an edge. For “I want to live in Rust”, A wins.

---

## 1. Jellyfin feature parity checklist (what you’re copying)

Jellyfin is a full system; don’t copy “video streaming” and forget the *ecosystem glue*. A Jellyfin-class experience includes:

### Libraries & organization
- Multiple libraries (Movies, Shows, Music, Books, Photos, Mixed)
- Multiple paths per library
- Periodic scans + manual scan triggers
- Per-library metadata settings (prefer local vs online, provider priority, refresh rules)

### Metadata & artwork
- Identification from filenames and/or provider IDs
- Auto-fetch of:
  - titles, plots, genres, studios, actors/roles
  - season/episode names and ordering
  - ratings (community + critic)
  - posters, backdrops, logos, banners, “clearart”
- Import/export local metadata (NFO), store artwork locally (optional)
- Manual “Identify” and “Refresh metadata” UI flows

### Users & personalization
- Multiple users with policies (admin vs restricted)
- Per-user playback preferences:
  - default audio language
  - subtitle mode and preferred subtitle language
  - playback speed
  - quality / max bitrate constraints
  - per-library access & parental controls
- Watch history, resume points, playstate sync

### Subtitles
- Sidecar subtitle detection (.srt/.ass/.vtt etc)
- Embedded subtitle extraction (optional)
- Remote subtitle download (OpenSubtitles and more)
- Subtitle delivery policies (embed vs burn-in vs HLS sidecar)

### Streaming engine
- Direct Play, Remux, Transcode
- HLS segmented transcode
- Device profiles / codec capability negotiation
- Hardware acceleration (NVENC/VAAPI/QSV/VideoToolbox/etc)

### “Bells & whistles”
- Chapter images + trickplay thumbnails
- Theme songs / backdrop videos (plugin/sidecar driven)
- Intro/segment metadata (skip intro/outro, etc) via plugins

---

## 2. Jellyfin’s metadata & artwork pipeline (how it really gets posters/backdrops/etc)

Think of metadata in Jellyfin as a **pipeline** that runs after the library scanner identifies “items”.

### Pipeline stages (conceptual)
1) **Discovery**
   - Walk library folders, find media files, group them into “items” (movie folder, show + seasons, etc.)
2) **Identification**
   - Decide *what* this item is (which movie/show/episode)
   - This can be:
     - confident (provider ID supplied)
     - fuzzy (title+year heuristic)
3) **Metadata fetch**
   - Pull structured data from providers (TMDb, TVDb, OMDb, MusicBrainz, etc.)
4) **Artwork fetch**
   - Pull posters/backdrops/logos/banners, then cache + resize
5) **Persistence**
   - Store normalized metadata in DB
   - Store images in cache and/or alongside media folder (optional)
6) **Client view model**
   - Serve metadata via API to clients
   - Clients request image URLs with size params

### The pragmatic truth
Media servers succeed or die on:
- **naming rules** (identification accuracy),
- **provider IDs** (determinism),
- **caching** (don’t hammer APIs),
- **merging** (local overrides vs remote data),
- **background jobs** (scans and refreshes must be cancelable and incremental).

---

## 3. Media identification: naming, provider IDs, and why your files must be boring

Jellyfin strongly prefers predictable folder/filename structure. The reason is simple: **matching media to the correct metadata entry is the hard problem**.

### Provider IDs in names: the “make it deterministic” move

Jellyfin supports putting provider identifiers in folder or file names (multiple IDs allowed), like:

- `Some Show (2010) [imdbid-tt0106145]`
- `Some Show (2018) [tmdbid-65567]`

This is a big deal because it bypasses fuzzy matching and reduces “wrong posters” syndrome.

### Multiple versions and “prefix must match exactly”
Jellyfin supports multiple versions of the same movie within one folder, but requires each filename to start with *exactly* the folder name (including year and IDs), before adding version labels. If the prefix differs, Jellyfin treats it as a different movie.

Rustfin should copy this rule (or improve it) because it prevents ambiguous grouping.

---

## 4. Local metadata: NFO, sidecar images, and “prefer local” rules

Local metadata matters because:
- it’s deterministic,
- it’s offline-friendly,
- it lets users override “the internet’s opinion”.

Jellyfin supports local metadata formats like **NFO** (Kodi-style), plus sidecar artwork (e.g., `poster.jpg`, `backdrop.jpg`, etc.). Many library types also support embedded metadata (music tags, EPUB metadata, etc.).

### Rustfin “local-first” rules (recommended)
- **Order of precedence**:
  1) user overrides (manual edits stored in DB)
  2) local metadata files (NFO, embedded tags)
  3) remote provider data (TMDb/TVDb/etc)
- Preserve:
  - provider IDs discovered locally
  - local images as “locked” unless user requests overwrite

---

## 5. Online metadata sources: what to use, what to avoid, and why

### The big warning: don’t scrape IMDb
Scraping tends to violate ToS, breaks frequently, and gets IPs blocked. Jellyfin’s ecosystem typically avoids direct scraping and instead relies on providers with proper APIs.

**Recommended approach for Rustfin:**
- Use **TMDb** as primary for movies and shows (rich, consistent, good images)
- Optional supplement: **TVDb** for alternate show ordering (requires account/key)
- Optional: **OMDb** for IMDb-linked data (requires key; effectively a gateway)
- Artwork enrichment: **fanart.tv** (logos/clearart/etc; requires key)
- Music: **MusicBrainz** (+ Cover Art Archive where applicable)
- Subtitles: **OpenSubtitles.com** (account required)

### Why multiple providers?
Because the world is messy:
- one provider may have better posters,
- another has better episode ordering,
- a third has the best translations.

But you should still design Rustfin so that **provider count is configurable and limited**, not a free-for-all.

---

## 6. Subtitles: sidecar rules, embedded tracks, extraction, and downloading

Subtitles are where “just stream the file” gets complicated.

### 6.1 Subtitle types you must support
1) **External sidecar files** (best for compatibility)
   - `.srt`, `.ass/.ssa`, `.vtt`, `.sub/.idx`, etc.
2) **Embedded tracks**
   - MKV often contains PGS/VobSub or text tracks
3) **Forced subtitles**
   - “only when needed” tracks (foreign dialogue)
4) **SDH / HI**
   - “captions” (Sound effects, speaker labels)

### 6.2 Sidecar subtitle discovery (practical rules)
Most Jellyfin setups rely on “subtitle file is in the same folder as video and matches the base filename”, with language/disposition in the filename.

Rustfin should implement:
- match sidecar files with same base stem
- parse language tags (`en`, `fr`, `es`, etc.)
- parse disposition tags (`default`, `forced`, `sdh`/`hi`)

### 6.3 Embedded subtitle extraction (optional, but very useful)
Jellyfin has plugins that can extract embedded subtitles during scans or scheduled tasks.

Rustfin should treat this as a **background job**:
- detect embedded streams (via ffprobe/libav)
- extract to sidecar files into:
  - either media folder (if writable)
  - or a managed “subs cache” directory keyed by media item + stream id

### 6.4 Remote subtitle downloading
Jellyfin provides this through subtitle provider plugins (e.g., OpenSubtitles plugin).

Rustfin should implement:
- server-wide configured subtitle providers (with credentials)
- per-item “Search subtitles” UI action:
  - choose language
  - show ranked results
  - download selected result
  - store it as sidecar + index in DB

---

## 7. Theme songs, backdrop videos, and other “bells & whistles”

Jellyfin has a plugin ecosystem where “theme songs” and “extra media” can be attached to items.

Rustfin can support this with a clean, local-first model:
- sidecar directories like:
  - `theme.mp3` (or `theme.*`)
  - `backdrop/` for background videos
- plus optional provider integrations (e.g., ThemerrDB-like catalogs)

Key point: you want these extras to be **optional** and **cacheable**.

---

## 8. Chapter images, trickplay, and intro/segment metadata

These features make the UI feel “premium”:

### 8.1 Chapter images
Chapter images are per-chapter preview thumbnails and can be enabled/disabled independently from chapters.

Rustfin can implement:
- `ffprobe` to read chapter markers
- frame extraction at chapter timestamps to JPEG/WebP
- store keyed by item_id + chapter_index

### 8.2 Trickplay (scrubbing thumbnails)
This is typically a grid of thumbnails at fixed intervals. Implement as:
- background job generating N images per minute or per N seconds
- store as sprite sheets or individual images
- serve via endpoints with byte-range support (for speed)

### 8.3 Intro/segment metadata
Jellyfin has plugins that interpret chapter names or other heuristics into “segments” like intro/outro/recap.

Rustfin should model “segments” as:
- a small table keyed by item + start/end time + segment_type + confidence
- produced by either:
  - regex rules on chapter titles
  - ML model (optional; likely overkill for local-only)

---

## 9. Users, personalization, and per-library permissions

Even local-only servers need:
- multiple users
- per-user watch state
- per-user subtitle and audio prefs
- per-library access restrictions

Rustfin should model:
- `users`
- `user_policies`
- `user_item_state` (played, progress, last_played, rating, favorite)
- `user_display_preferences` (home sections, sort orders, per-library)

---

## 10. Streaming & transcoding (short version) and where GPU comes in

Jellyfin’s model:
1) direct play (no change)
2) remux (container change)
3) transcode (re-encode; may use GPU)

Rustfin should:
- implement a decision engine that chooses the cheapest viable path
- build HLS for mobile clients (it’s the “least surprising” path)
- allow per-user/per-device max bitrate caps

GPU acceleration is done via FFmpeg hardware accelerators (NVENC/VAAPI/QSV/etc). Treat this as:
- a config-driven set of FFmpeg args
- a capability probe at startup
- per-job selection logic

---

## 11. Rustfin architecture: modules, traits, data model, and background jobs

### 11.1 Core crates/modules (monolith, but internally modular)
```
rustfin/
  crates/
    server/                # axum, routes, auth, sessions
    core/                  # domain types (Movie, Episode, ImageType, etc.)
    db/                    # sqlx, migrations, queries
    scanner/               # filesystem scanning + parsing
    metadata/              # provider traits + merge rules
    subtitles/             # subtitle detection/download/extraction
    transcoder/            # ffmpeg job orchestration + HLS serving
    ui/ (optional)         # leptos app (or separate Next.js app)
```

### 11.2 Database: keep it local, fast, and simple
Use SQLite unless you *know* you need Postgres. SQLite is perfect for local-first and avoids deployment friction.
- store DB on local SSD
- store media on NAS if needed
- store cache on SSD if possible

### 11.3 Background jobs (Tokio + queue)
You need cancelable tasks:
- scan library
- identify unknown items
- refresh metadata
- download artwork
- generate chapter images / trickplay
- extract subtitles

Use a job queue with:
- `job_id`
- `job_type`
- `status`
- `progress`
- `cancellation token`

---

## 12. Code-heavy skeletons (metadata, artwork, subtitles, and scanning)

> These are intentionally “starter-grade”: compile-worthy patterns, not a full implementation.
> They’re meant to reduce your “blank page” pain.

### 12.1 Core domain types

```rust
// crates/core/src/media.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LibraryType {
    Movies,
    Shows,
    Music,
    Books,
    Photos,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageType {
    Primary,    // poster/cover
    Backdrop,   // background fanart
    Banner,
    Logo,
    Thumb,
    Art,
    Disc,
    Box,
    BoxRear,
    Screenshot,
    Menu,
    Chapter,
    Profile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ItemKind {
    Movie,
    Series,
    Season,
    Episode,
    MusicArtist,
    MusicAlbum,
    Track,
    Book,
    Photo,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderIds {
    pub imdb: Option<String>,   // tt123...
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
    pub musicbrainz: Option<String>,
    // Extend safely.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: ItemId,
    pub kind: ItemKind,
    pub library_id: LibraryId,

    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<i32>,

    pub provider_ids: ProviderIds,

    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub studios: Vec<String>,

    pub runtime_seconds: Option<i64>,
}
```

### 12.2 Filename parsing for provider IDs

```rust
// crates/scanner/src/ids.rs
use regex::Regex;
use crate::ScanError;
use rustfin_core::media::ProviderIds;

pub fn parse_provider_ids(name: &str) -> Result<ProviderIds, ScanError> {
    // Matches: [imdbid-tt1234567]  [tmdbid-65567]  [tvdbid-12345]
    // Jellyfin allows multiple identifiers.
    let re = Regex::new(r"\[(?P<kind>imdbid|tmdbid|tvdbid)-(?P<val>[^\]]+)\]").unwrap();

    let mut ids = ProviderIds {
        imdb: None,
        tmdb: None,
        tvdb: None,
        musicbrainz: None,
    };

    for cap in re.captures_iter(name) {
        match &cap["kind"] {
            "imdbid" => ids.imdb = Some(cap["val"].to_string()),
            "tmdbid" => ids.tmdb = cap["val"].parse::<u64>().ok(),
            "tvdbid" => ids.tvdb = cap["val"].parse::<u64>().ok(),
            _ => {}
        }
    }

    Ok(ids)
}
```

### 12.3 Metadata provider traits (pluggable, but not chaotic)

```rust
// crates/metadata/src/provider.rs
use async_trait::async_trait;
use rustfin_core::media::{MediaItem, ProviderIds};

#[derive(Debug, Clone)]
pub enum SearchKind {
    Movie,
    Series,
    Episode { season: u32, episode: u32 },
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub title: String,
    pub year: Option<i32>,
    pub kind: SearchKind,
    pub ids: ProviderIds,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub provider: &'static str,
    pub score: f32,
    pub ids: ProviderIds,
    pub canonical_title: String,
    pub year: Option<i32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("network: {0}")]
    Network(String),
    #[error("rate limited")]
    RateLimited,
    #[error("not found")]
    NotFound,
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Search for candidate matches given a query.
    async fn search(&self, q: &SearchQuery) -> Result<Vec<SearchResult>, ProviderError>;

    /// Fetch full metadata for a specific item (by provider IDs).
    async fn fetch_item(&self, ids: &ProviderIds) -> Result<MediaItem, ProviderError>;
}
```

### 12.4 Provider ordering + merge strategy

```rust
// crates/metadata/src/merge.rs
use rustfin_core::media::MediaItem;

/// Merge remote data into an existing item without stomping user edits.
/// You can store per-field "locks" in the DB if you want true Jellyfin-style behavior.
pub fn merge_item(base: &mut MediaItem, incoming: MediaItem) {
    // Title/year tend to be stable; but don't overwrite if user already edited.
    if base.title.trim().is_empty() {
        base.title = incoming.title;
    }
    if base.year.is_none() {
        base.year = incoming.year;
    }

    // Prefer non-empty overview, but keep existing if present.
    if base.overview.as_deref().unwrap_or("").trim().is_empty() {
        base.overview = incoming.overview;
    }

    // Genres/studios: union.
    for g in incoming.genres {
        if !base.genres.iter().any(|x| x.eq_ignore_ascii_case(&g)) {
            base.genres.push(g);
        }
    }
    for s in incoming.studios {
        if !base.studios.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
            base.studios.push(s);
        }
    }

    // Provider IDs: fill in missing.
    if base.provider_ids.imdb.is_none() { base.provider_ids.imdb = incoming.provider_ids.imdb; }
    if base.provider_ids.tmdb.is_none() { base.provider_ids.tmdb = incoming.provider_ids.tmdb; }
    if base.provider_ids.tvdb.is_none() { base.provider_ids.tvdb = incoming.provider_ids.tvdb; }
}
```

### 12.5 Artwork pipeline: cache + resize + type variants

```rust
// crates/metadata/src/artwork.rs
use rustfin_core::media::{ImageType, ItemId};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ImageKey {
    pub item_id: ItemId,
    pub kind: ImageType,
    pub index: u32, // e.g., multiple backdrops
}

#[derive(Debug, Clone)]
pub struct ImageVariant {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
}

#[derive(Debug, Clone, Copy)]
pub enum ImageFormat { Jpeg, Webp, Png }

pub trait ImageStore {
    fn put_original(&self, key: &ImageKey, bytes: &[u8]) -> anyhow::Result<()>;
    fn get_original_path(&self, key: &ImageKey) -> Option<PathBuf>;

    fn put_variant(&self, key: &ImageKey, v: &ImageVariant, bytes: &[u8]) -> anyhow::Result<()>;
    fn get_variant_path(&self, key: &ImageKey, v: &ImageVariant) -> Option<PathBuf>;
}
```

### 12.6 Subtitles: sidecar discovery + provider download

```rust
// crates/subtitles/src/model.rs
use rustfin_core::media::ItemId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtitleKind {
    ExternalFile,      // .srt next to media
    ExtractedEmbedded, // extracted to cache
    RemoteDownloaded,  // fetched from provider
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub item_id: ItemId,
    pub language: Option<String>,     // "en"
    pub title: Option<String>,        // "SDH" / "Commentary"
    pub is_forced: bool,
    pub is_default: bool,
    pub is_sdh: bool,
    pub kind: SubtitleKind,
    pub path: String,                // local filesystem path
    pub format: String,              // "srt", "ass", "vtt"
}
```

```rust
// crates/subtitles/src/discovery.rs
use std::path::{Path, PathBuf};

pub fn discover_sidecar_subs(video_path: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let dir = match video_path.parent() {
        Some(d) => d,
        None => return out,
    };

    let stem = match video_path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return out,
    };

    // Very simple: find files that start with the same stem.
    // Example: Movie (2024).en.forced.srt
    if let Ok(rd) = std::fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            let fname = match p.file_name().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if fname.starts_with(stem) && is_sub_ext(&p) {
                out.push(p);
            }
        }
    }

    out
}

fn is_sub_ext(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("srt") | Some("ass") | Some("ssa") | Some("vtt") | Some("sub") | Some("idx")
    )
}
```

### 12.7 A minimal “OpenSubtitles-like” provider trait

```rust
// crates/subtitles/src/provider.rs
use async_trait::async_trait;
use rustfin_core::media::ProviderIds;

#[derive(Debug, Clone)]
pub struct SubtitleSearchQuery {
    pub title: String,
    pub year: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub ids: ProviderIds,
    pub language: String, // "en"
}

#[derive(Debug, Clone)]
pub struct SubtitleCandidate {
    pub provider: &'static str,
    pub id: String,
    pub file_name: String,
    pub score: f32,
    pub hearing_impaired: bool,
    pub is_forced: bool,
}

#[async_trait]
pub trait SubtitleProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn search(&self, q: &SubtitleSearchQuery) -> anyhow::Result<Vec<SubtitleCandidate>>;

    async fn download(&self, candidate_id: &str) -> anyhow::Result<Vec<u8>>;
}
```

### 12.8 Job queue: scan → identify → metadata → artwork → subtitles

```rust
// crates/server/src/jobs.rs
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum JobType {
    ScanLibrary { library_id: Uuid },
    IdentifyMissing { library_id: Uuid },
    RefreshMetadata { item_id: Uuid, full_replace: bool },
    DownloadArtwork { item_id: Uuid },
    GenerateChapterImages { item_id: Uuid },
    ExtractSubtitles { item_id: Uuid },
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: Uuid,
    pub ty: JobType,
}

pub struct JobQueue {
    tx: mpsc::Sender<Job>,
}

impl JobQueue {
    pub fn new(tx: mpsc::Sender<Job>) -> Self { Self { tx } }

    pub async fn enqueue(&self, ty: JobType) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        self.tx.send(Job { id, ty }).await?;
        Ok(id)
    }
}
```

### 12.9 ffprobe-based subtitle/chapter introspection (shell-out version)

> This matches how many media servers integrate quickly: call ffprobe, parse JSON.

```rust
// crates/transcoder/src/ffprobe.rs
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
pub struct FfprobeOutput {
    pub streams: Option<Vec<FfprobeStream>>,
    pub chapters: Option<Vec<FfprobeChapter>>,
}

#[derive(Debug, Deserialize)]
pub struct FfprobeStream {
    pub index: Option<u32>,
    pub codec_type: Option<String>, // "video" | "audio" | "subtitle"
    pub codec_name: Option<String>,
    pub tags: Option<std::collections::HashMap<String, String>>,
    pub disposition: Option<std::collections::HashMap<String, i32>>,
}

#[derive(Debug, Deserialize)]
pub struct FfprobeChapter {
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

pub async fn ffprobe(path: &Path) -> anyhow::Result<FfprobeOutput> {
    let out = Command::new("ffprobe")
        .arg("-v").arg("warning")
        .arg("-print_format").arg("json")
        .arg("-show_streams")
        .arg("-show_chapters")
        .arg("-i").arg(path)
        .output()
        .await?;

    if !out.status.success() {
        return Err(anyhow::anyhow!("ffprobe failed: {:?}", out.status));
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&out.stdout)?;
    Ok(parsed)
}
```

---

## 13. Roadmap: from “first scan works” to Jellyfin-class polish

### Phase 1: Minimal viable server
- Libraries, scan, DB persistence
- Basic metadata (title/year from filename)
- Direct file streaming (range requests)
- User login + sessions

### Phase 2: Metadata + artwork parity
- Provider IDs parsing
- TMDb provider (movies/shows)
- Artwork download + caching
- Manual Identify + Refresh flows

### Phase 3: Subtitles parity
- Sidecar discovery rules
- Remote provider (OpenSubtitles)
- Embedded extraction job
- Subtitle delivery policies

### Phase 4: Streaming quality + device support
- Client profiles (capabilities)
- Remux vs transcode decision engine
- HLS job manager
- GPU accel config presets

### Phase 5: Premium polish
- Chapter images + trickplay
- Theme songs / backdrop videos
- Intro skip via segment metadata
- Collections, playlists, mixed libraries smoothing

---

## Appendix A: sane defaults (config + caching + rate limits)

- Cache remote provider requests with ETags where possible.
- Store provider API keys encrypted at rest (even local-only).
- Rate-limit outbound calls per provider.
- Prefer local metadata; don’t stomp user edits.
- Never store the DB on flaky network storage.

---

## Appendix B: provider capability matrix (movies/shows/music/books)

| Provider | Movies | Shows | Images | Ratings | Notes |
|---|---:|---:|---:|---:|---|
| TMDb | ✅ | ✅ | ✅ | ✅ | best default; good translations |
| TVDb | ❌/✅* | ✅ | ✅ | ✅ | *mostly shows; needs key |
| OMDb | ✅ | ✅ | ❌ | ✅ | adds IMDb-style ratings via API |
| fanart.tv | ✅ | ✅ | ✅✅ | ❌ | logos/clearart/extra images |
| MusicBrainz | ❌ | ❌ | via CAA | ❌ | music metadata |

---

*End of document.*
