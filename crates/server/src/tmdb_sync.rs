use std::collections::HashMap;
use std::time::Duration;

use rustfin_core::error::ApiError;

use crate::error::AppError;
use crate::job_status::update_job_status_with_retry;
use crate::state::AppState;

pub async fn enqueue_library_tmdb_sync(
    state: &AppState,
    library_id: &str,
) -> Result<rustfin_db::repo::jobs::JobRow, AppError> {
    let payload = serde_json::json!({ "library_id": library_id });
    let payload_json = payload.to_string();
    if let Some(existing) = rustfin_db::repo::jobs::find_active_job_by_kind_and_payload(
        &state.db,
        "library_tmdb_sync",
        &payload_json,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    {
        return Ok(existing);
    }

    let job =
        rustfin_db::repo::jobs::create_job(&state.db, "library_tmdb_sync", Some(&payload_json))
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    spawn_tmdb_sync_job(state.clone(), job.id.clone(), library_id.to_string(), None);

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

fn spawn_tmdb_sync_job(state: AppState, job_id: String, library_id: String, reason: Option<&str>) {
    let pool = state.db.clone();
    let events_tx = state.events.clone();
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

        match run_tmdb_sync(&state, &library_id).await {
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
    state: &AppState,
    library_id: &str,
    library_kind: &str,
) -> Result<(), String> {
    if library_kind != "movies" && library_kind != "tv_shows" {
        return Ok(());
    }

    let settings = rustfin_db::repo::libraries::get_library_settings(&state.db, library_id)
        .await
        .map_err(|e| format!("failed to load library settings: {e}"))?;
    let Some(settings) = settings else {
        return Ok(());
    };

    if !settings.show_images || !settings.fetch_online_artwork || !settings.tmdb_sync_on_new_media {
        return Ok(());
    }

    enqueue_library_tmdb_sync(state, library_id)
        .await
        .map(|_| ())
        .map_err(|e| format!("failed to create TMDB sync job: {}", e.0))
}

pub async fn run_auto_tmdb_scheduler_tick(state: &AppState) -> Result<(), String> {
    let libraries = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to list libraries: {e}"))?;
    let now = chrono::Utc::now().timestamp();
    let media_library_ids = libraries
        .iter()
        .filter(|lib| lib.kind == "movies" || lib.kind == "tv_shows")
        .map(|lib| lib.id.clone())
        .collect::<Vec<_>>();
    let settings_by_library_id = rustfin_db::repo::libraries::get_library_settings_for_libraries(
        &state.db,
        &media_library_ids,
    )
    .await
    .map_err(|e| format!("failed to read TMDB library settings batch: {e}"))?
    .into_iter()
    .map(|settings| (settings.library_id.clone(), settings))
    .collect::<HashMap<_, _>>();

    for lib in libraries {
        if lib.kind != "movies" && lib.kind != "tv_shows" {
            continue;
        }
        let Some(settings) = settings_by_library_id.get(&lib.id) else {
            continue;
        };

        if !settings.show_images || !settings.fetch_online_artwork {
            continue;
        }

        let Some(interval_seconds) = tmdb_schedule_to_seconds(&settings.tmdb_sync_schedule) else {
            continue;
        };

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

async fn run_tmdb_sync(state: &AppState, library_id: &str) -> Result<(), String> {
    let base_url = state.tmdb_agent_url.trim_end_matches('/');
    let request_url = format!("{}/enrich/library/{}", base_url, library_id);

    let mut request = state
        .http
        .post(&request_url)
        .timeout(Duration::from_secs(900));
    if let Some(token) = state.tmdb_agent_token.as_ref().filter(|s| !s.is_empty()) {
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
