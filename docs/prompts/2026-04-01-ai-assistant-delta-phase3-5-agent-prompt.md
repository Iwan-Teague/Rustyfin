# Rustyfin AI Assistant Remaining Phases Prompt

Date: 2026-04-01  
Scope: complete the remaining AI assistant delta phases, deploy them, and ensure the live Ubuntu server is on the newest correct version and running

## Read First

Before changing anything, read these files in order:

1. `/Users/iwanteague/Desktop/Rustyfin/README.md`
2. `/Users/iwanteague/Desktop/Rustyfin/AGENTS.md`
3. `/Users/iwanteague/Desktop/Rustyfin/CLAUDE.md`
4. `/Users/iwanteague/Desktop/Rustyfin/docs/plans/2026-03-15-ai-grounded-tools-architecture.md`
5. `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`
6. `/Users/iwanteague/Desktop/Rustyfin/docs/prompts/2026-04-01-ai-assistant-delta-phase0-2-agent-prompt.md`

Treat `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md` as the source of truth.

## Objective

Complete the remaining AI assistant delta phases after the initial Phase 0-2 slice:

- Phase 3: safe calendar writes and birthday support
- Phase 4: voice input
- Phase 5: live AI runtime and GPU telemetry

Then ensure the live Ubuntu server is updated to the newest correct code and is running healthily on that latest version.

Do not stop at planning. Implement the code, run the tests you can run, deploy the final result, and verify the live server.

## First Checkpoint

Before starting new code:

1. inspect what has already been implemented locally
2. inspect what has already been deployed on the Ubuntu server
3. compare the repo against the phase 3-5 requirements in:
   - `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`
4. explicitly identify:
   - done
   - partial
   - not started

If any Phase 0-2 prerequisites are missing and they block the remaining phases, fix the minimum needed prerequisite work first, but keep the main objective focused on Phases 3-5.

## Non-Negotiable Constraints

- Keep backend logic in Rust.
- Do not expose raw chain-of-thought as a product feature.
- Do not weaken server-side auth, tool policy, or confirmation requirements.
- Keep admin audit separate from user-visible chat persistence.
- Do not route Rustyfin calendar writes through vague model behavior. The backend must remain authoritative.
- Birthday creation must target Rustyfin calendar data, not a fake external-calendar path.
- The write flow must require explicit confirmation and read-after-write verification.
- Keep the Rustyfin visual identity intact.
- If you create a branch, do not use `codex` in the branch name.
- All commits must use:
  - `user.name = Iwan-Teague`
  - `user.email = teague.iwan@outlook.com`

## Ubuntu Server Access

Use the live Ubuntu server credentials provided out-of-band when you execute this prompt.

Known connection target:

- SSH target: `server@192.168.0.36`

Credential handling rules:

- use the credential only for this deployment session
- do not write it into repo files
- do not commit it

## Live Server Requirement

By the end of this task, the live Ubuntu server must be:

- running the newest correct version of the code
- updated to the final commit you produced for this work
- verified healthy after deployment

Do not leave the server on an old checkout or a partial deployment.

## Required Phase 3 Work

Implement safe AI calendar writes with explicit confirmation.

This phase must include:

1. confirmation-token storage and lifecycle
2. AI confirmation-required flow
3. write-capable calendar AI tools for:
   - general event creation if needed
   - birthday creation specifically
4. birthday mapping rules:
   - default scope personal unless explicitly shared
   - `event_type = birthday`
   - `recurrence = yearly`
   - `birthday_year` must be set
5. read-after-write verification
6. truthful success/failure behavior

The assistant must never say a birthday was created unless the backend created it and verified it.

## Required Phase 4 Work

Implement `/ai` speech-to-text input.

This phase must include:

1. browser-native STT path when supported
2. Rustyfin server transcription fallback path
3. microphone UI
4. transcript preview before send
5. explicit recording/transcribing/error states
6. authentication, size, and duration limits on the backend route

This is speech-to-text only for this slice. Do not expand scope into text-to-speech.

## Required Phase 5 Work

Implement live AI runtime and GPU telemetry on `/ai`.

This phase must include:

1. a curated AI runtime summary endpoint
2. active model/backend reporting
3. turn-phase and queue-depth reporting
4. host/process resource reporting relevant to AI
5. GPU reporting when supported by the host/backend
6. a runtime panel on `/ai`
7. graceful fallback when live GPU telemetry is unsupported

Do not expose the full admin runtime diagnostics surface to normal users. Expose a curated AI-specific runtime view.

## Concrete Repo Targets

At minimum, inspect and modify the appropriate subset of:

### Frontend

- `/Users/iwanteague/Desktop/Rustyfin/ui/src/app/ai/page.tsx`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/lib/aiApi.ts`
- `/Users/iwanteague/Desktop/Rustyfin/ui/src/features/ai-assistant/` if Phase 0-2 work already introduced that feature module

### Backend

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_enabled.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/orchestrator.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/registry.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/tools.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_assistant/types.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/runtime_diagnostics.rs`
- likely additions:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_runtime.rs`
  - `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/ai_voice.rs`

### Database

- add the next appropriate PostgreSQL migrations under:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/migrations_pg/`
- add matching DB repo support under:
  - `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/`

### Calendar

- `/Users/iwanteague/Desktop/Rustyfin/crates/calendar/src/main.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/db/src/repo/calendar.rs`

### Transcription

- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/transcription_agent.rs`
- `/Users/iwanteague/Desktop/Rustyfin/crates/server/src/channels/handlers.rs`

## Implementation Rules For Phase 3

- use explicit confirmation tokens, not prompt-only confirmations
- if a confirmation token is missing, expired, or already consumed, do not perform the write
- after creating a birthday, read it back through a normal visible read path
- verify the recurrence and birthday year before returning success
- if verification fails, return failure and do not imply success

## Implementation Rules For Phase 4

- prefer browser-native STT when available
- use server transcription fallback when browser-native STT is unavailable or fails
- the user must be able to inspect and edit the transcript before sending it
- do not silently retain raw audio by default

## Implementation Rules For Phase 5

- expose a curated `/ai` runtime contract to authenticated users
- include active model/backend/phase/queue information
- include process and host metrics relevant to AI
- include GPU metrics only when the host/backend can provide them reliably
- do not fabricate unsupported telemetry

## Testing Requirements

Run the strongest relevant test set you can support locally before deployment.

At minimum, run the appropriate subset of:

- `cargo fmt --all`
- `cargo check`
- `cargo test`
- `npm --prefix ui run build`

Add and run focused tests for:

- confirmation-token lifecycle
- calendar birthday creation and recurrence verification
- failure behavior when confirmation is missing or verification fails
- voice transcription route behavior and limits
- AI runtime telemetry contract and fallback behavior

If UI/E2E tests are practical, cover:

- birthday confirmation flow
- voice prompt capture/transcription UX
- runtime panel behavior during active prompts

If any test cannot be run, say exactly why.

## Deployment Requirements

After local verification:

1. push the final code to GitHub
2. confirm which checkout the live Ubuntu service is actually using
3. update that live checkout to the final commit
4. deploy using the host’s existing Rustyfin runtime path
5. verify the service is healthy after deployment
6. confirm the live server is actually running the newest correct commit

Do not stop at “pushed to GitHub.” The live Ubuntu server must end on the latest correct version and be running.

## Ubuntu Verification Requirements

After deployment, verify at least the following on the live server:

- the active service units are healthy
- the HTTPS UI responds
- the backend AI routes respond
- the active live checkout matches the final deployed commit
- `/ai` can use the new calendar confirmation/write flow
- `/ai` can use speech-to-text input or its server fallback
- `/ai` shows the runtime panel with model/backend information

If some parts cannot be fully browser-verified in the environment, get as close as possible with:

- authenticated API calls
- server logs
- runtime checks
- explicit documentation of what remains unverified

## Search Handles

Use these to move quickly:

- `ToolConfirmationPolicy`
- `assistant writes are disabled`
- `birthday_year`
- `recurrence`
- `confirmation`
- `getUserMedia`
- `MediaRecorder`
- `transcription_agent`
- `RUSTFIN_AI_GPU_BACKEND`
- `get_gpu_caps`
- `runtime_metrics`
- `current_model_dir`

## Final Reporting Requirements

When you finish, report:

1. what was implemented for Phase 3, Phase 4, and Phase 5
2. what, if anything, remained partial or blocked
3. all commands run and their results
4. all deployment commands run on the Ubuntu server
5. the final live commit running on the Ubuntu server
6. service health verification results
7. every file changed
8. any remaining gaps relative to `/Users/iwanteague/Desktop/Rustyfin/docs/reports/2026-04-01-ai-assistant-delta-plan.md`

## Final Instruction

Do not reply with only a plan. Verify the current state, implement the remaining phases, deploy the result, and leave the live Ubuntu server on the newest correct version and running.
