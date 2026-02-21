use sqlx::SqlitePool;
use tracing::info;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_initial_schema",
        include_str!("../migrations/001_initial_schema.sql"),
    ),
    (
        "002_metadata_columns",
        include_str!("../migrations/002_metadata_columns.sql"),
    ),
    (
        "003_settings_and_setup",
        include_str!("../migrations/003_settings_and_setup.sql"),
    ),
    (
        "004_user_library_access",
        include_str!("../migrations/004_user_library_access.sql"),
    ),
    (
        "005_library_settings",
        include_str!("../migrations/005_library_settings.sql"),
    ),
];

/// Run forward-only migrations. Tracks applied migrations in a `_migrations` table.
pub async fn run(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Create migrations tracking table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_ts INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    for (name, sql) in MIGRATIONS {
        let applied: Option<(String,)> =
            sqlx::query_as("SELECT name FROM _migrations WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await?;

        if applied.is_some() {
            continue;
        }

        info!(migration = name, "applying migration");
        // Execute migration statements (split on semicolons for multi-statement)
        for statement in sql.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            sqlx::query(trimmed).execute(pool).await?;
        }

        let now = chrono::Utc::now().timestamp();
        sqlx::query("INSERT INTO _migrations (name, applied_ts) VALUES (?, ?)")
            .bind(name)
            .bind(now)
            .execute(pool)
            .await?;

        info!(migration = name, "migration applied");
    }

    Ok(())
}
