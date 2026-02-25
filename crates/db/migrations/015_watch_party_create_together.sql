ALTER TABLE watch_party_room
ADD COLUMN create_tool TEXT NOT NULL DEFAULT 'text'
CHECK (create_tool IN ('text', 'canvas'));

ALTER TABLE watch_party_room
ADD COLUMN create_document_name TEXT NOT NULL DEFAULT 'Untitled Document';

CREATE TABLE IF NOT EXISTS watch_party_create_state (
    room_id TEXT PRIMARY KEY REFERENCES watch_party_room(id) ON DELETE CASCADE,
    active_tool TEXT NOT NULL DEFAULT 'text' CHECK (active_tool IN ('text', 'canvas')),
    document_name TEXT NOT NULL DEFAULT 'Untitled Document',
    text_format TEXT NOT NULL DEFAULT 'plain' CHECK (text_format IN ('plain', 'markdown', 'pdf_text')),
    text_content TEXT NOT NULL DEFAULT '',
    canvas_strokes_json TEXT NOT NULL DEFAULT '[]',
    updated_ts INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_watch_party_create_state_updated
    ON watch_party_create_state(updated_ts);
