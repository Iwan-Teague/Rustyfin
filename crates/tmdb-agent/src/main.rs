use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rustfin_core::error::{ApiError, ErrorEnvelope};
use rustfin_metadata::provider::{MetadataProvider, SearchResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::time::Instant;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    image_root: PathBuf,
    tmdb_key_env: Option<String>,
    agent_token: Option<String>,
    http: reqwest::Client,
}

struct AppError(ApiError);

impl From<ApiError> for AppError {
    fn from(value: ApiError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
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
    elapsed_ms: u64,
    note: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnrichOutcome {
    Updated,
    Skipped,
}

fn normalized_secret(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn verify_agent_token(headers: &HeaderMap, expected: Option<&str>) -> Result<(), AppError> {
    let Some(expected_token) = expected.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };

    let supplied = headers
        .get("x-agent-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();

    if supplied != expected_token {
        return Err(ApiError::Unauthorized("missing or invalid tmdb-agent token".into()).into());
    }

    Ok(())
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

async fn resolve_tmdb_key(state: &AppState) -> Result<Option<String>, AppError> {
    let db_key = rustfin_db::repo::settings::get(&state.db, "tmdb_api_key")
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .and_then(|value| normalized_secret(Some(&value)));

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

async fn download_poster_to_disk(
    state: &AppState,
    library_id: &str,
    item_id: &str,
    poster_url: &str,
    force: bool,
) -> Result<Option<PathBuf>, AppError> {
    let ext = guess_extension_from_url(poster_url);
    let target_dir = state.image_root.join("tmdb_agent").join(library_id);
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| ApiError::Internal(format!("create poster dir failed: {e}")))?;

    let target_path = target_dir.join(format!("{item_id}.{ext}"));

    if target_path.exists() && !force {
        return Ok(None);
    }

    let resp = state
        .http
        .get(poster_url)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("poster download failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(ApiError::BadRequest(format!(
            "poster download failed with status {}",
            resp.status()
        ))
        .into());
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::Internal(format!("poster read failed: {e}")))?;

    tokio::fs::write(&target_path, &bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("poster write failed: {e}")))?;

    Ok(Some(target_path))
}

async fn enrich_one_item(
    state: &AppState,
    library_id: &str,
    item: &rustfin_db::repo::items::ItemRow,
    tmdb_client: &rustfin_metadata::tmdb::TmdbClient,
    force: bool,
) -> Result<(EnrichOutcome, bool), AppError> {
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
        let search_results = match item.kind.as_str() {
            "movie" => tmdb_client
                .search_movie(&item.title, item_year)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB search failed: {e}")))?,
            "series" => tmdb_client
                .search_series(&item.title, item_year)
                .await
                .map_err(|e| ApiError::BadRequest(format!("TMDB search failed: {e}")))?,
            _ => Vec::new(),
        };

        pick_best_search_provider_id(&item.title, item_year, &search_results)
    };

    let Some(provider_id) = provider_id else {
        return Ok((EnrichOutcome::Skipped, false));
    };

    let provider_meta = match item.kind.as_str() {
        "movie" => tmdb_client
            .get_movie(&provider_id)
            .await
            .map_err(|e| ApiError::BadRequest(format!("TMDB movie fetch failed: {e}")))?,
        "series" => tmdb_client
            .get_series(&provider_id)
            .await
            .map_err(|e| ApiError::BadRequest(format!("TMDB series fetch failed: {e}")))?,
        _ => return Ok((EnrichOutcome::Skipped, false)),
    };

    rustfin_metadata::merge::set_provider_id(&state.db, &item.id, "tmdb", &provider_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let merge = rustfin_metadata::merge::merge_metadata(&state.db, &item.id, &provider_meta)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let mut downloaded = false;

    if let Some(poster_url) = provider_meta.poster_url.as_deref() {
        let downloaded_path =
            download_poster_to_disk(state, library_id, &item.id, poster_url, force).await?;
        if let Some(local_path) = downloaded_path {
            downloaded = true;
            let existing = rustfin_db::repo::items::get_item_artwork(&state.db, &item.id)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
                .unwrap_or((None, None, None, None));
            rustfin_db::repo::items::update_item_artwork(
                &state.db,
                &item.id,
                Some(local_path.to_string_lossy().as_ref()),
                existing.1.as_deref(),
                existing.2.as_deref(),
                existing.3.as_deref(),
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        }
    }

    let updated = !merge.updated_fields.is_empty() || downloaded;
    let outcome = if updated {
        EnrichOutcome::Updated
    } else {
        EnrichOutcome::Skipped
    };

    Ok((outcome, downloaded))
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
    verify_agent_token(&headers, state.agent_token.as_deref())?;

    let started = Instant::now();
    let force = query.force.unwrap_or(false);

    let lib = rustfin_db::repo::libraries::get_library(&state.db, &library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
        .ok_or_else(|| ApiError::NotFound("library not found".into()))?;

    let settings = rustfin_db::repo::libraries::get_library_settings(&state.db, &library_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

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
        elapsed_ms: 0,
        note: None,
        errors: Vec::new(),
    };

    if let Some(s) = settings {
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

    for item in items
        .into_iter()
        .filter(|item| item.kind == "movie" || item.kind == "series")
    {
        response.processed_items += 1;

        match enrich_one_item(&state, &library_id, &item, &tmdb_client, force).await {
            Ok((EnrichOutcome::Updated, downloaded)) => {
                response.updated_items += 1;
                if downloaded {
                    response.downloaded_posters += 1;
                }
            }
            Ok((EnrichOutcome::Skipped, _)) => {
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
    let db_path = std::env::var("RUSTFIN_DB").unwrap_or_else(|_| "/config/rustfin.db".into());
    let cache_dir = std::env::var("RUSTFIN_CACHE_DIR").unwrap_or_else(|_| "/cache".into());

    let image_root = PathBuf::from(cache_dir);
    std::fs::create_dir_all(&image_root)
        .with_context(|| format!("failed to create cache dir {}", image_root.display()))?;

    let pool = rustfin_db::connect(&db_path)
        .await
        .context("failed to connect to db")?;
    rustfin_db::migrate::run(&pool)
        .await
        .context("failed to run db migrations")?;

    let agent_token = normalized_secret(std::env::var("RUSTFIN_TMDB_AGENT_TOKEN").ok().as_deref());
    let tmdb_key_env = normalized_secret(std::env::var("RUSTFIN_TMDB_KEY").ok().as_deref());

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
