-- Fifth-pass PostgreSQL optimization:
-- Add GIN tsvector index for online listen-together track search.

CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_fts_search_vector
    ON watch_party_online_audio_track_fts
    USING GIN (to_tsvector('simple', COALESCE(title, '') || ' ' || COALESCE(channel, '')));
