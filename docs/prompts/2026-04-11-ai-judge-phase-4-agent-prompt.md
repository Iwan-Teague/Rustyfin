# Rustyfin AI Judge Phase 4 Agent Prompt

Date: 2026-04-11  
Scope: turn production traces into curated judge cases with explicit redaction and review

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
9. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_audit.rs`
10. `/Users/iwan/Desktop/Rustyfin/crates/server/src/ai_eval_harness/corpus.rs`

Use the earlier phase docs as the shared contract for manifest and reporting fields.

## Objective

Build the ingestion and calibration path that converts live assistant traces into judge cases safely.

This phase should:

- redact or hash private data before import
- keep raw traces separate from curated benchmark material
- store provenance and access-boundary metadata
- require human approval for release-relevant traces
- support calibration queues and consensus labels

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Do not make private traces silently enter the judge corpus.
- Do not collapse raw archive storage and curated benchmark storage into one bucket.
- Do not treat imported traces as release-ready until reviewed.
- Preserve the access boundary explicitly.

## Work Plan

1. Define the trace import and curation flow.
2. Add redaction/hash helpers for sensitive fields.
3. Separate raw archive storage from curated judge cases.
4. Add reviewer, review-status, and access-boundary metadata.
5. Add calibration queue and consensus-label handling.
6. Add tests that prove private material does not leak into the curated corpus.
7. Update the phase doc checklist and umbrella plan status table.

## Progress Marking Rules

As you complete work, update all of the following:

- mark the matching checkbox in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md`
- add a short dated note in that phase doc's `Progress Notes` section
- update the status cell for Phase 4 in `/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md`

If trace provenance changes, update earlier phase docs that rely on those manifest fields.

## Tests and Checks

At minimum, run:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`

Add focused tests for:

- redaction and hashing
- approval gating
- raw-vs-curated separation
- no-leak guarantees for trace-derived corpus rows

If any UI or export surface changes, also run:

- `npm --prefix ui run build`

## Deliverable

When the phase is complete, report:

- files touched
- redaction and review design
- tests run
- any open calibration questions

