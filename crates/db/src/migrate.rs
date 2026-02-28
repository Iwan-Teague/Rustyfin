use crate::{DatabaseBackend, DbPool};
use tracing::info;

const POSTGRES_MIGRATIONS: &[(&str, &str)] = &[
    (
        "001_initial_schema",
        include_str!("../migrations_pg/001_initial_schema.sql"),
    ),
    (
        "002_metadata_columns",
        include_str!("../migrations_pg/002_metadata_columns.sql"),
    ),
    (
        "003_settings_and_setup",
        include_str!("../migrations_pg/003_settings_and_setup.sql"),
    ),
    (
        "004_user_library_access",
        include_str!("../migrations_pg/004_user_library_access.sql"),
    ),
    (
        "005_library_settings",
        include_str!("../migrations_pg/005_library_settings.sql"),
    ),
    (
        "006_watch_party",
        include_str!("../migrations_pg/006_watch_party.sql"),
    ),
    (
        "007_audio_library",
        include_str!("../migrations_pg/007_audio_library.sql"),
    ),
    (
        "008_channels",
        include_str!("../migrations_pg/008_channels.sql"),
    ),
    (
        "009_youtube_watchparty",
        include_str!("../migrations_pg/009_youtube_watchparty.sql"),
    ),
    (
        "010_web_watchparty",
        include_str!("../migrations_pg/010_web_watchparty.sql"),
    ),
    (
        "011_channel_attachments",
        include_str!("../migrations_pg/011_channel_attachments.sql"),
    ),
    (
        "012_watch_party_room_name",
        include_str!("../migrations_pg/012_watch_party_room_name.sql"),
    ),
    (
        "013_calendar",
        include_str!("../migrations_pg/013_calendar.sql"),
    ),
    (
        "014_watch_party_online_audio",
        include_str!("../migrations_pg/014_watch_party_online_audio.sql"),
    ),
    (
        "015_watch_party_create_together",
        include_str!("../migrations_pg/015_watch_party_create_together.sql"),
    ),
    (
        "016_library_tmdb_management",
        include_str!("../migrations_pg/016_library_tmdb_management.sql"),
    ),
    (
        "017_channel_transcription",
        include_str!("../migrations_pg/017_channel_transcription.sql"),
    ),
    (
        "018_query_performance_indexes",
        include_str!("../migrations_pg/018_query_performance_indexes.sql"),
    ),
    (
        "019_database_query_optimizations",
        include_str!("../migrations_pg/019_database_query_optimizations.sql"),
    ),
    (
        "020_online_audio_search_fts",
        include_str!("../migrations_pg/020_online_audio_search_fts.sql"),
    ),
    (
        "021_logs_channels_query_indexes",
        include_str!("../migrations_pg/021_logs_channels_query_indexes.sql"),
    ),
    (
        "022_transcript_query_indexes",
        include_str!("../migrations_pg/022_transcript_query_indexes.sql"),
    ),
    (
        "023_watch_party_invite_only_column",
        include_str!("../migrations_pg/023_watch_party_invite_only_column.sql"),
    ),
    (
        "024_online_audio_search_pg_indexes",
        include_str!("../migrations_pg/024_online_audio_search_pg_indexes.sql"),
    ),
    (
        "025_real_to_double_precision",
        include_str!("../migrations_pg/025_real_to_double_precision.sql"),
    ),
    (
        "026_user_profile_fields",
        include_str!("../migrations_pg/026_user_profile_fields.sql"),
    ),
    (
        "027_media_file_size_bigint",
        include_str!("../migrations_pg/027_media_file_size_bigint.sql"),
    ),
    (
        "028_upgrade_integer_columns_to_bigint",
        include_str!("../migrations_pg/028_upgrade_integer_columns_to_bigint.sql"),
    ),
];

/// Run forward-only migrations. Tracks applied migrations in a `_migrations` table.
pub async fn run(pool: &DbPool, backend: DatabaseBackend) -> Result<(), sqlx::Error> {
    let migrations = POSTGRES_MIGRATIONS;

    info!(backend = backend.as_str(), "running database migrations");

    // Create migrations tracking table.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_ts INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    for (name, sql) in migrations {
        let applied: Option<(String,)> =
            sqlx::query_as("SELECT name FROM _migrations WHERE name = $1")
                .bind(name)
                .fetch_optional(pool)
                .await?;

        if applied.is_some() {
            continue;
        }

        info!(
            backend = backend.as_str(),
            migration = name,
            "applying migration"
        );
        // Execute migration as a raw SQL script so PostgreSQL procedural blocks
        // (e.g. DO $$...$$) and semicolons inside function bodies are handled
        // correctly by the database parser.
        let mut conn = pool.acquire().await?;
        sqlx::raw_sql(sql).execute(&mut *conn).await?;
        drop(conn);

        let now = chrono::Utc::now().timestamp();
        sqlx::query("INSERT INTO _migrations (name, applied_ts) VALUES ($1, $2)")
            .bind(name)
            .bind(now)
            .execute(pool)
            .await?;

        info!(
            backend = backend.as_str(),
            migration = name,
            "migration applied"
        );
    }

    Ok(())
}
