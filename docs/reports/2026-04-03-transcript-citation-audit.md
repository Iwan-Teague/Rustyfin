# Transcript Citation Audit

Scope: transcript persistence, downloadable markdown output, and the transcript metadata that can support citation backlinks in the AI assistant.

## Key Files

- `crates/db/migrations_pg/017_channel_transcription.sql`
- `crates/db/migrations_pg/031_denormalized_counters.sql`
- `crates/db/migrations_pg/022_transcript_query_indexes.sql`
- `crates/db/src/repo/channel_transcripts.rs`
- `crates/server/src/channels/router.rs`
- `crates/server/src/channels/handlers.rs`
- `crates/server/src/ai_assistant/tools.rs`
- `crates/server/src/ai_assistant/orchestrator.rs`
- `crates/server/src/ai_assistant/types.rs`
- `crates/server/src/ai_enabled.rs`
- `crates/server/src/ai_conversations.rs`
- `crates/db/src/repo/ai_conversations.rs`
- `crates/db/migrations_pg/044_ai_conversations.sql`

## Exact Persisted Fields

### `channel_transcript_session`

Persisted columns:

- `id`
- `channel_id`
- `status`
- `started_by_user_id`
- `started_by_username`
- `started_ts`
- `ended_ts`
- `output_path`
- `failure_reason`
- `entry_count` from the later denormalized counter migration

Notes:

- `started_by_*` identifies who started transcription, not the speaker in the transcript.
- `output_path` is the server-local markdown file path that is written after finalization.

### `channel_transcript_entry`

Persisted columns:

- `id`
- `session_id`
- `channel_id`
- `user_id`
- `username`
- `started_ts_ms`
- `ended_ts_ms`
- `text`
- `created_ts`

Notes:

- `user_id` is the stable speaker identity.
- `username` is a snapshot label and can drift over time.
- The query order is by `started_ts_ms ASC, created_ts ASC`, so there is no explicit persisted entry index.

### `ai_conversation_turn`

Current persisted assistant-turn columns are:

- `id`
- `conversation_id`
- `user_id`
- `turn_index`
- `role`
- `content`
- `model_name`
- `grounding_tools_json`
- `follow_up_contexts_json`
- `grounding_sources_json`
- `activity_trace_json`
- `stats_json`
- `pending_action_json`
- `trace_id`
- `created_ts`

Important gap:

- There is no citation-specific field for transcript windows, entry IDs, or markdown backlinks today.

### Downloadable Markdown Output

The transcript file is written to `state.cache_dir/channel_transcripts/{channel_id}/{session_id}.md` and served as `voice-transcript-{session_id}.md` with `text/markdown; charset=utf-8`.

Header fields in the markdown:

- `Channel`
- `Channel ID`
- `Session ID`
- `Started`
- `Ended`

Per-line transcript format:

- Relative start and end timestamps computed from the session start
- Speaker username
- Transcript text

Example shape:

- `[00:00:01.500 - 00:00:02.000] alice: hello there`

## Where AI Transcript Summaries Are Built

Transcript summaries are assembled in `crates/server/src/ai_assistant/tools.rs`:

- `channels_get_transcript_summary` chooses the latest completed transcript session for an accessible voice channel.
- `summarize_transcript_session` builds the summary object.
- `transcript_highlights` selects compact highlight snippets.
- `transcript_excerpt` builds the sampled excerpt block.
- `transcript_excerpt_indexes` picks early, middle, and late lines for the excerpt sample.

The summary object currently contains:

- `channel_id`
- `channel_name`
- `session_id`
- `started_ts`
- `ended_ts`
- `duration_seconds`
- `started_by_username`
- `entry_count`
- `speaker_count`
- `top_terms`
- `speakers[]` with `username`, `segment_count`, `word_count`, `approx_spoken_seconds`
- `highlights[]` with `username`, `started_ts_ms`, `ended_ts_ms`, `relative_start`, `relative_end`, `text`
- `transcript_excerpt`
- `transcript_excerpt_truncated`

Current follow-up metadata:

- `build_follow_up_context` only preserves a channel-level backlink for transcript summaries.
- The transcript summary follow-up entity carries `channel_name` plus `channel_id`.
- `recent_transcript_query_hint` can reuse the prior `channels_query` or the follow-up entity label.

What is missing:

- No stored `session_id` or `entry_id` backlink in the follow-up entity graph.
- No persisted excerpt window object beyond the sampled excerpt string and highlight snippets.
- No raw markdown anchor or line anchor per transcript entry.

## Recommended Citation Model

Use a dedicated transcript citation object instead of overloading `grounding_sources_json`.

Recommended fields:

- `citation_id`
- `channel_id`
- `session_id`
- `entry_id` or `start_entry_id` / `end_entry_id`
- `speaker_user_id`
- `speaker_username`
- `started_ts_ms`
- `ended_ts_ms`
- `excerpt`
- `markdown_anchor`
- `source_kind` set to transcript

Recommended behavior:

- Persist citation objects on assistant turns in a dedicated JSON field or table.
- Keep `follow_up_contexts` for coarse channel/session reuse, but add transcript-specific IDs for exact resolution.
- Render a stable anchor for each transcript entry in the markdown export so citations can deep-link to the exact excerpt window.
- Treat `entry_id` as the primary durable citation key, not sampled line index positions.

For exact excerpt windows, the current data only supports segment-level precision. If word-level precision is required later, the transcription pipeline will need to persist finer-grained offsets.

## Risks and ACL Concerns

- Private voice channels are excluded from `channels_get_transcript_summary` unless the caller is admin.
- `download_transcription` uses channel access control, so citations exposed in the UI or assistant history must honor the same ACL boundary.
- `output_path` is a server-local filesystem path; it should remain internal and never be treated as a client-facing citation target.
- Markdown export and summary excerpts normalize and truncate text, so they should not be treated as the canonical raw transcript payload.
- Transcript summary excerpts are compact and sampled; they are not exhaustive citation coverage.
- `username` is not a stable identity key; use `user_id` for durable citations.
- Deleting a transcript session removes the session row and cascades the entries, so any persisted citation backlinks must degrade cleanly when the source transcript disappears.
- Existing tests cover relative timestamp formatting and excerpt sampling, but there is no backlink or citation-anchor test yet.
