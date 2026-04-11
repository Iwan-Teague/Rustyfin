# Rustyfin AI Judge Phase 2 Agent Prompt

Date: 2026-04-11  
Scope: add subjective rubric scoring on top of the deterministic judge foundation

## Read First

Read these files in order before changing anything:

1. `/Users/iwan/Desktop/Rustyfin/README.md`
2. `/Users/iwan/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwan/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`
5. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md`
6. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md`
7. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/report.rs`
8. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/judge_metrics.rs`
9. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/judge_rubric.rs`

Use Phase 1 as the base contract. Do not break the deterministic gate layer.

## Objective

Add model-graded rubric scoring for the judge.

This phase should:

- score subjective dimensions such as concision, clarity, groundedness, and completeness
- keep the rubric prompt and rubric schema versioned
- preserve hard-gate authority
- store confidence and rationale for auditability
- support human calibration and disagreement handling

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Hard gate failures must still fail regardless of rubric score.
- Do not collapse unrelated rubric concerns into one opaque score unless the phase doc explicitly says to do so.
- Do not let rubric scoring mutate product data.
- Keep the rubric output structured and reproducible.

## Work Plan

1. Define the rubric dimensions and the scoring contract.
2. Implement the rubric prompt/schema and the response parser.
3. Version the judge prompt separately from the rubric prompt.
4. Add confidence handling and low-confidence routing to human review.
5. Add calibration fixtures with human labels.
6. Add tests for score thresholds, pass semantics, and hard-gate isolation.
7. Update the phase doc checklist and the umbrella plan status table as you go.

## Progress Marking Rules

As you complete work, update all of the following:

- mark the matching checkbox in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md`
- add a short dated note in that phase doc's `Progress Notes` section
- update the status cell for Phase 2 in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`

If any shared report or manifest fields change, update Phase 1 and any later phase docs that depend on them.

## Tests and Checks

Run the relevant checks as you land each rubric milestone.

At minimum, run:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`

Add focused rubric tests or fixture-driven tests for:

- threshold semantics
- confidence handling
- human-calibration data loading
- hard-gate precedence over soft scores

If any UI surface or report rendering changes, also run:

- `npm --prefix ui run build`

## Deliverable

When the phase is complete, report:

- files touched
- rubric design decisions
- tests run
- any calibration caveats or follow-up work

