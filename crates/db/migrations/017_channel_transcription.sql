CREATE TABLE IF NOT EXISTS channel_transcript_session (
    id                TEXT PRIMARY KEY,
    channel_id        TEXT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    status            TEXT NOT NULL CHECK(status IN ('running','completed','cancelled','failed')),
    started_by_user_id TEXT NOT NULL,
    started_by_username TEXT NOT NULL,
    started_ts        INTEGER NOT NULL,
    ended_ts          INTEGER,
    output_path       TEXT,
    failure_reason    TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_channel_transcript_one_running
    ON channel_transcript_session(channel_id)
    WHERE status = 'running';

CREATE INDEX IF NOT EXISTS idx_channel_transcript_session_channel_started
    ON channel_transcript_session(channel_id, started_ts DESC);

CREATE TABLE IF NOT EXISTS channel_transcript_entry (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES channel_transcript_session(id) ON DELETE CASCADE,
    channel_id      TEXT NOT NULL REFERENCES channel(id) ON DELETE CASCADE,
    user_id         TEXT NOT NULL,
    username        TEXT NOT NULL,
    started_ts_ms   INTEGER NOT NULL,
    ended_ts_ms     INTEGER NOT NULL,
    text            TEXT NOT NULL,
    created_ts      INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_channel_transcript_entry_session_timeline
    ON channel_transcript_entry(session_id, started_ts_ms, created_ts);
