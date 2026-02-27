ALTER TABLE library_settings ADD COLUMN tmdb_store_in_media_dir INTEGER NOT NULL DEFAULT 0;
ALTER TABLE library_settings ADD COLUMN tmdb_sync_on_new_media INTEGER NOT NULL DEFAULT 1;
ALTER TABLE library_settings ADD COLUMN tmdb_sync_schedule TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE library_settings ADD COLUMN tmdb_last_sync_ts INTEGER;
ALTER TABLE library_settings ADD COLUMN tmdb_fetch_posters INTEGER NOT NULL DEFAULT 1;
ALTER TABLE library_settings ADD COLUMN tmdb_fetch_backdrops INTEGER NOT NULL DEFAULT 1;
ALTER TABLE library_settings ADD COLUMN tmdb_fetch_metadata INTEGER NOT NULL DEFAULT 1;
ALTER TABLE library_settings ADD COLUMN tmdb_fetch_reviews INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS item_tmdb_review (
    item_id TEXT PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
    reviews_json TEXT NOT NULL DEFAULT '[]',
    updated_ts INTEGER NOT NULL
);

