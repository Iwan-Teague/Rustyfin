CREATE TABLE IF NOT EXISTS channel (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK(kind IN ('text','voice')),
    position   INTEGER NOT NULL DEFAULT 0,
    is_private INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL,
    created_ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS channel_message (
    id         TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL,
    username   TEXT NOT NULL,
    content    TEXT NOT NULL,
    created_ts INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_channel_message_channel_ts
    ON channel_message(channel_id, created_ts);
