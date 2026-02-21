use std::time::Duration;

use rustfin_core::error::ApiError;

use crate::error::AppError;
use crate::state::AppState;

pub async fn enqueue_library_scan(
    state: &AppState,
    library_id: &str,
    library_kind: &str,
) -> Result<rustfin_db::repo::jobs::JobRow, AppError> {
    let payload = serde_json::json!({ "library_id": library_id });
    let job =
        rustfin_db::repo::jobs::create_job(&state.db, "library_scan", Some(&payload.to_string()))
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // Spawn scan in background.
    let job_id = job.id.clone();
    let pool = state.db.clone();
    let lib_id = library_id.to_string();
    let lib_kind = library_kind.to_string();
    let events_tx = state.events.clone();
    tokio::spawn(async move {
        if let Err(e) = update_job_status_with_retry(&pool, &job_id, "running", 0.0, None).await {
            tracing::error!(job_id = %job_id, error = %e, "failed to set job status to running");
        }
        let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
            job_id: job_id.clone(),
            status: "running".into(),
            progress: 0.0,
        });

        match rustfin_scanner::scan::run_library_scan(&pool, &lib_id, &lib_kind).await {
            Ok(result) => {
                if let Err(err) =
                    crate::artwork::enrich_library_artwork(&pool, &lib_id, &lib_kind).await
                {
                    tracing::warn!(
                        library_id = %lib_id,
                        error = %err,
                        "scan completed but artwork enrichment failed"
                    );
                }
                tracing::info!(
                    job_id = %job_id,
                    added = result.added,
                    skipped = result.skipped,
                    "scan completed"
                );
                if let Err(e) =
                    update_job_status_with_retry(&pool, &job_id, "completed", 1.0, None).await
                {
                    tracing::error!(
                        job_id = %job_id,
                        error = %e,
                        "failed to set job status to completed"
                    );
                }
                let _ = events_tx.send(crate::state::ServerEvent::ScanComplete {
                    library_id: lib_id,
                    job_id: job_id.clone(),
                    items_added: result.added as u64,
                });
                let _ = events_tx.send(crate::state::ServerEvent::JobUpdate {
                    job_id,
                    status: "completed".into(),
                    progress: 1.0,
                });
            }
            Err(e) => {
                tracing::error!(job_id = %job_id, error = %e, "scan failed");
                if let Err(update_err) = update_job_status_with_retry(
                    &pool,
                    &job_id,
                    "failed",
                    0.0,
                    Some(&e.to_string()),
                )
                .await
                {
                    tracing::error!(
                        job_id = %job_id,
                        error = %update_err,
                        "failed to set job status to failed"
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
