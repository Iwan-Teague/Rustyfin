# Rustyfin Remaining Work Agent Execution Program

Date: 2026-03-26
Status: execution program for remaining project work

## Purpose

This document is the current execution brief for everything in Rustyfin that is still:

- partially implemented
- placeholder-only
- documented but not built
- operationally incomplete
- validation-incomplete

It is written for AI agents, not for brainstorming. Each workstream below is intended to be actionable with minimal ambiguity.

## Evidence Base

This program is based on the current repo state, active plans, and placeholder/runtime code, including:

- `README.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/README.md`
- `docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`
- `docs/reports/2026-03-16-feature-gap-audit-excluding-network.md`
- `docs/reports/2026-03-26-installer-platform-runtime-open-work-audit.md`
- `docs/reports/servers-backups-open-work-audit-2026-03-26.md`
- `docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
- `docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
- `docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
- `docs/plans/2026-03-14-ai-assistant-design.md`
- `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- current placeholder/runtime files such as:
  - `ui/src/app/network/page.tsx`
  - `ui/src/app/backups/page.tsx`
  - `ui/src/app/downloads/page.tsx`
  - `crates/server/src/downloads.rs`
  - `crates/server/src/ai_assistant/tools.rs`
  - `crates/server/src/servers/handlers.rs`
  - `ui/src/app/servers/page.tsx`

## Current-State Summary

Rustyfin today is already a real product in these areas:

- native Debian 12/13 runtime
- setup/auth/account
- media libraries and playback
- rooms
- channels and transcription
- calendar
- Minecraft server management core
- RustyVault host integration
- initial Downloads surface
- grounded read-only AI assistant

The remaining work is concentrated in six open programs:

1. platform/installer expansion and operational validation
2. downloads and first-party client distribution
3. network surface implementation
4. smart-home product surface implementation
5. host backup/restore and advanced servers work
6. AI write-capable execution and final assistant maturity work

## Supporting Audits

These supporting audits were used to turn the remaining work into an execution-ready program:

- `docs/reports/2026-03-26-installer-platform-runtime-open-work-audit.md`
  - detailed platform, installer, runtime-layout, and validation open work
- `docs/reports/servers-backups-open-work-audit-2026-03-26.md`
  - detailed host backup/restore and advanced Servers/Minecraft open work
- `docs/plans/2026-04-15-backups-media-first-direction.md`
  - user-facing Backups direction for device/media backup, with account archive export as a companion path
- current AI state and code audit from:
  - `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
  - `crates/server/src/ai_assistant/**`
  - `crates/server/src/ai_enabled.rs`
  - `ui/src/app/ai/page.tsx`

## Execution Rules

1. Each agent owns only the files and domains assigned to it.
2. Agents must not create cross-cutting abstractions in other agents’ directories.
3. When behavior changes, update the matching docs in the same change.
4. All placeholder top-level pages should either become functional or remain clearly marked as blocked by another workstream.
5. The supported runtime target remains native Debian 12 and Debian 13 unless a specific task below expands it deliberately.
6. Backend logic should remain Rust-first.
7. AI write actions must not ship without explicit confirmation/protected-action mechanics.

## How To Use This Document

This document is meant to be executed, not interpreted loosely.

Read it in this order:

1. `Execution Rules`
2. your assigned `Agent Prompt`
3. your `Owned Files And Boundaries`
4. your `Detailed Task Inventory`
5. the `Cross-Agent Coordination Rules`

Important:

- the `Detailed Task Inventory` is authoritative
- the later `Problem` sections are supporting rationale and traceability, not a second independent backlog
- if a `Problem` section appears broader than the matching task inventory, follow the task inventory first
- do not expand scope by “helpfully” solving adjacent work owned by another agent

## Mandatory Agent Delivery Contract

Every agent task is incomplete unless it also delivers all of the following:

1. code or docs changes in the owned files only
2. updated docs for any public/runtime/architecture behavior that changed
3. tests or validation for the changed behavior
4. explicit unavailable/degraded/error states for new product/API surfaces
5. removal of placeholder/TODO language in any surface that became real

Every agent output must state:

- what changed
- which files changed
- which tests or validation steps ran
- which tasks remain blocked, if any

## Blocker Protocol

If an agent is blocked:

1. stop at the ownership boundary
2. do not work around the blocker by editing another agent’s domain
3. document the blocker as:
   - blocking task id
   - exact missing dependency
   - owning agent
   - smallest contract needed to unblock
4. only then move to another task that is explicitly marked parallel-safe

## Shared Non-Negotiables

These apply to every agent:

- do not add new top-level product areas beyond those already named in this program
- do not replace a truthful unavailable state with fake placeholder data
- do not leave routes/UI implying functionality that is still absent
- do not add backend logic in languages other than Rust for permanent product/runtime behavior
- do not bypass existing auth, audit, or protected-action patterns for convenience
- do not merge speculative framework work into the current task unless the task explicitly calls for it

## Fixed Naming And Route Conventions

These conventions are fixed for this program so agents do not create competing names or layouts:

- first-party client shells should live under:
  - `clients/desktop-shell/`
  - `clients/mobile-shell/`
- Downloads release-management UI should live inside the existing Admin surface, not as a new top-level route
- Network topology API should use a stable host route under:
  - `/api/v1/network/topology`
- Smart Home device inventory should use stable host routes under:
  - `/api/v1/smart-home/devices`
  - `/api/v1/smart-home/providers`
- Backup and restore APIs should use stable host routes under:
  - `/api/v1/backups`
  - `/api/v1/backups/jobs`
  - `/api/v1/backups/artifacts`
  - `/api/v1/backups/restores`
- AI write-confirmation flows must remain under the existing AI/protected-action surface and must not invent a parallel mutation channel

## Workstream Map

| Agent | Primary Scope | Status |
| --- | --- | --- |
| Agent 1 | Platform, installer, runtime validation, CI, operational hardening | partial |
| Agent 2 | Downloads, first-party clients, Network, Smart Home | mostly not started |
| Agent 3 | Backups, restore, advanced Servers/Minecraft work | partial |
| Agent 4 | AI finalization, write flows, cross-domain assistant integration | partial |

## Dependency Order

1. Agent 1 should establish stronger runtime/install validation first because the other workstreams depend on reliable host behavior.
2. Agent 3 should build backup primitives before Agent 4 attempts backup-aware assistant improvements.
3. Agent 2 should define stable Network and Smart Home backend contracts before Agent 4 integrates those domains deeply into AI.
4. Agent 2 should define the Downloads artifact model before client-release publishing or AI download-detail grounding is expanded.

## Global Acceptance Criteria

This program is complete only when:

- no top-level nav item is still a dead-end placeholder unless explicitly marked as disabled-by-design
- all documented “planned” product areas have either shipped foundations or a narrower, updated plan
- installer/runtime validation is proven on supported hosts
- AI docs reflect shipped reality rather than previous milestones
- remaining open work is genuinely future scope, not undocumented missing core behavior

---

## Agent 1

### Agent Prompt

```text
You are Agent 1 for Rustyfin. Your job is to finish the platform, installer, and runtime-quality work without changing product-surface features owned by other agents.

You own:
- crates/installer/**
- scripts/install_linux.sh
- scripts/rustfin-installer.sh
- scripts/start-native.sh
- scripts/deploy-native.sh
- scripts/install_native_systemd.sh
- scripts/stop-native.sh
- scripts/clean_install.sh
- docs/plans/2026-03-14-linux-bootstrap-installer-design.md
- docs/operations/debian-12-native-runtime.md
- README.md
- AGENTS.md
- CI/runtime validation scripts under scripts/ci/**
- deployment-like validation tasks referenced by docs/reports/rustyvault-migration-blueprint-2026-03-13.md

You must not own:
- ui/src/app/downloads/**
- ui/src/app/network/**
- any new Smart Home surface
- ui/src/app/servers/**
- ui/src/app/backups/**
- crates/server/src/ai_assistant/**

Your goals are:
1. finish the remaining installer/platform gaps documented in the Linux bootstrap plan
2. close runtime validation debt that still exists only in docs
3. reduce repo-relative runtime assumptions and make the installer more productized
4. update docs so they reflect the actual shipped installer/runtime behavior

Do not invent new product features. Focus on install/runtime correctness, packaging, validation, and operational quality.
```

### Owned Files And Boundaries

Primary ownership:

- `crates/installer/**`
- native wrapper scripts in `scripts/`
- runtime/install docs
- CI/runtime validation scripts

Do not edit without a hard blocker:

- `ui/src/app/downloads/**`
- `ui/src/app/network/**`
- `ui/src/app/backups/**`
- `ui/src/app/servers/**`
- `crates/server/src/ai_assistant/**`

### Agent 1 Implementation Decisions

These decisions are fixed for this program so Agent 1 does not need to choose policy on the fly:

- Ubuntu LTS is the only required new supported distro family in this program.
- Fedora/RHEL, Arch, and openSUSE should remain planned follow-on support unless explicitly reassigned later.
- Production installs must move toward canonical host layout; repo-relative runtime behavior should be preserved only as explicit development mode.
- The installer should prefer a shipped `rustfin-installer` binary once available, but `cargo run` remains an acceptable development fallback.
- Runtime/install validation work should be promoted into repeatable gates rather than expanded manual runbooks.

### Detailed Task Inventory

This is the canonical task queue for Agent 1. Execute these items in order unless a dependency below requires a different sequence.

#### Task 1A: Cross-distro installer adapters and package maps

Why open:

- full install is still hard-gated to Debian 12/13
- prerequisite installation is still effectively `apt`-centric

Where:

- `crates/installer/src/main.rs`
- `scripts/install_linux.sh`
- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`

What to do:

1. Split distro support into explicit adapter modules.
2. Land Ubuntu LTS support first.
3. Define package maps, version gates, and repo-refresh hooks per family.
4. Keep unsupported-host failures explicit and early.

Done looks like:

- Debian 12, Debian 13, and Ubuntu LTS work through one public installer path
- support boundaries are adapter-driven, not hard-coded scatter

#### Task 1B: Linux host-policy adapters

Why open:

- there is no dedicated installer phase for SELinux, firewall, repo-policy, or sudo/polkit differences

Where:

- `crates/installer/src/main.rs`
- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`

What to do:

1. Add a host-policy phase in the installer pipeline.
2. Encode distro-specific no-op vs mutate behavior deliberately.
3. Report every policy decision in install output/manifest.

Done looks like:

- host-policy handling is deterministic and reported, not implicit

#### Task 1C: Artifact acquisition, version pinning, and rollback

Why open:

- install/deploy still assume a local git checkout
- upgrades are not yet channel/pin/rollback aware

Where:

- `crates/installer/**`
- `scripts/rustfin-installer.sh`

What to do:

1. Add install-source modes: local checkout, git URL, release artifact.
2. Persist installed version/channel metadata.
3. Add bounded rollback for installer-owned assets and units.
4. Add a first-class shipped `rustfin-installer` binary mode.

Done looks like:

- fresh hosts can install without a pre-cloned repo
- upgrades are pinned and rollback-capable
- product installs do not require `cargo run`

#### Task 1D: GPU inventory and provisioning policy

Why open:

- GPU defaults are still mostly heuristic
- there is no vendor-aware provisioning or manifested inventory

Where:

- `crates/installer/**`
- install manifest generation
- runtime GPU env/default selection

What to do:

1. Record NVIDIA, AMD, and Intel inventory explicitly.
2. Decide when installer-managed driver/runtime provisioning is allowed.
3. Persist chosen backend and rationale in manifest output.
4. Map inventory to AI/transcription/transcode defaults.

Done looks like:

- supported hosts get explicit GPU policy with traceable decisions

#### Task 1E: Canonical production layout and service identity

Why open:

- runtime paths, logs, env snapshots, and service identity still retain repo-relative and invoking-user assumptions

Where:

- `crates/installer/**`
- `scripts/start-native.sh`
- `docs/operations/debian-12-native-runtime.md`

What to do:

1. Move production installs to canonical `/opt`, `/etc`, `/var/lib`, `/var/log`, and `/var/cache` roots.
2. Introduce a dedicated `rustyfin` system user/group.
3. Keep repo-relative behavior as development mode only.
4. Add migration logic for existing hosts.

Done looks like:

- production runtime survives repo relocation or removal
- services run under a deliberate non-human identity

#### Task 1F: Canonical persisted config and secrets model

Why open:

- repo-local runtime env snapshots still act like live config sources

Where:

- `crates/installer/**`
- `scripts/start-native.sh`
- systemd env rendering

What to do:

1. Make `/etc/rustyfin` the authoritative persisted runtime config root.
2. Demote repo-local snapshots to optional debug export only.
3. Update service launch paths to consume canonical config only.

Done looks like:

- runtime boot does not depend on repo-local config artifacts

#### Task 1G: Install reporting and preflight completeness

Why open:

- install reporting is still mostly JSON-manifest only
- preflight checks do not fully fail fast before host mutation

Where:

- `crates/installer/**`
- docs for install/runtime behavior

What to do:

1. Add operator-readable install summary output.
2. Expand manifest fields to cover install decisions.
3. Add explicit disk/network/systemd/package preflight stages.

Done looks like:

- failed installs stop before unsafe host mutation when prerequisites are missing
- operators get both human-readable and machine-readable install outputs

#### Task 1H: Bootstrap boundary and shell-drift cleanup

Why open:

- `install_linux.sh` still bootstraps some unsupported package-manager families before the Rust installer rejects them
- some legacy wrapper scripts still duplicate policy

Where:

- `scripts/install_linux.sh`
- `scripts/install_native_debian.sh`
- `scripts/start-native.sh`
- `crates/installer/**`

What to do:

1. Fail unsupported hosts before partial bootstrap work.
2. Retire or thin remaining shell duplicates.
3. Keep Rust as the single source of truth for policy.

Done looks like:

- unsupported hosts fail immediately and clearly
- wrapper scripts are glue only

#### Task 1I: Installer regression, idempotency, and runtime gates

Why open:

- fresh-install, rerun-idempotence, deploy, and clean-reset coverage are still thinner than the design target

Where:

- `scripts/ci/**`
- installer tests
- Linux host validation automation

What to do:

1. Add fresh-install gates for Debian 12 and Debian 13.
2. Add rerun-idempotence coverage.
3. Add deploy/update and clean-reset coverage.
4. Keep captured diagnostics on failure.

Done looks like:

- installer/runtime regressions are caught by repeatable automation instead of manual SSH investigation

### Supporting Context Only: Expanded Problem Breakdown

The following `Problem` entries are retained for traceability back to the source plans and audits.

They are not a second independent task queue.

### Problem 1.1: Installer support stops at Debian 12/13

Status:

- partial

Why this is still open:

- the public installer works on Debian 12 and Debian 13
- the active plan still calls for Ubuntu LTS first, then Fedora/RHEL, Arch, and openSUSE
- distro adapters are described, not delivered

Where:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `crates/installer/**`
- `scripts/install_linux.sh`

What needs to be done:

1. Add explicit distro-family detection and typed adapter selection inside `crates/installer`.
2. Land Ubuntu LTS support first because it is already in the recommended first support scope.
3. Encode package maps and preflight checks per distro family rather than branching ad hoc in shell.
4. Add unsupported-host failures that are explicit and actionable.
5. Stage Fedora/RHEL, Arch, and openSUSE as the next adapter layer after Ubuntu lands.

How it needs to be done:

- keep shell bootstrap thin
- keep install policy in Rust
- add adapter modules rather than growing one large `main.rs`
- define per-family package names, repo refresh commands, and supported-version checks

Done means:

- Debian 12, Debian 13, and Ubuntu LTS all work through `./scripts/install_linux.sh`
- unsupported hosts fail with a precise reason
- distro adapter logic is inside Rust, not duplicated in shell
- docs stop describing Ubuntu support as planned

### Problem 1.2: Installer still lacks the planned dedicated service-account/layout model

Status:

- partial

Why this is still open:

- the plan still calls for a dedicated `rustyfin` system user
- the plan still calls for fully standardized roots under `/opt/rustyfin`, `/etc/rustyfin`, `/var/lib/rustyfin`, `/var/log/rustyfin`, and `/var/cache/rustyfin`
- current runtime still retains repo-relative artifacts and invoking-user assumptions in places

Where:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `crates/installer/**`
- `scripts/start-native.sh`
- runtime env/default snapshot handling

What needs to be done:

1. Define the canonical install layout in code, not just docs.
2. Introduce a dedicated `rustyfin` system user/group model.
3. Move persistent state, logs, cache, and runtime outputs to the canonical roots.
4. Minimize repo-relative runtime paths to development-only cases.
5. Preserve compatibility for current live deployments with a deliberate migration path.

How it needs to be done:

- implement path/layout structs in Rust installer code
- migrate env/default generation to use canonical locations
- treat repo-local operation as development mode, not production mode
- keep the transition idempotent and migration-aware

Done means:

- supported production installs no longer depend on the invoking human user
- persistent runtime state is in canonical host locations
- repo-relative runtime behavior is reduced to explicit development mode
- live upgrades do not silently lose state

### Problem 1.3: Installer artifact acquisition and rollback model are incomplete

Status:

- not started

Why this is still open:

- the plan still lists git URL installs, release tarball installs, version pinning, upgrade channels, rollback, and a shipped installer binary as open work

Where:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `crates/installer/**`

What needs to be done:

1. Add an install-source model: local checkout, git URL, release tarball.
2. Add version pinning/channel semantics for repeatable installs.
3. Add a rollback mechanism for failed upgrades.
4. Produce a standalone packaged `rustfin-installer` binary once the runtime model is stable.

How it needs to be done:

- make install manifest authoritative
- persist install source and version metadata
- use immutable install records instead of implicit “current tree only”
- keep rollback bounded to installer-owned assets and service files

Done means:

- the installer can install from more than a preexisting checkout
- upgrades can be pinned and traced
- failed upgrades have a defined rollback path
- the installer can be shipped as a product component rather than only `cargo run`

### Problem 1.4: GPU stack handling is still only partially solved

Status:

- partial

Why this is still open:

- runtime probing exists
- build-time backend selection exists
- distro-specific driver installation/persistence and reboot-handling policy do not

Where:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `crates/installer/**`
- GPU/transcription/transcoder runtime config paths

What needs to be done:

1. Add vendor inventory for NVIDIA, AMD, and Intel.
2. Define when installer-managed driver installation is allowed and when it is not.
3. Persist multi-GPU inventory decisions into the install manifest.
4. Map detected hardware to Rustyfin defaults for AI, transcription, and transcoding.

How it needs to be done:

- keep unsafe driver mutation out of unknown distro paths
- prefer explicit, supported-family logic
- record every GPU-related decision in the install manifest

Done means:

- supported hosts get deterministic GPU defaults
- install manifests explain which GPU path was chosen and why
- driver policy is documented and enforced, not ad hoc

### Problem 1.5: Operational validation is still incomplete

Status:

- partial

Why this is still open:

- RustyVault migration still calls for deployment-like smoke and DB-backed validation
- installer docs still describe acceptance criteria beyond what has been fully exercised
- some DB-backed integration paths are compile-checked but not consistently executed in CI

Where:

- `docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
- `scripts/ci/**`
- runtime/deploy validation scripts

What needs to be done:

1. Add deployment-like RustyVault removability/runtime validation on Linux.
2. Execute DB-backed integration suites where docs currently only recommend them.
3. Add clean-install matrix coverage for supported host families as they land.
4. Capture logs/artifacts automatically for failed host validation.

How it needs to be done:

- promote manual validation steps into repeatable gates
- keep gates small and purpose-built
- make failures diagnosable without manual SSH archaeology

Done means:

- open validation items in the RustyVault blueprint are closed
- installer/runtime claims are backed by repeatable automation
- supported-host install/deploy regressions fail CI or gate scripts early

### Problem 1.6: Platform/runtime docs still need final cleanup

Status:

- partial

Why this is still open:

- some docs still describe planned states that are already shipped or partially outdated

Where:

- `docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
- `README.md`
- `AGENTS.md`
- `docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`

What needs to be done:

1. Remove claims that no longer match the code.
2. Update supported-host language as installer capabilities expand.
3. Keep runtime docs aligned with actual production layout and validation behavior.

Done means:

- no platform/runtime document describes already-landed work as future
- no doc claims a host/runtime path that the installer does not actually support

---

## Agent 2

### Agent Prompt

```text
You are Agent 2 for Rustyfin. Your job is to finish the product-surface work for Downloads, first-party clients, Network, and Smart Home. You own the host-facing product surfaces and the backend APIs they need.

You own:
- ui/src/app/downloads/**
- ui/src/lib/downloadsApi.ts
- crates/server/src/downloads.rs
- crates/server/src/routes.rs for downloads/network/smart-home mounts
- ui/src/app/network/**
- crates/server/src/network_diagnostics.rs and any new network/RustyNet host modules
- any new ui/src/app/smart-home/**
- any new crates/server/src/smart_home/**
- NavBar changes only if required to add Smart Home or adjust new product routes
- related docs in README.md and docs/reports/**

You must not own:
- crates/installer/**
- scripts/install_*.sh or deploy/start scripts
- ui/src/app/backups/**
- ui/src/app/servers/**
- crates/server/src/servers/**
- crates/server/src/ai_assistant/**

Your goals are:
1. turn Downloads into a real release center
2. define and ship the first supported first-party client artifact model
3. replace the placeholder Network page with a real RustyNet-backed surface
4. create the first Smart Home product surface and backend contract

Do not add AI integrations directly. Expose stable backend contracts and product surfaces; Agent 4 will consume them.
```

### Owned Files And Boundaries

Primary ownership:

- downloads catalog, downloads UI, future client-release metadata
- network page and RustyNet-facing host contract
- smart-home host surface and provider contract

Do not edit without a hard blocker:

- `crates/installer/**`
- `ui/src/app/backups/**`
- `ui/src/app/servers/**`
- `crates/server/src/servers/**`
- `crates/server/src/ai_assistant/**`

### Agent 2 Implementation Decisions

These decisions are fixed for this program so Agent 2 does not need to re-litigate them:

- Downloads must become DB-backed; do not keep release state hard-coded in Rust once release management lands.
- The first desktop-client strategy should be a thin shell, not a second full frontend.
- Use Tauri for the first Windows/macOS/Linux desktop-client shell unless a hard technical blocker is discovered.
- The first mobile-client strategy should also be a thin shell around the existing authenticated product surface, not a separate product rewrite.
- Use Capacitor for the first Android/iOS mobile shell unless a hard technical blocker is discovered.
- iOS distribution should be modeled as an external distribution destination in the Downloads catalog until a real signed iOS app pipeline exists.
- The first Smart Home provider integration should be Home Assistant as a single aggregator-style backend, not multiple direct vendor adapters in parallel.
- The first Network release must separate:
  - current host-known network data
  - future richer RustyNet mesh data
- The first Downloads admin surface should live under the existing Admin area rather than introducing a separate release-management product area.

### Detailed Task Inventory

This is the canonical task queue for Agent 2. Execute these items in order unless a later item is explicitly blocked by an earlier one.

#### Task 2A: Downloads artifact model and catalog rewrite

Why open:

- the current catalog is still extension-centered and uses generic planned entries

Where:

- `crates/server/src/downloads.rs`
- `ui/src/app/downloads/page.tsx`
- `ui/src/lib/downloadsApi.ts`

What to do:

1. Replace generic placeholders with explicit artifact rows for:
   - Windows desktop
   - macOS desktop
   - Linux desktop
   - Android APK
   - iOS distribution destination
2. Add explicit metadata fields:
   - artifact id
   - platform
   - architecture
   - version
   - checksum
   - signature status
   - release channel
   - distribution mode
   - availability state
3. Render Downloads as platform sections instead of one generic grid.

Done looks like:

- `/downloads` and `/api/v1/downloads/catalog` are explicit, platform-aware, and no longer placeholder-driven

#### Task 2B: Generalized artifact delivery path

Why open:

- delivery currently special-cases only the RustyVault extension package

Where:

- `crates/server/src/downloads.rs`
- `crates/server/src/routes.rs`
- `ui/src/lib/downloadsApi.ts`

What to do:

1. Add a generalized resolver by artifact id + version/channel.
2. Support:
   - direct binary delivery
   - APK delivery
   - external destination entries for iOS/App Store/TestFlight style links
3. Include content-type, content-disposition, checksum, and signature-friendly metadata.

Done looks like:

- artifact delivery is generic and not tied to a single extension package

#### Task 2C: Release-management workflow for Downloads

Why open:

- admins still cannot publish, retire, or update release entries without code changes

Where:

- downloads persistence layer
- new DB migrations under `crates/db/migrations_pg/**`
- admin-side routes and UI as needed

What to do:

1. Add DB-backed artifact and artifact-version records.
2. Add admin-only publish/retire/update APIs.
3. Add an admin release-management UI.
4. Audit all release state changes.

Done looks like:

- release management is productized and no longer hard-coded in Rust

#### Task 2D: First-party client foundations

Why open:

- the Downloads commitment now includes Windows, macOS, Linux, Android, and iOS, but no client code exists yet

Where:

- new client directories/modules
- packaging/build docs
- Downloads release metadata

What to do:

1. Use one thin desktop-shell strategy for Windows, macOS, and Linux.
2. Use one thin mobile-shell strategy for Android and iOS.
3. Add initial client scaffolds rather than full product rewrites.
4. Implement host URL, login, and persisted host selection flow.
5. Define build, packaging, and signing expectations for each platform.

How it needs to be done:

- reuse the existing Rustyfin product surface
- do not fork the UI into separate platform-specific product implementations
- prioritize host connection, auth, and durable shell structure over feature breadth

Done looks like:

- the repo contains real first-party client foundations with defined packaging/distribution paths

#### Task 2E: Network backend contract and first real `/network` page

Why open:

- `/network` is still a placeholder and there is no stable RustyNet-ready topology contract

Where:

- `ui/src/app/network/page.tsx`
- `crates/server/src/network_diagnostics.rs`
- new network/RustyNet backend modules

What to do:

1. Add a normalized topology contract with:
   - nodes
   - edges
   - node status
   - last-seen/updated metadata
   - degraded/unavailable state
2. Keep current host-known network data as the initial source of truth.
3. Leave a clean extension seam for future RustyNet mesh data.
4. Replace the placeholder page with a real node/edge visual.
5. Show hover/detail data for node name, IP, and concise status.

Done looks like:

- `/network` is a real read-first product surface with a stable backend contract

#### Task 2F: Smart Home MVP

Why open:

- Smart Home exists only in docs and has no provider or API model

Where:

- new `ui/src/app/smart-home/**`
- new `crates/server/src/smart_home/**`
- optional nav integration

What to do:

1. Pick one aggregator-style provider for MVP.
2. Add a read-only device inventory API.
3. Normalize devices into:
   - cameras
   - lights
   - doors/locks
   - alarms
   - generic devices
4. Group by room/zone where possible.
5. Add truthful unavailable and empty states.

How it needs to be done:

- start read-only
- do not ship security-sensitive write controls in MVP
- do not add multiple provider integrations before the first one is stable

Done looks like:

- Rustyfin has a real Smart Home foundation rather than docs-only intent

#### Task 2G: Placeholder-governance cleanup

Why open:

- product surfaces still have inconsistent placeholder behavior

Where:

- `ui/src/app/downloads/page.tsx`
- `ui/src/app/network/page.tsx`
- new Smart Home route if added
- product docs/nav as needed

What to do:

1. Remove misleading placeholder language from any surface that becomes real.
2. Keep clearly gated unavailable states where a committed feature is not yet fully live.
3. Ensure top-level nav does not land users on misleading dead ends.

Done looks like:

- top-level product pages are either functional or explicitly capability-gated

### Supporting Context Only: Expanded Problem Breakdown

The following `Problem` entries are retained for traceability back to the source plans and audits.

They are not a second independent task queue.

### Problem 2.1: Downloads catalog is still generic and extension-centered

Status:

- partial

Why this is still open:

- backend catalog ships one real artifact and two generic planned entries
- UI still renders “Coming Soon” cards instead of platform-aware client releases

Where:

- `crates/server/src/downloads.rs`
- `ui/src/app/downloads/page.tsx`
- `ui/src/lib/downloadsApi.ts`

What needs to be done:

1. Replace generic planned entries with explicit artifact records for:
   - Windows desktop
   - macOS desktop
   - Linux desktop
   - Android APK
   - iOS distribution entry
2. Expand artifact metadata to include:
   - platform
   - architecture
   - version
   - checksum
   - signature status
   - channel
   - distribution mode
3. Render platform-specific UI sections instead of a single generic “Coming Soon” grid.

How it needs to be done:

- make artifact types explicit in backend response models
- keep the downloads route host-owned
- support both direct downloads and external-store destinations

Done means:

- `/api/v1/downloads/catalog` returns explicit per-platform client entries
- `/downloads` renders those entries as first-class artifacts
- artifact states are no longer generic placeholders

### Problem 2.2: Downloads delivery path only supports one package resolver

Status:

- not started

Why this is still open:

- the package route resolves only `rustyvault-webext`

Where:

- `crates/server/src/downloads.rs`
- `crates/server/src/routes.rs`
- `ui/src/lib/downloadsApi.ts`

What needs to be done:

1. Build a generalized artifact resolver keyed by artifact id and version/channel.
2. Support:
   - direct binary downloads
   - APK downloads
   - external distribution destinations for iOS
3. Add content-disposition, content-type, checksum, and signature-friendly delivery metadata.

How it needs to be done:

- make delivery semantics part of the artifact model
- keep actual bytes behind authenticated host routes where appropriate
- treat store links as first-class catalog entries, not hacks

Done means:

- desktop/mobile artifacts can be resolved without hard-coded special casing
- iOS entries can point to App Store/TestFlight style destinations cleanly

### Problem 2.3: There is no release-management workflow for Downloads

Status:

- not started

Why this is still open:

- release state is still hard-coded in Rust
- admins cannot publish or retire artifacts without code changes

Where:

- `crates/server/src/downloads.rs`
- admin surface files if new admin UI is required
- DB migrations and repo layer for downloads persistence

What needs to be done:

1. Add DB-backed release records for artifacts and artifact versions.
2. Add admin-only publish/retire/update APIs.
3. Add admin UI to manage artifact metadata and status.
4. Add audit logging for release changes.

How it needs to be done:

- use immutable version records
- keep catalog generation DB-backed, not hard-coded
- separate artifact metadata from artifact storage

Done means:

- admins can publish and retire client artifacts from the product
- public downloads catalog reflects persisted release state

### Problem 2.4: First-party client applications do not exist yet

Status:

- not started

Why this is still open:

- docs now commit to Windows, macOS, Linux, Android APK, and iOS distribution
- there are no corresponding client app codebases or wrappers in the repo

Where:

- new client directories/modules will be required
- downloads catalog/docs must reflect the chosen delivery architecture

What needs to be done:

1. Define the client strategy explicitly:
   - desktop client shell
   - Android APK path
   - iOS distribution path
2. Choose a concrete implementation path rather than leaving client apps abstract.
3. Create the initial app scaffolds and packaging pipeline.
4. Define how clients authenticate to a Rustyfin host and remember the target host URL.

How it needs to be done:

- reuse the existing product/API surface as much as possible
- keep the first clients thin rather than duplicating the whole product stack
- define packaging and signing requirements together with Downloads artifact metadata

Done means:

- the repo contains real first-party client foundations, not just placeholders in Downloads
- each planned platform has a defined build/distribution path
- Downloads can point to actual client artifacts or real external-store destinations

### Problem 2.5: The Network page is still a placeholder

Status:

- not started

Why this is still open:

- `/network` is still placeholder-only in code
- the design baseline defines a RustyNet-powered topology surface, but that surface does not exist

Where:

- `ui/src/app/network/page.tsx`
- `crates/server/src/network_diagnostics.rs`
- new RustyNet-facing backend modules/routes
- any shared UI components for topology display

What needs to be done:

1. Define a stable backend topology API for RustyNet-derived network data.
2. Replace the placeholder page with a visual node-based map.
3. Show online nodes, offline nodes, and concise hover/detail payloads.
4. Degrade gracefully when RustyNet data is unavailable.

How it needs to be done:

- keep the first release read-first and visibility-focused
- build a normalized node/edge model in the backend
- separate host-known local network data from future richer RustyNet mesh data

Done means:

- `/network` is a real product surface
- the user can inspect network nodes and state visually
- the page no longer behaves like a dead-end placeholder

### Problem 2.6: Smart Home is planned but has no implementation

Status:

- not started

Why this is still open:

- Smart Home exists only in docs
- there is no page, no API family, no provider integration, and no normalized device model

Where:

- new `ui/src/app/smart-home/**`
- new `crates/server/src/smart_home/**`
- optional nav integration
- Smart Home docs in the design baseline/README

What needs to be done:

1. Add a dedicated Smart Home surface.
2. Add a read-only backend device inventory API.
3. Normalize at least one provider integration into cameras, lights, doors/locks, alarms, and generic device states.
4. Group devices by zone/room where possible.
5. Add graceful unavailable/empty states.

How it needs to be done:

- start read-only
- prioritize state visibility over control
- treat high-risk security controls as out of scope for MVP

Done means:

- authenticated users can see linked smart-home devices in Rustyfin
- cameras/lights/doors/alarms have normalized summary cards
- the product has a real Smart Home foundation rather than docs-only intent

### Problem 2.7: Product-surface placeholder governance is inconsistent

Status:

- partial

Why this is still open:

- some top-level routes are functional
- some are still placeholders
- the experience is inconsistent and makes the nav feel less production-ready

Where:

- `ui/src/app/network/page.tsx`
- `ui/src/app/downloads/page.tsx`
- nav and product-area docs

What needs to be done:

1. Replace placeholder routes with real functionality where committed.
2. If a surface cannot ship yet, clearly label capability state instead of implying completeness.
3. Keep top-level routes reserved only for active or explicitly staged product areas.

Done means:

- top-level product surfaces are either functional or intentionally capability-gated
- users do not land on misleading placeholders for core nav items

---

## Agent 3

### Agent Prompt

```text
You are Agent 3 for Rustyfin. Your job is to finish the backup/restore program and the remaining Servers/Minecraft work beyond the current Vanilla/Paper core.

You own:
- ui/src/app/backups/**
- ui/src/app/servers/**
- ui/src/lib/serversApi.ts
- crates/server/src/servers/**
- crates/servers-host/**
- crates/servers-agent/**
- any new backup modules under crates/server/src/** or crates/db/**
- DB migrations for backup/restore or advanced server features
- server/backups docs in docs/reports/**

You must not own:
- crates/installer/**
- ui/src/app/downloads/**
- ui/src/app/network/**
- any new Smart Home code
- crates/server/src/ai_assistant/**

Your goals are:
1. replace the backup placeholder with a real backup/restore subsystem
2. finish the next operational layer of the Minecraft servers product
3. build the data and API contracts that later AI and admin surfaces can consume safely

Do not add AI-facing integrations directly except where unavoidable for stable API shape. Agent 4 will consume your APIs.
```

### Owned Files And Boundaries

Primary ownership:

- backups product
- advanced server lifecycle/product work
- server backup hooks and restore flows

Do not edit without a hard blocker:

- `crates/installer/**`
- `ui/src/app/downloads/**`
- `ui/src/app/network/**`
- new Smart Home modules
- `crates/server/src/ai_assistant/**`

### Agent 3 Implementation Decisions

These decisions are fixed for this program so Agent 3 does not need to widen scope:

- Backup MVP is admin-only.
- Backup MVP should support local-disk storage first; object-store support is future work unless a task is added explicitly.
- Backup MVP scope should cover:
  - PostgreSQL data
  - managed server instances/worlds
  - Rustyfin runtime/config state that is required for recovery
- Backup MVP should not attempt full media-library blob backup unless that is separately assigned.
- Multi-game framework work should remain deferred unless the Minecraft backlog in this document is materially complete.

### Detailed Task Inventory

This is the canonical task queue for Agent 3. Do the backup control plane first, then the safety-critical restore flow, then the remaining Servers operational gaps.

#### Task 3A: Backup control plane

Why open:

- there is no real backup backend yet
- the Backups UI is placeholder-only
- the AI backup tool is still a stub

Where:

- `ui/src/app/backups/page.tsx`
- new backup modules under `crates/server/src/**`
- `crates/db/migrations_pg/**`

What to do:

1. Add persisted backup policy, job, artifact, and history models.
2. Add backup job APIs for create/list/inspect.
3. Add host execution adapters for filesystem and PostgreSQL capture.
4. Reuse existing job/audit patterns where possible.

Done looks like:

- real backup jobs can be created, listed, and inspected from API and UI

#### Task 3B: Restore workflow with safety boundaries

Why open:

- there is no restore path, no staged verification, and no rollback semantics

Where:

- new backup/restore modules
- host-execution layer
- `ui/src/app/backups/page.tsx`

What to do:

1. Implement staged restore: validate, preflight, quiesce, restore, verify, optional rollback.
2. Require checksum/manifest verification.
3. Gate restore behind strong confirmation/protected-action semantics.

Done looks like:

- restores are deliberate, auditable, and recoverable on failure

#### Task 3C: Backup scheduling and retention execution

Why open:

- scheduling and pruning are referenced conceptually, not implemented

Where:

- backup backend runtime loops
- `crates/server/src/main.rs`
- backup policy persistence

What to do:

1. Add periodic scheduler evaluation.
2. Enqueue backup jobs on schedule.
3. Prune snapshots by retention policy.
4. Surface missed and failed runs.

Done looks like:

- scheduled backup runs and retention cleanup happen automatically and are visible in product state

#### Task 3D: Replace the AI backup stub with real state

Why open:

- `system_get_backup_summary` is truthful but fake because the subsystem does not exist yet

Where:

- backup APIs/state
- Agent 4 AI integration point after backup APIs stabilize

What to do:

1. Ship real backup state first.
2. Then provide the stable summary contract Agent 4 should consume.

Done looks like:

- no shipped backup summary path reports stubbed state

#### Task 3E: Per-instance membership management

Why open:

- membership tables exist but there is no member-management API or UI

Where:

- `crates/db/src/repo/servers.rs`
- `crates/server/src/servers/router.rs`
- `ui/src/app/servers/page.tsx`

What to do:

1. Add member CRUD repo methods.
2. Add `/members` routes.
3. Add UI for grant/revoke/edit.
4. Enforce per-role authorization at runtime.

Done looks like:

- admins can manage per-server memberships as a first-class product flow

#### Task 3F: Role-model drift cleanup

Why open:

- docs describe `viewer/operator/manager`
- runtime behavior still does not cleanly implement `operator`

Where:

- servers auth checks
- DB role persistence
- servers docs/tests

What to do:

1. Normalize the persisted role enum.
2. Give `operator` explicit start/stop/restart privileges.
3. Keep higher-risk actions on `manager` or above.

Done looks like:

- documented role matrix and runtime behavior match exactly

#### Task 3G: Persistent discovery/import state

Why open:

- discovery candidate schema exists but scan results are still transient

Where:

- discovery repo methods
- servers handlers
- host discovery integration

What to do:

1. Persist discovery results across scans.
2. Track candidate/imported/ignored/invalid states.
3. Link imported instances back to discovery records.

Done looks like:

- discovery survives restart and accurately reflects import state

#### Task 3H: Import-mode semantics

Why open:

- import is effectively copy-to-managed only
- adopt-in-place and existing-unit adoption are still missing

Where:

- servers import handlers
- `crates/servers-host/**`

What to do:

1. Add explicit import modes.
2. Add preflight/preview before import.
3. Support `adopt_in_place` and `copy_to_managed`.
4. Only support existing-unit adoption behind strict validation.

Done looks like:

- users choose import behavior deliberately and can preview effects before execution

#### Task 3I: Safe delete semantics

Why open:

- current delete still combines unregister and file deletion

Where:

- servers delete handler
- servers UI confirmations

What to do:

1. Make unregister the default delete mode.
2. Make file deletion explicit and separately confirmed.
3. Audit both paths distinctly.

Done looks like:

- destructive file deletion is no longer the default server-removal path

#### Task 3J: Backend-driven reconciliation and operational policy

Why open:

- state freshness still leans too heavily on UI polling
- auto-stop and scheduled windows are still absent

Where:

- `crates/server/src/main.rs`
- servers backend/agent loops
- `ui/src/app/servers/page.tsx`

What to do:

1. Add backend reconcile loops for status, health, and player count.
2. Add auto-stop-when-empty enforcement.
3. Add scheduled start/stop windows.
4. Keep UI polling as freshness, not correctness.

Done looks like:

- server state remains correct even when no UI is open

#### Task 3K: Typed host control boundary

Why open:

- host control still relies on subprocess `systemctl` semantics
- the typed runner/systemd boundary in the design docs is only partially realized

Where:

- `crates/servers-host/**`
- servers-host lifecycle code

What to do:

1. Add a typed systemd adapter.
2. Improve deterministic lifecycle behavior and shutdown semantics.
3. Only add a separate runner boundary if it materially improves safety/telemetry.

Done looks like:

- server lifecycle control is typed, validated, and operationally predictable

#### Task 3L: Additional Minecraft distributions

Why open:

- only `vanilla` and `paper` are implemented

Where:

- servers backend validation
- host artifact/provisioning logic
- servers UI

What to do:

1. Add Fabric.
2. Add Forge.
3. Add NeoForge.
4. Validate/provision each distribution explicitly.

Done looks like:

- new distributions are first-class supported options with real runtime validation

#### Task 3M: Mods, plugins, templates, and world-content tooling

Why open:

- these remain docs-only future product capabilities

Where:

- servers UI
- servers backend
- host content management logic

What to do:

1. Add world-template inventory/import.
2. Add managed mod/plugin lifecycle.
3. Add modpack automation only after the above are stable.

Done looks like:

- advanced server content management is productized instead of manual host work

#### Task 3N: Multi-game framework decision

Why open:

- the UI advertises a future beyond Minecraft, but no reusable framework exists yet

Where:

- server domain model
- server plans/docs

What to do:

1. Decide whether to defer multi-game explicitly or formalize the reusable contract.
2. Do not start this before the Minecraft backlog above is materially stable.

Done looks like:

- either the docs explicitly defer multi-game work or a real reusable server framework exists

### Supporting Context Only: Expanded Problem Breakdown

The following `Problem` entries are retained for traceability back to the source plans and audits.

They are not a second independent task queue.

### Problem 3.1: Backup/restore does not exist beyond placeholders

Status:

- not started

Why this is still open:

- `/backups` is placeholder-only
- AI backup summary truthfully reports that backup/restore is not implemented
- server plans explicitly defer backup/restore

Where:

- `ui/src/app/backups/page.tsx`
- new backup modules in `crates/server/src/**`
- new DB migrations in `crates/db/migrations_pg/**`
- possible integration with `crates/servers-host/**`

What needs to be done:

1. Define backup domain model:
   - backup target
   - backup job
   - backup artifact
   - restore operation
   - retention policy
2. Define supported backup scopes:
   - database
   - server instances/worlds
   - optional media metadata/config where appropriate
3. Add scheduler/manual-trigger model.
4. Add restore flow with safety checks and audit trail.
5. Add admin/user visibility rules.

How it needs to be done:

- treat backup metadata as persisted product state
- keep restore high-friction and explicit
- make backup job execution observable from the product
- design restore with safety-first semantics rather than convenience-first

Done means:

- `/backups` is no longer a placeholder
- backups can be created, listed, and restored through defined APIs
- restore actions are audited and guarded
- AI and admin surfaces can report real backup state instead of a stub

### Problem 3.2: Backup UI does not exist

Status:

- not started

Why this is still open:

- the nav exposes Backups
- the page does not contain any functional controls

Where:

- `ui/src/app/backups/page.tsx`
- any supporting UI components/hooks

What needs to be done:

1. Replace placeholder copy with:
   - backup list/history
   - create backup action
   - schedule/retention settings
   - restore affordances
2. Add truthful empty, loading, failed, and unavailable states.
3. Define whether standard users can view backup status or admin-only.

Done means:

- the page is a real product surface
- it reflects actual backup state from backend APIs
- no placeholder language remains

### Problem 3.3: Servers product still lacks the next operational layer

Status:

- partial

Why this is still open:

- current product handles the Minecraft core
- follow-on operational capabilities remain explicitly deferred in the server plans

Where:

- `ui/src/app/servers/page.tsx`
- `crates/server/src/servers/**`
- `crates/servers-host/**`
- `crates/servers-agent/**`

What needs to be done:

1. Add stop-when-empty automation.
2. Add scheduled start windows.
3. Add stronger error/status surfacing in both backend and UI.
4. Improve import/discovery operational ergonomics where needed.

How it needs to be done:

- keep lifecycle logic in Rust
- keep long-running supervision with `systemd`
- persist desired/observed state transitions cleanly

Done means:

- core server lifecycle automation exists beyond manual start/stop
- server operators can understand failures and schedules from the UI
- the operational gap between “core exists” and “usable daily” is closed

### Problem 3.4: Advanced Minecraft distributions are still absent

Status:

- not started

Why this is still open:

- current UI/backend only allow `paper` and `vanilla`

Where:

- `ui/src/app/servers/page.tsx`
- `crates/server/src/servers/handlers.rs`
- `crates/servers-host/**`

What needs to be done:

1. Add Fabric support.
2. Add Forge support.
3. Add NeoForge support.
4. Extend provisioning/runtime validation for each distribution.

How it needs to be done:

- keep server distribution validation explicit and allowlisted
- add provisioning paths one distribution at a time
- do not generalize beyond what can be validated/tested

Done means:

- UI/backend allow the new distributions explicitly
- provisioning succeeds for supported distributions
- runtime state and validation are correct per distribution

### Problem 3.5: Templates, mods, and world-content tooling are still absent

Status:

- not started

Why this is still open:

- plans explicitly defer world template import, story-map creation, and managed mods/plugins

Where:

- `ui/src/app/servers/page.tsx`
- `crates/server/src/servers/**`
- `crates/servers-host/**`

What needs to be done:

1. Add world template import.
2. Add story-map/world-pack creation flow if templates are standardized.
3. Add managed mods/plugins install surface.
4. Add modpack automation only after template/plugin handling is stable.

How it needs to be done:

- define safe import/trust rules
- keep filesystem validation strict
- separate content metadata from raw uploaded files

Done means:

- operators can import managed world templates safely
- mods/plugins are not unmanaged filesystem hacks anymore
- advanced content workflows exist as product features, not manual host work

### Problem 3.6: Future game framework is still only an architectural promise

Status:

- deferred

Why this is still open:

- docs describe future games later, but no framework beyond Minecraft is operationally real

Where:

- server plans and server domain model

What needs to be done:

1. Decide whether to keep expanding Minecraft first or formalize a multi-game abstraction.
2. Only after the Minecraft backlog above is stable, define the reusable game-server contract.

Done means:

- either the docs explicitly defer multi-game work
- or a stable reusable server framework exists beyond Minecraft-specific assumptions

---

## Agent 4

### Agent Prompt

```text
You are Agent 4 for Rustyfin. Your job is to finish the AI assistant from a read-only grounded assistant into a production-ready assistant with safe write-capable actions, tighter prompt/response behavior, and clean domain integrations.

You own:
- crates/server/src/ai.rs
- crates/server/src/ai_enabled.rs
- crates/server/src/ai_admin.rs
- crates/server/src/ai_audit.rs
- crates/server/src/ai_storage.rs
- crates/server/src/ai_assistant/**
- ui/src/app/ai/**
- ui/src/lib/aiApi.ts
- ui/src/lib/aiAdminApi.ts
- AI docs under docs/plans/**
- AI sections in README.md and AGENTS.md

You must not own:
- crates/installer/**
- ui/src/app/downloads/**
- ui/src/app/network/**
- any new Smart Home backend/product code
- ui/src/app/backups/**
- ui/src/app/servers/**
- crates/server/src/servers/**

Your goals are:
1. close the remaining AI architecture gaps
2. add safe write-capable actions only behind explicit confirmation/protected-action mechanics
3. keep the assistant truthful, permission-safe, and well-tested
4. consume stable backend contracts from other agents instead of inventing cross-domain logic on the fly

Do not add direct DB/filesystem/network access for the model. Preserve server-side tool ownership and policy enforcement.
```

### Owned Files And Boundaries

Primary ownership:

- AI planner/orchestrator
- AI tool registry and assistant policies
- AI admin surface and audit surface
- AI docs and tests

Do not edit without a hard blocker:

- installer/platform code
- downloads/network/smart-home implementation owned by Agent 2
- backups/servers implementation owned by Agent 3

### Agent 4 Implementation Decisions

These decisions are fixed for this program so Agent 4 does not need to decide assistant policy ad hoc:

- No new read-only AI domain should be added until the hardening tasks in this section are complete or intentionally re-scoped.
- The first write-capable AI action should be low-risk and calendar-based before any room or broader product writes.
- The first server write-capable AI actions should be start/stop only, not destructive server mutation.
- End-user chain-of-thought rendering must be removed rather than refined.
- Generic public web tools remain admin-only; do not widen them to normal users to satisfy narrow use cases.
- If the underlying product does not yet support a precise state such as true unread counts, keep the AI contract narrow and truthful instead of inventing precision.

### Detailed Task Inventory

This is the canonical task queue for Agent 4. Do the hardening tasks first. Do not add new assistant features until these items are complete or intentionally re-scoped.

#### Task 4A: Confirmation-token and protected-action write path

Why open:

- write-capable tools are still intentionally blocked
- confirmation metadata exists conceptually, but there is no full execution path

Where:

- `crates/server/src/ai_assistant/**`
- AI audit and route modules
- UI confirmation flow on `/ai`

What to do:

1. Implement confirmation token issuance, expiry, replay protection, and audit.
2. Integrate protected-action handling for high-risk writes.
3. Keep write execution impossible in a single unconstrained model turn.

Done looks like:

- at least one write can only execute after explicit confirmation and token validation

#### Task 4B: Replace the backup-summary stub once Agent 3 exposes real backup state

Why open:

- `system_get_backup_summary` still reports placeholder state

Where:

- AI tools and registry
- backup APIs from Agent 3

What to do:

1. Wait for stable backup state contracts.
2. Replace the stub with real configured/history/restore-capability reporting.

Done looks like:

- backup summaries are grounded in real runtime state

#### Task 4C: Make channel unread activity real or narrow the contract

Why open:

- `channels_list_unread_activity` does not currently provide true unread counts

Where:

- AI tool contract
- channel read-state model once available

What to do:

1. Either add real unread tracking when the product supports it.
2. Or keep the tool explicitly framed as recent-activity only until then.

Done looks like:

- the tool contract and backend truth match exactly, with no implied unread precision that does not exist

#### Task 4D: Enforce tool timeout and result-size limits at execution time

Why open:

- tool specs declare `timeout_ms` and `max_result_bytes`
- runtime enforcement still focuses on role/access/confirmation, not execution budget

Where:

- AI tool execution path
- tool registry/spec handling

What to do:

1. Wrap tool calls in shared timeout enforcement.
2. Enforce serialization/result-size budgets.
3. Add deterministic failure/truncation behavior with tests.

Done looks like:

- oversized or slow tool calls fail in a controlled, tested way

#### Task 4E: Add AI abuse and lifecycle guardrails

Why open:

- current AI route accepts generous history payloads and lacks the full rate/concurrency guardrail set described in the architecture doc

Where:

- `crates/server/src/ai_enabled.rs`
- AI request types
- route/middleware integration

What to do:

1. Add AI-specific request bounds.
2. Add history-item/message-length caps.
3. Add per-user and per-session rate/concurrency controls.
4. Return explicit `400`/`429` behavior with tests.

Done looks like:

- abusive or oversized AI requests are rejected predictably

#### Task 4F: Route-level AI integration coverage

Why open:

- much of the current AI coverage is still closer to grounded-turn preparation than full HTTP/SSE endpoint behavior

Where:

- AI integration tests
- `/api/v1/ai/chat`
- admin AI routes

What to do:

1. Add endpoint-level tests for auth and role gating.
2. Add SSE lifecycle coverage.
3. Add audit-persistence success/failure coverage.
4. Add admin model-dir and model-management failure-path coverage.

Done looks like:

- AI route behavior is verified at the HTTP/SSE layer, not only through internal helpers

#### Task 4G: Remove end-user chain-of-thought exposure

Why open:

- UI still supports `<think>` rendering, which conflicts with the documented status-event-only progress model

Where:

- `ui/src/app/ai/page.tsx`
- optional server-side output sanitization if needed

What to do:

1. Strip or suppress `<think>` content before display.
2. Keep visible progress limited to explicit status/provenance events.

Done looks like:

- users never see hidden reasoning text

#### Task 4H: Final prompt/style/doc cleanup

Why open:

- prompt/response tightening is still listed as open
- one AI design doc still contains milestone drift

Where:

- AI prompt text
- AI docs under `docs/plans/**`
- `README.md`
- `AGENTS.md`

What to do:

1. Remove outdated milestone language.
2. Tighten planner/final-answer prompts for truthfulness and consistency.
3. Add regression tests for refusal/failure phrasing where practical.

Done looks like:

- AI docs describe the shipped state accurately and AI responses are more consistent

#### Task 4I: Consume stable new domains only after they are real

Why open:

- Network mesh, backup detail, Downloads release detail, and Smart Home AI work all depend on stable Agent 2/3 contracts

Where:

- AI tools and registry

What to do:

1. Wait for stable product APIs.
2. Add read-only AI tools only after auth rules and failure semantics are settled.
3. Add permission/failure coverage alongside every new AI domain.

Done looks like:

- AI never ships a domain integration ahead of the real product contract

### Supporting Context Only: Expanded Problem Breakdown

The following `Problem` entries are retained for traceability back to the source plans and audits.

They are not a second independent task queue.

### Problem 4.1: AI write-capable execution does not exist

Status:

- not started

Why this is still open:

- the AI tracker still lists write tools as future work
- there is no confirmation-token workflow or protected-action execution path for AI actions

Where:

- `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
- `crates/server/src/ai_assistant/**`
- existing protected-action patterns where relevant

What needs to be done:

1. Define write tool contract fields that are enforceable, not advisory.
2. Implement confirmation-token workflow for AI writes.
3. Integrate protected-action support for high-risk actions.
4. Add replay protection, expiry, and audit logging for confirmations.

How it needs to be done:

- reuse secure protected-action patterns instead of inventing weaker ad hoc flows
- never allow direct write execution from a single unconstrained model turn
- keep explicit user confirmation as a distinct step

Done means:

- AI can propose a write
- the system issues a confirmation challenge
- the user confirms
- the backend validates the confirmation/protected-action token
- the write executes and is auditable

### Problem 4.2: First safe write tools are still absent

Status:

- not started

Why this is still open:

- planned write tools are documented but not implemented

Where:

- `crates/server/src/ai_assistant/registry.rs`
- `crates/server/src/ai_assistant/tools.rs`
- dependent product APIs once stable

What needs to be done:

1. Implement the first write tools only after Problem 4.1 lands.
2. Prioritize:
   - `calendar_create_event`
   - `calendar_update_event`
   - `server_action_start`
   - `server_action_stop`
3. Only add `room_create` and `room_invite_user` when the confirmation flow and permission model are proven.

How it needs to be done:

- consume stable product APIs instead of bypassing them
- keep tool outputs compact and truthful
- test non-admin, wrong-owner, expired-token, and replay cases

Done means:

- at least one safe calendar write and one safe server write are fully supported
- non-admin or unauthorized attempts fail correctly
- audit history shows planned, confirmed, executed, and failed writes clearly

### Problem 4.3: Optional admin-only assistant mode is not defined

Status:

- not started

Why this is still open:

- docs still leave this as a future option
- admin-only tools exist, but there is no deliberate assistant-mode separation

Where:

- AI docs
- `/ai` UI and/or admin surfaces
- AI registry exposure rules

What needs to be done:

1. Decide whether admin-only assistant mode belongs on `/ai`, `/admin`, or a gated submode.
2. Separate normal-user assistant scope from privileged assistant scope explicitly.
3. Prevent accidental admin-tool exposure on the normal user surface.

Done means:

- the product has a deliberate answer to privileged assistant usage
- admin-only tool exposure is explicit and UI-backed

### Problem 4.4: AI prompt/response style still needs final tightening

Status:

- partial

Why this is still open:

- the tracker still lists prompt/response tightening as remaining
- one AI doc still contains outdated milestone language

Where:

- `docs/plans/2026-03-14-ai-assistant-design.md`
- `docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
- planner/orchestrator prompt definitions in `crates/server/src/ai_assistant/orchestrator.rs`

What needs to be done:

1. Remove outdated milestone language from AI docs.
2. Tighten the planner/final-answer prompts for consistency and truthfulness.
3. Keep source attribution and progress behavior aligned with the UI.

Done means:

- AI docs describe the shipped state accurately
- responses are more consistent without weakening safety

### Problem 4.5: AI needs stable integrations for new domains from other agents

Status:

- blocked by other workstreams

Why this is still open:

- richer network mesh data does not exist yet
- backup workflows do not exist yet
- Smart Home does not exist yet
- Downloads artifact model is still too generic

Where:

- `crates/server/src/ai_assistant/**`
- domain contracts produced by Agent 2 and Agent 3

What needs to be done:

1. After Agent 2 lands richer downloads/network/smart-home APIs, add read-only AI tools for them.
2. After Agent 3 lands backup/restore APIs, replace the current backup stub with real summaries and details.
3. Keep permission-bound integration tests aligned with the new domains.

How it needs to be done:

- do not invent domain logic inside AI modules
- consume stable backend contracts
- keep the assistant read-only for new domains until a separate write design is approved

Done means:

- AI can accurately summarize the new domains once their backend contracts exist
- there are no stubbed or misleading summaries left for shipped domains

### Problem 4.6: AI validation still needs to become fully repeatable

Status:

- partial

Why this is still open:

- many test paths are strong
- some DB-backed integration paths still depend on environment availability
- future new domains will need the same rigor

Where:

- AI integration tests
- AI tool tests
- docs/plans AI tracker

What needs to be done:

1. Turn the remaining AI validation expectations into repeatable gates where possible.
2. Keep permission, lifecycle, failure, and prompt-safety coverage expanding with new tools.
3. Update the tracker as each validation claim becomes real.

Done means:

- AI claims in docs are backed by repeatable tests
- new domains do not land without matching permission/failure coverage

---

## Cross-Agent Coordination Rules

### Coordination Rule 1: API Contracts First

If Agent 2 or Agent 3 creates a new domain API that Agent 4 will consume, the owning agent must first stabilize:

- route shape
- response model
- auth rules
- unavailable/failure semantics

Only then should Agent 4 add AI tooling for that domain.

### Coordination Rule 2: Placeholder Removal

Agents must not remove placeholder UI copy until there is a truthful replacement state:

- functional UI
- explicit preview state
- or hidden nav entry

### Coordination Rule 3: Docs Move With Code

Each agent must update:

- `README.md`
- `AGENTS.md`
- the matching plan/report docs in `docs/`

when its owned area materially changes.

### Coordination Rule 4: Shared Files Require Restraint

Shared files such as:

- `README.md`
- `AGENTS.md`
- `ui/src/app/NavBar.tsx`
- `crates/server/src/routes.rs`

must only be touched for the owning workstream’s required mount points or doc sync, not opportunistic refactors.

## Recommended Execution Order

1. Agent 1
   - finish platform/runtime/validation foundations first
2. Agent 3
   - build real backup/restore primitives and next-layer servers behavior
3. Agent 2
   - replace placeholder product surfaces with real Downloads/Network/Smart Home behavior
4. Agent 4
   - finalize AI write-mode and consume the now-stable cross-domain APIs

Parallelism that is safe:

- Agent 1 can run in parallel with everyone.
- Agent 2 and Agent 3 can run in parallel if they do not fight over nav or shared docs.
- Agent 4 should start with AI-internal work first, then integrate other agents’ new domains only after their APIs stabilize.

## Program Completion Definition

This program is complete when:

- installer/runtime claims are validated on supported hosts
- Downloads is a real multi-artifact release center
- Network is a real RustyNet-backed product surface
- Smart Home exists as a first shipped product surface
- Backups is a real product feature instead of a placeholder
- Servers has moved beyond the current minimal Minecraft core
- AI can safely perform approved writes with confirmation/protected-action support
- the remaining docs describe only genuinely future work, not missing core product behavior
