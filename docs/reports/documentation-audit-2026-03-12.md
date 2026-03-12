# Rustyfin Documentation Audit
Date: 2026-03-12

## Scope

This audit reviewed the current Rustyfin documentation against the live project shape:

- native Debian 12 runtime only
- supervised `systemd` deployment
- PostgreSQL-only database runtime
- current `Servers` product area focused on Minecraft
- current playback behavior centered on HLS/resume/continue-watching

## Findings

### 1. Authoritative docs were mostly correct but missing current product behavior

The main entry docs already described the Debian 12 native runtime correctly, but they were light on several now-live behaviors:

- Continue Watching and resume flow
- current Play Together game set
- Minecraft server wizard and auto-provision behavior
- post-start healthcheck service
- protected UI routing behavior

### 2. Older trackers were still easy to misread as current truth

Two older tracker documents still presented themselves as live implementation sources:

- `/Users/iwanteague/Desktop/Rustyfin/docs/project/RUSTFIN_AI_PROJECT_TRACKER.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Implementation_Progress.md`

They contained stale wording around:

- WAL mode
- Direct Play as a primary UI playback path
- current source-of-truth expectations

### 3. The docs index needed a clearer path to current truth

The docs index already separated current docs from archive material, but it did not explicitly surface this audit or call out the older trackers as historical documents that should not drive operational decisions.

## Changes made

Updated:

- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/project/RUSTFIN_AI_PROJECT_TRACKER.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Implementation_Progress.md`

## Current documentation authority

The repository should treat these as the current documentation authority, in order:

1. `/Users/iwanteague/Desktop/Rustyfin/README.md`
2. `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`
4. `/Users/iwanteague/Desktop/Rustyfin/docs/operations/debian-12-native-runtime.md`
5. `/Users/iwanteague/Desktop/Rustyfin/docs/setup-wizard/Rustyfin_Setup_Wizard_Package/Rustyfin_Setup_Wizard_Spec_v4_OpenAPI_Sequence.md`

## Recommendation

When future product/runtime behavior changes, update the authority set above in the same change. Treat reports, trackers, and older plans as secondary context unless they are explicitly promoted into the authority list.
