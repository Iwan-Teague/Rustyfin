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

    // Bootstrap admin if no users exist
    let user_count = rustfin_db::repo::users::count_users(&pool)
        .await
        .context("failed to count users")?;

    if user_count == 0 {
        let admin_pass =
            std::env::var("RUSTFIN_ADMIN_PASSWORD").unwrap_or_else(|_| "admin".to_string());
        rustfin_db::repo::users::create_user(&pool, "admin", &admin_pass, "admin")
            .await
            .context("failed to bootstrap admin user")?;
        info!("admin user bootstrapped (username: admin)");
    }

    // JWT secret: use env or generate random
    let jwt_secret = std::env::var("RUSTFIN_JWT_SECRET")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

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
