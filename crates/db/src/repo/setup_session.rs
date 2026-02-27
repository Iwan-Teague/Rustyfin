use crate::DbPool;

#[derive(Debug, Clone)]
pub struct SetupSessionRow {
    pub owner_token_hash: String,
    pub client_name: String,
    pub claimed_at: i64,
    pub expires_at: i64,
}

/// Get the current active setup session (if any and not expired).
pub async fn get_active(pool: &DbPool) -> Result<Option<SetupSessionRow>, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let row: Option<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT owner_token_hash, client_name, claimed_at, expires_at FROM setup_session WHERE id = 1 AND expires_at > $1",
    )
    .bind(now)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(owner_token_hash, client_name, claimed_at, expires_at)| SetupSessionRow {
            owner_token_hash,
            client_name,
            claimed_at,
            expires_at,
        },
    ))
}

/// Get session regardless of expiry (for force takeover).
pub async fn get_any(pool: &DbPool) -> Result<Option<SetupSessionRow>, sqlx::Error> {
    let row: Option<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT owner_token_hash, client_name, claimed_at, expires_at FROM setup_session WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(owner_token_hash, client_name, claimed_at, expires_at)| SetupSessionRow {
            owner_token_hash,
            client_name,
            claimed_at,
            expires_at,
        },
    ))
}

/// Claim (or force-reclaim) the setup session.
pub async fn claim(
    pool: &DbPool,
    token_hash: &str,
    client_name: &str,
    expires_at: i64,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO setup_session (id, owner_token_hash, client_name, claimed_at, expires_at) \
         VALUES (1, $1, $2, $3, $4) \
         ON CONFLICT(id) DO UPDATE SET owner_token_hash = excluded.owner_token_hash, \
         client_name = excluded.client_name, claimed_at = excluded.claimed_at, \
         expires_at = excluded.expires_at",
    )
    .bind(token_hash)
    .bind(client_name)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Release the setup session.
pub async fn release(pool: &DbPool) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM setup_session WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Refresh expiry for active session.
pub async fn refresh_expiry(pool: &DbPool, new_expires_at: i64) -> Result<bool, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    let result =
        sqlx::query("UPDATE setup_session SET expires_at = $1 WHERE id = 1 AND expires_at > $2")
            .bind(new_expires_at)
            .bind(now)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Purge expired sessions.
pub async fn purge_expired(pool: &DbPool) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("DELETE FROM setup_session WHERE expires_at <= $1")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}
