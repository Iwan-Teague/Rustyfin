ALTER TABLE watch_party_room ADD COLUMN room_mode TEXT NOT NULL DEFAULT 'video';
ALTER TABLE watch_party_room ADD COLUMN audio_library_id TEXT;

CREATE TABLE IF NOT EXISTS watch_party_audio_queue (
    room_id TEXT PRIMARY KEY REFERENCES watch_party_room(id) ON DELETE CASCADE,
    track_ids_json TEXT NOT NULL,   -- JSON array of track item IDs in play order
    current_index INTEGER NOT NULL DEFAULT 0,
    updated_ts INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_item_library_kind
    ON item(library_id, kind);
