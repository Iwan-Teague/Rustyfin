use anyhow::{Context, bail};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

fn auto_hw_accel_candidates(
    caps: &rustfin_transcoder::gpu::GpuCapabilities,
) -> Vec<rustfin_transcoder::HwAccel> {
    let mut candidates = Vec::new();
    if caps.nvenc {
        candidates.push(rustfin_transcoder::HwAccel::Nvenc);
    }
    if caps.vaapi {
        candidates.push(rustfin_transcoder::HwAccel::Vaapi);
    }
    if caps.qsv {
        candidates.push(rustfin_transcoder::HwAccel::Qsv);
    }
    if caps.videotoolbox {
        candidates.push(rustfin_transcoder::HwAccel::VideoToolbox);
    }
    candidates
}

async fn select_first_working_hw_accel(
    ffmpeg_path: &Path,
    candidates: Vec<rustfin_transcoder::HwAccel>,
) -> (Option<rustfin_transcoder::HwAccel>, Option<PathBuf>) {
    for candidate in candidates {
        match rustfin_transcoder::gpu::probe_runtime(ffmpeg_path, &candidate).await {
            Ok(device_path) => {
                info!(
                    selected = ?candidate,
                    hw_device = ?device_path,
                    "transcoder hardware acceleration runtime probe succeeded"
                );
                return (Some(candidate), device_path);
            }
            Err(err) => {
                warn!(
                    candidate = ?candidate,
                    error = %err,
                    "transcoder hardware acceleration runtime probe failed; trying next candidate"
                );
            }
        }
    }
    (None, None)
}

fn parse_hw_accel_mode(mode: &str) -> Option<rustfin_transcoder::HwAccel> {
    match mode {
        "nvenc" | "cuda" => Some(rustfin_transcoder::HwAccel::Nvenc),
        "vaapi" => Some(rustfin_transcoder::HwAccel::Vaapi),
        "qsv" => Some(rustfin_transcoder::HwAccel::Qsv),
        "videotoolbox" | "video_toolbox" => Some(rustfin_transcoder::HwAccel::VideoToolbox),
        _ => None,
    }
}

fn parse_env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

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

async fn wait_for_shutdown_signal(shutdown: CancellationToken) {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result {
                            warn!(error = %error, "failed to listen for ctrl-c shutdown signal");
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                warn!(error = %error, "failed to install SIGTERM handler; falling back to ctrl-c only");
                if let Err(ctrl_c_error) = tokio::signal::ctrl_c().await {
                    warn!(error = %ctrl_c_error, "failed to listen for ctrl-c shutdown signal");
                }
            }
        }
    }

    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!(error = %error, "failed to listen for ctrl-c shutdown signal");
    }

    info!("shutdown signal received");
    shutdown.cancel();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Database target: Postgres-only.
    let db_target = std::env::var("RUSTFIN_DATABASE_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| "postgresql://rustfin:rustfin@postgres:5432/rustfin".to_string());
    let db_target_lc = db_target.to_ascii_lowercase();
    if !db_target_lc.starts_with("postgres://") && !db_target_lc.starts_with("postgresql://") {
        bail!(
            "RUSTFIN_DATABASE_URL must be a PostgreSQL URL (postgres:// or postgresql://); non-PostgreSQL targets are not supported"
        );
    }
    let db_target_log = "postgres (credentials redacted)";
    info!(db_target = %db_target_log, "connecting to database");

    let pool = rustfin_db::connect(&db_target)
        .await
        .context("failed to connect to database")?;
    let db_backend = rustfin_db::DatabaseBackend::Postgres;
    let run_migrations = std::env::var("RUSTFIN_RUN_MIGRATIONS")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true);
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
    let outbound_http = reqwest::Client::builder()
        .user_agent(format!("Rustyfin/{}", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("failed to build shared outbound HTTP client")?;
    let tmdb_agent_url = std::env::var("RUSTFIN_TMDB_AGENT_URL")
        .unwrap_or_else(|_| "http://rustfin-tmdb-agent:8100".to_string());
    let tmdb_agent_token = std::env::var("RUSTFIN_TMDB_AGENT_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
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
    let servers_agent_url = std::env::var("RUSTFIN_SERVERS_AGENT_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let servers_agent_token = std::env::var("RUSTFIN_SERVERS_AGENT_TOKEN")
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
    let transcode_idle_timeout_secs: u64 = std::env::var("RUSTFIN_TRANSCODE_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 60)
        .unwrap_or(30 * 60);
    let ffmpeg_path = std::env::var("RUSTFIN_FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());
    let ffprobe_path =
        std::env::var("RUSTFIN_FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());
    let hw_accel_mode_raw =
        std::env::var("RUSTFIN_TRANSCODER_HW_ACCEL").unwrap_or_else(|_| "auto".to_string());
    let hw_accel_mode = hw_accel_mode_raw.trim().to_ascii_lowercase();
    let require_hw_accel = parse_env_bool("RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL", false);

    let (selected_hw_accel, selected_hw_device_path) = match hw_accel_mode.as_str() {
        "" | "auto" => {
            let caps = rustfin_transcoder::gpu::detect(Path::new(&ffmpeg_path)).await;
            let (selected, hw_device) = select_first_working_hw_accel(
                Path::new(&ffmpeg_path),
                auto_hw_accel_candidates(&caps),
            )
            .await;
            info!(
                mode = "auto",
                ?caps,
                selected = ?selected,
                hw_device = ?hw_device,
                "transcoder hardware acceleration auto-detected"
            );
            (selected, hw_device)
        }
        "none" | "off" | "cpu" | "disabled" => {
            info!(
                mode = %hw_accel_mode,
                "transcoder hardware acceleration explicitly disabled"
            );
            (None, None)
        }
        _ => {
            if let Some(accel) = parse_hw_accel_mode(&hw_accel_mode) {
                match rustfin_transcoder::gpu::probe_runtime(Path::new(&ffmpeg_path), &accel).await
                {
                    Ok(hw_device) => {
                        info!(
                            mode = %hw_accel_mode,
                            selected = ?accel,
                            hw_device = ?hw_device,
                            "transcoder hardware acceleration configured from environment"
                        );
                        (Some(accel), hw_device)
                    }
                    Err(err) => {
                        warn!(
                            mode = %hw_accel_mode,
                            error = %err,
                            "requested hardware acceleration is not available in this runtime; using CPU"
                        );
                        (None, None)
                    }
                }
            } else {
                warn!(
                    mode = %hw_accel_mode,
                    "unknown RUSTFIN_TRANSCODER_HW_ACCEL value; falling back to auto detection"
                );
                let caps = rustfin_transcoder::gpu::detect(Path::new(&ffmpeg_path)).await;
                let (selected, hw_device) = select_first_working_hw_accel(
                    Path::new(&ffmpeg_path),
                    auto_hw_accel_candidates(&caps),
                )
                .await;
                info!(
                    mode = "auto-fallback",
                    ?caps,
                    selected = ?selected,
                    hw_device = ?hw_device,
                    "transcoder hardware acceleration auto-detected"
                );
                (selected, hw_device)
            }
        }
    };
    if require_hw_accel && selected_hw_accel.is_none() {
        bail!(
            "RUSTFIN_TRANSCODER_REQUIRE_HW_ACCEL is enabled but no usable hardware transcoder was detected (mode={hw_accel_mode})"
        );
    }

    let tc_config = rustfin_transcoder::TranscoderConfig {
        ffmpeg_path: ffmpeg_path.clone().into(),
        ffprobe_path: ffprobe_path.clone().into(),
        transcode_dir: transcode_dir.into(),
        max_concurrent: max_transcodes,
        idle_timeout_secs: transcode_idle_timeout_secs,
        hw_accel: selected_hw_accel.clone(),
        hw_device_path: selected_hw_device_path.clone(),
        ..Default::default()
    };

    probe_binary(Path::new(&ffmpeg_path), "ffmpeg");
    probe_binary(Path::new(&ffprobe_path), "ffprobe");

    let session_mgr =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
    let shutdown = CancellationToken::new();
    let mut background_tasks: Vec<JoinHandle<()>> = Vec::new();

    // Spawn idle session cleanup task
    {
        let mgr = session_mgr.clone();
        let task_shutdown = shutdown.clone();
        background_tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(20)) => {
                        mgr.cleanup_idle().await;
                    }
                }
            }
        }));
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
        let task_shutdown = shutdown.clone();
        background_tasks.push(tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {
                        let _ = tx.send(rustfin_server::state::ServerEvent::Heartbeat { seq });
                        seq += 1;
                    }
                }
            }
        }));
    }

    let app_state = rustfin_server::state::AppState {
        db: pool,
        jwt_secret,
        http: outbound_http,
        runtime_metrics: rustfin_server::runtime_metrics::RuntimeMetrics::new(),
        tmdb_agent_url,
        tmdb_agent_token,
        youtube_agent_url,
        youtube_agent_token,
        transcription_agent_url,
        transcription_agent_token,
        servers_agent_url,
        servers_agent_token,
        transcoder: session_mgr,
        ffmpeg_path: std::path::PathBuf::from(&ffmpeg_path),
        ffprobe_path: std::path::PathBuf::from(&ffprobe_path),
        transcoder_hw_accel: selected_hw_accel.clone(),
        transcoder_hw_accel_required: require_hw_accel,
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
        let task_shutdown = shutdown.clone();
        background_tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(30)) => {
                        watch_party
                            .cleanup_empty_lobbies(&db, &watch_party_audio_dir)
                            .await;
                    }
                }
            }
        }));
    }

    // Spawn periodic TMDB auto-sync scheduler for libraries that opt in.
    {
        let tmdb_state = app_state.clone();
        let task_shutdown = shutdown.clone();
        background_tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        if let Err(err) =
                            rustfin_server::tmdb_sync::run_auto_tmdb_scheduler_tick(&tmdb_state).await
                        {
                            tracing::warn!(error = %err, "tmdb auto-sync scheduler tick failed");
                        }
                    }
                }
            }
        }));
    }

    let app = rustfin_server::routes::build_router(app_state);

    let bind_addr = std::env::var("RUSTFIN_BIND").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .context("failed to bind")?;
    info!(addr = %bind_addr, "server listening");

    let serve_result = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_for_shutdown_signal(shutdown.clone()))
    .await;

    shutdown.cancel();
    for task in background_tasks {
        let _ = task.await;
    }
    serve_result?;
    Ok(())
}
