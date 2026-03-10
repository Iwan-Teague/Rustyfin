# Rustyfin Agent Guide

This file defines repo-specific operating rules for coding agents and contributors.

## Project Summary

Rustyfin is a native-Debian-first local media platform with:

- Rust backend (`crates/server`, Axum + PostgreSQL)
- Rust microservices (`crates/calendar`, `crates/tmdb-agent`, `crates/youtube-agent`, `crates/transcription-agent`, `crates/servers-agent`)
- Next.js frontend (`ui`)
- Shared Rust domain/repo crates (`crates/core`, `crates/db`, `crates/scanner`, `crates/metadata`, `crates/transcoder`, `crates/servers-host`)
- A `Servers` product area for native game-server management, currently focused on Minecraft on Debian 12 through `systemd`

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
- The supported runtime target is native Debian 12. Do not add new macOS, Windows, or container runtime paths.

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
- Start native Debian runtime directly: `./scripts/start-native.sh`
- Deploy/update native Debian runtime: `./scripts/deploy-native.sh`
- Stop native Debian runtime directly: `./scripts/stop-native.sh`
- Install native Debian prerequisites: `./scripts/install_native_debian.sh`
- Install native Debian `systemd` integration: `./scripts/install_native_systemd.sh`
- Clean install/reset: `./scripts/clean_install.sh`

Runtime behavior:

- `start.sh` is a compatibility wrapper around `start-native.sh`
- `stop.sh` is a compatibility wrapper around `stop-native.sh`
- `start-native.sh` is the supported production and development runtime path:
  - builds Rust services directly on the Debian host
  - builds the Next.js UI directly on the host
  - runs PostgreSQL, Caddy, Node, and Rust services natively
  - writes logs and pid files under `.tmp/native-runtime/`
  - supports `--build-only` for artifact refreshes without launching
- After the first successful native build on Debian 12, use `./scripts/install_native_systemd.sh` so Rustyfin starts automatically after reboot
  - this also installs a dedicated root-run `rustfin-servers-agent.service` for privileged Minecraft host operations
- After native `systemd` services are installed, use `./scripts/deploy-native.sh` for updates
  - it stops the running runtime, pulls the current branch, rebuilds artifacts, and starts services again
- `rustyfin-native.service` is supervised through `scripts/run-native-supervisor.sh`
  - the supervisor keeps the native child-process set under `systemd` observation
  - if a core child process dies, the service exits and `systemd` restarts the stack
- On Linux hosts, use `RUSTFIN_TRANSCODER_HW_ACCEL` to control hardware acceleration (`auto`, `none`, `nvenc`, `vaapi`, `qsv`, `videotoolbox`)
- Transcription GPU path:
  - `RUSTFIN_TRANSCRIPTION_GPU_MODE=opencl|cuda|hip|auto` (default `opencl`)
  - `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU=1` by default
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
- UI build: `npm --prefix ui run build`

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
