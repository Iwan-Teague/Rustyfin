-- Fourth-pass DB optimization:
-- Add FTS-backed search index for online listen-together room tracks.

CREATE VIRTUAL TABLE IF NOT EXISTS watch_party_online_audio_track_fts
USING fts5(
    track_id UNINDEXED,
    room_id UNINDEXED,
    title,
    channel,
    tokenize = 'unicode61'
);

-- Rebuild FTS content from canonical room online-track table.
DELETE FROM watch_party_online_audio_track_fts;

INSERT INTO watch_party_online_audio_track_fts (track_id, room_id, title, channel)
SELECT id, room_id, title, channel
FROM watch_party_online_audio_track;
