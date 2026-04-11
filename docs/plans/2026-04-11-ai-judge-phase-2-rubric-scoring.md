# Rustyfin AI Judge Phase 2: Subjective Rubric Scoring

Date: 2026-04-11  
Status: completed

Parent plan: [docs/plans/2026-04-07-ai-judge-improvement-plan.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md)  
Prompt: [docs/prompts/2026-04-11-ai-judge-phase-2-agent-prompt.md](/Users/iwan/Desktop/Rustyfin/docs/prompts/2026-04-11-ai-judge-phase-2-agent-prompt.md)  
Previous phase: [docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md)  
Next phase: [docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md)

## Purpose

Add the model-graded layer for subjective scoring after the deterministic hard gates are already in place. This phase should judge quality dimensions such as concision, clarity, groundedness, and completeness without letting any soft score override a hard failure.

## Current Anchors

The rubric layer should plug into the same judge/report flow introduced in Phase 1:

- `crates/server/src/ai_eval_harness/report.rs`
- `crates/server/src/ai_eval_harness/judge_eval.rs`
- `crates/server/src/ai_eval_harness/judge_metrics.rs`
- `crates/server/src/ai_eval_harness/judge_rubric.rs`
- `crates/server/src/ai_eval_harness/judge.rs`
- `crates/ai-evals/src/main.rs`
- `tests/fixtures/ai/judge_cases.jsonl`
- `tests/fixtures/ai/judge_cases.schema.json`

If a model-backed grader is needed, keep it isolated behind a versioned judge prompt and a deterministic response schema.

## Checklist

- [x] Define rubric families for the judge, such as brevity, relevance, groundedness, clarity, and safety review
- [x] Version the judge prompt and rubric prompt separately
- [x] Define a structured rubric response schema with `pass`, `score`, `reason`, and confidence fields
- [x] Add confidence and disagreement handling so low-confidence scores can route to human review
- [x] Keep rubric scoring soft-only; hard gates must remain authoritative
- [x] Add calibration fixtures with human labels for the rubric judge
- [x] Add tests that prove threshold semantics are strict and reproducible
- [x] Record `judge_version` and `rubric_version` in every run artifact

## Implementation Notes

- Prefer the fastest reliable grading path that still matches the task.
- Use more than one rubric only when it removes ambiguity; do not collapse unrelated concerns into a single score.
- Keep rubric prompts short, explicit, and example-backed.
- Make the reason text useful for audit and tuning, not just decorative.

## Validation

Run deterministic and rubric-focused checks together so the new layer cannot drift from the hard gate layer:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- any focused rubric tests or fixtures added for the new judge backend

If the rubric implementation touches front-end review surfaces or run artifacts, add:

- `npm --prefix ui run build`

## Exit Criteria

This phase is done when all of the following are true:

- subjective scoring is versioned and reproducible
- the rubric layer reports confidence and rationale
- hard failures still fail regardless of soft score
- calibration cases can be reviewed against human labels
- the result format is stable enough for future pairwise and CI work

## Progress Notes

Use this section for dated implementation notes while the phase is in progress.

- 2026-04-11: Added `judge_metrics.rs` and `judge_rubric.rs` with a versioned `response_quality` rubric contract covering concision, clarity, groundedness, and completeness plus structured rationale/confidence parsing.
- 2026-04-11: Extended the Phase 1 report contract so case verdicts can carry optional rubric verdicts, human-review routing, calibration disagreement state, and non-blocking threshold metadata without weakening hard-gate authority.
- 2026-04-11: Added the `judge` calibration suite with fixture-backed human labels, low-confidence routing, disagreement handling, and deterministic pass/review/agreement checks over `tests/fixtures/ai/judge_cases.jsonl`.
