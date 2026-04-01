CREATE TABLE ai_conversation (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    last_message_preview TEXT,
    last_model_name TEXT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE INDEX idx_ai_conversation_user_updated
    ON ai_conversation (user_id, archived, updated_ts DESC);

CREATE TABLE ai_conversation_turn (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES ai_conversation(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    turn_index BIGINT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    model_name TEXT,
    grounding_tools_json TEXT NOT NULL,
    follow_up_contexts_json TEXT NOT NULL,
    grounding_sources_json TEXT NOT NULL,
    activity_trace_json TEXT NOT NULL,
    stats_json TEXT,
    trace_id TEXT,
    created_ts BIGINT NOT NULL
);

CREATE UNIQUE INDEX idx_ai_conversation_turn_conversation_turn_index
    ON ai_conversation_turn (conversation_id, turn_index);

CREATE INDEX idx_ai_conversation_turn_conversation_created
    ON ai_conversation_turn (conversation_id, created_ts ASC);
