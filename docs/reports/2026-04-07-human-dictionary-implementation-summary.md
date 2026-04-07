# Human Dictionary Implementation Summary

Date: 2026-04-07

## What Changed

- Added the Human Dictionary PostgreSQL schema in [055_dictionary_core.sql](/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/055_dictionary_core.sql).
- Added the Rust repository layer in [dictionary.rs](/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/dictionary.rs) for:
  - spaces and workspaces
  - memberships
  - people and aliases
  - tree nodes
  - relationship pairs
  - facts
  - documents
  - account links
- Added authenticated Dictionary routes in [dictionary.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs) and mounted them from [routes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs).
- Replaced the placeholder `/dictionary` page with a working tree-oriented UI in [page.tsx](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/dictionary/page.tsx).
- Added the frontend API client in [dictionaryApi.ts](/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/dictionaryApi.ts).
- Added Human Dictionary assistant tooling and deterministic replies across:
  - [types.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs)
  - [registry.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs)
  - [provider.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/provider.rs)
  - [providers/mod.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/providers/mod.rs)
  - [providers/dictionary.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/providers/dictionary.rs)
  - [tools.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs)
  - [orchestrator.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs)
  - [replies.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/replies.rs)
  - [ai_enabled.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs)
  - [ai_audit.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_audit.rs)

## Implemented Scope

- Workspace bootstrap for family, friends, and work.
- Tree-oriented browsing through seeded root/group nodes.
- Create, update, list, and archive people inside a workspace.
- Relationship pair create, patch, delete, and resolved relationship listing.
- Person facts and document editing.
- Rustyfin account to Dictionary person linking.
- Read-only assistant support for:
  - linked identity lookup
  - visible people search
  - person bundle fetch
  - relationship-relative reads such as:
    - `When is my mother's birthday?`
    - `What are my brother's hobbies?`
    - `Who are my co-workers?`

## Deviations From The v2 Bundle

- Route shape is workspace-nested for person and relationship operations, for example:
  - `/api/v1/dictionary/workspaces/{workspace_id}/people`
  - `/api/v1/dictionary/workspaces/{workspace_id}/relationships`
  Reason:
  Rustyfin already tends to anchor resource operations under an explicit parent scope when server-side access checks depend on that scope.

- The schema uses owner-scoped default-space uniqueness instead of a global single default space.
  Reason:
  a global default-space uniqueness rule would prevent more than one Rustyfin account from bootstrapping Dictionary state cleanly.

- `dictionary_relation` includes `relation_group_key`.
  Reason:
  pair identifiers alone are not strong enough for reliably updating and deleting symmetric relationship pairs.

## Security Notes

- Dictionary access is enforced server-side through workspace membership and write-role checks.
- Reads require workspace membership.
- Writes require owner/editor membership.
- Account linking validates same-space membership and visible-person membership.
- The assistant Dictionary path is read-only.
- No AI write path was enabled for Dictionary mutations.

## Validation

Passed:

- `cargo fmt --all`
- `cargo check -p rustfin-server --features ai`
- `npm --prefix ui run build`
- `cargo test -p rustfin-db --lib`

Focused Dictionary-related tests were added and are included in the server test target:

- Dictionary route auth guard test
- Dictionary reply formatter tests
- Dictionary planner routing tests

## Current Limitation

The broad `cargo test -p rustfin-server --features ai --lib` target is not green in this worktree, but the remaining failures are in pre-existing unrelated AI planner/memory expectations outside the Human Dictionary slice. The Human Dictionary-specific code compiles, the frontend build passes, the DB test target passes, and the new Dictionary-focused tests passed in the earlier full server run output before unrelated failures stopped the suite.
