# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, calendar planning, and native game server management.

The supported runtime target is native Debian 12 and Debian 13. The repository no longer ships or supports Docker, Windows, or macOS runtime paths.

## Supported Host

- Debian 12 (Bookworm)
- Debian 13 (Trixie)
- Headless/minimal install is recommended
- `systemd` is required for the native service model

## Stack

- Rust backend and services
- Rust-first Linux installer flow via `crates/installer`
- PostgreSQL database
- Next.js UI
- Caddy HTTPS edge
- FFmpeg/ffprobe for media probing, HLS transcoding, original-file delivery, and stream analysis

## Product Areas

- Vault
  - Client-side encrypted password vault hosted inside Rustyfin
  - Backed by the dedicated `crates/rustyvault` product crate and `ui/src/features/rustyvault`
  - Shared RustyVault types now live in `crates/rustyvault/src/types.rs`
  - RustyVault preference normalization is now owned by the canonical `rustyvault::types::RustyVaultPreferences` model
  - Canonical UI imports are feature-scoped under `ui/src/features/rustyvault`; generic `ui/src/lib/vault*` shims and the old `ui/src/lib/vaultGenerator.ts` path have been removed
  - Canonical persistence now lives in `crates/db/src/repo/rustyvault.rs`, and PostgreSQL schema ownership now includes dedicated RustyVault preferences storage via `crates/db/migrations_pg/040_rustyvault_preferences.sql`
  - RustyVault session/auth internals now use `x-rustyvault-*` headers, the `rustyvault_session` JWT audience, and host-scoped validation in `crates/server/src/rustyvault_host/auth.rs`
  - RustyVault settings now flow through the dedicated `/api/v1/vault/preferences` host adapter backed by `rustyvault_preference`, not the generic account-preferences JSON blob
  - RustyVault state reads and management views now default to the RustyVault session boundary; routes like `/api/v1/vault/config`, `/api/v1/vault/preferences`, `/api/v1/vault/device-sessions`, and `/api/v1/vault/audit` no longer rely on plain Rustyfin auth alone
  - Vault bootstrap, protected-action challenge, and revoke-other-device-session flows are now session-bound as well, and mixed Rustyfin-auth plus RustyVault-session routes reject cross-user token mixing explicitly
  - Runtime availability is graceful: if RustyVault is disabled or its schema is unavailable, `/vault` returns `503` and the rest of Rustyfin stays functional
  - Host-facing RustyVault routes are being audited down to live web UI and extension operations; dead convenience routes like `/api/v1/vault/sync` and `/api/v1/vault/protected-actions/complete` have been removed
  - Web `/vault` management UI remains the host-facing page
  - Browser extension MVP for pairing, page detection, save prompts, and manual autofill
  - The generic `/users/me/preferences` host API no longer carries Vault settings
  - Backend can be compiled without RustyVault via `cargo check -p rustfin-server --no-default-features`
  - Runtime availability can also be forced off with `RUSTFIN_RUSTYVAULT_ENABLED=0`, and the UI host fallback can be forced with `NEXT_PUBLIC_RUSTYVAULT_ENABLED=0`
- Downloads
  - Web `/downloads` release surface for official Rustyfin packages
  - Backed by a host-owned downloads catalog and artifact pipeline at `/api/v1/downloads/catalog`
  - Current implementation exposes the RustyVault browser extension package through `/api/v1/downloads/artifacts/rustyvault-webext/package`
  - The Downloads host route is the authoritative public package-delivery surface for first-party artifacts
  - Future first-party applications and companion downloads can land here without moving existing links
- AI
  - Web `/ai` assistant surface backed by the native Rust `crates/ai-agent` integration
  - End-user `/ai` is chat-focused; model downloads, deletion, and storage-folder management are admin-only through the Admin `AI` tab
  - `/api/v1/ai/chat` now has an initial grounded read-only assistant path backed by server-side Rust modules in `crates/server/src/ai_assistant`
  - `/api/v1/ai/chat` now uses a model-assisted structured planner for grounded tool selection, with strict registry/role validation plus deterministic fallback and deterministic entity-follow-up resolution
  - Current grounded domains are account summary, visible calendar events/birthdays, authenticated downloads catalog entries, accessible libraries, library title search, active public rooms, admin-only host runtime stats, admin-only constrained public web search/page summary when enabled, and accessible Minecraft server status
  - Query understanding now supports calendar windows, room-mode filtering such as YouTube or screen-share rooms, admin-only host-runtime prompts such as RAM/CPU/load/uptime questions, explicit public URLs, constrained public-web weather/current-info prompts when enabled, and Minecraft server status/name filtering such as online, offline, healthy, or a named server
  - `/api/v1/ai/chat` now streams tool-status events before the model answer so the `/ai` page can show lightweight progress like checking calendar, rooms, or server state
  - Short follow-up prompts can now reuse the last grounded tool context as a planner hint, but the backend still reruns fresh auth-scoped tools instead of trusting client-sent data
  - The assistant can now resolve simple entity references like `the second one`, `that server`, or `the first room` from the last grounded result set and then rerun a fresh scoped detail tool for that entity instead of only narrowing the prior list query
  - Grounded AI chat now emits traceable tool logs and assistant chat/tool counters into the admin runtime diagnostics surface, and recent assistant requests are durably persisted for review in the Admin `AI` tab
  - Assistant audit history now defaults to 30-day retention with hourly pruning; override with `RUSTFIN_AI_AUDIT_RETENTION_DAYS`
  - Constrained public web tools are disabled by default and require `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`; they are admin-only and use backend fetch restrictions instead of giving the model direct internet access
  - Grounding remains server-side and read-only; the assistant does not execute product writes, and the tool registry policy is enforced at execution time rather than treated as documentation only
  - Native host builds now select a host-safe AI backend automatically instead of assuming CUDA is present
  - On unsupported hosts, `auto` can fall back to AI being disabled so the rest of Rustyfin still runs
  - Use `RUSTFIN_AI_GPU_BACKEND=auto|disabled|cpu|cuda|rocm|vulkan` to control the server-side AI inference backend chosen at build time
  - AI models are resolved from the admin-managed `ai_model_dir` setting, then `RUSTFIN_AI_MODEL_DIR`, then the Rustyfin AI default path `/var/lib/rustyfin/ai/models`
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
  - Current implementation targets Minecraft on supported Debian hosts through `systemd`
  - Guided create/import wizard in the UI
  - Managed servers auto-provision when started
  - Only admins can create, import, and delete server records
- Admin
  - Users, libraries, channels, rooms, logs, TMDB configuration

## Runtime Services

Native Rustyfin on supported Debian hosts runs these services directly on the host:

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
- `/Users/iwanteague/Desktop/Rustyfin/crates/installer` - Rust-first Linux installer orchestration, currently delegating to the proven Debian-native flow
- `/Users/iwanteague/Desktop/Rustyfin/crates/scanner` - library scanning and parsing
- `/Users/iwanteague/Desktop/Rustyfin/crates/metadata` - metadata merge/provider logic
- `/Users/iwanteague/Desktop/Rustyfin/crates/rustyvault` - RustyVault product logic, shared types, and extension packaging, mounted into Rustyfin through host adapters
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder` - ffmpeg/ffprobe orchestration
- `/Users/iwanteague/Desktop/Rustyfin/crates/server` - main API server
- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar` - calendar service
- `/Users/iwanteague/Desktop/Rustyfin/crates/tmdb-agent` - TMDB sync service
- `/Users/iwanteague/Desktop/Rustyfin/crates/youtube-agent` - YouTube audio service
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent` - transcription service
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-host` - native Minecraft host/runtime operations
- `/Users/iwanteague/Desktop/Rustyfin/crates/servers-agent` - privileged Minecraft host agent
- `/Users/iwanteague/Desktop/Rustyfin/ui` - Next.js frontend
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/rustyvault` - RustyVault frontend feature module mounted by the host `/vault` route
- `/Users/iwanteague/Desktop/Rustyfin/extensions/rustyvault-webext` - browser extension MVP for RustyVault
- `/Users/iwanteague/Desktop/Rustyfin/scripts` - native install/start/stop/deploy/systemd scripts
- `/Users/iwanteague/Desktop/Rustyfin/tests` - tests and E2E harnesses
- `/Users/iwanteague/Desktop/Rustyfin/docs` - current operations guides, active plans, architecture docs, and setup specs

## Native Debian Quick Start

Preferred one-shot Linux installer:

```bash
./scripts/install_linux.sh
```

This bootstraps Rust if needed and then hands off to `cargo run -p rustfin-installer`.
The current full native install flow behind that installer is implemented for Debian 12 and Debian 13.
The Rust installer now owns Debian prerequisite installation, native-user detection, Rust toolchain provisioning for the native runtime user, `yt-dlp`, PostgreSQL bootstrap, managed Java 21 provisioning, installer-written native runtime defaults at `/etc/rustyfin/native-runtime.defaults.sh`, native runtime planning for ports/media/DB/origins, runtime TLS/token/snapshot persistence, native Linux binary build orchestration, native runtime artifact builds for Rust services plus the Next standalone UI, native runtime launch/stop orchestration, native clean-reset behavior, native deploy orchestration, direct `systemd` install/refresh, install-manifest output, and post-install `systemd` runtime validation with captured diagnostics if startup fails.
The public native scripts now act as compatibility wrappers around `rustfin-installer` subcommands.

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
- `./scripts/start.sh` ignores legacy Docker-era flags for backward compatibility and continues with native startup
- `./scripts/stop.sh` delegates to `./scripts/stop-native.sh`

Detailed native operations guide:

- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`

## Native Runtime Notes

- `./scripts/start-native.sh` is now a thin compatibility wrapper
- It loads the native env/default layers, drives runtime planning and artifact builds, and then hands off launch/health to `./scripts/rustfin-installer.sh launch-native-runtime`
- Installer-owned native runtime defaults are written to:
  - `/etc/rustyfin/native-runtime.defaults.sh`
- Runtime planning for ports, media path, DB URL, and browser/websocket origins is now emitted by:
  - `./scripts/rustfin-installer.sh plan-native-runtime`
- Native artifact builds for Rust services plus the Next standalone UI are now emitted by:
  - `./scripts/rustfin-installer.sh build-native-runtime-artifacts`
- Native deploy sequencing is now emitted by:
  - `./scripts/rustfin-installer.sh deploy-native`
- Runtime TLS material, service tokens, and the persisted runtime snapshot are now written by:
  - `./scripts/rustfin-installer.sh plan-native-runtime`
  - `./scripts/rustfin-installer.sh write-native-runtime-snapshot`
- Native runtime launch/stop/reset are now emitted by:
  - `./scripts/rustfin-installer.sh launch-native-runtime`
  - `./scripts/rustfin-installer.sh stop-native-runtime`
  - `./scripts/rustfin-installer.sh clean-native-runtime`
- Native systemd install/refresh is now emitted by:
  - `./scripts/rustfin-installer.sh install-native-systemd`
- Native installer-driven `systemd` setup now validates that the backend, agents, and HTTPS UI actually come up before reporting success.
- If that validation fails, the installer captures `systemctl status` output plus native log tails so fresh-host failures are diagnosable without manual digging.
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
- `RUSTFIN_AI_GPU_BACKEND` - native AI inference backend selection for host builds (`auto|disabled|cpu|cuda|rocm|vulkan`)

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

Supported-Debian native quality gates:

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

Focused RustyVault removability gate:

```bash
./scripts/ci/rustyvault_removability_gates.sh
```

This verifies the host can degrade the Vault surface cleanly while unrelated routes still respond, the backend still compiles without the `rustyvault` feature, and the host UI still builds with `NEXT_PUBLIC_RUSTYVAULT_ENABLED=0`.

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

Some archived planning/reference documents under `/Users/iwanteague/Desktop/Rustyfin/docs/` still discuss earlier design phases. Current operational guidance is limited to the supported-Debian native runtime documented in this file, `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`, and `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`.
