ALTER TABLE watch_party_room
ADD COLUMN audio_source TEXT NOT NULL DEFAULT 'library'
CHECK (audio_source IN ('library', 'online'));

CREATE TABLE IF NOT EXISTS watch_party_online_audio_track (
    id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL REFERENCES watch_party_room(id) ON DELETE CASCADE,
    video_id TEXT NOT NULL,
    title TEXT NOT NULL,
    channel TEXT NOT NULL,
    thumbnail_url TEXT,
    file_path TEXT NOT NULL,
    duration_ms INTEGER,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL,
    UNIQUE(room_id, video_id)
);

CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_room
    ON watch_party_online_audio_track(room_id, created_ts);
