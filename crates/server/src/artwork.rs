use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Context;
use rusqlite::{Connection, OpenFlags, OptionalExtension, types::ValueRef};
use rustfin_metadata::ItemMetadata;
use rustfin_metadata::provider::{MetadataProvider, SearchResult};
use tracing::{debug, warn};

#[derive(Clone, Debug, Default)]
struct Artwork {
    poster: Option<String>,
    backdrop: Option<String>,
    logo: Option<String>,
    thumb: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct FetchedProviderMetadata {
    provider_id: Option<String>,
    metadata: Option<ItemMetadata>,
}

#[derive(Clone, Debug)]
struct JellyfinMetadataSource {
    db_path: PathBuf,
    metadata_path: PathBuf,
}

#[derive(Clone, Debug)]
struct JellyfinEpisodeAnchor {
    guid: String,
    season_id: Option<String>,
    series_id: Option<String>,
}

async fn resolve_tmdb_api_key(pool: &rustfin_db::DbPool) -> anyhow::Result<Option<String>> {
    let db_key = rustfin_db::repo::settings::get(pool, "tmdb_api_key")
        .await
        .context("failed to read tmdb_api_key from settings")?
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
    if db_key.is_some() {
        return Ok(db_key);
    }

    Ok(std::env::var("RUSTFIN_TMDB_KEY").ok().and_then(|value| {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }))
}

pub async fn enrich_library_artwork(
    pool: &rustfin_db::DbPool,
    library_id: &str,
    library_kind: &str,
) -> anyhow::Result<()> {
    let settings = rustfin_db::repo::libraries::get_library_settings(pool, library_id)
        .await
        .context("failed to read library settings")?
        .unwrap_or_else(|| rustfin_db::repo::libraries::LibrarySettingsRow {
            library_id: library_id.to_string(),
            show_images: true,
            prefer_local_artwork: true,
            fetch_online_artwork: true,
            tmdb_store_in_media_dir: false,
            tmdb_sync_on_new_media: true,
            tmdb_sync_schedule: "manual".to_string(),
            tmdb_last_sync_ts: None,
            tmdb_fetch_posters: true,
            tmdb_fetch_backdrops: true,
            tmdb_fetch_metadata: true,
            tmdb_fetch_reviews: false,
            updated_ts: chrono::Utc::now().timestamp(),
        });

    if !settings.show_images {
        return Ok(());
    }

    let tmdb_client = if settings.fetch_online_artwork {
        resolve_tmdb_api_key(pool)
            .await?
            .map(rustfin_metadata::tmdb::TmdbClient::new)
    } else {
        None
    };
    if settings.fetch_online_artwork && tmdb_client.is_none() {
        warn!(
            library_id = %library_id,
            "online artwork/metadata is enabled but RUSTFIN_TMDB_KEY is not set; skipping TMDB enrichment"
        );
    }

    let top_level_items = rustfin_db::repo::items::get_library_items(pool, library_id)
        .await
        .context("failed to list library items")?;

    for item in top_level_items {
        if item.kind != "movie" && item.kind != "series" {
            continue;
        }

        let local = find_local_item_artwork(
            pool,
            &item.id,
            &item.kind,
            Some(item.title.as_str()),
            item.year,
        )
        .await
        .unwrap_or_default();
        let existing_tmdb_id = rustfin_metadata::merge::get_provider_ids(pool, &item.id)
            .await
            .context("failed to fetch provider IDs")?
            .into_iter()
            .find_map(|(provider, value)| {
                if provider.eq_ignore_ascii_case("tmdb") {
                    Some(value)
                } else {
                    None
                }
            });

        let fetched = match (&tmdb_client, library_kind, item.kind.as_str()) {
            (Some(client), "movies", "movie") => {
                fetch_tmdb_movie_metadata(client, &item, existing_tmdb_id.as_deref()).await
            }
            (Some(client), "tv_shows", "series") => {
                fetch_tmdb_series_metadata(client, &item, existing_tmdb_id.as_deref()).await
            }
            _ => FetchedProviderMetadata::default(),
        };

        if let Some(provider_id) = fetched.provider_id.as_deref() {
            rustfin_metadata::merge::set_provider_id(pool, &item.id, "tmdb", provider_id)
                .await
                .context("failed to store TMDB provider id")?;
        }
        if let Some(provider_meta) = fetched.metadata.as_ref() {
            rustfin_metadata::merge::merge_metadata(pool, &item.id, provider_meta)
                .await
                .context("failed to merge TMDB metadata")?;
        }

        let online = artwork_from_metadata(fetched.metadata.as_ref());

        merge_and_apply_artwork(
            pool,
            &item.id,
            &local,
            &online,
            settings.prefer_local_artwork,
            settings.fetch_online_artwork,
        )
        .await?;

        if item.kind == "series" {
            let children = rustfin_db::repo::items::get_children(pool, &item.id)
                .await
                .context("failed to fetch season children")?;
            for season in children.into_iter().filter(|c| c.kind == "season") {
                let season_local = find_local_item_artwork(
                    pool,
                    &season.id,
                    "season",
                    Some(season.title.as_str()),
                    season.year,
                )
                .await
                .unwrap_or_default();
                let fallback_from_series = Artwork {
                    poster: online.poster.clone().or(local.poster.clone()),
                    backdrop: online.backdrop.clone().or(local.backdrop.clone()),
                    logo: online.logo.clone().or(local.logo.clone()),
                    thumb: online.thumb.clone().or(local.thumb.clone()),
                };

                merge_and_apply_artwork(
                    pool,
                    &season.id,
                    &season_local,
                    &fallback_from_series,
                    settings.prefer_local_artwork,
                    settings.fetch_online_artwork,
                )
                .await?;
            }
        }
    }

    Ok(())
}

fn artwork_from_metadata(metadata: Option<&ItemMetadata>) -> Artwork {
    match metadata {
        Some(meta) => Artwork {
            poster: meta.poster_url.clone(),
            backdrop: meta.backdrop_url.clone(),
            logo: meta.logo_url.clone(),
            thumb: meta.thumb_url.clone(),
        },
        None => Artwork::default(),
    }
}

fn normalize_title_for_match(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn pick_best_search_provider_id(
    item_title: &str,
    item_year: Option<i32>,
    results: &[SearchResult],
) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let normalized_item_title = normalize_title_for_match(item_title);

    if let Some(year) = item_year {
        if let Some(hit) = results.iter().find(|hit| {
            normalize_title_for_match(&hit.title) == normalized_item_title && hit.year == Some(year)
        }) {
            return Some(hit.provider_id.clone());
        }
    }

    if let Some(hit) = results
        .iter()
        .find(|hit| normalize_title_for_match(&hit.title) == normalized_item_title)
    {
        return Some(hit.provider_id.clone());
    }

    if let Some(year) = item_year {
        if let Some(hit) = results.iter().find(|hit| hit.year == Some(year)) {
            return Some(hit.provider_id.clone());
        }
    }

    results.first().map(|hit| hit.provider_id.clone())
}

async fn fetch_tmdb_movie_metadata(
    client: &rustfin_metadata::tmdb::TmdbClient,
    item: &rustfin_db::repo::items::ItemRow,
    existing_tmdb_id: Option<&str>,
) -> FetchedProviderMetadata {
    let item_year = item.year.map(|y| y as i32);
    let provider_id = if let Some(existing) = existing_tmdb_id {
        Some(existing.to_string())
    } else {
        match client.search_movie(&item.title, item_year).await {
            Ok(results) => pick_best_search_provider_id(&item.title, item_year, &results),
            Err(err) => {
                warn!(item_id = %item.id, error = %err, "TMDB movie search failed");
                None
            }
        }
    };

    let Some(provider_id) = provider_id else {
        return FetchedProviderMetadata::default();
    };

    match client.get_movie(&provider_id).await {
        Ok(meta) => FetchedProviderMetadata {
            provider_id: Some(provider_id),
            metadata: Some(meta),
        },
        Err(err) => {
            warn!(
                item_id = %item.id,
                provider_id = %provider_id,
                error = %err,
                "failed to fetch TMDB movie metadata"
            );
            FetchedProviderMetadata::default()
        }
    }
}

async fn fetch_tmdb_series_metadata(
    client: &rustfin_metadata::tmdb::TmdbClient,
    item: &rustfin_db::repo::items::ItemRow,
    existing_tmdb_id: Option<&str>,
) -> FetchedProviderMetadata {
    let item_year = item.year.map(|y| y as i32);
    let provider_id = if let Some(existing) = existing_tmdb_id {
        Some(existing.to_string())
    } else {
        match client.search_series(&item.title, item_year).await {
            Ok(results) => pick_best_search_provider_id(&item.title, item_year, &results),
            Err(err) => {
                warn!(item_id = %item.id, error = %err, "TMDB series search failed");
                None
            }
        }
    };

    let Some(provider_id) = provider_id else {
        return FetchedProviderMetadata::default();
    };

    match client.get_series(&provider_id).await {
        Ok(meta) => FetchedProviderMetadata {
            provider_id: Some(provider_id),
            metadata: Some(meta),
        },
        Err(err) => {
            warn!(
                item_id = %item.id,
                provider_id = %provider_id,
                error = %err,
                "failed to fetch TMDB series metadata"
            );
            FetchedProviderMetadata::default()
        }
    }
}

async fn merge_and_apply_artwork(
    pool: &rustfin_db::DbPool,
    item_id: &str,
    local: &Artwork,
    online: &Artwork,
    prefer_local_artwork: bool,
    fetch_online_artwork: bool,
) -> anyhow::Result<()> {
    let existing = rustfin_db::repo::items::get_item_artwork(pool, item_id)
        .await
        .context("failed to load existing item artwork")?
        .map(|(poster, backdrop, logo, thumb)| Artwork {
            poster,
            backdrop,
            logo,
            thumb,
        })
        .unwrap_or_default();

    let choose = |current: &Option<String>, local_v: &Option<String>, online_v: &Option<String>| {
        if prefer_local_artwork {
            local_v
                .clone()
                .or_else(|| {
                    if fetch_online_artwork {
                        online_v.clone()
                    } else {
                        None
                    }
                })
                .or_else(|| current.clone())
        } else if fetch_online_artwork {
            online_v
                .clone()
                .or_else(|| local_v.clone())
                .or_else(|| current.clone())
        } else {
            local_v.clone().or_else(|| current.clone())
        }
    };

    let merged = Artwork {
        poster: choose(&existing.poster, &local.poster, &online.poster),
        backdrop: choose(&existing.backdrop, &local.backdrop, &online.backdrop),
        logo: choose(&existing.logo, &local.logo, &online.logo),
        thumb: choose(&existing.thumb, &local.thumb, &online.thumb),
    };

    if merged.poster != existing.poster
        || merged.backdrop != existing.backdrop
        || merged.logo != existing.logo
        || merged.thumb != existing.thumb
    {
        rustfin_db::repo::items::update_item_artwork(
            pool,
            item_id,
            merged.poster.as_deref(),
            merged.backdrop.as_deref(),
            merged.logo.as_deref(),
            merged.thumb.as_deref(),
        )
        .await
        .context("failed to save merged item artwork")?;
    }

    Ok(())
}

async fn find_local_item_artwork(
    pool: &rustfin_db::DbPool,
    item_id: &str,
    item_kind: &str,
    item_title: Option<&str>,
    item_year: Option<i64>,
) -> anyhow::Result<Artwork> {
    let direct_media_path = rustfin_db::repo::items::get_item_media_path(pool, item_id)
        .await
        .context("failed to read direct media path")?;
    let media_path = if let Some(path) = direct_media_path {
        Some(path)
    } else {
        rustfin_db::repo::items::get_first_descendant_media_path(pool, item_id)
            .await
            .context("failed to read descendant media path")?
    };

    let Some(media_path) = media_path else {
        return Ok(Artwork::default());
    };

    let media = PathBuf::from(&media_path);
    let Some(parent_dir) = media.parent() else {
        return Ok(Artwork::default());
    };

    let art_dir = match item_kind {
        "series" => parent_dir.parent().unwrap_or(parent_dir),
        "season" => parent_dir,
        _ => parent_dir,
    };

    if !art_dir.exists() || !art_dir.is_dir() {
        return Ok(Artwork::default());
    }

    let poster = find_named_file(
        art_dir,
        &[
            "poster.jpg",
            "poster.jpeg",
            "poster.png",
            "poster.webp",
            "poster.tbn",
            "folder.jpg",
            "folder.jpeg",
            "folder.png",
            "folder.webp",
            "cover.jpg",
            "cover.jpeg",
            "cover.png",
            "cover.webp",
            "season.jpg",
            "season.png",
            "season.webp",
        ],
    );
    let backdrop = find_named_file(
        art_dir,
        &[
            "backdrop.jpg",
            "backdrop.jpeg",
            "backdrop.png",
            "backdrop.webp",
            "fanart.jpg",
            "fanart.jpeg",
            "fanart.png",
            "fanart.webp",
            "banner.jpg",
            "banner.jpeg",
            "banner.png",
            "banner.webp",
            "background.jpg",
            "background.jpeg",
            "background.png",
            "background.webp",
        ],
    );
    let logo = find_named_file(
        art_dir,
        &[
            "logo.png",
            "logo.webp",
            "clearlogo.png",
            "clearlogo.webp",
            "logo.jpg",
            "logo.jpeg",
            "clearlogo.jpg",
            "clearlogo.jpeg",
        ],
    );
    let thumb = find_named_file(
        art_dir,
        &[
            "thumb.jpg",
            "thumb.jpeg",
            "thumb.png",
            "thumb.webp",
            "landscape.jpg",
            "landscape.jpeg",
            "landscape.png",
            "landscape.webp",
            "screenshot.jpg",
            "screenshot.jpeg",
            "screenshot.png",
            "screenshot.webp",
            "screen.jpg",
            "screen.jpeg",
            "screen.png",
            "screen.webp",
            "screens.jpg",
            "screens.jpeg",
            "screens.png",
            "screens.webp",
        ],
    );

    let mut discovered = Artwork {
        poster,
        backdrop,
        logo,
        thumb,
    };
    let had_local = has_any_artwork(&discovered);
    let mut jellyfin_filled = false;
    let needs_jellyfin_fill = discovered.poster.is_none()
        || discovered.backdrop.is_none()
        || discovered.logo.is_none()
        || discovered.thumb.is_none();
    if needs_jellyfin_fill {
        if let Some(jellyfin_artwork) =
            find_jellyfin_item_artwork(item_kind, item_title, item_year, &media_path, art_dir).await
        {
            if discovered.poster.is_none() {
                if let Some(poster) = jellyfin_artwork.poster {
                    discovered.poster = Some(poster);
                    jellyfin_filled = true;
                }
            }
            if discovered.backdrop.is_none() {
                if let Some(backdrop) = jellyfin_artwork.backdrop {
                    discovered.backdrop = Some(backdrop);
                    jellyfin_filled = true;
                }
            }
            if discovered.logo.is_none() {
                if let Some(logo) = jellyfin_artwork.logo {
                    discovered.logo = Some(logo);
                    jellyfin_filled = true;
                }
            }
            if discovered.thumb.is_none() {
                if let Some(thumb) = jellyfin_artwork.thumb {
                    discovered.thumb = Some(thumb);
                    jellyfin_filled = true;
                }
            }
        }
    }

    debug!(
        item_id = %item_id,
        kind = %item_kind,
        has_local = had_local,
        has_jellyfin = jellyfin_filled,
        "local artwork discovery finished"
    );

    Ok(discovered)
}

pub async fn resolve_fallback_item_image_path(
    pool: &rustfin_db::DbPool,
    item_id: &str,
    item_kind: &str,
    item_title: Option<&str>,
    item_year: Option<i64>,
    image_type: &str,
) -> Option<String> {
    let artwork = find_local_item_artwork(pool, item_id, item_kind, item_title, item_year)
        .await
        .ok()?;
    match image_type {
        "poster" => artwork.poster,
        "backdrop" => artwork.backdrop,
        "logo" => artwork.logo,
        "thumb" => artwork.thumb,
        _ => None,
    }
}

fn has_any_artwork(artwork: &Artwork) -> bool {
    artwork.poster.is_some()
        || artwork.backdrop.is_some()
        || artwork.logo.is_some()
        || artwork.thumb.is_some()
}

fn jellyfin_metadata_sources() -> &'static [JellyfinMetadataSource] {
    static SOURCES: OnceLock<Vec<JellyfinMetadataSource>> = OnceLock::new();
    SOURCES
        .get_or_init(discover_jellyfin_metadata_sources)
        .as_slice()
}

fn discover_jellyfin_metadata_sources() -> Vec<JellyfinMetadataSource> {
    let mut sources = Vec::<JellyfinMetadataSource>::new();
    let mut seen = std::collections::HashSet::<String>::new();

    let mut push_source = |db_path: PathBuf, metadata_path: PathBuf| {
        if !db_path.is_file() || !metadata_path.is_dir() {
            return;
        }
        let key = format!(
            "{}::{}",
            db_path.to_string_lossy(),
            metadata_path.to_string_lossy()
        );
        if seen.insert(key) {
            sources.push(JellyfinMetadataSource {
                db_path,
                metadata_path,
            });
        }
    };

    let env_db_path = std::env::var("RUSTFIN_JELLYFIN_DB_PATH")
        .ok()
        .map(PathBuf::from);
    let env_metadata_path = std::env::var("RUSTFIN_JELLYFIN_METADATA_PATH")
        .ok()
        .map(PathBuf::from);

    if let Some(db_path) = env_db_path.clone() {
        let inferred_metadata = db_path
            .parent()
            .and_then(|data_dir| data_dir.parent())
            .map(|config_dir| config_dir.join("metadata"));
        if let Some(metadata_path) = env_metadata_path.clone().or(inferred_metadata) {
            push_source(db_path, metadata_path);
        }
    }

    if env_db_path.is_none() {
        if let Some(metadata_path) = env_metadata_path {
            if let Some(config_dir) = metadata_path.parent() {
                push_source(config_dir.join("data/library.db"), metadata_path);
            }
        }
    }

    for (db_path, metadata_path) in [
        (
            "/home/server/docker/data/media-streaming/jellyfin/config/data/library.db",
            "/home/server/docker/data/media-streaming/jellyfin/config/metadata",
        ),
        (
            "/home/server/docker/media-streaming/jellyfin/config/data/library.db",
            "/home/server/docker/media-streaming/jellyfin/config/metadata",
        ),
        (
            "/var/lib/jellyfin/data/library.db",
            "/var/lib/jellyfin/metadata",
        ),
        ("/config/data/library.db", "/config/metadata"),
    ] {
        push_source(PathBuf::from(db_path), PathBuf::from(metadata_path));
    }

    sources
}

async fn find_jellyfin_item_artwork(
    item_kind: &str,
    item_title: Option<&str>,
    item_year: Option<i64>,
    media_path: &str,
    art_dir: &Path,
) -> Option<Artwork> {
    let kind = item_kind.to_string();
    let title = item_title.map(|value| value.to_string());
    let media_path = media_path.to_string();
    let art_dir = art_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        find_jellyfin_item_artwork_blocking(
            &kind,
            title.as_deref(),
            item_year,
            &media_path,
            &art_dir,
        )
    })
    .await
    .ok()
    .flatten()
}

fn find_jellyfin_item_artwork_blocking(
    item_kind: &str,
    item_title: Option<&str>,
    item_year: Option<i64>,
    media_path: &str,
    art_dir: &Path,
) -> Option<Artwork> {
    for source in jellyfin_metadata_sources() {
        let connection =
            match Connection::open_with_flags(&source.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                Ok(conn) => conn,
                Err(err) => {
                    debug!(
                        db_path = %source.db_path.to_string_lossy(),
                        error = %err,
                        "failed to open Jellyfin library db for artwork lookup"
                    );
                    continue;
                }
            };
        let Some(images_field) = query_jellyfin_images_field(
            &connection,
            item_kind,
            item_title,
            item_year,
            media_path,
            art_dir,
        ) else {
            continue;
        };
        let parsed = parse_jellyfin_images_field(&images_field, &source.metadata_path);
        if has_any_artwork(&parsed) {
            return Some(parsed);
        }
    }
    None
}

fn query_jellyfin_images_field(
    connection: &Connection,
    item_kind: &str,
    item_title: Option<&str>,
    item_year: Option<i64>,
    media_path: &str,
    art_dir: &Path,
) -> Option<String> {
    if item_kind == "movie" {
        if let Some(images) = query_jellyfin_images_by_path_patterns(
            connection,
            "(IsMovie = 1 OR lower(type) LIKE '%movies.movie')",
            media_path,
        ) {
            return Some(images);
        }
    }

    if item_kind == "series" {
        if let Some(images) = query_jellyfin_images_by_path_patterns(
            connection,
            "(IsSeries = 1 OR lower(type) LIKE '%tv.series')",
            &art_dir.to_string_lossy(),
        ) {
            return Some(images);
        }
    }

    if item_kind == "season" {
        if let Some(images) = query_jellyfin_images_by_path_patterns(
            connection,
            "lower(type) LIKE '%tv.season'",
            &art_dir.to_string_lossy(),
        ) {
            return Some(images);
        }
    }

    if item_kind == "episode" {
        if let Some(images) = query_jellyfin_images_by_path_patterns(
            connection,
            "lower(type) LIKE '%tv.episode'",
            media_path,
        ) {
            return Some(images);
        }
    }

    if matches!(item_kind, "series" | "season" | "episode") {
        if let Some(anchor) = query_jellyfin_episode_anchor(connection, media_path) {
            if item_kind == "episode" {
                if let Some(images) = query_jellyfin_images_by_guid(connection, &anchor.guid) {
                    return Some(images);
                }
            } else if item_kind == "season" {
                if let Some(season_id) = anchor.season_id.as_deref() {
                    if let Some(images) = query_jellyfin_images_by_guid(connection, season_id) {
                        return Some(images);
                    }
                }
            } else if let Some(series_id) = anchor.series_id.as_deref() {
                if let Some(images) = query_jellyfin_images_by_guid(connection, series_id) {
                    return Some(images);
                }
            }
        }
    }

    if let Some(title) = item_title.map(str::trim).filter(|value| !value.is_empty()) {
        let by_title = if item_kind == "movie" {
            let year = item_year.unwrap_or(-1);
            connection
                .query_row(
                    "SELECT Images
                     FROM TypedBaseItems
                     WHERE (IsMovie = 1 OR lower(type) LIKE '%movies.movie')
                       AND lower(Name) = lower(?1)
                       AND Images IS NOT NULL
                       AND Images <> ''
                     ORDER BY CASE WHEN ProductionYear = ?2 THEN 0 ELSE 1 END,
                              length(COALESCE(Path, '')) ASC
                     LIMIT 1",
                    (title, year),
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
        } else if item_kind == "series" {
            connection
                .query_row(
                    "SELECT Images
                     FROM TypedBaseItems
                     WHERE (IsSeries = 1 OR lower(type) LIKE '%tv.series')
                       AND lower(Name) = lower(?1)
                       AND Images IS NOT NULL
                       AND Images <> ''
                     ORDER BY length(COALESCE(Path, '')) ASC
                     LIMIT 1",
                    (title,),
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
        } else {
            None
        };
        if by_title.is_some() {
            return by_title;
        }
    }

    for candidate in [
        media_path.to_string(),
        art_dir.to_string_lossy().to_string(),
    ] {
        if let Some(images) = connection
            .query_row(
                "SELECT Images
                 FROM TypedBaseItems
                 WHERE Path = ?1
                   AND Images IS NOT NULL
                   AND Images <> ''
                 LIMIT 1",
                (candidate,),
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
        {
            return Some(images);
        }
    }

    if let Some(file_name) = Path::new(media_path)
        .file_name()
        .and_then(|value| value.to_str())
    {
        let like_pattern = format!("%{file_name}");
        let preferred_type_match = match item_kind {
            "movie" => "IsMovie = 1 OR lower(type) LIKE '%movies.movie'",
            "series" => "IsSeries = 1 OR lower(type) LIKE '%tv.series'",
            "season" => "lower(type) LIKE '%tv.season'",
            "episode" => "lower(type) LIKE '%tv.episode'",
            _ => "0 = 1",
        };
        let by_file_name_query = format!(
            "SELECT Images
             FROM TypedBaseItems
             WHERE lower(Path) LIKE lower(?1)
               AND Images IS NOT NULL
               AND Images <> ''
             ORDER BY CASE
                          WHEN {preferred_type_match} THEN 0
                          WHEN IsMovie = 1
                            OR IsSeries = 1
                            OR lower(type) LIKE '%movies.movie'
                            OR lower(type) LIKE '%tv.series'
                            OR lower(type) LIKE '%tv.season'
                            OR lower(type) LIKE '%tv.episode'
                          THEN 1
                          ELSE 2
                      END,
                      length(COALESCE(Path, '')) ASC
             LIMIT 1"
        );
        if let Some(images) = connection
            .query_row(&by_file_name_query, (like_pattern,), |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .ok()
            .flatten()
        {
            return Some(images);
        }
    }

    if item_kind == "series" {
        if let Some(series_dir_name) = art_dir.file_name().and_then(|value| value.to_str()) {
            let like_pattern = format!("%{series_dir_name}");
            if let Some(images) = connection
                .query_row(
                    "SELECT Images
                     FROM TypedBaseItems
                     WHERE (IsSeries = 1 OR lower(type) LIKE '%tv.series')
                       AND lower(Path) LIKE lower(?1)
                       AND Images IS NOT NULL
                       AND Images <> ''
                     ORDER BY length(COALESCE(Path, '')) ASC
                     LIMIT 1",
                    (like_pattern,),
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
            {
                return Some(images);
            }
        }
    }

    None
}

fn query_jellyfin_episode_anchor(
    connection: &Connection,
    media_path: &str,
) -> Option<JellyfinEpisodeAnchor> {
    for pattern in jellyfin_path_suffix_patterns(media_path, 5) {
        let anchor = connection
            .query_row(
                "SELECT guid, SeasonId, SeriesId
                 FROM TypedBaseItems
                 WHERE lower(type) LIKE '%tv.episode'
                   AND Path IS NOT NULL
                   AND lower(Path) LIKE lower(?1) ESCAPE '\\'
                 ORDER BY length(COALESCE(Path, '')) ASC
                 LIMIT 1",
                (pattern,),
                |row| {
                    Ok(JellyfinEpisodeAnchor {
                        guid: jellyfin_guid_value_to_text(row.get_ref(0)?).unwrap_or_default(),
                        season_id: jellyfin_guid_value_to_text(row.get_ref(1)?),
                        series_id: jellyfin_guid_value_to_text(row.get_ref(2)?),
                    })
                },
            )
            .optional()
            .ok()
            .flatten();
        if let Some(anchor) = anchor.filter(|row| !row.guid.is_empty()) {
            return Some(anchor);
        }
    }
    None
}

fn query_jellyfin_images_by_guid(connection: &Connection, guid: &str) -> Option<String> {
    let trimmed = guid.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(guid_blob) = jellyfin_guid_text_to_blob(trimmed) {
        if let Some(images) = connection
            .query_row(
                "SELECT Images
                 FROM TypedBaseItems
                 WHERE (guid = ?1 OR guid = ?2)
                   AND Images IS NOT NULL
                   AND Images <> ''
                 LIMIT 1",
                (trimmed, guid_blob.as_slice()),
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
        {
            return Some(images);
        }
    }

    connection
        .query_row(
            "SELECT Images
             FROM TypedBaseItems
             WHERE guid = ?1
               AND Images IS NOT NULL
               AND Images <> ''
             LIMIT 1",
            (trimmed,),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
}

fn query_jellyfin_images_by_path_patterns(
    connection: &Connection,
    type_filter_sql: &str,
    path: &str,
) -> Option<String> {
    let sql = format!(
        "SELECT Images
         FROM TypedBaseItems
         WHERE {type_filter_sql}
           AND Path IS NOT NULL
           AND lower(Path) LIKE lower(?1) ESCAPE '\\'
           AND Images IS NOT NULL
           AND Images <> ''
         ORDER BY length(COALESCE(Path, '')) ASC
         LIMIT 1"
    );
    for pattern in jellyfin_path_suffix_patterns(path, 5) {
        if let Some(images) = connection
            .query_row(&sql, (pattern,), |row| row.get::<_, String>(0))
            .optional()
            .ok()
            .flatten()
        {
            return Some(images);
        }
    }
    None
}

fn normalize_jellyfin_guid(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn jellyfin_guid_value_to_text(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        ValueRef::Text(value) => std::str::from_utf8(value)
            .ok()
            .and_then(|text| normalize_jellyfin_guid(Some(text.to_string()))),
        ValueRef::Blob(value) => jellyfin_guid_blob_to_text(value),
        _ => None,
    }
}

fn jellyfin_guid_text_to_blob(guid: &str) -> Option<[u8; 16]> {
    let hex = guid.trim().replace('-', "");
    if hex.len() != 32 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let mut raw = [0_u8; 16];
    for idx in 0..16 {
        let start = idx * 2;
        raw[idx] = u8::from_str_radix(&hex[start..start + 2], 16).ok()?;
    }

    // Jellyfin stores GUIDs in SQLite using the .NET byte layout:
    // first 4 + next 2 + next 2 bytes are little-endian, remaining 8 are big-endian.
    let mut mapped = [0_u8; 16];
    mapped[0..4].copy_from_slice(&raw[0..4]);
    mapped[0..4].reverse();
    mapped[4..6].copy_from_slice(&raw[4..6]);
    mapped[4..6].reverse();
    mapped[6..8].copy_from_slice(&raw[6..8]);
    mapped[6..8].reverse();
    mapped[8..16].copy_from_slice(&raw[8..16]);
    Some(mapped)
}

fn jellyfin_guid_blob_to_text(value: &[u8]) -> Option<String> {
    if value.len() != 16 {
        return None;
    }

    let mut canonical = [0_u8; 16];
    canonical[0..4].copy_from_slice(&value[0..4]);
    canonical[0..4].reverse();
    canonical[4..6].copy_from_slice(&value[4..6]);
    canonical[4..6].reverse();
    canonical[6..8].copy_from_slice(&value[6..8]);
    canonical[6..8].reverse();
    canonical[8..16].copy_from_slice(&value[8..16]);

    let mut hex = String::with_capacity(32);
    for byte in canonical {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn escape_like_pattern(raw: &str) -> String {
    raw.chars()
        .flat_map(|ch| match ch {
            '%' | '_' | '\\' => vec!['\\', ch],
            _ => vec![ch],
        })
        .collect()
}

fn jellyfin_path_suffix_patterns(path: &str, max_segments: usize) -> Vec<String> {
    let max_segments = max_segments.max(1);
    let components: Vec<String> = Path::new(path)
        .components()
        .filter_map(|component| {
            let segment = component.as_os_str().to_string_lossy().trim().to_string();
            if segment.is_empty() || segment == "." || segment == "/" {
                None
            } else {
                Some(segment)
            }
        })
        .collect();
    if components.is_empty() {
        return Vec::new();
    }

    let mut seen = std::collections::HashSet::<String>::new();
    let mut patterns = Vec::new();
    let max_take = components.len().min(max_segments);
    for take in (1..=max_take).rev() {
        let tail = components[components.len() - take..].join("/");
        let escaped_tail = escape_like_pattern(&tail);
        for pattern in [format!("%/{escaped_tail}"), format!("%{escaped_tail}")] {
            if seen.insert(pattern.clone()) {
                patterns.push(pattern);
            }
        }
    }
    patterns
}

fn parse_jellyfin_images_field(images_field: &str, metadata_path: &Path) -> Artwork {
    let mut artwork = Artwork::default();
    for chunk in images_field.split('|') {
        let entry = chunk.trim();
        if entry.is_empty() {
            continue;
        }

        let mut parts = entry.split('*');
        let raw_path = parts.next().unwrap_or_default().trim();
        if raw_path.is_empty() {
            continue;
        }
        let _created_ticks = parts.next();
        let kind_token = parts.next().unwrap_or_default();

        let Some(path) = resolve_jellyfin_image_path(raw_path, metadata_path) else {
            continue;
        };
        let normalized_kind = normalize_jellyfin_image_kind(kind_token, &path);
        let path_str = path.to_string_lossy().to_string();
        match normalized_kind.as_str() {
            "poster" if artwork.poster.is_none() => artwork.poster = Some(path_str),
            "backdrop" if artwork.backdrop.is_none() => artwork.backdrop = Some(path_str),
            "logo" if artwork.logo.is_none() => artwork.logo = Some(path_str),
            "thumb" if artwork.thumb.is_none() => artwork.thumb = Some(path_str),
            _ => {}
        }
    }
    artwork
}

fn resolve_jellyfin_image_path(raw_path: &str, metadata_path: &Path) -> Option<PathBuf> {
    let expanded = raw_path.replace("%MetadataPath%", &metadata_path.to_string_lossy());
    let candidate = PathBuf::from(expanded);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

fn normalize_jellyfin_image_kind(kind_token: &str, path: &Path) -> String {
    let token = kind_token.trim().to_ascii_lowercase();
    if token.contains("primary") {
        return "poster".to_string();
    }
    if token.contains("backdrop") || token == "art" || token == "banner" {
        return "backdrop".to_string();
    }
    if token.contains("logo") {
        return "logo".to_string();
    }
    if token.contains("thumb") || token.contains("landscape") {
        return "thumb".to_string();
    }

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if stem.contains("poster") || stem.contains("folder") || stem.contains("cover") {
        "poster".to_string()
    } else if stem.contains("backdrop")
        || stem.contains("fanart")
        || stem.contains("banner")
        || stem.contains("background")
    {
        "backdrop".to_string()
    } else if stem.contains("logo") {
        "logo".to_string()
    } else {
        "thumb".to_string()
    }
}

fn find_named_file(dir: &Path, candidates: &[&str]) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut by_name = std::collections::HashMap::<String, PathBuf>::new();
    let mut searchable = Vec::<(String, String, PathBuf)>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_supported_art_file(&path) {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            let lower_name = name.to_ascii_lowercase();
            let lower_stem = Path::new(&lower_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            by_name.insert(lower_name.clone(), path.clone());
            searchable.push((lower_name, lower_stem, path));
        }
    }

    if let Some(exact) = candidates
        .iter()
        .find_map(|name| by_name.get(&name.to_ascii_lowercase()).cloned())
    {
        return Some(exact.to_string_lossy().to_string());
    }

    let keywords = candidate_keywords(candidates);
    if keywords.is_empty() {
        return None;
    }

    searchable.sort_by(|a, b| a.0.cmp(&b.0));
    for keyword in keywords {
        if let Some((_, _, path)) = searchable
            .iter()
            .find(|(_, stem, _)| stem_matches_keyword(stem, &keyword))
        {
            return Some(path.to_string_lossy().to_string());
        }
    }

    None
}

fn candidate_keywords(candidates: &[&str]) -> Vec<String> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut keywords = Vec::new();
    for candidate in candidates {
        let stem = Path::new(candidate)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if stem.is_empty() || !seen.insert(stem.clone()) {
            continue;
        }
        keywords.push(stem);
    }
    keywords
}

fn stem_matches_keyword(stem: &str, keyword: &str) -> bool {
    if stem == keyword {
        return true;
    }

    let mut token = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            token.push(ch);
            continue;
        }
        if token_matches_keyword(&token, keyword) {
            return true;
        }
        token.clear();
    }

    token_matches_keyword(&token, keyword)
}

fn token_matches_keyword(token: &str, keyword: &str) -> bool {
    !token.is_empty()
        && (token == keyword || token.starts_with(keyword) || token.ends_with(keyword))
}

fn is_supported_art_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tbn" | "avif"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        find_named_file, jellyfin_guid_blob_to_text, jellyfin_guid_text_to_blob,
        jellyfin_path_suffix_patterns, parse_jellyfin_images_field, query_jellyfin_images_field,
    };
    use rusqlite::Connection;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_art_dir(label: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time is after epoch")
            .as_nanos();
        dir.push(format!(
            "rustyfin-artwork-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp artwork dir");
        dir
    }

    fn touch(path: &Path) {
        fs::write(path, b"test").expect("write fixture file");
    }

    #[test]
    fn find_named_file_matches_exact_case_insensitive_name() {
        let dir = temp_art_dir("exact");
        let poster = dir.join("PoStEr.JPG");
        touch(&poster);

        let found = find_named_file(&dir, &["poster.jpg", "poster.png"]);

        assert_eq!(found, Some(poster.to_string_lossy().to_string()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn find_named_file_matches_keyword_sidecar_name() {
        let dir = temp_art_dir("keyword-cover");
        let cover = dir.join("Dune.Part.Two.2024-Cover.png");
        touch(&cover);

        let found = find_named_file(&dir, &["poster.jpg", "folder.jpg", "cover.jpg"]);

        assert_eq!(found, Some(cover.to_string_lossy().to_string()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn find_named_file_skips_unrelated_image_names() {
        let dir = temp_art_dir("unrelated");
        touch(&dir.join("www.YTS.MX.jpg"));

        let found = find_named_file(&dir, &["poster.jpg", "folder.jpg", "cover.jpg"]);

        assert_eq!(found, None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn find_named_file_matches_plural_keyword_tokens() {
        let dir = temp_art_dir("keyword-screens");
        let screens = dir.join("Dune.Part.Two.2024-Screens.jpg");
        touch(&screens);

        let found = find_named_file(&dir, &["thumb.jpg", "screens.jpg"]);

        assert_eq!(found, Some(screens.to_string_lossy().to_string()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parse_jellyfin_images_field_maps_primary_backdrop_logo_thumb() {
        let metadata_dir = temp_art_dir("jellyfin-parse");
        let image_dir = metadata_dir.join("library").join("ab").join("itemhash");
        fs::create_dir_all(&image_dir).expect("create nested metadata dir");

        let poster = image_dir.join("poster.jpg");
        let backdrop = image_dir.join("backdrop.jpg");
        let logo = image_dir.join("logo.png");
        let thumb = image_dir.join("landscape.jpg");
        touch(&poster);
        touch(&backdrop);
        touch(&logo);
        touch(&thumb);

        let images_field = "%MetadataPath%/library/ab/itemhash/poster.jpg*1*Primary|\
%MetadataPath%/library/ab/itemhash/backdrop.jpg*2*Backdrop|\
%MetadataPath%/library/ab/itemhash/logo.png*3*Logo|\
%MetadataPath%/library/ab/itemhash/landscape.jpg*4*Thumb";

        let parsed = parse_jellyfin_images_field(images_field, &metadata_dir);

        assert_eq!(parsed.poster, Some(poster.to_string_lossy().to_string()));
        assert_eq!(
            parsed.backdrop,
            Some(backdrop.to_string_lossy().to_string())
        );
        assert_eq!(parsed.logo, Some(logo.to_string_lossy().to_string()));
        assert_eq!(parsed.thumb, Some(thumb.to_string_lossy().to_string()));
        let _ = fs::remove_dir_all(metadata_dir);
    }

    #[test]
    fn parse_jellyfin_images_field_skips_missing_files() {
        let metadata_dir = temp_art_dir("jellyfin-missing");

        let images_field = "%MetadataPath%/library/missing/poster.jpg*1*Primary";
        let parsed = parse_jellyfin_images_field(images_field, &metadata_dir);

        assert!(parsed.poster.is_none());
        assert!(parsed.backdrop.is_none());
        assert!(parsed.logo.is_none());
        assert!(parsed.thumb.is_none());
        let _ = fs::remove_dir_all(metadata_dir);
    }

    fn create_test_typed_base_items_table(connection: &Connection) {
        connection
            .execute_batch(
                "CREATE TABLE TypedBaseItems (
                    guid TEXT PRIMARY KEY,
                    type TEXT NOT NULL,
                    ParentId TEXT,
                    Path TEXT,
                    IsMovie INTEGER,
                    IsSeries INTEGER,
                    Name TEXT,
                    ProductionYear INTEGER,
                    SeasonId TEXT,
                    SeriesId TEXT,
                    Images TEXT
                );",
            )
            .expect("create TypedBaseItems table");
    }

    #[test]
    fn jellyfin_path_suffix_patterns_returns_longest_suffix_first() {
        let patterns = jellyfin_path_suffix_patterns(
            "/mnt/truenas_media/Shows/Breaking Bad/Season 01/Breaking.Bad.S01E01.mkv",
            4,
        );
        assert_eq!(
            patterns.first().map(String::as_str),
            Some("%/Shows/Breaking Bad/Season 01/Breaking.Bad.S01E01.mkv")
        );
        assert!(
            patterns
                .iter()
                .any(|pattern| pattern == "%/Breaking Bad/Season 01/Breaking.Bad.S01E01.mkv")
        );
        assert!(
            patterns
                .iter()
                .any(|pattern| pattern == "%/Breaking.Bad.S01E01.mkv")
        );
    }

    #[test]
    fn jellyfin_guid_text_to_blob_matches_dotnet_layout() {
        let blob =
            jellyfin_guid_text_to_blob("8BDCCF02-5F49-4AEC-C3D3-56565185022D").expect("guid blob");
        assert_eq!(
            blob,
            [
                0x02, 0xcf, 0xdc, 0x8b, 0x49, 0x5f, 0xec, 0x4a, 0xc3, 0xd3, 0x56, 0x56, 0x51, 0x85,
                0x02, 0x2d
            ]
        );
    }

    #[test]
    fn jellyfin_guid_blob_to_text_round_trips_dotnet_layout() {
        let original = "8bdccf02-5f49-4aec-c3d3-56565185022d";
        let blob = jellyfin_guid_text_to_blob(original).expect("guid blob");
        let restored = jellyfin_guid_blob_to_text(&blob).expect("guid text");
        assert_eq!(restored, original);
    }

    #[test]
    fn query_jellyfin_images_field_prefers_movie_path_tail_match() {
        let connection = Connection::open_in_memory().expect("open sqlite");
        create_test_typed_base_items_table(&connection);
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, ProductionYear, Images)
                 VALUES (?1, ?2, ?3, 1, 0, ?4, 2021, ?5)",
                (
                    "movie-wrong",
                    "MediaBrowser.Controller.Entities.Movies.Movie",
                    "/media/other/Dune (2021)/Dune 2021 2160p UHD BluRay x265 10bit HDR DDP5 1 Atmos RARBG.mkv",
                    "Dune",
                    "%MetadataPath%/library/wrong/poster.jpg*1*Primary",
                ),
            )
            .expect("insert wrong movie row");
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, ProductionYear, Images)
                 VALUES (?1, ?2, ?3, 1, 0, ?4, 2021, ?5)",
                (
                    "movie-correct",
                    "MediaBrowser.Controller.Entities.Movies.Movie",
                    "/media/movies/Dune (2021)/Dune 2021 2160p UHD BluRay x265 10bit HDR DDP5 1 Atmos RARBG.mkv",
                    "Dune",
                    "%MetadataPath%/library/correct/poster.jpg*1*Primary",
                ),
            )
            .expect("insert correct movie row");

        let images = query_jellyfin_images_field(
            &connection,
            "movie",
            Some("Dune"),
            Some(2021),
            "/mnt/truenas_media/Movies/Dune (2021)/Dune 2021 2160p UHD BluRay x265 10bit HDR DDP5 1 Atmos RARBG.mkv",
            Path::new("/mnt/truenas_media/Movies/Dune (2021)"),
        );

        assert_eq!(
            images.as_deref(),
            Some("%MetadataPath%/library/correct/poster.jpg*1*Primary")
        );
    }

    #[test]
    fn query_jellyfin_images_field_uses_episode_hierarchy_for_series_and_season() {
        let connection = Connection::open_in_memory().expect("open sqlite");
        create_test_typed_base_items_table(&connection);
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, Images)
                 VALUES (?1, ?2, ?3, 0, 1, ?4, ?5)",
                (
                    "series-guid",
                    "MediaBrowser.Controller.Entities.TV.Series",
                    "/media/shows/Breaking Bad",
                    "Breaking Bad",
                    "%MetadataPath%/series/poster.jpg*1*Primary",
                ),
            )
            .expect("insert series row");
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, SeriesId, Images)
                 VALUES (?1, ?2, ?3, 0, 0, ?4, ?5, ?6)",
                (
                    "season-guid",
                    "MediaBrowser.Controller.Entities.TV.Season",
                    "/media/shows/Breaking Bad/Season 01",
                    "Season 1",
                    "series-guid",
                    "%MetadataPath%/season/poster.jpg*1*Primary",
                ),
            )
            .expect("insert season row");
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, SeasonId, SeriesId, Images)
                 VALUES (?1, ?2, ?3, 0, 0, ?4, ?5, ?6, ?7)",
                (
                    "episode-guid",
                    "MediaBrowser.Controller.Entities.TV.Episode",
                    "/media/shows/Breaking Bad/Season 01/Breaking Bad - S01E01 - Pilot.mkv",
                    "Pilot",
                    "season-guid",
                    "series-guid",
                    "%MetadataPath%/episode/thumb.jpg*1*Thumb",
                ),
            )
            .expect("insert episode row");

        let media_path =
            "/mnt/truenas_media/Shows/Breaking Bad/Season 01/Breaking Bad - S01E01 - Pilot.mkv";
        let series_images = query_jellyfin_images_field(
            &connection,
            "series",
            Some("Breaking Bad"),
            None,
            media_path,
            Path::new("/mnt/truenas_media/Shows/Breaking Bad"),
        );
        assert_eq!(
            series_images.as_deref(),
            Some("%MetadataPath%/series/poster.jpg*1*Primary")
        );

        let season_images = query_jellyfin_images_field(
            &connection,
            "season",
            Some("Season 1"),
            None,
            media_path,
            Path::new("/mnt/truenas_media/Shows/Breaking Bad/Season 01"),
        );
        assert_eq!(
            season_images.as_deref(),
            Some("%MetadataPath%/season/poster.jpg*1*Primary")
        );
    }

    #[test]
    fn query_jellyfin_images_field_reads_blob_guids_for_hierarchy_anchor() {
        let connection = Connection::open_in_memory().expect("open sqlite");
        create_test_typed_base_items_table(&connection);

        let series_guid_text = "8BDCCF02-5F49-4AEC-C3D3-56565185022D";
        let season_guid_text = "A1111111-5F49-4AEC-C3D3-56565185022D";
        let episode_guid_text = "B2222222-5F49-4AEC-C3D3-56565185022D";
        let series_guid_blob = jellyfin_guid_text_to_blob(series_guid_text).expect("series guid");
        let season_guid_blob = jellyfin_guid_text_to_blob(season_guid_text).expect("season guid");
        let episode_guid_blob =
            jellyfin_guid_text_to_blob(episode_guid_text).expect("episode guid");

        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, Images)
                 VALUES (?1, ?2, ?3, 0, 1, ?4, ?5)",
                (
                    series_guid_blob.as_slice(),
                    "MediaBrowser.Controller.Entities.TV.Series",
                    "/media/index/series/asset",
                    "Blob GUID Series",
                    "%MetadataPath%/series/blob/poster.jpg*1*Primary",
                ),
            )
            .expect("insert blob-guid series");
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, SeriesId, Images)
                 VALUES (?1, ?2, ?3, 0, 0, ?4, ?5, ?6)",
                (
                    season_guid_blob.as_slice(),
                    "MediaBrowser.Controller.Entities.TV.Season",
                    "/media/index/season/asset",
                    "Season 1",
                    series_guid_blob.as_slice(),
                    "%MetadataPath%/season/blob/poster.jpg*1*Primary",
                ),
            )
            .expect("insert blob-guid season");
        connection
            .execute(
                "INSERT INTO TypedBaseItems (guid, type, Path, IsMovie, IsSeries, Name, SeasonId, SeriesId, Images)
                 VALUES (?1, ?2, ?3, 0, 0, ?4, ?5, ?6, ?7)",
                (
                    episode_guid_blob.as_slice(),
                    "MediaBrowser.Controller.Entities.TV.Episode",
                    "/media/shows/Blob GUID Series/Season 1/S01E01 - Pilot.mkv",
                    "Pilot",
                    season_guid_blob.as_slice(),
                    series_guid_blob.as_slice(),
                    "%MetadataPath%/episode/blob/thumb.jpg*1*Thumb",
                ),
            )
            .expect("insert blob-guid episode");

        let media_path = "/mnt/truenas_media/Shows/Blob GUID Series/Season 1/S01E01 - Pilot.mkv";
        let series_images = query_jellyfin_images_field(
            &connection,
            "series",
            None,
            None,
            media_path,
            Path::new("/mnt/truenas_media/Shows/Does Not Match"),
        );
        assert_eq!(
            series_images.as_deref(),
            Some("%MetadataPath%/series/blob/poster.jpg*1*Primary")
        );

        let season_images = query_jellyfin_images_field(
            &connection,
            "season",
            None,
            None,
            media_path,
            Path::new("/mnt/truenas_media/Shows/Does Not Match"),
        );
        assert_eq!(
            season_images.as_deref(),
            Some("%MetadataPath%/season/blob/poster.jpg*1*Primary")
        );
    }
}
