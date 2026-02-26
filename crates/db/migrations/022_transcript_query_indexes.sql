-- Sixth-pass transcript query/index optimization.

-- Running-session listings and active-session checks:
--   WHERE status = 'running' ORDER BY started_ts
CREATE INDEX IF NOT EXISTS idx_channel_transcript_session_status_started
    ON channel_transcript_session(status, started_ts DESC);

-- Session-level transcript entry count/group paths:
--   WHERE session_id IN (...) GROUP BY session_id
CREATE INDEX IF NOT EXISTS idx_channel_transcript_entry_session
    ON channel_transcript_entry(session_id);
