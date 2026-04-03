# AI Phase 4 Runtime Optimization Notes

This phase keeps the local GGUF path as the default and adds host-aware throughput controls around it.

## What Changed

- Added model benchmarking and profile persistence for host-specific recommendations.
- Added a turn scheduler with:
  - bounded queue depth
  - bounded warm-model pool size
  - overload classification
  - hot-model reuse when the same model is already warm
- Added an optional remote planner/backend configuration path behind the provider abstraction.
- Exposed benchmark, profile, scheduler, and remote-backend state in the Admin `AI` surface.

## Benchmark Model

The benchmark runner samples a small candidate set instead of trying to brute-force every possible layout:

- `current`
- `balanced`
- `cpu_safe` when the current profile uses GPU layers

Each candidate records:

- load duration
- prefill and decode timing
- first-token latency
- total duration
- tokens per second
- RSS before/after/peak memory usage

The persisted profile recommendation prefers the fastest successful candidate, then uses that evidence to derive:

- context window headroom
- preferred completion budget
- planner and summary output ceilings
- warmup cost class
- recommended thread / GPU-layer / split-mode layout

## Scheduler Behavior

The scheduler stays local-first and observable:

- queue admission is bounded
- warm-model memory is bounded by host memory budget
- overload state is reported as `normal`, `constrained`, `degraded`, or `overloaded`
- warm models are reused before reloading a model that is already hot
- remote planner routing is optional and only used when configured

## Verification

Executed during this phase:

- `cargo check -p rustfin-server -p rustfin-db -p rustfin-ai-agent --message-format short`
- `cargo check -p rustfin-server --features ai --message-format short`
- `cargo test -p rustfin-server --features ai --lib ai_assistant::scheduler::tests --message-format short`
- `cargo test -p rustfin-server --features ai --lib ai_benchmark::tests --message-format short`
- `cargo test -p rustfin-server --features ai --test integration --no-run --message-format short`

## Operational Note

The remote backend hook is intentionally optional. If it is not configured, the assistant keeps using the local inference path and the scheduler behavior stays bounded by the host's own memory and queue limits.
