# Rustyfin AI Judge Phase 3 Agent Prompt

Date: 2026-04-11  
Scope: implement pairwise comparison experiments without changing the default per-answer judge path

## Read First

Read these files in order before changing anything:

1. `/Users/iwan/Desktop/Rustyfin/README.md`
2. `/Users/iwan/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwan/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`
5. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md`
6. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md`
7. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md`
8. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/report.rs`
9. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/judge_rubric.rs`

Use the earlier phase docs to preserve the deterministic and rubric foundations.

## Objective

Add a dedicated comparison mode that can select the better of two candidate outputs for the same case.

This phase should:

- accept baseline and candidate responses separately
- keep pairwise logic out of the default verdict path
- reduce order bias
- emit comparison-specific reporting
- support prompt/model selection experiments

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Do not replace pointwise judge verdicts with pairwise selection.
- Do not let comparison results override hard failures.
- Keep comparison artifacts separate from regular judge artifacts.
- Keep the response-order bias problem explicit and testable.

## Work Plan

1. Add a comparison run mode and response model for baseline-vs-candidate input.
2. Implement response-order protection, such as flipping or randomization.
3. Add tie handling and explicit no-winner behavior where needed.
4. Keep select-best or max-score style aggregation isolated to comparison experiments.
5. Emit comparison report artifacts and summaries.
6. Add tests for winner stability, order flipping, and tie handling.
7. Update the phase doc checklist and umbrella plan status table.

## Progress Marking Rules

As you complete work, update all of the following:

- mark the matching checkbox in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md`
- add a short dated note in that phase doc's `Progress Notes` section
- update the status cell for Phase 3 in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`

If a shared report or manifest field changes, update the previous phase docs too.

## Tests and Checks

At minimum, run:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`

Add focused comparison tests for:

- order bias
- tie handling
- stable selection when inputs are swapped
- separate pointwise vs pairwise reporting

If any report artifact or UI surface changes, also run:

- `npm --prefix ui run build`

## Deliverable

When the phase is complete, report:

- files touched
- comparison strategy
- tests run
- any remaining bias or calibration risk

