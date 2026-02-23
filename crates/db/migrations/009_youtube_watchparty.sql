-- Recreate watch_party_room with nullable item_id (YouTube rooms have no item)
-- and add youtube_video_id column.
PRAGMA foreign_keys = OFF;

CREATE TABLE watch_party_room_v2 (
    id TEXT PRIMARY KEY,
    host_user_id TEXT NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    item_id TEXT REFERENCES item(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('lobby', 'ended')),
    policy_json TEXT NOT NULL,
    join_password_hash TEXT,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL,
    room_mode TEXT NOT NULL DEFAULT 'video',
    audio_library_id TEXT,
    youtube_video_id TEXT
);

INSERT INTO watch_party_room_v2
    SELECT id, host_user_id, NULLIF(item_id, ''), status, policy_json,
           join_password_hash, created_ts, updated_ts, room_mode, audio_library_id, NULL
    FROM watch_party_room;

DROP TABLE watch_party_room;

ALTER TABLE watch_party_room_v2 RENAME TO watch_party_room;

CREATE INDEX IF NOT EXISTS idx_watch_party_room_host_created
    ON watch_party_room(host_user_id, created_ts);
CREATE INDEX IF NOT EXISTS idx_watch_party_room_status_updated
    ON watch_party_room(status, updated_ts);

PRAGMA foreign_keys = ON;
