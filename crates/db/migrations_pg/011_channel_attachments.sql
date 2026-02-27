CREATE TABLE IF NOT EXISTS channel_message_attachment (
    id           TEXT PRIMARY KEY,
    message_id   TEXT NOT NULL REFERENCES channel_message(id) ON DELETE CASCADE,
    channel_id   TEXT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL CHECK(size_bytes >= 0),
    storage_path TEXT NOT NULL,
    created_ts   INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_channel_attachment_message
    ON channel_message_attachment(message_id);

CREATE INDEX IF NOT EXISTS idx_channel_attachment_channel
    ON channel_message_attachment(channel_id);
