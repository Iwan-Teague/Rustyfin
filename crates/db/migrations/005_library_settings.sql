CREATE TABLE IF NOT EXISTS library_settings (
    library_id TEXT PRIMARY KEY REFERENCES library(id) ON DELETE CASCADE,
    show_images INTEGER NOT NULL DEFAULT 1,
    prefer_local_artwork INTEGER NOT NULL DEFAULT 1,
    fetch_online_artwork INTEGER NOT NULL DEFAULT 1,
    updated_ts INTEGER NOT NULL
);

INSERT OR IGNORE INTO library_settings (
    library_id,
    show_images,
    prefer_local_artwork,
    fetch_online_artwork,
    updated_ts
)
SELECT
    l.id,
    1,
    1,
    1,
    strftime('%s', 'now')
FROM library l;

