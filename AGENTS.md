# Rustyfin Agent Guide

This file defines repo-specific operating rules for coding agents and contributors.

## Project Summary

Rustyfin is a Docker-first local media platform with:
- Rust backend (`crates/server`, Axum + SQLite)
- Rust microservices (`crates/calendar`, `crates/tmdb-agent`)
- Next.js frontend (`ui`)
- Shared Rust domain/repo crates (`crates/core`, `crates/db`, `crates/scanner`, `crates/metadata`, `crates/transcoder`)

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

3. Keep Existing Architecture Stable
- Do not break: setup flow, libraries/scanning, playback, channels, rooms, calendar, admin, start/stop/clean scripts.
- Favor additive, backward-compatible changes.

## Runtime and Scripts

- Start stack: `./scripts/start.sh`
- Stop stack: `./scripts/stop.sh`
- Clean install/reset: `./scripts/clean_install.sh`

Primary containers:
- `rustfin` (main API)
- `rustfin-calendar` (calendar service)
- `rustfin-tmdb-agent` (TMDB sync service)
- `rustfin-ui` (Next.js app)
- `rustfin-edge` (HTTPS edge proxy)

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

## Implementation Style

- Keep code pragmatic and production-oriented.
- Reuse existing repo patterns before adding new abstractions.
- Keep changes scoped and readable; avoid unrelated refactors.

