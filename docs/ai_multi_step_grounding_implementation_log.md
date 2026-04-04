# AI Multi-Step Grounding Implementation Log

## 2026-04-04 11:19:05 IST — Run Start

### Phase 0 — Preparation and Repo/Delta Alignment

Status:
- started

Summary:
- Read `README.md`, `AGENTS.md`, and `CLAUDE.md`.
- Read the authoritative delta from `/Users/iwanteague/Downloads/ai_multi_step_grounding_delta.md`.
- Confirmed the live repo still matches the delta closely enough to proceed.

Repo drift notes:
- `docs/ai_multi_step_grounding_delta.md` was not present in the repository.
- Copied the authoritative delta from `/Users/iwanteague/Downloads/ai_multi_step_grounding_delta.md` into [docs/ai_multi_step_grounding_delta.md](/Users/iwanteague/Desktop/Rustyfin/docs/ai_multi_step_grounding_delta.md) so the implementation contract now lives in-repo.
- The live repo already contains one narrow birthday-only recovery rule in [crates/server/src/ai_enabled.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs). This is consistent with the delta intent but much narrower than the required generic executor, so it will be absorbed or replaced by the new bounded execution model.
- The live repo already has `ToolProvider` / `ToolRegistry`, role routing, scheduler admission control, prompt-cache hints, eval harness modules, and compact grounding retrieval/memory. Those will be extended rather than replaced.

Initial implementation plan summary:
- Phase 1: introduce typed tool outcomes, execution budgets, execution trace/stop reasons, and executor scaffolding while keeping behavior compatible.
- Phase 2: add domain-family fallback graphs, bounded recovery decisions, and generic loop prevention.
- Phase 3: add richer evidence synthesis, mode-aware auxiliary role usage, and runtime/admin observability for the new execution model.
- Phase 4: expand eval coverage, finish diagnostics, run full validation, deploy to Ubuntu, verify live behavior, commit, and push to `main`.

Files inspected:
- [README.md](/Users/iwanteague/Desktop/Rustyfin/README.md)
- [AGENTS.md](/Users/iwanteague/Desktop/Rustyfin/AGENTS.md)
- [CLAUDE.md](/Users/iwanteague/Desktop/Rustyfin/CLAUDE.md)
- [docs/ai_multi_step_grounding_delta.md](/Users/iwanteague/Desktop/Rustyfin/docs/ai_multi_step_grounding_delta.md)
- [crates/server/src/ai_assistant/provider.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/provider.rs)
- [crates/server/src/ai_runtime.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs)

Validation:
- none yet

Current risk notes:
- The executor upgrade touches the main SSE request path in `ai_enabled.rs`, which is the highest integration-risk area.
- The current runtime/admin surfaces expose scheduler/resource state but not yet full execution-loop telemetry, so backend and UI/API changes will need to stay tightly aligned.

## 2026-04-04 11:56:45 IST — Phase 1 Complete

### Phase 1 — Typed Outcomes, Budgets, and Executor Scaffolding

Status:
- completed

What changed:
- Added typed execution-domain and outcome primitives, execution budgets, stop reasons, attempt records, and compact execution traces in [crates/server/src/ai_assistant/types.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs).
- Extended tool metadata in [crates/server/src/ai_assistant/registry.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs) and [crates/server/src/ai_assistant/provider.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/provider.rs) so the executor can reason about domain family, recovery eligibility, ambiguity-prone tools, and freshness-sensitive tools without weakening existing registry enforcement.
- Split raw tool execution from semantic outcome normalization in [crates/server/src/ai_assistant/tools.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs) and new [crates/server/src/ai_assistant/outcomes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/outcomes.rs).
- Added the bounded executor layer in new [crates/server/src/ai_assistant/executor.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/executor.rs) and exposed it through [crates/server/src/ai_assistant/mod.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/mod.rs).
- Kept the existing planner AST, repair, and registry flow intact. The executor sits above the planner instead of replacing it.

Files changed:
- [crates/server/src/ai_assistant/types.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs)
- [crates/server/src/ai_assistant/registry.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs)
- [crates/server/src/ai_assistant/provider.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/provider.rs)
- [crates/server/src/ai_assistant/tools.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs)
- [crates/server/src/ai_assistant/outcomes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/outcomes.rs)
- [crates/server/src/ai_assistant/executor.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/executor.rs)
- [crates/server/src/ai_assistant/mod.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/mod.rs)

Migrations added:
- none

Tests added or updated:
- provider metadata coverage in [crates/server/src/ai_assistant/provider.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/provider.rs)
- outcome normalization coverage in [crates/server/src/ai_assistant/outcomes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/outcomes.rs)
- executor budget/clarification tests in [crates/server/src/ai_assistant/executor.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/executor.rs)

Deviations from delta:
- No DB migration was added for execution traces. This intentionally follows the delta’s Option A recommendation to extend existing turn/audit JSON rather than adding a dedicated trace table in the first rollout.

Validation:
- `cargo fmt --all`
- `cargo test -p rustfin-server --lib --features ai`

Current risk notes:
- Outcome classification is intentionally conservative. If a tool result looks semantically complete, the executor will stop rather than fan out.
- The highest residual risk is still classification drift for broad “system/runtime” prompts, because those prompts can be semantically close while requiring different tools.

## 2026-04-04 11:56:45 IST — Phase 2 Complete

### Phase 2 — Domain-Family Fallback Graphs and Bounded Recovery

Status:
- completed

What changed:
- Added generic fallback-graph selection in new [crates/server/src/ai_assistant/recovery.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/recovery.rs).
- Recovery is bounded by mode-specific budgets, duplicate-signature checks, alternate-step caps, and recovery-depth caps.
- Automatic recovery stays read-only and ACL-preserving by reusing `ToolExecutionProfile` denial checks and registry metadata.
- Write/protected tools are excluded from automatic recovery.
- Added representative recovery paths for birthdays/next-event, AI runtime/model queries, weather normalization, and library detail enrichment.
- Fixed the AI-runtime edge case where a host-runtime summary was being treated as a full answer for `What AI model is loaded?`; host-runtime output is now a partial outcome for AI-runtime intent and triggers the bounded AI-runtime fallback path.

Files changed:
- [crates/server/src/ai_assistant/recovery.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/recovery.rs)
- [crates/server/src/ai_assistant/outcomes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/outcomes.rs)
- [tests/fixtures/ai/execution_cases.jsonl](/Users/iwanteague/Desktop/Rustyfin/tests/fixtures/ai/execution_cases.jsonl)
- [crates/server/src/ai_eval_harness/execution_eval.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_eval_harness/execution_eval.rs)

Migrations added:
- none

Tests added or updated:
- recovery graph tests in [crates/server/src/ai_assistant/recovery.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/recovery.rs)
- execution eval corpus and runner in [tests/fixtures/ai/execution_cases.jsonl](/Users/iwanteague/Desktop/Rustyfin/tests/fixtures/ai/execution_cases.jsonl) and [crates/server/src/ai_eval_harness/execution_eval.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_eval_harness/execution_eval.rs)
- additional outcome normalization coverage for AI-runtime intent in [crates/server/src/ai_assistant/outcomes.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/outcomes.rs)

Deviations from delta:
- Freshness is currently modeled as compact metadata on outcomes plus tool metadata rather than a broad emitted `Stale` recovery outcome across all domains. This keeps the first rollout aligned with live tool behavior without inventing synthetic stale states where the provider payloads do not expose them.

Validation:
- `cargo test -p rustfin-server --lib --features ai`
- `cargo run -p ai-evals -- execution --json-out target/ai-evals/execution-report.json`

Current risk notes:
- Recovery graphs are intentionally allowlisted by domain family. Expanding them carelessly would reintroduce tool spam.
- Weather and runtime recovery remain the most semantically sensitive cases because they often look superficially successful before the executor inspects the payload.

## 2026-04-04 11:56:45 IST — Phase 3 Complete

### Phase 3 — Synthesis, Role Usage, and Diagnostics

Status:
- completed

What changed:
- Added retained-evidence collection and conflict counting in new [crates/server/src/ai_assistant/synthesis.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/synthesis.rs).
- Extended deterministic reply handling in [crates/server/src/ai_assistant/replies.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/replies.rs) to consume multi-step traces and synthesize bounded failures/conflict disclosures from retained evidence.
- Replaced the remaining one-shot grounded execution block in [crates/server/src/ai_enabled.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs) with executor-driven step sequencing, stop reasons, clarification handling, attempt telemetry, and compact evidence retention.
- Persisted compact execution traces through existing JSON-bearing artifacts by storing them in `AssistantTurnStats` and embedding them into persisted planner diagnostics before audit writes.
- Extended [crates/server/src/ai_runtime.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs), [ui/src/lib/aiApi.ts](/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts), [ui/src/lib/aiAdminApi.ts](/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiAdminApi.ts), [ui/src/app/ai/page.tsx](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx), and [ui/src/app/admin/page.tsx](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/admin/page.tsx) so runtime and admin surfaces now show stop reason, attempt counts, tool steps, alternates, recovery steps, and outcome distributions.
- Planner and answer role usage stay on the existing role-routing path; the executor now records actual routed role backends in the execution trace instead of treating routing as invisible advisory state.

Files changed:
- [crates/server/src/ai_assistant/synthesis.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/synthesis.rs)
- [crates/server/src/ai_assistant/replies.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/replies.rs)
- [crates/server/src/ai_enabled.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs)
- [crates/server/src/ai.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai.rs)
- [crates/server/src/ai_runtime.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs)
- [ui/src/lib/aiApi.ts](/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts)
- [ui/src/lib/aiAdminApi.ts](/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiAdminApi.ts)
- [ui/src/app/ai/page.tsx](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx)
- [ui/src/app/admin/page.tsx](/Users/iwanteague/Desktop/Rustyfin/ui/src/app/admin/page.tsx)

Migrations added:
- none

Tests added or updated:
- runtime serialization coverage in [crates/server/src/ai_runtime.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs)
- deterministic multi-step reply coverage in [crates/server/src/ai_assistant/replies.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/replies.rs)
- retained-evidence/conflict counting tests in [crates/server/src/ai_assistant/synthesis.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/synthesis.rs)

Deviations from delta:
- The first rollout uses existing planner/answer role routing directly and records routed roles in the trace. It does not introduce a separate verifier or summarizer generation pass unless the bounded executor explicitly needs that later.

Validation:
- `cargo fmt --all`
- `cargo test -p rustfin-server --lib --features ai`
- `npm --prefix ui run build`

Current risk notes:
- The main integration risk remains [crates/server/src/ai_enabled.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs) because it now owns the executor loop, SSE emission, deterministic short-circuit replies, and final model generation handoff.
- Admin audit visibility depends on compact planner/turn JSON staying stable. That keeps rollout simple but means future analytics may eventually want a dedicated trace table if query pressure grows.

## 2026-04-04 11:56:45 IST — Phase 4 Validation and Rollout Start

Status:
- in progress

What changed:
- Added the execution eval suite to [crates/server/src/ai_eval_harness/mod.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_eval_harness/mod.rs).
- Updated operator docs in [README.md](/Users/iwanteague/Desktop/Rustyfin/README.md) and [AGENTS.md](/Users/iwanteague/Desktop/Rustyfin/AGENTS.md) to describe the bounded executor and the new execution telemetry surfaces.

Files changed:
- [crates/server/src/ai_eval_harness/mod.rs](/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_eval_harness/mod.rs)
- [README.md](/Users/iwanteague/Desktop/Rustyfin/README.md)
- [AGENTS.md](/Users/iwanteague/Desktop/Rustyfin/AGENTS.md)

Validation so far:
- `cargo fmt --all`
- `cargo test -p rustfin-server --lib --features ai`
- `cargo run -p ai-evals -- execution --json-out target/ai-evals/execution-report.json`
- `npm --prefix ui run build`

Current risk notes:
- Full-workspace `clippy` and `cargo test --workspace` have not been run yet in this phase marker.
- Live deployment verification is still pending.
