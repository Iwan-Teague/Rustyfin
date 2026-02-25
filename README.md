# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, and calendar planning.

It combines a Rust backend (Axum + SQLite), a Next.js UI, and a Docker-first runtime that works on one device or across your LAN.

## Current Product Surface

- Libraries
  - Movie, TV, and music libraries with deep directory scanning.
  - Per-library access control per user.
  - TMDB metadata enrichment (configurable in Admin).
  - Per-library TMDB management:
    - Guided movie-vs-TV matching by library kind.
    - Toggle poster/backdrop/metadata/reviews fetching.
    - Optional artwork storage directly in media folders.
    - Auto TMDB sync on newly detected media.
    - Scheduled TMDB sync (`hourly`, `daily`, `weekly`, `monthly`, or `manual`).
  - Optional dedicated TMDB sync agent (separate Rust container) for on-demand poster download.
- Playback
  - Direct Play (HTTP range) and HLS transcode modes.
  - Playback progress tracking.
  - Secure stream token handling for scoped streaming URLs.
- Rooms (formerly watch-party)
  - Watch Together: local media, YouTube embed, and shared web room.
  - Listen Together:
    - Local mode (music library based).
    - Online mode: YouTube search, room-scoped audio download to MP3, shared queue and playback.
  - Create Together:
    - Shared collaborative document editor (plain text / markdown / PDF-text workflow).
    - Shared paint-style canvas with synchronized strokes.
    - Import `.txt`, `.md`, `.pdf` (text extraction) and export `.txt`, `.md`, `.pdf`, `.png`.
  - Room permissions, invites, password-protected rooms, and reconfiguration.
  - Empty room auto-cleanup after 5 minutes.
- Channels
  - Text channels with live updates and file/image attachments.
  - Voice channels with speaking indicators, mute, deafen, per-peer output volume, and local mic gain.
- Calendar
  - Separate Rust calendar service with admin-global and user-personal events.
  - Recurring birthdays and multiple calendar views in UI.
- Admin Console
  - Tabbed management for users, libraries, channels, rooms, logs, and TMDB key configuration.

## Runtime Architecture

Rustyfin runs as a multi-service stack in Docker Compose:

- `rustfin` (Rust backend API)
  - Axum REST/WebSocket server.
  - SQLite-backed core application logic.
- `rustfin-calendar` (Rust calendar API)
  - Dedicated calendar microservice.
  - Shares the same SQLite database volume.
- `rustfin-tmdb-agent` (Rust TMDB sync API)
  - Dedicated metadata/poster sync microservice.
  - Scans indexed items in a library, resolves TMDB matches, downloads posters to shared cache, updates DB artwork paths.
- `rustfin-ui` (Next.js App Router frontend)
  - Browser client for all product areas.
- `rustfin-edge` (Caddy TLS edge)
  - HTTPS termination for LAN/browser secure-context needs (microphone/WebRTC).

Supporting host process:

- Native directory picker helper (started by `scripts/start.sh`)
  - Opens host OS folder picker for library path selection.

## Monorepo Layout

- `crates/core` - shared domain types and base error models.
- `crates/db` - schema migrations and repository layer.
- `crates/scanner` - media discovery and filename/path parsing.
- `crates/metadata` - metadata provider integration and merge logic.
- `crates/transcoder` - ffmpeg/ffprobe orchestration and HLS session logic.
- `crates/server` - main API server (auth, libraries, playback, rooms, channels, admin).
- `crates/calendar` - standalone calendar service API.
- `ui` - Next.js frontend.
- `scripts` - operational scripts (`start.sh`, `stop.sh`, `clean_install.sh`, packaging helpers).
- `tests` - test harness and E2E suites.
- `docs` - reports, plans, references, and setup wizard artifacts.

## Quick Start (Docker)

From repo root:

```bash
./scripts/start.sh
```

Stop stack:

```bash
./scripts/stop.sh
```

Reset to blank-slate install (wipes user/runtime data):

```bash
./scripts/clean_install.sh
```

After `clean_install.sh`, next `start.sh` requires full setup wizard again.

### `start.sh` options

```bash
./scripts/start.sh [--no-build|--full-rebuild] [--foreground] [--no-health-check] [-f docker-compose.yml]
```

- Default behavior performs a smart incremental rebuild:
  - It fingerprints source/build inputs per service.
  - Only changed services are rebuilt (`rustfin`, `rustfin-calendar`, `rustfin-tmdb-agent`, `rustfin-ui`).
  - If nothing changed, it skips image rebuild and reuses existing images.
- `--full-rebuild` forces no-cache rebuild.
- `--no-build` skips rebuild.
- If `RUSTFIN_YOUTUBE_COOKIE` is exported once, `start.sh` persists it to a local secrets file and auto-loads it on future runs.

## Access URLs and LAN Behavior

- Backend default host port: `8096` (HTTP)
- UI default host port: `3000` (HTTPS via edge)

`start.sh` auto-detects LAN IP, prints LAN URLs, and writes runtime values to `.rustyfin.runtime.env`.
If default ports are occupied, it picks free ports.

## Setup and Authentication

- First run requires setup wizard (`/setup`).
- Setup creates the first admin account and marks setup complete.
- Password minimum length is 6 characters.
- If setup is complete, setup pages are no longer the normal entry path.

## Media and Playback Notes

- ffmpeg/ffprobe are required for transcode and media probing.
- Room online-audio mode also depends on ffmpeg for conversion.
- Some YouTube videos cannot be embedded or downloaded due to provider restrictions.

## Key Environment Variables

Common runtime variables:

- `RUSTFIN_BACKEND_PORT`
- `RUSTFIN_UI_PORT`
- `RUSTFIN_PUBLIC_HOST`
- `RUSTFIN_MEDIA_PATH`
- `RUSTFIN_TMDB_KEY`
- `RUSTFIN_TMDB_AGENT_URL`
- `RUSTFIN_TMDB_AGENT_TOKEN`
- `RUSTFIN_YOUTUBE_COOKIE`
- `RUSTFIN_SECRETS_ENV_FILE`
- `RUSTFIN_WS_ALLOWED_ORIGINS`
- `RUSTFIN_FFMPEG_PATH`
- `RUSTFIN_FFPROBE_PATH`
- `RUSTFIN_MAX_TRANSCODES`
- `RUSTFIN_CACHE_DIR`
- `RUSTFIN_TRANSCODE_DIR`

## Build and Test

Rust workspace:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

UI:

```bash
npm --prefix ui run lint
npm --prefix ui run build
```

E2E harness:

```bash
./tests/test-all.sh
# or
./tests/run-suite.sh 00_smoke
```

## Known Limitations

- `Play Together` room creation UI is present, but room creation is currently marked as coming soon.
- Third-party website behavior in web rooms depends on iframe/embed policies.
- YouTube availability is subject to regional/content restrictions and provider-side policy.
- PDF import in Create Together is text-extraction based; scanned/image-only PDFs may not extract usable text.

## Documentation

See `/Users/iwanteague/Desktop/Rustyfin/docs/README.md` for the documentation index.

## License

MIT.
