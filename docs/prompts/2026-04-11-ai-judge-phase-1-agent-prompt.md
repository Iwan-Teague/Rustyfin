# Rustyfin AI Judge Phase 1 Agent Prompt

Date: 2026-04-11  
Scope: implement the deterministic judge foundation and make it reproducible, schema-checked, and fail-fast

## Read First

Read these files in order before changing anything:

1. `/Users/iwan/Desktop/Rustyfin/README.md`
2. `/Users/iwan/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwan/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`
5. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md`
6. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/mod.rs`
7. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/report.rs`
8. `/Users/iwan/Desktop/Rustyfin/crates/ai-evals/src/main.rs`

Use the phase doc as the implementation checklist and keep the umbrella plan status table in sync with it as you work.

## Objective

Implement the deterministic judge foundation for Rustyfin's AI eval harness.

This phase should:

- preserve the existing suite-based eval harness
- add a manifest-driven judge envelope
- add deterministic hard gates
- extend reports with stable replay metadata
- keep the output reproducible from fixtures

Do not add pairwise comparison or rubric scoring in this phase.

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Do not delete or weaken the existing suite evaluators.
- Hard gates must fail the run; they must not be averaged away.
- Do not add model-graded scoring in this phase.
- Keep the current `scripts/ci/debian_native_gates.sh` contract in mind if any judge output is intended for CI.
- Use the repo's existing docs and code style conventions.

## Work Plan

1. Inspect the current harness and report flow.
2. Add the shared run manifest and manifest digest fields.
3. Extend the report model so it can carry case-level verdicts and deterministic gate failures.
4. Add hard checks for schema validity, malformed output, length limits, refusal correctness, and ACL/privacy boundaries.
5. Add or extend fixture/schema validation for the current suite corpora.
6. Add tests that prove the same manifest produces the same verdicts.
7. Update the phase doc checklist and progress notes as each milestone lands.

## Progress Marking Rules

As you complete work, update all of the following:

- mark the matching checkbox in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md`
- add a short dated note in that phase doc's `Progress Notes` section
- update the status cell for Phase 1 in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`

If you discover a shared contract change, update dependent phase docs in the same change instead of leaving them inconsistent.

## Tests and Checks

Run the relevant checks at the point they become meaningful, not only at the end.

At minimum, run:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- `cargo test -p rustfin-server --test integration --no-run`

If any UI artifact or report-rendering path changes, also run:

- `npm --prefix ui run build`

If a test cannot run, state exactly why in your final summary.

## Deliverable

When the phase is complete, report:

- files touched
- core design decisions
- tests run
- anything deferred or blocked

