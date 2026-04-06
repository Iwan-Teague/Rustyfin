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
    (
        "029_servers_minecraft",
        include_str!("../migrations_pg/029_servers_minecraft.sql"),
    ),
    (
        "030_channel_message_sort_seq",
        include_str!("../migrations_pg/030_channel_message_sort_seq.sql"),
    ),
    (
        "031_denormalized_counters",
        include_str!("../migrations_pg/031_denormalized_counters.sql"),
    ),
    (
        "032_setup_table_bigint_alignment",
        include_str!("../migrations_pg/032_setup_table_bigint_alignment.sql"),
    ),
    (
        "033_upgrade_current_schema_integer_columns",
        include_str!("../migrations_pg/033_upgrade_current_schema_integer_columns.sql"),
    ),
    (
        "034_job_active_payload_indexes",
        include_str!("../migrations_pg/034_job_active_payload_indexes.sql"),
    ),
    (
        "035_continue_watching_indexes",
        include_str!("../migrations_pg/035_continue_watching_indexes.sql"),
    ),
    (
        "036_user_account_activity",
        include_str!("../migrations_pg/036_user_account_activity.sql"),
    ),
    ("037_vault", include_str!("../migrations_pg/037_vault.sql")),
    (
        "038_vault_refresh_tokens",
        include_str!("../migrations_pg/038_vault_refresh_tokens.sql"),
    ),
    (
        "039_rustyvault_schema_rename",
        include_str!("../migrations_pg/039_rustyvault_schema_rename.sql"),
    ),
    (
        "040_rustyvault_preferences",
        include_str!("../migrations_pg/040_rustyvault_preferences.sql"),
    ),
    (
        "041_ai_assistant_audit",
        include_str!("../migrations_pg/041_ai_assistant_audit.sql"),
    ),
    (
        "042_downloads_artifacts",
        include_str!("../migrations_pg/042_downloads_artifacts.sql"),
    ),
    (
        "043_backups",
        include_str!("../migrations_pg/043_backups.sql"),
    ),
    (
        "044_ai_conversations",
        include_str!("../migrations_pg/044_ai_conversations.sql"),
    ),
    (
        "045_ai_assistant_confirmation",
        include_str!("../migrations_pg/045_ai_assistant_confirmation.sql"),
    ),
    (
        "046_ai_generated_artifacts",
        include_str!("../migrations_pg/046_ai_generated_artifacts.sql"),
    ),
    (
        "047_ai_conversation_groups_order",
        include_str!("../migrations_pg/047_ai_conversation_groups_order.sql"),
    ),
    (
        "048_ai_conversation_memory",
        include_str!("../migrations_pg/048_ai_conversation_memory.sql"),
    ),
    (
        "049_ai_turn_journal_and_compaction",
        include_str!("../migrations_pg/049_ai_turn_journal_and_compaction.sql"),
    ),
    (
        "046_ai_retrieval_and_memory",
        include_str!("../migrations_pg/046_ai_retrieval_and_memory.sql"),
    ),
    (
        "050_ai_retrieval_and_memory",
        include_str!("../migrations_pg/050_ai_retrieval_and_memory.sql"),
    ),
    (
        "047_ai_model_benchmarks",
        include_str!("../migrations_pg/047_ai_model_benchmarks.sql"),
    ),
    (
        "051_ai_planner_audit",
        include_str!("../migrations_pg/051_ai_planner_audit.sql"),
    ),
    (
        "052_ai_tasks",
        include_str!("../migrations_pg/052_ai_tasks.sql"),
    ),
    (
        "053_rustyvault_password_generator_defaults",
        include_str!("../migrations_pg/053_rustyvault_password_generator_defaults.sql"),
    ),
];

const POSTGRES_MIGRATION_LOCK_ID: i64 = 0x7275737466696e;

/// Run forward-only migrations. Tracks applied migrations in a `_migrations` table.
pub async fn run(pool: &DbPool, backend: DatabaseBackend) -> Result<(), sqlx::Error> {
    let migrations = POSTGRES_MIGRATIONS;

    info!(backend = backend.as_str(), "running database migrations");

    let mut conn = pool.acquire().await?;
    if backend == DatabaseBackend::Postgres {
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(POSTGRES_MIGRATION_LOCK_ID)
            .execute(&mut *conn)
            .await?;
    }

    let result = async {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_ts INTEGER NOT NULL
            )",
        )
        .execute(&mut *conn)
        .await?;

        for (name, sql) in migrations {
            let applied: Option<(String,)> =
                sqlx::query_as("SELECT name FROM _migrations WHERE name = $1")
                    .bind(name)
                    .fetch_optional(&mut *conn)
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
            sqlx::raw_sql(sql).execute(&mut *conn).await?;

            let now = chrono::Utc::now().timestamp();
            sqlx::query("INSERT INTO _migrations (name, applied_ts) VALUES ($1, $2)")
                .bind(name)
                .bind(now)
                .execute(&mut *conn)
                .await?;

            info!(
                backend = backend.as_str(),
                migration = name,
                "migration applied"
            );
        }

        Ok(())
    }
    .await;

    if backend == DatabaseBackend::Postgres {
        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(POSTGRES_MIGRATION_LOCK_ID)
            .execute(&mut *conn)
            .await;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::POSTGRES_MIGRATIONS;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    #[test]
    fn migration_registry_covers_all_sql_files() {
        let migrations_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations_pg");
        let registered = POSTGRES_MIGRATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();

        let mut discovered = BTreeSet::new();
        for entry in fs::read_dir(&migrations_dir).expect("read migrations directory") {
            let entry = entry.expect("read migration dir entry");
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("sql migration file stem");
            discovered.insert(stem.to_owned());
        }

        let missing = discovered
            .into_iter()
            .filter(|name| !registered.contains(name.as_str()))
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "migration files missing from POSTGRES_MIGRATIONS: {missing:?}"
        );
    }
}
