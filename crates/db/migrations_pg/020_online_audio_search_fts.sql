-- PostgreSQL compatibility search table for online listen-together tracks.
CREATE TABLE IF NOT EXISTS watch_party_online_audio_track_fts (
    track_id TEXT NOT NULL,
    room_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(track_id, room_id)
);

CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_fts_room
    ON watch_party_online_audio_track_fts(room_id);

CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_fts_title_lower
    ON watch_party_online_audio_track_fts(LOWER(title));

CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_fts_channel_lower
    ON watch_party_online_audio_track_fts(LOWER(channel));

DELETE FROM watch_party_online_audio_track_fts;

INSERT INTO watch_party_online_audio_track_fts (track_id, room_id, title, channel)
SELECT id, room_id, title, channel
FROM watch_party_online_audio_track;
