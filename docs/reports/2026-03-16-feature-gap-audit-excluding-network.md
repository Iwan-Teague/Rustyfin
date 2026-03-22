# Feature Gap Audit (Excluding Network)

Date: 2026-03-16
Status: current audit of documented-but-unshipped features

## Scope

This audit reviews the active documentation set and identifies features that are still outlined in docs but are not currently implemented in the repository/runtime.

Reviewed sources:

- `README.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/README.md`
- `docs/plans/2026-03-14-ai-assistant-design.md`
- `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`
- `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
- `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
- `docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
- `docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`

Explicit exclusion for this audit:

- the `/network` page roadmap
- RustyNet topology-map work
- RustyNet mesh grounding work in AI

## Executive Summary

The main documented-but-unshipped feature areas are:

1. Downloads expansion beyond the RustyVault extension
2. Real backup and restore workflows
3. A planned Smart Home surface for linked home devices
4. AI write-capable actions and an optional admin-only assistant mode
5. Deferred Minecraft/Servers expansion features beyond the current Vanilla/Paper scope
6. The cross-distro/productized Linux installer work beyond the current Debian 12/13 path

The docs that are mostly complete from a feature-delivery perspective are:

- RustyVault migration
- setup wizard
- current read-only AI grounding, excluding future write/admin/network work

## Confirmed Missing Features

### 1. Downloads is still not a broader release center

Documented intent:

- `README.md` says Downloads should later carry future first-party applications and companion downloads.
- the design baseline now explicitly calls out Windows, macOS, Linux, Android APK, and iOS as planned Downloads artifacts.

Current reality:

- the shipped Downloads surface is still centered on the RustyVault browser extension
- there is no broader Rustyfin app/download catalog yet

Current evidence:

- `README.md`
- `ui/src/app/downloads/page.tsx`

Missing feature set:

- Windows client downloads
- macOS client downloads
- Linux client downloads
- Android APK downloads
- iOS app distribution/download guidance
- companion downloads beyond the RustyVault extension

### 2. Backup and restore workflows do not exist yet

Documented intent:

- the servers plans call out future backup and restore UI/hooks
- the Backups page is explicitly reserved for future scheduled backups, exports, and recovery tools

Current reality:

- the `/backups` page is a placeholder
- the AI backup summary tool explicitly reports that backup and restore workflows are not implemented

Current evidence:

- `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
- `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
- `ui/src/app/backups/page.tsx`
- `crates/server/src/ai_assistant/tools.rs`

Missing feature set:

- actual backup jobs
- restore workflows
- backup UI
- scheduled backups
- export/recovery tooling

### 3. Smart Home is now planned but entirely unimplemented

Documented intent:

- the main design baseline now defines Smart Home as a future Rustyfin product surface for linked cameras, lights, doors/locks, alarms, and other smart-home devices

Current reality:

- there is no Smart Home page, API family, or device integration surface in the shipped product

Current evidence:

- `docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`

Missing feature set:

- Smart Home page/surface
- linked smart-home device inventory
- security camera visibility/previews
- smart light state/control surface
- door/lock state surface
- alarm-system state surface
- room/zone grouping for smart-home devices
- graceful unavailable/empty-state Smart Home UI

### 4. AI write actions are still intentionally absent

Documented intent:

- the AI architecture tracker still lists write-capable tools as future work
- the same tracker also leaves an optional admin-only assistant mode for later

Current reality:

- AI is still read-only
- there is no confirmation-token workflow or protected-action write path for AI yet

Current evidence:

- `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`

Missing feature set:

- `calendar_create_event`
- `calendar_update_event`
- `room_create`
- `room_invite_user`
- `server_action_start`
- `server_action_stop`
- confirmation-token workflow for AI writes
- protected-action support for high-risk AI writes
- optional admin-only assistant mode

### 5. Minecraft/Servers follow-on features remain unshipped

Documented intent:

- the servers plans explicitly defer a second wave of Minecraft capabilities once the current core is stable

Current reality:

- the current implementation is still the narrower Minecraft core
- the server distribution choices in the current UI/backend remain `vanilla` and `paper`

Current evidence:

- `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
- `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
- `ui/src/app/servers/page.tsx`
- `crates/server/src/servers/handlers.rs`

Missing feature set:

- Fabric support
- Forge support
- NeoForge support
- world template import
- story-map creation
- managed mods/plugins install surface
- modpack automation / one-click modpack installers
- backup and restore UI/hooks for servers
- stop-when-empty automation
- scheduled start windows
- other game tabs on the same servers framework

Deferred but still explicitly outlined in docs:

- Bedrock support
- proxy network support such as Bungee/Velocity
- automatic router port forwarding
- public internet exposure from Rustyfin
- in-browser server console write access
- full mod marketplace management

### 6. The Linux installer is not yet at the full planned scope

Documented intent:

- the installer plan still describes a broader productized Linux installer than the current implementation

Current reality:

- the public installer path is working for Debian 12 and Debian 13
- the broader cross-distro/productized plan is still not delivered

Current evidence:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `README.md`

Missing feature set:

- Ubuntu LTS support
- Fedora / Rocky / Alma / RHEL-compatible support
- Arch support
- openSUSE support
- dedicated `rustyfin` system user / service-account model
- fully standardized install layout under `/opt/rustyfin`, `/etc/rustyfin`, `/var/lib/rustyfin`, `/var/log/rustyfin`, and `/var/cache/rustyfin`
- automated GPU stack installation/persistence policy
- SELinux/firewalld/ufw distro handling
- install from git URL
- install from release tarball
- version pinning
- upgrade channel selection
- rollback path
- shipped standalone installer binary such as `/opt/rustyfin/bin/rustfin-installer`

## Not Missing

These docs describe features or architecture that are already materially implemented, so they should not be counted as current feature gaps:

- RustyVault migration architecture in `docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
- setup wizard API/surface in `docs/setup-wizard/` and `crates/server/src/setup/handlers.rs`
- the current read-only AI assistant architecture and grounded tool set in `docs/plans/2026-03-14-ai-assistant-design.md` and `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`, excluding future write/admin/network work

## Doc Drift Worth Cleaning Up

These are not missing features, but they are documentation mismatches worth fixing later:

- `docs/plans/2026-03-14-ai-assistant-design.md` still has a `Planned Next Step` section that points to grounded server-side tool calling even though that work is already shipped
- `docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md` understates the current setup-wizard scope because the baseline summary omits setup libraries and setup reset even though those routes exist

## Recommended Priority Order

If this audit is turned into delivery work, the highest-value non-network sequence is:

1. real backup/restore workflows
2. Downloads expansion beyond the RustyVault extension
3. Smart Home surface and device integrations
4. AI write-capable actions with proper confirmation/protected-action flow
5. next-wave Minecraft features
6. cross-distro installer expansion

## Bottom Line

Excluding the Network page and RustyNet work, the docs currently point to five real unshipped feature areas:

- broader Downloads artifacts
- Smart Home product surface
- real backup/restore workflows
- AI write/admin mode work
- deferred Minecraft/Servers expansion
- broader Linux installer coverage/productization
