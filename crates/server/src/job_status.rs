use std::time::Duration;

pub async fn update_job_status_with_retry(
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
            Err(err) => {
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        sqlx::Error::protocol("job status retry exhausted without capturing a SQLx error")
    }))
}
