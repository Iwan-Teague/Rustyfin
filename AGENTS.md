# Rustyfin Agent Guide

This file defines repo-specific operating rules for coding agents and contributors.

## Project Summary

Rustyfin is a Docker-first local media platform with:
- Rust backend (`crates/server`, Axum + PostgreSQL)
- Rust microservices (`crates/calendar`, `crates/tmdb-agent`, `crates/youtube-agent`, `crates/transcription-agent`)
- Next.js frontend (`ui`)
- Shared Rust domain/repo crates (`crates/core`, `crates/db`, `crates/scanner`, `crates/metadata`, `crates/transcoder`)
- A new `Servers` product area for native game-server management, starting with Minecraft instance records and management APIs/UI
  - Current `Servers` slice includes native Minecraft lifecycle control (`start`, `stop`, `restart`, status refresh), live readiness/player-count probing, managed provisioning, existing-server import, journald log viewing, discovery scans for existing Minecraft directories, and host systemd unit rendering when Rustyfin is running in its supported native Debian deployment mode.
  - Privileged Minecraft host operations are now split behind a dedicated Rust `rustfin-servers-agent`; keep the main backend focused on orchestration, authorization, jobs, and DB/audit updates.

## Core Rules

1. Commit Identity (mandatory)
- All commits must use:
  - `user.name = Iwan-Teague`
  - `user.email = teague.iwan@outlook.com`
- No other commit identity is allowed for this repository.

2. Rust-First Policy
- Use Rust where possible for backend/business logic, services, and system integrations.
- Prefer extending existing Rust crates/services over introducing new non-Rust backend components.
- Keep frontend-only logic in UI when it is purely presentational/UX.

3. Rust Toolchain Policy
- Rust toolchain is pinned to stable via `/Users/iwanteague/Desktop/Rustyfin/rust-toolchain.toml`.
- Do not move this repository to nightly unless explicitly requested and documented.

4. Keep Existing Architecture Stable
- Do not break: setup flow, libraries/scanning, playback, channels, rooms, calendar, admin, start/stop/clean scripts.
- Favor additive, backward-compatible changes.

5. Script Platform Policy
- Repository runtime/ops scripts are POSIX shell-based (`.sh`) only.
- Do not add or reintroduce PowerShell (`.ps1`) script variants.

6. UI Animation Consistency (mandatory)
- Save/Create/primary actions:
  - Use `.btn-primary` for primary action buttons.
  - Keep click feedback centralized through `ui/src/app/components/PrimaryButtonEffects.tsx` and `.btn-click-burst` styles in `ui/src/app/globals.css`.
  - Do not introduce page-specific one-off save/click animations when the shared primary button animation can be used.
- Delete actions:
  - Use the shared delete animation helper `playTelegramDeleteAnimation` from `ui/src/lib/deleteAnimation.ts`.
  - Use `findDataDeleteTarget` (or a direct equivalent target lookup) so the element being removed visibly animates before deletion.
  - Keep the shared fade-out motion in `ui/src/app/globals.css` (`.tg-delete-target.tg-delete-out` and `tg-delete-fade-out`) as the canonical delete animation style.
  - Apply this consistently to all delete surfaces (messages, channels, transcripts, queue items, calendar/admin records, etc.).

## Runtime and Scripts

- Start stack: `./scripts/start.sh`
- Stop stack: `./scripts/stop.sh`
- Clean install/reset: `./scripts/clean_install.sh`

Rust build runtime behavior:
- `start.sh` defaults to native host Rust binary compilation for Linux targets, then Docker images copy the prebuilt binaries.
- To force legacy Docker builder-stage Rust compilation, use `--docker-rust-build` (or `RUSTFIN_NATIVE_RUST_BUILD=0`).
- On non-Linux hosts, native cross-build requires `zig` and `cargo-zigbuild`.
- Native prerequisite strict mode defaults to `RUSTFIN_NATIVE_RUST_BUILD_STRICT=1` (fail-fast).
  - Set `RUSTFIN_NATIVE_RUST_BUILD_STRICT=0` to allow fallback to Docker Rust build mode.
- On Linux hosts, `start.sh` auto-attaches `/dev/dri` to the `rustfin` container when present (unless `RUSTFIN_AUTO_HW_ACCEL=0`).
- Use `RUSTFIN_TRANSCODER_HW_ACCEL` to force hardware mode (`auto`, `none`, `nvenc`, `vaapi`, `qsv`, `videotoolbox`).
- Transcription GPU path:
  - `RUSTFIN_TRANSCRIPTION_GPU_MODE=opencl|cuda|hip|auto` (default `opencl`, where `auto` resolves to `opencl`).
  - `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU=1` by default (no CPU fallback; transcription requests are rejected if GPU backend is unavailable).
  - `start.sh` attempts GPU device mapping for both `/dev/dri` (Intel/AMD) and `/dev/nvidia*` (NVIDIA) for `rustfin-transcription-agent`.
  - `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES` controls agent backend build features (for example `gpu-opencl`, `gpu-cuda`, `gpu-hip`).

Primary containers:
- `postgres` (PostgreSQL database)
- `rustfin` (main API)
- `rustfin-calendar` (calendar service)
- `rustfin-tmdb-agent` (TMDB sync service)
- `rustfin-youtube-agent` (YouTube audio download service)
- `rustfin-transcription-agent` (Whisper transcription service)
- `rustfin-ui` (Next.js app)
- `rustfin-edge` (HTTPS edge proxy)

Optional native host companion:
- `rustfin-servers-agent`
  - Intended to run on the Debian host outside the main backend runtime.
  - Owns privileged Minecraft host operations (`systemctl`, `journalctl`, managed provisioning/import, discovery scans).
  - Main backend talks to it via `RUSTFIN_SERVERS_AGENT_URL` and `RUSTFIN_SERVERS_AGENT_TOKEN`.

Database runtime configuration:
- Prefer `RUSTFIN_DATABASE_URL` for new wiring.
- Runtime is PostgreSQL-only; `RUSTFIN_DATABASE_URL` must be `postgres://` or `postgresql://`.
- Docker runtime defaults to PostgreSQL (`postgres` service) when `RUSTFIN_DATABASE_URL` is not explicitly set.
- Migration authority is controlled by `RUSTFIN_RUN_MIGRATIONS`.
  - Compose defaults:
    - `rustfin`: `RUSTFIN_RUN_MIGRATIONS=true`
    - `rustfin-calendar`: `RUSTFIN_RUN_MIGRATIONS=false`
    - `rustfin-tmdb-agent`: `RUSTFIN_RUN_MIGRATIONS=false`
- PostgreSQL migrations live in `crates/db/migrations_pg/`.

## Quality Gates

Run before finalizing substantial changes:
- Rust format: `cargo fmt --all`
- Rust checks: `cargo check` (or targeted crate checks)
- Rust tests when relevant: `cargo test`
- UI build: `npm --prefix ui run build`

## Security/Operational Notes

- Do not place sensitive auth tokens in URL query strings.
- Enforce server-side authorization; UI checks are UX only.
- Keep credentials/secrets in environment variables, not hardcoded.
- Prefer explicit error handling and structured logging in Rust services.
- For online Listen Together downloads, prefer maintaining a current `yt-dlp` runtime in `rustfin-youtube-agent`; YouTube provider changes can break stale downloader builds.

## Implementation Style

- Keep code pragmatic and production-oriented.
- Reuse existing repo patterns before adding new abstractions.
- Keep changes scoped and readable; avoid unrelated refactors.
- When architecture, runtime behavior, or developer conventions change, update `README.md` and this `AGENTS.md` in the same change.
