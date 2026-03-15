# Rustyfin AI Grounded Tools Architecture

Date: 2026-03-15
Last revised: 2026-03-15
Status: implementation tracker

## Purpose

Rustyfin AI should stop behaving like an ungrounded local chat box and start behaving like a real assistant for the Rustyfin product.

The adopted direction is:

- keep the AI model local
- keep the end-user surface chat-focused at `/ai`
- add a server-side suite of scoped tools/functions that the assistant can call
- use those tools to fetch real Rustyfin state such as calendar events, active rooms, libraries, server status, and account-scoped context

This document defines the architecture, boundaries, rollout order, and security model for that grounded assistant system.

## Tracker

### Completed

- server-side grounded assistant module exists under `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant`
- grounded `/api/v1/ai/chat` orchestration is live behind the existing authenticated chat route
- current grounded read-only tools exist for:
  - `account_get_profile_summary`
  - `calendar_list_events`
  - `calendar_upcoming_birthdays`
  - `downloads_list_available_artifacts`
  - `libraries_list_accessible`
  - `library_search_titles`
  - `library_get_item_summary`
  - `rooms_list_active`
  - `rooms_get_room_summary`
  - `system_get_host_runtime_summary`
  - `servers_list_minecraft_status`
  - `servers_get_minecraft_server_summary`
- query understanding exists for calendar windows, room-mode filtering, named/availability-filtered Minecraft server queries, and ambiguity clarification for underspecified calendar or singular server prompts
- admin-only host/runtime stats grounding now exists for RAM, memory usage, CPU thread count, CPU usage, load, and uptime questions, backed by the shared runtime-diagnostics path
- grounded follow-up behavior exists for:
  - domain reuse such as `What about next week?`
  - filtered follow-ups such as `Which ones are healthy?`
  - entity references such as `the second one`, `that server`, or `the first room`
- streamed status/provenance exists on `/ai`, including source chips and progress steps before answer tokens
- runtime assistant logging and diagnostics now include per-request trace IDs plus assistant chat/tool counters in runtime diagnostics
- tool registry policy is now enforced at execution time, so admin-only or write-capable tools stay blocked unless the registry and runtime flow both allow them
- `/api/v1/ai/chat` now uses a model-assisted structured planner for tool selection, with strict registry/role validation plus deterministic fallback and deterministic entity-follow-up resolution
- admin-only constrained public web tools now exist for `web_search_public_web` and `web_fetch_public_page_summary`, gated behind `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1` with SSRF/private-network blocking and bounded public-page extraction
- assistant integration coverage now proves library-access scoping, public-room-only visibility, calendar visibility, Minecraft server access scoping, authenticated downloads catalog grounding, and admin-only host runtime gating through grounded turn preparation
- dedicated assistant audit persistence now exists with admin-readable recent request history in the Admin `AI` tab
- assistant audit retention/pruning policy now exists with a 30-day default window, hourly cleanup, and `RUSTFIN_AI_AUDIT_RETENTION_DAYS` override support

### In Progress

- permission-bound integration/security testing is now in place across the current differentiated grounded domains, but it is still lighter than the design target for broader lifecycle, failure, and future-domain coverage

### Future

- network grounding tools
- richer entity detail tools for calendar events
- write-capable tools with confirmation-token or protected-action gating
- admin-only assistant mode, if deliberately designed later

## Current State

The current AI implementation now has an initial grounded backend slice, but it is still early and intentionally narrow.

Current flow:

1. the client sends `model`, `message`, and chat `history` to `POST /api/v1/ai/chat`
2. the server builds a read-only grounded turn when the message matches supported product domains
3. grounded tool results are attached as authoritative server-side context
4. the model streams a plain text response

Current implementation points:

- user-facing route: `/api/v1/ai/chat` in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- AI chat client: `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts`
- end-user AI page: `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx`
- engine/inference primitives: `/Users/iwanteague/Desktop/Rustyfin/crates/ai-agent`
- grounded assistant modules: `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant`

Important limitation:

- the grounded path now uses a model-assisted structured planner, but the backend still keeps deterministic fallback and follow-up/entity normalization as the safety net
- the current tool set is read-only and deliberately small
- constrained public web search/page fetch now exists, but it is admin-only and disabled by default unless `RUSTFIN_AI_PUBLIC_WEB_ENABLED=1` is set on the host
- independent read-only tools can now execute in parallel in the backend orchestration layer
- there are no write tools or confirmation flows yet
- room grounding currently reflects active public rooms, not every possible room visibility edge case
- `/api/v1/ai/chat` now emits explicit server-driven tool-status events before token output so the UI can show user-visible progress without exposing hidden reasoning
- tool execution now hard-enforces the registry’s read-only, role, and confirmation policy instead of treating those fields as documentation only

Current implemented query-understanding improvements:

- common calendar windows are extracted server-side for `today`, `tomorrow`, `this week`, `next week`, `this month`, and `next month`
- numbered windows such as `next 10 days`, `next 2 weeks`, and `next 2 months` are supported
- explicit ISO dates can be targeted directly
- library title search heuristics now distinguish title-search intent from generic library-access questions
- ambiguous calendar requests without a useful time window now short-circuit to a clarification response instead of silently defaulting
- room queries can now narrow active public rooms by mode such as `youtube`, `watch`, `audio`, `web`, `screen`, `create`, and `play`
- Minecraft server queries can now narrow by availability such as `online`, `offline`, `healthy`, or `failed`
- Minecraft server queries can also target a named server when the prompt includes a quoted or explicitly named server
- singular server prompts such as “Is the server online?” now short-circuit to a clarification response instead of guessing which server was intended
- the UI can now render compact progress steps like `Checking calendar events for this week` before the final model answer begins streaming
- short follow-up prompts can now reuse the last grounded tool names as non-authoritative planner hints, but the backend still reruns fresh server-side reads before answering
- the assistant can now carry a minimal hidden follow-up context for the last grounded result set so references like `the second one` or `that server` can be resolved and re-grounded safely
- follow-up entity references can now escalate from a list tool to a tighter detail tool for the selected library item, active room, or Minecraft server
- explicit public URLs can now route to a constrained page-summary tool
- weather/current-public-info prompts can now route to constrained public web search when the host enables public web tools

## Goal

The assistant should be able to answer questions like:

- “What events are coming up this week?”
- “Are any rooms active right now?”
- “Are any YouTube rooms active right now?”
- “What music libraries do I have access to?”
- “What Minecraft servers are online?”
- “Is the Minecraft server called Survival online?”
- “How much RAM is the server using right now?”
- “How many CPU threads does the server have?”
- “What downloads are available right now?”
- “What about next week?”
- “Which ones are healthy?”
- “What about the second one?”
- “What birthdays are coming up?”

without hallucinating, and without broad or unsafe backend access.

## Non-Goals

Not part of the first implementation:

- arbitrary code execution
- shell access from the model
- raw SQL generation from the model
- direct model access to the full database
- agentic write actions such as creating calendar events or ending rooms
- admin-only AI model management changes on the end-user `/ai` page

The first grounded rollout should be read-only.

## Architecture Review

The current direction is modern and correct, but the original draft was still missing some production-grade detail.

What is already right about this approach:

- server-side tools instead of letting the model touch data sources directly
- typed backend boundaries instead of prompt-only behavior
- small public API surface
- read-only first rollout
- reuse of existing Rustyfin domain logic instead of building an AI-only shadow backend

What needed to be more explicit:

- how tool calling should stay model-provider-agnostic
- exactly what functions the assistant will be allowed to call
- how read vs write permissions are enforced
- how sensitive writes are confirmed
- how the UI exposes grounded status and tool provenance without exposing chain-of-thought
- how audit, retention, rate limiting, and failure behavior should work

This revised document makes those controls explicit.

## Recommended Architecture

### Core Principle

The model should not fetch Rustyfin data directly.

Instead:

1. the model receives the user request and a compact system prompt
2. the backend decides whether a registered tool should be called
3. the backend runs that tool with the authenticated user’s permissions
4. the backend feeds the compact structured result back into the model
5. the model writes the final user-visible answer

This is the correct boundary for accuracy, safety, and maintainability.

### High-Level Shape

- `ui/src/app/ai/page.tsx`
  - remains a chat UI only
- `ui/src/lib/aiApi.ts`
  - remains a thin transport client
- `crates/server/src/ai_enabled.rs`
  - becomes the orchestration entrypoint for grounded chat
- new backend module namespace, recommended:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_tools/`
  - or `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/`
- `crates/ai-agent`
  - stays focused on inference primitives and model interaction

Recommended backend split:

- `ai_enabled.rs`
  - HTTP route and SSE streaming
- `ai_tools/mod.rs`
  - tool registry and dispatch
- `ai_tools/types.rs`
  - tool schemas, request/response types
- `ai_tools/context.rs`
  - auth-scoped assistant context
- `ai_tools/orchestrator.rs`
  - model loop, tool execution loop, prompt assembly
- `ai_tools/<domain>.rs`
  - domain-specific tools such as calendar, rooms, libraries, servers

### Model Integration Strategy

The tool system should not depend on one model vendor or one model-specific function-calling format.

Recommended rule:

- Rustyfin owns an internal tool-call protocol
- model integrations adapt to that protocol
- if a model supports native structured tool calling reliably, Rustyfin can use it
- if a model does not, Rustyfin should fall back to constrained JSON output with strict backend validation

This keeps the system portable across local models and future backend changes.

Recommended adapter layers:

- `model_adapter`
  - converts Rustyfin tool specs into the format expected by the current model backend
- `tool_call_parser`
  - parses model output into validated internal tool calls
- `tool_call_validator`
  - rejects unknown tools, malformed args, oversized payloads, and disallowed access

### Recommended Orchestrator Pattern

For local-model reliability, a two-stage orchestration pattern is better than a loose freeform loop.

Recommended flow:

1. classify the user request and decide whether tools are needed
2. produce a structured tool plan
3. validate the requested tools and arguments in Rust
4. execute independent read-only tools in parallel where safe
5. assemble a compact grounded context packet
6. ask the model for the final user-visible answer
7. stream answer tokens plus lightweight status metadata to the UI

This is more stable than letting a small local model improvise tool calls indefinitely.

Recommended limits:

- planner pass should use low temperature
- final answer pass can use a slightly more natural setting
- independent read-only tools may fan out in parallel
- write tools should never fan out automatically

### Why This Is A Modern Approach

This design uses the techniques that actually matter for a modern local-product assistant:

- server-side tool calling instead of prompt stuffing
- typed structured tool schemas
- least-privilege execution
- model-agnostic adapters
- source-aware responses
- streaming status feedback
- bounded orchestration loops
- explicit audit and rate limiting

What should not be forced into phase 1:

- a vector database
- autonomous multi-agent behavior
- arbitrary web browsing
- broad write access

Those are not automatically “more modern.” In this product, they would mostly add risk and complexity.

## Why Tools Instead Of Prompt Stuffing

Prompt stuffing alone is the wrong long-term architecture.

Problems with prompt-only:

- stale or missing information
- poor permission boundaries
- model hallucination
- prompt bloat
- no clean audit trail of what data was accessed

Benefits of tools:

- current data at answer time
- user-scoped authorization on each query
- compact structured outputs
- explicit logging and auditability
- clean extension path as Rustyfin grows

## Tool Model

### Recommended Tool Contract

Each tool should have:

- a stable name
- a narrow purpose
- a typed input schema
- a typed output schema
- a maximum output size budget
- explicit auth behavior
- no side effects in phase 1
- explicit access mode
- explicit risk tier
- explicit confirmation policy
- explicit audit policy
- explicit timeout and output budget

Recommended Rust shape:

```rust
pub struct AssistantToolSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub access_mode: ToolAccessMode,
    pub risk_tier: ToolRiskTier,
    pub required_role: ToolRoleRequirement,
    pub confirmation: ToolConfirmationPolicy,
    pub timeout_ms: u64,
    pub max_result_bytes: usize,
    pub cache_policy: ToolCachePolicy,
    pub audit_level: ToolAuditLevel,
}
```

Suggested enums:

- `ToolAccessMode`
  - `ReadOnly`
  - `Write`
  - `DestructiveWrite`
- `ToolRiskTier`
  - `Low`
  - `Moderate`
  - `High`
  - `Critical`
- `ToolRoleRequirement`
  - `AnyAuthenticatedUser`
  - `AdminOnly`
- `ToolConfirmationPolicy`
  - `None`
  - `ExplicitUserConfirm`
  - `ProtectedAction`

Example shape:

- `calendar_list_events`
- `calendar_upcoming_birthdays`
- `rooms_list_active`
- `libraries_list_accessible`
- `library_search_titles`
- `servers_list_minecraft_status`
- `account_get_profile_summary`

### Tool Result Style

Do not return raw database rows to the model unless unavoidable.

Preferred tool output:

- human-meaningful
- compact
- normalized
- permission-filtered
- bounded in length

Example:

- good: `[{ "title": "Birthday dinner", "date": "2026-03-18", "scope": "personal" }]`
- bad: full table rows with internal IDs, audit metadata, and unused columns

### Function Inventory

The assistant should expose a small, explicit function inventory, grouped by risk and product domain.

Phase 1 read-only user tools:

- `calendar_list_events`
- `calendar_upcoming_birthdays`
- `downloads_list_available_artifacts`
- `rooms_list_active`
- `libraries_list_accessible`
- `library_search_titles`
- `servers_list_minecraft_status`
- `account_get_profile_summary`

Implemented admin-only host-opt-in read-only tools:

- `system_get_host_runtime_summary`
- `web_search_public_web`
- `web_fetch_public_page_summary`

Remaining likely phase 2 read-only tools:

- `calendar_get_event_details`
- `network_get_topology_summary` once the RustyNet page exists

Possible future write tools, not enabled initially:

- `calendar_create_event`
- `calendar_update_event`
- `room_create`
- `room_invite_user`
- `server_action_start`
- `server_action_stop`

Never-allow tools:

- raw SQL execution
- arbitrary shell commands
- arbitrary filesystem reads
- arbitrary filesystem writes
- arbitrary HTTP requests
- direct browser automation against the public internet
- direct model-owned outbound network access
- credential export
- unrestricted log dumping
- direct admin settings mutation from the end-user AI page

## Security Model

Security is the most important design constraint after correctness.

### Rules

- all tools run server-side only
- all tools execute under the authenticated Rustyfin user context
- tools must reuse existing authorization checks where possible
- no tool gets raw unrestricted DB access
- no admin-only data is visible to normal users
- no write tools in phase 1
- no shell, filesystem, or network escape tools
- any future internet capability must be mediated by backend-owned web tools, never by giving the model raw socket or HTTP access

### Permission Control Model

Permissions must be enforced by Rustyfin, not by the model prompt.

The model may request a tool, but the backend is the source of truth.

Recommended enforcement layers:

1. registry allowlist
   - only registered tools can be called
2. role gate
   - `AuthUser` vs `AdminUser`
3. scope gate
   - per-domain access such as library IDs, room membership, event ownership, or server visibility
4. access-mode gate
   - read-only vs write vs destructive write
5. confirmation gate
   - whether the user must explicitly approve the action
6. audit gate
   - whether the call must be logged at normal or elevated detail

Recommended default:

- deny by default
- every tool must opt in explicitly
- every tool must declare its risk and access class
- every write must be blocked unless the registry says otherwise
- external web tools should also be disabled by default until the host explicitly enables them

### Read Versus Write Policy

Phase 1:

- only `ReadOnly` tools are enabled
- all read tools still enforce product permissions
- admin-only read tools should not be exposed on the normal `/ai` page unless there is a deliberate admin assistant mode later

Future write policy:

- the model can propose a write
- the backend must not execute it immediately
- the UI must show a human-readable confirmation card
- execution only happens after explicit user approval

Recommended write gate:

- mint a short-lived confirmation token
- bind it to:
  - user id
  - session id
  - tool name
  - normalized argument hash
  - expiry time
- reject execution if any of those do not match

For high-risk actions:

- require a stronger protected-action challenge
- this can reuse the product pattern already used for sensitive RustyVault actions where appropriate

### Resource Scoping

Each tool must resolve access against the same product rules the UI already follows.

Examples:

- calendar tools:
  - restrict personal events to the signed-in user
  - only expose shared/global events the user is allowed to see
- library tools:
  - resolve `allowed_library_ids` from the current user before querying items
- room tools:
  - only expose rooms the user can join or already belongs to
- server tools:
  - avoid exposing host internals, logs, or credentials by default
- host/runtime stats tools:
  - keep host capacity and live usage data admin-only
  - reuse the existing runtime diagnostics shaping layer rather than reading arbitrary host files from the assistant path
  - expose compact operational summaries such as total memory, used memory, logical CPU thread count, CPU usage, load, uptime, and Rustyfin runtime counters
- external web tools:
  - never expose local network targets
  - never carry Rustyfin cookies, bearer tokens, or host credentials to third-party sites
  - treat fetched content as untrusted data, not instructions
  - reshape fetched pages into compact text/metadata summaries before they reach the model

### Data Shaping Before Model Access

The model should only receive an assistant-safe projection of each domain object.

That means:

- remove secret fields
- remove internal IDs unless needed
- remove paths, tokens, and config internals
- summarize verbose data before it reaches the model
- redact any accidental sensitive substrings from free-text fields when possible

### Prompt Injection Handling

The assistant must treat tool outputs as data, not instructions.

Rules:

- never let tool output change the system prompt
- never allow content from media metadata, room names, chat text, or calendar text to redefine tool policy
- the orchestrator should separate:
  - system rules
  - conversation messages
  - tool results

### Data Minimization

Only send the minimum necessary context to the model.

Examples:

- for “what’s on my calendar this week,” do not include the user’s full calendar history
- for “what servers are online,” do not include logs or full event history
- for web lookups, do not include the full fetched HTML document unless a deliberately bounded extraction requires it

### Auditing

Grounded assistant tool usage should be auditable.

Recommended event shape:

- user id
- tool name
- timestamp
- success/failure
- compact request summary
- compact result summary

This can start in normal server logs, then move to a dedicated audit table if needed.

Recommended additional fields:

- trace id
- model name
- tool latency
- token or prompt budget summary
- whether the answer used cached data

### Rate Limiting And Abuse Control

The AI assistant should have its own limits in addition to normal API auth.

Recommended controls:

- per-user request rate limit
- per-session concurrent chat limit
- per-tool call rate limit for expensive tools
- output size limits
- refusal when the user attempts repeated disallowed actions
- separate outbound-web rate limits and host-level concurrency caps for any future web tools

### External Web Tool Policy

If Rustyfin adds internet-backed assistant tools, they must use the same backend tooling pattern as internal product tools.

Required rule:

- the model does not browse the internet directly
- the backend exposes narrow web tools
- the backend fetches and sanitizes the result
- the model only sees the bounded, sanitized result

Implemented initial external tool set:

- `web_search_public_web`
  - purpose: fetch a small set of public search results for a user query
  - input: `query`, optional `limit`
  - output: compact result cards with title, URL, snippet, and source host
- `web_fetch_public_page_summary`
  - purpose: fetch one public page and return a safe text summary plus metadata
  - input: `url`
  - output: final resolved URL, page title, source host, short extracted text, and fetch status

Required restrictions:

- backend-only execution
- `GET` and `HEAD` only
- no cookies, no auth headers, no delegated Rustyfin credentials
- block SSRF and private-address targets
- block localhost, loopback, RFC1918, link-local, multicast, `.local`, and metadata-service targets
- enforce DNS resolution checks before connect and on redirect
- follow only a small redirect budget
- strict response-size and time budgets
- HTML-to-text extraction before model exposure
- no binary download or arbitrary file retrieval through the assistant
- clear source attribution in the final answer/UI
- audit every external fetch with URL host, tool, user, trace id, and status

Current permission posture:

- admin-only or explicit host opt-in for the first release
- if later expanded to normal users, keep it behind a dedicated feature flag and separate rate limits

Recommended cache posture:

- short TTL cache for repeated identical public lookups
- cache by normalized query or final URL
- never cache authenticated or private content because those tools should not exist

### Retention And Privacy

Do not silently turn AI into a long-term surveillance log.

Recommended default:

- store only the minimum needed operational logs
- keep full conversation persistence off by default unless explicitly implemented
- if conversations are persisted later, define a retention window and delete path
- never use raw prompts or tool outputs as unrestricted analytics exhaust
- never expose one user’s prior assistant history to another user

## Authorization Strategy

The orchestration layer should receive `AuthUser` and build an assistant-scoped context from it.

Recommended context contents:

- `user_id`
- `role`
- allowed library ids
- any other capability flags needed for domain tools

Important rule:

- tools should not duplicate ad hoc auth logic if an existing domain helper already enforces access correctly

Recommended assistant context additions:

- accessible library ids
- role flags
- admin mode flag if ever introduced
- locale and timezone
- a request trace id
- a per-request capability set derived from the registry and current user

This context should be built once and reused across the whole orchestration turn.

## Reuse Strategy

The plan should reuse existing Rustyfin domain logic rather than inventing a parallel AI-only backend.

### Calendar

Current reusable surfaces:

- calendar service routes in `/Users/iwanteague/Desktop/Rustyfin/crates/calendar/src/main.rs`
- calendar client shape in `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/calendarApi.ts`
- repo access already lives under `rustfin_db::repo::calendar`

Recommended direction:

- do not have AI call the public calendar HTTP API as its primary architecture
- extract or centralize reusable read helpers so both the calendar HTTP layer and AI tools can call shared Rust functions

If the current code organization makes that too expensive for phase 1:

- a temporary internal adapter may call the existing route logic or repo layer directly
- but the target state should be shared Rust functions, not HTTP-to-self

### Rooms

Current reusable surfaces:

- public room listing route in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs`
- router in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/router.rs`
- client route usage in `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/watchPartyApi.ts`

Recommended direction:

- extract room-summary mapping from watch-party handlers into reusable Rust helpers
- AI tools should consume the same underlying room summaries the product already uses

### Libraries

Current reusable surfaces:

- server routes in `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs`
- repo logic in `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/libraries.rs`
- item access checks already exist in the server runtime

Recommended direction:

- reuse existing library access validation
- create compact assistant-facing summaries rather than exposing full item payloads

### Servers

Current reusable surfaces:

- server routes and handlers under `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/`
- client API in `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/serversApi.ts`
- repo logic in `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/servers.rs`

Recommended direction:

- phase 1 should expose status and summary tools only
- do not expose logs, destructive actions, or provisioning through AI tools initially

## Orchestration Loop

Recommended model loop:

1. receive authenticated chat request
2. build assistant context from `AuthUser`
3. build a short system prompt explaining:
   - what Rustyfin is
   - what tools exist
   - that tool outputs are authoritative
   - that the model should not invent unavailable data
4. send user message and conversation history to the model
5. if the model requests a tool:
   - validate tool name
   - validate tool args
   - execute tool
   - capture structured result
   - append tool result to the conversation
6. continue until final answer
7. stream answer tokens to the UI

Recommended streamed event types:

- `assistant_status`
- `tool_started`
- `tool_finished`
- `tool_failed`
- `assistant_answer`
- `assistant_done`

These events should expose user-friendly status, not hidden reasoning or chain-of-thought.

### Execution Guardrails

- max tool calls per message, recommended: `3`
- per-tool timeout, recommended: `2-5s`
- total orchestration timeout, recommended: `15-30s`
- max serialized tool result size, recommended: small KB budget per tool
- deduplicate repeated identical tool calls within one turn
- allow safe parallel execution for independent read-only tools
- require clarification instead of guessing when a query is ambiguous

### Clarification Strategy

The assistant should ask a follow-up question when the user request is under-specified in a way that changes the answer materially.

Examples:

- “What’s on my calendar?” when the time window is unclear
- “Is the server online?” when multiple servers exist
- “Can you create an event?” without date/time details

For safe read questions, the assistant may choose a sensible default and state it explicitly.

For writes or admin-sensitive questions, it should prefer asking first.

## Recommended Initial Tool Set

Phase 1 should be small and useful.

### 1. `calendar_list_events`

Purpose:

- answer near-term calendar questions

Input:

- `from`
- `to`
- `scope`: `global | personal | all`
- `limit`

Output:

- compact array of events with title, date, scope, owner hint when appropriate

### 2. `calendar_upcoming_birthdays`

Purpose:

- answer birthday reminders quickly without broad event dumps

Input:

- `days_ahead`
- `limit`

Output:

- user or contact name
- birthday date
- days until birthday

### 3. `rooms_list_active`

Purpose:

- answer “what rooms are active” and “what can I join”

Input:

- optional `room_mode`
- optional `limit`

Output:

- room name
- room mode
- host username
- joined member count
- whether joinable

### 4. `libraries_list_accessible`

Purpose:

- answer “what libraries do I have”

Input:

- none

Output:

- library id
- library name
- library kind
- item count if cheap enough

### 5. `library_search_titles`

Purpose:

- answer “do I have X in my library”

Input:

- `query`
- optional `kind`
- optional `limit`

Output:

- compact list of matched items with title, year, kind, library name

### 6. `servers_list_minecraft_status`

Purpose:

- answer “what servers are online”

Input:

- optional `limit`

Output:

- server name
- status
- listen host/port if appropriate
- player or provisioning summary if available

### 7. `account_get_profile_summary`

Purpose:

- answer “who am I in Rustyfin” and provide grounding for permission-based answers

Input:

- none

Output:

- username
- role
- accessible library count

### 8. `system_get_host_runtime_summary`

Purpose:

- answer admin questions about current Rustyfin host health and capacity

Input:

- none in the first version

Output:

- host uptime
- total memory
- used memory
- used-memory percent
- logical CPU thread count
- optional physical core count when available
- current CPU usage percent
- estimated busy logical threads
- load average when available
- compact Rustyfin runtime counters for jobs, assistant calls, websockets, and transcoding

Security:

- `AdminOnly`
- read-only
- no logs, tokens, filesystem paths, secrets, or arbitrary process lists

## What Not To Ship In Phase 1

Do not ship these first:

- `calendar_create_event`
- `room_create`
- `room_end`
- `server_action_start`
- `server_action_stop`
- `vault` tools
- admin-only settings or path-management tools

Those can come later once read-only grounding is stable and audited.

## UI Impact

The `/ai` page should remain mostly unchanged.

Needed UI additions:

- better assistant copy that reflects grounded abilities truthfully
- clear unavailable message when a requested tool or backend domain is unavailable
- optional “sources used” or “fetched from calendar/rooms/servers” hint in the response metadata
- lightweight progress messages such as “Checking your calendar”
- explicit confirmation cards for any future write action
- visible refusal text when a requested action is disallowed by policy

Do not turn the `/ai` page into an admin dashboard.

Recommended UX rules:

- never pretend data was fetched if no tool was called
- tell the user which product areas were consulted
- do not expose raw tool payloads unless a debug/admin mode is deliberately built later
- do not stream hidden reasoning

## API Changes

The cleanest shape is to keep the public AI API surface small.

Preferred route strategy:

- keep `GET /api/v1/ai/models`
- keep `POST /api/v1/ai/chat`

Grounding should happen behind `/api/v1/ai/chat`, not through new client-visible tool routes unless debugging/admin introspection is explicitly needed.

Optional internal-only or admin-only additions later:

- tool usage diagnostics
- conversation trace inspector
- per-tool latency metrics
- a debug endpoint for tool schemas in admin mode only

## Data And Domain Extraction Work

Some domain logic is still trapped in route handlers or binary entrypoints.

Expected extraction work:

- move reusable calendar read logic out of HTTP-only handler flow
- extract room summary builders from watch-party handlers
- centralize assistant-safe library search and summary helpers
- expose server-status summary helpers that do not leak operational details

This extraction work is worth doing because it improves reuse beyond AI.

## Performance Strategy

Read tools should be cheap.

Recommended rules:

- prefer direct repo queries or shared domain functions over internal HTTP round-trips
- limit result sizes aggressively
- summarize large datasets before they reach the model
- add short-lived caching only where the data is naturally cacheable

Good cache candidates:

- library list
- room list for a few seconds
- server status summary for a few seconds
- account summary for a short period within one session

Poor cache candidates:

- user-specific rapidly changing personal event views if correctness would suffer

Additional performance guidance:

- prefer one compact query over many small follow-up queries
- pre-shape result objects before they hit the model
- cap expensive searches aggressively
- avoid loading full logs or oversized descriptions into context
- make parallel reads opt-in per tool, not automatic for all tools

## Failure Behavior

The assistant must degrade cleanly.

If a tool fails:

- do not crash the chat stream
- return a structured tool failure to the orchestrator
- let the assistant answer with something like:
  - “I couldn’t read your calendar right now”
  - not a fabricated answer

If AI is unavailable:

- preserve current `503` behavior and UI fallback

If one domain is unavailable:

- the assistant should still be able to answer questions using other tools

If a write is proposed but not permitted:

- the assistant should explain that the action is not enabled
- it should not imply the action has been performed
- it may describe the manual UI path instead

## Testing Strategy

### Unit tests

- tool input validation
- output normalization
- access filtering
- prompt/tool orchestration edge cases

### Integration tests

- authenticated user can query accessible data
- normal user cannot see admin-only or inaccessible data
- assistant returns grounded answer when tool succeeds
- assistant returns truthful failure message when tool fails

### Security tests

- prompt injection attempts through tool result text
- overbroad library access leaks
- cross-user data leakage
- malformed tool arguments
- confirmation token replay attempts
- write-tool execution without matching consent token
- admin-only tool access from non-admin sessions
- cached result leakage across users

## Rollout Plan

### Phase 0

- done: document architecture
- done: freeze the security model
- done: define the tool registry shape

### Phase 1

- done: implement backend tool registry
- done: implement initial read-only calendar, rooms, libraries, servers, and account tools
- done: wire grounded orchestration into `/api/v1/ai/chat`
- done: add streamed tool-status metadata to improve UX

### Phase 2

- done: improve UI copy and assistant metadata
- done: add source attribution chips
- done: add clarification handling improvements
- done: add tool usage logging and runtime metrics
- remaining:
  - tighten prompts and response style further
  - expand permission-bound integration coverage across the remaining grounded domains
  - expand grounded domains such as Network

### Phase 3

- future:
  - evaluate carefully scoped write tools
  - only after read-only grounding is reliable
  - implement confirmation-token workflow
  - add protected-action support for high-risk writes

## Recommended First Implementation Slice

The highest-value first slice is:

1. add `ai_tools` backend module structure
2. implement assistant context + tool registry
3. ship:
   - `calendar_list_events`
   - `rooms_list_active`
   - `libraries_list_accessible`
4. update `/ai` copy so it reflects actual grounded behavior
5. add integration tests for permission boundaries

That will give Rustyfin AI its first genuinely useful and trustworthy product behavior without opening unsafe surfaces.

## Final Recommendation

Rustyfin should adopt a server-side grounded tools architecture.

Not because it is fashionable, but because it is the only approach here that is:

- accurate enough
- secure enough
- modular enough
- maintainable enough

The assistant should remain a chat surface, while the backend becomes the trusted brain that decides what Rustyfin data can be fetched and how it is summarized for the model.
