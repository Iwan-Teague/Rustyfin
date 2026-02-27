CREATE TABLE IF NOT EXISTS watch_party_room (
    id TEXT PRIMARY KEY,
    host_user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('lobby', 'ended')),
    policy_json TEXT NOT NULL,
    join_password_hash TEXT,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS watch_party_member (
    room_id TEXT NOT NULL REFERENCES watch_party_room(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('host', 'controller', 'viewer')),
    status TEXT NOT NULL CHECK (status IN ('invited', 'joined', 'declined', 'left')),
    invited_by TEXT REFERENCES "user"(id) ON DELETE SET NULL,
    invited_ts INTEGER,
    joined_ts INTEGER,
    last_seen_ts INTEGER,
    PRIMARY KEY (room_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_watch_party_room_host_created
    ON watch_party_room(host_user_id, created_ts);
CREATE INDEX IF NOT EXISTS idx_watch_party_room_status_updated
    ON watch_party_room(status, updated_ts);
CREATE INDEX IF NOT EXISTS idx_watch_party_member_room
    ON watch_party_member(room_id);
CREATE INDEX IF NOT EXISTS idx_watch_party_member_user_status
    ON watch_party_member(user_id, status);
