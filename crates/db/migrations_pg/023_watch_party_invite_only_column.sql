ALTER TABLE watch_party_room
ADD COLUMN invite_only INTEGER NOT NULL DEFAULT 0
CHECK (invite_only IN (0, 1));

UPDATE watch_party_room
SET invite_only = CASE
    WHEN policy_json ILIKE '%\"invite_only\"%true%' THEN 1
    ELSE 0
END;

CREATE INDEX IF NOT EXISTS idx_watch_party_room_status_invite_created
    ON watch_party_room(status, invite_only, created_ts DESC);
