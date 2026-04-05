# Rustyfin

Rustyfin is a local-first home server platform for media playback, live rooms, channels, calendar planning, and native game server management.

The supported native Linux install flow is implemented for Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04. The repository no longer ships or supports Docker, Windows, or macOS runtime paths.

## Supported Linux Install Hosts

- Debian 12 (Bookworm)
- Debian 13 (Trixie)
- Ubuntu 22.04 (Jammy)
- Ubuntu 24.04 (Noble)
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
  - Planned first-party client artifacts now explicitly include Windows, macOS, Linux, Android APK, and iOS distribution paths
- Backups
  - Web `/backups` now focuses on user-scoped archive exports instead of host restore controls
  - Current `Accounts` exports create a compressed archive of profile state, preferences, AI conversation history, playback progress, continue-watching data, and activity history
  - `/backups` can also embed an optional RustyVault export snapshot captured through the protected vault export flow
  - `Gallery` is reserved as the separate page section for future media/gallery-oriented backup flows
  - Host/system backup and restore routes remain operational surfaces under `/api/v1/system/backups`
- AI
  - Web `/ai` assistant surface backed by the native Rust `crates/ai-agent` integration
  - End-user `/ai` is chat-focused; model downloads, deletion, and storage-folder management are admin-only through the Admin `AI` tab
  - `/api/v1/ai/chat` and `/api/v1/ai/conversations/{id}/messages/stream` now back a grounded assistant path owned by server-side Rust modules in `crates/server/src/ai_assistant`
  - `/api/v1/ai/conversations` now provides per-user persisted conversation history plus a conversation-backed streaming route for `/ai`; `/api/v1/ai/chat` remains the compatibility path during rollout
  - `/api/v1/ai/chat` now uses a model-assisted structured planner for grounded tool selection, with strict registry/role validation plus deterministic fallback and deterministic entity-follow-up resolution
  - Admin `AI` now includes host-specific model benchmarking, profile recommendations, warm-model pool visibility, and optional remote planner backend configuration
  - The AI scheduler keeps the local inference path as the default, bounds queueing and warm-model pooling by host memory, and degrades predictably under overload instead of silently overcommitting the host
  - Grounded retrieval now persists compact ranked grounding chunks, typed memory items, and an entity graph in PostgreSQL so older but relevant facts can be recalled without stuffing raw tool payloads into prompts
  - Transcript summaries now keep citation backlinks to exact excerpts, and the Admin `AI` audit trail exposes the stored grounding chunks for review and debugging
  - `/ai` now renders a stored activity stack driven by server `phase` and `tool` events so the visible `Thinking...` state reflects structured backend progress rather than raw model-emitted chain-of-thought
  - Current grounded domains are account summary, visible calendar events/birthdays plus specific event details, confirmation-gated personal/shared calendar event creation and recurring birthday creation with read-after-write verification, calendar deletion with read-after-write verification, confirmation-gated AI conversation archive/delete/group-move actions scoped to the signed-in user with explicit title previews, confirmation-gated markdown/plain-text document generation with authenticated download links, recent visible channel activity, transcript-based summaries of accessible completed voice calls, authenticated downloads catalog entries, host-visible network topology and Rustyfin network settings, accessible libraries, library title search, recently added accessible library items, active public rooms, joinable rooms and invites, authenticated public weather via a fixed provider, authenticated AI runtime/model summary, admin-only host runtime stats, admin-only backup/service/transcode/storage/recent-error summaries, admin-only constrained public web search/page summary when enabled, and accessible Minecraft server status
  - Library search replies use compact server-authored summaries and grounded match lists instead of raw JSON payloads
  - Query understanding now supports calendar windows, deterministic `What is my next event?` prompts through `calendar_get_next_event`, broader next-up phrasing such as `What is the next thing coming up in my calendar?`, specific calendar-event detail prompts, named/self/next birthday lookups such as `When is Rachel's birthday?`, `When is my birthday?`, or `What's the next birthday in my calendar?`, confirmation-gated delete prompts such as `Delete dentist appointment on 2026-06-09 from my calendar`, confirmation-gated document prompts such as `Create a markdown document summarizing my next event`, explicit grounded capability/tool-inventory prompts such as `What functions do you have access to in this environment?`, AI runtime/model questions such as `What AI model is loaded right now?` or `What backend are you using?`, network questions such as interface/IP/hostname/remote-access prompts plus local-network connect prompts like `What IP should I use to connect to Rustyfin on my LAN?`, recent channel-activity prompts, transcript-summary prompts such as asking what a transcribed call was about, room-mode filtering such as YouTube or screen-share rooms, joinable-room and invite prompts, recently-added library prompts, authenticated public-weather prompts such as `What is the temperature in Dublin right now?`, `Will it rain in Galway today?`, `What is the weather like this week for Campile in County Wexford?`, and recent-history prompts such as `Did it rain yesterday in Galway?`, admin-only host-runtime/service-health/storage/transcode/recent-error prompts, explicit public URLs, and Minecraft server status/name filtering such as online, offline, healthy, or a named server
  - Calendar date understanding now resolves relative and natural-language prompts like `next Tuesday`, `today`, `tomorrow`, and `7th of April` against the Rustyfin host's local date instead of relying on model guesses
  - `/api/v1/ai/chat` can now ground explicit current-date/current-time questions through a host-local `system_get_current_datetime` tool so relative-date follow-ups have a deterministic server-side anchor
  - Weather grounding now normalizes location phrases like `Campile in County Wexford`, carries recent weather follow-up context across a bare location reply, supports recent history through `weather_get_history`, and emits deterministic server-authored weather answers from grounded payloads instead of relying on model paraphrase alone
  - `/api/v1/ai/chat` now streams tool-status events before the model answer so the `/ai` page can show lightweight progress like checking calendar, rooms, or server state
  - Grounded assistant turn stats now distinguish planning, tool, generation, queue, model-load, and end-to-end wall-clock durations, and host-runtime grounding now includes human-readable RAM summaries instead of only raw byte counts
  - Short follow-up prompts can now reuse the last grounded tool context as a planner hint, but the backend still reruns fresh auth-scoped tools instead of trusting client-sent data
  - Casual chat, jokes, tone/style requests, and simple math now stay off grounded Rustyfin tools so unrelated prompts do not fall through to library search, and explicit destructive host-action requests such as deleting system files or formatting a machine are refused server-side
  - In `Thinking` and `Extended` response modes, grounded calendar birthday lookups may perform one bounded server-side recovery pass when the first birthday search comes back empty but a stronger deterministic birthday window/query can be derived from the prompt
  - `Instant`, `Thinking`, and `Extended` now run through a bounded multi-step executor with typed tool outcomes, explicit stop reasons, recovery budgets, duplicate-signature protection, and compact evidence retention instead of relying on a one-shot plan/execute/wrap flow
  - Runtime and Admin AI diagnostics now expose the last execution stop reason, attempt counts, tool-step counts, recovery counts, and outcome distributions so bounded recovery behavior stays observable without exposing hidden reasoning
  - The assistant can now resolve simple entity references like `the second one`, `that server`, or `the first room` from the last grounded result set and then rerun a fresh scoped detail tool for that entity instead of only narrowing the prior list query
  - Grounded AI chat now emits traceable tool logs and assistant chat/tool counters into the admin runtime diagnostics surface, and recent assistant requests are durably persisted for review in the Admin `AI` tab
  - Assistant audit history now defaults to 30-day retention with hourly pruning; override with `RUSTFIN_AI_AUDIT_RETENTION_DAYS`
  - Constrained public web tools are disabled by default and require `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`; they are admin-only and use backend fetch restrictions instead of giving the model direct internet access
  - Safer fixed-provider public data tools can be exposed to all authenticated users without opening generic browsing; the current implementation ships authenticated weather lookup through Open-Meteo while keeping generic public-web search/fetch admin-only
  - Calendar writes now require explicit confirmation tokens, server-side execution, and read-after-write verification before the assistant reports success; other unsupported create/edit/delete prompts still return server-authored refusals instead of model-written faux success
  - `/api/v1/ai/transcribe` now provides authenticated speech-to-text fallback for `/ai`, with browser-native recognition preferred in supported browsers and server transcription handling size/duration-limited uploads when the browser path is unavailable
  - `/api/v1/ai/runtime` now exposes a curated authenticated AI runtime summary with active model/backend, current turn phase, queue depth, process/host memory, accurate host RAM totals/usage, host CPU, selected multi-GPU split metadata, and graceful per-GPU telemetry when the host can provide it
  - `/ai` now includes a microphone workflow with transcript preview/editing before send plus a live runtime panel that surfaces the curated AI runtime summary during and between turns
  - `/ai` now compacts older conversation history before planner/answer prompt assembly and retries once with emergency compaction if a long-running chat would otherwise exceed the active model context window
  - Native host builds now select a host-safe AI backend automatically instead of assuming CUDA is present
  - On unsupported hosts, `auto` can fall back to AI being disabled so the rest of Rustyfin still runs
  - Use `RUSTFIN_AI_GPU_BACKEND=auto|disabled|cpu|cuda|rocm|vulkan` to control the server-side AI inference backend chosen at build time
  - When GPU split mode allows it, Rustyfin now defaults to using all visible llama backend GPU devices for model loading; override with `RUSTFIN_AI_GPU_SPLIT_MODE`, `RUSTFIN_AI_GPU_MAIN_DEVICE`, and `RUSTFIN_AI_GPU_DEVICES`
  - AI models are resolved from the admin-managed `ai_model_dir` setting, then `RUSTFIN_AI_MODEL_DIR`, then the Rustyfin AI default path `/var/lib/rustyfin/ai/models`
  - First-time native installs now seed a starter GGUF model into the active AI model directory when AI is enabled and no models are present, so `/ai` is usable immediately after setup
  - If the default AI model path is not writable for the native runtime user, Rustyfin falls back to `~/.local/share/rustyfin/ai/models` and surfaces the storage warning in Admin `AI`
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
  - Saved voice-call transcripts now prefer browser speech recognition when available, fall back to per-user browser audio uploads on stop/save, and merge the resulting transcript entries by timestamp into the final markdown document
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

## Native Linux Quick Start

Preferred one-shot Linux installer:

```bash
./scripts/install_linux.sh
```

This bootstraps the host for Rustyfin, including sudo preflight, Ubuntu repository components, official Caddy setup, and Node.js 20 bootstrap where needed, then hands off to `cargo run --locked -p rustfin-installer -- install --skip-prereqs`.
The current full native install flow is implemented for Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04.
The Rust installer now owns native-user detection, Rust toolchain provisioning for the native runtime user, `yt-dlp`, PostgreSQL bootstrap, managed Java 21 provisioning, installer-written native runtime defaults at `/etc/rustyfin/native-runtime.defaults.sh`, first-install starter AI model seeding into the active AI model directory when AI is enabled, native runtime planning for ports/media/DB/origins, runtime TLS/token/snapshot persistence, native Linux binary build orchestration, native runtime artifact builds for Rust services plus the Next standalone UI, native runtime launch/stop orchestration, native clean-reset behavior, native deploy orchestration, direct `systemd` install/refresh, install-manifest output, and post-install `systemd` runtime validation with captured diagnostics if startup fails.
The public native scripts now act as compatibility wrappers around `rustfin-installer` subcommands.

Compatibility note:
- `./scripts/install_native_debian.sh` is a deprecated shim that delegates to `./scripts/install_linux.sh`.
- If you invoke the installer directly as `root`, set `RUSTFIN_NATIVE_USER=<user>` unless root-owned repo/build artifacts are intentional.
- Debian CUDA installs still require the appropriate `non-free` / `non-free-firmware` repository components to be enabled.
- The Whisper transcription model is downloaded lazily on first transcription use rather than during install.

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
- The installer-generated edge TLS certificate now covers the detected public host plus `localhost`, `127.0.0.1`, and detected local hostname aliases such as `server`, so browser access through the host name does not depend on an IP-only certificate SAN
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
- Supervisor child matching is exact-binary-aware, so `rustfin-servers-agent` cannot be mistaken for `rustfin-server`
- The native supervisor also verifies backend and edge health continuously, so a dead API process cannot leave `/login` and `/ai` serving UI shells against a broken upstream
- A separate `rustyfin-post-healthcheck.service` now runs after startup to verify backend/UI/agent readiness and recover from half-ready boots
- The post-start healthcheck also retries a configured network-backed `RUSTFIN_MEDIA_PATH` mount after boot when the NAS becomes reachable later, so a boot-time `network is unreachable` mount failure does not remain stuck until manual intervention

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
- `RUSTFIN_AI_GPU_SPLIT_MODE` - llama multi-GPU split mode for model loading (`layer|row|none`)
- `RUSTFIN_AI_GPU_MAIN_DEVICE` - preferred single-GPU backend device index when split mode is `none`
- `RUSTFIN_AI_GPU_DEVICES` - comma-separated llama backend device indices to use for model loading (empty or `all` uses all visible GPU devices)

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
- `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU` (`0` by default; set to `1` only when you want transcription requests to hard-fail unless a supported GPU backend is ready)
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

Database-backed server integration suite:

```bash
cargo test -p rustfin-server --test integration --features db-integration-tests
```

This suite is opt-in because it requires a live PostgreSQL target through `RUSTFIN_TEST_DATABASE_URL` or `RUSTFIN_DATABASE_URL`. The default `cargo test --workspace` path now stays portable on developer machines that do not have an integration database configured.

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

Some archived planning/reference documents under `/Users/iwanteague/Desktop/Rustyfin/docs/` still discuss earlier design phases. Current operational guidance is limited to the supported native Linux install/runtime surface documented in this file, `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`, and `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`.
