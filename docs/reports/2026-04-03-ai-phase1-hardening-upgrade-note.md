# AI Phase 1 Hardening Upgrade Note

This change hardens Rustyfin's grounded `/ai` stack around planner validation, turn durability, prompt budgeting, deterministic rendering, and generated document verification.

## New Persistence Records

- `ai_assistant_turn_journal`
  - durable per-turn journal written before first token generation
  - stores request identity, response mode, planner mode, phase/status, planner debug JSON, prompt debug JSON, metrics JSON, overload reason, error message, compact-boundary count, artifact verification JSON, and finish timestamp
- `ai_conversation_compact_boundary`
  - durable memory checkpoint written when older conversation turns are compacted into persisted memory
  - stores conversation/user linkage, turn-index range, summarized turn count, and the memory snapshot JSON used for recovery
- `ai_generated_artifact`
  - now stores verification metadata:
    - `verification_status`
    - `verification_attempts`
    - `verification_notes_json`
    - `verified_ts`

## New Runtime / Debug Surfaces

- `/api/v1/system/ai/journals`
  - admin-only recent turn journals
- `/api/v1/system/ai/compact-boundaries`
  - admin-only recent compaction boundaries
- `/api/v1/ai/runtime`
  - now exposes richer prompt telemetry and the last turn's expanded stats, including planner validation counts, compaction counts, overload state, and artifact verification counts

## Planner Changes

- model planner output is now normalized through a typed schema shape with:
  - nested `arguments`
  - validation error capture
  - legacy-shape repair
  - validated call counts
- planner outputs that fail typed validation do not execute tools

## Prompt Budgeting / Profiles

- response-mode budgets are centralized in `crates/server/src/ai_assistant/profiles.rs`
- the same profile registry now drives:
  - planner sampling
  - answer sampling
  - memory summarization sampling
  - artifact verification sampling
- context budgeting now records:
  - effective context length
  - prompt budget
  - reserved completion tokens
  - summarized vs retained raw turn counts
  - compact-boundary counts

## Deterministic Renderer Expansion

The deterministic grounded reply path now also covers:

- account profile summary
- downloads catalog summaries
- service/backup/transcode/storage/recent-error summaries

This keeps stable read-only domains out of the freeform answer path when grounded structured data already succeeded.

## Generated Document Verification

Generated downloadable documents now go through:

1. initial grounded document generation
2. deterministic validation for obvious structural/content failures
3. grounded verifier pass
4. at most one repair pass
5. re-validation before persistence

If verification still fails, no artifact is saved.

Verification metadata is attached to the saved artifact record and to the turn journal/admin diagnostics surfaces.

## Failure Modes To Know

- invalid planner JSON
  - logged as planner validation errors
  - no tool call executes from the rejected plan
- overload
  - the turn is journaled with `status=overloaded`
  - the user gets a retry-later response before model generation
- model path / model load failures
  - the accepted turn remains journaled
  - the journal is updated to `failed` with an error message
- compaction recovery
  - if conversation memory is missing from the row but a compact boundary exists, Rustyfin rebuilds working memory from the latest boundary snapshot
- document verification failure
  - the generated artifact is not stored
  - the user receives a verification error instead of a broken download link

## Compatibility Notes

- `/api/v1/ai/chat` remains the compatibility path
- `/api/v1/ai/conversations/{id}/messages/stream` gets the full durability/compaction benefits because it persists accepted user turns before generation
- existing `/ai` UX remains compatible, with new admin/debug visibility for journals and compact boundaries
