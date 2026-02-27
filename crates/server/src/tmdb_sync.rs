use std::time::Duration;

use rustfin_core::error::ApiError;
use tokio::sync::broadcast::Sender;

use crate::error::AppError;
use crate::state::AppState;

pub async fn enqueue_library_tmdb_sync(
    state: &AppState,
    library_id: &str,
) -> Result<rustfin_db::repo::jobs::JobRow, AppError> {
    let payload = serde_json::json!({ "library_id": library_id });
    let job = rustfin_db::repo::jobs::create_job(
        &state.db,
        "library_tmdb_sync",
        Some(&payload.to_string()),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    spawn_tmdb_sync_job(
        &state.db,
        &state.events,
        job.id.clone(),
        library_id.to_string(),
        None,
    );

    Ok(job)
}

fn tmdb_schedule_to_seconds(schedule: &str) -> Option<i64> {
    match schedule {
        "hourly" => Some(60 * 60),
        "daily" => Some(60 * 60 * 24),
        "weekly" => Some(60 * 60 * 24 * 7),
        "monthly" => Some(60 * 60 * 24 * 30),
        _ => None,
    }
}

async fn has_running_sync_job(
    pool: &rustfin_db::DbPool,
    library_id: &str,
) -> Result<bool, sqlx::Error> {
    let payload_like = format!("%\"library_id\":\"{}\"%", library_id);
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM job WHERE kind = 'library_tmdb_sync' AND status = 'running' AND payload_json LIKE $1 LIMIT 1",
    )
    .bind(payload_like)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

fn spawn_tmdb_sync_job(
    pool: &rustfin_db::DbPool,
    events: &Sender<crate::state::ServerEvent>,
    job_id: String,
    library_id: String,
    reason: Option<&str>,
) {
    let pool = pool.clone();
    let events_tx = events.clone();
    let reason = reason.map(ToOwned::to_owned);

    tokio::spawn(async move {
        if let Err(e) = update_job_status_with_retry(&pool, &job_id, "running", 0.0, None).await {
            tracing::error!(job_id = %job_id, error = %e, "failed to set TMDB sync job running");
        }
        let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
            job_id: job_id.clone(),
            status: "running".into(),
            progress: 0.0,
        });

        match run_tmdb_sync(&library_id).await {
            Ok(_) => {
                let now = chrono::Utc::now().timestamp();
                let _ =
                    rustfin_db::repo::libraries::touch_tmdb_last_sync_ts(&pool, &library_id, now)
                        .await;
                if let Err(e) =
                    update_job_status_with_retry(&pool, &job_id, "completed", 1.0, None).await
                {
                    tracing::error!(
                        job_id = %job_id,
                        error = %e,
                        "failed to set TMDB sync job completed"
                    );
                }
                let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
                    job_id,
                    status: "completed".into(),
                    progress: 1.0,
                });
            }
            Err(e) => {
                tracing::error!(
                    job_id = %job_id,
                    library_id = %library_id,
                    reason = reason.as_deref().unwrap_or("manual"),
                    error = %e,
                    "TMDB sync failed"
                );
                if let Err(update_err) =
                    update_job_status_with_retry(&pool, &job_id, "failed", 0.0, Some(&e)).await
                {
                    tracing::error!(
                        job_id = %job_id,
                        error = %update_err,
                        "failed to set TMDB sync job failed"
                    );
                }
                let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
                    job_id,
                    status: "failed".into(),
                    progress: 0.0,
                });
            }
        }
    });
}

pub async fn maybe_enqueue_post_scan_tmdb_sync(
    pool: &rustfin_db::DbPool,
    events: &Sender<crate::state::ServerEvent>,
    library_id: &str,
    library_kind: &str,
) -> Result<(), String> {
    if library_kind != "movies" && library_kind != "tv_shows" {
        return Ok(());
    }

    let settings = rustfin_db::repo::libraries::get_library_settings(pool, library_id)
        .await
        .map_err(|e| format!("failed to load library settings: {e}"))?;
    let Some(settings) = settings else {
        return Ok(());
    };

    if !settings.show_images || !settings.fetch_online_artwork || !settings.tmdb_sync_on_new_media {
        return Ok(());
    }

    if has_running_sync_job(pool, library_id)
        .await
        .map_err(|e| format!("failed to check running TMDB jobs: {e}"))?
    {
        return Ok(());
    }

    let payload = serde_json::json!({ "library_id": library_id, "reason": "new_media_detected" });
    let job =
        rustfin_db::repo::jobs::create_job(pool, "library_tmdb_sync", Some(&payload.to_string()))
            .await
            .map_err(|e| format!("failed to create TMDB sync job: {e}"))?;

    spawn_tmdb_sync_job(
        pool,
        events,
        job.id,
        library_id.to_string(),
        Some("new_media_detected"),
    );

    Ok(())
}

pub async fn run_auto_tmdb_scheduler_tick(state: &AppState) -> Result<(), String> {
    let libraries = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to list libraries: {e}"))?;
    let now = chrono::Utc::now().timestamp();

    for lib in libraries {
        if lib.kind != "movies" && lib.kind != "tv_shows" {
            continue;
        }
        let Some(settings) = rustfin_db::repo::libraries::get_library_settings(&state.db, &lib.id)
            .await
            .map_err(|e| format!("failed to read settings for {}: {e}", lib.id))?
        else {
            continue;
        };

        if !settings.show_images || !settings.fetch_online_artwork {
            continue;
        }

        let Some(interval_seconds) = tmdb_schedule_to_seconds(&settings.tmdb_sync_schedule) else {
            continue;
        };

        if has_running_sync_job(&state.db, &lib.id)
            .await
            .map_err(|e| format!("failed checking running job for {}: {e}", lib.id))?
        {
            continue;
        }

        let due = settings
            .tmdb_last_sync_ts
            .map(|last| now.saturating_sub(last) >= interval_seconds)
            .unwrap_or(true);
        if !due {
            continue;
        }

        if let Err(err) = enqueue_library_tmdb_sync(state, &lib.id).await {
            tracing::warn!(
                library_id = %lib.id,
                status = err.0.status_code(),
                "failed to enqueue scheduled TMDB sync"
            );
        }
    }

    Ok(())
}

async fn run_tmdb_sync(library_id: &str) -> Result<(), String> {
    let base_url = std::env::var("RUSTFIN_TMDB_AGENT_URL")
        .unwrap_or_else(|_| "http://rustfin-tmdb-agent:8100".to_string());
    let request_url = format!(
        "{}/enrich/library/{}",
        base_url.trim_end_matches('/'),
        library_id
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(|e| format!("failed to build TMDB sync client: {e}"))?;

    let mut request = client.post(&request_url);
    if let Some(token) = std::env::var("RUSTFIN_TMDB_AGENT_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        request = request.header("x-agent-token", token);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("TMDB sync request failed: {e}"))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read response body>".to_string());

    if !status.is_success() {
        return Err(format!(
            "TMDB sync request returned {}: {}",
            status, body_text
        ));
    }

    tracing::info!(
        library_id = %library_id,
        status = %status,
        response = %body_text,
        "TMDB sync completed via tmdb-agent"
    );

    Ok(())
}

async fn update_job_status_with_retry(
    pool: &rustfin_db::DbPool,
    job_id: &str,
    status: &str,
    progress: f64,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let mut last_err: Option<sqlx::Error> = None;
    for _ in 0..5 {
        match rustfin_db::repo::jobs::update_job_status(pool, job_id, status, progress, error).await
        {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    }
    Err(last_err.expect("last_err must be set on retry failure"))
}
