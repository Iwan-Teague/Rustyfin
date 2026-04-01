# Rustyfin AI Assistant Delta Implementation Prompt

Date: 2026-04-01  
Scope: implement the first execution slice of the AI delta plan, deploy it to the Ubuntu server, and verify it there

## Read First

Before changing anything, read these files in order:

1. `/Users/iwanteague/Desktop/Rustyfin/README.md`
2. `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwanteague/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
5. `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`

Treat `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md` as the implementation source of truth for this task.

## Objective

Implement the first major execution slice of the AI delta plan:

- Phase 0: correctness, honesty, and timing
- Phase 1: conversation persistence and navigation
- Phase 2: assistant activity view and observability

Then deploy the result to the Ubuntu server, verify the live runtime, and report exactly what was completed versus deferred.

Do not stop at planning. Make the code changes, run the tests you can run, deploy to the live Ubuntu server, and verify the deployed behavior.

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Do not expose raw chain-of-thought as a product feature.
- The visible `Thinking...` UI must be driven by structured server activity/status events, not by relying on raw model-emitted `<think>` content as the canonical product behavior.
- Keep admin audit persistence separate from user-visible conversation persistence.
- Do not weaken server-side auth or tool policy.
- Do not add write-capable assistant actions in this slice. Phase 3 is deferred.
- Preserve compatibility for the current `POST /api/v1/ai/chat` route while introducing the conversation-backed flow.
- Keep the Rustyfin visual identity: cinematic dark UI with the orange-to-pink-to-purple accent language.
- Reuse existing Rustyfin sidebar/mobile interaction patterns where appropriate instead of inventing unrelated UI behavior.
- If you create a branch, do not use a branch name with `codex` in it.
- All commits must use:
  - `user.name = Iwan-Teague`
  - `user.email = teague.iwan@outlook.com`

## Current Local Baseline

As of this handoff:

- local branch: `main`
- local commit: `6604208`

If local `main`, `origin/main`, and the live server checkout differ, reconcile them before deploying. Use the latest correct commit and document what was changed.

## Ubuntu Server Access

Use the live Ubuntu server credentials provided out-of-band when you execute this prompt.

Known connection target:

- SSH target: `server@192.168.0.36`

Important handling rules:

- use the credential only for this deployment task
- do not commit it
- do not write it into repo files
- do not leave it in shell history or temporary files unnecessarily

## Server Discovery Requirements

Do not assume the active live checkout path. Discover it first.

On the Ubuntu server:

1. identify the active Rustyfin systemd unit configuration
2. identify the live working directory actually used by the service
3. verify whether the live checkout is on `main` and whether it matches local `main`
4. if multiple Rustyfin trees exist, deploy only to the active one or perform a clearly documented safe cutover

Use evidence such as:

- `systemctl cat rustyfin-native.service`
- `systemctl show rustyfin-native.service`
- running process command lines
- current working directory of the service processes

Do not blindly deploy into an inactive or stale checkout if the live service is using a different tree.

## Deliverables

### Phase 0 Deliverables

Implement all of the following:

1. richer AI turn timing
   - add explicit planner/tool/generation/end-to-end timing
   - preserve backward compatibility with existing `total_duration_ms` during transition
2. host-runtime answer normalization
   - return human-readable RAM values from the grounded tool itself
   - do not rely on the model to convert raw bytes cleanly
3. deterministic unsupported-write refusal
   - if the user asks the assistant to create/edit/delete data and no supported write-capable AI tool exists, return a clear server-authored refusal instead of allowing faux-success text
4. deterministic `next event`
   - add a server capability such as `calendar_get_next_event`
   - do not rely on the model inferring “next” from a generic list

### Phase 1 Deliverables

Implement all of the following:

1. conversation persistence
   - add dedicated PostgreSQL tables for conversations and turns
   - do not reuse `ai_assistant_audit_event`
2. conversation APIs
   - list/create/get/update/delete conversations
   - strict per-user ownership checks
3. conversation-backed streaming
   - add a conversation-based message streaming route
   - keep the old `/api/v1/ai/chat` route working during migration
4. UI conversation shell
   - desktop left rail
   - mobile hamburger + left drawer
   - real `New chat`
   - stored history
   - rename/archive/delete flows

### Phase 2 Deliverables

Implement all of the following:

1. assistant activity stack
   - `Thinking...` row
   - ordered tool/function rows beneath it
   - final answer beneath that
2. Codex-style thinking affordance
   - subtle animated sweep through the `Thinking...` row
   - calm, technical motion
   - not flashy
3. structured activity events
   - extend SSE semantics so the UI can render explicit phases and tool activity
4. remove the current loose source-chip-first presentation from the main answer flow
5. keep exact tool names available as secondary detail if needed, but default to user-readable labels

## Concrete Repo Targets

You are expected to touch the real Rustyfin files implicated by the design doc. At minimum, inspect and modify the appropriate subset of:

### Frontend

- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts`
- existing shared/mobile interaction patterns such as:
  - `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/channels/page.tsx`

If the AI page gets too large, create a dedicated feature module under:

- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-assistant/`

### Backend

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/runtime_diagnostics.rs`
- likely new server modules:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_conversations.rs`

### Database

- add the next appropriate PostgreSQL migrations under:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/`
- add matching repo modules under:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/`

### Calendar

- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/calendar.rs`

## Required Data / API Shape

Follow the design doc’s concrete recommendations unless you discover a strong repo-specific reason to adapt them.

At minimum, implement:

- richer turn stats fields
- explicit user-visible conversation objects
- stored assistant activity traces per assistant turn
- deterministic `calendar_get_next_event`
- structured SSE events or a compatibility-preserving evolution of them that supports:
  - thinking/planning
  - tool running/completed/error
  - generating

## UI Behavior Requirements

The new `/ai` page should behave like this:

1. desktop
   - persistent left rail with recent chats
   - `New chat` at the top
   - active conversation in the main panel
2. mobile
   - a hamburger icon at top left
   - a left drawer that opens with chat history
   - selecting a chat closes the drawer
3. assistant turn rendering
   - `Thinking...` row while work is in progress
   - neat tool-call list beneath it
   - final answer below
   - turn stats below the answer

Do not keep the current “grounding source chips floating above the answer bubble” as the main activity presentation.

## Testing Requirements

Run the relevant local verification commands before deployment. Use the strongest reasonable subset based on the files you changed.

At minimum, run:

- `cargo fmt --all`
- `cargo check`
- `cargo test` for the affected Rust crates
- `npm --prefix ui run build`

Also add and run targeted tests for:

- deterministic `next event`
- unsupported-write refusal
- conversation ownership and ordering
- conversation CRUD
- any new repo logic you add

If browser or E2E coverage exists and is practical, add or update focused tests for:

- mobile drawer behavior
- conversation switching
- assistant activity stack rendering

If any test cannot be run, state exactly why.

## Deployment Requirements

After local verification:

1. push the implementation to GitHub
2. update the live Ubuntu server checkout to that exact commit
3. deploy using the host’s existing Rustyfin runtime/deploy model
4. verify the service comes back healthy
5. verify the `/ai` endpoints and UI-relevant behavior as far as the environment allows

Prefer the existing native deploy path already used on that host. If the service is installed via a wrapper or systemd path that differs from the Debian docs, follow the live host’s actual configuration rather than forcing a new runtime model.

## Ubuntu Verification Requirements

After deployment, verify at least the following:

- active service units are healthy
- the HTTPS UI responds
- the backend AI routes respond as expected
- the active server checkout points to the deployed commit
- the AI page can:
  - load models
  - create a new conversation
  - persist and reload conversation history
  - show the new activity stack
  - answer a host-runtime RAM question in human units
  - answer `What is my next event?` correctly if calendar data exists

If fully authenticated browser verification is not possible in the environment, get as close as possible with:

- authenticated API calls
- server logs
- runtime checks
- explicit documentation of what remains unverified

## Search Handles

Use these to move quickly in the repo:

- `AssistantStatusEvent`
- `plan_tool_calls_with_model_assist`
- `system_get_host_runtime_summary`
- `total_duration_ms`
- `status_label_for_tool_call`
- `handleNewChat`
- `messages: ChatEntry[]`
- `ThinkingBlock`
- `StatusList`
- `groundingSources`
- `sidebarOpen`
- `Toggle sidebar`
- `ai_assistant_audit_event`

## Reporting Requirements

When you finish, report:

1. what you implemented
2. exactly which phases/subsections are complete versus still partial
3. all commands run and their results
4. deployment commands run on the Ubuntu server
5. live verification results on the Ubuntu server
6. every file changed
7. any remaining gaps relative to `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`

Do not give a vague summary. Be explicit.

## Final Instruction

Do not respond with only a plan. Start implementing immediately, carry the work through code changes, tests, deployment, and verification, and only stop when this first slice is actually landed and checked on the Ubuntu server.
