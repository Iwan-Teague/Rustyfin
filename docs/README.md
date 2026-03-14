# Rustyfin Docs Index

This directory contains both current project documentation and archived planning material.

The supported runtime for Rustyfin is **native Debian 12**. Any document that suggests Docker, Windows, or macOS runtime support should be treated as historical unless it explicitly says otherwise.

## Current Authoritative Docs

Read these first:

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
  - top-level product summary and native Debian quick start
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
  - repository operating rules and runtime policy
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
  - native Debian install, start, deploy, ports, logs, and service model
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/impeccable-comprehensive-coverage-playbook.md`
  - comprehensive command-first workflow for full-project Impeccable UX/design coverage
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`
  - current setup wizard contract/spec
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/documentation-audit-2026-03-12.md`
  - latest documentation drift audit and remediation summary
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-implementation-plan-2026-03-08.md`
  - current Minecraft `Servers` implementation plan
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/servers-minecraft-rust-native-design-2026-03-08.md`
  - adopted Rust-native Debian design rationale for `Servers`
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/password-vault-design-2026-03-12.md`
  - current Rustyfin Vault security and implementation design
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyvault-migration-blueprint-2026-03-13.md`
  - current migration blueprint and extraction plan for evolving Rustyfin Vault into RustyVault
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/rustyfin-current-state-design-baseline-2026-03-13.md`
  - current verified architecture, runtime, WBS, and planning baseline for the overall Rustyfin project

## Directory Map

- `docs/operations/`
  - current deployment and operational guides
- `docs/plans/`
  - implementation plans for active features
- `docs/project/`
  - project trackers and status notes
- `docs/prompts/`
  - prompt and instruction templates used during development
- `docs/reports/`
  - audits, implementation notes, investigations, and design reports
- `docs/setup-wizard/`
  - setup flow specs and implementation artifacts
- `docs/reference/`
  - archived bundled reference material

## Archive Policy

These locations are historical by default:

- `/Users/iwanteague/Desktop/Rustyfin/docs/reference/`
- older point-in-time reports under `/Users/iwanteague/Desktop/Rustyfin/docs/reports/`
- older tracker documents that describe superseded architecture constraints

Historical docs are kept for context, not as the source of truth for current runtime behavior.

The following files are especially likely to contain historical implementation notes rather than current runtime truth:

- `/Users/iwanteague/Desktop/Rustyfin/docs/project/RUSTFIN_AI_PROJECT_TRACKER.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Implementation_Progress.md`

## Documentation Rules

- If documentation and code diverge, update the docs in the same change.
- Current runtime/deployment docs must describe **native Debian 12** only.
- Historical reports may mention older Docker-era design decisions, but they should not be mistaken for current operational guidance.
