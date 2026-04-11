# Rustyfin AI Judge Phase 5 Agent Prompt

Date: 2026-04-11  
Scope: wire the judge into CI and release enforcement so hard failures actually block merges

## Read First

Read these files in order before changing anything:

1. `/Users/iwan/Desktop/Rustyfin/README.md`
2. `/Users/iwan/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwan/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`
5. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-1-deterministic-foundation.md`
6. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-2-rubric-scoring.md`
7. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md`
8. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md`
9. `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-5-ci-enforcement.md`
10. `/Users/iwan/Desktop/Rustyfin/scripts/ci/debian_native_gates.sh`

Use the earlier phase docs as the source of truth for the judge contract itself.

## Objective

Make the judge enforceable through the repo's actual CI and release tooling.

This phase should:

- add or extend a judge-specific gate entrypoint
- separate PR smoke checks from full release checks
- require deterministic replay for release
- block releases on hard gate failures
- publish stable artifacts for review

## Non-Negotiable Constraints

- Keep backend logic in Rust and gate logic in shell only where the repo already uses shell for CI entrypoints.
- Do not invent a new CI contract if the existing `scripts/ci/debian_native_gates.sh` path can be extended cleanly.
- Hard failures must stay hard failures.
- Keep soft metrics informational.
- Do not make the judge gate opaque; humans must be able to read why a release failed.

## Work Plan

1. Inspect the current native gate script and identify the right insertion point.
2. Add a judge-specific gate path or extend the native gate script to call the judge.
3. Define PR smoke versus release behavior.
4. Require pinned manifest fields and deterministic replay for release.
5. Emit JSON and markdown artifacts in a stable location.
6. Add tests or shell checks for gate exit codes and artifact generation.
7. Update the phase doc checklist and umbrella plan status table.

## Progress Marking Rules

As you complete work, update all of the following:

- mark the matching checkbox in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-5-ci-enforcement.md`
- add a short dated note in that phase doc's `Progress Notes` section
- update the status cell for Phase 5 in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`

If the gate contract changes, update the earlier phase docs that describe shared manifest or report fields.

## Tests and Checks

At minimum, run:

- `bash -n /Users/iwan/Desktop/Rustyfin/scripts/ci/debian_native_gates.sh`
- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`

If the gate path or report rendering changes, also run:

- `npm --prefix ui run build`

If you add a dedicated judge gate script, syntax-check it too.

## Deliverable

When the phase is complete, report:

- files touched
- the gate contract you implemented
- tests and checks run
- any release-policy caveats or follow-up items

