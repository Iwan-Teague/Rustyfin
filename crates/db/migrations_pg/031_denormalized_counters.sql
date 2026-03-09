ALTER TABLE channel_transcript_session
    ADD COLUMN IF NOT EXISTS entry_count BIGINT NOT NULL DEFAULT 0;

UPDATE channel_transcript_session AS s
SET entry_count = counts.entry_count
FROM (
    SELECT session_id, COUNT(*)::BIGINT AS entry_count
    FROM channel_transcript_entry
    GROUP BY session_id
) AS counts
WHERE s.id = counts.session_id;

ALTER TABLE watch_party_room
    ADD COLUMN IF NOT EXISTS joined_member_count BIGINT NOT NULL DEFAULT 0;

UPDATE watch_party_room AS r
SET joined_member_count = counts.joined_member_count
FROM (
    SELECT room_id, COUNT(*)::BIGINT AS joined_member_count
    FROM watch_party_member
    WHERE status = 'joined'
    GROUP BY room_id
) AS counts
WHERE r.id = counts.room_id;

CREATE INDEX IF NOT EXISTS idx_watch_party_member_room_joined
    ON watch_party_member(room_id)
    WHERE status = 'joined';
