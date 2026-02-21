use std::path::{Path, PathBuf};

use anyhow::Context;
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

async fn resolve_tmdb_api_key(pool: &sqlx::SqlitePool) -> anyhow::Result<Option<String>> {
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
    pool: &sqlx::SqlitePool,
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

        let local = find_local_item_artwork(pool, &item.id, &item.kind)
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
                let season_local = find_local_item_artwork(pool, &season.id, "season")
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
    pool: &sqlx::SqlitePool,
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
    pool: &sqlx::SqlitePool,
    item_id: &str,
    item_kind: &str,
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
            "folder.jpg",
            "folder.jpeg",
            "folder.png",
            "cover.jpg",
            "cover.jpeg",
            "cover.png",
            "season.jpg",
            "season.png",
        ],
    );
    let backdrop = find_named_file(
        art_dir,
        &[
            "backdrop.jpg",
            "backdrop.jpeg",
            "backdrop.png",
            "fanart.jpg",
            "fanart.jpeg",
            "fanart.png",
            "banner.jpg",
            "banner.jpeg",
            "banner.png",
        ],
    );
    let logo = find_named_file(
        art_dir,
        &["logo.png", "clearlogo.png", "logo.jpg", "logo.jpeg"],
    );
    let thumb = find_named_file(
        art_dir,
        &[
            "thumb.jpg",
            "thumb.jpeg",
            "thumb.png",
            "landscape.jpg",
            "landscape.jpeg",
            "landscape.png",
        ],
    );

    debug!(
        item_id = %item_id,
        kind = %item_kind,
        has_local = poster.is_some() || backdrop.is_some() || logo.is_some() || thumb.is_some(),
        "local artwork discovery finished"
    );

    Ok(Artwork {
        poster,
        backdrop,
        logo,
        thumb,
    })
}

fn find_named_file(dir: &Path, candidates: &[&str]) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut by_name = std::collections::HashMap::<String, PathBuf>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            by_name.insert(name.to_ascii_lowercase(), path);
        }
    }

    candidates
        .iter()
        .find_map(|name| by_name.get(&name.to_ascii_lowercase()).cloned())
        .map(|p| p.to_string_lossy().to_string())
}
