ALTER TABLE ai_conversation
    ADD COLUMN memory_state_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE ai_conversation
    ADD COLUMN memory_turn_index BIGINT NOT NULL DEFAULT -1;

ALTER TABLE ai_conversation
    ADD COLUMN memory_updated_ts BIGINT;
