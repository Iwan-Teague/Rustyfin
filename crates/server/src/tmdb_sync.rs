use std::time::Duration;

use rustfin_core::error::ApiError;

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

    let job_id = job.id.clone();
    let pool = state.db.clone();
    let lib_id = library_id.to_string();
    let events_tx = state.events.clone();

    tokio::spawn(async move {
        if let Err(e) = update_job_status_with_retry(&pool, &job_id, "running", 0.0, None).await {
            tracing::error!(job_id = %job_id, error = %e, "failed to set TMDB sync job running");
        }
        let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
            job_id: job_id.clone(),
            status: "running".into(),
            progress: 0.0,
        });

        match run_tmdb_sync(&lib_id).await {
            Ok(_) => {
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
                tracing::error!(job_id = %job_id, library_id = %lib_id, error = %e, "TMDB sync failed");
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

    Ok(job)
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
    pool: &sqlx::SqlitePool,
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
