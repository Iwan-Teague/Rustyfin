# AI Assistant Delta Phase 3-5 Execution Report

Date: 2026-04-01
Local branch: `ai-assistant-delta-phase3-5`
Implementation commit: `dd5197ed86e94c3daf3a932fe4c9d03da9bd5afe` (`Implement AI assistant phases 3-5`)
Implementation source of truth: `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`

## Scope Requested

Execute Phases 3, 4, and 5 of the AI assistant delta plan end-to-end:

- Phase 3: safe calendar writes with explicit confirmation
- Phase 4: voice input
- Phase 5: live runtime and GPU telemetry

Then push the final code, deploy it to the live Ubuntu host at `server@192.168.0.36`, verify the running system there, and report exact commands, results, deployment steps, final live commit, files changed, and remaining gaps.

## Completion Status

### Complete

#### Phase 3

- Added confirmation-token persistence in PostgreSQL through:
  - `crates/db/migrations_pg/045_ai_assistant_confirmation.sql`
  - `crates/db/src/repo/ai_assistant_confirmation.rs`
- Extended persisted assistant turns with `pending_action_json` so confirmation cards survive conversation reload.
- Added server-side confirmation parsing and token-gated execution in:
  - `crates/server/src/ai_assistant/confirmation.rs`
  - `crates/server/src/ai_enabled.rs`
- Added write-capable calendar assistant tools:
  - `calendar_create_event`
  - `calendar_create_birthday`
- Enforced write capability and explicit confirmation through registry metadata instead of documentation-only flags.
- Added read-after-write verification so the assistant only reports success when the created calendar entity is visible through the normal read path.
- Added deterministic birthday normalization:
  - personal scope by default
  - shared/global only when explicitly requested and permitted
  - yearly recurrence
  - preserved `birthday_year`

#### Phase 4

- Added authenticated `POST /api/v1/ai/transcribe`.
- Added browser-native voice input path in `/ai` using `SpeechRecognition` or `webkitSpeechRecognition` when available.
- Added server fallback upload path with:
  - multipart `file`
  - 10 MB request cap
  - 30 second duration cap
  - WAV decode or ffmpeg transcode to mono 16 kHz `s16le`
- Reused the existing transcription agent path rather than introducing a second transcription stack.
- Added transcript preview/edit before send plus explicit recording, transcribing, and error states in the `/ai` composer.

#### Phase 5

- Added authenticated `GET /api/v1/ai/runtime`.
- Exposed runtime status for:
  - model name
  - backend
  - context length
  - threads
  - GPU layers
  - loaded status
  - runtime phase
  - queue depth
  - active request count
  - process RSS
  - host CPU and RAM
  - NVIDIA GPU metrics when available
- Added runtime polling and a live runtime summary panel to `/ai`.

### Partial

- Phase 4 is implemented and locally validated, and the live route is reachable and authenticated correctly, but live fallback transcription did not complete on the Ubuntu host because the transcription runtime currently requires GPU OpenCL support and the host is missing `clinfo`.
- Local DB-backed integration execution was compiled but not run because `RUSTFIN_TEST_DATABASE_URL` and `RUSTFIN_DATABASE_URL` were not set locally.

### Deferred

- No additional product phases were deferred inside the requested Phase 3-5 slice.
- Host remediation for live fallback transcription remains an operational follow-up:
  - install or expose the expected OpenCL tooling such as `clinfo`, or
  - relax the transcription runtime requirement if CPU fallback is acceptable for this deployment

## Exact Local Commands And Results

### Validation Commands

```bash
cargo fmt --all
cargo check
cargo check -p rustfin-server --features ai
cargo test -p rustfin-server --features ai --lib
cargo test -p rustfin-server --features ai --test integration --no-run
npm --prefix ui run build
```

Results:

- `cargo fmt --all`: passed
- `cargo check`: passed
- `cargo check -p rustfin-server --features ai`: passed
- `cargo test -p rustfin-server --features ai --lib`: passed, `183` tests
- `cargo test -p rustfin-server --features ai --test integration --no-run`: passed
- `npm --prefix ui run build`: passed

Notes:

- `cargo check` emitted unrelated warnings from pre-existing local changes outside this work.
- DB-backed integration execution was not run because local database test env vars were unavailable.

### Git Commands

```bash
git switch -c ai-assistant-delta-phase3-5
git commit -m "Implement AI assistant phases 3-5"
git push -u origin ai-assistant-delta-phase3-5
```

Results:

- branch created successfully
- implementation commit created successfully as `dd5197ed86e94c3daf3a932fe4c9d03da9bd5afe`
- branch pushed successfully to `origin/ai-assistant-delta-phase3-5`

## Live Deployment

### Pre-Deploy Host State

Verified the active checkout and runtime on the Ubuntu host before deployment:

- checkout path: `/home/server/docker/Rustyfin`
- active branch: `main`
- active commit: `8a9f79f080300c498cad0a737194da62cee81679`
- `rustyfin-native.service`: active
- `rustfin-servers-agent.service`: active
- `rustyfin-post-healthcheck.service`: success

The live checkout already contained unrelated local modifications. They were preserved before branch switching with:

```bash
git stash push -m "pre-ai-assistant-delta-phase3-5-live-backup-2026-04-01"
```

Result:

- created `stash@{0}: On main: pre-ai-assistant-delta-phase3-5-live-backup-2026-04-01`

### Exact Deployment Commands

Switched the live checkout to the implementation branch:

```bash
git fetch origin ai-assistant-delta-phase3-5
git switch ai-assistant-delta-phase3-5 || git switch -c ai-assistant-delta-phase3-5 --track origin/ai-assistant-delta-phase3-5
```

Initial deploy attempt:

```bash
cd /home/server/docker/Rustyfin
./scripts/deploy-native.sh
```

Result:

- failed because the deploy path requires a TTY for `sudo -v`

TTY-backed deploy attempt:

```bash
cd /home/server/docker/Rustyfin
./scripts/deploy-native.sh
```

Result:

- failed during the `rustfin-server` rebuild with:
  - `error: could not find native static library 'cudart_static', perhaps an -L flag is missing?`

Successful deploy command:

```bash
cd /home/server/docker/Rustyfin
CUDA_PATH=/usr/lib/cuda RUSTFLAGS="-L/usr/lib/x86_64-linux-gnu -L/usr/lib/cuda/lib64" ./scripts/deploy-native.sh
```

Result:

- native artifacts rebuilt successfully
- native systemd units refreshed successfully
- live stack restarted successfully

Host-side CUDA linker config already present and confirmed:

- `/etc/environment.d/50-rustyfin-cuda.conf`
  - `CUDA_PATH=/usr/lib/cuda`
  - `RUSTFLAGS=-L/usr/lib/x86_64-linux-gnu -L/usr/lib/cuda/lib64`

## Live Verification

### Service And Checkout Verification

Verified after deploy:

- live checkout path: `/home/server/docker/Rustyfin`
- live branch: `ai-assistant-delta-phase3-5`
- live implementation commit: `dd5197ed86e94c3daf3a932fe4c9d03da9bd5afe`
- `rustyfin-native.service`: active
- `rustfin-servers-agent.service`: active
- `rustyfin-post-healthcheck.service`: success

Verified the active systemd unit now points at the updated checkout:

- `WorkingDirectory=/home/server/docker/Rustyfin`
- `ExecStart=/usr/bin/env bash /home/server/docker/Rustyfin/scripts/run-native-supervisor.sh`

### Edge And API Surface Checks

Commands:

```bash
curl -k -I https://127.0.0.1:3008/login
curl -k -I https://127.0.0.1:3008/ai
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8097/api/v1/ai/models
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8097/api/v1/ai/conversations
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:8097/api/v1/ai/transcribe
curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8097/api/v1/ai/runtime
```

Results:

- `/login`: `200`
- `/ai`: `200`
- `/api/v1/ai/models`: `401`
- `/api/v1/ai/conversations`: `401`
- `/api/v1/ai/transcribe`: `401`
- `/api/v1/ai/runtime`: `401`

An earlier operator check mistakenly used `GET /api/v1/ai/chat`, which correctly returned `405` because that route expects `POST`.

### Authenticated Live Verification

To verify the live system without reusing an existing account, a temporary test user was created directly in PostgreSQL, exercised through the real auth and AI endpoints, and removed after verification.

Verified outcomes:

- authenticated login succeeded
- `GET /api/v1/ai/runtime` returned runtime telemetry
- a birthday-create request emitted `confirmation_required`
- a follow-up `Confirm` with the returned token succeeded
- the conversation persisted with four messages
- the pending action status became `confirmed`
- the created birthday row was present in PostgreSQL with the correct normalized values

Captured verification result:

```json
{
  "login_role": "user",
  "runtime": {
    "model_name": null,
    "backend": "cuda",
    "loaded": false,
    "phase": "idle",
    "queue_depth": 0,
    "active_request_count": 0,
    "gpu_count": 2
  },
  "confirmation": {
    "token": "37b54a55-bc56-4c0e-b378-de8c3fb4fe21",
    "action_kind": "calendar_create_birthday",
    "summary": "Create recurring birthday for Rachel on June 9, 2003 in your personal calendar",
    "expires_ts": 1775086615
  },
  "first_stream_has_confirmation": true,
  "second_stream_has_success_text": true,
  "conversation_message_count": 4,
  "pending_action_status": "confirmed",
  "calendar_row": "Rachel birthday|birthday|yearly|2003|personal",
  "transcribe_status": 400,
  "transcribe_body": "{\"error\":{\"code\":\"bad_request\",\"message\":\"bad request: failed to start transcription session: bad request: transcription requires a GPU (opencl) and cannot run on CPU fallback: failed to run clinfo: No such file or directory (os error 2)\",\"details\":{}}}"
}
```

Interpretation:

- Phase 3 live confirmation-gated write flow is fully verified.
- Phase 5 live runtime telemetry is fully verified.
- Phase 4 live fallback endpoint is mounted and authenticated, but the host transcription runtime is currently misconfigured for actual transcription work.

### Post-Verification Cleanup

Removed the temporary verification user and confirmed the cascade cleanup removed the related AI and calendar artifacts:

```bash
sudo -u postgres psql rustfin -P pager=off -v ON_ERROR_STOP=1 \
  -c "DELETE FROM \"user\" WHERE username = 'ai_live_temp_20260401';" \
  -c "SELECT COUNT(*) AS remaining_users FROM \"user\" WHERE username = 'ai_live_temp_20260401';" \
  -c "SELECT COUNT(*) AS remaining_events FROM calendar_event WHERE title = 'Rachel birthday' AND owner_user_id = 'c8b0313b-d1a9-44f0-92e8-bc2235c1f111';" \
  -c "SELECT COUNT(*) AS remaining_conversations FROM ai_conversation WHERE user_id = 'c8b0313b-d1a9-44f0-92e8-bc2235c1f111';" \
  -c "SELECT COUNT(*) AS remaining_tokens FROM ai_assistant_confirmation_token WHERE user_id = 'c8b0313b-d1a9-44f0-92e8-bc2235c1f111';"
```

Results:

- temporary user removed
- remaining users: `0`
- remaining birthday events: `0`
- remaining conversations: `0`
- remaining confirmation tokens: `0`

## Changed Files In Scope

Implementation commit `dd5197ed86e94c3daf3a932fe4c9d03da9bd5afe` changed:

- `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
- `/Users/iwanteague/Desktop/Rustyfin/Cargo.lock`
- `/Users/iwanteague/Desktop/Rustyfin/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/crates/ai-agent/src/engine.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/045_ai_assistant_confirmation.sql`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/migrate.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/ai_assistant_confirmation.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/ai_conversations.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/mod.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/Cargo.toml`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/confirmation.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/context.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/mod.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_audit.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_conversations.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_transcribe.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/lib.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/tests/integration.rs`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts`

Documentation follow-up changed:

- `/Users/iwanteague/Desktop/Rustyfin/docs/README.md`
- `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-phase3-5-execution-report.md`

## Remaining Gaps

- Live `POST /api/v1/ai/transcribe` is not operational yet on this Ubuntu host because the transcription runtime cannot satisfy its current OpenCL GPU requirement.
- Full DB-backed integration execution still needs a local or CI PostgreSQL test database to run instead of compile-only coverage.
- Existing unrelated local worktree changes outside this slice were intentionally left untouched and uncommitted.

## Final Assessment

Phases 3, 4, and 5 are implemented.

What is fully complete:

- Phase 3 code and live verification
- Phase 5 code and live verification
- Phase 4 code, UI wiring, and local validation

What is only partial:

- Phase 4 live fallback transcription execution on the current Ubuntu host

Operationally, the live deployment succeeded, the branch tip code for the requested feature slice is on the server, the services are healthy, and the only material remaining issue is the host transcription runtime prerequisite for fallback STT.
