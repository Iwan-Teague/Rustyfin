-- Add optional display name for watch-party rooms.
ALTER TABLE watch_party_room ADD COLUMN room_name TEXT NOT NULL DEFAULT '';
