CREATE TABLE IF NOT EXISTS ai_generated_artifact (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES ai_conversation(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    file_name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    content_text TEXT NOT NULL,
    byte_size BIGINT NOT NULL,
    trace_id TEXT,
    created_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_generated_artifact_user_created_ts
    ON ai_generated_artifact(user_id, created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_ai_generated_artifact_conversation
    ON ai_generated_artifact(conversation_id, created_ts DESC);
