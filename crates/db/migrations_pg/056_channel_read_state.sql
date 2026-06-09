-- 056_channel_read_state.sql
-- Per-user channel read cursor for "new activity" / unread tracking.
-- Notes:
--   * Uses BIGINT epoch-ms timestamps to match the repo's current direction.
--   * last_read_sort_seq tracks the highest channel_message.sort_seq the user has seen;
--     unread = count(messages with sort_seq > last_read_sort_seq).
--   * Rows cascade-delete with their channel, so a deleted channel leaves no read state behind.

CREATE TABLE IF NOT EXISTS channel_read_state (
    user_id            TEXT NOT NULL,
    channel_id         TEXT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    last_read_sort_seq BIGINT NOT NULL DEFAULT 0,
    updated_ts         BIGINT NOT NULL,
    PRIMARY KEY (user_id, channel_id)
);

CREATE INDEX IF NOT EXISTS idx_channel_read_state_channel
    ON channel_read_state(channel_id);
