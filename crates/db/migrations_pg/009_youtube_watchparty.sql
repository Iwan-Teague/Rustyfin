-- Make item_id nullable (YouTube rooms have no item) and add youtube_video_id.
ALTER TABLE watch_party_room
    ALTER COLUMN item_id DROP NOT NULL;

ALTER TABLE watch_party_room
    ADD COLUMN IF NOT EXISTS youtube_video_id TEXT;

CREATE INDEX IF NOT EXISTS idx_watch_party_room_host_created
    ON watch_party_room(host_user_id, created_ts);
CREATE INDEX IF NOT EXISTS idx_watch_party_room_status_updated
    ON watch_party_room(status, updated_ts);
