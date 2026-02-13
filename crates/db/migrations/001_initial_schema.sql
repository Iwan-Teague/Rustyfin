-- Enable WAL mode (set once, persists in the database file)
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Users
CREATE TABLE IF NOT EXISTS user (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    created_ts INTEGER NOT NULL
);

-- User preferences (JSON blob per user)
CREATE TABLE IF NOT EXISTS user_pref (
    user_id TEXT PRIMARY KEY REFERENCES user(id) ON DELETE CASCADE,
    json TEXT NOT NULL DEFAULT '{}',
    updated_ts INTEGER NOT NULL
);

-- User per-item playback state
CREATE TABLE IF NOT EXISTS user_item_state (
    user_id TEXT NOT NULL REFERENCES user(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    played INTEGER NOT NULL DEFAULT 0,
    progress_ms INTEGER NOT NULL DEFAULT 0,
    last_played_ts INTEGER,
    favorite INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(user_id, item_id)
);

-- Libraries
CREATE TABLE IF NOT EXISTS library (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS library_path (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    is_read_only INTEGER NOT NULL DEFAULT 1,
    created_ts INTEGER NOT NULL
);

-- Items (hierarchical: movie, series, season, episode)
CREATE TABLE IF NOT EXISTS item (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES library(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    parent_id TEXT REFERENCES item(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    sort_title TEXT,
    year INTEGER,
    overview TEXT,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_item_parent ON item(parent_id);
CREATE INDEX IF NOT EXISTS idx_item_library ON item(library_id);

-- Provider IDs and field locks
CREATE TABLE IF NOT EXISTS item_provider_id (
    item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    value TEXT NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(item_id, provider)
);

CREATE TABLE IF NOT EXISTS item_field_lock (
    item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    field TEXT NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(item_id, field)
);

-- Expected episodes (TV series)
CREATE TABLE IF NOT EXISTS episode_expected (
    series_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    season_number INTEGER NOT NULL,
    episode_number INTEGER NOT NULL,
    title TEXT,
    overview TEXT,
    air_date TEXT,
    provider_episode_id TEXT,
    PRIMARY KEY(series_id, season_number, episode_number)
);

-- Media files
CREATE TABLE IF NOT EXISTS media_file (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    size_bytes INTEGER NOT NULL,
    mtime_ts INTEGER NOT NULL,
    quick_hash INTEGER,
    strong_hash BLOB,
    container TEXT,
    duration_ms INTEGER,
    stream_info_json TEXT,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
);

-- Episode-to-file mapping (supports multi-part)
CREATE TABLE IF NOT EXISTS episode_file_map (
    id TEXT PRIMARY KEY,
    episode_item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL REFERENCES media_file(id) ON DELETE CASCADE,
    map_kind TEXT NOT NULL,
    part_index INTEGER,
    created_ts INTEGER NOT NULL
);

-- Jobs queue
CREATE TABLE IF NOT EXISTS job (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL NOT NULL DEFAULT 0,
    payload_json TEXT,
    error TEXT,
    created_ts INTEGER NOT NULL,
    updated_ts INTEGER NOT NULL
);
