# Rustfin Master Spec v2 (Extreme Detail)
*A “Jellyfin‑class” media server, but Rust-first, local-first, and engineered to handle the annoying TV edge-cases (seasons, missing episodes, weird naming), with Docker support.*

> This is an **additive** spec: it extends the original Rustfin master spec with **TV-series correctness**, **season/episode artwork**, **missing-episode accounting**, **naming heuristics**, and **Docker + GPU** operational details.

---

## What it covers (and where it’s grounded in how Jellyfin actually behaves)

This doc explicitly models behaviors Jellyfin already exhibits, so “Rustfin” doesn’t accidentally become a media player that *almost* understands TV.

- **Series → Seasons → Episodes is the default mental model**: Jellyfin recommends organizing TV under a *series folder*, then *season folders* (`Season 01`, `Season 02`, `Season 00` for specials), with SxxExx naming for episodes.  
  Source: Jellyfin TV show organization and naming examples.  
- **Multi-episode files exist, but Jellyfin treats them as a single entry** (metadata aggregated) and recommends splitting them.  
  Source: Jellyfin TV show doc notes multi-episode behavior and recommends splitting.  
- **Local images override remote** and Jellyfin supports a defined set of artwork filenames and types (Primary/Backdrop/Banner/Logo/Thumb).  
  Source: Jellyfin TV show doc “Metadata Images” table and precedence rule.  
- **Specials live in Season 00 and can be injected into the main season order** via “airsbefore/airsafter” metadata + settings.  
  Source: Jellyfin TV show doc “Show Specials” behavior.  
- **Provider IDs in folder names** can be used to improve identification and reduce wrong matches; Jellyfin documents supported providers (TMDb / TVDb for shows / OMDb).  
  Source: Jellyfin “Metadata Provider Identifiers” doc.  
- **.nfo local metadata has priority** (Jellyfin states local metadata has priority over remote providers).  
  Source: Jellyfin NFO metadata doc.  
- **Subtitles can be sidecars** (naming via suffix flags) and Jellyfin can also download subs via the OpenSubtitles plugin.  
  Sources: Jellyfin “External Subtitles and Audio Tracks” + OpenSubtitles plugin doc.  
- **Jellyfin’s container layout** uses `/config`, `/cache`, `/media` volumes and documents rootless + GPU device access patterns.  
  Source: Jellyfin container install doc.

References are at the end of this file.

---

## 0. The user intent we’re implementing

You described the UX you want:

- If “Breaking Bad” has 6 seasons, the UI should show **one show** called **Breaking Bad**, and inside it **Season 01 … Season 06** by default.  
  Not “Breaking Bad (Season 1)” as separate shows, and not a flattened mess by default.
- Season cover art should be fetched and displayed per-season (Season posters/primary images).
- If an episode is missing on disk, Rustfin should be able to say:  
  “Season 03 has 16 expected episodes; 15 present; missing: S03E07 (title…).”
- Weirdly named files should still match episodes reliably (“Season 3 Episode 1”, “3x01”, “S3E1”, etc.).
- Episode titles should be filled from metadata providers when the filename has none.
- This should be configurable, but with minimal “server admin fiddling”; config lives in DB, defaults are sane.

---

## 1. Core data model: series are entities; seasons are views; episodes are both metadata and files

Jellyfin’s recommendations imply a data model where:

- A **Series** is an entity (stable ID).
- A **Season** is a child entity of a series (by season_number).
- An **Episode** is a child entity of a season (by episode_number), but may have 0–N physical files.

Rustfin should model this explicitly so we can:

- list “expected episodes” even when files are missing,
- support multi-file/parted episodes,
- preserve ordering modes (aired vs DVD vs absolute),
- avoid accidental “season becomes its own show”.

### 1.1 Identifiers

You want deterministic identity. Do it like this:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderIds {
    pub tmdb: Option<u32>,
    pub tvdb: Option<u32>,
    pub imdb: Option<String>, // tt...
    pub anidb: Option<u32>,   // optional future
}
```

Store ProviderIds on **Series**, and optionally on **Season** and **Episode** if the provider gives stable IDs.

Why: provider IDs are the “no more guessing” lever Jellyfin explicitly documents (folder name tags like `[tvdbid-79168]`). Without them, “similar-name” series are an endless swamp.

### 1.2 Canonical episode key

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EpisodeKey {
    pub season: u16,
    pub episode: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesEpisodeKey {
    pub series_id: uuid::Uuid,
    pub key: EpisodeKey,
}
```

### 1.3 Physical media mapping (files)

An “episode” may have:

- exactly one file (normal),
- multiple parts (S01E01-part-1 / part-2),
- one file that contains multiple episodes (S01E01-E02),
- multiple encodes (“versions”) (1080p vs 4K), though TV “versions” are trickier than movies.

Model this:

```rust
#[derive(Debug, Clone)]
pub struct MediaFile {
    pub id: uuid::Uuid,
    pub path: std::path::PathBuf,
    pub container: String,      // mkv/mp4
    pub duration_ms: u64,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
    pub hash_fast: u64,         // quick-change detection
    pub hash_strong: Option<[u8; 32]>, // optional
}

#[derive(Debug, Clone)]
pub enum EpisodeFileMapping {
    Single { episode: EpisodeKey, file_id: uuid::Uuid },
    MultiPart { episode: EpisodeKey, parts: Vec<uuid::Uuid> },
    MultiEpisodeInOneFile { episodes: Vec<EpisodeKey>, file_id: uuid::Uuid },
}
```

---

## 2. Folder + filename parsing strategy: accept the real world, but prefer determinism

Jellyfin’s “happy path” is SxxExx and Season folders. We’ll implement that fully, but we’ll also accept messy patterns.

### 2.1 Supported canonical layouts (recommended)

Rustfin should fully support Jellyfin’s recommended organization (series folder → season folder → episodes) and specials in `Season 00`.

Example (from Jellyfin docs):

```
Shows/
  Series Name A (2010)/
    Season 00/
      Series Name A S00E01.mkv
    Season 01/
      Series Name A S01E01.mkv
```

### 2.2 Disambiguation: year + provider id tags

You mentioned “alchemy / philosopher’s stone / reverse effect” vibes — that’s the classic “same-ish name, different adaptation” problem. Example in the wild: *Fullmetal Alchemist (2003)* vs *Fullmetal Alchemist: Brotherhood (2009)*.

The rule:

- If folder name includes a year, use it as a hard hint.
- If folder name includes a provider id tag, treat it as authoritative identity.

```text
Shows/
  Fullmetal Alchemist (2003) [tvdbid-...]/
  Fullmetal Alchemist Brotherhood (2009) [tvdbid-...]/
```

This prevents “two different shows” from merging.

---

## 3. Seasons in the UI: default behavior and configuration

### 3.1 Default: series tile → season grid → episode list

Default browsing flow:

1. Library view shows **Series** tiles.
2. Selecting a series shows **Seasons** (Season 01..N, plus Specials if present).
3. Selecting a season shows **Episodes** (sorted by configured order).

### 3.2 Optional: “flatten seasons” view (off by default)

Some users prefer “all episodes” as one list.

Make it a per-user preference:

```rust
pub enum SeriesBrowseMode {
    SeasonsFirst, // default
    FlatEpisodes, // optional
}
```

Store this in user preferences, not global server settings.

### 3.3 Empty seasons and “missing episodes”

Jellyfin has toggles related to “display missing episodes” and the community has seen edge-cases where empty seasons/specials still show depending on settings and clients.

Rustfin design goal:

- If missing episodes display is OFF, do not show “empty” seasons.
- If missing episodes display is ON, show expected seasons/episodes as placeholders, but clearly label them and don’t break “next up” logic.

---

## 4. Metadata providers: picking a truth source, and why this matters for missing episodes

The nasty secret: different providers disagree on episode lists (especially for specials, anime, and older shows). Jellyfin users repeatedly hit this.

Therefore Rustfin must support:

- per-library provider preference,
- per-series override,
- consistent provider choice for: (series metadata, season list, episode list, images, people).

### 4.1 Default provider policy (recommended)

- **TV**: prefer TVDb for episode lists when available; otherwise TMDb.
- **Movies**: TMDb (plus optional OMDb enrichment for IMDb-specific fields).
- Always allow local NFO to override.

Reason: “missing episode counting” is meaningless unless you’ve chosen a canonical episode list.

### 4.2 Provider mismatch handling

When providers disagree:

- store multiple mappings, but select one as **canonical ordering** for display and missing checks.
- allow the user to switch and re-map, without re-scanning files.

```rust
pub enum EpisodeOrdering {
    Aired,
    Dvd,
    Absolute,
}

pub struct SeriesProviderPolicy {
    pub canonical_provider: Provider,
    pub ordering: EpisodeOrdering,
    pub allow_fallback_provider: bool,
}
```

---

## 5. Fetching season cover art (and other artwork)

### 5.1 Local artwork first

Jellyfin’s docs state that external images placed alongside media take precedence over other sources, and it documents artwork filename conventions (poster/folder/cover/backdrop/logo/thumb etc) that work for Series, Season, Episode. That is exactly what Rustfin should do too.

Implement a local artwork scanner:

```rust
pub enum ImageType { Primary, Backdrop, Banner, Logo, Thumb }

pub struct LocalImage {
    pub path: std::path::PathBuf,
    pub image_type: ImageType,
    pub for_item: ImageOwner,
}

pub enum ImageOwner {
    Series(uuid::Uuid),
    Season { series: uuid::Uuid, season: u16 },
    Episode { series: uuid::Uuid, season: u16, episode: u16 },
}
```

Practical season poster rules you should support:

- `Season 01/poster.jpg` (season primary)
- `Season 01/folder.jpg` (season primary)
- `Season 01/cover.jpg` (season primary)
- In some Jellyfin setups, season posters may also exist at series root with names like `season01-poster.jpg` (seen in the wild). Support it as a compatibility feature.

### 5.2 Remote artwork (TMDb, fanart.tv, etc)

You want background images, season posters, logos, etc.

For TMDb, image URLs are built by combining:

- base_url (from `/configuration`)
- file_size (like `w500`)
- file_path (like `/abc123.jpg`)

That’s TMDb’s documented image scheme.

Rustfin plan:

1. call TMDb `/configuration` once and cache the result (day-long TTL).
2. for series / season / episode, fetch image lists.
3. download originals into cache (content-addressed).
4. generate size variants on demand (or precompute a few common sizes).
5. store `ImageRef` rows in DB per item.

```rust
pub struct TmdbImageConfig {
    pub base_url: String,
    pub poster_sizes: Vec<String>,
    pub backdrop_sizes: Vec<String>,
    pub logo_sizes: Vec<String>,
}

pub fn tmdb_image_url(cfg: &TmdbImageConfig, size: &str, path: &str) -> String {
    format!("{}/{}/{}", cfg.base_url.trim_end_matches('/'), size, path.trim_start_matches('/'))
}
```

### 5.3 Season posters specifically

Two sources of season art:

- provider season poster lists (TMDb seasons often include poster paths),
- fanart.tv style curated images,
- user-supplied local images.

Rustfin should choose by priority:

1) local season images  
2) user-selected remote image  
3) default remote “best” image (heuristic: language match, highest vote/quality)

---

## 6. Missing episodes: the “completeness” engine

This is the feature you’re asking for most directly.

We want to compute, per series and per season:

- expected episodes (canonical list),
- present episodes (files mapped),
- missing episodes,
- “unknown files” (present on disk but not mapped cleanly).

### 6.1 The canonical episode list

When metadata is refreshed, Rustfin should store the canonical episode list as rows, even if no files exist.

```sql
CREATE TABLE episode_expected (
  series_id TEXT NOT NULL,
  season_number INTEGER NOT NULL,
  episode_number INTEGER NOT NULL,
  title TEXT,
  air_date TEXT,
  provider TEXT NOT NULL,
  provider_episode_id TEXT,
  PRIMARY KEY(series_id, season_number, episode_number)
);
```

### 6.2 Presence mapping

As the scanner maps files to episodes, store presence:

```sql
CREATE TABLE episode_presence (
  series_id TEXT NOT NULL,
  season_number INTEGER NOT NULL,
  episode_number INTEGER NOT NULL,
  present INTEGER NOT NULL, -- 0/1
  file_id TEXT,            -- optional, for single-file mapping
  last_seen_ts INTEGER NOT NULL,
  PRIMARY KEY(series_id, season_number, episode_number)
);
```

### 6.3 Missing calculation query

```sql
SELECT e.season_number, e.episode_number, e.title
FROM episode_expected e
LEFT JOIN episode_presence p
  ON p.series_id = e.series_id
 AND p.season_number = e.season_number
 AND p.episode_number = e.episode_number
WHERE e.series_id = ?
  AND (p.present IS NULL OR p.present = 0)
ORDER BY e.season_number, e.episode_number;
```

### 6.4 UX rules

- If missing episodes display is ON:
  - show placeholders for missing episodes with a “Missing” badge,
  - allow “Download subtitles” / “Search metadata” actions to still work for missing entries (useful for planning).
- If missing episodes display is OFF:
  - hide placeholders entirely,
  - hide empty seasons entirely.

### 6.5 “Future episodes” vs “missing episodes”

Providers may list unaired future episodes. Treat them separately:

- `status = Future` if air_date is in the future.
- `status = Missing` if air_date is in past and not present.

---

## 7. Odd naming schemes: robust parsing without turning into a regex crime scene

You want to support:

- `S03E01`
- `S3E1`
- `3x01`
- `Season 3 Episode 1`
- `Season 03 Ep 01`
- `Series.Name.S03E01.1080p.WEB-DL.mkv`
- `Series Name - 301 - Title` (some scene releases)
- date-based shows (news): `2025-01-31`

### 7.1 Parser layering (fastest, least wrong first)

Order of operations:

1. **Exact provider-id pinned**: if the series folder has `[tvdbid-…]`, parse within that series only.
2. **Strong SxxExx patterns**.
3. **Season/Episode words** patterns.
4. **3x01 patterns**.
5. **numeric block patterns** like `301` if the show is known and the season has enough episodes to disambiguate.
6. **date-based** episode detection for shows that are configured as date-based.

Do not jump to “301 means S03E01” unless:
- the series is known,
- season 3 exists in canonical list,
- the file’s directory context suggests season 3, or
- the pattern appears consistently for that series.

### 7.2 Rust parsing skeleton

```rust
use regex::Regex;

#[derive(Debug, Clone)]
pub struct EpisodeParse {
    pub season: u16,
    pub episode: u16,
    pub confidence: f32,
    pub pattern: &'static str,
}

pub fn parse_episode_tokens(name: &str) -> Option<EpisodeParse> {
    // 1) S01E02 / s1e2
    static SXXEXX: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(?i)\bs(?P<s>\d{1,2})\s*e(?P<e>\d{1,3})\b").unwrap());

    if let Some(c) = SXXEXX.captures(name) {
        let s: u16 = c["s"].parse().ok()?;
        let e: u16 = c["e"].parse().ok()?;
        return Some(EpisodeParse { season: s, episode: e, confidence: 0.95, pattern: "SxxExx" });
    }

    // 2) 3x01
    static XSEP: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(?i)\b(?P<s>\d{1,2})x(?P<e>\d{1,3})\b").unwrap());
    if let Some(c) = XSEP.captures(name) {
        let s: u16 = c["s"].parse().ok()?;
        let e: u16 = c["e"].parse().ok()?;
        return Some(EpisodeParse { season: s, episode: e, confidence: 0.90, pattern: "SxE" });
    }

    // 3) Season 3 Episode 1
    static WORDY: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(?i)\bseason\s*(?P<s>\d{1,2}).*?\b(ep|episode)\s*(?P<e>\d{1,3})\b").unwrap());
    if let Some(c) = WORDY.captures(name) {
        let s: u16 = c["s"].parse().ok()?;
        let e: u16 = c["e"].parse().ok()?;
        return Some(EpisodeParse { season: s, episode: e, confidence: 0.85, pattern: "Season Episode" });
    }

    None
}
```

### 7.3 Handling “episode title missing”

If the filename has no useful title, do:

- map season/episode from parse,
- fetch canonical title from provider list,
- store it in DB,
- present it in UI.

Never trust a random “garbage string” as title unless user explicitly says “use filename title”.

Config options:

- `title_source = Provider` (default)
- `title_source = Filename`
- `title_source = ProviderUnlessFilenameHasTitle`

---

## 8. Multi-episode files and split episodes: don’t lie to the user

Jellyfin documents multi-episode files and warns they appear as a single entry containing metadata from multiple episodes.

Rustfin options:

### 8.1 Default: treat multi-episode file as “one playable item” but show both episodes

Represent it as a single file with multiple EpisodeKeys.

UI should show:

- Episode 1 & 2 grouped, playable as one.
- Mark both as present.

### 8.2 Split episodes (one episode across multiple files)

Jellyfin supports `-part-1`, `-part-2` naming for stacking.

Rustfin should:

- stack parts into one logical playable episode,
- ensure resume/chapters work across parts (hard; optional for v1),
- alternatively play sequentially.

---

## 9. Specials and weird ordering (the “why is this episode here?” problem)

Jellyfin’s docs define specials:

- `Season 00` folder
- specials can be inserted into a season order using `airsbefore_*` / `airsafter_*` metadata and a setting.

Rustfin should replicate this with a stored “insertion rule” per special:

```rust
pub enum SpecialPlacement {
    SpecialsSeasonOnly, // default
    AlsoInSeason { season: u16, before_episode: Option<u16>, after_season: Option<u16> },
}
```

Store placement in DB and allow it in NFO import.

---

## 10. Configuration: minimal knobs, but everything is overridable

Your constraint: flexible and writable, but minimal manual config.

Rule: **everything configurable exists in DB**, not in “edit 12 YAML files”.

### 10.1 Library config (DB)

```sql
CREATE TABLE library (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,         -- shows/movies/music
  root_path TEXT NOT NULL,
  provider_default TEXT NOT NULL,
  display_missing INTEGER NOT NULL,
  created_ts INTEGER NOT NULL
);
```

### 10.2 Series overrides

```sql
CREATE TABLE series_settings (
  series_id TEXT PRIMARY KEY,
  canonical_provider TEXT,
  ordering TEXT, -- aired/dvd/absolute
  browse_mode TEXT, -- seasons/flat
  title_source TEXT, -- provider/filename/merge
  date_based INTEGER DEFAULT 0
);
```

---

## 11. Docker support: packaging Rustfin like a grown-up, even if local-only

Jellyfin’s container docs lay out:

- ports,
- volumes (/config /cache /media),
- rootless recommendations,
- GPU access notes.

Rustfin should follow the same operational shape because users already understand it.

### 11.1 Dockerfile (multi-stage Rust build)

```dockerfile
# syntax=docker/dockerfile:1

FROM rust:1.78-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release -p rustfin_server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    ffmpeg \
    tini \
  && rm -rf /var/lib/apt/lists/*

# runtime dirs
RUN mkdir -p /config /cache /media /transcode \
 && useradd -u 1000 -m rustfin \
 && chown -R rustfin:rustfin /config /cache /transcode

COPY --from=builder /src/target/release/rustfin_server /usr/local/bin/rustfin

USER rustfin
EXPOSE 8096
VOLUME ["/config", "/cache", "/media", "/transcode"]
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["rustfin", "--config-dir", "/config", "--cache-dir", "/cache", "--media-dir", "/media", "--transcode-dir", "/transcode"]
```

Why include ffmpeg in the image?
- Jellyfin’s container images install ffmpeg because transcoding is essential.  
- Rustfin will also need ffmpeg (or an ffmpeg-compatible library stack) for transcoding, subtitle extraction, probe, etc.

### 11.2 docker-compose.yml

```yaml
services:
  rustfin:
    image: rustfin/rustfin:latest
    container_name: rustfin
    ports:
      - "8096:8096/tcp"
      # optional discovery port if you implement it
      # - "7359:7359/udp"
    volumes:
      - /path/to/config:/config
      - /path/to/cache:/cache
      - /path/to/media:/media:ro
      - /path/to/transcode:/transcode
    # Intel/AMD VAAPI / QuickSync (Linux)
    devices:
      - /dev/dri:/dev/dri
    environment:
      - RUST_LOG=info
```

### 11.3 NVIDIA GPUs

If you want NVENC/NVDEC in containers, you typically need the NVIDIA container runtime/toolkit on the host. Keep it optional.

Rustfin should detect GPU availability at runtime and expose a “transcode capabilities” endpoint so clients can pick direct play vs transcode.

---

## 12. “Other Jellyfin features” you should not forget

Jellyfin’s TV show docs explicitly include:

- extras folders (`trailers`, `theme-music`, `backdrops`, etc.)
- external subtitles/audio tracks
- metadata images precedence

Rustfin should include parity hooks even if v1 doesn’t implement every client UI:

- **Extras**: index and present (trailers, behind-the-scenes, theme music, backdrops).
- **Theme music autoplay** in web UI (optional).
- **Background backdrops cycling** (multiple backdrops supported).
- **3D flags** (optional, but parseable).
- **Multiple parts stacking**.
- **User permissions / users**.
- **Playback speed**.
- **Transcode quality profiles** (mobile vs desktop).

---

## 13. Why Rust + these techniques solve actual problems

- **Rust memory safety + async I/O**: streaming servers do lots of concurrent sockets + file reads. Rust reduces “one bad buffer ruins your day”.
- **Single binary server** (plus ffmpeg): fewer moving parts than “glue 7 runtimes together”.
- **Strong types for media identity**: prevents accidental merges (series vs season), which is *exactly* the bug-class you’re worried about.
- **DB-first configuration**: UI-driven configuration with schema migrations beats “edit configs until you cry”.
- **Provider-canonical episode lists**: makes “missing episodes” meaningful and stable.

---

## 14. Implementation roadmap for these new requirements

1) **Episode list canonicalization**
- Implement TMDb/TVDb episode list fetch and store `episode_expected`.
- Implement per-series provider override.

2) **Scanner + parser robustness**
- Implement layered parser (SxxExx / 3x01 / Season Episode / date-based).
- Implement mapping and store `episode_presence`.

3) **Missing engine + UI**
- Implement queries and UI toggles (missing/future).
- Ensure empty seasons don’t show when missing is off.

4) **Season artwork**
- Implement local season artwork resolver (poster/folder/cover in season folder).
- Implement provider season poster fetch and caching.

5) **Docker + GPU**
- Ship container image and compose template.
- Implement device capability probing + transcode profiles.

---

## References (primary sources used)

- Jellyfin TV Shows docs (organization, specials, images, extras, subtitles): https://jellyfin.org/docs/general/server/media/shows/
- Jellyfin Metadata Provider Identifiers (supported providers + ID tags): https://jellyfin.org/docs/general/server/metadata/identifiers/
- Jellyfin Local .nfo metadata (priority + naming): https://jellyfin.org/docs/general/server/metadata/nfo/
- Jellyfin OpenSubtitles plugin doc: https://jellyfin.org/docs/general/server/plugins/open-subtitles/
- Jellyfin Container installation docs (volumes, rootless, GPU access patterns): https://jellyfin.org/docs/general/installation/container/
- TMDb image URL construction (configuration + sizes): https://developer.themoviedb.org/docs/image-basics
- Season artwork naming discussion (community evidence for seasonXX naming variations): https://forum.jellyfin.org/t-naming-season-artwork
- TV folder/season artwork naming variation discussion (community context): https://forum.jellyfin.org/t-tv-folder-structure-has-changed
- Missing episodes / empty season behaviors (issue tracker context): https://github.com/jellyfin/jellyfin/issues/12661
- Missing episodes setting context (older issue): https://github.com/jellyfin/jellyfin/issues/5125
- Provider mismatch pain point (why per-series provider selection matters): https://github.com/jellyfin/jellyfin/issues/13924
