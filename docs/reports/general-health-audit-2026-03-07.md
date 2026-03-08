# Rustyfin General Health Audit
Date: 2026-03-07  
Scope: backend security, database reliability/efficiency, and operational robustness.

## Executive Summary
Rustyfin is functionally strong and has shipped significant capability quickly, but there are still high-impact hardening opportunities.

The biggest risks are:
1. setup trust-boundary logic that can fail open in non-ideal deployment paths,
2. internal error detail leakage to clients,
3. schema/type drift caused by broad integer migration strategy.

The biggest efficiency wins are:
1. removing repeated N+1 access checks in streaming/authorization paths,
2. normalizing boolean columns and query typing,
3. reducing dynamic SQL generation for common `IN (...)` patterns.

## Method
- Code-path review of server, DB repo, migrations, agents, startup/runtime scripts.
- Hotspot scan for large files, repeated anti-patterns, and error handling.
- Focused command checks already run in this workspace:
  - `cargo fmt --all -- --check` (pass)
  - `cargo check -p rustfin-server` (pass)
  - `cargo test -p rustfin-server --lib` (pass)
  - `cargo clippy -p rustfin-server --lib -- -D warnings ...` (pass)
  - `npm --prefix ui run lint` (runs real lint; fails on existing lint debt)
  - `cargo check --workspace --all-targets --all-features` (fails on local host when `gpu-hip` toolchain (`hipcc`) is unavailable)

## Priority Findings

### P0. Setup trust boundary can fail open
Evidence:
- `crates/server/src/setup/guard.rs:28` returns local-only trust check.
- `crates/server/src/setup/guard.rs:38` defaults to `true` when remote/local cannot be determined.
- `crates/server/src/main.rs:395` serves without connect-info extraction wiring.
- `crates/server/src/setup/guard.rs:95` and `crates/server/src/setup/guard.rs:100` only check presence of `x-setup-remote-token`, not validity.

Risk:
- Setup write endpoints can be misclassified as local in environments where peer info is absent.
- Remote-gate behavior is weaker than intended.

Recommendation:
- Fail closed when origin cannot be proven local.
- Wire connection metadata explicitly and validate proxy headers only from `trusted_proxies`.
- Replace “header exists” remote token logic with cryptographically validated token/session binding.
- Add integration tests for local, trusted-proxy, and untrusted-proxy setup flows.

---

### P0. Internal error details are leaked to clients
Evidence:
- `crates/core/src/error.rs:31` defines `Internal(String)` and `crates/core/src/error.rs:135` emits `e.to_string()` to API clients.
- 256 call sites in `crates/server/src` currently map DB errors directly into `ApiError::Internal(format!("db error: {e}"))`.

Risk:
- SQL/schema internals and filesystem/runtime details leak into user-facing API responses.

Recommendation:
- Introduce a sanitizing mapper (`internal_error(code, safe_message, err)`).
- Log detailed cause server-side with correlation/request ID.
- Return stable, user-safe messages externally.

---

### P0. Auth hardening baseline is below production target
Evidence:
- `crates/server/src/user_pipeline.rs:10` password minimum is 6.
- `crates/server/src/routes.rs:258` login handler has no dedicated brute-force throttle.

Risk:
- Higher credential stuffing and brute-force exposure.

Recommendation:
- Increase password baseline (for example min 10-12, keep max cap).
- Add per-IP + per-username login throttling with backoff.
- Add failed-login telemetry and optional temporary lockout.

---

### P1. Migration strategy introduces schema drift risk
Evidence:
- `crates/db/migrations_pg/028_upgrade_integer_columns_to_bigint.sql:1` upgrades **all** integer columns indiscriminately.

Risk:
- Type drift and unexpected behavior in code paths that assume specific integer widths/semantics.
- Harder future migrations and debugging.

Recommendation:
- Replace broad migration pattern with targeted, explicitly enumerated column migrations.
- Add startup schema assertions for critical columns/types.
- Introduce a migration verification step in CI against a fresh Postgres instance.

---

### P1. Boolean-as-integer design still causes complexity and fragility
Evidence:
- `crates/db/src/repo/libraries.rs:3` encodes booleans as DB ints.
- 67 cast/boolean-coercion patterns across DB repos (`<> 0`, `CASE WHEN ... THEN 1 ELSE 0`, explicit bigint casts).

Risk:
- Repeated conversion logic, decoding mismatch incidents, and avoidable query noise.

Recommendation:
- Normalize DB flags to `BOOLEAN` columns.
- Remove manual int<->bool conversion helpers from repos.
- Use typed query rows without coercion expressions.

---

### P1. N+1 DB access on hot authorization/file-path validation paths
Evidence:
- `crates/server/src/streaming.rs:250` lists libraries then queries paths per library.
- `crates/server/src/streaming.rs:290` repeats per-library path lookups for user checks.
- `crates/server/src/user_pipeline.rs:63` validates library IDs by querying each ID one-by-one.
- `crates/db/src/repo/settings.rs:25` `get_many` loops with one query per key.

Risk:
- Unnecessary DB round trips and latency under load.

Recommendation:
- Use batch fetches (`IN/ANY`) and map in memory.
- Introduce cached canonicalized library roots with invalidation on library/path updates.
- Replace `settings::get_many` with a single query.

---

### P1. SSE event stream is global, not authorization-filtered
Evidence:
- `crates/server/src/routes.rs:3811` subscribes every authenticated user to a shared global broadcast stream.

Risk:
- Cross-library/workspace metadata visibility beyond least privilege.

Recommendation:
- Partition events by scope (user, library, room, admin).
- Filter event emission/subscription based on caller authorization.

---

### P1. Core server files remain very large and high-risk to modify
Evidence:
- `crates/server/src/routes.rs` ~3852 LOC
- `crates/server/src/watch_party/ws.rs` ~3832 LOC
- `crates/server/src/watch_party/handlers.rs` ~3350 LOC
- `crates/server/src/watch_party/manager.rs` ~2622 LOC

Risk:
- Regression risk, slower reviews, and merge conflict frequency.

Recommendation:
- Continue extracting by domain slices (authz, room lifecycle, queue ops, game ops, playback ops, SSE).
- Target sub-800 LOC modules with focused tests.

---

### P2. Observability is still minimal for production incident response
Evidence:
- Health endpoints exist, but there is no request ID middleware, no metrics endpoint, and no centralized HTTP tracing layer in server router.

Risk:
- Slow root-cause analysis for playback, transcription, and websocket incidents.

Recommendation:
- Add request IDs + structured access logs for API/WS.
- Add Prometheus metrics (DB latency, transcoder session counts, websocket connections, transcription queue depth).
- Add explicit redaction policy for logs containing user/media paths.

---

### P2. Workspace all-features builds are not host-portable
Evidence:
- `cargo check --workspace --all-targets --all-features` failed locally because HIP toolchain (`hipcc`) was unavailable while GPU HIP features were enabled.

Risk:
- Developer friction and inconsistent CI behavior across machines.

Recommendation:
- Split CI into feature-matrix jobs:
  - baseline CPU/OpenCL default,
  - CUDA job in CUDA-capable runner,
  - HIP job in ROCm-capable runner.
- Keep default contributor checks toolchain-portable.

---

### P2. Jobs table is dual-purposed as audit log storage
Evidence:
- `crates/server/src/audit_log.rs:6` writes audit events into `job` rows and immediately marks them completed.

Risk:
- Operational jobs and audit events are coupled; querying/retention semantics conflict.

Recommendation:
- Create dedicated `audit_event` table (actor, action, scope, payload, ts, request_id).
- Keep `job` table for task lifecycle only.
- Add retention/archival policy per table.

---

### P2. DB pool sizing and timeouts are hard-coded
Evidence:
- `crates/db/src/lib.rs:72` uses fixed `max_connections(15)` with no env tuning.

Risk:
- Under/over-provisioned DB connections across different hardware and workloads.

Recommendation:
- Add env-configurable DB pool parameters (`max_connections`, `min_connections`, `acquire_timeout`, statement timeout).
- Emit pool config at startup (sanitized).

## Suggested Execution Plan
1. **Week 1 (Security hardening):** fix setup trust boundary, sanitize error responses, add login throttling/password baseline update.
2. **Week 2 (DB stability):** replace broad migration strategy, normalize booleans, remove N+1 checks in streaming/auth paths.
3. **Week 3 (Operability):** add request IDs/metrics, split audit table, finalize CI feature matrix for GPU backends.

## Quick Wins (Low Risk, High Return)
- Replace `settings::get_many` N+1 with single query.
- Batch library ID existence validation in `user_pipeline`.
- Add safe internal error mapper and convert top 20 highest-traffic handlers first.
- Add startup warning when setup remote policy cannot determine peer address, then fail closed.
