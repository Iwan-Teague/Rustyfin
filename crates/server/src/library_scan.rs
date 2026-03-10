use rustfin_core::error::ApiError;

use crate::error::AppError;
use crate::job_status::update_job_status_with_retry;
use crate::state::AppState;

pub async fn enqueue_library_scan(
    state: &AppState,
    library_id: &str,
    library_kind: &str,
) -> Result<rustfin_db::repo::jobs::JobRow, AppError> {
    let payload = serde_json::json!({ "library_id": library_id });
    let payload_json = payload.to_string();
    if let Some(existing) = rustfin_db::repo::jobs::find_active_job_by_kind_and_payload(
        &state.db,
        "library_scan",
        &payload_json,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
    {
        return Ok(existing);
    }

    let job = rustfin_db::repo::jobs::create_job(&state.db, "library_scan", Some(&payload_json))
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    // Spawn scan in background.
    let job_id = job.id.clone();
    let pool = state.db.clone();
    let lib_id = library_id.to_string();
    let lib_kind = library_kind.to_string();
    let events_tx = state.events.clone();
    let state = state.clone();
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
                if result.added > 0 {
                    let maybe_sync = crate::tmdb_sync::maybe_enqueue_post_scan_tmdb_sync(
                        &state, &lib_id, &lib_kind,
                    )
                    .await;
                    if let Err(err) = maybe_sync {
                        tracing::warn!(
                            library_id = %lib_id,
                            error = %err,
                            "scan completed but TMDB auto-sync enqueue failed"
                        );
                    }
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
