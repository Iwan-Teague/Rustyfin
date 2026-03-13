# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, calendar planning, and native game server management.

The supported runtime target is native Debian 12. The repository no longer ships or supports Docker, Windows, or macOS runtime paths.

## Supported Host

- Debian 12 (Bookworm)
- Headless/minimal install is recommended
- `systemd` is required for the native service model

## Stack

- Rust backend and services
- PostgreSQL database
- Next.js UI
- Caddy HTTPS edge
- FFmpeg/ffprobe for media probing, HLS transcoding, original-file delivery, and stream analysis

## Product Areas

- Vault
  - Client-side encrypted password vault
  - Rust vault API and PostgreSQL ciphertext storage
  - Web `/vault` management UI
  - Vault page download flow for the browser extension package
  - Browser extension MVP for pairing, page detection, save prompts, and manual autofill
- Libraries
  - Movie, TV, and music libraries with recursive scanning
  - TMDB metadata enrichment and artwork sync
  - Library-level permissions
- Playback
  - HLS transcode sessions for the embedded video players
  - Resume state and Continue Watching
  - HTTP range/original-file delivery for raw media access and downloads
  - Playback progress tracking
- Rooms
  - Watch Together
  - Listen Together
  - Create Together
  - Play Together
    - Chess
    - Connect Four
    - Battleship
- Channels
  - Text channels with attachments
  - Voice channels with WebRTC audio
  - Whisper-based transcription
- Calendar
  - Shared and personal event planning
- Servers
  - Native game server management
  - Current implementation targets Minecraft on Debian 12 through `systemd`
  - Guided create/import wizard in the UI
  - Managed servers auto-provision when started
  - Only admins can create, import, and delete server records
- Admin
  - Users, libraries, channels, rooms, logs, TMDB configuration

## Runtime Services

Native Rustyfin on Debian 12 runs these services directly on the host:

- `rustfin` on `127.0.0.1:8096`
- `rustfin-calendar` on `127.0.0.1:8099`
- `rustfin-tmdb-agent` on `127.0.0.1:8100`
- `rustfin-youtube-agent` on `127.0.0.1:8101`
- `rustfin-transcription-agent` on `127.0.0.1:8102`
- `rustfin-servers-agent` on `127.0.0.1:8103`
- Next.js standalone UI on `127.0.0.1:3001`
- Caddy HTTPS edge on `:3000`
- PostgreSQL on `127.0.0.1:5432`

## Repository Layout

- `/Users/iwanteague/Desktop/Rustyfin/crates/core` - shared domain types and base errors
- `/Users/iwanteague/Desktop/Rustyfin/crates/db` - PostgreSQL migrations and repositories
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner` - library scanning and parsing
- `/Users/iwanteague/Desktop/Rustyfin/crates/metadata` - metadata merge/provider logic
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder` - ffmpeg/ffprobe orchestration
- `/Users/iwanteague/Desktop/Rustyfin/crates/server` - main API server
- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar` - calendar service
- `/Users/iwanteague/Desktop/Rustyfin/crates/tmdb-agent` - TMDB sync service
- `/Users/iwanteague/Desktop/Rustyfin/crates/youtube-agent` - YouTube audio service
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent` - transcription service
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-host` - native Minecraft host/runtime operations
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-agent` - privileged Minecraft host agent
- `/Users/iwanteague/Desktop/Rustyfin/ui` - Next.js frontend
- `/Users/iwanteague/Desktop/Rustyfin/extensions/rustfin-vault-webext` - browser extension MVP for Rustyfin Vault
- `/Users/iwanteague/Desktop/Rustyfin/scripts` - native install/start/stop/deploy/systemd scripts
- `/Users/iwanteague/Desktop/Rustyfin/tests` - tests and E2E harnesses
- `/Users/iwanteague/Desktop/Rustyfin/docs` - reports, plans, references, setup docs

## Native Debian Quick Start

Install host dependencies:

```bash
./scripts/install_native_debian.sh
```

Start Rustyfin:

```bash
./scripts/start-native.sh
```

Stop Rustyfin:

```bash
./scripts/stop-native.sh
```

Install boot-time `systemd` services:

```bash
./scripts/install_native_systemd.sh
```

Deploy updates on a Debian host:

```bash
./scripts/deploy-native.sh
```

Reset runtime state and database contents:

```bash
./scripts/clean_install.sh
```

Compatibility aliases:

- `./scripts/start.sh` delegates to `./scripts/start-native.sh`
- `./scripts/stop.sh` delegates to `./scripts/stop-native.sh`

Detailed native operations guide:

- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`

## Native Runtime Notes

- `./scripts/start-native.sh` builds Rust services directly on the Debian host
- It also builds the Next.js UI directly on the host and runs the standalone server natively
- Runtime values are written to:
  - `/Users/iwanteague/Desktop/Rustyfin/.rustyfin.runtime.env`
- Native logs and pid files live under:
  - `/Users/iwanteague/Desktop/Rustyfin/.tmp/native-runtime/`
- For persistent service startup after reboot, install:
  - `./scripts/install_native_systemd.sh`
- After systemd services are installed, use:
  - `./scripts/deploy-native.sh`
  - not a raw `systemctl restart`, because deploy also rebuilds artifacts before restart
- The main `rustyfin-native.service` now runs under a lightweight native supervisor script so `systemd` can detect child-process failure and restart the stack if required
- A separate `rustyfin-post-healthcheck.service` now runs after startup to verify backend/UI/agent readiness and recover from half-ready boots

## Access URLs

- HTTPS UI edge: `https://<host>:3000`
- Backend API: `http://127.0.0.1:8096`

Rustyfin does not manage router port forwarding. Remote access should be handled by your VPN or network layer.

## Setup and Authentication

- First run goes through `/setup`
- The setup wizard creates the first admin account
- Setup then closes and normal login becomes the entry path
- Protected UI routes redirect unauthenticated users to `/login`

## Media and Playback Notes

- `ffmpeg` and `ffprobe` are required
- Listen Together online audio uses `rustfin-youtube-agent`
- Some YouTube media cannot be embedded or downloaded because provider restrictions still apply
- Library browsing and directory selection always resolve on the server host, not on the browser client

## Key Environment Variables

Core runtime:

- `RUSTFIN_DATABASE_URL` - PostgreSQL target, required for non-default DB wiring
- `RUSTFIN_RUN_MIGRATIONS` - backend migration authority flag
- `RUSTFIN_BACKEND_PORT`
- `RUSTFIN_BACKEND_BIND_IP`
- `RUSTFIN_UI_PORT`
- `RUSTFIN_PUBLIC_HOST`
- `RUSTFIN_MEDIA_PATH`
- `RUSTFIN_DIRECTORY_BROWSE_ROOTS`
- `RUSTFIN_WS_ALLOWED_ORIGINS`
- `RUSTFIN_CACHE_DIR`
- `RUSTFIN_TRANSCODE_DIR`

Playback and transcoding:

- `RUSTFIN_FFMPEG_PATH`
- `RUSTFIN_FFPROBE_PATH`
- `RUSTFIN_MAX_TRANSCODES`
- `RUSTFIN_TRANSCODER_HW_ACCEL` - `auto|none|nvenc|vaapi|qsv|videotoolbox`
- `RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL`
- `RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS`
- `RUSTFIN_STREAM_TOKEN_TTL_SECONDS`

TMDB:

- `RUSTFIN_TMDB_KEY`
- `RUSTFIN_TMDB_AGENT_URL`
- `RUSTFIN_TMDB_AGENT_TOKEN`

YouTube agent:

- `RUSTFIN_YOUTUBE_COOKIE`
- `RUSTFIN_YOUTUBE_COOKIE_FILE`
- `RUSTFIN_YOUTUBE_AGENT_URL`
- `RUSTFIN_YOUTUBE_AGENT_TOKEN`
- `RUSTFIN_YTDLP_PATH`

Transcription:

- `RUSTFIN_TRANSCRIPTION_AGENT_URL`
- `RUSTFIN_TRANSCRIPTION_AGENT_TOKEN`
- `RUSTFIN_WHISPER_MODEL_PATH`
- `RUSTFIN_WHISPER_MODEL_URL`
- `RUSTFIN_TRANSCRIPTION_GPU_MODE`
- `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU`
- `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES`
- `RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES`
- `RUSTFIN_TRANSCRIPTION_MAX_WORKERS`
- `RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION`
- `RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER`
- `RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS`

Servers:

- `RUSTFIN_ENABLE_SERVERS_AGENT`
- `RUSTFIN_SERVERS_AGENT_URL`
- `RUSTFIN_SERVERS_AGENT_TOKEN`
- `RUSTFIN_SERVERS_AGENT_PORT`
- `RUSTFIN_SERVERS_DEFAULT_JAVA`
- `RUSTFIN_SERVERS_SYSTEMCTL_BIN`
- `RUSTFIN_SERVERS_SYSTEMD_UNIT_DIR`
- `RUSTFIN_SERVERS_ARTIFACT_CACHE_ROOT`
- `RUSTFIN_SERVERS_IMPORT_ROOTS`
- `RUSTFIN_SERVERS_SYSTEM_USER`
- `RUSTFIN_SERVERS_SYSTEM_GROUP`

Secrets/runtime support:

- `RUSTFIN_SECRETS_ENV_FILE`
- `RUSTFIN_DIRECTORY_PICKER_HELPER_URL`
- `RUSTFIN_TEST_DATABASE_URL`

## Build and Test

Debian 12 native quality gates:

```bash
./scripts/ci/debian_native_gates.sh
```

This is the main post-update confidence check for the supported runtime. It emits:

- a Markdown report under `/Users/iwanteague/Desktop/Rustyfin/.tmp/gates/`
- per-gate logs under the same run directory
- a latest report copy at:
  - `/Users/iwanteague/Desktop/Rustyfin/.tmp/gates/debian-native-gates-latest.md`

It now also includes:

- an isolated browser smoke pass for setup/login, channels, rooms, servers, and playback
- a live unauthenticated-access gate for representative protected API routes

Run the browser smoke independently if needed:

```bash
./scripts/ci/debian_browser_smoke.sh
```

Rust:

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

Database:

- PostgreSQL is the only supported runtime database
- Migrations are in:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/`

## UI Animation Standards

- Primary/save/create actions use the shared `.btn-primary` styling and `PrimaryButtonEffects`
- Delete actions use the shared fade-out path from:
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/deleteAnimation.ts`
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/globals.css`

## Historical Notes

Some archived planning/reference documents under `/Users/iwanteague/Desktop/Rustyfin/docs/` still discuss earlier design phases. Current operational guidance is limited to the Debian 12 native runtime documented in this file, `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`, and `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`.
