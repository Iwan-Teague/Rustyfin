use sqlx::SqlitePool;
use std::path::Path;
use tracing::{info, warn};

use crate::parser::{self, ParsedMedia};
use crate::walk;

/// Run a full scan for a library, creating/updating items and media files.
pub async fn run_library_scan(
    pool: &SqlitePool,
    library_id: &str,
    library_kind: &str,
) -> Result<ScanResult, ScanError> {
    let paths = rustfin_db::repo::libraries::get_library_paths(pool, library_id)
        .await
        .map_err(ScanError::Db)?;

    let mut result = ScanResult::default();

    for lib_path in &paths {
        let root = Path::new(&lib_path.path);
        if !root.exists() {
            warn!(path = %lib_path.path, "library path does not exist, skipping");
            continue;
        }

        let entries = walk::walk_media_dir(root);
        info!(
            library_id = library_id,
            path = %lib_path.path,
            files_found = entries.len(),
            "scan found video files"
        );

        for entry in &entries {
            let path_str = entry.path.to_string_lossy().to_string();

            // If a media row already exists and is mapped, this file is already indexed.
            // If the media row exists but has no mapping (e.g. old library deleted),
            // reuse it so re-scans can rebuild items without manual DB cleanup.
            let existing = get_existing_media_file(pool, &path_str)
                .await
                .map_err(ScanError::Db)?;
            let existing_file_id = match existing {
                Some(existing) if existing.has_mapping => {
                    result.skipped += 1;
                    continue;
                }
                Some(existing) => Some(existing.id),
                None => None,
            };

            // Determine relative path for parsing
            let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);

            // Parse based on library kind
            let parsed = match library_kind {
                "movies" => parse_movie_entry(rel),
                "tv_shows" => parse_tv_entry(rel),
                _ => {
                    warn!(kind = library_kind, "unknown library kind");
                    continue;
                }
            };

            match parsed {
                ParsedMedia::Movie(info) => {
                    create_movie_item(
                        pool,
                        library_id,
                        &info,
                        &path_str,
                        entry,
                        existing_file_id.as_deref(),
                    )
                    .await
                    .map_err(ScanError::Db)?;
                    result.added += 1;
                }
                ParsedMedia::Episode(info) => {
                    create_episode_item(
                        pool,
                        library_id,
                        &info,
                        &path_str,
                        entry,
                        existing_file_id.as_deref(),
                    )
                    .await
                    .map_err(ScanError::Db)?;
                    result.added += 1;
                }
                ParsedMedia::Unknown(name) => {
                    warn!(file = %name, "could not parse media filename");
                    result.skipped += 1;
                }
            }
        }
    }

    Ok(result)
}

/// Parse a relative path for a movie entry.
/// Supports: `Movie (Year)/Movie (Year).mkv` or just `Movie.Year.mkv`
fn parse_movie_entry(rel: &Path) -> ParsedMedia {
    // Try folder name first if there's a parent directory
    if let Some(parent) = rel.parent() {
        if parent != Path::new("") {
            let folder = parent.to_string_lossy();
            let parsed = parser::parse_filename(&folder);
            if matches!(&parsed, ParsedMedia::Movie(m) if m.year.is_some()) {
                return parsed;
            }
        }
    }
    // Fall back to filename
    let name = rel.file_name().unwrap_or_default().to_string_lossy();
    parser::parse_filename(&name)
}

/// Parse a relative path for a TV entry.
/// Supports: `Show Name/Season 01/S01E02.mkv` or `Show Name/S01E02.mkv`
fn parse_tv_entry(rel: &Path) -> ParsedMedia {
    let filename = rel.file_name().unwrap_or_default().to_string_lossy();

    let parsed = parser::parse_filename(&filename);

    match parsed {
        ParsedMedia::Episode(mut ep) => {
            // If series_title is empty, try parent directory
            if ep.series_title.is_empty() {
                if let Some(series_dir) = find_series_dir(rel) {
                    ep.series_title = parser::extract_provider_ids(&series_dir)
                        .first()
                        .map(|_| {
                            // Strip provider IDs from folder name
                            let cleaned = regex::Regex::new(r"\s*\[.*?\]\s*")
                                .unwrap()
                                .replace_all(&series_dir, "")
                                .trim()
                                .to_string();
                            cleaned
                        })
                        .unwrap_or_else(|| series_dir.clone());
                    if ep.series_title.is_empty() {
                        ep.series_title = series_dir;
                    }
                }
            }
            ParsedMedia::Episode(ep)
        }
        other => other,
    }
}

/// Walk up from the file to find the series root directory name.
/// Typical structure: `Show Name/Season XX/file.mkv` — we want `Show Name`.
fn find_series_dir(rel: &Path) -> Option<String> {
    let components: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    // First component is the series directory
    components.first().cloned()
}

// ─── DB helpers ──────────────────────────────────────────────────────────────

struct ExistingMediaFile {
    id: String,
    has_mapping: bool,
}

async fn get_existing_media_file(
    pool: &SqlitePool,
    path: &str,
) -> Result<Option<ExistingMediaFile>, sqlx::Error> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT mf.id, \
            CASE WHEN EXISTS ( \
                SELECT 1 FROM episode_file_map efm WHERE efm.file_id = mf.id \
            ) THEN 1 ELSE 0 END AS has_mapping \
         FROM media_file mf \
         WHERE mf.path = ?",
    )
    .bind(path)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, has_mapping)| ExistingMediaFile {
        id,
        has_mapping: has_mapping != 0,
    }))
}

async fn ensure_media_file(
    pool: &SqlitePool,
    path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
) -> Result<String, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();

    if let Some(existing_id) = existing_file_id {
        sqlx::query(
            "UPDATE media_file \
             SET size_bytes = ?, mtime_ts = ?, updated_ts = ? \
             WHERE id = ?",
        )
        .bind(entry.size_bytes as i64)
        .bind(entry.mtime_ts)
        .bind(now)
        .bind(existing_id)
        .execute(pool)
        .await?;
        return Ok(existing_id.to_string());
    }

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO media_file (id, path, size_bytes, mtime_ts, created_ts, updated_ts) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(path)
    .bind(entry.size_bytes as i64)
    .bind(entry.mtime_ts)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

async fn link_file_to_item(
    pool: &SqlitePool,
    item_id: &str,
    file_id: &str,
) -> Result<(), sqlx::Error> {
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM episode_file_map WHERE episode_item_id = ? AND file_id = ? AND map_kind = 'primary'",
    )
    .bind(item_id)
    .bind(file_id)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Ok(());
    }

    let map_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO episode_file_map (id, episode_item_id, file_id, map_kind, created_ts) \
         VALUES (?, ?, ?, 'primary', ?)",
    )
    .bind(&map_id)
    .bind(item_id)
    .bind(file_id)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

async fn find_or_create_item(
    pool: &SqlitePool,
    library_id: &str,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    year: Option<u16>,
) -> Result<String, sqlx::Error> {
    // Try to find existing item with same title, kind, and parent
    let existing: Option<(String,)> = if let Some(pid) = parent_id {
        sqlx::query_as(
            "SELECT id FROM item WHERE library_id = ? AND kind = ? AND parent_id = ? AND title = ?",
        )
        .bind(library_id)
        .bind(kind)
        .bind(pid)
        .bind(title)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id FROM item WHERE library_id = ? AND kind = ? AND parent_id IS NULL AND title = ?",
        )
        .bind(library_id)
        .bind(kind)
        .bind(title)
        .fetch_optional(pool)
        .await?
    };

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO item (id, library_id, kind, parent_id, title, year, created_ts, updated_ts) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(library_id)
    .bind(kind)
    .bind(parent_id)
    .bind(title)
    .bind(year.map(|y| y as i64))
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

async fn create_movie_item(
    pool: &SqlitePool,
    library_id: &str,
    info: &parser::MovieInfo,
    file_path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let item_id =
        find_or_create_item(pool, library_id, "movie", None, &info.title, info.year).await?;
    let file_id = ensure_media_file(pool, file_path, entry, existing_file_id).await?;
    link_file_to_item(pool, &item_id, &file_id).await?;

    Ok(())
}

async fn create_episode_item(
    pool: &SqlitePool,
    library_id: &str,
    info: &parser::EpisodeInfo,
    file_path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    // Create or find series
    let series_id =
        find_or_create_item(pool, library_id, "series", None, &info.series_title, None).await?;

    // Create or find season
    let season_title = if info.season == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", info.season)
    };
    let season_id = find_or_create_item(
        pool,
        library_id,
        "season",
        Some(&series_id),
        &season_title,
        None,
    )
    .await?;

    // Create episode
    let ep_title = info
        .episode_title
        .clone()
        .unwrap_or_else(|| format!("Episode {}", info.episode));
    let episode_id = find_or_create_item(
        pool,
        library_id,
        "episode",
        Some(&season_id),
        &ep_title,
        None,
    )
    .await?;

    let file_id = ensure_media_file(pool, file_path, entry, existing_file_id).await?;
    link_file_to_item(pool, &episode_id, &file_id).await?;

    Ok(())
}

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ScanResult {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("database error: {0}")]
    Db(sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
