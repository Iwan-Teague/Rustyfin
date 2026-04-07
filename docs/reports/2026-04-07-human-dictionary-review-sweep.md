# Human Dictionary Review Sweep

Date: 2026-04-07

## Scope

This was a read-only review of the current Human Dictionary implementation in Rustyfin.

The review focused on:

- backend access control and privacy boundaries
- schema and repository efficiency
- API completeness versus the product goals
- UI data-loading patterns
- AI retrieval coverage for Human Dictionary reads

No code changes were made as part of this sweep.

## High-Level Assessment

The Human Dictionary feature is real and usable today:

- the schema is graph-capable enough for a tree-first v1
- the core backend logic is Rust-owned
- workspace access checks are mostly enforced server-side
- the `/dictionary` page is functional
- the AI has read-only retrieval hooks for linked-identity and relationship-relative queries

The main gaps are not in whether the feature exists. They are in whether the current implementation fully matches the privacy and product claims:

- person/profile data is modeled more globally than the workspace UI suggests
- shared access exists in schema but not yet as a user-manageable product surface
- AI retrieval is present but still narrower than the Human Dictionary concept implies
- some retrieval paths are doing avoidable extra database round-trips

## Findings

### P1: Space-scoped person edits can leak across workspaces

The current data model keeps `dictionary_person` and `dictionary_person_alias` at the space level, not the workspace level:

- [055_dictionary_core.sql:68](/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/055_dictionary_core.sql#L68)
- [055_dictionary_core.sql:94](/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/055_dictionary_core.sql#L94)

The route layer only verifies that the caller can write to the current workspace before it updates the underlying global person row:

- [dictionary.rs:927](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs#L927)
- [dictionary.rs:1278](/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/dictionary.rs#L1278)
- [dictionary.rs:1310](/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/dictionary.rs#L1310)

Practical effect:

- if the same person appears in multiple workspaces inside the same space, editing their display name, summary, or aliases in one workspace changes them everywhere

Why this matters:

- the current product language emphasizes private/shared visibility boundaries
- the UI reads like a workspace-oriented people system
- the backend currently behaves more like shared canonical identity inside a space

This is either:

1. an intentional canonical-person design that needs clearer product rules, or
2. a privacy/modeling bug if users expect per-workspace person presentation

## P2: Shared visibility exists in schema, but not as an operable product flow

The schema includes `dictionary_workspace_member`:

- [055_dictionary_core.sql:56](/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/055_dictionary_core.sql#L56)

The route surface currently does not expose member-management endpoints:

- [dictionary.rs:27](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs#L27)

Practical effect:

- the platform can enforce shared access if memberships already exist
- but end users cannot actually manage shared access through the Human Dictionary product surface

Why this matters:

- the feature goal explicitly includes private/shared visibility boundaries
- at the moment, “shared” is primarily architectural readiness, not finished user functionality

## P2: AI support is real, but narrower than the product suggests

The Dictionary provider is registered and exposed to the assistant:

- [providers/dictionary.rs:16](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/providers/dictionary.rs#L16)

Current tools are:

- `dictionary_get_account_identity`
- `dictionary_search_people`
- `dictionary_get_person_bundle`
- `dictionary_resolve_relationship_reference`

Relevant code:

- [tools.rs:2331](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2331)
- [tools.rs:2381](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2381)
- [tools.rs:2456](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2456)

The planner currently auto-routes strongly only for fixed relationship phrases such as:

- `my mother`
- `my brother`
- `my coworkers`

Relevant planner logic:

- [orchestrator.rs:3059](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs#L3059)
- [orchestrator.rs:11300](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs#L11300)

What this means in practice:

- relationship-relative reads are supported
- direct person search is supported if the planner already has a workspace and search target path
- but the assistant still lacks a more general browse/discovery path for the Dictionary

Examples of current likely weak spots:

- “find Rachel in my dictionary”
- “show me everyone in my family dictionary”
- “who is in my work dictionary”
- anything depending on a custom workspace without explicit IDs

So the answer to “have we added the functions for AI retrieval?” is:

- yes, but only for a narrow first slice
- no, not yet for the broader Human Dictionary browsing model implied by the feature

## P2: Relationship resolution has an N+1 query pattern

The AI relationship-resolution tool loads relations once, but then fetches facts and documents per candidate:

- [tools.rs:2526](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2526)
- [tools.rs:2545](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2545)
- [tools.rs:2562](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs#L2562)

Practical effect:

- `my coworkers`
- `my siblings`
- any plural family/work query

can scale poorly as the number of related people grows.

This is not catastrophic for a small household dataset, but it is not the most efficient way to serve grounded AI reads.

## P2: Account linking can save incomplete default workspace mappings

During account linking, the linked person is validated for visibility in the family workspace:

- [dictionary.rs:775](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs#L775)

For friends/work, the code only validates that the workspace belongs to the same space:

- [dictionary.rs:789](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs#L789)

What is missing:

- the linked person is not required to be visible in the chosen friends/work workspace at link time

Practical effect:

- the link can save successfully
- later AI reads can fail because the linked identity is not present in that workspace tree

This shifts a configuration problem from setup time into runtime assistant failures.

## P3: The Dictionary page search is currently client-side over a preloaded list

The UI stores a full `workspacePeople` list and filters it locally:

- [page.tsx:282](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/dictionary/page.tsx#L282)

The workspace load path fetches both:

- tree
- people list

on every workspace switch:

- [page.tsx:331](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/dictionary/page.tsx#L331)

The server already has a dedicated search endpoint/path:

- [dictionary.rs:829](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/dictionary.rs#L829)

But the page currently does not use it for interactive search.

Practical effect:

- search is limited to the preloaded page-size slice
- the UI fetches more than it strictly needs up front
- larger workspaces will feel heavier and less accurate

## P3: The page is doing too much in one file

The Dictionary page is currently about 1,468 lines:

- [page.tsx:1](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/dictionary/page.tsx#L1)

This is not a correctness bug, but it does make:

- behavior harder to reason about
- reload/refetch behavior harder to optimize
- future sharing/tree interaction work harder to extend cleanly

For a first slice this is acceptable, but it is not the most maintainable shape long term.

## AI Capability Review

### What the AI can retrieve now

The grounded assistant can currently:

- confirm which Human Dictionary person the current Rustyfin account is linked to
- search visible people inside one workspace
- fetch a visible person bundle by known workspace and person ID
- resolve relationship-relative prompts like `my mother`, `my brother`, `my friend`, and `my coworkers`
- answer birthdays and hobbies deterministically from those grounded results

### What the AI cannot do well enough yet

The current slice is weaker on:

- broad “browse my dictionary” prompts
- custom workspace exploration
- multi-step discovery like “find Rachel, then tell me about her”
- general relationship reasoning outside the fixed recognized prompt patterns

### Bottom line on AI retrieval

The retrieval foundation is in place.

The current implementation is enough for:

- safe relationship-relative reads
- narrow direct lookup flows

It is not yet enough to say the assistant has full Human Dictionary retrieval coverage.

## Efficiency Review

### What is already efficient enough

- workspace access checks are simple and explicit
- visible-people search uses indexed text paths and workspace-scoped joins
- tree loading is straightforward
- person bundle reads are bounded to one workspace/person

### What is not yet as efficient as it could be

- AI relationship resolution does repeated fact/document lookups per candidate
- UI search does not lean on the backend search path
- the page refetch pattern reloads broad slices after many writes

## Recommended Next Improvements

Priority order:

1. Decide the canonical privacy model for `dictionary_person`
2. Add real membership-management routes and UI
3. Tighten account-link validation so all default linked workspaces must actually contain the linked person
4. Add AI browse tools for visible workspace listing and safer direct person discovery
5. Collapse AI relation-candidate enrichment into fewer queries
6. Move Dictionary UI search onto the backend search path
7. Split the page into smaller view/state modules once the behavior is stable

## Suggested Acceptance Criteria For The Next Iteration

- editing a person cannot accidentally expose or mutate data outside the intended privacy boundary
- users can explicitly share/unshare workspaces through the product
- account linking fails early if the linked person is not visible in the selected default workspaces
- AI can answer both relationship-relative questions and broader direct Human Dictionary discovery prompts
- plural relationship reads do not require one fact/document round-trip per candidate
- Dictionary search remains accurate for large workspaces

## Assumptions

This review assumes:

- a person may intentionally appear in more than one workspace within a space
- “shared visibility” is meant to be product-visible functionality, not just schema readiness
- AI writes remain intentionally disabled for now

## Conclusion

The Human Dictionary feature is a solid first implementation, not a placeholder. The biggest improvements are now in:

- tightening privacy semantics
- finishing the shared-access product surface
- broadening AI retrieval beyond the first relationship slice
- and reducing a few avoidable query and UI inefficiencies

The most important question to settle before more feature growth is this:

Should a `person` be canonical across a whole space, or should more of the editable profile surface become workspace-scoped?

That answer determines whether the current behavior is acceptable architecture or the main bug to fix next.
