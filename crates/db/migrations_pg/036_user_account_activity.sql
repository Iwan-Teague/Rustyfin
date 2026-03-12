ALTER TABLE "user"
    ADD COLUMN IF NOT EXISTS time_zone TEXT;

CREATE TABLE IF NOT EXISTS user_activity_session (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    activity_kind TEXT NOT NULL,
    section_key TEXT NOT NULL DEFAULT '',
    subject_type TEXT NOT NULL DEFAULT '',
    subject_id TEXT NOT NULL DEFAULT '',
    tab_id TEXT,
    client_session_id TEXT,
    started_ts BIGINT NOT NULL,
    last_heartbeat_ts BIGINT NOT NULL,
    ended_ts BIGINT,
    accumulated_ms BIGINT NOT NULL DEFAULT 0,
    last_position_ms BIGINT,
    rolled_up_ts BIGINT,
    created_ts BIGINT NOT NULL,
    updated_ts BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_user_activity_session_user_kind_started
    ON user_activity_session(user_id, activity_kind, started_ts DESC);

CREATE INDEX IF NOT EXISTS idx_user_activity_session_open
    ON user_activity_session(user_id, ended_ts, last_heartbeat_ts DESC);

CREATE INDEX IF NOT EXISTS idx_user_activity_session_subject
    ON user_activity_session(user_id, activity_kind, subject_type, subject_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_activity_browser_open
    ON user_activity_session(user_id, client_session_id)
    WHERE activity_kind = 'browser_section' AND ended_ts IS NULL AND client_session_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_activity_voice_open
    ON user_activity_session(user_id, activity_kind, subject_id)
    WHERE activity_kind = 'voice_channel' AND ended_ts IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_activity_room_open
    ON user_activity_session(user_id, activity_kind, subject_id)
    WHERE activity_kind = 'watch_room' AND ended_ts IS NULL;

CREATE TABLE IF NOT EXISTS user_activity_daily (
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    day_utc TEXT NOT NULL,
    activity_kind TEXT NOT NULL,
    section_key TEXT NOT NULL DEFAULT '',
    subject_type TEXT NOT NULL DEFAULT '',
    subject_id TEXT NOT NULL DEFAULT '',
    total_ms BIGINT NOT NULL DEFAULT 0,
    session_count BIGINT NOT NULL DEFAULT 0,
    first_started_ts BIGINT,
    last_ended_ts BIGINT,
    PRIMARY KEY (user_id, day_utc, activity_kind, section_key, subject_type, subject_id)
);

CREATE INDEX IF NOT EXISTS idx_user_activity_daily_user_kind_day
    ON user_activity_daily(user_id, activity_kind, day_utc DESC);
