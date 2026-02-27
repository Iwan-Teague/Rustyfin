use crate::DbPool;

/// Store an idempotency key with its payload hash and cached response.
pub async fn store(
    pool: &DbPool,
    key: &str,
    endpoint: &str,
    payload_hash: &str,
    response: &str,
    status_code: i64,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO idempotency_keys (key, endpoint, payload_hash, response, status_code, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(key)
    .bind(endpoint)
    .bind(payload_hash)
    .bind(response)
    .bind(status_code)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    pub key: String,
    pub endpoint: String,
    pub payload_hash: String,
    pub response: String,
    pub status_code: i64,
    pub created_at: i64,
}

/// Lookup an existing idempotency key.
pub async fn lookup(pool: &DbPool, key: &str) -> Result<Option<IdempotencyRecord>, sqlx::Error> {
    let row: Option<(String, String, String, String, i64, i64)> = sqlx::query_as(
        "SELECT key, endpoint, payload_hash, response, status_code, created_at FROM idempotency_keys WHERE key = $1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(key, endpoint, payload_hash, response, status_code, created_at)| IdempotencyRecord {
            key,
            endpoint,
            payload_hash,
            response,
            status_code,
            created_at,
        },
    ))
}

/// Delete all idempotency keys (used by setup reset).
pub async fn delete_all(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM idempotency_keys")
        .execute(pool)
        .await?;
    Ok(())
}
