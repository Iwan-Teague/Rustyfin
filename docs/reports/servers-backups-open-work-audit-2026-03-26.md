# Servers/Backups Open Work Audit

Date: 2026-03-26  
Status: still-open work only

## Core Backup/Restore Work (Still Open)

### Problem: No backup control plane (API + jobs + persistence)
Why it is open: there is no backup backend surface yet. The Backups UI is a placeholder and the assistant backup tool is hard-coded to "not implemented."  
Key files/modules: `ui/src/app/backups/page.tsx`, `crates/server/src/ai_assistant/tools.rs`, `crates/server/src/servers/router.rs`  
Recommended implementation approach: add a dedicated backup module in `crates/server` with `POST/GET` backup job endpoints, backup metadata tables in `crates/db/migrations_pg`, and host execution adapters for filesystem + PostgreSQL backup capture. Reuse existing `job` lifecycle and audit logging patterns already used in Servers operations.  
Dependencies: backup manifest schema, storage target configuration (local path/object store), permissions model (admin-only operations), host-level execution identity.  
What done looks like: backup jobs can be created/listed/inspected from API, state persists in DB, and UI can render real backup history and status.

### Problem: No restore workflow with safety boundaries
Why it is open: no restore endpoint, no restore job flow, and no staged safety checks exist; only a `backups/` directory is created during server provisioning.  
Key files/modules: `crates/servers-host/src/lib.rs` (writes `backups/` dir only), `ui/src/app/backups/page.tsx`, `crates/server/src/servers/router.rs`  
Recommended implementation approach: implement restore as a staged job (`validate -> preflight -> stop/quiesce -> restore -> verify -> optional rollback`) with strict manifest checksum verification and protected confirmations for destructive restore actions.  
Dependencies: backup manifest/checksum design, service coordination policy (Rustyfin + managed server downtime), rollback strategy.  
What done looks like: admins can run a restore from an existing snapshot with auditable checkpoints and deterministic rollback behavior on failure.

### Problem: No schedule/retention execution for backups
Why it is open: backup scheduling and pruning are referenced in UI copy but there is no backup scheduler task in server runtime.  
Key files/modules: `ui/src/app/backups/page.tsx`, `crates/server/src/main.rs`  
Recommended implementation approach: add a periodic backup scheduler (same style as existing TMDB scheduler loop) that evaluates backup policies, enqueues jobs, and prunes old snapshots by retention rules.  
Dependencies: policy storage table(s), time-window semantics (timezone/clock drift behavior), retention rules per backup scope.  
What done looks like: scheduled backup runs are visible in jobs/events, retention cleanup occurs automatically, and missed/failed runs are surfaced in UI and audit logs.

### Problem: AI backup reporting is a stub
Why it is open: assistant backup summary always returns `configured=false` and `restore_supported=false` regardless of host state.  
Key files/modules: `crates/server/src/ai_assistant/tools.rs`  
Recommended implementation approach: make `system_get_backup_summary` read real backup policy + recent backup metadata from DB/service state and return actual capability/health values.  
Dependencies: completed backup metadata model and backup service endpoints.  
What done looks like: AI backup responses reflect real runtime state (configured status, last successful backup timestamp, restore capability).

## Core Servers/Minecraft Work (Still Open)

### Problem: Per-instance membership management surface is missing
Why it is open: membership tables exist, but there are no server routes/UI for listing, granting, revoking, or editing server memberships.  
Key files/modules: `crates/db/migrations_pg/029_servers_minecraft.sql` (`server_instance_member`), `crates/server/src/servers/router.rs` (no members routes), `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: add repo methods for member CRUD in `crates/db/src/repo/servers.rs`, add `/members` routes in `crates/server/src/servers/router.rs`, and add a member-management panel in `ui/src/app/servers/page.tsx`.  
Dependencies: role enum normalization/migration, user search/selection UX for assignment.  
What done looks like: admins can assign and revoke per-server roles through UI/API, and permissions are enforced in lifecycle/config/delete actions.

### Problem: Role model drift (`operator` behavior not implemented cleanly)
Why it is open: docs define `viewer/operator/manager`, but control checks currently allow only `manager` (plus owner/admin), and DB role field is unconstrained.  
Key files/modules: `crates/server/src/servers/handlers.rs` (`can_control_server`), `crates/db/migrations_pg/029_servers_minecraft.sql` (`role TEXT`), `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: enforce a strict role enum at persistence boundary and update control logic so `operator` can run start/stop/restart while `manager` keeps higher-privilege config/delete/member actions.  
Dependencies: one migration to normalize existing role values and update tests.  
What done looks like: documented role matrix and runtime authorization behavior match exactly.

### Problem: Discovery persistence table is unused
Why it is open: `server_discovery_candidate` exists in schema but scan results are currently transient responses; no persistence/update lifecycle is implemented.  
Key files/modules: `crates/db/migrations_pg/029_servers_minecraft.sql` (`server_discovery_candidate`), `crates/server/src/servers/handlers.rs` (`scan_minecraft_discovery_candidates`), `crates/servers-host/src/lib.rs` (`scan_discovery_candidates`)  
Recommended implementation approach: persist scan results on each scan (upsert by canonical path), track `candidate/imported/ignored/invalid` states, and return persisted records for consistent UI.  
Dependencies: new repo methods for discovery rows, imported-instance linkage updates.  
What done looks like: discovery entries survive restarts, are not duplicated, and accurately reflect import state over time.

### Problem: Import mode semantics are incomplete (adopt vs migrate)
Why it is open: current import path copies source into managed root; docs call for explicit import mode support, including adopt-in-place and existing-unit adoption paths.  
Key files/modules: `crates/servers-host/src/lib.rs` (`import_existing_instance`), `crates/server/src/servers/handlers.rs` (`import_minecraft_server`), `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`  
Recommended implementation approach: add explicit import mode parameter and preflight endpoint; support `adopt_in_place` (no file copy) and `copy_to_managed` (current behavior), plus optional existing-systemd-unit adoption flow behind strict validation.  
Dependencies: import preview contract, safe path canonicalization, unit ownership validation.  
What done looks like: users can choose import mode intentionally, preview effects before execution, and preserve existing server layouts when desired.

### Problem: Delete flow has no unregister-only default path
Why it is open: current delete removes DB record and managed files in one operation; docs call for unregister-only default plus explicit destructive delete-files path.  
Key files/modules: `crates/server/src/servers/handlers.rs` (`delete_minecraft_server`), `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: change delete API to mode-based behavior (`unregister` default, `delete_files` explicit), and reflect both paths in UI confirmation with stronger destructive confirmation for file deletion.  
Dependencies: API contract change, UI modal update, audit payload updates.  
What done looks like: safe default unregister works without touching disk, and destructive deletion requires explicit second intent.

### Problem: Runtime reconciliation is still primarily UI-driven polling
Why it is open: docs call for agent reconcile loop cadence, but current state refresh mainly occurs via client polling/explicit refresh calls.  
Key files/modules: `ui/src/app/servers/page.tsx` (polling loop), `crates/server/src/main.rs` (no servers reconcile scheduler), `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: add backend/agent periodic reconcile workers for status + player count + health transitions, then keep UI polling lightweight for freshness, not correctness.  
Dependencies: reconcile cadence policy, event deduplication rules, host probe timeout/failure behavior.  
What done looks like: server state remains accurate without active UI sessions, and UI polling becomes a read optimization rather than the core reconciliation mechanism.

### Problem: Planned runner/typed systemd boundary is not fully realized
Why it is open: design docs prefer typed D-Bus control and a dedicated runner process boundary; current implementation uses subprocess `systemctl` calls and direct Java `ExecStart`.  
Key files/modules: `crates/servers-host/src/lib.rs` (`run_systemctl`, generated unit `ExecStart`), `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`, `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: incrementally add a typed systemd adapter (D-Bus-backed where feasible), then introduce runner boundary only if needed for tighter graceful-stop semantics and richer process telemetry.  
Dependencies: host D-Bus library choice, service-account/runtime hardening policy, migration strategy for existing units.  
What done looks like: lifecycle control is fully typed/validated end-to-end, and runtime process shutdown/restart behavior is deterministic under host restarts and failure conditions.

## Future Minecraft Distribution/Feature Expansion (Still Open)

### Problem: Additional distributions beyond Vanilla/Paper are not implemented
Why it is open: backend and UI currently restrict distribution to `vanilla` and `paper`; follow-on docs call out Fabric/Forge/NeoForge expansion.  
Key files/modules: `crates/server/src/servers/handlers.rs` (`ALLOWED_SERVER_DISTRIBUTIONS`), `ui/src/app/servers/page.tsx` (distribution selector), `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: add distribution provider abstraction in `crates/servers-host` (artifact resolver + launch strategy) and extend validation/UI selection with per-distribution required fields.  
Dependencies: trusted artifact-source policy, loader metadata caching, compatibility matrix and validation tests.  
What done looks like: admins can create/provision/manage Fabric/Forge/NeoForge servers with first-class validation and artifact handling.

### Problem: Mod/plugin/modpack management surface is missing
Why it is open: follow-on docs include managed mods/plugins and modpack automation, but there is no backend/UI pipeline for these operations yet.  
Key files/modules: `ui/src/app/servers/page.tsx`, `crates/servers-host/src/lib.rs`, `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`, `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`  
Recommended implementation approach: add managed content pipeline (upload/install/enable/disable/remove) with distribution-aware target folders and integrity checks, then layer curated modpack install workflows on top.  
Dependencies: package metadata schema, source trust policy/signature checks, rollback strategy for failed installs.  
What done looks like: admins can safely manage server mods/plugins/modpacks from Rustyfin UI with auditable operations.

### Problem: World template/story map workflow is missing
Why it is open: docs call for world template import/story map creation, but current create/import flow only handles generic world naming and full-directory import.  
Key files/modules: `ui/src/app/servers/page.tsx`, `crates/server/src/servers/handlers.rs`, `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: add world-template inventory + import API with preflight metadata checks and safe destination wiring into managed instance creation.  
Dependencies: template storage format, template provenance policy, world compatibility checks.  
What done looks like: creating a server from a template/story world is a first-class guided flow with validation and clear provenance.

### Problem: Auto-stop-when-empty and scheduled start windows are not implemented
Why it is open: schema includes auto-stop fields, but no runtime enforcement scheduler exists; scheduled start windows are also listed as follow-on and absent.  
Key files/modules: `crates/db/migrations_pg/029_servers_minecraft.sql`, `crates/db/src/repo/servers.rs`, `crates/server/src/main.rs`, `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: add operational policy engine in servers backend/agent for idle-stop and start-window execution, with explicit conflict handling for manual user actions.  
Dependencies: policy configuration UX, scheduler semantics, race handling with interactive lifecycle actions.  
What done looks like: idle servers stop automatically when policy says so, scheduled windows start/stop predictably, and policy decisions are visible in events/audit.

### Problem: Multi-game framework is still a placeholder
Why it is open: the Servers UI includes a disabled "More soon" tab and current APIs are Minecraft-specific.  
Key files/modules: `ui/src/app/servers/page.tsx`, `crates/server/src/servers/router.rs`, `crates/servers-agent/src/main.rs`, `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`  
Recommended implementation approach: extract game-agnostic instance lifecycle contracts (`game_kind`, capabilities, actions) and keep Minecraft-specific handlers as one provider implementation under that shared framework.  
Dependencies: stable cross-game domain model, per-game capability negotiation, UI composition model for game-specific settings panels.  
What done looks like: Servers supports at least one non-Minecraft game type without copy-pasting the control plane.
