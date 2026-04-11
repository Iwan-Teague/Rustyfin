# Rustyfin AI Judge Phase 1: Deterministic Foundation

Date: 2026-04-11  
Status: completed

Parent plan: [docs/plans/2026-04-07-ai-judge-improvement-plan.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md)  
Prompt: [docs/prompts/2026-04-11-ai-judge-phase-1-agent-prompt.md](/Users/iwan/Desktop/Rustyfin/docs/prompts/2026-04-11-ai-judge-phase-1-agent-prompt.md)  
Next phase: [docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md)

## Purpose

Build the deterministic base of the judge system on top of Rustyfin's existing suite-based eval harness. This phase should make judge runs reproducible, schema-checked, and fail-fast before any model-graded scoring is introduced.

## Current Anchors

This phase should extend the current harness rather than replace it:

- `crates/server/src/ai_eval_harness/mod.rs`
- `crates/server/src/ai_eval_harness/corpus.rs`
- `crates/server/src/ai_eval_harness/report.rs`
- `crates/server/src/ai_eval_harness/planner_eval.rs`
- `crates/server/src/ai_eval_harness/retrieval_eval.rs`
- `crates/server/src/ai_eval_harness/memory_eval.rs`
- `crates/server/src/ai_eval_harness/execution_eval.rs`
- `crates/server/src/ai_eval_harness/tasks_eval.rs`
- `crates/ai-evals/src/main.rs`
- `tests/fixtures/ai/*.jsonl`

Keep the existing five suite corpora as the load-bearing baseline. A later unified manifest may normalize them, but this phase should not break the current suite split.

## Checklist

- [x] Define a run manifest that records `run_id`, `git_sha`, `base_sha`, `dataset_version`, `judge_version`, `rubric_version`, `fixture_digest`, `schema_digest`, `tool_registry_digest`, `model_id`, `backend_kind`, `seed`, `timezone`, and `locale`
- [x] Extend the report model so runs can carry case-level verdicts instead of only suite totals
- [x] Add deterministic hard gates for schema validity, JSON validity, length, refusal correctness, ACL/privacy boundaries, and exact-answer domains
- [x] Keep hard gates binary: fail the run when any blocker fails
- [x] Add a stable markdown summary and JSON output contract for CI and local review
- [x] Add failure buckets so the judge can report why a case failed without turning those reasons into soft pass criteria
- [x] Add loader and schema tests for every current suite fixture file
- [x] Add run replay tests that prove the same manifest produces the same verdicts

## Implementation Notes

- Use the current suite-specific loaders as the first implementation surface.
- Do not add pairwise comparison or rubric scoring in this phase.
- Keep deterministic checks cheap so they can run in PR smoke gates.
- Keep the report shape forward-compatible with later rubric and comparison metadata.

## Validation

Run the strongest relevant checks after the deterministic layer lands:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- `cargo test -p rustfin-server --test integration --no-run`

If the phase touches any UI surface or artifact rendering path, add the UI build too:

- `npm --prefix ui run build`

## Exit Criteria

This phase is done when all of the following are true:

- judge cases load through a versioned, validated manifest
- hard failures stop the run deterministically
- the report contains stable replay metadata
- the existing harness suites still run
- CI can consume the JSON and markdown outputs without custom ad hoc parsing

## Progress Notes

Use this section for dated implementation notes while the phase is in progress.

- 2026-04-11: Added a manifest-aware eval report contract with `fixture_digest`, `schema_digest`, `tool_registry_digest`, per-case blocker verdicts, failure buckets, and stable markdown/JSON output versions.
- 2026-04-11: Added checked-in schemas for all five current suite corpora, made the loaders fail fast on invalid JSON/schema rows, and wired suite-specific hard gates for malformed output, length, refusal, ACL/privacy, and exact-answer checks.
- 2026-04-11: Added replay coverage for fixed-manifest verdict stability and extended the `ai-evals` CLI with `--markdown-out` so CI and local runs can archive both machine-readable and human-readable artifacts.
- 2026-04-11: Removed volatile wall-clock task timings from serialized task-case details so fixed-manifest replays now produce stable report payloads while still preserving runtime-budget pass/fail semantics.
- 2026-04-11: Phase 2 extended the shared report contract to `v3`, adding optional rubric verdicts, human-review counts, calibration disagreement metadata, and explicit non-blocking threshold support while preserving the original hard-gate pass authority.
