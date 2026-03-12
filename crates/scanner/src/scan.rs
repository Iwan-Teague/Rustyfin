use rustfin_db::DbPool;
use std::path::Path;
use std::sync::LazyLock;
use tracing::{info, warn};

use crate::parser::{self, ParsedMedia};
use crate::walk;

static PROVIDER_ID_CLEAN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\s*\[.*?\]\s*").expect("valid provider cleanup regex"));

/// Run a full scan for a library, creating/updating items and media files.
pub async fn run_library_scan(
    pool: &DbPool,
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

        if library_kind == "music" {
            let sub = parse_music_library(pool, library_id, root)
                .await
                .map_err(ScanError::Db)?;
            result.added += sub.added;
            result.skipped += sub.skipped;
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

            // If a media row already exists and is mapped, this file is already indexed.
            // If the media row exists but has no mapping (e.g. old library deleted),
            // reuse it so re-scans can rebuild items without manual DB cleanup.
            //
            // TV entries are reconciled on every scan so stale root-level mappings from older
            // parser behavior cannot survive once the file is correctly recognized as an episode.
            let existing = get_existing_media_file(pool, &path_str)
                .await
                .map_err(ScanError::Db)?;
            let existing_file_id = existing.as_ref().map(|existing| existing.id.as_str());

            match parsed {
                ParsedMedia::Movie(info) => {
                    if existing
                        .as_ref()
                        .is_some_and(|existing| existing.has_mapping)
                    {
                        result.skipped += 1;
                        continue;
                    }

                    let changed = match create_movie_item(
                        pool,
                        library_id,
                        &info,
                        &path_str,
                        entry,
                        existing_file_id,
                    )
                    .await
                    {
                        Ok(changed) => changed,
                        Err(error) => {
                            warn!(
                                library_id = library_id,
                                file = %path_str,
                                error = %error,
                                "failed to index movie file; skipping entry"
                            );
                            result.skipped += 1;
                            continue;
                        }
                    };

                    if changed {
                        result.added += 1;
                    } else {
                        result.skipped += 1;
                    }
                }
                ParsedMedia::Episode(info) => {
                    let changed = match create_episode_item(
                        pool,
                        library_id,
                        &info,
                        &path_str,
                        entry,
                        existing_file_id,
                    )
                    .await
                    {
                        Ok(changed) => changed,
                        Err(error) => {
                            warn!(
                                library_id = library_id,
                                file = %path_str,
                                error = %error,
                                "failed to index episode file; skipping entry"
                            );
                            result.skipped += 1;
                            continue;
                        }
                    };

                    if changed {
                        result.added += 1;
                    } else {
                        result.skipped += 1;
                    }
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
    // Walk up all ancestor directories and prefer the nearest folder with a year.
    let mut cursor = rel.parent();
    while let Some(parent) = cursor {
        if parent == Path::new("") {
            break;
        }
        if let Some(folder_name) = parent.file_name().and_then(|n| n.to_str()) {
            let parsed = parser::parse_filename(folder_name);
            if matches!(&parsed, ParsedMedia::Movie(m) if m.year.is_some()) {
                return parsed;
            }
        }
        cursor = parent.parent();
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
                            let cleaned = PROVIDER_ID_CLEAN_RE
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
    let mut dirs: Vec<_> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if dirs.is_empty() {
        return None;
    }
    // Drop filename.
    dirs.pop();
    if dirs.is_empty() {
        return None;
    }

    // Prefer directory before a season-like folder (e.g. Show/Season 01/file.mkv),
    // even when extra category folders exist above the series directory.
    if let Some(season_idx) = dirs.iter().position(|d| is_season_dir(d)) {
        if season_idx > 0 {
            return Some(dirs[season_idx - 1].clone());
        }
    }

    // Fallback: nearest parent directory.
    dirs.last().cloned()
}

fn is_season_dir(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if let Some(stripped) = lower.strip_prefix("season ") {
        return stripped.trim().parse::<u32>().is_ok();
    }
    if lower.len() >= 2 && lower.starts_with('s') {
        return lower[1..].parse::<u32>().is_ok();
    }
    false
}

fn is_disc_dir(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.starts_with("disc ") || lower.starts_with("disk ") {
        let tail = lower.split_once(' ').map(|(_, t)| t.trim()).unwrap_or("");
        return !tail.is_empty() && tail.parse::<u32>().is_ok();
    }
    if let Some(stripped) = lower.strip_prefix("cd ") {
        let tail = stripped.trim();
        return !tail.is_empty() && tail.parse::<u32>().is_ok();
    }
    false
}

fn infer_music_artist_album(
    root: &Path,
    rel: &Path,
) -> (Option<String>, Option<String>, Option<std::path::PathBuf>) {
    let mut dirs: Vec<String> = rel
        .parent()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    if dirs.is_empty() {
        return (None, None, None);
    }

    // Strip disc-like folders from the end, preserving album grouping for
    // structures like Artist/Album/Disc 1/track.ext.
    while dirs.len() > 1 && dirs.last().is_some_and(|name| is_disc_dir(name)) {
        dirs.pop();
    }

    let album_name = dirs.last().cloned();
    let artist_name = if dirs.len() >= 2 {
        Some(dirs[dirs.len() - 2].clone())
    } else {
        None
    };

    let album_dir = if dirs.is_empty() {
        None
    } else {
        let mut rel_dir = std::path::PathBuf::new();
        for seg in &dirs {
            rel_dir.push(seg);
        }
        Some(root.join(rel_dir))
    };

    (artist_name, album_name, album_dir)
}

// ─── DB helpers ──────────────────────────────────────────────────────────────

struct ExistingMediaFile {
    id: String,
    has_mapping: bool,
}

async fn get_existing_media_file(
    pool: &DbPool,
    path: &str,
) -> Result<Option<ExistingMediaFile>, sqlx::Error> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT mf.id, \
            EXISTS ( \
                SELECT 1 FROM episode_file_map efm WHERE efm.file_id = mf.id \
            ) AS has_mapping \
         FROM media_file mf \
         WHERE mf.path = $1",
    )
    .bind(path)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, has_mapping)| ExistingMediaFile { id, has_mapping }))
}

async fn ensure_media_file(
    pool: &DbPool,
    path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
    probe_duration: bool,
) -> Result<String, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let size_bytes_i64 = match i64::try_from(entry.size_bytes) {
        Ok(value) => value,
        Err(_) => {
            warn!(
                file = %path,
                size_bytes = entry.size_bytes,
                "file size exceeds i64 range; clamping to i64::MAX"
            );
            i64::MAX
        }
    };

    if let Some(existing_id) = existing_file_id {
        sqlx::query(
            "UPDATE media_file \
             SET size_bytes = $1, mtime_ts = $2, updated_ts = $3 \
             WHERE id = $4",
        )
        .bind(size_bytes_i64)
        .bind(entry.mtime_ts)
        .bind(now)
        .bind(existing_id)
        .execute(pool)
        .await?;
        return Ok(existing_id.to_string());
    }

    let duration_ms = if probe_duration {
        probe_audio_duration_ms(path)
    } else {
        None
    };

    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO media_file (id, path, size_bytes, mtime_ts, duration_ms, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&id)
    .bind(path)
    .bind(size_bytes_i64)
    .bind(entry.mtime_ts)
    .bind(duration_ms)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

/// Run ffprobe on an audio file and return its duration in milliseconds.
/// Returns `None` if ffprobe is not available or the output cannot be parsed.
fn probe_audio_duration_ms(path: &str) -> Option<i64> {
    let output = std::process::Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_format", path])
        .output()
        .ok()?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let duration_str = json["format"]["duration"].as_str()?;
    let duration_secs: f64 = duration_str.parse().ok()?;
    Some((duration_secs * 1000.0) as i64)
}

async fn link_file_to_item(
    pool: &DbPool,
    item_id: &str,
    file_id: &str,
) -> Result<bool, sqlx::Error> {
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM episode_file_map WHERE episode_item_id = $1 AND file_id = $2 AND map_kind = 'primary'",
    )
    .bind(item_id)
    .bind(file_id)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Ok(false);
    }

    let map_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO episode_file_map (id, episode_item_id, file_id, map_kind, created_ts) \
         VALUES ($1, $2, $3, 'primary', $4)",
    )
    .bind(&map_id)
    .bind(item_id)
    .bind(file_id)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(true)
}

async fn find_or_create_item(
    pool: &DbPool,
    library_id: &str,
    kind: &str,
    parent_id: Option<&str>,
    title: &str,
    year: Option<u16>,
) -> Result<(String, bool), sqlx::Error> {
    // Try to find existing item with same title, kind, and parent
    let existing: Option<(String,)> = if let Some(pid) = parent_id {
        sqlx::query_as(
            "SELECT id FROM item WHERE library_id = $1 AND kind = $2 AND parent_id = $3 AND title = $4",
        )
        .bind(library_id)
        .bind(kind)
        .bind(pid)
        .bind(title)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id FROM item WHERE library_id = $1 AND kind = $2 AND parent_id IS NULL AND title = $3",
        )
        .bind(library_id)
        .bind(kind)
        .bind(title)
        .fetch_optional(pool)
        .await?
    };

    if let Some((id,)) = existing {
        return Ok((id, false));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO item (id, library_id, kind, parent_id, title, year, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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

    Ok((id, true))
}

async fn remove_conflicting_file_mappings(
    pool: &DbPool,
    canonical_item_id: &str,
    file_id: &str,
) -> Result<bool, sqlx::Error> {
    let mapped_item_ids: Vec<(String,)> =
        sqlx::query_as("SELECT episode_item_id FROM episode_file_map WHERE file_id = $1")
            .bind(file_id)
            .fetch_all(pool)
            .await?;

    let mut changed = false;

    for (mapped_item_id,) in mapped_item_ids {
        if mapped_item_id == canonical_item_id {
            continue;
        }

        sqlx::query("DELETE FROM episode_file_map WHERE file_id = $1 AND episode_item_id = $2")
            .bind(file_id)
            .bind(&mapped_item_id)
            .execute(pool)
            .await?;

        prune_item_chain_if_orphaned(pool, &mapped_item_id).await?;
        changed = true;
    }

    Ok(changed)
}

async fn prune_item_chain_if_orphaned(
    pool: &DbPool,
    start_item_id: &str,
) -> Result<(), sqlx::Error> {
    let mut current_item_id = Some(start_item_id.to_string());

    while let Some(item_id) = current_item_id.take() {
        let row: Option<(Option<String>, i64, i64)> = sqlx::query_as(
            "SELECT i.parent_id, \
                    (SELECT COUNT(*) FROM item child WHERE child.parent_id = i.id) AS child_count, \
                    (SELECT COUNT(*) FROM episode_file_map efm WHERE efm.episode_item_id = i.id) AS map_count \
             FROM item i \
             WHERE i.id = $1",
        )
        .bind(&item_id)
        .fetch_optional(pool)
        .await?;

        let Some((parent_id, child_count, map_count)) = row else {
            break;
        };

        if child_count > 0 || map_count > 0 {
            break;
        }

        sqlx::query("DELETE FROM item WHERE id = $1")
            .bind(&item_id)
            .execute(pool)
            .await?;

        current_item_id = parent_id;
    }

    Ok(())
}

async fn create_movie_item(
    pool: &DbPool,
    library_id: &str,
    info: &parser::MovieInfo,
    file_path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let (item_id, item_created) =
        find_or_create_item(pool, library_id, "movie", None, &info.title, info.year).await?;
    let file_id = ensure_media_file(pool, file_path, entry, existing_file_id, false).await?;
    let mapping_created = link_file_to_item(pool, &item_id, &file_id).await?;

    Ok(item_created || mapping_created)
}

async fn create_episode_item(
    pool: &DbPool,
    library_id: &str,
    info: &parser::EpisodeInfo,
    file_path: &str,
    entry: &walk::MediaEntry,
    existing_file_id: Option<&str>,
) -> Result<bool, sqlx::Error> {
    // Create or find series
    let (series_id, series_created) =
        find_or_create_item(pool, library_id, "series", None, &info.series_title, None).await?;

    // Create or find season
    let season_title = if info.season == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", info.season)
    };
    let (season_id, season_created) = find_or_create_item(
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
    let (episode_id, episode_created) = find_or_create_item(
        pool,
        library_id,
        "episode",
        Some(&season_id),
        &ep_title,
        None,
    )
    .await?;

    let file_id = ensure_media_file(pool, file_path, entry, existing_file_id, false).await?;
    let removed_conflicts = remove_conflicting_file_mappings(pool, &episode_id, &file_id).await?;
    let mapping_created = link_file_to_item(pool, &episode_id, &file_id).await?;

    Ok(series_created || season_created || episode_created || removed_conflicts || mapping_created)
}

// ─── Music library scanner ───────────────────────────────────────────────────

/// Scan a music library root directory and create artist/album/track items.
async fn parse_music_library(
    pool: &rustfin_db::DbPool,
    library_id: &str,
    root: &Path,
) -> Result<ScanResult, sqlx::Error> {
    let entries = walk::walk_audio_dir(root);
    info!(
        library_id = library_id,
        path = %root.display(),
        files_found = entries.len(),
        "scan found audio files"
    );

    let mut result = ScanResult::default();

    for entry in &entries {
        let path_str = entry.path.to_string_lossy().to_string();

        let existing = get_existing_media_file(pool, &path_str).await?;
        let existing_file_id = match existing {
            Some(existing) if existing.has_mapping => {
                result.skipped += 1;
                continue;
            }
            Some(existing) => Some(existing.id),
            None => None,
        };

        let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);

        let track_title = {
            let filename = rel
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Strip extension
            if let Some(pos) = filename.rfind('.') {
                filename[..pos].to_string()
            } else {
                filename
            }
        };

        let (artist_name, album_name, album_dir) = infer_music_artist_album(root, rel);

        // Create artist item if present
        let artist_id = if let Some(ref artist) = artist_name {
            Some(
                find_or_create_item(pool, library_id, "artist", None, artist, None)
                    .await?
                    .0,
            )
        } else {
            None
        };

        // Create album item if present
        let album_id = if let Some(ref album) = album_name {
            let parent = artist_id.as_deref();
            let album_id = find_or_create_item(pool, library_id, "album", parent, album, None)
                .await?
                .0;

            // Look for cover art in the album directory
            if let Some(ref dir) = album_dir {
                let cover_names = [
                    "cover.jpg",
                    "folder.jpg",
                    "album.jpg",
                    "front.jpg",
                    "cover.png",
                    "folder.png",
                    "album.png",
                    "front.png",
                ];
                for cover_name in &cover_names {
                    let cover_path = dir.join(cover_name);
                    if cover_path.exists() {
                        let cover_str = cover_path.to_string_lossy().to_string();
                        // Only update if poster_url is not already set
                        let existing_art: Option<(Option<String>,)> =
                            sqlx::query_as("SELECT poster_url FROM item WHERE id = $1")
                                .bind(&album_id)
                                .fetch_optional(pool)
                                .await?;
                        if existing_art.and_then(|(p,)| p).is_none() {
                            sqlx::query(
                                "UPDATE item SET poster_url = $1, updated_ts = $2 WHERE id = $3",
                            )
                            .bind(&cover_str)
                            .bind(chrono::Utc::now().timestamp())
                            .bind(&album_id)
                            .execute(pool)
                            .await?;
                        }
                        break;
                    }
                }
            }

            Some(album_id)
        } else {
            None
        };

        // Create track item under album (or artist if no album, or directly under library)
        let track_parent = album_id.as_deref().or(artist_id.as_deref());
        let track_id =
            find_or_create_item(pool, library_id, "track", track_parent, &track_title, None)
                .await?
                .0;

        let file_id =
            ensure_media_file(pool, &path_str, entry, existing_file_id.as_deref(), true).await?;
        let _ = link_file_to_item(pool, &track_id, &file_id).await?;

        result.added += 1;
    }

    Ok(result)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_movie_entry_uses_deep_ancestor_with_year() {
        let rel = Path::new("Archive/4K/The Matrix (1999)/Extras/scene.mkv");
        let parsed = parse_movie_entry(rel);
        assert_eq!(
            parsed,
            ParsedMedia::Movie(parser::MovieInfo {
                title: "The Matrix".to_string(),
                year: Some(1999),
            })
        );
    }

    #[test]
    fn find_series_dir_prefers_folder_before_season_in_deep_tree() {
        let rel = Path::new("Category/Drama/Breaking Bad/Season 01/S01E02.mkv");
        assert_eq!(find_series_dir(rel), Some("Breaking Bad".to_string()));
    }

    #[test]
    fn infer_music_artist_album_handles_deep_branches_and_disc_folders() {
        let root = Path::new("/media/music");
        let rel = Path::new("Genre/Artist/Album/Disc 1/01 Track.mp3");
        let (artist, album, album_dir) = infer_music_artist_album(root, rel);
        assert_eq!(artist.as_deref(), Some("Artist"));
        assert_eq!(album.as_deref(), Some("Album"));
        assert_eq!(
            album_dir,
            Some(
                Path::new("/media/music")
                    .join("Genre")
                    .join("Artist")
                    .join("Album")
            )
        );
    }
}
