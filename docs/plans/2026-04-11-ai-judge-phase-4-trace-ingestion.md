# Rustyfin AI Judge Phase 4: Production Trace Ingestion and Calibration

Date: 2026-04-11  
Status: completed

Parent plan: [docs/plans/2026-04-07-ai-judge-improvement-plan.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md)  
Prompt: [docs/prompts/2026-04-11-ai-judge-phase-4-agent-prompt.md](/Users/iwan/Desktop/Rustyfin/docs/prompts/2026-04-11-ai-judge-phase-4-agent-prompt.md)  
Previous phase: [docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-3-pairwise-comparison.md)  
Next phase: [docs/plans/2026-04-11-ai-judge-phase-5-ci-enforcement.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-5-ci-enforcement.md)

## Purpose

Create the path that turns live assistant traces into curated judge cases without leaking private data or collapsing review boundaries. This phase should also make human calibration a first-class input to the judge lifecycle.

## Current Anchors

This phase should integrate with the existing report and dataset flow rather than inventing a parallel archive:

- `crates/server/src/ai_audit.rs`
- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_eval_harness/report.rs`
- `crates/server/src/ai_eval_harness/corpus.rs`
- `crates/server/src/ai_eval_harness/judge.rs`
- `tests/fixtures/ai/*.jsonl`

If a trace is imported, it must be curated before it becomes judge corpus material.

## Checklist

- [x] Define a trace import path that can serialize live cases into judge-ready rows
- [x] Redact or hash private user data, secrets, and sensitive identifiers before import
- [x] Keep the raw trace archive separate from the curated benchmark corpus
- [x] Store provenance fields such as `trace_id`, source type, access boundary, redaction state, reviewer, and review status
- [x] Require reviewer approval before a trace can become a release-relevant judge case
- [x] Add a human calibration queue for low-confidence or disputed cases
- [x] Record consensus labels for high-risk cases when practical
- [x] Add tests that prove private trace material never leaks into the curated corpus
- [x] Add tests that prove draft or unreconciled cases cannot be treated as release-ready

## Implementation Notes

- Treat trace import as a curation workflow, not a blind export.
- Align the import and review flow with the trace-to-dataset pattern used by observability/eval tools.
- Keep the access boundary explicit so the judge does not generalize private data into public test fixtures.
- Preserve the original raw archive so reviewers can audit what was redacted and why.

## Validation

Run the import and calibration checks together with the base judge suite:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- any import/redaction/calibration-specific tests added for the phase

If trace import touches UI or export surfaces, include:

- `npm --prefix ui run build`

## Exit Criteria

This phase is done when all of the following are true:

- production traces can be curated into judge cases safely
- redaction and access-boundary rules are explicit and enforced
- human review can approve or reject imported cases
- the curated corpus remains separate from the raw trace archive
- the judge can consume imported cases without treating them as unreviewed truth

## Progress Notes

Use this section for dated implementation notes while the phase is in progress.

- 2026-04-11: Added `trace_ingest.rs` with a two-step production-trace workflow: raw archive records retain the original audit/journal payloads, while curated trace cases hash identifiers, redact sensitive text, and serialize to a separate schema-validated JSONL corpus path.
- 2026-04-11: Added explicit provenance, access-boundary, sensitivity, review-status, redaction-state, reviewer-approval, and calibration-queue metadata plus release-readiness gating that blocks draft or unresolved production-trace cases from being treated as release-ready.
- 2026-04-11: Added fixture schema coverage and redaction/no-leak tests proving that raw archive files keep original trace material for audit while curated corpus rows do not leak emails, bearer tokens, URLs, or internal identifiers.
- 2026-04-11: Reconciled the Phase 4 draft against the live repo by aligning trace access-boundary inference to the current `AiGroundingVisibility` enum, loosening raw-archive derives to match audit/journal types, and adding a raw-archive reload helper so reviewers can inspect archived traces without promoting them into the curated corpus.
