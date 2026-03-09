# Backend Rust Runtime Audit

Date: 2026-03-09

Scope:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server`
- supporting hot paths in `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder`
- supporting long-running service behavior in `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent`

Goal:
- identify backend Rust changes that improve long-running stability, latency, and operational stamina
- avoid durability or security regressions
- prefer low-risk changes first

## Summary

The backend is structurally sound for a long-running Debian 12 service. The main risks are not memory-unsafe code or obvious security faults. The main risks are operational:

- avoidable blocking work on async request paths
- repeated short-lived HTTP client construction
- background tasks that do not have explicit shutdown coordination
- a few hot in-memory structures that were more expensive than they needed to be under sustained load

This pass implemented four low-risk improvements immediately:

1. setup rate-limiter buckets now prune in O(1) front-pop style and stale buckets are swept periodically
2. library artwork cache I/O no longer uses synchronous filesystem calls on the async request path
3. setup path validation no longer performs direct blocking filesystem probes on the async runtime
4. active watch-party room eviction is now least-recently-active instead of arbitrary

## Implemented Now

### 1. Setup rate limiter cleanup cost reduced

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/rate_limit.rs:20`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/rate_limit.rs:44`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/rate_limit.rs:93`

Change:
- replaced per-key `Vec<Instant>` storage with `VecDeque<Instant>`
- added `last_seen` and periodic stale-bucket sweeping
- kept the same external behavior while reducing repeated retain scans

Why it matters:
- setup traffic is not the hottest path in the system, but rate-limit state should still age out cleanly on a server that runs for months
- the old layout could accumulate cold buckets longer than necessary and paid more pruning cost per check than needed

### 2. Artwork cache I/O moved onto async filesystem APIs

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:3087`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:3101`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:3143`

Change:
- switched image cache create/read/write/copy/metadata calls to `tokio::fs`

Why it matters:
- poster and backdrop requests happen on normal user browsing flows
- synchronous disk calls inside async handlers increase tail latency under concurrent browsing

### 3. Setup path validation moved off the async reactor

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/handlers.rs:559`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/setup/handlers.rs:608`

Change:
- wrapped existence/readability/writability probing in `tokio::task::spawn_blocking`

Why it matters:
- this path deliberately touches real filesystem permissions and write probes
- even though setup is not a hot path, it is still better to keep blocking directory probes off the main async runtime

### 4. Watch-party runtime eviction is now activity-aware

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/manager.rs:1920`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/manager.rs:2392`

Change:
- when the in-memory room runtime cap is reached, Rustyfin now evicts the least recently active room instead of whichever map entry happened to be visited first

Why it matters:
- arbitrary eviction is cheap but wrong
- under sustained room usage it increases cache churn and makes the system feel unstable for active rooms

## Highest-Value Next Passes

### P1. Share long-lived `reqwest::Client` instances

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/state.rs:31`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:3107`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/transcription_agent.rs:92`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/agent_client.rs:61`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/youtube.rs:495`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/youtube.rs:603`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:606`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:2520`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/handlers.rs:2745`
- `/Users/iwanteague/Desktop/Rustyfin/crates/tmdb-agent/src/main.rs:697`
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/src/main.rs:853`

Recommendation:
- add shared `reqwest::Client` instances to application/service state
- centralize timeouts, connection pooling, idle reuse, and headers per downstream service

Why:
- `reqwest::Client` is designed to be reused
- repeated `Client::new()` or ad hoc builder creation discards pooling benefits and adds avoidable connection churn
- this is a real win for long-running services that repeatedly call TMDB, YouTube, the servers agent, or the transcription agent

Risk:
- low
- the only care point is preserving different timeout requirements where needed

### P1. Add explicit shutdown coordination for spawned background loops

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs:310`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs:333`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs:373`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/main.rs:386`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/library_scan.rs:25`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/tmdb_sync.rs:68`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/watch_party/ws.rs:3350`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/handlers.rs:1349`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/handlers.rs:1454`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/servers/handlers.rs:1556`

Recommendation:
- introduce a shared shutdown token or `CancellationToken`
- make long-lived loops observe it and exit cleanly
- join critical tasks during shutdown where possible

Why:
- this matters more on native Debian than in short-lived dev runs
- clean shutdown reduces partial work, noisy logs, and odd restart-time races

Risk:
- moderate
- touches application wiring, but worth doing because the repo is now explicitly native long-running infrastructure

### P1. Fix the current Clippy blocker by reducing transcode function fan-in

File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder/src/session.rs:307`

Finding:
- `cargo clippy` is currently blocked by `spawn_ffmpeg` having too many arguments

Recommendation:
- replace the wide parameter list with a small config struct, for example `SpawnFfmpegOptions`

Why:
- this is not just style
- wide argument lists are easier to misuse and harder to evolve safely in a long-lived codebase

Risk:
- low
- local refactor, but it touches a critical playback path so it should be done carefully and tested

## Second-Tier Improvements

### P2. Review transcription worker registry behavior under churn

Files:
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/src/main.rs:122`
- `/Users/iwanteague/Desktop/Rustyfin/crates/transcription-agent/src/main.rs:862`

Finding:
- worker ownership is kept in a single `Arc<Mutex<HashMap<String, WorkerHandle>>>`
- that is acceptable at the current scale, but it is the right place to watch for contention if transcription sessions become more common

Recommendation:
- keep the current design for now
- if transcription concurrency grows, move worker lifecycle to an actor-style manager or sharded map with explicit idle reap

Why:
- this is not yet a proven bottleneck
- changing it now would be speculative compared with the P1 work above

### P2. Consider streaming image responses instead of fully buffering them

File:
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/routes.rs:3146`

Finding:
- the artwork path now uses async filesystem APIs, but still reads the whole cached image into memory before responding

Recommendation:
- leave this as-is for now
- revisit only if image-heavy browsing turns into a measurable memory hotspot

Why:
- posters and thumbnails are usually small enough that the complexity tradeoff is not yet justified

## What I Did Not Find

- no obvious unsafe Rust usage problem on the audited backend paths
- no clear evidence that PostgreSQL access patterns in the audited Rust backend are broadly unstable after the recent denormalization and sequencing passes
- no obvious long-session memory leak in the newly audited paths, though shutdown coordination and shared HTTP clients would still materially improve stamina

## Validation

This pass was validated with:

- `cargo fmt --all`
- `cargo check -p rustfin-server`
- `cargo test -p rustfin-server --lib`

Note:
- `cargo clippy` still reports the pre-existing `too_many_arguments` issue in `/Users/iwanteague/Desktop/Rustyfin/crates/transcoder/src/session.rs:307`

## Recommended Next Order

1. add shared `reqwest::Client` instances to backend/service state
2. add explicit shutdown coordination for spawned background loops
3. refactor `spawn_ffmpeg` behind a config struct and re-enable a clean `cargo clippy` pass

