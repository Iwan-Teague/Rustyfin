# Backend Security, Durability, and Performance Audit

Date: 2026-03-10
Runtime target: Debian 12 native
Scope: Rust backend/services, PostgreSQL access patterns, long-running job/runtime durability

## Summary

The project is already in a materially better state than a typical hobby server application:
- shared outbound HTTP client reuse is in place
- graceful shutdown for major background loops is in place
- PostgreSQL denormalized counters and sequenced channel message ordering are already implemented
- channel uploads/downloads are streamed instead of fully buffered in memory
- authentication and setup paths already have trust-boundary hardening and basic login throttling

The main remaining weaknesses are not broad architectural failures. They are concentrated in a smaller set of backend hot paths where long-running work can be duplicated, retried inconsistently, or reported unclearly.

## Findings

### P0. Duplicate long-running library jobs are still possible

Library scan and TMDB sync jobs can still be enqueued multiple times for the same library while earlier work is queued or running. This is a durability and efficiency problem rather than just a UI issue.

Current behavior:
- `library_scan` creates a new job every time the route is called
- TMDB sync only checks for `running`, not `queued`
- both rely on ad hoc payload matching rather than a typed repo helper

Impact:
- repeated scans/syncs waste I/O and CPU
- users see confusing duplicate jobs and stale progress signals
- repeated work increases wear on a long-running host

Planned fix:
- add repo helpers for active library jobs
- dedupe against both `queued` and `running`
- add a PostgreSQL expression index for `(kind, status, payload_json->>'library_id')`

### P0. Background job retry helpers are copy-pasted and panic-prone

Both `library_scan.rs` and `tmdb_sync.rs` contain their own `update_job_status_with_retry` helper. Both end with `last_err.expect(...)`.

Why this matters:
- the current logic probably never panics in the normal path, but it is still an unnecessary panic surface in background infrastructure
- the duplication makes future behavior drift more likely
- long-lived infrastructure should use one hardened retry implementation

Planned fix:
- extract a shared helper for job-status retry
- return a real fallback SQL error instead of panicking
- use a single retry/backoff policy in both job producers

### P1. Job orchestration logic is not centralized enough

The project has several places that create jobs and then hand-roll surrounding lifecycle behavior. This is not broken yet, but it increases maintenance cost and the chance of subtle divergence.

Short-term fix in this pass:
- centralize the retry path for job-state updates
- centralize active-library-job detection in the repo layer

Later follow-up:
- consider a shared job-runner utility for queued/running/completed/failed transitions and event emission

### P1. Some server/runtime paths still emphasize host detail over operational clarity

This is partly a product issue rather than a pure backend issue. The current server-management backend already exposes enough structure to support cleaner operational UI, and some of that work is already in progress locally.

Recommendation:
- keep runtime diagnostics available, but move verbose host details behind admin/diagnostic views by default
- keep user-facing server rows focused on desired state, observed state, health, and actionable controls

### P2. Remaining non-test `expect` / `unwrap` uses should keep shrinking

Most remaining `expect`/`unwrap` calls are in tests or in safe static initialization. That is acceptable. The remaining runtime-relevant ones should still be reduced over time.

Most relevant current examples:
- retry helpers in `library_scan.rs` and `tmdb_sync.rs`
- a few JSON/streaming helper paths elsewhere in the server

This pass will fix the highest-value ones first.

## Implementation plan for this pass

1. Add a shared backend helper for updating job status with retry and no panic fallback.
2. Add repository helpers for looking up active library jobs.
3. Add a PostgreSQL index to make active-library-job lookups cheap.
4. Change library scan and TMDB sync enqueue paths to reuse existing queued/running jobs instead of creating duplicates.
5. Validate with `cargo fmt`, `cargo check`, and targeted tests.

## Expected outcomes

After this pass:
- repeated scan/sync clicks should stop creating duplicate active jobs
- background job status updates will no longer rely on panic-prone retry fallbacks
- long-running library maintenance work will be cheaper and more predictable
- the project will be better suited for long-lived, unattended Debian operation

## Recommended next pass after this one

1. Build a small shared job runner abstraction for background workflows.
2. Audit remaining runtime `expect`/`unwrap` outside tests and remove the meaningful ones.
3. Add more explicit per-route idempotency for expensive admin actions where repeated clicks are common.
