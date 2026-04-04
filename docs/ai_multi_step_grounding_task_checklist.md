# AI Multi-Step Grounding Task Checklist

## Phase 0 — Preparation and Alignment

- [x] Read current `README.md`, `AGENTS.md`, and `CLAUDE.md` before modifying architecture.
- [x] Read the authoritative delta document.
- [x] Copy the delta into the repo at `docs/ai_multi_step_grounding_delta.md`.
- [x] Confirm the live repo still matches the delta closely enough to proceed.
- [x] Record any repo drift in the implementation log.
- [x] Summarize the implementation plan in the implementation log.

## Phase 1 — Introduce Typed Outcomes, Budgets, and Executor Scaffolding

- [x] Add typed semantic outcome model for tool execution.
- [x] Add explicit mode budgets for `Instant`, `Thinking`, and `Extended`.
- [x] Introduce a bounded executor layer above planner and tool registry.
- [x] Keep existing planner AST/repair path; do not replace it wholesale.
- [x] Keep `ToolProvider`/`ToolRegistry`; extend metadata instead of rebuilding it.
- [x] Split raw tool execution from semantic outcome normalization.
- [x] Ensure every turn records an explicit stop reason.
- [x] Keep confirmation/write gate behavior unchanged.
- [x] Add Phase 1 unit/integration tests.
- [x] Update implementation log with Phase 1 validation.

## Phase 2 — Add Domain-Family Fallback Graphs and Bounded Recovery

- [x] Add domain-family fallback graph registry.
- [x] Implement duplicate-signature and loop-prevention logic.
- [x] Keep retries read-only and ACL-preserving.
- [x] Exclude write/protected tools from automatic recovery.
- [x] Add bounded recovery across representative domains.
- [x] Keep evidence compact and prompt-budget aware across attempts.
- [x] Add Phase 2 recovery/ACL/loop tests.
- [x] Update implementation log with Phase 2 validation.

## Phase 3 — Richer Synthesis, Role Usage, and Diagnostics

- [x] Extend deterministic reply/synthesis to consume multi-step evidence.
- [x] Wire actual role-bound backend usage only where budget justifies it.
- [x] Persist compact execution traces with stop reasons.
- [x] Extend runtime/admin diagnostics with attempt and stop-reason visibility.
- [x] Add SSE/runtime trace events for execution-loop observability.
- [x] Add synthesis and role-routing integration tests.
- [x] Update implementation log with Phase 3 validation.

## Phase 4 — Eval Harness, Rollout Hardening, Deployment

- [x] Add unit tests for outcome normalization and graph selection.
- [x] Add integration tests for multi-step recovery traces.
- [x] Add eval corpus for empty/ambiguous/partial/conflict/ACL/write safety cases.
- [ ] Verify `Instant` remains narrow and low-latency.
- [ ] Verify `Thinking` improves bounded recovery quality.
- [ ] Verify `Extended` remains hard-capped and observable.
- [ ] Verify no chain-of-thought exposure.
- [ ] Verify no ACL regressions.
- [ ] Verify no write-confirmation regressions.
- [ ] Run full relevant validation suite and record results.
- [ ] Deploy to Ubuntu.
- [ ] Verify service health, AI/runtime health, admin diagnostics, stop reasons, and recovery telemetry live.
- [ ] Update implementation log with deployment verification.
- [ ] Commit the completed work.
- [ ] Push to `main`.

## Final Checklist

- [x] Read current `README.md`, `AGENTS.md`, and `CLAUDE.md` before modifying architecture.
- [x] Add typed semantic outcome model for tool execution.
- [x] Add explicit mode budgets for `Instant`, `Thinking`, and `Extended`.
- [x] Introduce a bounded executor layer above the existing planner and tool registry.
- [x] Keep existing planner AST/repair path; do not replace it wholesale.
- [x] Keep `ToolProvider`/`ToolRegistry`; extend metadata instead of rebuilding it.
- [x] Split raw tool execution from semantic outcome normalization.
- [x] Add domain-family fallback graph registry.
- [x] Implement duplicate-signature and loop-prevention logic.
- [x] Keep retries read-only and ACL-preserving.
- [x] Exclude write/protected tools from automatic recovery.
- [x] Extend deterministic reply/synthesis to consume multi-step evidence.
- [x] Keep evidence compact and prompt-budget aware.
- [x] Persist compact execution traces with stop reasons.
- [x] Extend admin/runtime diagnostics with attempt and stop-reason visibility.
- [x] Wire actual role-bound backend usage only where budget justifies it.
- [x] Add unit tests for outcome normalization and graph selection.
- [x] Add integration tests for multi-step recovery traces.
- [x] Add eval corpus for empty/ambiguous/partial/conflict/ACL/write safety cases.
- [ ] Verify `Instant` remains narrow and low-latency.
- [ ] Verify `Thinking` improves bounded recovery quality.
- [ ] Verify `Extended` remains hard-capped and observable.
- [ ] Verify no chain-of-thought exposure.
- [ ] Verify no ACL regressions.
- [ ] Verify no write-confirmation regressions.

## Completion

- [ ] All phases completed
- [ ] Deployment verified
- [ ] Pushed to `main`
