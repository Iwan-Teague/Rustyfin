# Rustyfin Servers Minecraft Design

Date: 2026-03-08

## Purpose

This document defines the first design pass for adding a new top-level `Servers` area to Rustyfin.

Scope for this pass:

- Minecraft only.
- No implementation yet.
- Focus on:
  - listing existing Minecraft servers,
  - starting and stopping servers,
  - creating new servers through a guided UI,
  - keeping Rustyfin Rust-first,
  - avoiding a Python-based control plane.

## What Was Examined

Rustyfin-side code inspected for this design:

- Top navigation and page entry pattern:
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/NavBar.tsx`
- Existing tabbed UI patterns:
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/rooms/components/RoomModeTabsBar.tsx`
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/rooms/components/WatchSourceTabsBar.tsx`
- Server shared state and event bus:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/state.rs`
- Existing job/audit patterns:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/jobs.rs`
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/audit_log.rs`
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- Existing host filesystem browsing:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/host_directories.rs`
- Existing room creation/reconfiguration flow:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`

External home-server code status:

- The provided GitHub token returned `401`, so the private home-server repository could not be inspected from this session.
- Public GitHub repo discovery for `Iwan-Teague` did not expose a public home-server repository containing the Minecraft stack.
- SSH-based inspection of the Ubuntu server did not return usable command output from this environment.

Result:

- This design is grounded in the actual Rustyfin codebase and the previously known Debian/Docker deployment model.
- A second pass should be done once the private home-server repo or server-side Minecraft management code is directly inspectable.

## Current Rustyfin Capabilities We Should Reuse

Rustyfin already has several building blocks that fit this feature well:

- A centralized top nav where `Servers` can be added cleanly.
- PostgreSQL-backed state and repository patterns already used for rooms, libraries, and jobs.
- A broadcast event system that can carry lifecycle updates such as `starting`, `running`, `stopped`, `error`, and `provisioning`.
- An existing `job` model that can back long-running work such as `create server`, `import existing server`, `download modpack`, and `stop server`.
- A host-directory browsing model that already safely exposes server-side directories under configured roots.
- A sidecar-agent pattern already used by:
  - `rustfin-youtube-agent`
  - `rustfin-transcription-agent`

This means the servers feature should not be built as ad hoc shell calls from the main API. It should follow the same split:

- Rustyfin main server:
  - UI-facing API,
  - auth,
  - authorization,
  - persisted metadata,
  - audit trail,
  - job tracking.
- Dedicated Rust servers agent:
  - host/runtime control,
  - server process lifecycle,
  - Docker interaction,
  - filesystem provisioning,
  - log collection,
  - status probes.

## Product Goal

The `Servers` area should let a user do four things for Minecraft:

1. See all known Minecraft servers.
2. See whether each server is running, stopped, provisioning, or failed.
3. Start and stop a server without leaving Rustyfin.
4. Create a new server from a guided form instead of editing raw files manually.

Secondary goal:

- Later support Minecraft content variants such as Paper, Fabric, Forge, NeoForge, modpacks, datapacks, and world/adventure templates.

## Recommended Architecture

### Summary

Use a Rust control plane and Docker runtime model.

Recommended components:

- `rustfin`:
  - owns database records,
  - exposes REST APIs,
  - authorizes requests,
  - writes audit logs,
  - tracks jobs,
  - consumes status updates from the servers agent.
- `rustfin-servers-agent`:
  - new Rust service,
  - runs on the same private Docker network as Rustyfin,
  - has the only access to Docker runtime control,
  - has restricted access to the configured servers root on the host.
- Docker containers for each Minecraft instance:
  - one container per server,
  - labeled for discovery,
  - mounted to a dedicated per-instance directory,
  - exposed on a configured host port.

### Why This Is The Best Fit

This is the cleanest practical path because:

- Rustyfin is already Docker-first.
- Minecraft servers are operationally easiest to isolate as one server per container.
- The main backend should not own Docker socket access directly.
- The servers agent pattern matches how Rustyfin already isolates specialized runtime concerns.
- Existing Python orchestration can be replaced by a smaller Rust-only control surface without forcing Rustyfin to become a general-purpose init system.

## Runtime Recommendation For Minecraft

### Phase 1 Runtime

For the actual Minecraft server runtime, use a containerized Minecraft image and wrap it with Rust-owned orchestration.

Recommended initial runtime target:

- Docker-managed Minecraft containers using a stable server image with broad feature coverage.

The key design point is this:

- Rustyfin should own the control plane.
- It does not need to reimplement the Minecraft server process bootstrap itself on day one.

That means:

- No Python management layer.
- No shell-script-heavy management path in Rustyfin.
- The servers agent uses a Rust Docker client and strict templates to manage instance containers.

### Why Not Build The Minecraft Runtime From Scratch In Rust

That would be the wrong first step.

Minecraft server hosting needs:

- Java runtime management,
- version downloads,
- server jar selection,
- mod loader handling,
- EULA handling,
- world persistence,
- optional modpack/template flows.

Rebuilding all of that immediately would be slow, brittle, and unnecessary.

The correct boundary is:

- Rustyfin in Rust:
  - lifecycle,
  - permissions,
  - provisioning decisions,
  - UI,
  - metadata,
  - logs,
  - validation.
- Minecraft runtime:
  - isolated containerized workload.

## Proposed User Experience

## Top-Level Navigation

Add a new entry in the main nav:

- `Servers`

Visibility recommendation:

- Visible to all authenticated users.
- Server creation, deletion, configuration edits, and import should be restricted to admins.
- Start/stop can be permission-based per server.

## Servers Page Layout

Recommended page structure:

- Left column:
  - server list,
  - search/filter,
  - status chips,
  - game type badge (`Minecraft`),
  - running/stopped state,
  - current connected players when available.
- Main column:
  - selected server overview,
  - start/stop/restart buttons,
  - address/port,
  - world name,
  - server type,
  - version,
  - memory allocation,
  - quick logs pane,
  - recent events/jobs.
- Right column:
  - configuration panel or create/import wizard entry,
  - permissions,
  - auto-stop settings,
  - content options (later: modpack/template/datapack).

## Core User Flows

### 1. List Existing Servers

The page should show:

- display name,
- runtime state,
- host/port,
- Minecraft version,
- server type,
- world name,
- last started time,
- current player count if available,
- whether the instance is:
  - managed,
  - imported,
  - unhealthy,
  - needs attention.

### 2. Start Server

User clicks `Start`.

Expected flow:

- Rustyfin writes a `servers.minecraft.start` job.
- Rustyfin asks the servers agent to start the instance.
- Agent validates:
  - instance exists,
  - image/runtime template allowed,
  - port not already occupied,
  - filesystem root still valid.
- Agent starts the backing container.
- Agent streams state transitions:
  - `queued`
  - `starting`
  - `running`
  - `error`

### 3. Stop Server

User clicks `Stop`.

Expected flow:

- Rustyfin writes a `servers.minecraft.stop` job.
- Agent sends graceful stop first.
- If RCON is configured, optionally warn players first later.
- If graceful stop exceeds timeout, force stop.
- State transitions:
  - `stopping`
  - `stopped`
  - `error`

### 4. Create New Server

Admin clicks `Create Server`.

Initial Minecraft wizard should capture:

- display/server name,
- world name,
- Minecraft version,
- server distribution:
  - Vanilla,
  - Paper,
  - Fabric,
  - Forge,
  - NeoForge.
- game mode:
  - Survival,
  - Creative,
  - Adventure,
  - Spectator.
- difficulty:
  - Peaceful,
  - Easy,
  - Normal,
  - Hard.
- hardcore:
  - on/off.
- max players.
- memory allocation:
  - min memory,
  - max memory.
- optional seed.
- MOTD.
- whitelist on/off.
- public host port.
- auto-stop when empty:
  - disabled,
  - enabled after N idle minutes.

Later fields:

- world template/adventure map import,
- datapack upload,
- mod loader pack,
- curated modpack selection,
- ops list,
- view distance,
- simulation distance.

### 5. Import Existing Server

Admins should be able to import an already existing Minecraft server instead of recreating it.

Supported import models:

- Import by existing labeled Docker container.
- Import by directory containing a known server structure:
  - `server.properties`,
  - `eula.txt`,
  - world data,
  - logs,
  - mods/config directories when present.

Import should never blindly take control of arbitrary directories.

It should:

- validate the structure,
- copy or register it under the managed servers root,
- create a DB record,
- mark it as `imported`,
- allow the user to review settings before first managed start.

## Data Model

Add dedicated servers tables rather than overloading `job` or `watch_party`.

### `server_instance`

Suggested fields:

- `id`
- `game_kind`
  - `minecraft`
- `display_name`
- `slug`
- `runtime_kind`
  - `docker`
- `state`
  - `stopped`
  - `queued`
  - `provisioning`
  - `starting`
  - `running`
  - `stopping`
  - `error`
- `server_type`
  - `vanilla`
  - `paper`
  - `fabric`
  - `forge`
  - `neoforge`
- `mc_version`
- `world_name`
- `host_port`
- `container_name`
- `image_ref`
- `instance_root`
- `managed_mode`
  - `managed`
  - `imported`
- `config_json`
- `last_error`
- `created_by_user_id`
- `created_ts`
- `updated_ts`
- `last_started_ts`
- `last_stopped_ts`

### `server_instance_permission`

Suggested fields:

- `instance_id`
- `user_id`
- `can_view`
- `can_start_stop`
- `can_configure`
- `can_delete`

This allows:

- admin creates/edits,
- trusted users start/stop,
- broader users only view status.

### `server_instance_event`

Suggested fields:

- `id`
- `instance_id`
- `event_kind`
- `payload_json`
- `created_ts`

This is distinct from the generic `job` table. Jobs track actions. Instance events track lifecycle history.

### `server_instance_secret`

Needed later if Rustyfin manages:

- RCON password,
- operator secret material,
- provider tokens for curated content sources.

These must not be stored in plain text in the main record.

## API Design

Recommended new API namespace:

- `/api/v1/servers/...`

### Core Endpoints

- `GET /api/v1/servers/instances`
  - list visible instances.
- `GET /api/v1/servers/instances/{id}`
  - get details.
- `POST /api/v1/servers/instances`
  - create new instance.
- `PATCH /api/v1/servers/instances/{id}`
  - update configuration.
- `DELETE /api/v1/servers/instances/{id}`
  - remove record and optionally data.
- `POST /api/v1/servers/instances/{id}/start`
- `POST /api/v1/servers/instances/{id}/stop`
- `POST /api/v1/servers/instances/{id}/restart`
- `GET /api/v1/servers/instances/{id}/logs`
- `GET /api/v1/servers/instances/{id}/status`
- `POST /api/v1/servers/instances/import`
- `GET /api/v1/servers/discovery/minecraft`
  - list importable Minecraft directories/containers.

### Optional Later Endpoints

- `POST /api/v1/servers/instances/{id}/template`
- `POST /api/v1/servers/instances/{id}/mods`
- `POST /api/v1/servers/instances/{id}/datapacks`
- `GET /api/v1/servers/catalog/minecraft/modpacks`

## Agent Design

Create a dedicated Rust service:

- `crates/servers-agent`

Responsibilities:

- validate instance actions against strict templates,
- provision instance directories,
- write server config files,
- manage Docker containers using a Rust Docker client,
- tail logs,
- run health checks,
- report status back to Rustyfin,
- enforce a limited set of allowed operations.

### The Main Backend Must Not Do This Directly

The main API server should not:

- own Docker socket access,
- run arbitrary shell commands,
- write arbitrary host files outside a controlled root,
- exec into containers directly from user input.

Those are trust-boundary violations.

## Docker Management Model

Use one container per Minecraft instance.

Each managed container should have labels like:

- `com.rustyfin.servers=true`
- `com.rustyfin.game=minecraft`
- `com.rustyfin.instance_id=<uuid>`

These labels enable:

- discovery,
- reconciliation,
- orphan detection,
- import of preexisting managed containers,
- safe filtering so Rustyfin does not touch unrelated containers.

### Filesystem Layout

Recommended root:

- `/srv/rustyfin-servers/minecraft/<instance-id>/`

Inside each instance:

- `data/`
- `logs/`
- `backups/`
- `templates/`
- `config/`

If you want compatibility with an existing host layout, the import flow can register older directories and optionally migrate them into this structure later.

## Minecraft Content Strategy

## Phase 1

Support:

- Vanilla
- Paper
- Fabric
- Forge
- NeoForge

This gives a useful range without overloading the first pass.

## Phase 2

Support:

- curated modpack install flows,
- world template/adventure map import,
- datapack install,
- optional server-type presets.

## Important Product Reality

“Story mode” for Minecraft is not usually a server toggle.

In practice it tends to mean one of:

- an adventure/world template,
- a datapack set,
- a modpack,
- a plugin bundle for Paper-based servers.

So the UI should model content as:

- server type,
- world template,
- datapacks,
- modpack/plugins,

not as a single generic `story mode` boolean.

## Status, Logs, and Realtime Feedback

Rustyfin should show:

- lifecycle state,
- current host/port,
- last action,
- last error,
- recent logs,
- player count,
- idle shutdown countdown if enabled.

Use:

- `job` rows for action tracking,
- `ServerEvent` or a new servers SSE stream for live updates,
- instance event history for auditability.

The user should always know whether a server is:

- provisioning,
- downloading assets,
- starting the JVM,
- listening for connections,
- stopping gracefully,
- force-stopped,
- failed.

## Security Model

This feature needs stricter handling than ordinary room state.

### Hard Requirements

- No arbitrary shell construction from user input.
- No direct Docker socket access from the main Rustyfin API.
- No arbitrary host path mounts.
- All instance roots must live under a configured servers root, except explicit import flows.
- All import flows must validate and canonicalize paths.
- Only whitelisted Minecraft runtime templates may be used.
- Container names, image refs, environment variables, and volume mounts must be generated by the agent, not passed through raw from the UI.
- Every create/start/stop/restart/import/delete/config change must be audit-logged.

### Permissions

Recommended initial policy:

- authenticated users:
  - can view only the instances they have access to.
- delegated users:
  - may start/stop specific instances.
- admins:
  - create,
  - import,
  - edit,
  - delete,
  - grant permissions.

### Secrets

If RCON or similar is enabled later:

- keep secrets out of logs,
- store encrypted-at-rest if persisted,
- never return raw secrets in list/detail APIs after creation.

## Operational Controls

The feature should include sane resource controls from the start.

Per-instance configuration:

- memory min/max,
- CPU advisory limits when supported,
- auto-stop on idle,
- stop timeout,
- startup timeout.

Global controls:

- max simultaneous starting instances,
- max total managed Minecraft instances,
- reserved port range,
- max log retention,
- max backup storage later.

## Existing Server Import Strategy

Because you already have existing Minecraft servers/worlds, import matters as much as create.

Recommended import flow:

1. Agent scans configured import roots and labeled containers.
2. Rustyfin shows candidate servers.
3. Admin selects one to import.
4. Agent validates:
   - files exist,
   - world data present,
   - server properties parse,
   - port and runtime details discoverable.
5. Rustyfin creates a managed record with `managed_mode=imported`.
6. First managed start uses the imported directory but under Rustyfin lifecycle control.

This lets you preserve the existing worlds while replacing the old orchestration layer.

## Recommended Implementation Order

### Phase 1: Foundation

- Add `Servers` nav page.
- Add DB schema for servers instances and permissions.
- Add new Rust servers agent.
- Add list/detail/start/stop APIs.
- Add basic UI page for listing and controlling servers.

### Phase 2: Minecraft Create Flow

- Add guided create form.
- Provision instance directories.
- Create Docker-managed Minecraft instances.
- Stream job/state/log updates to UI.

### Phase 3: Import Existing Servers

- Add directory/container discovery.
- Add import wizard.
- Register existing worlds and servers safely.

### Phase 4: Better Minecraft Content

- Add Paper/Fabric/Forge/NeoForge presets.
- Add world template import.
- Add datapack upload/install.
- Add curated modpack support.

### Phase 5: Smarter Operations

- Auto-stop when empty.
- Player count display.
- Health monitoring.
- Backup/restore hooks.

## Recommendation

Build this as a new Rustyfin subsystem, not as an extension of rooms.

Best shape:

- top-level `Servers` page,
- new servers DB tables,
- new Rust servers agent,
- Docker-managed Minecraft instance containers,
- explicit import support for your existing servers/worlds.

That gives you:

- proper UI,
- proper permissions,
- proper logs,
- proper lifecycle control,
- no Python control plane,
- a clean path to mods, templates, and later non-Minecraft games.

## Open Questions For The Next Pass

These need real answers before implementation starts:

- What exact host path currently holds the Minecraft server roots?
- Are the current Minecraft servers Docker containers, Compose services, raw JVM processes, or something else?
- Do you want Java edition only, or Bedrock later too?
- Do you want all authenticated users to start/stop, or only users explicitly granted control per server?
- What reserved host port range should Rustyfin use for managed game servers?
- Should imported servers stay in place, or be migrated into a Rustyfin-managed root?
- Which modpack/template sources should be supported first:
  - local zip upload,
  - Modrinth,
  - CurseForge,
  - manual URL,
  - world-template import only?

## Immediate Next Step

Before implementation, the next best move is to inspect the existing home-server Minecraft control code directly and map:

- current directory layout,
- current start/stop model,
- current port allocation,
- current data preservation expectations,
- whether the existing servers are already Dockerized.

Once that is available, this design can be tightened into an implementation plan with concrete DB migrations, API contracts, agent interfaces, and UI screens.
