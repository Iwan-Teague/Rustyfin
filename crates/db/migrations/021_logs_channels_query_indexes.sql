-- Fifth-pass query/index optimization for admin logs and channel reads.

-- Logs filtering and ordering.
CREATE INDEX IF NOT EXISTS idx_job_created_ts
    ON job(created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_job_status_created_ts
    ON job(status, created_ts DESC);

CREATE INDEX IF NOT EXISTS idx_job_kind_created_ts
    ON job(kind, created_ts DESC);

-- Channel sidebar ordering.
CREATE INDEX IF NOT EXISTS idx_channel_position_created
    ON channel(position, created_ts);

-- Text channel pagination stability/perf for created_ts + id ordering.
CREATE INDEX IF NOT EXISTS idx_channel_message_channel_ts_id
    ON channel_message(channel_id, created_ts DESC, id DESC);
