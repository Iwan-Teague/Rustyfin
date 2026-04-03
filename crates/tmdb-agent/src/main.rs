use anyhow::{Context, bail};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    routing::{get, post},
};
use rustfin_core::agent_auth::{normalize_secret, verify_agent_token};
use rustfin_core::axum_error::AppError;
use rustfin_core::error::ApiError;
use rustfin_db::DbPool;
use rustfin_metadata::provider::{MetadataProvider, SearchResult};
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};
use std::time::Instant;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    db: DbPool,
    image_root: PathBuf,
    tmdb_key_env: Option<String>,
    agent_token: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize, Default)]
struct EnrichQuery {
    force: Option<bool>,
}

#[derive(Debug, Serialize)]
struct EnrichLibraryResponse {
    library_id: String,
    library_name: String,
    scan_added: u64,
    scan_skipped: u64,
    processed_items: u64,
    updated_items: u64,
    skipped_items: u64,
    failed_items: u64,
    downloaded_posters: u64,
    downloaded_backdrops: u64,
    reviews_updated: u64,
    elapsed_ms: u64,
    note: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnrichOutcome {
    Updated,
    Skipped,
}

#[derive(Debug, Clone, Copy)]
enum ImageSlot {
    Poster,
    Backdrop,
}

impl ImageSlot {
    fn as_str(self) -> &'static str {
        match self {
            Self::Poster => "poster",
            Self::Backdrop => "backdrop",
        }
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

    if let Some(year) = item_year
        && let Some(hit) = results.iter().find(|hit| {
            normalize_title_for_match(&hit.title) == normalized_item_title && hit.year == Some(year)
        })
    {
        return Some(hit.provider_id.clone());
    }

    if let Some(hit) = results
        .iter()
        .find(|hit| normalize_title_for_match(&hit.title) == normalized_item_title)
    {
        return Some(hit.provider_id.clone());
    }

    if let Some(year) = item_year
        && let Some(hit) = results.iter().find(|hit| hit.year == Some(year))
    {
        return Some(hit.provider_id.clone());
    }

    results.first().map(|hit| hit.provider_id.clone())
}

async fn resolve_tmdb_key(state: &AppState) -> Result<Option<String>, AppError> {
    let db_key = rustfin_db::repo::settings::get(&state.db, "tmdb_api_key")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .and_then(|value| normalize_secret(Some(&value)));

    if db_key.is_some() {
        return Ok(db_key);
    }

    Ok(state.tmdb_key_env.clone())
}

fn guess_extension_from_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        "png"
    } else if lower.contains(".webp") {
        "webp"
    } else {
        "jpg"
    }
}

async fn resolve_item_art_dir(
    state: &AppState,
    item_id: &str,
    item_kind: &str,
    store_in_media_dir: bool,
    library_id: &str,
) -> Result<PathBuf, AppError> {
    if !store_in_media_dir {
        let target = state
            .image_root
            .join("tmdb_agent")
            .join(library_id)
            .join(item_id);
        std::fs::create_dir_all(&target)
            .map_err(|e| ApiError::Internal(format!("create artwork dir failed: {e}")))?;
        return Ok(target);
    }

    let direct_media_path = rustfin_db::repo::items::get_item_media_path(&state.db, item_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let media_path = if let Some(path) = direct_media_path {
        Some(path)
    } else {
        rustfin_db::repo::items::get_first_descendant_media_path(&state.db, item_id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    };

    let Some(media_path) = media_path else {
        return Err(ApiError::BadRequest("item has no media file mapping yet".into()).into());
    };
    let media = PathBuf::from(&media_path);
    let Some(parent_dir) = media.parent() else {
        return Err(ApiError::BadRequest("item media path has no parent directory".into()).into());
    };
    let art_dir = if item_kind == "series" {
        parent_dir.parent().unwrap_or(parent_dir).to_path_buf()
    } else {
        parent_dir.to_path_buf()
    };
    std::fs::create_dir_all(&art_dir)
        .map_err(|e| ApiError::Internal(format!("create media artwork dir failed: {e}")))?;
    Ok(art_dir)
}

fn find_existing_local_artwork(dir: &StdPath, slot: ImageSlot) -> Option<PathBuf> {
    let names: &[&str] = match slot {
        ImageSlot::Poster => &[
            "poster.jpg",
            "poster.jpeg",
            "poster.png",
            "folder.jpg",
            "folder.jpeg",
            "folder.png",
            "cover.jpg",
            "cover.jpeg",
            "cover.png",
        ],
        ImageSlot::Backdrop => &[
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
    };
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.exists() && path.is_file())
}

async fn download_image_to_disk(
    state: &AppState,
    target_dir: &StdPath,
    image_url: &str,
    slot: ImageSlot,
    force: bool,
) -> Result<Option<PathBuf>, AppError> {
    let ext = guess_extension_from_url(image_url);
    let target_path = target_dir.join(format!("{}.{}", slot.as_str(), ext));

    if target_path.exists() && !force {
        return Ok(None);
    }

    let resp = state
        .http
        .get(image_url)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("{} download failed: {e}", slot.as_str())))?;

    if !resp.status().is_success() {
        return Err(ApiError::BadRequest(format!(
            "{} download failed with status {}",
            slot.as_str(),
            resp.status()
        ))
        .into());
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(format!("{} read failed: {e}", slot.as_str())))?;

    tokio::fs::write(&target_path, &bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("{} write failed: {e}", slot.as_str())))?;

    Ok(Some(target_path))
}

async fn upsert_tmdb_reviews(
    pool: &DbPool,
    item_id: &str,
    reviews: &serde_json::Value,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO item_tmdb_review (item_id, reviews_json, updated_ts) \
         VALUES ($1, $2, $3) \
         ON CONFLICT(item_id) DO UPDATE SET \
            reviews_json = excluded.reviews_json, \
            updated_ts = excluded.updated_ts",
    )
    .bind(item_id)
    .bind(reviews.to_string())
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(())
}

async fn enrich_one_item(
    state: &AppState,
    library_id: &str,
    library_kind: &str,
    settings: &rustfin_db::repo::libraries::LibrarySettingsRow,
    item: &rustfin_db::repo::items::ItemRow,
    tmdb_client: &rustfin_metadata::tmdb::TmdbClient,
    force: bool,
) -> Result<(EnrichOutcome, bool, bool, bool), AppError> {
    let expects_movie = library_kind == "movies";
    let expects_series = library_kind == "tv_shows";
    if (expects_movie && item.kind != "movie") || (expects_series && item.kind != "series") {
        return Ok((EnrichOutcome::Skipped, false, false, false));
    }

    let item_year = item.year.map(|y| y as i32);

    let existing_tmdb_id = rustfin_metadata::merge::get_provider_ids(&state.db, &item.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .into_iter()
        .find_map(|(provider, value)| {
            if provider.eq_ignore_ascii_case("tmdb") {
                Some(value)
            } else {
                None
            }
        });

    let provider_id = if let Some(existing) = existing_tmdb_id {
        Some(existing)
    } else {
        let search_results = if expects_movie {
            tmdb_client
                .search_movie(&item.title, item_year)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB movie search failed: {e}")))?
        } else if expects_series {
            tmdb_client
                .search_series(&item.title, item_year)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB series search failed: {e}")))?
        } else {
            Vec::new()
        };

        pick_best_search_provider_id(&item.title, item_year, &search_results)
    };

    let Some(provider_id) = provider_id else {
        return Ok((EnrichOutcome::Skipped, false, false, false));
    };

    let mut provider_meta = if expects_movie {
        tmdb_client
            .get_movie(&provider_id)
            .await
            .map_err(|e| ApiError::BadRequest(format!("TMDB movie fetch failed: {e}")))?
    } else if expects_series {
        tmdb_client
            .get_series(&provider_id)
            .await
            .map_err(|e| ApiError::BadRequest(format!("TMDB series fetch failed: {e}")))?
    } else {
        return Ok((EnrichOutcome::Skipped, false, false, false));
    };

    rustfin_metadata::merge::set_provider_id(&state.db, &item.id, "tmdb", &provider_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !settings.tmdb_fetch_metadata {
        provider_meta.title = None;
        provider_meta.original_title = None;
        provider_meta.sort_title = None;
        provider_meta.overview = None;
        provider_meta.tagline = None;
        provider_meta.year = None;
        provider_meta.premiere_date = None;
        provider_meta.end_date = None;
        provider_meta.runtime_minutes = None;
        provider_meta.community_rating = None;
        provider_meta.official_rating = None;
        provider_meta.genres = None;
        provider_meta.studios = None;
        provider_meta.people = None;
    }
    if !settings.tmdb_fetch_posters {
        provider_meta.poster_url = None;
    }
    if !settings.tmdb_fetch_backdrops {
        provider_meta.backdrop_url = None;
    }

    let merge = rustfin_metadata::merge::merge_metadata(&state.db, &item.id, &provider_meta)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut downloaded_poster = false;
    let mut downloaded_backdrop = false;
    let mut reviews_updated = false;

    let art_dir = resolve_item_art_dir(
        state,
        &item.id,
        &item.kind,
        settings.tmdb_store_in_media_dir,
        library_id,
    )
    .await?;

    let existing_art = rustfin_db::repo::items::get_item_artwork(&state.db, &item.id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .unwrap_or((None, None, None, None));

    let mut next_poster = existing_art.0.clone();
    let mut next_backdrop = existing_art.1.clone();

    if settings.tmdb_fetch_posters {
        if let Some(local_existing) = find_existing_local_artwork(&art_dir, ImageSlot::Poster) {
            next_poster = Some(local_existing.to_string_lossy().to_string());
        } else if let Some(poster_url) = provider_meta.poster_url.as_deref()
            && let Some(downloaded_path) =
                download_image_to_disk(state, &art_dir, poster_url, ImageSlot::Poster, force)
                    .await?
        {
            downloaded_poster = true;
            next_poster = Some(downloaded_path.to_string_lossy().to_string());
        }
    }

    if settings.tmdb_fetch_backdrops {
        if let Some(local_existing) = find_existing_local_artwork(&art_dir, ImageSlot::Backdrop) {
            next_backdrop = Some(local_existing.to_string_lossy().to_string());
        } else if let Some(backdrop_url) = provider_meta.backdrop_url.as_deref()
            && let Some(downloaded_path) =
                download_image_to_disk(state, &art_dir, backdrop_url, ImageSlot::Backdrop, force)
                    .await?
        {
            downloaded_backdrop = true;
            next_backdrop = Some(downloaded_path.to_string_lossy().to_string());
        }
    }

    if next_poster != existing_art.0 || next_backdrop != existing_art.1 {
        rustfin_db::repo::items::update_item_artwork(
            &state.db,
            &item.id,
            next_poster.as_deref(),
            next_backdrop.as_deref(),
            existing_art.2.as_deref(),
            existing_art.3.as_deref(),
        )
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    }

    if settings.tmdb_fetch_reviews {
        let reviews = if expects_movie {
            tmdb_client
                .get_movie_reviews(&provider_id)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB reviews fetch failed: {e}")))?
        } else {
            tmdb_client
                .get_series_reviews(&provider_id)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB reviews fetch failed: {e}")))?
        };
        upsert_tmdb_reviews(&state.db, &item.id, &reviews).await?;
        reviews_updated = true;
    }

    let updated = !merge.updated_fields.is_empty()
        || downloaded_poster
        || downloaded_backdrop
        || reviews_updated;
    let outcome = if updated {
        EnrichOutcome::Updated
    } else {
        EnrichOutcome::Skipped
    };

    Ok((
        outcome,
        downloaded_poster,
        downloaded_backdrop,
        reviews_updated,
    ))
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, AppError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::Internal(format!("database check failed: {e}")))?;

    Ok(Json(HealthResponse { status: "ok" }))
}

async fn enrich_library(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(library_id): Path<String>,
    Query(query): Query<EnrichQuery>,
) -> Result<Json<EnrichLibraryResponse>, AppError> {
    verify_agent_token(&headers, state.agent_token.as_deref(), "tmdb-agent")?;

    let started = Instant::now();
    let force = query.force.unwrap_or(false);

    let lib = rustfin_db::repo::libraries::get_library(&state.db, &library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;

    let settings = rustfin_db::repo::libraries::get_library_settings(&state.db, &library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let s = settings.unwrap_or(rustfin_db::repo::libraries::LibrarySettingsRow {
        library_id: library_id.clone(),
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

    let mut response = EnrichLibraryResponse {
        library_id: library_id.clone(),
        library_name: lib.name.clone(),
        scan_added: 0,
        scan_skipped: 0,
        processed_items: 0,
        updated_items: 0,
        skipped_items: 0,
        failed_items: 0,
        downloaded_posters: 0,
        downloaded_backdrops: 0,
        reviews_updated: 0,
        elapsed_ms: 0,
        note: None,
        errors: Vec::new(),
    };

    if !s.show_images {
        response.note = Some("library has artwork thumbnails disabled".to_string());
        response.elapsed_ms = started.elapsed().as_millis() as u64;
        return Ok(Json(response));
    }
    if !s.fetch_online_artwork {
        response.note = Some("library has online artwork fetching disabled".to_string());
        response.elapsed_ms = started.elapsed().as_millis() as u64;
        return Ok(Json(response));
    }

    let scan = rustfin_scanner::scan::run_library_scan(&state.db, &library_id, &lib.kind)
        .await
        .map_err(|e| ApiError::Internal(format!("library scan failed before TMDB sync: {e}")))?;
    response.scan_added = scan.added as u64;
    response.scan_skipped = scan.skipped as u64;

    let tmdb_key = resolve_tmdb_key(&state).await?.ok_or_else(|| {
        ApiError::BadRequest(
            "TMDB API key is not configured. Set it in Admin > TMDB Metadata or RUSTFIN_TMDB_KEY"
                .into(),
        )
    })?;

    let tmdb_client = rustfin_metadata::tmdb::TmdbClient::new(tmdb_key);

    let items = rustfin_db::repo::items::get_library_items(&state.db, &library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    for item in items {
        response.processed_items += 1;

        match enrich_one_item(
            &state,
            &library_id,
            &lib.kind,
            &s,
            &item,
            &tmdb_client,
            force,
        )
        .await
        {
            Ok((
                EnrichOutcome::Updated,
                downloaded_poster,
                downloaded_backdrop,
                reviews_updated,
            )) => {
                response.updated_items += 1;
                if downloaded_poster {
                    response.downloaded_posters += 1;
                }
                if downloaded_backdrop {
                    response.downloaded_backdrops += 1;
                }
                if reviews_updated {
                    response.reviews_updated += 1;
                }
            }
            Ok((EnrichOutcome::Skipped, _, _, _)) => {
                response.skipped_items += 1;
            }
            Err(err) => {
                response.failed_items += 1;
                warn!(
                    library_id = %library_id,
                    item_id = %item.id,
                    item_title = %item.title,
                    error = %err.0,
                    "tmdb-agent failed to enrich item"
                );
                response
                    .errors
                    .push(format!("{} ({}) -> {}", item.title, item.id, err.0));
            }
        }
    }

    response.elapsed_ms = started.elapsed().as_millis() as u64;

    info!(
        library_id = %library_id,
        scan_added = response.scan_added,
        scan_skipped = response.scan_skipped,
        processed = response.processed_items,
        updated = response.updated_items,
        skipped = response.skipped_items,
        failed = response.failed_items,
        downloaded_posters = response.downloaded_posters,
        downloaded_backdrops = response.downloaded_backdrops,
        reviews_updated = response.reviews_updated,
        elapsed_ms = response.elapsed_ms,
        "tmdb-agent enrich library completed"
    );

    Ok(Json(response))
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/enrich/library/{id}", post(enrich_library))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind = std::env::var("RUSTFIN_TMDB_AGENT_BIND").unwrap_or_else(|_| "0.0.0.0:8100".into());
    let db_target = std::env::var("RUSTFIN_DATABASE_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "postgresql://rustfin:rustfin@postgres:5432/rustfin".to_string());
    let db_target_lc = db_target.to_ascii_lowercase();
    if !db_target_lc.starts_with("postgres://") && !db_target_lc.starts_with("postgresql://") {
        bail!(
            "RUSTFIN_DATABASE_URL must be a PostgreSQL URL (postgres:// or postgresql://); non-PostgreSQL targets are not supported"
        );
    }
    let cache_dir = std::env::var("RUSTFIN_CACHE_DIR").unwrap_or_else(|_| "/cache".into());

    let image_root = PathBuf::from(cache_dir);
    std::fs::create_dir_all(&image_root)
        .with_context(|| format!("failed to create cache dir {}", image_root.display()))?;

    let pool = rustfin_db::connect(&db_target)
        .await
        .context("failed to connect to db")?;
    let db_backend = rustfin_db::DatabaseBackend::Postgres;
    let run_migrations = std::env::var("RUSTFIN_RUN_MIGRATIONS")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true);
    if run_migrations {
        rustfin_db::migrate::run(&pool, db_backend)
            .await
            .context("failed to run db migrations")?;
    } else {
        warn!(
            "RUSTFIN_RUN_MIGRATIONS disabled for tmdb-agent service; assuming schema is pre-migrated"
        );
    }

    let agent_token = normalize_secret(std::env::var("RUSTFIN_TMDB_AGENT_TOKEN").ok().as_deref());
    let tmdb_key_env = normalize_secret(std::env::var("RUSTFIN_TMDB_KEY").ok().as_deref());

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("rustfin-tmdb-agent/1.0")
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/json,image/*,*/*"),
            );
            headers
        })
        .build()
        .context("failed to build http client")?;

    let app_state = AppState {
        db: pool,
        image_root,
        tmdb_key_env,
        agent_token,
        http,
    };

    let app = build_router(app_state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("failed to bind {bind}"))?;

    info!(addr = %bind, "tmdb-agent listening");
    axum::serve(listener, app).await?;
    Ok(())
}
