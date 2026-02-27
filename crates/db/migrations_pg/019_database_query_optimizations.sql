-- Third-pass DB optimizations for high-frequency room/audio queries.

-- Member listing in rooms:
--   WHERE room_id = ?
--   ORDER BY invited_ts, user_id
CREATE INDEX IF NOT EXISTS idx_watch_party_member_room_invited_user
    ON watch_party_member(room_id, invited_ts, user_id);

-- Local audio duration lookup subquery:
--   WHERE efm.episode_item_id = ?
--   ORDER BY efm.created_ts ASC LIMIT 1
CREATE INDEX IF NOT EXISTS idx_episode_file_map_episode_created
    ON episode_file_map(episode_item_id, created_ts);
