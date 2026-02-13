use sqlx::SqlitePool;

/// Get a setting value by key.
pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// Set a setting value (upsert).
pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get multiple settings by keys. Returns a Vec of (key, value) pairs.
pub async fn get_many(
    pool: &SqlitePool,
    keys: &[&str],
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let mut results = Vec::new();
    for key in keys {
        if let Some(val) = get(pool, key).await? {
            results.push((key.to_string(), val));
        }
    }
    Ok(results)
}

/// Delete a setting.
pub async fn delete(pool: &SqlitePool, key: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete all settings (used by setup reset).
pub async fn delete_all(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM settings").execute(pool).await?;
    Ok(())
}

/// Re-insert defaults after a reset.
pub async fn insert_defaults(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let defaults = [
        ("setup_completed", "false"),
        ("setup_state", "NotStarted"),
        ("server_name", "Rustyfin"),
        ("default_ui_locale", "en"),
        ("default_region", "US"),
        ("default_time_zone", ""),
        ("metadata_language", "en"),
        ("metadata_region", "US"),
        ("allow_remote_access", "false"),
        ("enable_automatic_port_mapping", "false"),
        ("trusted_proxies", "[]"),
    ];
    for (key, value) in defaults {
        sqlx::query(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?, ?)",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
    }
    Ok(())
}
