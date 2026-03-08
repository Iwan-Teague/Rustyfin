# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, and calendar planning.

It combines a Rust backend (Axum + PostgreSQL), a Next.js UI, and a Docker-first runtime that works on one device or across your LAN.

## Recommended Host OS

The intended server host for Rustyfin is **Debian 12 (headless/minimal install)**.

- Recommended for stability, low overhead, and predictable Docker runtime behavior.
- A desktop environment is not required for normal server operation.

## Current Product Surface

- Libraries
  - Movie, TV, and music libraries with deep directory scanning.
  - Music library YouTube import workflow (download MP3 into artist/album folders with explicit metadata fields).
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
- Servers
  - New `Servers` area for native game server management.
  - Current implementation adds PostgreSQL-backed Minecraft instance records, event history, status refresh, native start/stop/restart controls, managed provisioning, existing-server import, journald log viewing, and discovery scans for host-side Minecraft directories through the Rust API/UI surface.
  - Rustyfin now renders native Debian 12 systemd units for Minecraft instances, can copy an existing host server directory into its managed instance path, and can route privileged host operations through a dedicated Rust `rustfin-servers-agent`.
- Rooms (formerly watch-party)
  - Watch Together: local media, YouTube embed, and shared web room.
  - Listen Together:
    - Unified mode with shared queue and playback.
    - Online search/download from YouTube plus offline local-library search in the same room.
  - Create Together:
    - Shared collaborative document editor (plain text / markdown / PDF-text workflow).
    - Shared paint-style canvas with synchronized strokes.
    - Import `.txt`, `.md`, `.pdf` (text extraction) and export `.txt`, `.md`, `.pdf`, `.png`.
  - Play Together:
    - Shared Chess, Connect Four, and Battleship games with synchronized state.
    - Chess supports local player seats (white/black), AI opponent mode, promotion choice, legal-move indicators, and board reset flows.
    - Connect Four supports red/yellow seats, turn-validated drops, win/draw detection, and reset confirmation flows.
    - Battleship supports blue/red seats, auto ship placement, ready-up flow, turn-validated firing, sink/win detection, and reset confirmation flows.
  - Room permissions, invites, password-protected rooms, and reconfiguration.
  - Empty room auto-cleanup after 5 minutes.
- Channels
  - Text channels with live updates and file/image attachments.
  - Voice channels with speaking indicators, mute, deafen, per-peer output volume, and local mic gain.
  - Voice channel transcription with per-speaker capture, Whisper-based transcription, stop/save, cancel, and markdown download.
  - User profile controls in channels UI (display name/avatar updates plus local audio device selection where browser support exists).
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
  - DB layer targets PostgreSQL for runtime.
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
  - Enforces transcription resource limits (parallel inference, worker caps, per-session worker caps) to prevent runaway CPU/RAM spikes in large calls.
  - GPU-only transcription path (no CPU fallback) with OpenCL default and optional CUDA/HIP backends.
  - If GPU backend requirements are not satisfied, transcription start/chunk requests are rejected with a clear GPU-required error.
- `rustfin-ui` (Next.js App Router frontend)
  - Browser client for all product areas.
- `rustfin-edge` (Caddy TLS edge)
  - HTTPS termination for LAN/browser secure-context needs (microphone/WebRTC).

Supporting host process:

- Native directory picker helper (started by `scripts/start.sh`)
  - Opens host OS folder picker for library path selection.
- Optional native `rustfin-servers-agent` (recommended for `Servers`)
  - Runs on the Debian host outside the main Rustyfin backend runtime.
  - Owns privileged Minecraft host operations: `systemctl`, `journalctl`, managed provisioning/import, and discovery scans.
  - The main Rustyfin API talks to it over an internal authenticated HTTP boundary using `RUSTFIN_SERVERS_AGENT_URL` and `RUSTFIN_SERVERS_AGENT_TOKEN`.

## Monorepo Layout

- `crates/core` - shared domain types and base error models.
- `crates/db` - schema migrations and repository layer.
- `crates/scanner` - media discovery and filename/path parsing.
- `crates/metadata` - metadata provider integration and merge logic.
- `crates/transcoder` - ffmpeg/ffprobe orchestration and HLS session logic.
- `crates/server` - main API server (auth, libraries, playback, rooms, channels, admin).
- `crates/server/src/servers` - game server management HTTP surface.
  - includes lifecycle orchestration, DB/audit/event handling, logs/discovery routes, and servers-agent client fallback logic for Minecraft.
- `crates/servers-host` - shared native Debian host runtime for Minecraft `systemd`, journald, provisioning, import, and discovery operations.
- `crates/servers-agent` - dedicated Rust agent exposing privileged Minecraft host operations over authenticated internal HTTP.
- `crates/calendar` - standalone calendar service API.
- `crates/youtube-agent` - standalone YouTube online-audio download/conversion API.
- `crates/transcription-agent` - standalone Whisper transcription API for channel voice capture.
- `ui` - Next.js frontend.
- `scripts` - operational scripts (`start.sh`, `stop.sh`, `clean_install.sh`, packaging helpers).
  - Shell-only scripts (`.sh`); PowerShell (`.ps1`) scripts are not part of this repository.
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
  - `RUSTFIN_NATIVE_RUST_BUILD_STRICT` defaults to `1` (strict): missing prerequisites fail startup.
  - Set `RUSTFIN_NATIVE_RUST_BUILD_STRICT=0` to allow automatic fallback to Docker Rust build mode.
- On Linux hosts, `start.sh` auto-enables onboard GPU passthrough for transcoding when `/dev/dri` exists.
  - It injects a compose overlay that maps `/dev/dri` into `rustfin` and sets `RUSTFIN_TRANSCODER_HW_ACCEL=auto`.
  - Auto-overlay also sets `RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL=1` by default.
  - Disable with `RUSTFIN_AUTO_HW_ACCEL=0`.
  - Force a specific mode with `RUSTFIN_TRANSCODER_HW_ACCEL` (`none`, `nvenc`, `vaapi`, `qsv`, `videotoolbox`).
- `--full-rebuild` forces no-cache rebuild.
- `--no-build` skips rebuild.
- Health checks in detached mode wait for critical services (`postgres`, `rustfin`, `rustfin-calendar`, `rustfin-tmdb-agent`, `rustfin-youtube-agent`, `rustfin-transcription-agent`, `rustfin-ui`, `rustfin-edge`) before final success output.
- If `RUSTFIN_YOUTUBE_COOKIE` is exported once, `start.sh` persists it to a local secrets file and auto-loads it on future runs.
- If `RUSTFIN_DATABASE_URL` is not set, `start.sh` defaults Docker runtime to:
  - `postgresql://<RUSTFIN_PG_USER>:<RUSTFIN_PG_PASSWORD>@postgres:5432/<RUSTFIN_PG_DB>`

## Access URLs and LAN Behavior

- Backend default host port: `8096` (HTTP)
- UI default host port: `3000` (HTTPS via edge)
- Backend host bind default: `127.0.0.1` (loopback-only by default)

`start.sh` auto-detects LAN IP, prints LAN URLs, and writes runtime values to `.rustyfin.runtime.env`.
If default ports are occupied, it picks free ports.

Rustyfin does not perform automatic router port mapping (UPnP/NAT-PMP).  
Remote access should be handled by your network/VPN layer.

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
- Library path browse/host directory listing is resolved on the server host through the directory-picker helper/API, not from browser-local filesystem paths.

## Key Environment Variables

Common runtime variables:

- `RUSTFIN_DATABASE_URL` (required PostgreSQL target; `postgres://` or `postgresql://`)
- `RUSTFIN_RUN_MIGRATIONS` (`true`/`false`; default `true` for backend)
- `RUSTFIN_CALENDAR_RUN_MIGRATIONS` (compose default: `false`)
- `RUSTFIN_TMDB_AGENT_RUN_MIGRATIONS` (compose default: `false`)
- `RUSTFIN_PG_USER` (compose default: `rustfin`)
- `RUSTFIN_PG_PASSWORD` (compose default: `rustfin`)
- `RUSTFIN_PG_DB` (compose default: `rustfin`)
- `RUSTFIN_NATIVE_RUST_BUILD` (`1` default; set `0` to compile Rust binaries inside Docker)
- `RUSTFIN_NATIVE_LINUX_TARGET` (optional Linux target triple override for native Rust cross-build)
- `RUSTFIN_NATIVE_RUST_BUILD_STRICT` (`1` default; set `0` to allow fallback when native prerequisites are missing)
- `RUSTFIN_NATIVE_GNU_COMPAT_BUILD` (`1` default; enforce Debian-compatible glibc target via zig for native Linux GNU builds)
- `RUSTFIN_NATIVE_GNU_GLIBC_VERSION` (`2.36` default; target glibc version for native GNU compatibility builds)
- `RUSTFIN_AUTO_HW_ACCEL` (`1` default; Linux-only auto `/dev/dri` passthrough)
- `RUSTFIN_TRANSCODER_HW_ACCEL` (`auto` default; `none|nvenc|vaapi|qsv|videotoolbox`)
- `RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL` (`0` in base compose; auto-overlay path sets `1` by default)
- `RUSTFIN_TRANSCRIPTION_GPU_MODE` (`opencl` default; `opencl|cuda|hip|auto`)
- `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU` (`1` default; rejects transcription when no usable GPU backend is available)
- `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES` (optional; `gpu-opencl` default in compose, can be set to `gpu-cuda` or `gpu-hip`)
- `RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS` (default `1800`)
- `RUSTFIN_STREAM_TOKEN_TTL_SECONDS` (default `21600`)
- `RUSTFIN_TEST_DATABASE_URL` (optional test DB target override for integration/E2E harness)
- `RUSTFIN_BACKEND_PORT`
- `RUSTFIN_SERVERS_SYSTEMCTL_BIN` (optional; defaults to `systemctl`)
- `RUSTFIN_SERVERS_SYSTEMD_UNIT_DIR` (optional; defaults to `/etc/systemd/system`)
- `RUSTFIN_SERVERS_ARTIFACT_CACHE_ROOT` (optional; defaults to `/var/cache/rustyfin-servers/minecraft/artifacts`)
- `RUSTFIN_SERVERS_IMPORT_ROOTS` (optional `:`-separated list of allowed import source roots; falls back to `RUSTFIN_DIRECTORY_BROWSE_ROOTS`)
- `RUSTFIN_SERVERS_SYSTEM_USER` / `RUSTFIN_SERVERS_SYSTEM_GROUP` (optional; written into rendered Minecraft systemd units when set)
- `RUSTFIN_BACKEND_BIND_IP` (default `127.0.0.1`)
- `RUSTFIN_UI_PORT`
- `RUSTFIN_PUBLIC_HOST`
- `RUSTFIN_MEDIA_PATH`
- `RUSTFIN_MEDIA_HOST_PATH`
- `RUSTFIN_MEDIA_CONTAINER_ROOT`
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
- `RUSTFIN_WHISPER_MODEL_PATH` (default `/cache/whisper/ggml-small.en.bin`)
- `RUSTFIN_WHISPER_MODEL_URL` (default `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin`)
- `RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES` (default `3`)
- `RUSTFIN_TRANSCRIPTION_MAX_WORKERS` (default `6`)
- `RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION` (default `8`)
- `RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER` (default `2`)
- `RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS` (default `2500`)
- `RUSTFIN_SECRETS_ENV_FILE`
- `RUSTFIN_DIRECTORY_PICKER_HELPER_URL`
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

Database note:

- Docker runtime is PostgreSQL-only via compose + `start.sh`.
- PostgreSQL migrations are in `crates/db/migrations_pg/`.
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

- `Play Together` supports Chess, Connect Four, and Battleship.
- Third-party website behavior in web rooms depends on iframe/embed policies.
- YouTube availability is subject to regional/content restrictions and provider-side policy.
- PDF import in Create Together is text-extraction based; scanned/image-only PDFs may not extract usable text.

## Documentation

See `/Users/iwanteague/Desktop/Rustyfin/docs/README.md` for the documentation index.

## License

MIT.
