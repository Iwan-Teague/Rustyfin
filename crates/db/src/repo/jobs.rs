use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub progress: f64,
    pub payload_json: Option<String>,
    pub error: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub async fn create_job(
    pool: &SqlitePool,
    kind: &str,
    payload_json: Option<&str>,
) -> Result<JobRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO job (id, kind, status, progress, payload_json, created_ts, updated_ts) \
         VALUES (?, ?, 'queued', 0, ?, ?, ?)",
    )
    .bind(&id)
    .bind(kind)
    .bind(payload_json)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(JobRow {
        id,
        kind: kind.to_string(),
        status: "queued".to_string(),
        progress: 0.0,
        payload_json: payload_json.map(String::from),
        error: None,
        created_ts: now,
        updated_ts: now,
    })
}

pub async fn list_jobs(pool: &SqlitePool) -> Result<Vec<JobRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        f64,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, kind, status, progress, payload_json, error, created_ts, updated_ts \
             FROM job ORDER BY created_ts DESC",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_job).collect())
}

pub async fn get_job(pool: &SqlitePool, job_id: &str) -> Result<Option<JobRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        f64,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, kind, status, progress, payload_json, error, created_ts, updated_ts \
             FROM job WHERE id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_job))
}

pub async fn update_job_status(
    pool: &SqlitePool,
    job_id: &str,
    status: &str,
    progress: f64,
    error: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE job SET status = ?, progress = ?, error = ?, updated_ts = ? WHERE id = ?",
    )
    .bind(status)
    .bind(progress)
    .bind(error)
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Cancel a job (only if queued or running).
pub async fn cancel_job(pool: &SqlitePool, job_id: &str) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE job SET status = 'cancelled', updated_ts = ? \
         WHERE id = ? AND status IN ('queued', 'running')",
    )
    .bind(now)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

fn row_to_job(
    r: (
        String,
        String,
        String,
        f64,
        Option<String>,
        Option<String>,
        i64,
        i64,
    ),
) -> JobRow {
    JobRow {
        id: r.0,
        kind: r.1,
        status: r.2,
        progress: r.3,
        payload_json: r.4,
        error: r.5,
        created_ts: r.6,
        updated_ts: r.7,
    }
}
