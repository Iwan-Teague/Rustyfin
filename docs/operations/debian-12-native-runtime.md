# Rustyfin Native Debian 12 Runtime

This is the current operational runtime model for Rustyfin.

## Supported Environment

- Debian 12 (Bookworm)
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
./scripts/install_native_debian.sh
./scripts/start-native.sh
```

That path:

- installs and checks host dependencies
- builds Rust services directly on the Debian host
- builds the Next.js UI directly on the Debian host
- launches the native runtime

## Boot Persistence

Install native `systemd` services:

```bash
./scripts/install_native_systemd.sh
```

That installs:

- `rustyfin-native.service`
- `rustfin-servers-agent.service`

Use `systemctl` normally after installation:

```bash
sudo systemctl status rustyfin-native.service
sudo systemctl status rustfin-servers-agent.service
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
