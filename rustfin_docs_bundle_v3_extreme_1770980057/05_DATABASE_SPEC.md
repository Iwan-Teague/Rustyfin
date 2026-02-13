# Database Specification (Extreme Expansion, SQLite)

Goals:
- fast browse queries
- user overrides survive refresh
- expected-vs-present episodes supported
- easy backups, WAL mode

## 1. Libraries + paths
```sql
CREATE TABLE library (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  created_ts INTEGER NOT NULL,
  updated_ts INTEGER NOT NULL
);

CREATE TABLE library_path (
  id TEXT PRIMARY KEY,
  library_id TEXT NOT NULL REFERENCES library(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  is_read_only INTEGER NOT NULL DEFAULT 1,
  created_ts INTEGER NOT NULL
);
```

## 2. Items
```sql
CREATE TABLE item (
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
CREATE INDEX idx_item_parent ON item(parent_id);
```

## 3. Provider IDs + locks
```sql
CREATE TABLE item_provider_id (
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  value TEXT NOT NULL,
  locked INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(item_id, provider)
);

CREATE TABLE item_field_lock (
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  field TEXT NOT NULL,
  locked INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(item_id, field)
);
```

## 4. Expected episodes
```sql
CREATE TABLE episode_expected (
  series_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  season_number INTEGER NOT NULL,
  episode_number INTEGER NOT NULL,
  title TEXT,
  overview TEXT,
  air_date TEXT,
  provider_episode_id TEXT,
  PRIMARY KEY(series_id, season_number, episode_number)
);
```

## 5. Files + mapping
```sql
CREATE TABLE media_file (
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

CREATE TABLE episode_file_map (
  id TEXT PRIMARY KEY,
  episode_item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  file_id TEXT NOT NULL REFERENCES media_file(id) ON DELETE CASCADE,
  map_kind TEXT NOT NULL,
  part_index INTEGER,
  created_ts INTEGER NOT NULL
);
```

## 6. Users + playstate
```sql
CREATE TABLE user (
  id TEXT PRIMARY KEY,
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  role TEXT NOT NULL,
  created_ts INTEGER NOT NULL
);

CREATE TABLE user_pref (
  user_id TEXT PRIMARY KEY REFERENCES user(id) ON DELETE CASCADE,
  json TEXT NOT NULL,
  updated_ts INTEGER NOT NULL
);

CREATE TABLE user_item_state (
  user_id TEXT NOT NULL REFERENCES user(id) ON DELETE CASCADE,
  item_id TEXT NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  played INTEGER NOT NULL DEFAULT 0,
  progress_ms INTEGER NOT NULL DEFAULT 0,
  last_played_ts INTEGER,
  favorite INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(user_id, item_id)
);
```

## 7. Jobs
```sql
CREATE TABLE job (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  progress REAL NOT NULL DEFAULT 0,
  payload_json TEXT,
  error TEXT,
  created_ts INTEGER NOT NULL,
  updated_ts INTEGER NOT NULL
);
```
