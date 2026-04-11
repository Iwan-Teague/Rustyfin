# Rustyfin AI Judge Phase 3: Pairwise Comparison Experiments

Date: 2026-04-11  
Status: completed

Parent plan: [docs/plans/2026-04-07-ai-judge-improvement-plan.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md)  
Prompt: [docs/prompts/2026-04-11-ai-judge-phase-3-agent-prompt.md](/Users/iwan/Desktop/Rustyfin/docs/prompts/2026-04-11-ai-judge-phase-3-agent-prompt.md)  
Previous phase: [docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md)  
Next phase: [docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md)

## Purpose

Add explicit comparison-mode evaluation so Rustyfin can compare two candidate outputs for the same case without folding the comparison into the default per-answer verdict path.

## Current Anchors

Use the same shared judge/report infrastructure as the earlier phases and add a dedicated comparison path on top:

- `crates/server/src/ai_eval_harness/judge.rs`
- `crates/server/src/ai_eval_harness/judge_rubric.rs`
- `crates/server/src/ai_eval_harness/judge_metrics.rs`
- `crates/server/src/ai_eval_harness/judge_reports.rs`
- `crates/server/src/ai_eval_harness/report.rs`
- `crates/ai-evals/src/main.rs`

Comparison runs should consume the same manifest metadata but must keep baseline and candidate responses separate.

## Checklist

- [x] Add a comparison run mode that accepts `baseline_model_response` plus the candidate response
- [x] Keep pairwise selection separate from the default verdict path
- [x] Add order-bias protection, such as response flipping or randomized presentation order
- [x] Add tie handling and explicit “no winner” behavior where appropriate
- [x] Emit comparison-specific report artifacts instead of reusing pointwise verdict output
- [x] Keep max-score or select-best style aggregation isolated to comparison experiments
- [x] Add tests that prove the winner changes only when the candidate meaningfully changes
- [x] Add tests that prove swapping response order does not change the final result unexpectedly

## Implementation Notes

- Pairwise comparison is for experiments and prompt/model selection, not for the standard single-answer judge verdict.
- If the same case can be judged pointwise and pairwise, keep those outputs in separate report sections.
- Prefer a response-flipping strategy when the underlying grader is susceptible to presentation bias.
- Record the model/config pair used for each side of the comparison.

## Validation

Run the focused comparison suite and the base judge checks after this phase lands:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- any new comparison-specific unit tests or fixtures

If this phase adds any report or UI artifact surface, also run:

- `npm --prefix ui run build`

## Exit Criteria

This phase is done when all of the following are true:

- comparison experiments can compare two outputs for the same case
- pairwise results stay separate from the default verdict path
- order bias is controlled or documented
- report artifacts clearly distinguish pointwise and pairwise output
- the implementation can support prompt/model selection work without changing the base judge semantics

## Progress Notes

Use this section for dated implementation notes while the phase is in progress.

- 2026-04-11: Added a fixture-backed `comparison` eval mode with separate JSON/markdown comparison report contracts so pairwise experiments do not alter the default pointwise `EvalReport` path.
- 2026-04-11: Added a versioned pairwise prompt/schema parser, baseline-first plus candidate-first presentation checks, and a deterministic `no_winner` fallback whenever the normalized winner changes across the flipped order.
- 2026-04-11: Added comparison fixtures and unit coverage for stable candidate wins, tie preservation, explicit order-bias disagreement handling, and CLI/report-path separation from the pointwise suites.
