# Rustyfin Docs

This tree is intentionally trimmed to active documentation only.

The supported runtime for Rustyfin is **native Debian 12 and Debian 13**. If a document stops matching the code or the supported runtime, update or remove it rather than keeping an in-repo archive.

## Read First

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
  - product summary and native Debian quick start
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
  - repository operating rules and runtime policy
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
  - install, start, deploy, ports, and service model
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`
  - current architecture and product baseline

## Current Docs By Area

- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
  - RustyVault architecture and migration tracker
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
  - current `Servers` implementation plan
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
  - adopted Rust-native `Servers` design
- `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-14-ai-assistant-design.md`
  - current AI architecture and admin-management model
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
