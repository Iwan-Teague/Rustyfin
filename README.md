# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, and calendar planning.

It combines a Rust backend (Axum + SQLite), a Next.js UI, and a Docker-first runtime that works on one device or across your LAN.

## Recommended Host OS

The intended server host for Rustyfin is **Debian 12 (headless/minimal install)**.

- Recommended for stability, low overhead, and predictable Docker runtime behavior.
- A desktop environment is not required for normal server operation.

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
    - Unified mode with shared queue and playback.
    - Online search/download from YouTube plus offline local-library search in the same room.
  - Create Together:
    - Shared collaborative document editor (plain text / markdown / PDF-text workflow).
    - Shared paint-style canvas with synchronized strokes.
    - Import `.txt`, `.md`, `.pdf` (text extraction) and export `.txt`, `.md`, `.pdf`, `.png`.
  - Room permissions, invites, password-protected rooms, and reconfiguration.
  - Empty room auto-cleanup after 5 minutes.
- Channels
  - Text channels with live updates and file/image attachments.
  - Voice channels with speaking indicators, mute, deafen, per-peer output volume, and local mic gain.
  - Voice channel transcription with per-speaker capture, Whisper-based transcription, stop/save, cancel, and markdown download.
- Calendar
  - Separate Rust calendar service with admin-global and user-personal events.
  - Recurring birthdays and multiple calendar views in UI.
- Admin Console
  - Tabbed management for users, libraries, channels, rooms, logs, and TMDB key configuration.

## Runtime Architecture

Rustyfin runs as a multi-service stack in Docker Compose:

- `postgres` (PostgreSQL database)
  - Default DB backend for Docker runtime (`start.sh` default).
  - Persistent data volume managed by compose.
- `rustfin` (Rust backend API)
  - Axum REST/WebSocket server.
  - DB-backend abstraction via SQLx AnyPool (SQLite and PostgreSQL).
  - Default migration authority (`RUSTFIN_RUN_MIGRATIONS=true`).
- `rustfin-calendar` (Rust calendar API)
  - Dedicated calendar microservice.
  - Uses shared DB target; migrations disabled by default in compose.
- `rustfin-tmdb-agent` (Rust TMDB sync API)
  - Dedicated metadata/poster sync microservice.
  - Scans indexed items in a library, resolves TMDB matches, downloads posters to shared cache, updates DB artwork paths.
  - Uses shared DB target; migrations disabled by default in compose.
- `rustfin-youtube-agent` (Rust YouTube download API)
  - Dedicated online-audio fetch/convert service for Listen Together.
  - Downloads YouTube source audio with multi-strategy fallback, converts to MP3, writes room-scoped files under shared cache.
- `rustfin-transcription-agent` (Rust Whisper transcription API)
  - Dedicated voice transcription microservice for channels.
  - Lazily loads/downloads Whisper model, runs per-session-per-user worker contexts, and returns timestamped transcript segments.
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
- `crates/youtube-agent` - standalone YouTube online-audio download/conversion API.
- `crates/transcription-agent` - standalone Whisper transcription API for channel voice capture.
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
./scripts/start.sh [--no-build|--full-rebuild] [--foreground] [--no-health-check] [--youtube-cookie "<cookie>"] [--native-rust-build|--docker-rust-build] [-f docker-compose.yml]
```

- Default behavior performs a smart incremental rebuild:
  - It fingerprints source/build inputs per service.
  - Only changed services are rebuilt (`rustfin`, `rustfin-calendar`, `rustfin-tmdb-agent`, `rustfin-transcription-agent`, `rustfin-youtube-agent`, `rustfin-ui`).
  - If nothing changed, it skips image rebuild and reuses existing images.
  - Compose config impact is hashed per service (`docker compose config --hash <service>`), so unrelated compose edits do not invalidate all service fingerprints.
- Rust build profile defaults to `dev` for faster local builds.
  - Set `RUSTFIN_RUST_BUILD_PROFILE=release` when you need optimized binaries.
- Rust service binaries default to native host cross-compilation, then Docker runtime images copy those prebuilt binaries.
  - This avoids repeated Rust compilation inside Docker build stages.
  - Use `--docker-rust-build` (or `RUSTFIN_NATIVE_RUST_BUILD=0`) to force old Docker builder-stage behavior.
  - Native cross-build prerequisites on non-Linux hosts:
    - `zig`
    - `cargo-zigbuild` (`cargo install cargo-zigbuild --locked`)
  - If prerequisites are missing, `start.sh` automatically falls back to Docker Rust build mode (unless `RUSTFIN_NATIVE_RUST_BUILD_STRICT=1`).
- `--full-rebuild` forces no-cache rebuild.
- `--no-build` skips rebuild.
- Health checks in detached mode wait for critical services (`postgres`, `rustfin`, `rustfin-calendar`, `rustfin-tmdb-agent`, `rustfin-youtube-agent`, `rustfin-transcription-agent`, `rustfin-ui`, `rustfin-edge`) before final success output.
- If `RUSTFIN_YOUTUBE_COOKIE` is exported once, `start.sh` persists it to a local secrets file and auto-loads it on future runs.
- If `RUSTFIN_DATABASE_URL` is not set, `start.sh` defaults Docker runtime to:
  - `postgresql://<RUSTFIN_PG_USER>:<RUSTFIN_PG_PASSWORD>@postgres:5432/<RUSTFIN_PG_DB>`

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
- Listen Together online downloads use `rustfin-youtube-agent` with `yt-dlp` fallback and a JavaScript runtime (`node`) for modern YouTube signature handling.
- Some YouTube videos cannot be embedded or downloaded due to provider restrictions.

## Key Environment Variables

Common runtime variables:

- `RUSTFIN_DATABASE_URL` (preferred database target; accepts `sqlite:` URLs/paths and `postgres://` URLs)
- `RUSTFIN_DB` (legacy SQLite DB path fallback)
- `RUSTFIN_RUN_MIGRATIONS` (`true`/`false`; default `true` for backend)
- `RUSTFIN_CALENDAR_RUN_MIGRATIONS` (compose default: `false`)
- `RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS` (compose default: `false`)
- `RUSTFIN_PG_USER` (compose default: `rustfin`)
- `RUSTFIN_PG_PASSWORD` (compose default: `rustfin`)
- `RUSTFIN_PG_DB` (compose default: `rustfin`)
- `RUSTFIN_NATIVE_RUST_BUILD` (`1` default; set `0` to compile Rust binaries inside Docker)
- `RUSTFIN_NATIVE_LINUX_TARGET` (optional Linux target triple override for native Rust cross-build)
- `RUSTFIN_NATIVE_RUST_BUILD_STRICT` (`0` default; set `1` to fail instead of fallback when native prerequisites are missing)
- `RUSTFIN_TEST_DATABASE_URL` (optional test DB target override for integration/E2E harness)
- `RUSTFIN_BACKEND_PORT`
- `RUSTFIN_UI_PORT`
- `RUSTFIN_PUBLIC_HOST`
- `RUSTFIN_MEDIA_PATH`
- `RUSTFIN_TMDB_KEY`
- `RUSTFIN_TMDB_AGENT_URL`
- `RUSTFIN_TMDB_AGENT_TOKEN`
- `RUSTFIN_YOUTUBE_COOKIE`
- `RUSTFIN_YOUTUBE_COOKIE_FILE`
- `RUSTFIN_YOUTUBE_AGENT_URL`
- `RUSTFIN_YOUTUBE_AGENT_TOKEN`
- `RUSTFIN_YTDLP_PATH`
- `RUSTFIN_TRANSCRIPTION_AGENT_URL`
- `RUSTFIN_TRANSCRIPTION_AGENT_TOKEN`
- `RUSTFIN_WHISPER_MODEL_PATH`
- `RUSTFIN_WHISPER_MODEL_URL`
- `RUSTFIN_SECRETS_ENV_FILE`
- `RUSTFIN_WS_ALLOWED_ORIGINS`
- `RUSTFIN_FFMPEG_PATH`
- `RUSTFIN_FFPROBE_PATH`
- `RUSTFIN_MAX_TRANSCODES`
- `RUSTFIN_CACHE_DIR`
- `RUSTFIN_TRANSCODE_DIR`

## Build and Test

Rust toolchain policy:

- The repository is pinned to stable Rust via `/Users/iwanteague/Desktop/Rustyfin/rust-toolchain.toml`.
- Docker Rust builder images use `rust:bookworm` (stable channel image).

Rust workspace:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

Database transition note:

- `crates/db/migrations/` remains the active SQLite migration set.
- `crates/db/migrations_pg/` is the PostgreSQL migration track introduced for transition work.
- Docker runtime now defaults to PostgreSQL via compose + `start.sh`.
- SQLite remains supported for legacy/dev targets via explicit `RUSTFIN_DATABASE_URL` or `RUSTFIN_DB`.
- SQLite-to-PostgreSQL migration helpers:
  - `scripts/db/migrate_sqlite_to_postgres.sh`
  - `scripts/db/validate_sqlite_postgres_counts.sh`
- Operational cutover document:
  - `docs/reports/postgres-cutover-runbook.md`

UI:

```bash
npm --prefix ui run lint
npm --prefix ui run build
```

## UI Animation Standards

To keep interaction behavior consistent across pages:

- Primary/save/create actions:
  - Use `.btn-primary` buttons for primary actions.
  - Click feedback is standardized via `ui/src/app/components/PrimaryButtonEffects.tsx` and CSS in `ui/src/app/globals.css` (`.btn-click-burst`).
  - Do not add one-off custom save/click animations for individual pages/components.
- Delete actions (messages, channels, transcripts, queues, admin records, calendar events):
  - Use `playTelegramDeleteAnimation` from `ui/src/lib/deleteAnimation.ts`.
  - Resolve DOM targets with `findDataDeleteTarget` (or equivalent explicit delete target node lookup).
  - Keep the shared delete animation defined in `ui/src/app/globals.css` (`.tg-delete-target.tg-delete-out` + `tg-delete-fade-out`) as the single delete animation style.

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
