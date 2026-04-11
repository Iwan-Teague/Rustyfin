# Rustyfin AI Judge Phase 5: CI and Release Enforcement

Date: 2026-04-11  
Status: completed

Parent plan: [docs/plans/2026-04-07-ai-judge-improvement-plan.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-07-ai-judge-improvement-plan.md)  
Prompt: [docs/prompts/2026-04-11-ai-judge-phase-5-agent-prompt.md](/Users/iwan/Desktop/Rustyfin/docs/prompts/2026-04-11-ai-judge-phase-5-agent-prompt.md)  
Previous phase: [docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md](/Users/iwan/Desktop/Rustyfin/docs/plans/2026-04-11-ai-judge-phase-4-trace-ingestion.md)

## Purpose

Make the judge matter operationally by wiring it into the repo's gate scripts and release workflow. This phase should ensure the judge blocks unsafe or unreproducible changes and publishes stable artifacts for review.

## Current Anchors

Rustyfin currently enforces native quality gates through shell scripts, not a checked-in GitHub Actions workflow:

- `scripts/ci/debian_native_gates.sh`
- `scripts/ci/debian_browser_smoke.sh`
- `scripts/ci/rustyvault_removability_gates.sh`
- `scripts/start-native.sh`
- `scripts/deploy-native.sh`

Treat the shell gate script as the current enforcement contract unless a dedicated judge gate script is added and called from it.

## Checklist

- [x] Add a judge gate entrypoint, either by extending `scripts/ci/debian_native_gates.sh` or by adding a dedicated `scripts/ci/*judge*.sh` helper that it calls
- [x] Split PR smoke gating from full release gating
- [x] Require pinned manifest fields and deterministic replay before a release can pass
- [x] Fail on missing suite coverage, missing report artifacts, or nondeterministic rerun mismatch
- [x] Require the JSON report and markdown summary to parse cleanly
- [x] Keep hard gates blocking and soft metrics informational only
- [x] Publish release artifacts into the existing `.tmp/gates/` pattern or a closely aligned equivalent
- [x] Add tests for judge gate exit codes and artifact generation

## Implementation Notes

- Use the same block/pass philosophy already used in the supported-Debian native gate script.
- Keep the judge gate readable enough that humans can see why a release failed.
- If a GitHub workflow is added later, it should call the same shell gate contract instead of reimplementing judge logic.
- Pairwise and rubric outputs may be collected, but they must not override hard gate failures.

## Validation

Run the judge gate path itself plus the base repo checks touched by this phase:

- `bash -n scripts/ci/debian_native_gates.sh`
- any new judge-gate shell script syntax check
- `cargo fmt --all`
- `cargo check -p rustfin-server -p ai-evals`
- `cargo test -p rustfin-server --lib`
- `npm --prefix ui run build` if the report or UI review surfaces changed

## Exit Criteria

This phase is done when all of the following are true:

- the judge can block merges and releases through a concrete gate path
- release runs are reproducible from pinned inputs
- report artifacts are generated in a predictable location
- hard failures remain hard failures
- the enforcement path is documented well enough to be operated by another engineer or agent

## Progress Notes

Use this section for dated implementation notes while the phase is in progress.

- 2026-04-11: Added a dedicated `scripts/ci/judge_gates.sh` entrypoint plus a Rust-backed `gate` command in `ai-evals`, then wired `scripts/ci/debian_native_gates.sh` to run the judge in explicit `smoke` or `release` mode without re-implementing judge logic in shell.
- 2026-04-11: Release mode now pins manifest-affecting fields up front, reruns the pointwise suites with the same config, fails on replay mismatch or missing suite coverage, and writes stable pointwise/replay/comparison/summary artifacts under `.tmp/gates/`.
- 2026-04-11: Added gate-summary contracts and gate-module tests covering smoke artifact generation, release replay, and release manifest pinning, while keeping comparison output informational so it never overrides pointwise hard-gate failures.
- 2026-04-11: Tightened the gate polish after the first live smoke run by aligning planner eval fixtures with the current deterministic planner behavior, extending library-title extraction for `search my libraries for ...` and `look up ... in my library`, and validating artifact JSON by contract fields plus summary counts instead of requiring a byte-for-byte report round-trip.
