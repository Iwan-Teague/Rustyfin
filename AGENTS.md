# Rustyfin Agent Guide

This file defines repo-specific operating rules for coding agents and contributors.

## Project Summary

Rustyfin is a native-Debian-first local media platform with:

- Rust backend (`crates/server`, Axum + PostgreSQL)
- Rust product crate (`crates/rustyvault`) for RustyVault-owned logic, shared types, and packaging
- Rust installer crate (`crates/installer`) for Rust-first Linux install orchestration
- Rust microservices (`crates/calendar`, `crates/tmdb-agent`, `crates/youtube-agent`, `crates/transcription-agent`, `crates/servers-agent`)
- Next.js frontend (`ui`)
- Product-scoped frontend feature module (`ui/src/features/rustyvault`) for the Vault surface
- Browser extension MVP (`extensions/rustyvault-webext`)
  - downloadable from the host Downloads page via `/api/v1/downloads/artifacts/rustyvault-webext/package`
- Shared Rust domain/repo crates (`crates/core`, `crates/db`, `crates/scanner`, `crates/metadata`, `crates/transcoder`, `crates/servers-host`)
- A `Servers` product area for native game-server management, currently focused on Minecraft on supported Debian hosts through `systemd`
- A `Vault` product area for client-side encrypted password storage, web management, and browser-extension pairing/autofill
  - implementation ownership is being moved behind RustyVault host adapters so Rustyfin keeps only the host-facing mount points
  - shared Rust types now live in `crates/rustyvault/src/types.rs`
  - canonical RustyVault preference normalization now lives on `rustyvault::types::RustyVaultPreferences`
  - canonical UI ownership is `ui/src/features/rustyvault`; do not reintroduce generic `ui/src/lib/vault*` shims or `ui/src/lib/vaultGenerator.ts`
  - canonical persistence ownership is `crates/db/src/repo/rustyvault.rs`, including dedicated RustyVault preferences storage through `crates/db/migrations_pg/040_rustyvault_preferences.sql`
  - RustyVault session/auth internals use `x-rustyvault-*` headers, the `rustyvault_session` JWT audience, and host-scoped validation in `crates/server/src/rustyvault_host/auth.rs`
  - canonical RustyVault settings I/O is the host `/api/v1/vault/preferences` adapter backed by RustyVault-owned persistence; do not route RustyVault UI settings through generic account-profile API helpers or `/users/me/preferences`
  - retained RustyVault reads and management views should prefer `RustyVaultSessionUser` over plain `AuthUser`; keep routes like `/api/v1/vault/config`, `/api/v1/vault/preferences`, `/api/v1/vault/device-sessions`, and `/api/v1/vault/audit` session-bound unless there is a strong documented reason not to
  - keep bootstrap, protected-action challenge, and revoke-other-session flows session-bound too unless there is a strong documented bootstrap/recovery reason not to
  - when a RustyVault route accepts both Rustyfin auth and a RustyVault session, explicitly reject cross-user token mixing instead of assuming the headers belong to the same user
  - backend removability path exists through the `rustyvault` Cargo feature, and runtime graceful-disable exists through `RUSTFIN_RUSTYVAULT_ENABLED=0`
  - host-facing RustyVault routes should map to live web UI or extension consumers; do not reintroduce removed convenience endpoints such as `/api/v1/vault/sync` or `/api/v1/vault/protected-actions/complete` without a concrete product consumer
  - `crates/server/src/account_prefs.rs` is host-only account state now; do not reintroduce RustyVault settings into that shared model
  - when RustyVault is unavailable, the Vault surface should return `503` or render an unavailable state; the rest of Rustyfin should continue operating normally
- A `Downloads` product area for first-party packages, extensions, and future Rustyfin client releases
  - keep Downloads host-owned; do not make `ui/src/app/downloads/page.tsx` depend on `ui/src/features/rustyvault/api.ts`
  - treat the Downloads catalog/artifact routes as the authoritative public delivery surface for first-party packages
- A `Backups` product area for end-user archive exports and future gallery/media backup flows
  - web `/backups` should stay user-scoped and archive-oriented, not a thin wrapper over destructive host restore controls
  - current account archive exports include profile state, user preferences, AI conversation history, playback progress, continue-watching data, and activity history
  - optional RustyVault material on `/backups` must flow through the existing protected export path; do not add weaker host-side shortcuts around RustyVault export safeguards
  - host/system backup and restore routes remain operational surfaces under `/api/v1/system/backups`
- An `AI` product area for the `/ai` assistant surface backed by `crates/ai-agent`
  - native host builds must not assume CUDA is present; use a host-safe backend selection path and allow AI to be disabled when the host cannot support inference cleanly
  - use `RUSTFIN_AI_GPU_BACKEND=auto|disabled|cpu|cuda|rocm|vulkan` to control native AI backend selection during host builds
  - keep model download/delete/storage-folder management admin-only in the Admin `AI` tab; do not reintroduce end-user model installation controls on `/ai`
  - AI model storage resolves from the `ai_model_dir` DB setting first, then `RUSTFIN_AI_MODEL_DIR`, then `/var/lib/rustyfin/ai/models`
  - first-time native installs should seed a small starter GGUF model into the active AI model directory when AI is enabled and no models exist yet, so `/ai` is usable immediately after setup
  - if that default AI model path is not writable for the native runtime user, Rustyfin falls back to `~/.local/share/rustyfin/ai/models` and should surface the storage warning in Admin `AI` instead of collapsing the state into generic AI unavailability
  - grounded assistant access should stay server-side under `crates/server/src/ai_assistant`; do not let the model call DB/network/filesystem primitives directly
  - current grounded `/ai` behavior is read-mostly and limited to account summary, visible calendar events/birthdays plus specific event details, confirmation-gated personal/shared calendar event creation and recurring birthday creation, confirmation-gated archive/delete/group-move actions over the signed-in user's own AI conversations, recent visible channel activity, transcript-based summaries of accessible completed voice calls, authenticated downloads catalog entries, authenticated AI runtime/model summary, host-visible network topology and Rustyfin network settings, accessible libraries, library title search, recently added library items, active public rooms, joinable rooms and invites, authenticated public weather via a fixed provider, admin-only host runtime stats, admin-only backup/service/transcode/storage/recent-error summaries, admin-only constrained public web search/page summary when enabled, and accessible Minecraft server status
  - library search turns should stay server-authored and compact; do not surface raw JSON query/match arrays in the user-visible assistant response or grounding prompt
  - `/api/v1/ai/conversations` is the canonical per-user persisted chat history surface for `/ai`, including the conversation-backed streaming route; keep `/api/v1/ai/chat` as the compatibility path unless there is a documented removal plan
  - grounded `/api/v1/ai/chat` now uses a model-assisted structured planner, but the backend remains the authority for registry validation, role filtering, deterministic fallback, and entity-follow-up normalization
  - current query understanding also supports calendar window extraction, deterministic next-event prompts through `calendar_get_next_event`, broader next-up phrasing such as `What is the next thing coming up in my calendar?`, specific calendar-event detail prompts, named/self/next birthday lookups such as `When is Rachel's birthday?`, `When is my birthday?`, or `What's the next birthday in my calendar?`, confirmation-gated birthday or event-create prompts such as `Add Rachel's birthday on June 9, 2003`, confirmation-gated delete prompts such as `Delete dentist appointment on 2026-06-09 from my calendar`, confirmation-gated document prompts such as `Create a markdown document summarizing my next event`, AI runtime/model questions such as `What AI model is loaded right now?` or `What backend are you using?`, network questions such as interface/IP/hostname/remote-access prompts plus LAN connect questions like `What IP should I use to connect to Rustyfin on my local network?`, recent channel-activity prompts, transcript-summary prompts for transcribed voice calls, room-mode filtering, joinable-room and invite prompts, recently added library prompts, authenticated public-weather prompts such as current temperature, rain/forecast questions, county-style location phrasing, and recent-history prompts like yesterday for a named location, admin-only host-runtime/service-health/storage/transcode/recent-error prompts, explicit public URLs, Minecraft server availability filtering, and named-server matching; keep new capability docs aligned with actual behavior
  - explicit tool/capability inventory questions such as `What functions do you have access to in this environment?` must stay backend-owned and deterministic; do not let those prompts fall through to unrelated grounded tools or model-invented function names
  - host-local assistant date understanding must resolve relative and natural-language calendar prompts like `next Tuesday`, `today`, `tomorrow`, and `7th of April` deterministically on the backend, and short corrective follow-ups must stay anchored to the active date question instead of replanning loosely
  - `system_get_current_datetime` is the canonical grounded tool for explicit current-date/current-time questions and relative-date anchoring; use the Rustyfin host local clock as the authority
  - grounded weather behavior must stay backend-owned and deterministic: normalize location follow-ups like a bare `Campile, County Wexford, Ireland`, route current vs forecast vs recent-history questions to the correct tool, and prefer server-authored grounded weather answers over model-only refusals when weather payloads already succeeded
  - constrained public web tools must remain backend-owned, admin-only, and disabled by default unless `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1`; keep SSRF/private-network blocking, bounded extraction, and no-auth-forwarding rules intact
  - curated fixed-provider public data tools are the preferred way to expose external read-only information to normal users; do not widen generic public-web search/fetch just to satisfy narrow weather/news-style use cases
  - grounded `/ai` now emits server-driven `phase`, `tool`, and compatibility `status` events before token streaming; use those structured events for the visible `Thinking...` and tool activity UI, not hidden reasoning or chain-of-thought exposure
  - persisted assistant turns must retain `activity_trace_json`, `stats_json`, grounding sources, follow-up contexts, and pending confirmation payload state so reloaded chats can replay the same assistant activity stack and confirmation cards cleanly
  - long-running `/ai` conversations must compact older history before planner/answer prompt assembly and should retry once with a more aggressive compacted prompt before surfacing a context-window failure to the user
  - AI runtime optimization work should stay local-first by default: benchmark recommendations must come from evidence on the host, the warm-model pool must respect host memory limits, queueing and overload behavior must stay visible, and remote backends remain optional provider abstractions rather than hardwired orchestration assumptions
  - grounded turns also persist compact grounding chunks, typed memory items, and entity-graph rows; keep that retrieval path ACL-safe, CPU-friendly, and compact instead of injecting raw tool payloads
  - in `Thinking` and `Extended`, grounded birthday lookups may take one bounded server-side recovery pass when the first birthday search is empty but the backend can derive a better birthday window or self/name query from the prompt; keep that retry deterministic and observable
  - grounded `/api/v1/ai/chat` now runs through a bounded multi-step executor with typed tool outcomes, explicit stop reasons, duplicate-signature protection, compact evidence retention, and mode-specific retry budgets; preserve that executor layer rather than reverting to one-shot tool execution
  - runtime and admin AI diagnostics should keep exposing bounded execution telemetry such as stop reason, attempt count, tool steps, alternates, recovery steps, and outcome distributions without surfacing hidden reasoning text
  - casual chat, jokes, tone/style requests, and simple math must stay off grounded Rustyfin tools, and explicit destructive host-action requests like deleting system files or formatting a machine must be refused server-side instead of falling through to model/tool planning
  - short follow-up turns may use prior grounded tool names as planner hints only; never trust client-sent grounding payloads as authoritative data, always rerun server-side tools
  - short follow-up turns may also carry minimal hidden follow-up context for entity references like `the second one`; treat it as a hint, rerun the relevant server-side detail tool, and never trust the client payload as authoritative state
  - grounded host-runtime summaries should include accurate human-readable memory fields and per-turn stats should preserve planner/tool/generation/queue/load/end-to-end timing so `/ai` does not mislabel generation time as total turn time
  - grounded `/ai` now records traceable server logs plus assistant chat/tool runtime counters, and recent requests are durably persisted for the Admin `AI` tab; preserve those diagnostics when extending the assistant
  - assistant audit retention now defaults to 30 days with hourly pruning and `RUSTFIN_AI_AUDIT_RETENTION_DAYS` override support; preserve that lifecycle when extending audit persistence
  - `/api/v1/ai/transcribe` is the authenticated `/ai` speech-to-text fallback surface; keep uploads transient, enforce backend size/duration limits, and prefer browser-native speech recognition when it is available
  - `/api/v1/ai/runtime` is the curated authenticated runtime/telemetry surface for `/ai`; keep it focused on active model/backend, turn phase, queue depth, AI-relevant process/host resource usage, selected multi-GPU split/device state, and graceful per-GPU telemetry when available
  - grounded tool registry metadata must be enforced at execution time; do not treat read/write, role, or confirmation requirements as documentation-only fields
  - confirmation-gated calendar write tools must not execute without a valid unexpired token and a server-side read-after-write verification pass; do not let the model imply product writes succeeded before that verification completes
  - unsupported create/edit/delete prompts outside the explicit calendar create/birthday flow must still return server-authored refusals
  - do not introduce additional write-capable assistant actions without an explicit confirmation-token or protected-action design

## Core Rules

1. Commit Identity (mandatory)
- All commits must use:
  - `user.name = Iwan-Teague`
  - `user.email = teague.iwan@outlook.com`
- No other commit identity is allowed for this repository.

2. Rust-First Policy
- Use Rust where possible for backend logic, services, and system integrations.
- Prefer extending existing Rust crates/services over introducing new backend components in other languages.
- Keep frontend-only logic in UI when it is purely presentational or UX-specific.

3. Rust Toolchain Policy
- Rust toolchain is pinned to stable via `/Users/iwanteague/Desktop/Rustyfin/rust-toolchain.toml`.
- Do not move this repository to nightly unless explicitly requested and documented.

4. Keep Existing Architecture Stable
- Do not break: setup flow, libraries/scanning, playback, channels, rooms, calendar, admin, native start/stop/clean scripts.
- Favor additive, backward-compatible changes unless the requested change is an intentional runtime cutover.

5. Script Platform Policy
- Runtime and operational scripts are POSIX shell (`.sh`) only.
- Do not add or reintroduce PowerShell (`.ps1`) variants.
- The supported runtime target is native Debian 12 and Debian 13. Do not add new macOS, Windows, or container runtime paths.

6. UI Animation Consistency (mandatory)
- Save/Create/primary actions:
  - Use `.btn-primary` for primary action buttons.
  - Keep click feedback centralized through `ui/src/app/components/PrimaryButtonEffects.tsx` and `.btn-click-burst` styles in `ui/src/app/globals.css`.
  - Do not introduce page-specific one-off save/click animations when the shared primary button animation can be used.
- Delete actions:
  - Use the shared delete animation helper `playTelegramDeleteAnimation` from `ui/src/lib/deleteAnimation.ts`.
  - Use `findDataDeleteTarget` (or a direct equivalent target lookup) so the element being removed visibly animates before deletion.
  - Keep the shared fade-out motion in `ui/src/app/globals.css` (`.tg-delete-target.tg-delete-out` and `tg-delete-fade-out`) as the canonical delete animation style.
  - Apply this consistently to all delete surfaces.

## Runtime and Scripts

- Start runtime: `./scripts/start.sh`
- Stop runtime: `./scripts/stop.sh`
- Preferred Linux bootstrap installer: `./scripts/install_linux.sh`
- Start native Debian runtime directly: `./scripts/start-native.sh`
- Deploy/update native Debian runtime: `./scripts/deploy-native.sh`
- Stop native Debian runtime directly: `./scripts/stop-native.sh`
- Install native Debian prerequisites: `./scripts/install_native_debian.sh`
- Install native Debian `systemd` integration: `./scripts/install_native_systemd.sh`
- Clean install/reset: `./scripts/clean_install.sh`

Runtime behavior:

- `install_linux.sh` is the preferred public installer entrypoint:
  - installs minimal Rust bootstrap dependencies for the detected Linux package manager
  - installs Rust via `rustup` when needed
  - hands off to `cargo run -p rustfin-installer`
  - the current full install flow behind `rustfin-installer` is implemented for Debian 12 and Debian 13
  - `rustfin-installer` now owns Debian prerequisite installation, native-user detection, Rust toolchain provisioning for the native runtime user, `yt-dlp`, PostgreSQL bootstrap, managed Java 21 provisioning, installer-written native runtime defaults at `/etc/rustyfin/native-runtime.defaults.sh`, first-install starter AI model seeding into the active AI model directory when AI is enabled, native runtime planning for ports/media/DB/origins, runtime TLS/token/snapshot persistence, native Linux binary build orchestration, native runtime artifact builds for Rust services plus the Next standalone UI, native runtime launch/stop orchestration, native clean-reset behavior, native deploy orchestration, direct `systemd` install/refresh, install-manifest output, and post-install `systemd` runtime validation with captured diagnostics when startup fails
  - public native shell scripts are now compatibility wrappers around installer subcommands
- `start.sh` is a compatibility wrapper around `start-native.sh`
  - legacy Docker-era flags passed to `start.sh` are ignored for backward compatibility before delegating to native startup
- `stop.sh` is a compatibility wrapper around `stop-native.sh`
- `start-native.sh` is the supported production and development runtime path:
  - loads the native env/default layers and drives the native runtime flow
  - consumes `rustfin-installer` for runtime planning, runtime snapshot persistence, native runtime artifact builds, and native runtime launch/health orchestration
  - writes logs and pid files under `.tmp/native-runtime/`
  - installer-generated edge TLS certificates must cover the detected public host plus `localhost`, `127.0.0.1`, and detected local hostname aliases so browser access through the machine hostname does not fail on an IP-only SAN
  - supports `--build-only` for artifact refreshes without launching
- `stop-native.sh`, `install_native_systemd.sh`, and `clean_install.sh` are compatibility wrappers around `rustfin-installer` subcommands
- After the first successful native build on a supported Debian host, use `./scripts/install_native_systemd.sh` so Rustyfin starts automatically after reboot
  - this also installs a dedicated root-run `rustfin-servers-agent.service` for privileged Minecraft host operations
- After native `systemd` services are installed, use `./scripts/deploy-native.sh` for updates
  - it is now a compatibility wrapper around `rustfin-installer deploy-native`
  - the Rust deploy path stops the running runtime, pulls the current branch, rebuilds artifacts, and starts services again
- `rustyfin-native.service` is supervised through `scripts/run-native-supervisor.sh`
  - the supervisor keeps the native child-process set under `systemd` observation
  - keep supervisor child matching exact-binary-aware so `rustfin-servers-agent` can never be mistaken for `rustfin-server`
  - keep supervisor backend/edge health checks active so a dead API process cannot leave `/login` and `/ai` serving UI shells against a broken upstream
  - if a core child process dies, the service exits and `systemd` restarts the stack
- `rustyfin-post-healthcheck.service` is installed alongside the native runtime
  - it verifies backend/UI/agent readiness after startup
  - it also retries a configured network-backed `RUSTFIN_MEDIA_PATH` mount after boot when the NAS endpoint becomes reachable later
  - it performs one native-service restart if the host boots half-ready
- installer-driven `systemd` setup must also prove that the backend, agents, and HTTPS UI come up before reporting success
- if installer validation fails, keep the captured `systemctl status` output and native log tails; do not replace this with a silent or best-effort success path
- On Linux hosts, use `RUSTFIN_TRANSCODER_HW_ACCEL` to control hardware acceleration (`auto`, `none`, `nvenc`, `vaapi`, `qsv`, `videotoolbox`)
- On Linux hosts, use `RUSTFIN_AI_GPU_BACKEND` to control the AI inference backend (`auto`, `disabled`, `cpu`, `cuda`, `rocm`, `vulkan`)
- On Linux hosts, use `RUSTFIN_AI_GPU_SPLIT_MODE` to control llama model split mode (`layer`, `row`, `none`)
- On Linux hosts, use `RUSTFIN_AI_GPU_MAIN_DEVICE` to prefer a single llama backend GPU device index when split mode is `none`
- On Linux hosts, use `RUSTFIN_AI_GPU_DEVICES` to pin a comma-separated llama backend GPU device index list; empty or `all` means use every visible GPU backend device
- Transcription GPU path:
  - `RUSTFIN_TRANSCRIPTION_GPU_MODE=opencl|cuda|hip|auto` (default `opencl`)
  - `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU=0` by default so transcription can fall back to CPU when GPU probing or runtime support is unavailable
  - `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES` controls compiled GPU backends

Primary native services:

- `postgres`
- `rustfin`
- `rustfin-calendar`
- `rustfin-tmdb-agent`
- `rustfin-youtube-agent`
- `rustfin-transcription-agent`
- `rustfin-ui`
- `rustfin-edge` (Caddy)
- `rustfin-servers-agent`
- `rustyfin-post-healthcheck.service`

Database runtime configuration:

- Prefer `RUSTFIN_DATABASE_URL`
- Runtime is PostgreSQL-only
- `RUSTFIN_DATABASE_URL` must be `postgres://` or `postgresql://`
- PostgreSQL migrations live in `crates/db/migrations_pg/`

## Quality Gates

Run before finalizing substantial changes:

- Rust format: `cargo fmt --all`
- Rust checks: `cargo check`
- Rust tests when relevant: `cargo test`
- DB-backed server integration suite when PostgreSQL is available:
  `cargo test -p rustfin-server --test integration --features db-integration-tests`
  with `RUSTFIN_TEST_DATABASE_URL` or `RUSTFIN_DATABASE_URL`
- UI build: `npm --prefix ui run build`
- Focused RustyVault removability validation when touching that boundary: `./scripts/ci/rustyvault_removability_gates.sh`

## Security and Operational Notes

- Do not place sensitive auth tokens in URL query strings
- Enforce server-side authorization; UI checks are UX only
- Keep credentials and secrets in environment variables, not hardcoded
- Prefer explicit error handling and structured logging in Rust services
- For online Listen Together downloads, prefer maintaining a current `yt-dlp` runtime in `rustfin-youtube-agent`

## Implementation Style

- Keep code pragmatic and production-oriented
- Reuse existing repo patterns before adding new abstractions
- Keep changes scoped and readable; avoid unrelated refactors
- When architecture, runtime behavior, or developer conventions change, update `/Users/iwanteague/Desktop/Rustyfin/README.md` and `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md` in the same change

## Documentation Authority

Current documentation authority, in order:

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`

When older docs stop matching the current code or runtime model, remove them instead of keeping in-repo archival copies.
