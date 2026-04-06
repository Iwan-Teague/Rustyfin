# Rustyfin Docs

This tree is intentionally trimmed to active documentation only.

The supported native Linux install flow for Rustyfin is **Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04**. If a document stops matching the code or the supported install/runtime surface, update or remove it rather than keeping an in-repo archive.

## Read First

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
  - product summary and native Linux quick start
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
  - repository operating rules and runtime policy
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
  - install, start, deploy, ports, and service model
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/rustyvault-browser-access.md`
  - secure browser publication of `/vault`, exact origin settings, and edge TLS modes
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/rustyvault-browser-extension-api.md`
  - browser-extension pairing, lookup, CRUD, and packaging surface for RustyVault
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`
  - current architecture and product baseline
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-03-16-feature-gap-audit-excluding-network.md`
  - audit of documented-but-unshipped feature areas, excluding the Network page / RustyNet work
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-03-26-remaining-work-agent-execution-program.md`
  - execution-ready breakdown of remaining project work, split into four agent workstreams with prompts and done criteria
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-03-26-installer-platform-runtime-open-work-audit.md`
  - detailed open installer, platform, runtime-layout, and validation audit used by the execution program
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-05-rustyfin-linux-install-audit-second-pass-execution.md`
  - second-pass Linux install cleanup execution checklist and completion record
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-backups-open-work-audit-2026-03-26.md`
  - detailed open backups and advanced `Servers` audit used by the execution program

## Current Docs By Area

- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
  - RustyVault architecture and migration tracker
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
  - current `Servers` implementation plan
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
  - adopted Rust-native `Servers` design
- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-14-ai-assistant-design.md`
  - current AI architecture and admin-management model
- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
  - grounded AI assistant architecture, status tracker, tool model, security boundaries, and rollout
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`
  - current `/ai` bug and capability delta plan for multi-chat, timing accuracy, calendar writes, voice input, and live runtime telemetry
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-phase0-2-execution-report.md`
  - completed execution report for `/ai` phases 0 to 2
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-phase3-5-execution-report.md`
  - completed execution report for `/ai` phases 3 to 5, deployment, and live verification
- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-04-02-ai-memory-human-dictionary-knowledge-plan.md`
  - proposed implementation-grade delta plan for AI personal memory, people and group memory, account-to-person linking, and knowledge collections
- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-14-linux-bootstrap-installer-design.md`
  - design for a one-shot Linux bootstrap installer and cross-distro install strategy
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`
  - setup wizard contract
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/rustyfin-setup-wizard.openapi.yaml`
  - setup wizard OpenAPI spec

## Directory Map

- `docs/operations/`
  - runtime and deployment guides
- `docs/plans/`
  - active implementation plans
- `docs/reports/`
  - current design and architecture documents
- `docs/setup-wizard/`
  - setup contract and OpenAPI spec

## Documentation Rules

- Update docs in the same change when architecture, runtime, or public behavior changes.
- Keep runtime and deployment docs aligned to the supported Debian-native runtime.
- Remove stale docs instead of preserving superseded in-repo archives.
