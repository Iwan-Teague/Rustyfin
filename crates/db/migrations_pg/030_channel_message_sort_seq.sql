CREATE SEQUENCE IF NOT EXISTS channel_message_sort_seq_seq AS BIGINT;

ALTER TABLE channel_message
    ADD COLUMN IF NOT EXISTS sort_seq BIGINT;

ALTER TABLE channel_message
    ALTER COLUMN sort_seq SET DEFAULT nextval('channel_message_sort_seq_seq');

WITH ordered AS (
    SELECT id
    FROM channel_message
    WHERE sort_seq IS NULL
    ORDER BY created_ts, id
)
UPDATE channel_message AS m
SET sort_seq = nextval('channel_message_sort_seq_seq')
FROM ordered
WHERE m.id = ordered.id;

DO $$
DECLARE
    max_sort_seq BIGINT;
BEGIN
    SELECT MAX(sort_seq) INTO max_sort_seq FROM channel_message;

    IF max_sort_seq IS NULL THEN
        PERFORM setval('channel_message_sort_seq_seq', 1, false);
    ELSE
        PERFORM setval('channel_message_sort_seq_seq', max_sort_seq, true);
    END IF;
END $$;

ALTER TABLE channel_message
    ALTER COLUMN sort_seq SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_channel_message_sort_seq
    ON channel_message(sort_seq);

CREATE INDEX IF NOT EXISTS idx_channel_message_channel_sort_seq
    ON channel_message(channel_id, sort_seq DESC);
