# Rustfin Documentation Library (Extreme Expansion)
Generated: 2026-02-13

This folder is a *spec library* for building a Jellyfin-class media server in a smaller, Rust-first stack:
- **Rust server** (single binary, modular internals)
- **One UI app** (choose *either* Leptos/WASM *or* Next.js)
- **FFmpeg** is the only “codec brain” (probe/remux/transcode/thumbs/subs)
- **SQLite** as the local-first DB (no external infra)

## What you get in this bundle

### Specs you build from (new / expanded)
1) **01_ARCHITECTURE_OVERVIEW.md** — full system decomposition + runtime flows  
2) **02_UI_UX_SPEC.md** — mobile-first UX contract + screen-by-screen behavior  
3) **03_THEME_STYLE_MOTION.md** — tokens, theming, styling approach, motion + a11y rules  
4) **04_API_SPEC.md** — full REST surface + streaming endpoints (Range + HLS) + eventing  
5) **05_DATABASE_SPEC.md** — schema, indices, migrations, consistency, job queue  
6) **06_BACKEND_REST_IMPLEMENTATION.md** — Axum code patterns + concrete handler skeletons  
7) **07_METADATA_SUBTITLES_ARTWORK_PROVIDERS.md** — providers, matching, artwork rules, subs  
8) **08_STREAMING_TRANSCODING_GPU.md** — playback decision engine + HLS authoring + GPU  
9) **09_DOCKER_OPS_BUILD_RELEASE.md** — Dockerfile/compose + volumes + GPU runtime + backups  
10) **10_PROJECT_PLAN_AND_MILESTONES.md** — build plan, acceptance criteria, risks  
11) **11_TESTING_OBSERVABILITY_SECURITY.md** — tests, perf targets, logs/metrics, threat model

### Unedited originals (exact copies)
- `sources_original/jellyfin_deep_dive.md`
- `sources_original/rustfin_build_blueprint.md`
- `sources_original/rustfin_master_spec_extreme_detail.md`
- `sources_original/rustfin_master_spec_extreme_detail_v2_tv_seasons_missing_docker.md`

## Guiding rules (prevents roadblocks)
1) **Two-language cap**: Rust server + (Rust UI OR TypeScript UI). No microservice zoo.  
2) **DB-first configuration**: UI edits config stored in SQLite; minimal config files.  
3) **Determinism beats cleverness**: provider IDs when available; heuristics only as fallback.  
4) **Streaming correctness is a spec problem**: implement Range + HLS correctly and players behave.

## Reading order
Architecture → Database → API → Backend → Streaming → Metadata → UI → Theme → Docker/Ops → Testing.
