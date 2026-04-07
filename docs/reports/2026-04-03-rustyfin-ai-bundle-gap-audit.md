# Rustyfin AI Research Bundle Gap Audit

Date: 2026-04-03

## Scope

This audit compares the Rustyfin AI implementation on `main` against the attached research bundle:

- `rustyfin_ai_research_delta.md`
- `rustyfin_ai_phase_prompts.md`
- `rustyfin_ai_backlog.json`
- `rustyfin_ai_findings_matrix.csv`

## Status

- The merged code has already been pushed to `origin/main` at `a1ce08c`.
- The live Ubuntu deployment verification step was not completed in this session because SSH access to `server@192.168.0.36` was not available from the local environment.

## What Appears Covered

- Turn journaling and compact-boundary persistence exist.
- Typed memory items and topic-based retrieval exist.
- Operational retrieval over live Rustyfin sources exists with CPU-friendly full-text search.
- Grounding chunk compression and citation metadata exist.
- Follow-up entity handling is upgraded with graph-style storage.
- Transcript Q&A now has citation backlinks to excerpt windows.
- The scheduler, warm-model pool, overload states, benchmark persistence, and remote backend hooks exist.
- Admin runtime and benchmarking panels exist.

## Missing Or Partial Requirements

### 1. Planner schema hardening is incomplete

- The planner still parses model JSON into a loose `ModelPlannerResponse` and normalizes it.
- I did not find a full typed planner AST or a strict repair loop.
- The planner diagnostics fields exist in `AssistantTurnStats`, but the runtime turn builder does not populate them.

Relevant code:

- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_assistant/types.rs`
- `crates/server/src/ai_enabled.rs`

### 2. Role-based model routing is incomplete

- The scheduler can choose local vs remote planner execution.
- I did not find distinct routing for planner, summarizer, answer, memory-selector, or verifier roles.
- The stored benchmark recommendations are visible in admin UI, but they are not fed back into runtime model selection automatically.

Relevant code:

- `crates/server/src/ai_assistant/scheduler.rs`
- `crates/server/src/ai_benchmark.rs`
- `crates/server/src/ai_enabled.rs`
- `crates/server/src/ai_enabled.rs` still uses `engine_params_from_env()`

### 3. Background task framework is missing

- I did not find task storage, task runner, task progress endpoints, task cancellation, or resume logic for long-running AI jobs.
- Turn journaling exists, but that is not the same as a resumable task system.

Relevant code search result:

- No `task_runner`, `task_store`, or task progress API surface in `crates/server/`

### 4. Coordinator / worker orchestration is missing

- I did not find bounded worker profiles for research/synthesis/verification.
- There is no coordinator that fans out sub-work and merges structured worker results.

Relevant code search result:

- No `coordinator.rs` or `worker_profiles.rs` in the AI assistant modules

### 5. ToolProvider abstraction is missing

- Tools are still centralized through the current registry and executor paths.
- I did not find a `ToolProvider` trait boundary with provider registration and per-provider summarize/context methods.

Relevant code:

- `crates/server/src/ai_assistant/registry.rs`
- `crates/server/src/ai_assistant/tools.rs`

### 6. Evaluation harness is missing

- I did not find a dedicated eval corpus or benchmark harness for planner accuracy, memory recall, retrieval quality, or long-task behavior.
- The repo still relies on unit and integration tests rather than a structured AI eval suite.

Relevant code search result:

- No dedicated eval harness module or eval corpus surfaced in `crates/server/`, `crates/db/`, `crates/ai-agent/`, or `docs/`

### 7. Benchmark recommendations are not yet auto-applied

- Host-specific benchmarking and recommendation persistence are implemented.
- I did not find code that automatically applies the stored recommended `n_threads`, `n_gpu_layers`, or split-mode profile to runtime startup.
- As written, benchmarking is advisory and observable, but not a closed-loop auto-tuning system.

Relevant code:

- `crates/server/src/ai_benchmark.rs`
- `crates/server/src/ai_admin.rs`
- `crates/server/src/ai_enabled.rs`
- `crates/ai-agent/src/engine.rs`

## Bottom Line

The bundle’s core grounding, retrieval, memory, scheduler, benchmarking, and remote-backend objectives are broadly covered. The main remaining gaps are the higher-level orchestration features:

- typed planner schema + repair loop
- role-based model routing
- long-running task framework
- coordinator / worker orchestration
- ToolProvider abstraction
- evaluation harness
- automatic application of benchmark recommendations

