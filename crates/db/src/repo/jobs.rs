use crate::DbPool;

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
    pool: &DbPool,
    kind: &str,
    payload_json: Option<&str>,
) -> Result<JobRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO job (id, kind, status, progress, payload_json, created_ts, updated_ts) \
         VALUES ($1, $2, 'queued', 0, $3, $4, $5)",
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

pub async fn find_active_job_by_kind_and_payload(
    pool: &DbPool,
    kind: &str,
    payload_json: &str,
) -> Result<Option<JobRow>, sqlx::Error> {
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
         FROM job \
         WHERE kind = $1 \
           AND status IN ('queued', 'running') \
           AND payload_json = $2 \
         ORDER BY created_ts DESC \
         LIMIT 1",
    )
    .bind(kind)
    .bind(payload_json)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_job))
}

pub async fn list_jobs(pool: &DbPool) -> Result<Vec<JobRow>, sqlx::Error> {
    list_jobs_filtered(pool, &[], None, None, None).await
}

pub async fn list_jobs_filtered(
    pool: &DbPool,
    statuses: &[&str],
    kind: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<JobRow>, sqlx::Error> {
    let mut sql = String::from(
        "SELECT id, kind, status, progress, payload_json, error, created_ts, updated_ts FROM job",
    );
    let mut where_clauses: Vec<String> = Vec::new();
    let mut next_param = 1usize;

    if !statuses.is_empty() {
        let placeholders = crate::repo::dollar_placeholders(next_param, statuses.len());
        next_param += statuses.len();
        where_clauses.push(format!("status IN ({placeholders})"));
    }

    let normalized_kind = kind.map(str::trim).filter(|value| !value.is_empty());
    if normalized_kind.is_some() {
        where_clauses.push(format!("kind = ${next_param}"));
        next_param += 1;
    }

    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }

    sql.push_str(" ORDER BY created_ts DESC");

    let normalized_limit = limit.map(|value| value.clamp(1, 1000));
    let normalized_offset = offset.map(|value| value.clamp(0, 1_000_000)).unwrap_or(0);

    match normalized_limit {
        Some(_) => {
            sql.push_str(&format!(" LIMIT ${next_param} OFFSET ${}", next_param + 1));
            next_param += 2;
        }
        None if normalized_offset > 0 => {
            sql.push_str(&format!(" OFFSET ${next_param}"));
            next_param += 1;
        }
        None => {}
    }

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            f64,
            Option<String>,
            Option<String>,
            i64,
            i64,
        ),
    >(&sql);

    for status in statuses {
        query = query.bind(status);
    }

    if let Some(kind) = normalized_kind {
        query = query.bind(kind);
    }

    match normalized_limit {
        Some(limit) => {
            query = query.bind(limit).bind(normalized_offset);
        }
        None if normalized_offset > 0 => {
            query = query.bind(normalized_offset);
        }
        None => {}
    }

    debug_assert!(next_param > 0);

    let rows = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(row_to_job).collect())
}

pub async fn get_job(pool: &DbPool, job_id: &str) -> Result<Option<JobRow>, sqlx::Error> {
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
             FROM job WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_job))
}

pub async fn update_job_status(
    pool: &DbPool,
    job_id: &str,
    status: &str,
    progress: f64,
    error: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE job SET status = $1, progress = $2, error = $3, updated_ts = $4 WHERE id = $5",
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

/// Delete a job log entry. Only permitted for terminal states (completed, failed, cancelled, error).
/// Returns true if a row was deleted, false if not found or still active.
pub async fn delete_job(pool: &DbPool, job_id: &str) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM job WHERE id = $1 AND status NOT IN ('queued', 'running')")
            .bind(job_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Cancel a job (only if queued or running).
pub async fn cancel_job(pool: &DbPool, job_id: &str) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query(
        "UPDATE job SET status = 'cancelled', updated_ts = $1 \
         WHERE id = $2 AND status IN ('queued', 'running')",
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
