# Rustyfin Native Debian Runtime

This is the current operational runtime model for Rustyfin.

## Supported Environment

- Debian 12 (Bookworm)
- Debian 13 (Trixie)
- headless or minimal install
- `systemd`
- PostgreSQL installed on the host
- Caddy installed on the host
- Node.js installed on the host
- Rust toolchain installed on the host
- `ffmpeg` and `ffprobe` installed on the host

Rustyfin does **not** support Docker, Windows, or macOS as runtime targets.

## Runtime Shape

Rustyfin runs as native host processes:

- `rustfin`
- `rustfin-calendar`
- `rustfin-tmdb-agent`
- `rustfin-youtube-agent`
- `rustfin-transcription-agent`
- `rustfin-servers-agent`
- Next.js standalone UI
- Caddy HTTPS edge
- PostgreSQL

For Minecraft server management, privileged host operations are handled by `rustfin-servers-agent`, typically installed as a root-run `systemd` service.

## First-Time Install

From the repository root:

```bash
./scripts/install_linux.sh
```

This is the preferred one-shot entrypoint. It bootstraps Rust if needed and then hands off to the Rust installer (`cargo run -p rustfin-installer`), which currently drives the supported Debian native install flow.
At this stage, the Rust installer owns Debian prerequisite installation, native-user detection, Rust toolchain provisioning for the native runtime user, `yt-dlp`, PostgreSQL bootstrap, managed Java 21 provisioning, installer-written native runtime defaults at `/etc/rustyfin/native-runtime.defaults.sh`, native runtime planning for ports/media/DB/origins, runtime TLS/token/snapshot persistence, native Linux binary build orchestration, native runtime artifact builds for Rust services plus the Next standalone UI, native runtime launch/stop orchestration, native clean-reset behavior, native deploy orchestration, direct `systemd` install/refresh, install-manifest output, and post-install `systemd` runtime validation with captured diagnostics if the stack fails to come up.
The public native shell scripts now act as compatibility wrappers around `rustfin-installer` subcommands.

Manual Debian-native path:

```bash
./scripts/install_native_debian.sh
./scripts/start-native.sh
```

That path now loads the native env/default layers in shell and hands artifact build plus launch/health orchestration to the Rust installer.

## Boot Persistence

Install native `systemd` services:

```bash
./scripts/install_native_systemd.sh
```

That installs:

- `rustyfin-native.service`
- `rustfin-servers-agent.service`
- `rustyfin-post-healthcheck.service`

Use `systemctl` normally after installation:

```bash
sudo systemctl status rustyfin-native.service
sudo systemctl status rustfin-servers-agent.service
sudo systemctl status rustyfin-post-healthcheck.service
```

## Day-2 Operations

Start:

```bash
./scripts/start.sh
```

Stop:

```bash
./scripts/stop.sh
```

Direct native start:

```bash
./scripts/start-native.sh
```

Direct native stop:

```bash
./scripts/stop-native.sh
```

Deploy/update on a Debian host:

```bash
./scripts/deploy-native.sh
```

Use `deploy-native.sh` for updates instead of a raw `systemctl restart`, because deploy rebuilds artifacts before restart.
`deploy-native.sh` is now a compatibility wrapper around `rustfin-installer deploy-native`.

Operational expectation:

- `rustyfin-native.service` keeps the Rustyfin child process set supervised under `systemd`
- `rustyfin-post-healthcheck.service` verifies the stack after boot/deploy and performs one restart attempt if the host comes up half-ready
- a healthy boot should leave backend, UI, websocket paths, and native agents ready without manual intervention

Post-update quality gate:

```bash
./scripts/ci/debian_native_gates.sh
```

This is the main supported-Debian confidence sweep. It checks:

- host/runtime assumptions
- Rust formatting, lint, and targeted crate tests
- UI lint, typecheck, and production build
- isolated browser smoke for setup/login, channels, rooms, servers, and playback
- unauthenticated access control on representative protected API routes
- native runtime health endpoints, migration state, and recent journal errors

You can also run the browser smoke path directly:

```bash
./scripts/ci/debian_browser_smoke.sh
```

That smoke script:

- creates an isolated PostgreSQL schema inside the configured runtime database
- starts a temporary backend on `127.0.0.1:18096`
- starts a temporary UI on `127.0.0.1:13000`
- runs Playwright against that isolated runtime
- cleans the schema up after the run

Outputs:

- Markdown report under `/Users/iwanteague/Desktop/Rustyfin/.tmp/gates/`
- latest report copy at `/Users/iwanteague/Desktop/Rustyfin/.tmp/gates/debian-native-gates-latest.md`
- per-gate logs beside the report

Reset runtime state and PostgreSQL contents:

```bash
./scripts/clean_install.sh
```

## Ports

Default listeners:

- HTTPS edge: `3000`
- internal UI: `3001`
- backend API: `8096`
- calendar: `8099`
- TMDB agent: `8100`
- YouTube agent: `8101`
- transcription agent: `8102`
- servers agent: `8103`
- PostgreSQL: `5432`

The normal user-facing entrypoint is:

- `https://<host>:3000`

## Logs and Runtime Files

Native runtime state is written under:

- `/Users/iwanteague/Desktop/Rustyfin/.tmp/native-runtime/`

Important subpaths:

- logs: `/Users/iwanteague/Desktop/Rustyfin/.tmp/native-runtime/logs/`
- pid files: `/Users/iwanteague/Desktop/Rustyfin/.tmp/native-runtime/pids/`

Runtime environment snapshot:

- `/Users/iwanteague/Desktop/Rustyfin/.rustyfin.runtime.env`

Installer-owned native runtime defaults:

- `/etc/rustyfin/native-runtime.defaults.sh`

Installer-driven runtime planner:

- `./scripts/rustfin-installer.sh plan-native-runtime`

Installer-driven native artifact build:

- `./scripts/rustfin-installer.sh build-native-runtime-artifacts`

Installer-driven native runtime launch:

- `./scripts/rustfin-installer.sh launch-native-runtime`

Installer-driven native runtime stop/reset:

- `./scripts/rustfin-installer.sh stop-native-runtime`
- `./scripts/rustfin-installer.sh clean-native-runtime`

Installer-driven deploy entrypoint:

- `./scripts/rustfin-installer.sh deploy-native`

Installer-driven native systemd install/refresh:

- `./scripts/rustfin-installer.sh install-native-systemd`
- this now waits for the main services plus HTTPS UI readiness before reporting success
- on failure it captures `systemctl status` output and recent native log tails so fresh-host install failures are diagnosable immediately

Installer-driven runtime snapshot writer:

- `./scripts/rustfin-installer.sh write-native-runtime-snapshot`

## Database

Rustyfin runtime is PostgreSQL-only.

Primary runtime variable:

- `RUSTFIN_DATABASE_URL`

PostgreSQL migrations live in:

- `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/`

## Media Paths

Server-side media browsing and validation are native host operations.

Primary variables:

- `RUSTFIN_MEDIA_PATH`
- `RUSTFIN_DIRECTORY_BROWSE_ROOTS`

Selected media/library paths are validated as host paths. There is no container path translation layer.

On GUI-enabled Debian hosts, the optional native folder picker can still be used.
On headless Debian hosts, Rustyfin falls back to:

- server-side host directory browsing
- manual path entry

## GPU Usage

Playback/transcoding GPU mode is controlled through:

- `RUSTFIN_TRANSCODER_HW_ACCEL`
- `RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL`

Transcription GPU mode is controlled through:

- `RUSTFIN_TRANSCRIPTION_GPU_MODE`
- `RUSTFIN_TRANSCRIPTION_REQUIRE_GPU`
- `RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES`

Transcription is GPU-required by default.

## Servers / Minecraft

Minecraft server management is native Debian-only.

Runtime components:

- main Rustyfin backend for auth/orchestration
- `rustfin-servers-agent` for privileged host operations
- `systemd` for actual Minecraft unit supervision

Current UI behavior:

- the `Servers` page is a guided create/import wizard
- managed Minecraft servers auto-provision on `Start`
- only admins can create, import, and delete server records
- users can still refresh, start, stop, and restart visible server instances

Key variables:

- `RUSTFIN_SERVERS_AGENT_URL`
- `RUSTFIN_SERVERS_AGENT_TOKEN`
- `RUSTFIN_SERVERS_DEFAULT_JAVA`
- `RUSTFIN_SERVERS_SYSTEMCTL_BIN`
- `RUSTFIN_SERVERS_SYSTEMD_UNIT_DIR`
- `RUSTFIN_SERVERS_ARTIFACT_CACHE_ROOT`
- `RUSTFIN_SERVERS_IMPORT_ROOTS`

## Non-Goals

These are not supported runtime targets:

- Docker runtime
- Windows runtime
- macOS runtime
- router-managed port forwarding from Rustyfin itself

Remote access should be handled by your VPN or network layer.
