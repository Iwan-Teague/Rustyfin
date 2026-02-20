use anyhow::Context;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // DB path: use RUSTFIN_DB env or default
    let db_path = std::env::var("RUSTFIN_DB").unwrap_or_else(|_| "rustfin.db".to_string());
    info!(db_path = %db_path, "connecting to database");

    let pool = rustfin_db::connect(&db_path)
        .await
        .context("failed to connect to database")?;

    // Run migrations
    rustfin_db::migrate::run(&pool)
        .await
        .context("failed to run migrations")?;
    info!("migrations complete");

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

    // Transcoder config
    let transcode_dir = std::env::var("RUSTFIN_TRANSCODE_DIR")
        .unwrap_or_else(|_| "/tmp/rustfin_transcode".to_string());
    let max_transcodes: usize = std::env::var("RUSTFIN_MAX_TRANSCODES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: transcode_dir.into(),
        max_concurrent: max_transcodes,
        ..Default::default()
    };
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
        transcoder: session_mgr,
        cache_dir,
        events: events_tx,
    };

    let app = rustfin_server::routes::build_router(app_state);

    let bind_addr = std::env::var("RUSTFIN_BIND").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .context("failed to bind")?;
    info!(addr = %bind_addr, "server listening");

    axum::serve(listener, app).await?;
    Ok(())
}
