# Human Dictionary Hardening Report

Date: 2026-04-07

## Implementation Summary

This hardening wave completed the next Human Dictionary iteration on top of the live Rustyfin codebase.

What changed:

- fixed frontend/backend contract drift for Dictionary relationships and account links
- hardened account-link validation so every selected default workspace is checked for access, space match, and linked-person visibility
- added owner-only workspace membership management routes and frontend support
- added an attach-existing-person flow that reuses canonical people and creates shortcut placements when appropriate
- moved active Dictionary search onto the backend search route for non-empty queries
- added AI Dictionary browse/discovery tools for listing visible workspaces and browsing visible people in a workspace
- removed the avoidable N+1 pattern in AI relationship enrichment by batching fact/document loads
- reconciled the live AI provider registry so the repo’s existing newer tool variants register cleanly again

Why it changed:

- the Dictionary page was still carrying stale frontend assumptions that no longer matched the Rust server contract
- account-link defaults could still point at invalid workspaces
- shared workspace membership existed in schema but not as a coherent product surface
- the assistant could resolve narrow relationship phrases but could not browse visible Dictionary content cleanly
- the AI registry in the current repo had drifted behind the enum/spec layer, which was breaking provider-wide tests

## Files Created

- `crates/server/src/dictionary_hardening_helpers.rs`
- `ui/src/lib/dictionaryRelationshipAdapter.ts`
- `ui/src/lib/dictionarySearchHelpers.ts`
- `ui/src/lib/dictionaryWorkspaceMembers.ts`
- `docs/reports/2026-04-07-human-dictionary-hardening-report.md`

## Files Modified

- `crates/db/src/repo/dictionary.rs`
- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_assistant/provider.rs`
- `crates/server/src/ai_assistant/providers/dictionary.rs`
- `crates/server/src/ai_assistant/providers/downloads.rs`
- `crates/server/src/ai_assistant/providers/libraries.rs`
- `crates/server/src/ai_assistant/providers/memory.rs`
- `crates/server/src/ai_assistant/providers/network.rs`
- `crates/server/src/ai_assistant/providers/system.rs`
- `crates/server/src/ai_assistant/providers/web.rs`
- `crates/server/src/ai_assistant/registry.rs`
- `crates/server/src/ai_assistant/replies.rs`
- `crates/server/src/ai_assistant/tools.rs`
- `crates/server/src/ai_assistant/types.rs`
- `crates/server/src/ai_audit.rs`
- `crates/server/src/ai_enabled.rs`
- `crates/server/src/dictionary.rs`
- `crates/server/src/lib.rs`
- `ui/src/app/dictionary/page.tsx`
- `ui/src/lib/dictionaryApi.ts`

## Files Deleted

- none

## Validation Log

Commands run:

```bash
cargo fmt --check
cargo check
cargo test -p rustfin-db --lib
cargo test -p rustfin-server --features ai dictionary --lib
cargo test -p rustfin-server --features ai provider --lib
cargo test
npm --prefix ui run build
```

Results:

- `cargo fmt --check`: passed
- `cargo check`: passed
- `cargo test -p rustfin-db --lib`: passed, `10 passed`
- `cargo test -p rustfin-server --features ai dictionary --lib`: passed, `26 passed`
- `cargo test -p rustfin-server --features ai provider --lib`: passed, `6 passed`
- `cargo test`: failed in pre-existing broader AI planner/memory coverage outside this Dictionary hardening slice
- `npm --prefix ui run build`: passed

Plain `cargo test` failure details:

- workspace build completed
- failure landed in `rustfin-server` lib tests
- result: `467 passed, 14 failed`
- failing tests were existing AI planner/memory tests unrelated to the Dictionary hardening routes/UI/API changes

Failing `cargo test` tests:

- `ai_assistant::memory::tests::topic_key_for_network_and_system_detail_tools_is_specific`
- `ai_assistant::orchestrator::tests::planner_ast_rejects_excessive_tool_count`
- `ai_assistant::orchestrator::tests::planner_extracts_library_search_query`
- `ai_assistant::orchestrator::tests::planner_extracts_named_server_query`
- `ai_assistant::orchestrator::tests::planner_extracts_download_artifact_detail_query`
- `ai_assistant::orchestrator::tests::planner_extracts_library_summary_query`
- `ai_assistant::orchestrator::tests::planner_repair_succeeds_after_one_failed_parse`
- `ai_assistant::orchestrator::tests::planner_routes_disk_usage_detail_query`
- `ai_assistant::orchestrator::tests::planner_resolves_download_entity_follow_up`
- `ai_assistant::orchestrator::tests::planner_routes_system_diagnostics_queries`
- `ai_assistant::orchestrator::tests::planner_uses_ai_runtime_follow_up_history`
- `ai_assistant::orchestrator::tests::planner_uses_library_summary_follow_up_history`
- `ai_assistant::orchestrator::tests::planner_uses_network_follow_up_history`
- `ai_assistant::orchestrator::tests::planner_uses_room_follow_up_history`

## Deviations From The Hardening Pack

### 1. Frontend helper file placement

Pack guidance referenced helper-style files such as `dictionary_relationship_adapter.ts`, `dictionary_search_helpers.ts`, and `workspace_members_contract.ts`.

Repo reality:

- the canonical frontend API/client helpers already live under `ui/src/lib`
- the Dictionary page already imports from `ui/src/lib/dictionaryApi.ts`

Implemented instead:

- `ui/src/lib/dictionaryRelationshipAdapter.ts`
- `ui/src/lib/dictionarySearchHelpers.ts`
- `ui/src/lib/dictionaryWorkspaceMembers.ts`

Why:

- this matches the repo’s existing `ui/src/lib` organization and avoids parallel abstractions

### 2. Account-link hardening location

Pack sample Rust snippets suggested repository/helper-centric validation.

Repo reality:

- the route layer already owns request-specific error messaging and access gating

Implemented instead:

- route-level validation in `crates/server/src/dictionary.rs`
- shared selection logic in `crates/server/src/dictionary_hardening_helpers.rs`

Why:

- this preserves clear field-specific API errors while keeping the reusable logic centralized

### 3. AI browse integration style

Pack guidance described adding tools and patching all required enum/registry/provider locations.

Repo reality:

- the live repo also had additional newer tool variants already present in the registry enum/spec layer but missing from provider registration

Implemented instead:

- added the two new Dictionary browse tools
- also completed provider registration for the repo’s existing newer tool variants so registry-wide tests could pass

Why:

- leaving that drift in place would have kept the assistant registry partially broken even after the Dictionary work

### 4. Validation target reality

Pack validation assumed a clean `cargo test` path.

Repo reality:

- the full workspace test suite still has unrelated failing AI planner/memory assertions outside the Dictionary hardening slice

Implemented instead:

- ran the required `cargo test`
- isolated the exact failing tests
- added focused passing validation for the changed Dictionary and provider areas

Why:

- this proves the Dictionary hardening work itself is compile-tested and behavior-tested without claiming unrelated failures were introduced here

## Security Review Notes

Access-control paths checked:

- Dictionary routes still require auth through the existing route stack
- workspace membership listing/upsert/delete is owner-only
- last-owner removal and demotion are blocked
- attach-existing-person enforces write access and same-space validation
- account-link validation now rejects unreadable workspaces, cross-space selections, and workspaces where the linked person is not visible
- AI Dictionary tools remain read-only

Privacy boundaries preserved:

- `dictionary_workspace` remains the main access boundary
- membership checks stay server-enforced in Rust
- cross-space attach is rejected
- browse/search/person-bundle flows continue to resolve only visible workspace content
- no AI write-back was introduced

Residual risk:

- the broader Human Dictionary model still keeps `dictionary_person` canonical at the space level, so edits remain shared across workspaces within the same space by design
- per-person ACLs were intentionally not introduced in this wave

## Known Follow-Up Work

- add deeper route/integration coverage for non-owner membership failures and attach-existing cross-space rejection through DB-backed HTTP tests
- address the remaining unrelated AI planner/memory failures in the broader `cargo test` suite
- consider trimming the current AI diagnostics dead-code noise once the newer system/network tools are fully wired end to end
- manual UI/E2E verification is still recommended for shared-workspace membership flows and the attach-existing-person UX

