ALTER TABLE ai_conversation
    ADD COLUMN group_name TEXT,
    ADD COLUMN sort_order BIGINT NOT NULL DEFAULT 0;

WITH ranked AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY user_id, archived
            ORDER BY updated_ts DESC, id DESC
        ) AS row_num
    FROM ai_conversation
)
UPDATE ai_conversation AS conversation
SET sort_order = ranked.row_num * 1024
FROM ranked
WHERE ranked.id = conversation.id;

DROP INDEX IF EXISTS idx_ai_conversation_user_updated;

CREATE INDEX idx_ai_conversation_user_group_order
    ON ai_conversation (user_id, archived, group_name, sort_order DESC, updated_ts DESC);
