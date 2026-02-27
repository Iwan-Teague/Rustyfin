-- Targeted indexes for second-pass query optimization on high-traffic paths.

-- Batched channel attachment fetches:
--   WHERE message_id IN (...) ORDER BY message_id, created_ts, id
CREATE INDEX IF NOT EXISTS idx_channel_attachment_message_created
    ON channel_message_attachment(message_id, created_ts, id);

-- Batched library path fetches:
--   WHERE library_id IN (...) ORDER BY library_id, created_ts, id
CREATE INDEX IF NOT EXISTS idx_library_path_library_created
    ON library_path(library_id, created_ts, id);

-- Library top-level item counting:
--   WHERE library_id IN (...) AND parent_id IS NULL GROUP BY library_id
CREATE INDEX IF NOT EXISTS idx_item_library_parent
    ON item(library_id, parent_id);

-- Online audio room queue/list ordering:
--   WHERE room_id = ? ORDER BY created_ts DESC, updated_ts DESC
CREATE INDEX IF NOT EXISTS idx_watch_party_online_audio_track_room_created_updated
    ON watch_party_online_audio_track(room_id, created_ts DESC, updated_ts DESC);
