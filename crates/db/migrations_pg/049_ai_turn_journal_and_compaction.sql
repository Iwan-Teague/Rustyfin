CREATE TABLE IF NOT EXISTS ai_assistant_turn_journal (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES ai_conversation(id) ON DELETE CASCADE,
    request_turn_id TEXT REFERENCES ai_conversation_turn(id) ON DELETE SET NULL,
    request_turn_index BIGINT,
    trace_id TEXT NOT NULL,
    request_message TEXT NOT NULL,
    model_name TEXT NOT NULL,
    response_mode TEXT NOT NULL,
    planner_mode TEXT,
    status TEXT NOT NULL,
    current_phase TEXT NOT NULL,
    history_len BIGINT NOT NULL DEFAULT 0,
    planner_debug_json TEXT NOT NULL DEFAULT '{}',
    prompt_debug_json TEXT,
    metrics_json TEXT,
    overload_reason TEXT,
    error_message TEXT,
    compact_boundary_count BIGINT NOT NULL DEFAULT 0,
    artifact_verification_json TEXT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL,
    finished_ts BIGINT
);

CREATE INDEX IF NOT EXISTS idx_ai_assistant_turn_journal_user_created_ts
    ON ai_assistant_turn_journal(user_id, created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_assistant_turn_journal_conversation_created_ts
    ON ai_assistant_turn_journal(conversation_id, created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_assistant_turn_journal_trace_id
    ON ai_assistant_turn_journal(trace_id);

CREATE TABLE IF NOT EXISTS ai_conversation_compact_boundary (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES ai_conversation(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    trace_id TEXT,
    from_turn_index BIGINT NOT NULL,
    to_turn_index BIGINT NOT NULL,
    summarized_turn_count BIGINT NOT NULL,
    memory_state_json TEXT NOT NULL,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_conversation_compact_boundary_conversation
    ON ai_conversation_compact_boundary(conversation_id, to_turn_index DESC);

ALTER TABLE ai_generated_artifact
    ADD COLUMN IF NOT EXISTS verification_status TEXT NOT NULL DEFAULT 'passed';

ALTER TABLE ai_generated_artifact
    ADD COLUMN IF NOT EXISTS verification_attempts INTEGER NOT NULL DEFAULT 1;

ALTER TABLE ai_generated_artifact
    ADD COLUMN IF NOT EXISTS verification_notes_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE ai_generated_artifact
    ADD COLUMN IF NOT EXISTS verified_ts BIGINT;
