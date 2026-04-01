CREATE TABLE ai_assistant_confirmation_token (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    action_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    expires_ts BIGINT NOT NULL,
    consumed_ts BIGINT
);

CREATE INDEX idx_ai_assistant_confirmation_user_expires
    ON ai_assistant_confirmation_token (user_id, expires_ts DESC);

ALTER TABLE ai_conversation_turn
    ADD COLUMN IF NOT EXISTS pending_action_json TEXT;
