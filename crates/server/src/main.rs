use anyhow::Context;
use std::path::Path;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

fn probe_binary(path: &Path, name: &str) {
    match std::process::Command::new(path).arg("-version").output() {
        Ok(out) if out.status.success() => {
            tracing::info!(binary = %name, path = %path.display(), "binary available");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(
                binary = %name,
                path = %path.display(),
                status = %out.status,
                stderr = %stderr.trim(),
                "binary check failed; playback/transcoding may fail"
            );
        }
        Err(err) => {
            tracing::warn!(
                binary = %name,
                path = %path.display(),
                error = %err,
                "binary is not executable or missing; playback/transcoding may fail"
            );
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Database target: prefer URL-based config, then fall back to legacy DB path.
    let db_target = std::env::var("RUSTFIN_DATABASE_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| {
            std::env::var("RUSTFIN_DB").unwrap_or_else(|_| "rustfin.db".to_string())
        });
    let db_target_log =
        if db_target.starts_with("postgres://") || db_target.starts_with("postgresql://") {
            "postgres (credentials redacted)"
        } else {
            db_target.as_str()
        };
    info!(db_target = %db_target_log, "connecting to database");

    let pool = rustfin_db::connect(&db_target)
        .await
        .context("failed to connect to database")?;
    let db_backend = rustfin_db::detect_backend(&db_target);
    let run_migrations = std::env::var("RUSTFIN_RUN_MIGRATIONS")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true);
    if db_backend == rustfin_db::DatabaseBackend::Postgres {
        warn!(
            "PostgreSQL backend is experimental in this transition phase; SQLite-oriented query paths may still require follow-up conversion."
        );
    }

    if run_migrations {
        rustfin_db::migrate::run(&pool, db_backend)
            .await
            .context("failed to run migrations")?;
        info!("migrations complete");
    } else {
        warn!("RUSTFIN_RUN_MIGRATIONS disabled; assuming schema is pre-migrated");
    }

    // Ensure setup defaults exist (idempotent)
    rustfin_db::repo::settings::insert_defaults(&pool)
        .await
        .context("failed to ensure setup defaults")?;

    // Auto-migrate: if users already exist but setup not completed, mark setup as completed
    // (handles existing installs that pre-date the setup wizard)
    let user_count = rustfin_db::repo::users::count_users(&pool)
        .await
        .context("failed to count users")?;

    if user_count > 0 {
        let setup_completed = rustfin_db::repo::settings::get(&pool, "setup_completed")
            .await
            .context("failed to read setup_completed")?
            .unwrap_or_else(|| "false".to_string());

        if setup_completed != "true" {
            rustfin_db::repo::settings::set(&pool, "setup_completed", "true")
                .await
                .context("failed to auto-set setup_completed")?;
            rustfin_db::repo::settings::set(&pool, "setup_state", "Completed")
                .await
                .context("failed to auto-set setup_state")?;
            info!("auto-migrated existing install to setup_completed=true");
        }
    }

    // JWT secret: use env or generate random
    let jwt_secret =
        std::env::var("RUSTFIN_JWT_SECRET").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let youtube_agent_url = std::env::var("RUSTFIN_YOUTUBE_AGENT_URL")
        .unwrap_or_else(|_| "http://rustfin-youtube-agent:8101".to_string());
    let youtube_agent_token = std::env::var("RUSTFIN_YOUTUBE_AGENT_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let transcription_agent_url = std::env::var("RUSTFIN_TRANSCRIPTION_AGENT_URL")
        .unwrap_or_else(|_| "http://rustfin-transcription-agent:8102".to_string());
    let transcription_agent_token = std::env::var("RUSTFIN_TRANSCRIPTION_AGENT_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Transcoder config
    let transcode_dir = std::env::var("RUSTFIN_TRANSCODE_DIR")
        .unwrap_or_else(|_| "/tmp/rustfin_transcode".to_string());
    let max_transcodes: usize = std::env::var("RUSTFIN_MAX_TRANSCODES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let ffmpeg_path = std::env::var("RUSTFIN_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());
    let ffprobe_path =
        std::env::var("RUSTFIN_FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());

    let tc_config = rustfin_transcoder::TranscoderConfig {
        ffmpeg_path: ffmpeg_path.clone().into(),
        ffprobe_path: ffprobe_path.clone().into(),
        transcode_dir: transcode_dir.into(),
        max_concurrent: max_transcodes,
        ..Default::default()
    };

    probe_binary(Path::new(&ffmpeg_path), "ffmpeg");
    probe_binary(Path::new(&ffprobe_path), "ffprobe");

    let session_mgr =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    // Spawn idle session cleanup task
    {
        let mgr = session_mgr.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                mgr.cleanup_idle().await;
            }
        });
    }

    // Cache directory
    let cache_dir: std::path::PathBuf = std::env::var("RUSTFIN_CACHE_DIR")
        .unwrap_or_else(|_| "/tmp/rustfin_cache".to_string())
        .into();
    std::fs::create_dir_all(&cache_dir).context("failed to create cache dir")?;
    let watch_party_audio_dir = cache_dir.join("watch_party_audio");
    std::fs::create_dir_all(&watch_party_audio_dir)
        .context("failed to create watch-party audio dir")?;

    // Event broadcast channel
    let (events_tx, _) = tokio::sync::broadcast::channel::<rustfin_server::state::ServerEvent>(256);

    // Spawn heartbeat emitter
    {
        let tx = events_tx.clone();
        tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let _ = tx.send(rustfin_server::state::ServerEvent::Heartbeat { seq });
                seq += 1;
            }
        });
    }

    let app_state = rustfin_server::state::AppState {
        db: pool,
        jwt_secret,
        youtube_agent_url,
        youtube_agent_token,
        transcription_agent_url,
        transcription_agent_token,
        transcoder: session_mgr,
        ffmpeg_path: std::path::PathBuf::from(&ffmpeg_path),
        ffprobe_path: std::path::PathBuf::from(&ffprobe_path),
        cache_dir,
        watch_party_audio_dir: watch_party_audio_dir.clone(),
        events: events_tx,
        watch_party: std::sync::Arc::new(
            rustfin_server::watch_party::manager::WatchPartyManager::new(),
        ),
        channel_manager: std::sync::Arc::new(
            rustfin_server::channels::manager::ChannelManager::new(),
        ),
    };

    // Spawn watch-party empty-lobby cleanup task.
    {
        let watch_party = app_state.watch_party.clone();
        let db = app_state.db.clone();
        let watch_party_audio_dir = watch_party_audio_dir.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                watch_party
                    .cleanup_empty_lobbies(&db, &watch_party_audio_dir)
                    .await;
            }
        });
    }

    // Spawn periodic TMDB auto-sync scheduler for libraries that opt in.
    {
        let tmdb_state = app_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                if let Err(err) =
                    rustfin_server::tmdb_sync::run_auto_tmdb_scheduler_tick(&tmdb_state).await
                {
                    tracing::warn!(error = %err, "tmdb auto-sync scheduler tick failed");
                }
            }
        });
    }

    let app = rustfin_server::routes::build_router(app_state);

    let bind_addr = std::env::var("RUSTFIN_BIND").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .context("failed to bind")?;
    info!(addr = %bind_addr, "server listening");

    axum::serve(listener, app).await?;
    Ok(())
}
