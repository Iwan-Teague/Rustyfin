# Rustyfin Servers Minecraft Implementation Plan

Date: 2026-03-08

Status: concrete implementation plan

## Baseline Decision

This plan is built around **Debian 12 headless**.

That is the correct target for this project today because:

- `README.md` explicitly defines Debian 12 headless/minimal as the intended host OS.
- the current Docker images are Bookworm-based (`debian:bookworm-slim`, `postgres:16-bookworm`, `node:22-bookworm-slim`).
- the project is already standardized around PostgreSQL and stable Rust.

This plan does **not** assume Debian 13.

## Planning Assumptions

- The private home-server repository could not be inspected because both provided GitHub tokens returned `401`.
- The plan therefore treats Minecraft hosting as a fresh Rustyfin-native capability instead of adapting an existing private implementation.
- PostgreSQL remains the only supported database.
- Rust stable remains the only supported Rust toolchain.
- The Minecraft hosting path should not depend on Docker.
- The Minecraft hosting path should not depend on Python.

## Product Goal

Add a top-level `Servers` surface to Rustyfin where users can:

- view all Minecraft servers known to Rustyfin,
- see whether each server is running,
- see version, server type, address, player count, and recent logs,
- create a new Minecraft server from the UI,
- import an existing Minecraft server directory,
- start, stop, and restart servers from the UI,
- manage permissions around who can control a given server,
- later extend into Paper/Fabric/Forge/NeoForge, modpacks, maps, and story worlds.

## Scope For The First Delivery

Initial implementation should support:

- Minecraft Java Edition only,
- Debian 12 host only,
- PostgreSQL only,
- native host runtime only for Minecraft servers,
- Rust UI/API/agent control plane,
- start/stop/restart/create/import/list/log viewing,
- Vanilla and Paper as the first supported server distributions,
- structured room-style permissions for who can view or control a server.

The first delivery should not try to ship all of the future feature ideas at once.

Initial non-goals:

- Bedrock edition,
- proxy networks such as Bungee/Velocity,
- automatic router port forwarding,
- public internet exposure from Rustyfin,
- in-browser server console write access,
- full mod marketplace management,
- one-click modpack installers,
- backups/snapshots as a required day-one feature.

Those can all be added later once the core lifecycle and storage model is stable.

## Architectural Decision

The correct Rust-first design is:

- Rustyfin UI for all user interaction,
- Rustyfin backend API for auth, authorization, persistence, and UI-facing APIs,
- a new native Rust `servers-agent` for privileged host orchestration,
- a small native Rust `servers-runner` for per-instance process management,
- `systemd` for durable service supervision,
- PostgreSQL as the source of truth,
- journald for full runtime logs,
- Rustyfin DB tables for durable lifecycle events and UI state.

This avoids Docker in the Minecraft runtime path while keeping control logic in Rust.

## Why `systemd` Still Belongs In The Design

The goal is to move away from Docker, not to replace a proven process supervisor with ad-hoc long-lived child-process handling.

`systemd` should still supervise Minecraft instances because it gives:

- boot integration,
- restart-on-failure,
- durable unit state,
- resource controls,
- clean start/stop lifecycle,
- journald log capture,
- a standard Debian 12 operational model.

Rust should own the orchestration logic, validation logic, file rendering, and lifecycle decisions.

`systemd` should own long-running process supervision.

## Rust-First Runtime Split

The implementation should use three Rust layers:

### 1. Rustyfin API

Responsibilities:

- expose the `Servers` UI APIs,
- enforce authentication and authorization,
- persist instance metadata in PostgreSQL,
- create and update lifecycle jobs,
- record audit events,
- read durable state for the UI.

### 2. `rustfin-servers-agent`

Responsibilities:

- validate and provision Minecraft instances,
- render instance manifests,
- render and install systemd unit files,
- start/stop/restart/reconcile units through systemd D-Bus,
- read unit status,
- read journald logs,
- run Minecraft ping probes for player counts and readiness,
- discover existing unmanaged Minecraft server directories,
- enforce filesystem trust boundaries.

### 3. `rustfin-servers-runner`

Responsibilities:

- run as the process launched by systemd for a specific instance,
- spawn the actual Java server process,
- keep stdin open to the Java process,
- forward stdout/stderr to journald,
- on stop, send a graceful `stop` command to Minecraft before escalating,
- surface exit codes cleanly to systemd.

This runner is important. It keeps the steady-state lifecycle Rust-native instead of shell-script driven.

## Recommended Deployment Mode

### Primary Supported Mode

For Minecraft serversing, the first-class supported deployment target should be:

- Debian 12 native backend,
- Debian 12 native servers agent,
- Debian 12 native servers runner,
- PostgreSQL native service,
- Caddy or equivalent edge service,
- no Docker requirement in the Minecraft hosting path.

### Transitional Mode

If the rest of Rustyfin remains Docker-based for a while, the servers feature can still be added, but only through a native host agent.

That transitional mode is more operationally awkward because a containerized backend needs a clean route to a native host service.

Because of that, the concrete implementation should treat **native Debian 12 deployment as the supported target for servers** and keep Docker-only installs as servers-disabled until the native profile is ready.

That is the lowest-risk plan.

## Concrete Filesystem Layout

Use these Debian 12 host paths.

### Managed Instance Root

Default root:

- `/srv/rustyfin-servers/minecraft/instances/`

Per instance:

- `/srv/rustyfin-servers/minecraft/instances/<instance-id>/`

Per instance layout:

- `server/`
- `meta/`
- `backups/`
- `uploads/`

Detailed structure:

```text
/srv/rustyfin-servers/minecraft/instances/<instance-id>/
  server/
    eula.txt
    server.properties
    whitelist.json
    ops.json
    usercache.json
    world/
    world_nether/
    world_the_end/
    plugins/
    mods/
    config/
    logs/
  meta/
    instance.json
    rendered-unit.service
    provision-report.json
  backups/
  uploads/
```

### Global Artifact Cache

Use a shared cache for downloaded server jars and later mod loader artifacts:

- `/var/cache/rustyfin-servers/minecraft/artifacts/`

Example:

```text
/var/cache/rustyfin-servers/minecraft/artifacts/
  vanilla/1.21.1/server.jar
  paper/1.21.1/build-88/paperclip.jar
  fabric/1.21.1/loader-0.16.10/server.jar
```

This avoids redownloading identical server binaries for every instance.

### Agent State

- `/var/lib/rustyfin-servers/agent/`

Used for:

- local locks,
- durable probe cursors,
- discovery scan checkpoints,
- temporary provisioning state.

### Runtime State

- `/run/rustyfin-servers/`

Used for:

- agent socket/pid files,
- transient runtime markers,
- temporary per-instance control files if needed.

### Systemd Units

- `/etc/systemd/system/rustfin-servers-agent.service`
- `/etc/systemd/system/rustyfin-minecraft-<instance-id>.service`

Generate per-instance unit files instead of a generic template.

Reason:

- easier per-instance memory limits,
- easier per-instance working directories,
- simpler debugging,
- simpler uninstall and reconciliation logic,
- no need for systemd drop-ins just to express instance-specific values.

## Concrete Database Plan

Add a new PostgreSQL migration:

- `crates/db/migrations_pg/029_servers_minecraft.sql`

Create the following tables.

### 1. `server_instance`

Purpose:

- one row per managed or imported game server instance,
- generic enough to support future games later,
- concrete enough to ship Minecraft now.

Recommended columns:

| Column | Type | Notes |
| --- | --- | --- |
| `id` | `uuid` | primary key |
| `game_kind` | `text` | initially always `minecraft` |
| `display_name` | `text` | user-facing name |
| `slug` | `text` | stable UI-safe identifier |
| `description` | `text` | optional |
| `owner_user_id` | `text` | FK to `user.id` |
| `created_by_user_id` | `text` | FK to `user.id` |
| `install_mode` | `text` | `managed` or `adopted` |
| `runtime_mode` | `text` | initial value `native_systemd` |
| `desired_state` | `text` | `stopped`, `running` |
| `observed_state` | `text` | `draft`, `provisioning`, `stopped`, `starting`, `running`, `stopping`, `failed`, `deleting` |
| `health_state` | `text` | `unknown`, `starting`, `ready`, `degraded`, `failed` |
| `instance_root` | `text` | canonical root path |
| `server_work_dir` | `text` | canonical working directory |
| `systemd_unit_name` | `text` | unique |
| `listen_host` | `text` | usually `0.0.0.0` |
| `listen_port` | `integer` | unique while active |
| `advertised_host` | `text` | optional LAN/VPN host |
| `advertised_port` | `integer` | optional override |
| `autostart` | `boolean` | default false |
| `auto_stop_when_empty` | `boolean` | default false |
| `auto_stop_idle_minutes` | `integer` | nullable |
| `current_player_count` | `integer` | default 0 |
| `max_player_count` | `integer` | nullable |
| `last_ready_ts` | `bigint` | nullable |
| `last_started_ts` | `bigint` | nullable |
| `last_stopped_ts` | `bigint` | nullable |
| `last_exit_code` | `integer` | nullable |
| `last_error_summary` | `text` | nullable |
| `created_ts` | `bigint` | required |
| `updated_ts` | `bigint` | required |

Indexes:

- `server_instance(game_kind, observed_state)`
- `server_instance(owner_user_id)`
- unique on `slug`
- unique on `systemd_unit_name`
- unique on `instance_root`

### 2. `minecraft_server_config`

Purpose:

- Minecraft-specific runtime and gameplay configuration.

Recommended columns:

| Column | Type | Notes |
| --- | --- | --- |
| `instance_id` | `uuid` | PK and FK to `server_instance.id` |
| `server_distribution` | `text` | `vanilla`, `paper`, `fabric`, `forge`, `neoforge`, `custom` |
| `minecraft_version` | `text` | required |
| `loader_version` | `text` | nullable |
| `java_path` | `text` | default `/usr/bin/java` |
| `min_memory_mb` | `integer` | required |
| `max_memory_mb` | `integer` | required |
| `jvm_flags_json` | `jsonb` | controlled allowlist only |
| `world_name` | `text` | required |
| `world_seed` | `text` | nullable |
| `level_type` | `text` | default `minecraft\:normal` |
| `gamemode` | `text` | `survival`, `creative`, `adventure`, `spectator` |
| `difficulty` | `text` | `peaceful`, `easy`, `normal`, `hard` |
| `hardcore` | `boolean` | required |
| `motd` | `text` | required |
| `online_mode` | `boolean` | required |
| `pvp` | `boolean` | required |
| `allow_flight` | `boolean` | required |
| `enable_command_block` | `boolean` | required |
| `view_distance` | `integer` | required |
| `simulation_distance` | `integer` | required |
| `spawn_protection` | `integer` | required |
| `white_list_enabled` | `boolean` | required |
| `server_icon_path` | `text` | nullable |
| `eula_accepted` | `boolean` | required |
| `eula_accepted_by_user_id` | `text` | nullable |
| `eula_accepted_ts` | `bigint` | nullable |
| `created_ts` | `bigint` | required |
| `updated_ts` | `bigint` | required |

Important constraint:

- the UI must capture EULA confirmation explicitly before provisioning a managed server.

### 3. `server_instance_member`

Purpose:

- per-instance access control.

Recommended columns:

| Column | Type | Notes |
| --- | --- | --- |
| `instance_id` | `uuid` | FK |
| `user_id` | `text` | FK |
| `role` | `text` | `viewer`, `operator`, `manager` |
| `created_by_user_id` | `text` | FK |
| `created_ts` | `bigint` | required |

Primary key:

- `(instance_id, user_id)`

Rules:

- `viewer`: can see details and logs.
- `operator`: can start, stop, and restart.
- `manager`: can edit settings, delete, import, and manage members.
- global Rustyfin admins bypass per-instance checks.

### 4. `server_instance_event`

Purpose:

- durable lifecycle and audit history for the UI.

Recommended columns:

| Column | Type | Notes |
| --- | --- | --- |
| `id` | `uuid` | PK |
| `instance_id` | `uuid` | FK |
| `job_id` | `text` | nullable link to `job.id` |
| `actor_user_id` | `text` | nullable |
| `level` | `text` | `info`, `warn`, `error` |
| `event_kind` | `text` | `create_requested`, `provision_started`, `unit_started`, `probe_ready`, etc. |
| `message` | `text` | compact UI text |
| `details_json` | `jsonb` | structured details |
| `created_ts` | `bigint` | required |

Indexes:

- `server_instance_event(instance_id, created_ts desc)`
- `server_instance_event(job_id)`

### 5. `server_discovery_candidate`

Purpose:

- track unmanaged Minecraft directories discovered on the host before import.

Recommended columns:

| Column | Type | Notes |
| --- | --- | --- |
| `id` | `uuid` | PK |
| `game_kind` | `text` | initially `minecraft` |
| `canonical_path` | `text` | unique |
| `detected_name` | `text` | nullable |
| `detected_distribution` | `text` | nullable |
| `detected_version` | `text` | nullable |
| `detection_json` | `jsonb` | raw detection details |
| `imported_instance_id` | `uuid` | nullable |
| `last_scan_status` | `text` | `candidate`, `invalid`, `imported`, `ignored` |
| `first_seen_ts` | `bigint` | required |
| `last_seen_ts` | `bigint` | required |

This makes “existing servers listed in the UI” a real feature instead of a manual admin memory problem.

## Reuse Existing Rustyfin Infrastructure

Do not build this from scratch in an isolated style.

Reuse these existing project patterns:

- `job` table for action progress,
- `audit_log::record_event` for admin-visible logs,
- `host_directories` style browsing for server-side directory selection,
- existing auth model,
- existing admin/user identities,
- existing confirmation modal patterns,
- existing button styling and layout system,
- existing PostgreSQL-only repo conventions.

## Concrete Rust Module Plan

### New Crates

Add:

- `crates/servers-core`
- `crates/servers-agent`
- `crates/servers-runner`

### `crates/servers-core`

Suggested modules:

- `instance.rs`
- `minecraft.rs`
- `permissions.rs`
- `validation.rs`
- `discovery.rs`
- `systemd_unit.rs`
- `manifest.rs`

Responsibilities:

- shared types,
- config validation,
- unit rendering,
- instance manifest model,
- detection result model,
- permission enums.

### `crates/servers-agent`

Suggested modules:

- `main.rs`
- `config.rs`
- `db.rs`
- `systemd.rs`
- `journal.rs`
- `provision.rs`
- `artifact_cache.rs`
- `minecraft_probe.rs`
- `discovery.rs`
- `reconcile.rs`
- `api.rs`

Responsibilities:

- privileged host integration,
- internal authenticated API,
- systemd control through D-Bus,
- journald log reading,
- provisioning and reconciliation.

### `crates/servers-runner`

Suggested modules:

- `main.rs`
- `manifest.rs`
- `java.rs`
- `signals.rs`
- `stdio.rs`
- `shutdown.rs`

Responsibilities:

- spawn Java,
- forward output,
- hold stdin,
- graceful stop on signal,
- controlled timeout and forced kill if the server refuses to stop.

### New DB Repo Module

Add:

- `crates/db/src/repo/servers.rs`

Responsibilities:

- CRUD for instances,
- CRUD for Minecraft config,
- list members,
- add/remove members,
- append/list instance events,
- list discovery candidates,
- reconcile observed state.

### New Server Module

Add:

- `crates/server/src/servers/`

Suggested files:

- `mod.rs`
- `handlers.rs`
- `models.rs`
- `permissions.rs`
- `agent_client.rs`

Responsibilities:

- UI-facing REST endpoints,
- auth checks,
- request validation,
- job creation,
- agent RPC calls,
- response shaping.

### New UI Surface

Add:

- `ui/src/app/servers/page.tsx`
- `ui/src/lib/serversApi.ts`
- `ui/src/app/components/servers/...`

Suggested UI components:

- `MinecraftServerList.tsx`
- `MinecraftServerDetail.tsx`
- `MinecraftServerCreateWizard.tsx`
- `MinecraftServerImportWizard.tsx`
- `ServerLogPanel.tsx`
- `ServerAccessPanel.tsx`

Also update:

- `ui/src/app/NavBar.tsx`

to add:

- `Servers`

## Concrete Runtime Lifecycle

### Server Creation Flow

1. User opens `Servers`.
2. User clicks `Create server`.
3. UI runs a guided wizard.
4. Backend validates request and authorization.
5. Backend creates:
   - a `server_instance` row in `draft` or `provisioning`,
   - a `minecraft_server_config` row,
   - a `job` row such as `servers.minecraft.create`.
6. Backend calls the servers agent with the new `job_id` and `instance_id`.
7. Agent:
   - allocates the instance directory,
   - resolves or downloads the correct server artifact,
   - writes `instance.json`,
   - writes `eula.txt`,
   - writes `server.properties`,
   - writes the systemd unit file,
   - runs `daemon-reload`,
   - optionally starts the instance,
   - updates DB state and instance events as it progresses.
8. UI polls the job and instance endpoints until the instance is ready.

### Server Start Flow

1. User clicks `Start`.
2. Backend checks `operator` or `manager` permission.
3. Backend creates `servers.minecraft.start` job.
4. Backend calls agent.
5. Agent requests `systemd` start for the instance unit.
6. Runner launches Java and begins forwarding logs.
7. Agent probes the server port until readiness or timeout.
8. Agent marks the instance `running` and `ready`.

### Server Stop Flow

1. User clicks `Stop`.
2. Backend checks permission.
3. Backend creates `servers.minecraft.stop` job.
4. Backend calls agent.
5. Agent asks systemd to stop the unit.
6. Runner receives termination signal.
7. Runner writes `stop\n` to Minecraft stdin and waits.
8. If the server exits cleanly, runner exits 0.
9. If it hangs past timeout, runner escalates and exits non-zero.
10. Agent records final observed state and exit summary.

### Server Restart Flow

Same as stop then start, but it should remain one tracked job in the UI.

### Import Existing Server Flow

1. User clicks `Import existing server`.
2. UI opens server-side directory browser.
3. User selects a candidate path.
4. Backend asks the agent to validate the directory.
5. Agent checks for expected markers such as:
   - `server.properties`,
   - `eula.txt`,
   - world folder with `level.dat`,
   - server jar or distribution markers,
   - writable working directory.
6. Agent returns a detection preview.
7. User confirms import.
8. Backend creates DB rows and a `servers.minecraft.import` job.
9. Agent creates a systemd unit for the adopted path without moving files.
10. Imported instance appears in the managed list.

Initial import mode should be **adopt in place**.

Do not make “copy into a new managed root” mandatory in v1.

That keeps the first import path simple and safe.

## Guided Create Wizard

The first create wizard should be compact and opinionated.

### Step 1. Basics

- server display name,
- distribution: `Vanilla` or `Paper`,
- Minecraft version,
- world name,
- optional description.

### Step 2. Gameplay

- gamemode,
- difficulty,
- hardcore,
- allow command blocks,
- PVP,
- max players,
- MOTD.

### Step 3. Runtime

- memory preset,
- optional advanced memory edit,
- port,
- auto-start,
- optional stop-when-empty,
- online mode,
- whitelist.

### Step 4. Review

- show resolved paths,
- show port,
- show memory,
- show distribution and version,
- require explicit EULA confirmation,
- create server.

Keep advanced JVM flags out of the first wizard.

If exposed at all, they should live behind an explicit advanced settings surface with server-side allowlist validation.

## Internal Manifest

Each instance should have a rendered manifest file:

- `/srv/rustyfin-servers/minecraft/instances/<instance-id>/meta/instance.json`

This file is the contract between the agent and the runner.

It should contain:

- instance id,
- display name,
- working directory,
- Java path,
- memory values,
- JVM flags,
- artifact path,
- server jar launch mode,
- port,
- readiness timeout,
- graceful stop timeout.

This makes the runner stateless and deterministic.

## Systemd Unit Shape

Each instance gets a generated unit:

- `rustyfin-minecraft-<instance-id>.service`

Conceptual form:

```ini
[Unit]
Description=Rustyfin Minecraft instance <display-name>
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=rustfin-servers
Group=rustfin-servers
WorkingDirectory=/srv/rustyfin-servers/minecraft/instances/<instance-id>/server
ExecStart=/usr/local/bin/rustfin-servers-runner --manifest /srv/rustyfin-servers/minecraft/instances/<instance-id>/meta/instance.json
Restart=on-failure
RestartSec=5
TimeoutStopSec=120
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadWritePaths=/srv/rustyfin-servers /var/cache/rustyfin-servers /var/lib/rustyfin-servers
MemoryMax=6G
CPUWeight=100

[Install]
WantedBy=multi-user.target
```

The exact hardening directives can be tuned during implementation, but the direction should be:

- minimal privileges,
- explicit writable paths,
- resource limits,
- no shell wrapper.

## Agent API Plan

Expose a host-local internal API for the backend to call.

Suggested base:

- `/internal/v1/servers/minecraft/...`

This API is not user-facing.

It should be protected with:

- loopback-only bind in the native profile, or
- explicit internal token,
- optional Unix socket support later.

Suggested endpoints:

- `POST /instances/create`
- `POST /instances/import`
- `POST /instances/{id}/start`
- `POST /instances/{id}/stop`
- `POST /instances/{id}/restart`
- `POST /instances/{id}/delete`
- `POST /instances/{id}/reconcile`
- `GET /instances/{id}/status`
- `GET /instances/{id}/logs`
- `POST /discovery/scan`
- `POST /validate-path`

The backend should be the only expected client.

## Rustyfin Public API Plan

Expose UI-facing routes under:

- `/api/v1/servers/minecraft/...`

Suggested routes:

- `GET /api/v1/servers/minecraft/instances`
- `POST /api/v1/servers/minecraft/instances`
- `GET /api/v1/servers/minecraft/instances/{id}`
- `PATCH /api/v1/servers/minecraft/instances/{id}`
- `DELETE /api/v1/servers/minecraft/instances/{id}`
- `POST /api/v1/servers/minecraft/instances/{id}/start`
- `POST /api/v1/servers/minecraft/instances/{id}/stop`
- `POST /api/v1/servers/minecraft/instances/{id}/restart`
- `GET /api/v1/servers/minecraft/instances/{id}/logs`
- `GET /api/v1/servers/minecraft/instances/{id}/events`
- `GET /api/v1/servers/minecraft/instances/{id}/members`
- `POST /api/v1/servers/minecraft/instances/{id}/members`
- `DELETE /api/v1/servers/minecraft/instances/{id}/members/{user_id}`
- `GET /api/v1/servers/minecraft/discovery`
- `POST /api/v1/servers/minecraft/discovery/scan`
- `POST /api/v1/servers/minecraft/import-preview`
- `POST /api/v1/servers/minecraft/import`

## UI Plan

### Top-Level Navigation

Add a new top nav item:

- `Servers`

### Servers Page Layout

Recommended layout:

- left column:
  - managed server list,
  - unmanaged discovered servers,
  - filters for `running`, `stopped`, `failed`.
- center column:
  - selected server dashboard,
  - start/stop/restart buttons,
  - status badges,
  - player count,
  - version and distribution,
  - connect address,
  - recent events,
  - recent logs.
- right column:
  - create wizard,
  - import wizard,
  - settings editor,
  - access control panel.

This matches the rest of Rustyfin’s multi-pane operational style.

### UI Behavior Requirements

- Creation and import actions should feel job-based and live.
- Buttons should not block the whole page.
- Every action should surface status immediately.
- Destructive actions should use the same centered confirmation modal pattern already used elsewhere.
- Deleting an instance should default to unregister-only.
- “Delete files from disk” should be a second explicit destructive path, admin-only.

## Discovery Plan

Existing servers cannot appear in the UI unless Rustyfin can detect them.

Add discovery roots:

- `/srv`
- `/home`
- `/opt`
- configurable additional roots through environment or settings

Detection heuristics:

- `server.properties` exists,
- world directory with `level.dat` exists,
- server jar exists or distribution marker exists,
- directory is readable,
- directory is not already imported.

Discovery should populate `server_discovery_candidate`.

The UI should show:

- discovered name,
- path,
- detected distribution,
- detected version if known,
- import eligibility,
- why a candidate is invalid if it failed validation.

## Status And Player Count Plan

The agent should maintain observed runtime state through a reconcile loop.

Suggested loop:

- every 15 seconds while any instance is running,
- every 60 seconds when everything is stopped.

For running instances:

- read systemd active state,
- read last exit state if relevant,
- run a Minecraft status ping,
- update `current_player_count`,
- update `health_state`,
- append a durable event when important state changes.

This is enough for:

- “how many players are online now,”
- “is this instance ready,”
- “did it crash,”
- “should the UI show start or stop.”

## Logging Plan

Use two layers:

### Full Logs

Source of truth:

- journald for `rustyfin-minecraft-<instance-id>.service`

The agent should expose a log tail endpoint that reads journal lines for the unit and returns:

- timestamp,
- level if parseable,
- line text,
- cursor for pagination.

### UI History

Do not try to store full logs in PostgreSQL.

Only store durable summarized lifecycle events in `server_instance_event`.

That keeps the DB useful without turning it into a log warehouse.

## Permission Model

Use a two-level model:

### Global

- Rustyfin `admin` can do everything.

### Per Instance

- `viewer`
- `operator`
- `manager`

This is enough for the first version.

Do not over-design with dozens of bitwise flags at this stage.

## Security And Trust Boundaries

This feature creates a new privileged surface, so hardening needs to be explicit.

Rules:

- only the servers agent touches systemd,
- only the servers agent touches managed instance roots,
- backend validates user permissions before any agent call,
- agent validates canonical paths before import or write,
- agent rejects paths outside configured discovery/import roots,
- unit names are generated by Rustyfin, never user-supplied,
- JVM flags exposed to users must be allowlisted,
- server distribution downloads must come from known providers only,
- instance roots must be canonicalized and deduplicated,
- delete-file operations must require a second confirmation,
- no arbitrary shell commands from the UI.

## Recommended Settings And Environment Variables

Add a small servers configuration surface.

Recommended variables:

- `RUSTFIN_SERVERS_ENABLE=1`
- `RUSTFIN_SERVERS_AGENT_BIND=127.0.0.1:9472`
- `RUSTFIN_SERVERS_AGENT_TOKEN=<secret>`
- `RUSTFIN_SERVERS_INSTANCE_ROOT=/srv/rustyfin-servers/minecraft/instances`
- `RUSTFIN_SERVERS_ARTIFACT_CACHE_ROOT=/var/cache/rustyfin-servers/minecraft/artifacts`
- `RUSTFIN_SERVERS_DISCOVERY_ROOTS=/srv:/home:/opt`
- `RUSTFIN_SERVERS_DEFAULT_JAVA=/usr/bin/java`

These should be documented as Debian 12-native settings, not Docker-only settings.

## Concrete Delivery Phases

### Phase 1. Data Model And Shared Types

Build:

- migration `029_servers_minecraft.sql`,
- `crates/servers-core`,
- `crates/db/src/repo/servers.rs`,
- server-side domain types.

Exit criteria:

- instances can be created in DB only,
- configs validate,
- permissions validate,
- discovery rows can be stored.

### Phase 2. Native Agent And Runner

Build:

- `crates/servers-agent`,
- `crates/servers-runner`,
- systemd D-Bus integration,
- manifest rendering,
- unit rendering,
- local provisioning.

Exit criteria:

- agent can provision a managed instance directory,
- agent can generate and install a unit,
- runner can start Java,
- stop is graceful,
- journald logs are readable.

### Phase 3. Public Rustyfin APIs

Build:

- `/api/v1/servers/minecraft/...` endpoints,
- internal agent client,
- job creation and durable lifecycle events,
- permission checks.

Exit criteria:

- backend can create, import, start, stop, restart, list, and inspect instances.

### Phase 4. Servers UI

Build:

- top nav `Servers`,
- instance list,
- detail dashboard,
- create wizard,
- import wizard,
- logs and events panel,
- access control panel.

Exit criteria:

- the full lifecycle is user-operable from the UI.

### Phase 5. Discovery, Reconciliation, And Status

Build:

- discovery scans,
- reconcile loop,
- player count probing,
- health state updates,
- better error summaries.

Exit criteria:

- imported and running servers stay accurately represented in the UI.

### Phase 6. Hardening And Fit-And-Finish

Build:

- destructive confirmations,
- better error messages,
- audit log integration,
- Debian 12 native install docs,
- test coverage,
- operational recovery docs.

Exit criteria:

- feature is production-safe enough to operate on a home server without manual babysitting.

## Testing Plan

### Unit Tests

Add tests for:

- path validation,
- discovery heuristics,
- config validation,
- memory bounds,
- unit rendering,
- manifest rendering,
- permission checks.

### Agent Integration Tests

Use trait-backed supervisor abstractions so unit tests can run without real systemd.

Test:

- create flow,
- import flow,
- start flow,
- stop flow,
- restart flow,
- crash reconciliation.

### Debian 12 Manual E2E

Run on a real Debian 12 host with:

- `openjdk-21-jre-headless`,
- PostgreSQL,
- native Rustyfin backend,
- native servers agent,
- one Vanilla server,
- one Paper server,
- one imported existing server directory.

Verify:

- create works,
- import works,
- start/stop works,
- player counts update,
- logs show,
- permissions are enforced,
- reboots do not corrupt state.

## Recommended First-Cut Constraints

To keep the first implementation sane:

- ship Vanilla and Paper only,
- support adopt-in-place import only,
- keep logs read-only,
- keep backups out of the first milestone,
- keep modpack automation out of the first milestone,
- keep servers feature enabled only for native Debian 12 installs in v1.

That produces a smaller, correct core.

## Follow-On Extensions After The Core Ships

Once the first implementation is stable, the next layers should be:

1. Fabric/Forge/NeoForge support.
2. World template import and “story map” creation.
3. Managed mods/plugins install surface.
4. Backup and restore UI.
5. Stop-when-empty automation.
6. Scheduled start windows.
7. Other game tabs using the same servers framework.

## Final Recommendation

The project should treat Minecraft serversing as a **native Debian 12 Rust capability**, not as another Docker-managed sidecar.

The concrete build path should be:

- PostgreSQL as durable state,
- a new Rust servers schema,
- a new Rust host agent,
- a new Rust per-instance runner,
- `systemd` as the long-lived supervisor,
- a new `Servers` page in the UI,
- adopt-in-place import for existing servers,
- Vanilla and Paper first,
- everything operated from Rustyfin’s UI.

That is the most coherent path if the priorities are:

- lower orchestration overhead,
- Debian-native behavior,
- Rust-first control,
- future extensibility to more games,
- and a UI that remains the single control surface.
