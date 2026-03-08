# Rustyfin Servers Minecraft Rust-Native Design

Date: 2026-03-08

## Context

This document is a refinement of the earlier Minecraft servers design.

New requirement:

- move away from Docker as the runtime model,
- keep Rustyfin Rust-first,
- keep PostgreSQL,
- keep the Rustyfin UI as the main control surface,
- make the backend control plane as close to fully Rust-managed as practical.

The provided GitHub PAT was tested and returned `401`, so this design does not yet include direct inspection of the private home-server repository.

## Short Answer

Yes. Rustyfin can support Minecraft hosting without Docker and still keep:

- start/stop controls,
- guided server creation,
- existing server discovery/import,
- logs,
- status,
- permissions,
- PostgreSQL-backed state,
- Rust-first orchestration.

The best runtime model for Debian 12 headless is:

- Rustyfin UI + Rust backend,
- a dedicated Rust servers agent,
- per-server Minecraft Java processes managed by `systemd`,
- PostgreSQL for metadata and permissions,
- journald plus structured Rust events for lifecycle visibility.

That is the cleanest non-Docker design.

## Recommended Architecture

### Control Plane

Keep the control plane in Rust.

Recommended components:

- `rustfin`:
  - UI-facing REST API,
  - auth and authorization,
  - PostgreSQL persistence,
  - job tracking,
  - audit logging,
  - SSE/WebSocket status fanout.
- `rustfin-servers-agent`:
  - new Rust service,
  - host-local orchestration,
  - Minecraft instance provisioning,
  - systemd integration,
  - file layout validation,
  - log streaming,
  - health probes.

### Runtime Plane

Do not run Minecraft inside Docker containers.

Run each Minecraft server as:

- one Java server process,
- supervised by `systemd`,
- with a dedicated working directory,
- with a dedicated service unit,
- with explicit memory and restart limits.

This keeps the actual game server runtime close to bare-metal Linux while preserving lifecycle control.

## Why `systemd` Is The Right Fit

For Debian 12 headless, `systemd` is the right process supervisor.

It gives you:

- start/stop/restart semantics,
- boot-time auto-start if desired,
- no orphaned child processes,
- journald log capture,
- CPU and memory accounting,
- restart policies,
- easy status inspection,
- one clean unit per Minecraft instance.

This is better than having Rustyfin keep long-lived child processes directly under `tokio::process`, because raw child-process supervision becomes fragile when:

- Rustyfin restarts,
- the servers agent restarts,
- the host reboots,
- multiple game servers are active,
- you want durable logs and resource accounting.

## Why Not Raw Direct Process Management Only

Rust can absolutely spawn and stop Java processes directly, but that should not be the final supervisor.

Problems with direct-only process ownership:

- process ownership dies with the agent unless you reimplement supervision,
- restart-on-failure becomes custom logic,
- boot integration becomes custom logic,
- resource controls become weaker,
- logs become harder to centralize cleanly,
- PID tracking becomes more fragile.

The right split is:

- Rust decides what should run,
- `systemd` owns the long-lived process lifecycle.

## Performance Reality

If the goal is maximum home-server performance, moving away from Docker is reasonable, but the gain should be understood correctly.

The biggest benefits are:

- simpler runtime path,
- no image/build/pull overhead,
- no container-layer filesystem indirection,
- no Docker daemon dependency,
- cleaner host-level service management,
- fewer moving parts.

What it probably will not give you is a dramatic Minecraft tick-performance jump by itself. The Minecraft server is still a Java process, and that is where most of the runtime cost lives.

So the non-Docker design is still the right one if your priorities are:

- operational control,
- simplicity,
- licensing avoidance,
- lower orchestration overhead,
- Debian-native hosting.

## What “Rust-First” Means Here

This feature cannot be literally all-Rust end-to-end because Minecraft Java Edition runs on Java.

The realistic meaning of “Rust-first” is:

- UI and control logic in Rustyfin remain Rust and TypeScript,
- all orchestration logic is Rust,
- provisioning logic is Rust,
- lifecycle control is Rust,
- validation is Rust,
- status probing is Rust,
- import/discovery is Rust,
- the actual Minecraft server runtime remains Java.

That is the correct boundary.

## Recommended Servers Page Behavior

Add a top-level nav item:

- `Servers`

Minecraft-first page behavior:

- list all Minecraft instances,
- show running state,
- show version and server type,
- show host and port,
- show current player count if available,
- show recent logs,
- allow start,
- allow stop,
- allow restart,
- allow create,
- allow import existing server,
- later allow backup, template import, mods, and tuning.

## UI Layout Recommendation

### Main Navigation

Add:

- `Servers`

to the same nav area that currently holds:

- Libraries
- Channels
- Rooms
- Calendar
- Admin

### Servers Page

Recommended three-column layout:

- left:
  - server list,
  - search,
  - filter by state,
  - quick start/stop action.
- center:
  - selected server dashboard,
  - status,
  - logs,
  - recent events,
  - player count,
  - address and connection details.
- right:
  - create/import/configure panel,
  - permissions,
  - memory and world settings,
  - content options.

This fits the current Rustyfin style well because the product already uses:

- tabbed control bars,
- split content panes,
- event-driven status updates,
- explicit admin actions.

## Recommended Rust Components

### New Crates

Recommended new crates:

- `crates/servers-core`
  - shared domain types,
  - typed Minecraft config models,
  - validation logic,
  - instance state enums.
- `crates/servers-agent`
  - host-local orchestration agent,
  - systemd integration,
  - process inspection,
  - log collection,
  - probes,
  - provisioning.

Optional later split:

- `crates/servers-minecraft`
  - Minecraft-specific provisioning,
  - version/provider logic,
  - server property generation,
  - ping/status parser.

## Debian 12 Runtime Model

### Filesystem Layout

Recommended root:

- `/srv/rustyfin-servers/minecraft/<instance-id>/`

Per-instance structure:

- `server/`
- `worlds/`
- `downloads/`
- `logs/`
- `backups/`
- `config/`

Suggested files:

- `server/server.jar`
- `server/eula.txt`
- `server/server.properties`
- `config/rustyfin-instance.json`

### systemd Unit Model

Use one unit per instance.

Example shape:

- `rustyfin-minecraft@<instance-id>.service`

The unit should:

- set the working directory,
- point to the chosen Java binary,
- set minimum and maximum memory,
- run the server jar directly,
- redirect logs into journald,
- define restart behavior,
- run under a dedicated service user if practical.

Important detail:

- the agent should generate the `ExecStart` arguments directly,
- no shell wrapper should be required for normal start.

That keeps the trust boundary tighter.

## How Rust Controls systemd

Rustyfin should not call `systemctl` with free-form strings from the UI.

Use a Rust integration path that is typed and constrained.

Recommended approach:

- the servers agent talks to `systemd` over D-Bus,
- actions are limited to:
  - create/update known unit,
  - start known unit,
  - stop known unit,
  - restart known unit,
  - read unit state,
  - tail logs,
  - read resource counters.

This is preferable to shelling out to `systemctl`.

Fallback if needed:

- constrained subprocess calls to `systemctl` with argv-only execution,
- but D-Bus is the better end-state.

## Minecraft Creation Flow

### Phase 1 Supported Server Types

To keep the first implementation realistic, phase 1 should support:

- Vanilla
- Paper

Reason:

- these cover the common base case and one high-value optimized server path,
- Fabric/Forge/NeoForge add complexity in installer and dependency handling,
- those can be added once the base lifecycle and import model are proven.

### Create Wizard Fields

Recommended phase 1 fields:

- display name,
- world name,
- server type:
  - Vanilla,
  - Paper,
- Minecraft version,
- game mode,
- difficulty,
- hardcore on/off,
- max players,
- memory min,
- memory max,
- optional seed,
- MOTD,
- host port,
- auto-start on boot on/off,
- auto-stop when empty on/off.

### Creation Steps

1. Validate requested configuration in Rust.
2. Allocate instance ID and reserved paths.
3. Allocate or validate host port.
4. Download server artifact.
5. Write `eula.txt` and `server.properties`.
6. Write Rustyfin-managed instance metadata file.
7. Generate or update systemd unit.
8. Persist instance row in PostgreSQL.
9. Optionally start the instance.
10. Stream job updates back to the UI.

## Existing Server Import

This matters because you already have working Minecraft worlds and running servers.

### Import Targets

Support importing:

- an existing Minecraft server directory,
- an existing systemd unit if one already exists,
- an existing raw Java server layout that Rustyfin can adopt.

### Import Validation

The agent should validate:

- directory exists,
- canonical path is inside configured import roots,
- `server.properties` is present or inferable,
- world directory exists,
- jar/runtime path is discoverable,
- port can be parsed,
- existing launch parameters can be normalized into Rustyfin’s config model.

### Import Modes

Two import modes are useful:

- `adopt-in-place`
  - keep the files where they already are,
  - create Rustyfin metadata and lifecycle control around them.
- `migrate-into-managed-root`
  - copy or move the server into `/srv/rustyfin-servers/...`,
  - then manage it there.

Recommendation:

- implement `adopt-in-place` first,
- add `migrate-into-managed-root` later as an explicit admin action.

## Data Model

Add dedicated tables.

### `server_instance`

Suggested fields:

- `id`
- `game_kind`
- `display_name`
- `slug`
- `runtime_kind`
  - `systemd`
- `state`
- `managed_mode`
  - `managed`
  - `imported`
- `host`
- `port`
- `instance_root`
- `unit_name`
- `created_by_user_id`
- `last_error`
- `created_ts`
- `updated_ts`
- `last_started_ts`
- `last_stopped_ts`

### `minecraft_server_config`

Suggested fields:

- `instance_id`
- `server_type`
- `mc_version`
- `world_name`
- `game_mode`
- `difficulty`
- `hardcore`
- `motd`
- `seed`
- `max_players`
- `memory_min_mb`
- `memory_max_mb`
- `java_path`
- `jar_path`
- `auto_start`
- `auto_stop_when_empty`
- `auto_stop_idle_minutes`
- `properties_json`

### `server_instance_permission`

Suggested fields:

- `instance_id`
- `user_id`
- `can_view`
- `can_start_stop`
- `can_configure`
- `can_delete`

### `server_instance_event`

Suggested fields:

- `id`
- `instance_id`
- `event_kind`
- `payload_json`
- `created_ts`

## API Shape

Recommended namespace:

- `/api/v1/servers/...`

Phase 1 endpoints:

- `GET /api/v1/servers/instances`
- `GET /api/v1/servers/instances/{id}`
- `POST /api/v1/servers/instances`
- `PATCH /api/v1/servers/instances/{id}`
- `POST /api/v1/servers/instances/{id}/start`
- `POST /api/v1/servers/instances/{id}/stop`
- `POST /api/v1/servers/instances/{id}/restart`
- `POST /api/v1/servers/instances/import`
- `GET /api/v1/servers/instances/{id}/logs`
- `GET /api/v1/servers/instances/{id}/status`
- `GET /api/v1/servers/discovery/minecraft`

## Status And Logs

This should be live, not static.

Recommended sources:

- PostgreSQL instance state for durable status,
- agent heartbeats for live state,
- journald tail for server logs,
- Rustyfin job table for long-running actions.

The user should be able to see:

- provisioning,
- download progress,
- starting,
- online,
- stopping,
- stopped,
- failed,
- recent log lines,
- last error.

## Player Count And Auto-Stop

This should be handled by the Rust servers agent.

Recommended mechanism:

- implement Minecraft status ping in Rust,
- periodically query each running server,
- record:
  - online/offline,
  - version,
  - player count,
  - max player count,
  - MOTD.

For auto-stop:

- if player count is zero for a configured idle window,
- and no pending admin action exists,
- the agent stops the server gracefully.

This avoids leaving idle Minecraft worlds consuming RAM.

## Security Model

This feature crosses a real host trust boundary, so it must stay strict.

### Hard Rules

- No arbitrary shell commands from user input.
- No arbitrary unit names from user input.
- No arbitrary Java args from user input in phase 1.
- No arbitrary filesystem paths outside approved import roots.
- All generated units must come from Rust-owned templates.
- All start/stop actions must target known instance IDs only.
- All privileged host actions must live in the servers agent, not the main API.

### Admin/User Boundaries

Recommended initial permission model:

- all authenticated users:
  - may view only servers they are allowed to see.
- delegated users:
  - may start and stop only assigned servers.
- admins:
  - create,
  - import,
  - edit,
  - delete,
  - assign permissions.

## Legal And Packaging Angle

If one of your reasons for avoiding Docker is packaging/licensing concern, the non-Docker design cleanly removes Docker from the servers subsystem entirely.

That means the servers feature can depend on:

- Rust,
- PostgreSQL,
- Java,
- systemd on Debian 12,

without requiring Docker for game-server runtime control.

## Best Recommendation

For your stated goals, the best design is:

- Rustyfin UI in Next.js,
- Rustyfin API in Rust,
- PostgreSQL for instance/config/permission state,
- a new Rust servers agent,
- Debian 12 `systemd` for actual Minecraft server supervision,
- Minecraft Java server processes running directly on the host,
- no Docker dependency for the Minecraft runtime path.

This gives you the interactive UI you want while keeping the backend orchestration strongly Rust-first and Debian-native.

## Recommended Implementation Order

### Phase 1

- add `Servers` nav item and page,
- add PostgreSQL schema,
- add Rust servers agent,
- add list/detail/start/stop/status/log APIs,
- add journald-backed live logs,
- add Vanilla and Paper creation.

### Phase 2

- add import of existing server directories and existing units,
- add player count probes,
- add auto-stop when empty,
- add better error/status surfacing.

### Phase 3

- add Fabric,
- add mod and datapack management,
- add world template import,
- add backup/restore hooks.

## Open Questions For The Next Pass

- Where exactly are the existing Minecraft server directories stored on the host?
- Are the current servers already represented as `systemd` services, raw Java commands, or something else?
- Do you want Rustyfin to manage Java installation too, or only manage server instances once Java exists?
- Do you want only admins to create servers, or should trusted users also be able to create them?
- What port range should be reserved for managed Minecraft instances?
- Should import keep servers in place or migrate them into a Rustyfin-managed root by default?

