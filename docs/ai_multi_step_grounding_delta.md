# Rustyfin AI Multi-Step Grounding Delta

## 1. Title and Executive Summary

Rustyfin’s current AI assistant already has several strong foundations: it is local-first, deterministic-first, ACL-aware, confirmation-gated for writes, compact in how it injects evidence, and increasingly structured in how it plans tool usage. The current codebase already contains a schema-aware planner, planner repair, provider-based tool dispatch, scheduler-based runtime admission control, benchmark-aware routing primitives, memory retrieval, grounding compression, and deterministic reply paths for a number of domains. The next upgrade should not replace those pieces. It should add a bounded execution layer above them.

The core problem is that the current assistant is still too brittle once the first planned tool set is wrong, incomplete, weakly grounded, or only partially useful. In practice, the system still behaves like a mostly single-pass flow: propose tools, execute them, assemble grounding, answer. That works for happy-path questions, but it degrades when the model picks the wrong tool, supplies incomplete arguments, gets an empty result, hits an ambiguous result, or needs to combine evidence from multiple sources before answering. The target end-state is a bounded, deterministic-first, multi-step grounded execution system that can recover from those cases without turning Rustyfin into an unbounded autonomous agent.

Plain-language target state:

- `Instant` stays narrow, fast, and conservative.
- `Thinking` becomes the default “smart recovery” mode with limited retries, alternate tool selection, and bounded evidence synthesis.
- `Extended` becomes the deepest read-only investigation path with stricter observability, richer synthesis, and hard stop budgets.
- The assistant remains local-first, compact, ACL-safe, and confirmation-gated for writes.
- The upgrade is implemented as Rust-first server/runtime changes that fit the current Rustyfin architecture.

---

## 2. Current-State Architecture

### 2.1 High-level request flow today

On current `main`, Rustyfin’s `/ai` path already runs through a fairly structured stack:

1. Request enters the AI server route in `crates/server/src/ai_enabled.rs`.
2. Conversation metadata, response mode, role routing, and scheduler decisions are resolved.
3. The planner path in `crates/server/src/ai_assistant/orchestrator.rs` chooses grounded read-only tools.
4. Tools execute through `crates/server/src/ai_assistant/tools.rs` using the provider/registry abstraction in `crates/server/src/ai_assistant/provider.rs` and `crates/server/src/ai_assistant/providers/`.
5. Tool outputs and memory retrieval are compacted into grounding chunks in `crates/server/src/ai_assistant/memory.rs`.
6. Deterministic reply helpers in `crates/server/src/ai_assistant/replies.rs` may answer directly for supported domains.
7. Otherwise, the answer backend generates a grounded answer using compact evidence and current conversation context.
8. Runtime, benchmark, and scheduler data are exposed through admin/runtime surfaces including `crates/server/src/ai_runtime.rs`.

### 2.2 Repo-grounded file/function map

#### Top-level guidance
- `README.md`
- `AGENTS.md`
- `CLAUDE.md`

These files establish the assistant’s intended boundaries: server-side grounding, compact prompts, read-only tools by default, confirmation for write actions, ACL preservation, local-first runtime, benchmark-aware routing, and visible runtime activity.

#### Main AI request/runtime surface
- `crates/server/src/ai_enabled.rs`
  - primary streaming route and SSE orchestration
  - request parsing, role routing, scheduler acquisition, streaming status events, persistence, confirmation-token handling

#### Planner and orchestration
- `crates/server/src/ai_assistant/orchestrator.rs`
  - `build_model_planner_messages(...)`
  - `resolve_model_plan_with_repair(...)`
  - `run_planner_repair(...)`
  - `validate_planner_ast(...)`
  - `plan_tool_calls_with_model_assist(...)`
  - `prepare_assistant_turn(...)`

#### Tool execution
- `crates/server/src/ai_assistant/tools.rs`
  - `execute_tool(...)`
  - `execute_tool_with_profile(...)`
  - current wrapping of tool success/error into `AssistantToolContextBlock`

#### Tool registry and providers
- `crates/server/src/ai_assistant/provider.rs`
  - `ToolProvider`
  - `ToolRegistry`
  - `ToolRegistryBuilder`
  - `default_tool_registry()`
  - `ToolExecutionProfile`
- `crates/server/src/ai_assistant/providers/`
  - account/calendar/channels/documents/downloads/libraries/network/rooms/servers/system/weather/web providers

#### Replies and grounding
- `crates/server/src/ai_assistant/replies.rs`
  - compact grounding prompt construction
  - deterministic domain-specific replies
  - evidence ranking/compression helpers
- `crates/server/src/ai_assistant/memory.rs`
  - `build_grounding_chunks_for_turn(...)`
  - `persist_grounding_artifacts(...)`
  - `search_memory_chunks(...)`
  - ACL-aware retrieval and chunk ranking/compression

#### Types and runtime state
- `crates/server/src/ai_assistant/types.rs`
  - `AssistantResponseMode`
  - `AssistantPlannerMode`
  - `AssistantPlannerDebug`
  - `PlannerExecutionStats`
  - `AssistantTurnStats`
  - `AssistantToolContextBlock`
- `crates/server/src/ai_assistant/scheduler.rs`
  - `TurnScheduler`
  - overload handling, queueing, warm model policy, remote planner backend selection
- `crates/server/src/ai_runtime.rs`
  - admin/runtime response assembly
- `crates/server/src/ai_model_routing.rs`
  - `ModelRole`
  - `resolve_role_routing_plan(...)`
  - benchmark-aware and remote-aware role selection

#### Local backend/runtime crate
- `crates/ai-agent/src/engine.rs`
- `crates/ai-agent/src/backend.rs`
- `crates/ai-agent/src/backend/role_router.rs`

These already support local inference, structured output, streaming, and role-bound backends.

### 2.3 Current request-flow sketch

```text
User prompt
  -> ai_enabled.rs request/stream setup
  -> resolve role routing + scheduler decision
  -> orchestrator.rs planner prompt
  -> planner AST parse/validate/repair
  -> execute selected read-only tools through ToolRegistry
  -> build grounding chunks from tool outputs + memory
  -> deterministic reply if supported
     else grounded answer generation
  -> persist turn artifacts, debug, stats, activity
  -> stream final answer
```

### 2.4 What is already stronger than expected

The current repo is stronger than a plain “single JSON planner and one tool call” design in several important ways:

- Planner schema hardening is already partly present through `PlannerAst`, `validate_planner_ast(...)`, `resolve_model_plan_with_repair(...)`, and `run_planner_repair(...)`.
- The tool layer is already provider-based; a `ToolProvider` abstraction and `ToolRegistry` exist.
- Role-based model routing primitives already exist in `ai_model_routing.rs`.
- The scheduler already supports overload-aware behavior, warm model reuse, and planner backend selection.
- Deterministic reply and compact grounding patterns already exist and should remain central.

### 2.5 Where brittleness still exists

The missing layer is not “add more tools” and not “replace the planner.” The missing layer is a generic bounded executor that sits above the planner and tool registry.

Concrete brittleness points:

1. **Execution is still effectively one-shot.**
   The existing flow still assumes a first planned tool set is usually sufficient. `prepare_assistant_turn(...)` in `orchestrator.rs` is a clear example of this shape: plan, execute tool list, join results, build prompt, answer.

2. **Tool result semantics are too coarse.**
   `AssistantToolContextBlock` carries a `status` string plus arbitrary JSON `data`. This is not rich enough to drive generic recovery logic. “ok” vs “error” is insufficient for: empty result, ambiguous result, partial result, stale result, weak match, conflicting result, validation failure, or clarification-needed.

3. **Modes are not yet execution-budget contracts.**
   `Instant`, `Thinking`, and `Extended` exist, but they are not yet enforced as distinct multi-step runtime budgets with explicit planner-pass/tool-step/fallback/synthesis limits.

4. **Role routing exists, but the main request path is still answer-centric.**
   Role selection primitives exist, but the server route visibly prioritizes the answer role and does not yet fully exploit distinct planner/summarizer/verifier/worker backends during execution.

5. **There is no generic fallback graph.**
   Domain-specific deterministic replies exist, but there is no shared framework for: wrong tool, wrong arguments, empty results, ambiguous results, partial results, weak matches, or conflicts.

6. **Observability is good, but not execution-loop aware.**
   Runtime and benchmark/admin surfaces exist, but they do not yet expose attempt-by-attempt recovery traces, stop reasons, outcome histograms, or domain-family fallback counters.

---

## 3. Current Failure Modes

The following are the main failure classes that the new design must handle. These are repo-grounded problems, not hypothetical framework issues.

### 3.1 Wrong tool selected

Current pattern:
- Planner selects a tool set.
- Tools run.
- The system proceeds to answer assembly.

Problem:
- If the first tool is plausible but wrong, the system currently has no generic framework to inspect the result and pivot to a more suitable tool in a bounded way.

Why current code is vulnerable:
- Planner validation checks tool legality and shape, not whether the selected tool produced the right kind of evidence.
- Tool results are only `ok/error`, so the executor cannot cleanly distinguish “valid call, wrong domain fit” from “hard failure.”

### 3.2 Right tool, wrong arguments

Current pattern:
- Planner validation already normalizes some arguments, especially around location/weather.

Problem:
- Many argument mistakes are semantically recoverable rather than fatal.
- Example classes: missing entity ID, wrong detail level, too-broad search term, missing date window, missing location, or incomplete person disambiguation.

Why current code is vulnerable:
- There is no generic `ValidationFailed` or `ClarificationNeeded` outcome type driving a next step.

### 3.3 Empty result / no-result

Problem:
- A tool can succeed operationally yet produce no meaningful data.
- Today that may still appear as `status = "ok"` with empty payload.

What should happen instead:
- The executor must distinguish “transport success” from “semantic emptiness.”
- Empty result should be eligible for alternate-tool or broader-search fallback in `Thinking`/`Extended`, but usually not in `Instant`.

### 3.4 Ambiguous result requiring clarification

Problem:
- A tool can return multiple plausible matches.
- Current reply code may attempt to wrap those results directly or ask an ad hoc clarification, but there is no reusable ambiguity contract.

Needed behavior:
- `Ambiguous` and `ClarificationNeeded` outcomes must be first-class.
- Clarification must be targeted, minimal, and mode-aware.

### 3.5 Partial result that should trigger recovery

Problem:
- One tool may return a partial answer that is not enough for the user’s full question.
- Example: a list tool finds an item, but details are still needed; transcript search finds the right segment, but excerpt retrieval still needs a follow-up step.

Needed behavior:
- `Partial` must trigger bounded enrichment steps rather than immediate answer generation.

### 3.6 Contradictory results from multiple sources

Problem:
- Multi-source grounding can produce conflicts.
- The current system can compress evidence, but it does not yet expose a reusable conflict-handling model.

Needed behavior:
- `Conflicting` must be a typed outcome.
- `Extended` should be allowed to reconcile or clearly state irreconcilable conflict.

### 3.7 Hallucinated wrap-up from weak grounding

Problem:
- Even compact grounded prompts can be too weak if the underlying tool result is empty, partial, stale, or ambiguous.

Needed behavior:
- Final answer synthesis should be gated by typed evidence quality.
- Weak evidence should produce clarification, a bounded retry, or a constrained “not enough evidence” response.

### 3.8 Prompt bloat from raw tool injection

Problem:
- Recovery loops can easily bloat prompts if every raw result is appended verbatim.

Current mitigation:
- Rustyfin already has compact grounding chunk generation and ranking.

Needed behavior:
- The new executor must preserve this discipline by summarizing intermediate results into compact, typed evidence entries and strict union budgets.

### 3.9 ACL leakage risk from broad fallback behavior

Problem:
- Generic retry logic can accidentally widen scope if it ignores tool policy or access control.

Needed behavior:
- Recovery must remain within the same or narrower ACL envelope unless the current user explicitly broadens scope through an allowed tool path.
- Fallback graphs must encode allowed alternates by domain and profile.

### 3.10 Write-action safety risk

Problem:
- Generalized retries can become dangerous if they are ever allowed to retry protected or write-capable tools.

Needed behavior:
- The multi-step executor must be read-only by default.
- Confirmation-gated writes remain outside the retry graph unless a future explicit write subframework is designed separately.

---

## 4. External Research

This section extracts patterns from projects that are actually relevant to Rustyfin’s architecture and deployment model.

### 4.1 Ollama

- Repo / docs:
  - `https://github.com/ollama/ollama`
  - `https://docs.ollama.com/api`
  - `https://docs.ollama.com/capabilities/structured-outputs`

#### What is relevant
- Structured outputs with a JSON schema.
- Tool support in the chat API.
- Local model serving orientation.

#### Pattern worth considering
- Keep planner/grader/verifier outputs schema-constrained.
- Treat structured output as a first-class contract, not a prompt wish.

#### What not to copy
- Do not redesign Rustyfin around an external model server dependency.
- Rustyfin already has a local backend path through its own runtime crate; keep that as the default path.

### 4.2 llama.cpp

- Repo / docs:
  - `https://github.com/ggml-org/llama.cpp`
  - `https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md`

#### What is relevant
- Grammar-constrained output, including valid JSON via GBNF.
- Server-side support for structured outputs and function calling.
- Emphasis on low-latency local inference.

#### Pattern worth considering
- Use output constraints for planner/repair/verifier roles where current backend support allows.
- Keep schemas small and mechanically validated.

#### What not to copy
- Do not expose generic function calling or a public open model server surface to end users.
- Do not assume every local model can follow large complex schemas reliably.

### 4.3 LocalAI

- Repo / docs:
  - `https://github.com/mudler/LocalAI`
  - `https://localai.io/features/openai-functions/`

#### What is relevant
- OpenAI-compatible tool/function interfaces over local backends.
- Grammar-backed function selection.
- Optional parallel tool-call support.

#### Pattern worth considering
- Limited parallel tool fan-out can be useful in higher modes, but only for explicitly safe read-only domains.
- Function-selection grammar is useful inspiration for keeping planner outputs tight.

#### What not to copy
- Do not enable broad parallel tool use across all domains.
- Do not let parallelism override deterministic or ACL-safe behavior.

### 4.4 Continue

- Repo / docs:
  - `https://github.com/continuedev/continue`
  - `https://docs.continue.dev/customize/deep-dives/agent`
  - `https://docs.continue.dev/customize/deep-dives/context-providers`
  - `https://docs.continue.dev/customize/deep-dives/docs`

#### What is relevant
- Context-provider model for selectively injecting external context.
- Retrieval pipelines that retrieve wide, rerank, then trim to final prompt budget.
- Hybrid local-first workflows.

#### Pattern worth considering
- Rustyfin should stage evidence exactly the same way in spirit: retrieve candidate evidence, rerank, synthesize compact final evidence, enforce injection budget.
- Do not treat every tool result as prompt-ready; treat it as candidate evidence.

#### What not to copy
- Do not import IDE-centric code-editing concerns into Rustyfin’s user-facing assistant.
- Do not widen Rustyfin’s tool surface to generic repo/file/system manipulation.

### 4.5 Open WebUI

- Repo / docs:
  - `https://github.com/open-webui/open-webui`
  - `https://docs.openwebui.com/features/plugin/functions/`

#### What is relevant
- Composition functions that combine multiple model/API calls.
- A clear warning that server-side function execution is highly privileged.

#### Pattern worth considering
- Internal composition nodes can be valuable, especially as deterministic meta-tools or synthesis steps.

#### What not to copy
- Do not expose arbitrary server-side scripting or plugin execution.
- Rustyfin should keep tool behavior in reviewed Rust code and provider modules.

### 4.6 AnythingLLM

- Repo / docs:
  - `https://github.com/Mintplex-Labs/anything-llm`
  - `https://docs.anythingllm.com/agents`
  - `https://docs.anythingllm.com/agent/custom/introduction`

#### What is relevant
- Selective skill enabling.
- Workspace/provider-specific agent behavior.
- Avoiding prompt bloat by not exposing every tool on every turn.

#### Pattern worth considering
- Rustyfin’s planner/executor should advertise only the domain-relevant shortlist of tools for the current turn.
- Mode-specific tool visibility should be explicit and budgeted.

#### What not to copy
- Do not introduce arbitrary custom code skills or OS-level command execution.

### 4.7 OpenHands

- Repo / docs:
  - `https://github.com/OpenHands/OpenHands`
  - `https://docs.openhands.dev/sdk/arch/agent`
  - `https://docs.openhands.dev/openhands/usage/developers/evaluation-harness`

#### What is relevant
- Explicit step loop architecture.
- Event/state-based execution.
- Hard iteration budgeting.
- Dedicated evaluation harness concepts.

#### Pattern worth considering
- Rustyfin should adopt the bounded-loop idea, not the full coding-agent model.
- The key reusable patterns are: per-step execution, global step caps, explicit stop reasons, and a benchmark/eval harness for regressions.

#### What not to copy
- Do not import coding-agent capabilities, shell access, file edit tools, or autonomous task delegation.
- Rustyfin’s assistant domains are narrower and should remain narrower.

### 4.8 Claw Code

- Repo / docs:
  - `https://github.com/ultraworkers/claw-code`
  - docs under the repo describing runtime, tools, and query engine behavior

#### What is relevant
- Rust workspace decomposition into runtime, tools, commands, plugins, and compatibility harnesses.
- Tool specs with JSON schemas and permission checks.
- Query engine budgets for turns/tokens/retries.

#### Pattern worth considering
- A dedicated Rust `query/execution engine` layer with explicit turn and token budgets is directly relevant to Rustyfin.
- The best reusable idea is not “make Rustyfin a coding agent.” It is “make execution limits explicit, typed, and observable.”

#### What not to copy
- Do not bring over coding-agent tool categories, filesystem editing, shell, MCP sprawl, or plugin complexity to the end-user assistant.

### 4.9 Distilled reusable patterns

These are the external patterns that fit Rustyfin best:

1. **Structured outputs are non-negotiable for planner/repair/verifier stages.**
2. **Execution must be step-based and hard-capped.**
3. **Tool results need semantic grading, not just transport success/failure.**
4. **Recovery should follow bounded fallback graphs, not blind retries.**
5. **Evidence should be retrieved wide, reranked, and injected narrow.**
6. **Observability must expose attempts, stop reasons, and budget usage.**
7. **A dedicated evaluation harness is required once multi-step recovery exists.**
8. **Do not broaden Rustyfin into a generic autonomous agent platform.**

---

## 5. Target Architecture

### 5.1 Design summary

Add a new bounded execution layer above the existing planner and below final answer generation. This layer should be responsible for:

- executing one read-only step at a time,
- inspecting each result using a typed quality taxonomy,
- selecting a bounded next step using domain-specific fallback graphs,
- optionally synthesizing evidence across multiple steps,
- stopping for explicit reasons,
- preserving deterministic-first behavior and ACL safety.

### 5.2 Proposed new runtime layer

Introduce a new module family under `crates/server/src/ai_assistant/`:

- `executor.rs`
- `outcomes.rs`
- `recovery.rs`
- `synthesis.rs`
- `budgets.rs` (optional; could live in `types.rs` if preferred)

Primary runtime concept:

- `GroundedExecutor` or `AssistantGroundedExecutor`

Primary responsibilities:

1. Accept the user request, mode, planner AST, tool registry, tool context, and routing state.
2. Build a bounded execution plan.
3. Execute at most one step at a time.
4. Normalize raw tool results into typed outcomes.
5. Consult recovery graph rules.
6. Persist attempt traces and stop reasons.
7. Emit compact evidence packets.
8. Hand final evidence to deterministic reply or grounded answer synthesis.

### 5.3 Required execution characteristics

The target executor must be:

- **bounded**: every mode has hard caps,
- **deterministic-first**: deterministic rules and replies get first right of refusal,
- **read-only-by-default**: no write retries,
- **ACL-preserving**: same or narrower access scope on retries,
- **evidence-driven**: no answer without typed evidence or explicit insufficiency,
- **observable**: every stop reason and attempt must be visible to admin/runtime surfaces,
- **compact**: evidence accumulation must stay token-budget aware.

### 5.4 Result-quality taxonomy

The new executor should operate on typed results, not raw `ok/error` alone.

Core idea:

- transport success is not answer success
- transport failure is not the only failure mode
- semantic quality must drive the next step

### 5.5 Recovery/fallback model

Recovery should not be generic “retry with another tool.” It must use registered domain-family graphs.

Each domain family defines:

- primary tools,
- alternate tools,
- enrichment tools,
- clarification triggers,
- stop conditions,
- disallowed transitions.

### 5.6 Synthesis model

Multiple tool results should not be dumped into the final prompt verbatim.

Instead:
- each step yields one or more `EvidenceItem`s,
- evidence is normalized and deduplicated,
- weak or stale evidence is marked as such,
- only a final compact evidence bundle is injected into answer generation.

### 5.7 Mode-specific behavior

- `Instant`: one-step answer path, minimal recovery, early stop.
- `Thinking`: bounded recovery with one or two alternates and compact synthesis.
- `Extended`: widest bounded read-only search with verifier/summarizer support and stricter traceability.

### 5.8 Stop conditions

The executor must stop when any of the following occurs:

- direct deterministic answer available,
- sufficient grounded answer available,
- clarification required,
- budgets exhausted,
- ACL or confirmation barrier reached,
- repeated outcome signature detected,
- only weak/conflicting evidence remains,
- no permitted fallback edge exists.

### 5.9 Observability expectations

Each turn should record:

- selected mode,
- planner passes used,
- tool attempts,
- normalized outcomes,
- chosen fallback edges,
- evidence items retained/dropped,
- final stop reason,
- final answer provenance.

### 5.10 Safety constraints

The new executor must not:

- expose chain-of-thought,
- expose shell or arbitrary filesystem tools,
- retry write operations automatically,
- widen ACL scope during fallback,
- keep searching indefinitely,
- inject full raw tool payloads into prompts by default.

---

## 6. Mode Matrix

| Mode | Max planner passes | Max tool steps | Alternate tools | Parallel fan-out | Cross-tool synthesis | Clarification behavior | Prompt budget | Rough latency target | Stop examples |
|---|---:|---:|---|---|---|---|---|---|---|
| `Instant` | 1 planner pass, no replan loop beyond existing AST repair | 1 | No | No | Minimal; only deterministic direct merge | Ask only when a single missing field blocks a high-confidence answer and entity graph cannot fill it | Smallest | ~1.5s to 3s local target | stop after first high-quality answer; stop on empty/ambiguous rather than exploring alternates |
| `Thinking` | up to 2 planner passes total | 3 | Yes, same domain family only | Only for cheap read-only sibling lookups, max 2 concurrent | Yes, bounded compact merge | One targeted clarification allowed when better than blind fallback | Medium | ~3s to 8s local target | continue on `Empty`, `Partial`, `WeakMatch`; stop on conflict after one reconciliation attempt |
| `Extended` | up to 3 planner passes total | 5 | Yes | Yes, but only on allowlisted domain families and only read-only | Yes, plus verifier/summarizer stage | One targeted clarification or one explicit “I need X to continue” stop | Largest, but still hard-capped | ~6s to 15s hard ceiling | continue through partial chains; stop on repeated signature, exhausted graph, or unresolved conflict |

### 6.1 Budget rules

These budgets must be enforced in code, not just described in prompts.

Recommended explicit budget struct:

- `max_planner_passes`
- `max_tool_steps`
- `max_alternate_steps`
- `max_parallel_tools`
- `max_evidence_items`
- `max_grounding_chars`
- `max_recovery_depth`
- `max_same_signature_repeats`
- `allow_verifier`
- `allow_parallel_read_fanout`

### 6.2 Mode stop heuristics

#### `Instant`
Stop when:
- direct deterministic answer is available,
- first tool result is empty or ambiguous,
- required identifier/location/date is missing,
- first tool result is good enough.

Do not continue into alternate search unless the fallback is deterministic, cheap, and same-domain.

#### `Thinking`
Continue when:
- first result is `Partial`, `Empty`, `WeakMatch`, or `ValidationFailed` with a known corrective edge,
- two tools are needed to answer the same domain question,
- a list step needs a detail step.

Stop when:
- a second fallback would be speculative,
- ambiguity is user-facing and cannot be resolved from entity graph/history,
- conflict remains after one reconciliation step.

#### `Extended`
Continue when:
- answer quality is still low but graph-permitted alternates remain,
- evidence from multiple read-only tools can still improve confidence,
- verifier can resolve a conflict or stale result cheaply.

Stop when:
- budgets are near exhaustion,
- repeated evidence signatures indicate loop risk,
- further steps would broaden scope without clear gain.

---

## 7. Result Quality Model

Replace the implicit `ok/error` mental model with a reusable outcome taxonomy.

### 7.1 Proposed enum

Add a new enum in `crates/server/src/ai_assistant/types.rs` or `outcomes.rs`:

- `Answer`
- `Partial`
- `Empty`
- `Ambiguous`
- `ClarificationNeeded`
- `NotFound`
- `WeakMatch`
- `ValidationFailed`
- `Stale`
- `Conflicting`
- `Denied`
- `TransientError`
- `FatalError`

### 7.2 Outcome definitions and executor behavior

| Outcome | Meaning | Executor response | Retry/fallback allowed | Show directly to user | Can final reply synthesize around it |
|---|---|---|---|---|---|
| `Answer` | Tool produced sufficient answer-grade evidence | stop or enrich only if user explicitly asked for more depth | Usually no | Yes | Yes |
| `Partial` | Useful but incomplete evidence | follow enrichment edge | Yes | Not alone unless framed as partial | Yes |
| `Empty` | Valid tool call, no meaningful result | try broader/sibling edge if allowed | Yes | Usually no | Only as part of a “nothing found” response |
| `Ambiguous` | Multiple plausible matches | clarify or disambiguate via secondary lookup | Yes | Yes, if asking concise clarification | No final answer without resolution |
| `ClarificationNeeded` | missing required user input | stop and ask targeted question | No blind retry | Yes | No |
| `NotFound` | domain-specific absence confidently established | stop or try one alternate source if graph says so | Sometimes | Yes | Yes |
| `WeakMatch` | fuzzy/low-confidence result | try stronger search/detail tool | Yes | Usually no | Only with explicit uncertainty |
| `ValidationFailed` | arguments invalid/incomplete after normalization | repair args or ask clarification | Yes | Only as concise clarification | No |
| `Stale` | result may be outdated | try fresher source or runtime view | Yes | Only with freshness caveat | Yes, with caveat |
| `Conflicting` | evidence sources disagree | verifier/reconcile or surface conflict | Yes, bounded | Yes, if unresolved | Yes, only if conflict is explicit |
| `Denied` | ACL/confirmation/tool policy barrier | stop | No | Yes | No |
| `TransientError` | temporary operational failure | one retry or alternate tool if graph allows | Yes, bounded | Usually no | Rarely |
| `FatalError` | hard failure or unsafe path | stop | No | Yes, minimal | No |

### 7.3 Normalization rules

Every tool execution should produce two layers:

1. **Transport/result block**
   - current raw provider payload
2. **Normalized semantic outcome**
   - typed outcome enum
   - `confidence`
   - `domain_family`
   - `evidence_items`
   - `recovery_hints`
   - `ambiguity_keys`
   - `freshness`

### 7.4 Outcome classification source

Outcome classification should come from deterministic inspectors first:

- payload shape checks,
- empty-list checks,
- required-field checks,
- ambiguity-count checks,
- stale timestamp rules,
- conflict detection across evidence IDs/values,
- domain-specific post-processors.

Use a model-based verifier only in `Extended` when deterministic classification cannot safely resolve conflict quality.

---

## 8. Recovery and Fallback Graphs

### 8.1 Core framework

Introduce a domain-family fallback registry.

Each domain family defines a bounded graph of recovery edges. Every edge specifies:

- `from_tool` or `from_family_stage`
- `trigger_outcome`
- `next_tool`
- `arg_transform`
- `mode_min`
- `cost_class`
- `requires_disambiguation`
- `max_uses`
- `stop_if_same_signature`

### 8.2 Domain-family registration

Add explicit domain metadata to tool specs or a parallel registry:

- `Calendar`
- `Weather`
- `AiRuntime`
- `Library`
- `Transcript`
- `Downloads`
- `Rooms`
- `Network`
- `Servers`
- `Documents`
- `Channels`
- `System`

The registry should be code-defined, reviewed, and versioned inside the repo.

### 8.3 How executor chooses next step

The executor should choose the next step in this order:

1. Check stop conditions.
2. Classify outcome.
3. Load fallback edges for the current domain family and current stage.
4. Filter edges by:
   - mode budget,
   - ACL/profile,
   - current confirmation state,
   - already-used signature,
   - duplicate tool/arg hash,
   - cost budget,
   - ambiguity requirements.
5. Pick the highest-priority deterministic edge.
6. If none exists, either stop or ask targeted clarification.

### 8.4 Preventing blind retries

The executor must reject a candidate next step if:

- the same tool + normalized args hash already ran,
- the last normalized outcome kind is unchanged and no arguments changed,
- the graph edge requires a missing ID/location/date that is still missing,
- the retry would widen scope without explicit graph approval,
- the retry would exceed budget,
- the retry would switch into a write/protected tool.

### 8.5 Preventing loops

Loop-prevention requirements:

- maintain `attempt_signature = (tool_name, normalized_args_hash, outcome_kind, salient_result_hash)`
- `max_same_signature_repeats = 1`
- `max_family_bounces = 2`
- no edge can point back to a previous stage without an argument transform or stronger scope
- every execution trace must end with an explicit `AssistantExecutionStopReason`

### 8.6 Domain-specific examples

#### A. Calendar / birthdays / next-event

Primary family behaviors:

- `calendar_get_next_event`
- `calendar_upcoming_birthdays`
- `calendar_list_events`
- `calendar_get_event_details`

Example graph:

1. Start with `calendar_get_next_event`
2. If `Answer` -> stop
3. If `Empty` and the prompt contains person/birthday intent -> `calendar_upcoming_birthdays`
4. If `Partial` with event identifier but missing details -> `calendar_get_event_details`
5. If `Ambiguous` on participant/name -> clarification or disambiguation search
6. If `NotFound` -> optional broader `calendar_list_events(window=soon)` in `Thinking`/`Extended`

#### B. Weather

Primary family behaviors:

- `weather_get_current`
- `weather_get_forecast`

Example graph:

1. Start with `weather_get_current(location)` if location exists or entity graph/history can resolve it
2. If `ClarificationNeeded` -> ask for location
3. If `Partial` and user asked “later/tomorrow/this week” -> `weather_get_forecast`
4. If `WeakMatch` on location -> retry once with normalized location
5. If `Empty` -> stop with concise failure unless `Extended` allows one alternate location-resolution attempt

#### C. AI runtime / model questions

Primary family behaviors:

- runtime summary / loaded model / scheduler / benchmark recommendation lookups
- current answer may be deterministic from server state rather than external tool payload

Example graph:

1. Start with runtime summary source or dedicated runtime read tool
2. If `Partial` and question asks “why” or “which role” -> enrich with role routing summary
3. If `Stale` -> refresh from current runtime state rather than persisted snapshot
4. If `Conflicting` between stored recommendation and loaded model -> verifier/synthesis stage must explain both

#### D. Library search / item detail

Primary family behaviors:

- library search
- item detail
- excerpt/detail retrieval

Example graph:

1. Start with search tool
2. If `Ambiguous` with many near-matches -> ask for title/author/library
3. If `Partial` and exact item found -> item detail tool
4. If `WeakMatch` -> one stronger search with normalized title keywords
5. If `NotFound` -> stop or suggest narrower query

#### E. Transcript / summary / excerpt retrieval

Primary family behaviors:

- transcript search
- transcript excerpt fetch
- transcript summary

Example graph:

1. Start with transcript search
2. If `Partial` -> fetch excerpt around matched segment
3. If `Ambiguous` -> clarification on speaker/date/topic
4. If `Answer` but user asked summary of matched region -> transcript summary on bounded excerpt
5. If `Empty` -> optional broader search in `Extended`

#### F. Downloads / rooms / network / server status

Primary family behaviors:

- list/status
- detail/health
- active/current versus historical view

Example graph:

1. Start with the smallest scope status/list tool
2. If `Partial` -> fetch detail for identified entity
3. If `Stale` -> query live status source
4. If `Conflicting` -> prefer live source, then explain snapshot discrepancy
5. If `Ambiguous` -> ask for host/room/server identifier

---

## 9. Execution Loop Design

### 9.1 Planner output shape

Keep the existing `PlannerAst` foundation, but introduce an executor-facing plan type.

Recommended shape:

- `PlannerAst` remains the model-facing structured object.
- Add `ExecutionPlanCandidate` as the executor-facing normalized plan.

Suggested fields:

- `goal_kind`
- `primary_domain_family`
- `requested_response_mode`
- `candidate_steps: Vec<CandidateStep>`
- `clarification_slots`
- `expected_answer_shape`
- `requires_freshness`
- `requires_entity_resolution`

This is important because the executor needs more than “tool name + args.” It needs a typed sense of what kind of answer is expected.

### 9.2 Executor loop

High-level loop:

```text
input request
  -> determine mode budgets
  -> obtain planner candidate(s)
  -> select first candidate step
  -> execute one tool
  -> normalize raw result to semantic outcome
  -> collect evidence
  -> if outcome is sufficient, stop
  -> else consult fallback graph
  -> run next bounded step
  -> merge evidence
  -> stop on explicit reason
  -> deterministic reply or grounded synthesis
```

### 9.3 Tool result inspection

Add deterministic inspectors per domain family.

Example responsibilities:

- identify empty payloads,
- identify multi-match ambiguity,
- extract evidence items and entity keys,
- detect stale timestamps,
- detect exact vs fuzzy match,
- identify whether a detail step is warranted.

### 9.4 Recovery decision

Recovery must be a dedicated pure decision function:

- input: current trace, latest outcome, mode budget, graph registry
- output: `RecoveryDecision`

Possible decisions:

- `Stop(StopReason)`
- `AskClarification(ClarificationPrompt)`
- `RunNext(CandidateStep)`
- `SynthesizeNow`
- `DeterministicReplyNow`
- `VerifierPass`

### 9.5 Synthesis

Synthesis should have two layers:

1. **deterministic synthesis**
   - combine evidence items into a final grounded summary structure
2. **model answer generation**
   - only if deterministic reply does not fully cover the request

The model should receive:

- compact question restatement,
- compact evidence list,
- explicit unknowns/conflicts,
- direct instruction to answer only from evidence.

### 9.6 Final answer selection

Preferred order:

1. Deterministic direct reply
2. Deterministic synthesized reply from multiple evidence items
3. Role-bound answer backend using compact evidence
4. Clarification question
5. Explicit bounded failure with what is missing

### 9.7 Telemetry

Every turn should record:

- planner pass count,
- per-tool latency,
- normalized outcome distribution,
- fallback edge usage,
- evidence retained/dropped,
- final stop reason,
- whether final answer was deterministic or model-generated.

### 9.8 Persistence

Persist the trace in a compact structured form for replay/debugging. Do not persist hidden reasoning.

### 9.9 Pseudocode by mode

#### `Instant`

```text
resolve budgets(Instant)
resolve initial plan
if deterministic direct domain reply available:
    answer
execute first candidate tool
normalize outcome
if outcome == Answer:
    answer
if outcome in {ClarificationNeeded, Ambiguous}:
    ask concise clarification
else:
    stop with bounded insufficiency / not found response
```

#### `Thinking`

```text
resolve budgets(Thinking)
resolve initial plan
loop while steps < max_tool_steps:
    execute next candidate step
    normalize outcome
    collect evidence
    if deterministic reply possible:
        answer
    decision = recovery_decision(trace, outcome, graph, budgets)
    match decision:
        Stop => break
        AskClarification => return clarification
        RunNext(step) => continue
        SynthesizeNow => break
finalize from compact evidence
```

#### `Extended`

```text
resolve budgets(Extended)
resolve initial plan
loop while planner/tool/recovery budgets remain:
    execute next candidate step
    normalize outcome
    collect evidence
    if conflict and verifier allowed:
        run verifier pass once
    decision = recovery_decision(...)
    apply bounded next step or stop
if evidence set is sufficient:
    deterministic synthesis or answer backend
else:
    explicit grounded insufficiency response
```

### 9.10 Structured sequence flow

```text
User
  -> ai_enabled.rs
  -> resolve role routing + scheduler
  -> planner/orchestrator
  -> GroundedExecutor
      -> execute_tool_with_profile
      -> normalize outcome
      -> recovery graph
      -> maybe execute another read-only step
      -> evidence synth
  -> deterministic reply or answer backend
  -> persist trace + emit runtime metrics
  -> SSE final answer
```

---

## 10. Concrete Repo Delta

This section is the implementation contract.

### 10.1 `crates/server/src/ai_assistant/types.rs`

#### Current responsibility
Defines request/response modes, planner debug, tool call shapes, turn stats, and simple tool context blocks.

#### Required change
Extend the type system to support bounded multi-step execution and typed tool outcomes.

#### Add
- `AssistantToolOutcomeKind`
- `AssistantToolOutcome`
- `AssistantEvidenceItem`
- `AssistantExecutionBudget`
- `AssistantExecutionAttempt`
- `AssistantExecutionTrace`
- `AssistantExecutionStopReason`
- `AssistantRecoveryDecision`
- `AssistantDomainFamily`
- `AssistantSynthesisMode`
- `AssistantClarificationRequest`

#### Extend existing types
- `AssistantPhase`
  - add `Grounding`
  - add `Recovering`
  - add `Synthesizing`
  - add `Clarifying`
  - add `Verifying`
- `AssistantTurnStats`
  - add `tool_step_count`
  - add `alternate_tool_count`
  - add `recovery_step_count`
  - add `attempt_count`
  - add `clarification_count`
  - add `conflict_count`
  - add `stop_reason`
  - add `final_outcome_kind`
  - add `deterministic_answer_used`
  - add `synthesis_used`
  - add `role_backend_usage`
  - add per-phase durations where practical
- `AssistantToolContextBlock`
  - keep for backward compatibility, but stop using it as the primary semantic contract

#### Refactor expectation
Existing code paths should gradually shift from raw block inspection to `AssistantToolOutcome` inspection.

#### Tests
- serialization/deserialization tests for all new enums/structs
- backward compatibility tests where existing JSON/debug output is still consumed

### 10.2 `crates/server/src/ai_assistant/provider.rs`

#### Current responsibility
Defines `ToolProvider`, registry builder, registry lookup, execution profiles, subset registries.

#### Required change
Keep the current abstraction. Do not replace it.

#### Add or extend
- add tool metadata needed by the executor either directly in registry entries or through associated tool-spec data:
  - domain family
  - read/write class
  - recovery eligibility
  - can_parallelize
  - ambiguity-prone
  - typical result class
  - freshness semantics

#### Refactor expectation
Prefer adding metadata to registry entries/tool specs rather than expanding the provider trait in a disruptive way.

#### Tests
- verify metadata is present for all public tools
- verify write/protected tools are excluded from recovery-eligible graphs

### 10.3 `crates/server/src/ai_assistant/tools.rs`

#### Current responsibility
Executes a tool through the registry/provider layer and wraps result as a simple `AssistantToolContextBlock`.

#### Required change
Split raw execution from semantic normalization.

#### Add
- `execute_tool_raw(...)`
- `normalize_tool_result(...)`
- `tool_result_to_outcome(...)`
- optional domain-specific inspectors delegated to `outcomes.rs`

#### Refactor
- `execute_tool(...)` should become a thin compatibility wrapper or call the new executor-aware path.
- Protected and confirmation-gated write handling remains exactly as strict as today.
- Recovery logic must never bypass `ToolExecutionProfile` or tool policy checks.

#### Tests
- unit tests for raw-success -> semantic-empty normalization
- ambiguity normalization tests
- denied/protected tests remain intact

### 10.4 `crates/server/src/ai_assistant/outcomes.rs` (new)

#### Current responsibility
New module.

#### Required content
- semantic outcome taxonomy
- deterministic inspectors per domain family
- evidence extraction helpers
- stale/conflict/partial detection rules
- arg-normalization-aware validation helpers

#### Tests
- one file of table-driven tests per domain family
- assert deterministic classification for representative payload fixtures

### 10.5 `crates/server/src/ai_assistant/recovery.rs` (new)

#### Current responsibility
New module.

#### Required content
- fallback graph registry
- domain family stage definitions
- `choose_recovery_step(...)`
- duplicate/signature loop prevention
- budget-aware edge filtering

#### Tests
- graph edge selection tests
- loop prevention tests
- mode budget filter tests
- ACL/profile exclusion tests

### 10.6 `crates/server/src/ai_assistant/synthesis.rs` (new)

#### Current responsibility
New module.

#### Required content
- compact evidence synthesis
- deterministic merge helpers
- conflict rendering rules
- final evidence packet builder for model answer generation

#### Tests
- evidence dedupe tests
- ranking/truncation tests
- conflict summary tests

### 10.7 `crates/server/src/ai_assistant/orchestrator.rs`

#### Current responsibility
Planner prompt construction, AST validation/repair, some one-shot turn preparation.

#### Required change
Keep current planner/repair logic, but make it the front end of the executor rather than the whole execution path.

#### Add
- `plan_execution_candidates(...)`
- `planner_ast_to_execution_candidates(...)`
- optional `replan_with_trace_hint(...)` for `Thinking`/`Extended` only, bounded by `max_planner_passes`

#### Refactor
- `prepare_assistant_turn(...)` should stop being the conceptual center of the flow.
- Its responsibilities should move into the new executor or become a compatibility helper that internally delegates to the executor.
- Planner prompts should output enough structure to support recovery without requiring a full redesign.

#### Planner prompt changes
The planner should continue choosing read-only grounded tools, but should also emit or imply:
- primary domain family
- likely answer shape
- whether a detail/follow-up step is likely
- whether clarification should be preferred over broad search

Do not make the planner schema overly large. Keep it small enough for local structured output reliability.

#### Tests
- planner AST -> execution candidate translation tests
- repair-path tests remain and should expand to cover candidate creation

### 10.8 `crates/server/src/ai_enabled.rs`

#### Current responsibility
Primary request path, streaming, state loading, role routing, scheduler acquisition, answer generation, persistence.

#### Required change
Integrate the new bounded executor into the live request path.

#### Add
- mode budget resolution based on `AssistantResponseMode`
- loading of auxiliary role backends when needed and budget-permitted
- SSE activity events for:
  - `tool_attempt`
  - `recovery_attempt`
  - `clarification_required`
  - `synthesis_started`
  - `verifier_started`
  - `stop_reason`

#### Refactor
- Replace any remaining one-shot planner->join_all->answer flow with executor-driven step-by-step flow.
- Keep confirmation-token write execution on its current strict path.
- Continue to persist grounding artifacts, but now persist compact execution traces too.

#### Role backend wiring
Use the existing role-routing layer more fully:
- `Planner` backend for structured planner passes
- `Answer` backend for final answer generation
- `Summarizer` backend for evidence compaction only if needed in `Extended`
- `Verifier` backend only for bounded conflict or weak-evidence adjudication
- `Worker` role should not imply autonomous sub-agents; use it only if a narrow helper role is truly necessary

If role loading would inflate startup/latency too much, load auxiliary roles lazily and only when a given mode/budget allows them.

#### Tests
- integration tests for mode-specific execution traces
- SSE event coverage for new phases and stop reasons

### 10.9 `crates/server/src/ai_model_routing.rs`

#### Current responsibility
Role routing and recommendation-aware selection.

#### Required change
Keep the existing routing logic and extend its runtime usage, not its conceptual scope.

#### Add if needed
- helper methods for “is auxiliary role worth loading under this budget?”
- explicit source markers for planner/verifier/summarizer routes in telemetry

#### Tests
- preserve current recommendation/fallback tests
- add tests for mode-aware auxiliary role load decisions if helpers are added

### 10.10 `crates/ai-agent/src/backend/role_router.rs` and related backend usage

#### Current responsibility
Role-bound prompt backend wrapper.

#### Required change
Likely minimal.

#### Required usage change
The server should actually exploit the existing role-bound backend support rather than treating role routing as mostly advisory.

#### Tests
- add server-side integration tests that prove planner and answer can bind to different roles/backends without breaking local-default behavior

### 10.11 `crates/server/src/ai_assistant/replies.rs`

#### Current responsibility
Deterministic reply helpers and compact grounding prompt assembly.

#### Required change
Extend replies to understand multi-step evidence and explicit unknowns/conflicts.

#### Add
- deterministic synthesis helpers that accept `Vec<AssistantEvidenceItem>` rather than only raw tool blocks
- response templates for:
  - conflict disclosure
  - clarification questions
  - bounded insufficiency
  - not-found with evidence
  - partial answer plus what remains unknown

#### Refactor
- keep `grounding_chunks_prompt(...)` compact and stable
- allow multi-step evidence union with strict truncation and ranking
- do not inject raw result payloads directly when compact evidence is sufficient

#### Tests
- deterministic multi-evidence reply tests
- contradiction disclosure tests
- not-found and partial-answer tests

### 10.12 `crates/server/src/ai_assistant/memory.rs`

#### Current responsibility
Memory retrieval, grounding artifact persistence, chunk ranking/compression.

#### Required change
Use memory as a supporting evidence source in the new executor, not as an uncontrolled prompt append.

#### Add
- optional `build_grounding_chunks_for_attempt(...)`
- evidence dedupe across multiple attempts
- stronger freshness scoring for multi-step turns

#### Refactor
- ensure unioned evidence across attempts remains compact
- keep ACL filtering identical to today

#### Tests
- duplicate evidence suppression across attempts
- retrieval-quality tests with multi-step traces

### 10.13 `crates/server/src/ai_runtime.rs`

#### Current responsibility
Admin/runtime status response assembly.

#### Required change
Extend runtime response with execution-loop observability.

#### Add fields
- recovery counters by outcome kind
- stop reason distribution
- tool attempts by mode
- deterministic-vs-model answer distribution
- auxiliary role usage distribution
- average step count per mode

#### Tests
- runtime response serialization and population tests

### 10.14 Admin AI diagnostics UI surfaces

#### Current responsibility
Current runtime and benchmark/admin surfaces already expose runtime information.

#### Required change
Add operator-facing visibility for the new executor without bloating end-user UI.

#### Add to admin/runtime views
- selected mode
- selected tools vs attempted tools
- normalized outcomes per attempt
- stop reason
- whether answer was deterministic or synthesized/model-generated
- planner/answer/verifier backend routing decisions
- budget exhaustion indicators

#### Do not add to normal end-user surface
- chain-of-thought
- raw hidden planner repair traces
- internal prompt text dumps by default

### 10.15 Tests and evaluation locations

#### Expand existing module tests
- `orchestrator.rs`
- `provider.rs`
- `scheduler.rs`
- `replies.rs`
- `ai_model_routing.rs`

#### Add new test areas
- `outcomes.rs`
- `recovery.rs`
- `synthesis.rs`
- integration tests covering executor traces and mode contracts

---

## 11. Data Model / Persistence Changes

### 11.1 Why persistence changes are needed

Once Rustyfin becomes multi-step, “what happened during the turn” becomes important for debugging, regression analysis, and admin/runtime trust.

### 11.2 What to persist

Persist compact execution-trace data:

- turn ID / conversation ID
- response mode
- planner mode used
- planner pass count
- attempted tool sequence
- normalized args hash per attempt
- normalized outcome kind per attempt
- fallback edge used per attempt
- evidence item IDs retained
- stop reason
- deterministic vs model-generated final answer path
- role backends used
- per-step latency

### 11.3 What not to persist

Do not persist:

- hidden chain-of-thought,
- unrestricted raw intermediate prompts,
- oversized raw payload dumps unless already part of bounded audited artifacts,
- sensitive data beyond what current audit and ACL design already stores.

### 11.4 Suggested storage shape

Two acceptable approaches:

#### Option A: extend existing turn/audit record JSON
Use one compact `execution_trace_json` field in the existing turn audit path.

Pros:
- low migration complexity
- easy rollout

Cons:
- harder analytics if overused

#### Option B: add typed trace table
Add a dedicated table for execution traces plus compact child rows for attempts.

Pros:
- better analytics and runtime dashboards
- easier aggregation by mode/tool/outcome

Cons:
- more migration work

### 11.5 Recommended approach

Start with Option A if turnaround speed matters. Move to Option B only if admin/runtime analytics become hard to query.

### 11.6 Storage control

Keep trace entries compact:
- store hashes for normalized args and salient result signatures,
- store evidence IDs rather than full repeated evidence text,
- store counts and summaries rather than raw payload duplication.

---

## 12. API / UI Changes

### 12.1 Compatibility goal

Keep `/ai` request/response compatibility unless a clearly justified diagnostic extension is needed.

### 12.2 End-user behavior

Normal users should see:
- better answers,
- better clarification behavior,
- fewer dead-end tool choices,
- occasional concise status text such as “checking another source” or “I found multiple matches.”

Normal users should not see:
- internal recovery graphs,
- raw repair traces,
- chain-of-thought,
- internal confidence numbers unless already part of an approved UX.

### 12.3 SSE/runtime events

Add bounded new SSE event types if the current client/admin surfaces can consume them safely:
- `assistant_tool_attempt`
- `assistant_recovery_attempt`
- `assistant_clarification_required`
- `assistant_synthesis`
- `assistant_stop_reason`

These should be compact and non-sensitive.

### 12.4 Admin/runtime views

Expose:
- selected mode,
- attempted tools,
- outcome sequence,
- stop reason,
- budgets used,
- fallback edge usage,
- final answer path,
- role backend usage.

### 12.5 Prompt details

Admin diagnostics may expose:
- selected vs attempted tools,
- evidence count retained/dropped,
- compact grounding size,
- planner and answer backend route.

They should not expose hidden reasoning text.

---

## 13. Evaluation and Testing Plan

### 13.1 Required test layers

#### Unit tests
Focus:
- outcome normalization
- domain-family fallback selection
- loop prevention
- synthesis dedupe/truncation
- budget enforcement
- role routing decisions under mode budgets

#### Integration tests
Focus:
- full executor traces from prompt to stop reason
- deterministic reply precedence
- model answer fallback with compact evidence
- SSE status/event sequencing
- persistence of execution traces

#### Regression fixtures
Focus on fixed prompt/fixture pairs for:
- wrong-tool first choice
- empty result
- ambiguous entity
- partial result needing detail lookup
- conflicting sources
- stale source
- ACL-denied path
- confirmation-gated write path
- prompt-bloat regression

### 13.2 Dedicated eval harness

Rustyfin now needs a dedicated AI eval harness for grounded execution.

Recommended structure:
- a manifest-driven corpus under `docs/` or a dedicated `crates/*` test support area
- fixtures for tool payloads and expected normalized outcomes
- expected stop reason and expected answer-class assertions

Each eval case should record:
- prompt
- mode
- allowed tools
- fixture payloads
- expected step count range
- expected outcome kind
- expected stop reason
- expected evidence IDs
- expected answer constraints

### 13.3 Pass/fail criteria

Minimum pass criteria:
- no ACL regressions
- no write-confirmation regressions
- `Instant` never exceeds its hard step budget
- `Thinking` recovers correctly on key empty/partial/ambiguity cases
- `Extended` never loops and always records a stop reason
- contradiction cases either reconcile correctly or explicitly disclose conflict
- prompt-budget regression tests remain within cap

### 13.4 Latency guardrails

Add latency assertions by mode where feasible:
- `Instant`: strict upper-bound integration budget
- `Thinking`: median and p95 budget
- `Extended`: hard stop budget and tool-step cap

### 13.5 ACL and write safety tests

Required cases:
- non-admin retrieval cannot widen library scope on fallback
- protected/write tools remain denied without explicit confirmation token
- read-only recovery graph never auto-selects write tools
- a confirmed write action does not inherit multi-step retry behavior

### 13.6 Ambiguity and contradiction tests

Required cases:
- multiple matching people/items/events -> concise clarification, not hallucinated selection
- conflicting runtime/model or live/snapshot values -> bounded reconciliation or explicit conflict answer

### 13.7 Eval corpus contents

The corpus should include at least:
- calendar / birthday / next-event
- weather current vs forecast
- AI runtime / loaded model / benchmark recommendation
- library search/detail
- transcript search/excerpt/summary
- downloads/rooms/network/servers status
- follow-up questions using entity graph / memory
- ACL boundary cases
- confirmation-gated write cases

---

## 14. Risks, Tradeoffs, and Non-Goals

### 14.1 Risks

#### Latency inflation
A multi-step executor can easily make the assistant feel slower. This is the biggest runtime risk.

Mitigation:
- mode-specific hard budgets
- deterministic-first stop conditions
- no model-based verifier except when clearly justified
- compact evidence reuse across steps

#### Prompt bloat
More steps can create larger evidence payloads.

Mitigation:
- evidence dedupe
- capped retained evidence items
- stable ranking/compression
- no raw payload dumps by default

#### Over-calling tools
A generic retry layer can drift into tool spam.

Mitigation:
- domain-family graphs only
- duplicate-signature prevention
- budget caps
- strict same-family alternates in `Thinking`

#### Contradictory results
More sources can increase contradictions.

Mitigation:
- explicit `Conflicting` outcome
- bounded verifier stage
- explicit conflict disclosure path

#### User trust issues
Repeated visible tool attempts can look noisy or confused.

Mitigation:
- concise user-facing status
- stronger final answer quality
- admin-only deep diagnostics

#### ACL boundary risk
Fallback can accidentally widen scope.

Mitigation:
- graph edges encoded with policy/profile checks
- same-or-narrower scope rule
- ACL tests required before rollout

#### Write-action risk
Generalized retries can become unsafe if they touch write flows.

Mitigation:
- no automatic write retries
- explicit separation between read-only recovery graph and confirmation-gated writes

#### Hidden complexity
This upgrade adds a real execution engine.

Mitigation:
- phase rollout
- strong types
- good admin telemetry
- dedicated eval harness

### 14.2 Non-goals

This upgrade is **not** intended to:
- create an unbounded autonomous agent,
- expose shell, REPL, or filesystem editing tools to end users,
- weaken ACL boundaries,
- weaken confirmation gates,
- rely on GPU-heavy serving infrastructure,
- replace Rustyfin’s local-default inference path,
- rewrite the existing planner from scratch,
- replace deterministic replies with free-form model behavior.

---

## 15. Phased Implementation Plan

### Phase 1 — Introduce typed outcomes, budgets, and executor scaffolding

#### Goal
Create the type system and runtime scaffolding for bounded execution without yet turning on broad multi-step recovery.

#### Files likely touched
- `crates/server/src/ai_assistant/types.rs`
- `crates/server/src/ai_assistant/tools.rs`
- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_enabled.rs`
- `crates/server/src/ai_runtime.rs`
- new `crates/server/src/ai_assistant/outcomes.rs`
- new `crates/server/src/ai_assistant/executor.rs`

#### Migrations
- optional: none if trace is initially in existing turn debug JSON

#### Tests to add
- outcome enum serialization
- raw result -> normalized outcome tests
- budget enforcement tests
- executor single-step compatibility tests

#### Acceptance criteria
- live request path uses executor scaffolding even if only one step is enabled
- every turn records explicit stop reason
- `Instant` budget is enforceable in code
- no behavior regressions for current confirmation/write gates

#### Rollback risk
Low to medium. Primary risk is instrumentation or trace-wiring regressions.

#### Deployment behavior changes
Minimal. Most user-visible behavior should remain similar.

### Phase 2 — Add domain-family fallback graphs and bounded recovery

#### Goal
Enable read-only multi-step recovery for the highest-value domains.

#### Files likely touched
- new `crates/server/src/ai_assistant/recovery.rs`
- `crates/server/src/ai_assistant/outcomes.rs`
- `crates/server/src/ai_assistant/tools.rs`
- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_assistant/replies.rs`
- `crates/server/src/ai_assistant/memory.rs`
- provider metadata locations

#### Migrations
- optional execution trace persistence extension

#### Tests to add
- calendar fallback tests
- weather fallback tests
- library fallback tests
- transcript fallback tests
- ambiguity and empty-result regression cases
- loop prevention tests

#### Acceptance criteria
- `Thinking` successfully recovers from representative wrong/empty/partial/ambiguous cases
- no repeated identical tool attempt loops
- compact evidence remains within budget
- no ACL broadening regressions

#### Rollback risk
Medium. Domain graphs can mis-rank alternates if poorly tuned.

#### Deployment behavior changes
Noticeable improvement in answer resilience for `Thinking`.

### Phase 3 — Wire role-bound planner/summarizer/verifier usage and richer synthesis

#### Goal
Exploit the existing role-routing system to improve bounded planning, synthesis, and conflict handling.

#### Files likely touched
- `crates/server/src/ai_enabled.rs`
- `crates/server/src/ai_model_routing.rs`
- `crates/ai-agent/src/backend/role_router.rs` (likely minimal)
- `crates/server/src/ai_assistant/synthesis.rs`
- `crates/server/src/ai_assistant/replies.rs`
- `crates/server/src/ai_runtime.rs`

#### Migrations
- extend persisted trace for role backend usage if not already stored

#### Tests to add
- planner role and answer role split integration tests
- verifier-on-conflict tests
- deterministic-vs-model synthesis path tests

#### Acceptance criteria
- auxiliary roles are only loaded when mode/budget requires them
- `Extended` can perform one bounded verifier pass for conflict/weak evidence cases
- runtime surfaces show backend-role usage clearly

#### Rollback risk
Medium to high if auxiliary model loading affects latency or memory.

#### Deployment behavior changes
`Extended` becomes more capable and more observable; `Instant` remains mostly unchanged.

### Phase 4 — Eval harness, admin diagnostics, and rollout hardening

#### Goal
Make the new executor measurable, regressions catchable, and operator-visible.

#### Files likely touched
- `crates/server/src/ai_runtime.rs`
- admin frontend/runtime diagnostics surfaces
- dedicated eval harness files/tests
- documentation under `docs/`

#### Migrations
- optional analytics fields/tables if needed for richer dashboards

#### Tests to add
- full eval corpus execution
- latency guardrails
- mode-specific regression snapshots
- admin serialization/UI contract tests

#### Acceptance criteria
- dedicated eval corpus exists and runs in CI or pre-release workflow
- admin surfaces expose stop reasons, attempts, and outcome distributions
- rollout can be monitored by mode and domain family

#### Rollback risk
Low. Mostly observability/test additions.

#### Deployment behavior changes
Operator visibility improves substantially; user-facing behavior is largely stable.

---

## 16. Final Target State

When complete, the correct final version should behave like this:

### User-visible behavior
- `Instant` answers fast and cleanly for direct questions without over-searching.
- `Thinking` recovers from common tool-selection mistakes, empty results, and partial answers instead of giving up immediately.
- `Extended` can combine evidence across multiple safe read-only steps and explain ambiguity or conflict clearly.
- Users see better clarification questions when required.
- Users do not see hidden reasoning or unsafe tool behavior.

### Backend behavior
- Planner remains structured and repaired when needed.
- Every step produces a typed semantic outcome.
- Recovery is graph-driven, bounded, and deterministic-first.
- Final answers are derived from compact retained evidence, not raw result sprawl.
- Distinct model roles can be used where justified, but local/default behavior remains intact.

### Observability
- Every turn has a stop reason.
- Admin/runtime surfaces can show attempted tools, outcomes, fallback edges, and role usage.
- Benchmark and eval data can measure recovery quality, not just raw generation.

### Safety properties
- ACL boundaries remain intact across retries.
- Confirmation-gated writes remain outside the automatic recovery graph.
- No shell/filesystem edit/repl exposure is added.
- No chain-of-thought is surfaced or persisted.

### Evaluation quality bar
- wrong-tool, empty, ambiguous, partial, stale, and conflict cases have deterministic expected outcomes
- `Instant` remains fast
- `Thinking` materially improves grounded answer success rate
- `Extended` remains bounded and observable

---

## 17. Final Checklist

- [ ] Read current `README.md`, `AGENTS.md`, and `CLAUDE.md` before modifying architecture.
- [ ] Add typed semantic outcome model for tool execution.
- [ ] Add explicit mode budgets for `Instant`, `Thinking`, and `Extended`.
- [ ] Introduce a bounded executor layer above planner and tool registry.
- [ ] Keep existing planner AST/repair path; do not replace it wholesale.
- [ ] Keep `ToolProvider`/`ToolRegistry`; extend metadata instead of rebuilding it.
- [ ] Split raw tool execution from semantic outcome normalization.
- [ ] Add domain-family fallback graph registry.
- [ ] Implement duplicate-signature and loop-prevention logic.
- [ ] Keep retries read-only and ACL-preserving.
- [ ] Exclude write/protected tools from automatic recovery.
- [ ] Extend deterministic reply/synthesis to consume multi-step evidence.
- [ ] Keep evidence compact and prompt-budget aware.
- [ ] Persist compact execution traces with stop reasons.
- [ ] Extend admin/runtime diagnostics with attempt and stop-reason visibility.
- [ ] Wire actual role-bound backend usage only where budget justifies it.
- [ ] Add unit tests for outcome normalization and graph selection.
- [ ] Add integration tests for multi-step recovery traces.
- [ ] Add eval corpus for empty/ambiguous/partial/conflict/ACL/write safety cases.
- [ ] Verify `Instant` remains narrow and low-latency.
- [ ] Verify `Thinking` improves bounded recovery quality.
- [ ] Verify `Extended` remains hard-capped and observable.
- [ ] Verify no chain-of-thought exposure.
- [ ] Verify no ACL regressions.
- [ ] Verify no write-confirmation regressions.

---

## Appendix A — Most Important Repo-Grounded Recommendation

The single most important architectural change is:

**Add a typed, bounded `GroundedExecutor` above the existing planner and tool registry rather than changing either of those subsystems wholesale.**

Why this is the right center of gravity:
- the planner is already materially stronger than a loose JSON parser,
- the tool layer is already provider-based and policy-aware,
- the scheduler and runtime already support bounded local-first operation,
- deterministic reply and compact evidence patterns already exist,
- the missing capability is generalized multi-step recovery, not another planner rewrite.

---

## Appendix B — Research References

Primary Rustyfin repo sources consulted:
- `https://github.com/Iwan-Teague/Rustyfin`
- `https://raw.githubusercontent.com/Iwan-Teague/Rustyfin/main/README.md`
- `https://raw.githubusercontent.com/Iwan-Teague/Rustyfin/main/AGENTS.md`
- `https://raw.githubusercontent.com/Iwan-Teague/Rustyfin/main/CLAUDE.md`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/orchestrator.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/tools.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/provider.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/replies.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/memory.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_assistant/scheduler.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_enabled.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_runtime.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/server/src/ai_model_routing.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/ai-agent/src/engine.rs`
- `https://github.com/Iwan-Teague/Rustyfin/blob/main/crates/ai-agent/src/backend/role_router.rs`

External projects/docs consulted:
- Ollama: `https://github.com/ollama/ollama`, `https://docs.ollama.com/api`, `https://docs.ollama.com/capabilities/structured-outputs`
- llama.cpp: `https://github.com/ggml-org/llama.cpp`, `https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md`
- LocalAI: `https://github.com/mudler/LocalAI`, `https://localai.io/features/openai-functions/`
- Continue: `https://github.com/continuedev/continue`, `https://docs.continue.dev/customize/deep-dives/agent`, `https://docs.continue.dev/customize/deep-dives/context-providers`, `https://docs.continue.dev/customize/deep-dives/docs`
- Open WebUI: `https://github.com/open-webui/open-webui`, `https://docs.openwebui.com/features/plugin/functions/`
- AnythingLLM: `https://github.com/Mintplex-Labs/anything-llm`, `https://docs.anythingllm.com/agents`, `https://docs.anythingllm.com/agent/custom/introduction`
- OpenHands: `https://github.com/OpenHands/OpenHands`, `https://docs.openhands.dev/sdk/arch/agent`, `https://docs.openhands.dev/openhands/usage/developers/evaluation-harness`
- Claw Code: `https://github.com/ultraworkers/claw-code`

