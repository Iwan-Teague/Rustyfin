# Runtime Diagnostics Counters Pass 2

## Scope

This pass extended the runtime diagnostics work with two concrete additions:

- rolling failure-rate windows for job families, agent calls, and transcode session creation
- a compact admin diagnostics panel backed by `/api/v1/system/runtime-diagnostics`

## Backend changes

### Rolling failure windows

Added rolling failure counters for:

- job families
- agent calls
- transcode creation failures

Each category now reports:

- failures in the last 1 minute
- failures in the last 5 minutes

This complements lifetime totals and makes transient spikes visible without requiring external metrics infrastructure.

### Diagnostics endpoint expansion

`GET /api/v1/system/runtime-diagnostics` now returns:

- job family totals and recent failure windows
- websocket connection activity
- agent call totals, in-flight counts, and recent failure windows
- transcoder session totals plus recent create-failure windows

## Admin UI changes

Added a compact diagnostics panel under `Admin -> Logs`.

The panel shows:

- runtime uptime
- active jobs and recent job failures
- transcoder active sessions and recent create failures
- websocket connection counts
- per-agent in-flight calls and recent failure windows

The panel reuses the existing admin page refresh path instead of creating a new polling loop.

## Why this matters

For a long-running home server deployment, totals alone are not enough. A rolling window answers questions like:

- Is the YouTube agent failing right now, or did it fail yesterday?
- Are transcode startups currently spiking failures?
- Is there an active job failure burst, or only historical noise?

That improves operational clarity without adding a heavy observability stack.

## Validation

Validated with:

- `cargo fmt --all`
- `cargo check -p rustfin-server -p rustfin-transcoder -p rustfin-db`
- `cargo test -p rustfin-server --lib`
- `cargo test -p rustfin-server runtime_metrics::tests --lib`
- `cargo test -p rustfin-transcoder --lib`
- `./ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json`
