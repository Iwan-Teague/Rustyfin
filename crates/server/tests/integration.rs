use axum_test::{TestServer, TestWebSocket};
use rustfin_server::routes::build_router;
use rustfin_server::state::AppState;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static POSTGRES_TEST_SCHEMA_COUNTER: AtomicU64 = AtomicU64::new(0);

fn localhost_header() -> (axum::http::HeaderName, axum::http::HeaderValue) {
    (
        axum::http::header::HOST,
        axum::http::HeaderValue::from_static("localhost"),
    )
}

fn test_database_target() -> String {
    std::env::var("RUSTFIN_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("RUSTFIN_DATABASE_URL"))
        .unwrap_or_else(|_| {
            panic!(
                "RUSTFIN_TEST_DATABASE_URL or RUSTFIN_DATABASE_URL is required for integration tests"
            )
        })
}

fn build_schema_db_url(base_url: &str, schema_name: &str) -> String {
    let options_param = format!("options=-c%20search_path%3D{schema_name}");
    if base_url.contains('?') {
        format!("{base_url}&{options_param}")
    } else {
        format!("{base_url}?{options_param}")
    }
}

async fn create_test_pool() -> rustfin_db::DbPool {
    let target = test_database_target();
    let backend = rustfin_db::detect_backend(&target);
    if backend == rustfin_db::DatabaseBackend::Postgres
        && !target.to_ascii_lowercase().contains("test")
        && std::env::var("RUSTFIN_TEST_DB_ALLOW_ANY").ok().as_deref() != Some("1")
    {
        panic!(
            "Refusing to run integration tests against non-test PostgreSQL URL: {target}. \
Set RUSTFIN_TEST_DB_ALLOW_ANY=1 to bypass."
        );
    }

    let isolated_target = if backend == rustfin_db::DatabaseBackend::Postgres {
        let admin_pool = rustfin_db::connect(&target).await.unwrap();
        let schema_index = POSTGRES_TEST_SCHEMA_COUNTER.fetch_add(1, Ordering::Relaxed);
        let schema_name = format!("rustfin_it_{}_{}", std::process::id(), schema_index);
        sqlx::query(&format!("CREATE SCHEMA {schema_name}"))
            .execute(&admin_pool)
            .await
            .unwrap();
        build_schema_db_url(&target, &schema_name)
    } else {
        target.clone()
    };

    let pool = rustfin_db::connect(&isolated_target).await.unwrap();
    rustfin_db::migrate::run(&pool, backend).await.unwrap();
    pool
}

fn build_test_state(
    pool: rustfin_db::DbPool,
    transcoder: std::sync::Arc<rustfin_transcoder::session::SessionManager>,
    ffmpeg_path: PathBuf,
    ffprobe_path: PathBuf,
    events_tx: tokio::sync::broadcast::Sender<rustfin_server::state::ServerEvent>,
    cache_dir: PathBuf,
    watch_party_audio_dir: PathBuf,
) -> AppState {
    AppState {
        db: pool,
        rustyvault: rustfin_server::state::RustyVaultRuntimeState::available(),
        jwt_secret: "test-secret-key".to_string(),
        http: reqwest::Client::builder().build().unwrap(),
        runtime_metrics: rustfin_server::runtime_metrics::RuntimeMetrics::new(),
        tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
        tmdb_agent_token: None,
        youtube_agent_url: "http://127.0.0.1:8101".to_string(),
        youtube_agent_token: None,
        transcription_agent_url: "http://127.0.0.1:8102".to_string(),
        transcription_agent_token: None,
        servers_agent_url: None,
        servers_agent_token: None,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        transcoder_hw_accel: None,
        transcoder_hw_accel_required: false,
        cache_dir,
        watch_party_audio_dir,
        events: events_tx,
        watch_party: std::sync::Arc::new(
            rustfin_server::watch_party::manager::WatchPartyManager::new(),
        ),
        channel_manager: std::sync::Arc::new(
            rustfin_server::channels::manager::ChannelManager::new(),
        ),
    }
}

/// Create a test server using the configured test database target.
async fn test_app() -> TestServer {
    let pool = create_test_pool().await;

    // Ensure setup defaults exist
    rustfin_db::repo::settings::insert_defaults(&pool)
        .await
        .unwrap();

    // Bootstrap admin user and mark setup as completed for existing tests
    rustfin_db::repo::users::create_user(&pool, "admin", "admin_secure_123", "admin")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_completed", "true")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_state", "Completed")
        .await
        .unwrap();

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: std::env::temp_dir().join(format!("rf_test_{}", std::process::id())),
        max_concurrent: 2,
        ..Default::default()
    };
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = build_test_state(
        pool,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        events_tx,
        std::env::temp_dir().join(format!("rf_cache_{}", std::process::id())),
        std::env::temp_dir().join(format!("rf_watch_audio_{}", std::process::id())),
    );

    let app = build_router(state);
    TestServer::new(app).unwrap()
}

async fn test_app_http() -> TestServer {
    let pool = create_test_pool().await;

    rustfin_db::repo::settings::insert_defaults(&pool)
        .await
        .unwrap();

    rustfin_db::repo::users::create_user(&pool, "admin", "admin_secure_123", "admin")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_completed", "true")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_state", "Completed")
        .await
        .unwrap();

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: std::env::temp_dir().join(format!("rf_test_{}", std::process::id())),
        max_concurrent: 2,
        ..Default::default()
    };
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = build_test_state(
        pool,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        events_tx,
        std::env::temp_dir().join(format!("rf_cache_{}", std::process::id())),
        std::env::temp_dir().join(format!("rf_watch_audio_{}", std::process::id())),
    );

    let app = build_router(state);
    for _ in 0..12 {
        if let Ok(server) = TestServer::builder().http_transport().build(app.clone()) {
            return server;
        }
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    }

    panic!("failed to create HTTP transport test server for websocket tests");
}

/// Helper: login and return JWT token.
async fn login(server: &TestServer, username: &str, password: &str) -> String {
    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": username, "password": password }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    body["token"].as_str().unwrap().to_string()
}

async fn create_user_as_admin(
    server: &TestServer,
    admin_token: &str,
    username: &str,
    password: &str,
) -> String {
    let resp = server
        .post("/api/v1/users")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {admin_token}")
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "username": username,
            "password": password,
            "role": "user",
            "library_ids": [],
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    body["id"].as_str().unwrap().to_string()
}

async fn create_vault_session(server: &TestServer, auth_token: &str, device_name: &str) -> Value {
    let resp = server
        .post("/api/v1/vault/device-sessions/pair")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {auth_token}")
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "client_kind": "rustyvault_web",
            "device_name": device_name,
            "device_platform": "integration-test",
        }))
        .await;
    resp.assert_status_ok();
    resp.json::<Value>()
}

async fn bootstrap_vault_for_user_with_access(
    server: &TestServer,
    auth_token: &str,
    vault_access_token: &str,
) {
    let mut request = server.post("/api/v1/vault/bootstrap");
    for (name, value) in auth_and_vault_headers(auth_token, vault_access_token).await {
        request = request.add_header(name, value);
    }
    let resp = request
        .json(&json!({
            "wrapped_key": {
                "key_version": 1,
                "kdf_algorithm": "argon2id",
                "kdf_memory_kib": 65536,
                "kdf_iterations": 3,
                "kdf_parallelism": 4,
                "kdf_salt_hex": "00112233445566778899aabbccddeeff",
                "hkdf_algorithm": "hkdf-sha-256",
                "wrap_algorithm": "aes-256-gcm",
                "wrap_nonce_hex": "00112233445566778899aabb",
                "wrapped_vault_key_hex": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                "created_ts": 0
            }
        }))
        .await;
    resp.assert_status_ok();
}

async fn bootstrap_vault_for_user(server: &TestServer, auth_token: &str) {
    let session = create_vault_session(server, auth_token, "Bootstrap Vault").await;
    let access_token = session["session"]["access_token"].as_str().unwrap();
    bootstrap_vault_for_user_with_access(server, auth_token, access_token).await;
}

async fn auth_and_vault_headers(
    auth_token: &str,
    vault_access_token: &str,
) -> Vec<(axum::http::HeaderName, axum::http::HeaderValue)> {
    vec![
        (
            axum::http::header::AUTHORIZATION,
            format!("Bearer {auth_token}")
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        ),
        (
            axum::http::HeaderName::from_static("x-rustyvault-access"),
            vault_access_token
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        ),
    ]
}

async fn create_vault_item(
    server: &TestServer,
    auth_token: &str,
    vault_access_token: &str,
    item_id: &str,
    match_hash_hex: &str,
) {
    let headers = auth_and_vault_headers(auth_token, vault_access_token).await;
    let mut request = server.post("/api/v1/vault/items");
    for (name, value) in headers {
        request = request.add_header(name, value);
    }
    let resp = request
        .json(&json!({
            "id": item_id,
            "item_type": "login",
            "key_version": 1,
            "summary_version": 1,
            "summary_nonce_hex": "00112233445566778899aabb",
            "summary_ciphertext_hex": "00112233445566778899aabbccddeeff",
            "payload_version": 1,
            "payload_nonce_hex": "00112233445566778899aabb",
            "payload_ciphertext_hex": "00112233445566778899aabbccddeeff0011223344556677",
            "favorite": false,
            "revision": 1,
            "uri_indexes": [
                {
                    "match_hash_hex": match_hash_hex,
                    "match_type": "base_domain",
                    "rank": 2
                }
            ]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
}

fn create_fake_ffmpeg_script() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rf_fake_ffmpeg_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    #[cfg(unix)]
    {
        let script = dir.join("fake_ffmpeg.sh");
        let content = r#"#!/usr/bin/env bash
set -euo pipefail

out="${@: -1}"
seg_pattern=""
for ((i=1; i<=$#; i++)); do
  arg="${!i}"
  if [[ "$arg" == "-hls_segment_filename" ]]; then
    j=$((i+1))
    seg_pattern="${!j}"
  fi
done

mkdir -p "$(dirname "$out")"
cat > "$out" <<'EOF'
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:4.0,
seg_00000.ts
EOF

if [[ -n "$seg_pattern" ]]; then
  seg="${seg_pattern//%05d/00000}"
  mkdir -p "$(dirname "$seg")"
  printf 'FAKE_TS' > "$seg"
fi

sleep 30
"#;
        std::fs::write(&script, content).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    #[cfg(windows)]
    {
        // On Windows, create a PowerShell wrapper script invoked via a .cmd launcher.
        // The .cmd file is what tokio::process::Command will execute.
        let ps_script = dir.join("fake_ffmpeg.ps1");
        let ps_content = r#"
$args_list = $args
$out = $args_list[$args_list.Count - 1]
$seg_pattern = ""
for ($i = 0; $i -lt $args_list.Count; $i++) {
    if ($args_list[$i] -eq "-hls_segment_filename" -and ($i + 1) -lt $args_list.Count) {
        $seg_pattern = $args_list[$i + 1]
    }
}

$out_dir = Split-Path -Parent $out
if ($out_dir -and !(Test-Path $out_dir)) {
    New-Item -ItemType Directory -Path $out_dir -Force | Out-Null
}

$playlist = @"
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:4.0,
seg_00000.ts
"@
Set-Content -Path $out -Value $playlist -NoNewline

if ($seg_pattern -ne "") {
    $seg = $seg_pattern -replace "%05d", "00000"
    $seg_dir = Split-Path -Parent $seg
    if ($seg_dir -and !(Test-Path $seg_dir)) {
        New-Item -ItemType Directory -Path $seg_dir -Force | Out-Null
    }
    Set-Content -Path $seg -Value "FAKE_TS" -NoNewline
}

Start-Sleep -Seconds 30
"#;
        std::fs::write(&ps_script, ps_content).unwrap();

        let cmd_script = dir.join("fake_ffmpeg.cmd");
        let _ps_path_escaped = ps_script.to_string_lossy().replace('\\', "\\\\");
        let cmd_content = format!(
            "@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\" %*\r\n",
            ps_script.to_string_lossy()
        );
        std::fs::write(&cmd_script, cmd_content).unwrap();
        cmd_script
    }
}

async fn test_app_with_fake_ffmpeg() -> TestServer {
    let pool = create_test_pool().await;
    rustfin_db::repo::settings::insert_defaults(&pool)
        .await
        .unwrap();
    rustfin_db::repo::users::create_user(&pool, "admin", "admin_secure_123", "admin")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_completed", "true")
        .await
        .unwrap();
    rustfin_db::repo::settings::set(&pool, "setup_state", "Completed")
        .await
        .unwrap();

    let fake_ffmpeg = create_fake_ffmpeg_script();
    let tc_config = rustfin_transcoder::TranscoderConfig {
        ffmpeg_path: fake_ffmpeg,
        ffprobe_path: PathBuf::from("ffprobe"),
        transcode_dir: std::env::temp_dir().join(format!("rf_test_hls_{}", uuid::Uuid::new_v4())),
        max_concurrent: 2,
        ..Default::default()
    };
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = build_test_state(
        pool,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        events_tx,
        std::env::temp_dir().join(format!("rf_cache_{}", std::process::id())),
        std::env::temp_dir().join(format!("rf_watch_audio_{}", std::process::id())),
    );

    let app = build_router(state);
    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let server = test_app().await;
    let resp = server.get("/health").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn login_with_valid_credentials() {
    let server = test_app().await;
    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "admin", "password": "admin_secure_123" }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(body["token"].as_str().is_some());
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
}

#[tokio::test]
async fn login_with_invalid_credentials() {
    let server = test_app().await;
    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "admin", "password": "wrong" }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    let body: Value = resp.json();
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn users_me_requires_auth() {
    let server = test_app().await;
    let resp = server.get("/api/v1/users/me").await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn users_me_with_valid_token() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;

    let resp = server
        .get("/api/v1/users/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}")
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
}

#[tokio::test]
async fn preferences_crud() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let auth_header = format!("Bearer {token}");

    // GET default prefs
    let resp = server
        .get("/api/v1/users/me/preferences")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["version"], 1);
    assert_eq!(body["audio"]["input_device_id"], Value::Null);
    assert_eq!(body["activity"]["default_range"], "7d");
    assert_eq!(body["privacy"]["personal_activity_enabled"], true);

    // PATCH prefs
    let new_prefs = json!({
        "version": 1,
        "audio": {
            "input_device_id": "mic-1",
            "output_device_id": "speaker-1"
        },
        "activity": {
            "default_range": "30d"
        },
        "privacy": {
            "personal_activity_enabled": false
        },
        "notifications": {
            "desktop_enabled": true
        },
        "accessibility": {
            "reduce_motion": true
        },
        "appearance": {
            "density": "compact"
        }
    });
    let resp = server
        .patch("/api/v1/users/me/preferences")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&new_prefs)
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["audio"]["input_device_id"], "mic-1");
    assert_eq!(body["activity"]["default_range"], "30d");
    assert_eq!(body["privacy"]["personal_activity_enabled"], false);

    // GET updated prefs
    let resp = server
        .get("/api/v1/users/me/preferences")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["audio"]["output_device_id"], "speaker-1");
    assert_eq!(body["notifications"]["desktop_enabled"], true);
    assert_eq!(body["accessibility"]["reduce_motion"], true);
    assert_eq!(body["appearance"]["density"], "compact");
}

#[tokio::test]
async fn profile_update_persists_time_zone() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let auth_header = format!("Bearer {token}");

    let resp = server
        .patch("/api/v1/users/me/profile")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "display_name": "Admin Person",
            "time_zone": "Europe/Dublin"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["username"], "Admin Person");
    assert_eq!(body["time_zone"], "Europe/Dublin");

    let resp = server
        .get("/api/v1/users/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["username"], "Admin Person");
    assert_eq!(body["time_zone"], "Europe/Dublin");
}

#[tokio::test]
async fn password_change_requires_current_password_and_relogin() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let auth_header = format!("Bearer {token}");

    let resp = server
        .post("/api/v1/users/me/password")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "current_password": "wrong-password",
            "new_password": "admin_secure_456",
            "confirm_password": "admin_secure_456"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let resp = server
        .post("/api/v1/users/me/password")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "current_password": "admin_secure_123",
            "new_password": "admin_secure_456",
            "confirm_password": "admin_secure_456"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["relogin_required"], true);

    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "admin", "password": "admin_secure_123" }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "admin", "password": "admin_secure_456" }))
        .await;
    resp.assert_status_ok();
}

#[tokio::test]
async fn browser_activity_summary_and_clear_history() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let auth_header = format!("Bearer {token}");

    let start_resp = server
        .post("/api/v1/users/me/activity/browser")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "client_session_id": "browser-session-1",
            "tab_id": "tab-1",
            "section": "rooms",
            "event": "start"
        }))
        .await;
    start_resp.assert_status_ok();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let stop_resp = server
        .post("/api/v1/users/me/activity/browser")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "client_session_id": "browser-session-1",
            "tab_id": "tab-1",
            "section": "rooms",
            "event": "stop"
        }))
        .await;
    stop_resp.assert_status_ok();

    let resp = server
        .get("/api/v1/users/me/activity?range=7d")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["activity_enabled"], true);
    assert_eq!(body["most_used_sections"][0]["key"], "rooms");
    assert!(body["totals"]["total_time_ms"].as_i64().unwrap_or_default() >= 1000);

    let resp = server
        .delete("/api/v1/users/me/activity")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();

    let resp = server
        .get("/api/v1/users/me/activity?range=7d")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(body["most_used_sections"].as_array().unwrap().is_empty());

    let resp = server
        .patch("/api/v1/users/me/preferences")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "version": 1,
            "audio": {
                "input_device_id": null,
                "output_device_id": null
            },
            "activity": {
                "default_range": "7d"
            },
            "privacy": {
                "personal_activity_enabled": false
            },
            "notifications": {
                "desktop_enabled": false
            },
            "accessibility": {
                "reduce_motion": false
            },
            "appearance": {}
        }))
        .await;
    resp.assert_status_ok();

    let start_resp = server
        .post("/api/v1/users/me/activity/browser")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "client_session_id": "browser-session-2",
            "tab_id": "tab-1",
            "section": "channels",
            "event": "start"
        }))
        .await;
    start_resp.assert_status_ok();
    let stop_resp = server
        .post("/api/v1/users/me/activity/browser")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({
            "client_session_id": "browser-session-2",
            "tab_id": "tab-1",
            "section": "channels",
            "event": "stop"
        }))
        .await;
    stop_resp.assert_status_ok();

    let resp = server
        .get("/api/v1/users/me/activity?range=7d")
        .add_header(
            axum::http::header::AUTHORIZATION,
            auth_header.parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["activity_enabled"], false);
    assert!(body["most_used_sections"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn migrations_are_idempotent() {
    let target = test_database_target();
    let backend = rustfin_db::detect_backend(&target);
    let pool = rustfin_db::connect(&target).await.unwrap();
    // Run migrations twice — should not error
    rustfin_db::migrate::run(&pool, backend).await.unwrap();
    rustfin_db::migrate::run(&pool, backend).await.unwrap();
}

// ---------------------------------------------------------------------------
// Library tests
// ---------------------------------------------------------------------------

fn auth_hdr(token: &str) -> (axum::http::HeaderName, axum::http::HeaderValue) {
    (
        axum::http::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse::<axum::http::HeaderValue>()
            .unwrap(),
    )
}

async fn create_library_and_first_item(
    server: &TestServer,
    admin_hdr: &(axum::http::HeaderName, axum::http::HeaderValue),
    library_name: &str,
    media_file_name: &str,
) -> (String, String, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("rf_watch_party_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join(media_file_name), "fake video data").unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "name": library_name,
            "kind": "movies",
            "paths": [tmp.to_str().unwrap()]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = resp.json();
    let library_id = body["id"].as_str().unwrap().to_string();

    let resp = server
        .post(&format!("/api/v1/libraries/{library_id}/scan"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status(axum::http::StatusCode::ACCEPTED);

    for _ in 0..20 {
        let resp = server
            .get(&format!("/api/v1/libraries/{library_id}/items"))
            .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
            .await;
        resp.assert_status_ok();
        let items: Vec<Value> = resp.json();
        if let Some(first) = items.first() {
            let item_id = first["id"].as_str().unwrap().to_string();
            return (library_id, item_id, tmp);
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    panic!("library scan did not produce items in time");
}

async fn create_user_with_libraries(
    server: &TestServer,
    admin_hdr: &(axum::http::HeaderName, axum::http::HeaderValue),
    username: &str,
    password: &str,
    role: &str,
    library_ids: &[String],
) -> String {
    let resp = server
        .post("/api/v1/users")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "username": username,
            "password": password,
            "role": role,
            "library_ids": library_ids
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    body["id"].as_str().unwrap().to_string()
}

async fn create_watch_party_room(
    server: &TestServer,
    auth_hdr: &(axum::http::HeaderName, axum::http::HeaderValue),
    payload: Value,
) -> String {
    let resp = server
        .post("/api/v1/watch-party/rooms")
        .add_header(auth_hdr.0.clone(), auth_hdr.1.clone())
        .json(&payload)
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = resp.json();
    body["room_id"].as_str().unwrap().to_string()
}

async fn receive_ws_json_of_type(ws: &mut TestWebSocket, expected_type: &str) -> Value {
    for _ in 0..12 {
        let message: Value = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            ws.receive_json::<Value>(),
        )
        .await
        .expect("websocket should produce a message");
        if message["type"].as_str() == Some(expected_type) {
            return message;
        }
    }

    panic!("websocket did not receive expected message type: {expected_type}");
}

#[tokio::test]
async fn create_library_requires_admin() {
    let server = test_app().await;
    // Test that unauthenticated requests fail
    let resp = server
        .post("/api/v1/libraries")
        .json(&json!({ "name": "Movies", "kind": "movies", "paths": ["/media/movies"] }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn library_crud_flow() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let tmp = std::env::temp_dir().join(format!("rf_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    // Create library
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "Movies", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["name"], "Movies");
    assert_eq!(body["kind"], "movies");
    assert_eq!(body["item_count"], 0);
    assert_eq!(body["paths"][0]["path"], tmp.to_str().unwrap());
    let lib_id = body["id"].as_str().unwrap().to_string();

    // List libraries
    let resp = server
        .get("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Get single library
    let resp = server
        .get(&format!("/api/v1/libraries/{lib_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["name"], "Movies");

    // Update library name
    let resp = server
        .patch(&format!("/api/v1/libraries/{lib_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "My Movies" }))
        .await;
    resp.assert_status_ok();

    // Verify update
    let resp = server
        .get(&format!("/api/v1/libraries/{lib_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["name"], "My Movies");

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn scan_recreate_library_reuses_existing_media_rows() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let tmp = std::env::temp_dir().join(format!("rf_recreate_scan_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("American Sniper.mp4"), "fake video bytes").unwrap();

    let create_resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "Desktop Test A", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    create_resp.assert_status(axum::http::StatusCode::CREATED);
    let first_library_id = create_resp.json::<Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let scan_resp = server
        .post(&format!("/api/v1/libraries/{first_library_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    scan_resp.assert_status(axum::http::StatusCode::ACCEPTED);

    let mut first_items = Vec::new();
    for _ in 0..15 {
        let items_resp = server
            .get(&format!("/api/v1/libraries/{first_library_id}/items"))
            .add_header(hdr_name.clone(), hdr_val.clone())
            .await;
        items_resp.assert_status_ok();
        first_items = items_resp.json::<Vec<Value>>();
        if !first_items.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    assert!(
        !first_items.is_empty(),
        "initial library scan should produce at least one item"
    );

    let delete_resp = server
        .delete(&format!("/api/v1/libraries/{first_library_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    delete_resp.assert_status_ok();

    let create_resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "Desktop Test B", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    create_resp.assert_status(axum::http::StatusCode::CREATED);
    let second_library_id = create_resp.json::<Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let scan_resp = server
        .post(&format!("/api/v1/libraries/{second_library_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    scan_resp.assert_status(axum::http::StatusCode::ACCEPTED);

    let mut second_items = Vec::new();
    for _ in 0..15 {
        let items_resp = server
            .get(&format!("/api/v1/libraries/{second_library_id}/items"))
            .add_header(hdr_name.clone(), hdr_val.clone())
            .await;
        items_resp.assert_status_ok();
        second_items = items_resp.json::<Vec<Value>>();
        if !second_items.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    assert!(
        !second_items.is_empty(),
        "recreated library scan should still produce items for existing files"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn create_library_validates_kind() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name, hdr_val)
        .json(&json!({ "name": "Bad", "kind": "invalid", "paths": ["/x"] }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_nonexistent_library_returns_404() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let resp = server
        .get("/api/v1/libraries/nonexistent-id")
        .add_header(hdr_name, hdr_val)
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Job + scan tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scan_library_creates_job() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let tmp = std::env::temp_dir().join(format!("rf_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    // Create library first
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "TV", "kind": "tv_shows", "paths": [tmp.to_str().unwrap()] }))
        .await;
    let lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    // Trigger scan
    let resp = server
        .post(&format!("/api/v1/libraries/{lib_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status(axum::http::StatusCode::ACCEPTED);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "library_scan");
    // Status is "queued" at creation time
    assert_eq!(body["status"], "queued");
    let job_id = body["id"].as_str().unwrap().to_string();

    // List jobs — should have at least 1
    let resp = server
        .get("/api/v1/jobs")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert!(!body.as_array().unwrap().is_empty());

    // Get job by ID — should exist regardless of status
    let resp = server
        .get(&format!("/api/v1/jobs/{job_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["kind"], "library_scan");

    // Wait briefly for background task, then check final state
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let resp = server
        .get(&format!("/api/v1/jobs/{job_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let body: Value = resp.json();
    // Job should have reached a terminal state (completed, since path doesn't exist = no-op scan)
    let status = body["status"].as_str().unwrap();
    assert!(
        status == "completed" || status == "running" || status == "queued",
        "unexpected job status: {status}"
    );
}

#[tokio::test]
async fn scan_nonexistent_library_returns_404() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let resp = server
        .post("/api/v1/libraries/nonexistent/scan")
        .add_header(hdr_name, hdr_val)
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Scanner integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scan_movie_library_creates_items() {
    // Create temp dir with movie files
    let tmp = std::env::temp_dir().join(format!("rustfin_test_movies_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(tmp.join("The Matrix (1999)")).unwrap();
    std::fs::write(tmp.join("The Matrix (1999)/The Matrix (1999).mkv"), b"fake").unwrap();
    std::fs::create_dir_all(tmp.join("Inception (2010)")).unwrap();
    std::fs::write(tmp.join("Inception (2010)/Inception.2010.mkv"), b"fake").unwrap();

    let pool = create_test_pool().await;

    // Create library pointing to tmp dir
    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "Movies",
        "movies",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    // Run scan directly
    let result = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    assert_eq!(result.added, 2);

    // Verify items created
    let items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 2);

    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert!(titles.contains(&"The Matrix"));
    assert!(titles.contains(&"Inception"));

    // Verify year is set
    let matrix = items.iter().find(|i| i.title == "The Matrix").unwrap();
    assert_eq!(matrix.year, Some(1999));
    assert_eq!(matrix.kind, "movie");

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn scan_tv_library_creates_series_hierarchy() {
    // Create temp dir with TV show structure
    let tmp = std::env::temp_dir().join(format!("rustfin_test_tv_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(tmp.join("Breaking Bad/Season 01")).unwrap();
    std::fs::write(
        tmp.join("Breaking Bad/Season 01/Breaking.Bad.S01E01.Pilot.mkv"),
        b"fake",
    )
    .unwrap();
    std::fs::write(
        tmp.join("Breaking Bad/Season 01/Breaking.Bad.S01E02.Cat's.in.the.Bag.mkv"),
        b"fake",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.join("Breaking Bad/Season 02")).unwrap();
    std::fs::write(
        tmp.join("Breaking Bad/Season 02/Breaking.Bad.S02E01.Seven.Thirty.Seven.mkv"),
        b"fake",
    )
    .unwrap();

    let pool = create_test_pool().await;

    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "TV Shows",
        "tv_shows",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    let result = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "tv_shows")
        .await
        .unwrap();
    assert_eq!(result.added, 3);

    // Top-level items should be series only
    let items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "series");
    assert_eq!(items[0].title, "Breaking Bad");

    // Series should have seasons as children
    let seasons = rustfin_db::repo::items::get_children(&pool, &items[0].id)
        .await
        .unwrap();
    assert_eq!(seasons.len(), 2);
    assert!(seasons.iter().all(|s| s.kind == "season"));

    // Season 1 should have 2 episodes
    let s1 = seasons.iter().find(|s| s.title == "Season 1").unwrap();
    let episodes = rustfin_db::repo::items::get_children(&pool, &s1.id)
        .await
        .unwrap();
    assert_eq!(episodes.len(), 2);
    assert!(episodes.iter().all(|e| e.kind == "episode"));

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn scan_tv_library_reconciles_stale_root_movie_mapping_for_episode_variant() {
    let tmp = std::env::temp_dir().join(format!(
        "rustfin_test_tv_reconcile_{}",
        uuid::Uuid::new_v4()
    ));
    let season_dir = tmp.join("Breaking Bad/Season 02");
    std::fs::create_dir_all(&season_dir).unwrap();
    let episode_path = season_dir.join("Breaking Bad S02 E01.mkv");
    std::fs::write(&episode_path, b"fake").unwrap();

    let pool = create_test_pool().await;

    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "TV Shows",
        "tv_shows",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    let now = chrono::Utc::now().timestamp();
    let stale_item_id = uuid::Uuid::new_v4().to_string();
    let file_id = uuid::Uuid::new_v4().to_string();
    let path_str = episode_path.to_string_lossy().to_string();
    let metadata = std::fs::metadata(&episode_path).unwrap();
    let mtime_ts = metadata
        .modified()
        .ok()
        .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|ts| i64::try_from(ts.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(now);

    sqlx::query(
        "INSERT INTO item (id, library_id, kind, parent_id, title, created_ts, updated_ts) \
         VALUES ($1, $2, 'movie', NULL, $3, $4, $5)",
    )
    .bind(&stale_item_id)
    .bind(&lib.id)
    .bind("Breaking Bad S02 E01")
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO media_file (id, path, size_bytes, mtime_ts, created_ts, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&file_id)
    .bind(&path_str)
    .bind(i64::try_from(metadata.len()).unwrap_or(i64::MAX))
    .bind(mtime_ts)
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO episode_file_map (id, episode_item_id, file_id, map_kind, created_ts) \
         VALUES ($1, $2, $3, 'primary', $4)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&stale_item_id)
    .bind(&file_id)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let result = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "tv_shows")
        .await
        .unwrap();
    assert_eq!(result.added, 1);

    let items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "series");
    assert_eq!(items[0].title, "Breaking Bad");

    let seasons = rustfin_db::repo::items::get_children(&pool, &items[0].id)
        .await
        .unwrap();
    assert_eq!(seasons.len(), 1);
    assert_eq!(seasons[0].title, "Season 2");

    let episodes = rustfin_db::repo::items::get_children(&pool, &seasons[0].id)
        .await
        .unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].kind, "episode");
    assert_eq!(episodes[0].title, "Episode 1");

    let file_mappings: Vec<(String, String)> = sqlx::query_as(
        "SELECT i.kind, i.title \
         FROM episode_file_map efm \
         JOIN item i ON i.id = efm.episode_item_id \
         WHERE efm.file_id = $1 \
         ORDER BY i.kind, i.title",
    )
    .bind(&file_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        file_mappings,
        vec![("episode".to_string(), "Episode 1".to_string())]
    );

    let stale_item_exists: Option<(String,)> = sqlx::query_as("SELECT id FROM item WHERE id = $1")
        .bind(&stale_item_id)
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(stale_item_exists.is_none());

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn scan_movie_library_prunes_deleted_files_and_admin_counts() {
    let tmp = std::env::temp_dir().join(format!(
        "rustfin_test_movies_prune_deleted_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(tmp.join("Arrival (2016)")).unwrap();
    std::fs::write(tmp.join("Arrival (2016)/Arrival (2016).mkv"), b"fake").unwrap();
    std::fs::create_dir_all(tmp.join("Looper (2012)")).unwrap();
    let deleted_file = tmp.join("Looper (2012)/Looper (2012).mkv");
    std::fs::write(&deleted_file, b"fake").unwrap();

    let pool = create_test_pool().await;
    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "Movies",
        "movies",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    let initial = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    assert_eq!(initial.added, 2);

    let initial_items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(initial_items.len(), 2);
    assert_eq!(
        rustfin_db::repo::libraries::count_library_items(&pool, &lib.id)
            .await
            .unwrap(),
        2
    );

    std::fs::remove_file(&deleted_file).unwrap();

    let rescan = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    assert_eq!(rescan.added, 0);

    let items_after = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items_after.len(), 1);
    assert_eq!(items_after[0].title, "Arrival");

    let count_after = rustfin_db::repo::libraries::count_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(count_after, 1);

    let grouped_counts =
        rustfin_db::repo::libraries::count_library_items_for_libraries(&pool, &[lib.id.clone()])
            .await
            .unwrap();
    assert_eq!(grouped_counts, vec![(lib.id.clone(), 1)]);

    let deleted_path = deleted_file.to_string_lossy().to_string();
    let stale_media: Option<(String,)> =
        sqlx::query_as("SELECT id FROM media_file WHERE path = $1")
            .bind(&deleted_path)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(stale_media.is_none());

    let stale_map_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) \
         FROM episode_file_map efm \
         JOIN media_file mf ON mf.id = efm.file_id \
         WHERE mf.path = $1",
    )
    .bind(&deleted_path)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_map_count, 0);

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn scan_movie_library_prunes_media_from_removed_library_paths() {
    let tmp_a = std::env::temp_dir().join(format!(
        "rustfin_test_movies_path_a_{}",
        uuid::Uuid::new_v4()
    ));
    let tmp_b = std::env::temp_dir().join(format!(
        "rustfin_test_movies_path_b_{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(tmp_a.join("Alien (1979)")).unwrap();
    std::fs::create_dir_all(tmp_b.join("Blade Runner (1982)")).unwrap();
    let old_file = tmp_a.join("Alien (1979)/Alien (1979).mkv");
    let new_file = tmp_b.join("Blade Runner (1982)/Blade Runner (1982).mkv");
    std::fs::write(&old_file, b"fake").unwrap();
    std::fs::write(&new_file, b"fake").unwrap();

    let pool = create_test_pool().await;
    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "Movies",
        "movies",
        &[tmp_a.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    let first_items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(first_items.len(), 1);
    assert_eq!(first_items[0].title, "Alien");

    rustfin_db::repo::libraries::replace_library_paths(
        &pool,
        &lib.id,
        &[tmp_b.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();

    let items_after = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items_after.len(), 1);
    assert_eq!(items_after[0].title, "Blade Runner");

    let old_path = old_file.to_string_lossy().to_string();
    let stale_old_media: Option<(String,)> =
        sqlx::query_as("SELECT id FROM media_file WHERE path = $1")
            .bind(&old_path)
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(stale_old_media.is_none());

    std::fs::remove_dir_all(&tmp_a).ok();
    std::fs::remove_dir_all(&tmp_b).ok();
}

#[tokio::test]
async fn scan_is_idempotent() {
    let tmp = std::env::temp_dir().join(format!("rustfin_test_idem_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("Movie (2020).mkv"), b"fake").unwrap();

    let pool = create_test_pool().await;

    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "Test",
        "movies",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    // Scan twice
    let r1 = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    assert_eq!(r1.added, 1);

    let r2 = rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();
    assert_eq!(r2.added, 0);
    assert_eq!(r2.skipped, 1);

    // Still only 1 item
    let items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);

    std::fs::remove_dir_all(&tmp).ok();
}

// ---------------------------------------------------------------------------
// Range streaming tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stream_file_with_range_returns_206() {
    // Create temp dir with a movie file containing known data
    let tmp = std::env::temp_dir().join(format!("rustfin_test_stream_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    // Create a 5000-byte test file with known content
    let test_data: Vec<u8> = (0u8..=255).cycle().take(5000).collect();
    std::fs::write(tmp.join("TestMovie (2020).mkv"), &test_data).unwrap();

    // Set up DB + scan
    let pool = create_test_pool().await;
    rustfin_db::repo::users::create_user(&pool, "admin", "admin_secure_123", "admin")
        .await
        .unwrap();

    let lib = rustfin_db::repo::libraries::create_library(
        &pool,
        "Movies",
        "movies",
        &[tmp.to_string_lossy().to_string()],
    )
    .await
    .unwrap();

    rustfin_scanner::scan::run_library_scan(&pool, &lib.id, "movies")
        .await
        .unwrap();

    // Find the media file ID
    let items = rustfin_db::repo::items::get_library_items(&pool, &lib.id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);

    let file_id = rustfin_db::repo::items::get_item_file_id(&pool, &items[0].id)
        .await
        .unwrap()
        .expect("should have a file linked");

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: std::env::temp_dir().join(format!("rf_stream_{}", std::process::id())),
        max_concurrent: 2,
        ..Default::default()
    };
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = build_test_state(
        pool,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        events_tx,
        std::env::temp_dir().join(format!("rf_cache_stream_{}", std::process::id())),
        std::env::temp_dir().join(format!("rf_watch_audio_stream_{}", std::process::id())),
    );
    let app = rustfin_server::routes::build_router(state);
    let server = TestServer::new(app).unwrap();
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // Unauthenticated stream requests are rejected.
    let resp = server.get(&format!("/stream/file/{file_id}")).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);

    // Request Range: bytes=0-999 (first 1000 bytes)
    let resp = server
        .get(&format!("/stream/file/{file_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .add_header(
            axum::http::header::RANGE,
            "bytes=0-999".parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;

    assert_eq!(resp.status_code(), axum::http::StatusCode::PARTIAL_CONTENT);
    let body = resp.as_bytes().to_vec();
    assert_eq!(body.len(), 1000);
    assert_eq!(&body[..], &test_data[0..1000]);

    // Check Content-Range header
    let cr = resp
        .headers()
        .get("content-range")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cr, "bytes 0-999/5000");

    // Check Accept-Ranges header
    let ar = resp
        .headers()
        .get("accept-ranges")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(ar, "bytes");

    // Request full file (no Range header)
    let resp = server
        .get(&format!("/stream/file/{file_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    assert_eq!(resp.as_bytes().len(), 5000);

    // Request open-ended range: bytes=4000-
    let resp = server
        .get(&format!("/stream/file/{file_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .add_header(
            axum::http::header::RANGE,
            "bytes=4000-".parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.as_bytes().len(), 1000);
    let cr = resp
        .headers()
        .get("content-range")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cr, "bytes 4000-4999/5000");

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn playback_descriptor_returns_file_id_and_reports_unmapped_items() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // Movies fixture with a mapped playable file.
    let movies_tmp =
        std::env::temp_dir().join(format!("rf_playback_desc_movies_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&movies_tmp).unwrap();
    std::fs::write(movies_tmp.join("Sample Movie (2020).mp4"), b"fake").unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "name": "Playback Movies",
            "kind": "movies",
            "paths": [movies_tmp.to_str().unwrap()]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let movies_lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/api/v1/libraries/{movies_lib_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let resp = server
        .get(&format!("/api/v1/libraries/{movies_lib_id}/items"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let items: Value = resp.json();
    let movie_item_id = items.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = server
        .get(&format!("/api/v1/items/{movie_item_id}/playback"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let playback: Value = resp.json();
    let file_id = playback["file_id"].as_str().unwrap().to_string();
    let direct_url = playback["direct_url"].as_str().unwrap();
    assert!(direct_url.contains(&format!("/stream/file/{file_id}?st=")));
    assert!(!direct_url.contains("?token="));

    // TV fixture where top-level series item has no direct file mapping.
    let tv_tmp = std::env::temp_dir().join(format!("rf_playback_desc_tv_{}", uuid::Uuid::new_v4()));
    let season_dir = tv_tmp.join("Example Show/Season 01");
    std::fs::create_dir_all(&season_dir).unwrap();
    std::fs::write(season_dir.join("Example.Show.S01E01.mp4"), b"fake").unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "name": "Playback TV",
            "kind": "tv_shows",
            "paths": [tv_tmp.to_str().unwrap()]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let tv_lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/api/v1/libraries/{tv_lib_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let resp = server
        .get(&format!("/api/v1/libraries/{tv_lib_id}/items"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let tv_items: Value = resp.json();
    let series_item_id = tv_items.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = server
        .get(&format!("/api/v1/items/{series_item_id}/playback"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status(axum::http::StatusCode::CONFLICT);
    let body: Value = resp.json();
    assert_eq!(body["error"]["code"], "conflict");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("No playable file mapped to this item")
    );

    std::fs::remove_dir_all(&movies_tmp).ok();
    std::fs::remove_dir_all(&tv_tmp).ok();
}

#[tokio::test]
async fn hls_endpoints_require_auth_and_enforce_session_owner() {
    let server = test_app_with_fake_ffmpeg().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let tmp = std::env::temp_dir().join(format!("rf_hls_auth_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("Auth Movie (2020).mp4"), b"fake").unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "name": "HLS Auth Movies",
            "kind": "movies",
            "paths": [tmp.to_str().unwrap()]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/api/v1/libraries/{lib_id}/scan"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let resp = server
        .get(&format!("/api/v1/libraries/{lib_id}/items"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    let items: Value = resp.json();
    let item_id = items.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = server
        .get(&format!("/api/v1/items/{item_id}/playback"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let playback: Value = resp.json();
    let file_id = playback["file_id"].as_str().unwrap().to_string();

    let resp = server
        .post("/api/v1/playback/sessions")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "file_id": file_id }))
        .await;
    resp.assert_status_ok();
    let session: Value = resp.json();
    let sid = session["session_id"].as_str().unwrap().to_string();

    // Unauthenticated master request is rejected.
    let resp = server.get(&format!("/stream/hls/{sid}/master.m3u8")).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);

    // Session owner can fetch HLS resources.
    let resp = server
        .get(&format!("/stream/hls/{sid}/master.m3u8"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    let master_playlist = String::from_utf8(resp.as_bytes().to_vec()).unwrap();
    let first_child = master_playlist
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string);
    let first_child_path = first_child
        .as_ref()
        .map(|child| {
            if child.starts_with('/') {
                child.clone()
            } else {
                format!("/stream/hls/{sid}/{child}")
            }
        })
        .unwrap_or_else(|| format!("/stream/hls/{sid}/master.m3u8"));
    if let Some(child) = first_child.as_ref() {
        assert!(
            child.contains("st="),
            "child playlist URI should contain scoped stream token"
        );
    }

    let resp = server
        .get(&first_child_path)
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);

    // Derive a concrete segment/media URL so we can verify auth there as well.
    let child_body = String::from_utf8(resp.as_bytes().to_vec()).unwrap_or_default();
    let maybe_segment = if first_child_path.contains(".m3u8") {
        child_body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
    } else {
        Some(first_child_path.clone())
    };
    let segment_path = maybe_segment
        .map(|uri| {
            if uri.starts_with('/') {
                uri
            } else {
                format!("/stream/hls/{sid}/{uri}")
            }
        })
        .unwrap_or_else(|| format!("/stream/hls/{sid}/seg_00000.ts"));
    let segment_path_no_query = segment_path
        .split('?')
        .next()
        .unwrap_or(&segment_path)
        .to_string();

    let resp = server.get(&segment_path_no_query).await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);

    // Create a non-owner user and ensure they cannot access this session.
    let resp = server
        .post("/api/v1/users")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "username": "otheruser",
            "password": "otheruser_pass_123",
            "role": "user",
            "library_ids": [lib_id]
        }))
        .await;
    resp.assert_status_ok();
    let other_token = login(&server, "otheruser", "otheruser_pass_123").await;
    let other_hdr = auth_hdr(&other_token);

    let resp = server
        .get(&format!("/stream/hls/{sid}/master.m3u8"))
        .add_header(other_hdr.0.clone(), other_hdr.1.clone())
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);

    let resp = server
        .get(&segment_path_no_query)
        .add_header(other_hdr.0.clone(), other_hdr.1.clone())
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::FORBIDDEN);

    std::fs::remove_dir_all(&tmp).ok();
}

// ---------------------------------------------------------------------------
// Playback progress tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playback_progress_update_and_get() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // Create a library with a real temp dir and scan it for playback tests
    let tmp = std::env::temp_dir().join(format!("rf_play_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("Inception (2010).mkv"), "fake video data").unwrap();

    // Create library with real path
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "PlayMovies", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    let lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    // Scan
    server
        .post(&format!("/api/v1/libraries/{lib_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;

    // Wait for scan
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Get items
    let resp = server
        .get(&format!("/api/v1/libraries/{lib_id}/items"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let items: Value = resp.json();
    let item_id = items.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Get play state — should be default (no progress)
    let resp = server
        .get(&format!("/api/v1/playback/state/{item_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["progress_ms"], 0);
    assert_eq!(body["played"], false);

    // Early progress should be ignored for continue watching.
    let resp = server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": item_id,
            "progress_ms": 20000,
            "played": false
        }))
        .await;
    resp.assert_status_ok();

    let resp = server
        .get(&format!("/api/v1/playback/state/{item_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let body: Value = resp.json();
    assert_eq!(body["progress_ms"], 0);
    assert_eq!(body["played"], false);

    // Meaningful progress should be stored.
    let resp = server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": item_id.clone(),
            "progress_ms": 120000,
            "played": false
        }))
        .await;
    resp.assert_status_ok();

    // Verify updated
    let resp = server
        .get(&format!("/api/v1/playback/state/{item_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let body: Value = resp.json();
    assert_eq!(body["progress_ms"], 120000);
    assert_eq!(body["played"], false);

    // Mark as played
    let resp = server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": item_id.clone(),
            "progress_ms": 120000,
            "played": true
        }))
        .await;
    resp.assert_status_ok();

    let resp = server
        .get(&format!("/api/v1/playback/state/{item_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let body: Value = resp.json();
    assert_eq!(body["played"], true);
    assert_eq!(body["progress_ms"], 0);
    assert!(body["last_played_ts"].as_i64().unwrap() > 0);

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn continue_watching_feed_lists_only_in_progress_items() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    let tmp = std::env::temp_dir().join(format!("rf_continue_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("Arrival (2016).mkv"), "fake video data").unwrap();
    std::fs::write(tmp.join("Looper (2012).mkv"), "fake video data").unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "ContinueMovies", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    let lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/api/v1/libraries/{lib_id}/scan"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let resp = server
        .get(&format!("/api/v1/libraries/{lib_id}/items"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let items: Value = resp.json();
    let items = items.as_array().unwrap();
    assert!(items.len() >= 2);
    let first_item_id = items[0]["id"].as_str().unwrap().to_string();
    let second_item_id = items[1]["id"].as_str().unwrap().to_string();

    server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": first_item_id,
            "progress_ms": 120000,
            "played": false
        }))
        .await
        .assert_status_ok();

    server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": second_item_id,
            "progress_ms": 120000,
            "played": true
        }))
        .await
        .assert_status_ok();

    let resp = server
        .get("/api/v1/playback/continue")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let entries = body.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"].as_str().unwrap(), first_item_id);
    assert_eq!(entries[0]["progress_ms"].as_i64().unwrap(), 120000);

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn user_management_crud() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin_secure_123").await;
    let hdr_name = axum::http::header::AUTHORIZATION;
    let hdr_val = axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

    // Create a library that can be assigned to regular users
    let tmp = std::env::temp_dir().join(format!("rf_user_mgmt_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "User Movies", "kind": "movies", "paths": [tmp.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let library_body: Value = resp.json();
    let library_id = library_body["id"].as_str().unwrap().to_string();

    // List users — should have the bootstrap admin
    let resp = server
        .get("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();
    let users: Vec<Value> = resp.json();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["username"], "admin");

    // Create a new user
    let resp = server
        .post("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "username": "testuser",
            "password": "testpass_secure",
            "role": "user",
            "library_ids": [library_id]
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let new_user_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["username"], "testuser");
    assert_eq!(body["role"], "user");
    assert_eq!(body["library_ids"], json!([library_id.clone()]));

    // Create a simple user without any library access (allowed).
    let resp = server
        .post("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "username": "nolibuser",
            "password": "nolibuser_secure",
            "role": "user",
            "library_ids": []
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let no_library_user_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["username"], "nolibuser");
    assert_eq!(body["role"], "user");
    assert_eq!(body["library_ids"], json!([]));

    // List again — should have 3 users
    let resp = server
        .get("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let users: Vec<Value> = resp.json();
    assert_eq!(users.len(), 3);

    // New user can login
    let _user_token = login(&server, "testuser", "testpass_secure").await;
    let _nolib_token = login(&server, "nolibuser", "nolibuser_secure").await;

    // Existing simple user can be updated to no library access.
    let resp = server
        .patch(&format!("/api/v1/users/{new_user_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "role": "user",
            "library_ids": []
        }))
        .await;
    resp.assert_status_ok();
    let patched: Value = resp.json();
    assert_eq!(patched["library_ids"], json!([]));

    // Delete the new user
    let resp = server
        .delete(&format!("/api/v1/users/{new_user_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();

    // Delete the no-library user
    let resp = server
        .delete(&format!("/api/v1/users/{no_library_user_id}"))
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    resp.assert_status_ok();

    // List again — should have 1 user
    let resp = server
        .get("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let users: Vec<Value> = resp.json();
    assert_eq!(users.len(), 1);

    std::fs::remove_dir_all(&tmp).ok();
}

// ---------------------------------------------------------------------------
// Watch party tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn watch_party_audio_online_room_can_be_created_without_library() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "room_mode": "audio",
            "audio_source": "online",
            "invites": []
        }),
    )
    .await;

    let resp = server
        .get(&format!("/api/v1/watch-party/rooms/{room_id}"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["room_mode"], "audio");
    assert_eq!(body["audio_source"], "online");

    let resp = server
        .get(&format!("/api/v1/watch-party/rooms/{room_id}/audio/tracks"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let tracks: Vec<Value> = resp.json();
    assert!(
        tracks.is_empty(),
        "new online audio room should start empty"
    );
}

#[tokio::test]
async fn watch_party_create_room_rejects_invitee_without_library_access() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let (allowed_library_id, item_id, tmp_allowed) = create_library_and_first_item(
        &server,
        &admin_hdr,
        "Watch Allowed",
        "Allowed Movie (2024).mp4",
    )
    .await;

    let (restricted_library_id, _, tmp_restricted) = create_library_and_first_item(
        &server,
        &admin_hdr,
        "Watch Restricted",
        "Restricted Movie (2024).mp4",
    )
    .await;

    let allowed_user_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "watch_allowed_user",
        "watch_allowed_user_pass_123",
        "user",
        std::slice::from_ref(&allowed_library_id),
    )
    .await;

    let restricted_user_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "watch_restricted_user",
        "watch_restricted_user_pass_123",
        "user",
        std::slice::from_ref(&restricted_library_id),
    )
    .await;

    let resp = server
        .post("/api/v1/watch-party/rooms")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "item_id": item_id,
            "invites": [
                { "user_id": allowed_user_id, "role": "viewer" },
                { "user_id": restricted_user_id, "role": "viewer" }
            ],
            "policy": {
                "allow_non_host_play_pause": true,
                "allow_non_host_seek": false,
                "default_join_role": "viewer",
                "invite_only": true
            }
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::FORBIDDEN);

    std::fs::remove_dir_all(&tmp_allowed).ok();
    std::fs::remove_dir_all(&tmp_restricted).ok();
}

#[tokio::test]
async fn watch_party_invites_and_password_join_flow() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let (library_id, item_id, tmp) = create_library_and_first_item(
        &server,
        &admin_hdr,
        "Watch Password",
        "Password Movie (2024).mp4",
    )
    .await;

    let invitee_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "watch_invitee_user",
        "watch_invitee_user_pass_123",
        "user",
        std::slice::from_ref(&library_id),
    )
    .await;
    let invitee_token = login(&server, "watch_invitee_user", "watch_invitee_user_pass_123").await;
    let invitee_hdr = auth_hdr(&invitee_token);

    let room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "item_id": item_id,
            "invites": [
                { "user_id": invitee_id, "role": "viewer" }
            ],
            "password": "party-pass",
            "policy": {
                "allow_non_host_play_pause": true,
                "allow_non_host_seek": false,
                "default_join_role": "viewer",
                "invite_only": true
            }
        }),
    )
    .await;

    let resp = server
        .get("/api/v1/watch-party/invites")
        .add_header(invitee_hdr.0.clone(), invitee_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let invites: Vec<Value> = resp.json();
    assert_eq!(invites.len(), 1);
    assert_eq!(invites[0]["room_id"], room_id);
    assert_eq!(invites[0]["password_required"], true);

    let resp = server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(invitee_hdr.0.clone(), invitee_hdr.1.clone())
        .json(&json!({ "password": "wrong-pass" }))
        .await;
    resp.assert_status(axum::http::StatusCode::FORBIDDEN);

    let resp = server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(invitee_hdr.0.clone(), invitee_hdr.1.clone())
        .json(&json!({ "password": "party-pass" }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["role"], "viewer");

    let resp = server
        .get("/api/v1/watch-party/invites")
        .add_header(invitee_hdr.0.clone(), invitee_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let invites: Vec<Value> = resp.json();
    assert!(invites.is_empty());

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn watch_party_websocket_requires_auth_and_enforces_permissions() {
    let server = test_app_http().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let (library_id, item_id, tmp) = create_library_and_first_item(
        &server,
        &admin_hdr,
        "Watch WS",
        "WebSocket Movie (2024).mp4",
    )
    .await;

    let viewer_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "watch_ws_viewer",
        "watch_ws_viewer_pass_123",
        "user",
        std::slice::from_ref(&library_id),
    )
    .await;
    let outsider_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "watch_ws_outsider",
        "watch_ws_outsider_pass_123",
        "user",
        std::slice::from_ref(&library_id),
    )
    .await;
    assert_ne!(viewer_id, outsider_id);

    let room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "item_id": item_id,
            "invites": [
                { "user_id": viewer_id, "role": "viewer" }
            ],
            "policy": {
                "allow_non_host_play_pause": false,
                "allow_non_host_seek": false,
                "default_join_role": "viewer",
                "invite_only": true
            }
        }),
    )
    .await;

    let viewer_token = login(&server, "watch_ws_viewer", "watch_ws_viewer_pass_123").await;
    let viewer_hdr = auth_hdr(&viewer_token);
    let outsider_token = login(&server, "watch_ws_outsider", "watch_ws_outsider_pass_123").await;

    let resp = server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(viewer_hdr.0.clone(), viewer_hdr.1.clone())
        .json(&json!({}))
        .await;
    resp.assert_status_ok();

    let ws_host_header = axum::http::header::HOST;
    let ws_host_value = "watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();
    let ws_origin_header = axum::http::header::ORIGIN;
    let ws_origin_value = "http://watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();

    let mut unauth_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    unauth_ws
        .send_json(&json!({ "type": "auth", "token": "invalid.token.value" }))
        .await;
    let unauth_msg: Value = unauth_ws.receive_json().await;
    assert_eq!(unauth_msg["type"], "error");
    assert!(
        unauth_msg["message"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid token")
    );
    unauth_ws.close().await;

    let mut host_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    host_ws
        .send_json(&json!({ "type": "auth", "token": admin_token }))
        .await;
    let host_state: Value = host_ws.receive_json().await;
    assert_eq!(host_state["type"], "state");

    let mut viewer_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    viewer_ws
        .send_json(&json!({ "type": "auth", "token": viewer_token }))
        .await;
    let viewer_state: Value = viewer_ws.receive_json().await;
    assert_eq!(viewer_state["type"], "state");

    viewer_ws
        .send_json(&json!({ "type": "play", "position_ms": 1000 }))
        .await;

    let mut viewer_error = None;
    for _ in 0..6 {
        let message: Value = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            viewer_ws.receive_json::<Value>(),
        )
        .await
        .expect("viewer websocket should receive a response");
        if message["type"] == "error" {
            viewer_error = Some(message);
            break;
        }
    }
    let viewer_error = viewer_error.expect("viewer action should be rejected");
    assert!(
        viewer_error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("play/pause is not allowed")
    );

    let mut outsider_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    outsider_ws
        .send_json(&json!({ "type": "auth", "token": outsider_token }))
        .await;
    let outsider_msg: Value = outsider_ws.receive_json().await;
    assert_eq!(outsider_msg["type"], "error");
    assert!(
        outsider_msg["message"]
            .as_str()
            .unwrap_or_default()
            .contains("room membership not found")
    );

    host_ws.close().await;
    viewer_ws.close().await;
    outsider_ws.close().await;

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn watch_party_screen_room_can_be_created_and_reconfigured() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let screen_room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "room_mode": "screen",
            "invites": []
        }),
    )
    .await;

    let resp = server
        .get(&format!("/api/v1/watch-party/rooms/{screen_room_id}"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["room_mode"], "screen");

    let (_library_id, item_id, tmp) = create_library_and_first_item(
        &server,
        &admin_hdr,
        "Watch Reconfigure Screen",
        "Reconfigure Screen Movie (2024).mp4",
    )
    .await;

    let video_room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "room_mode": "video",
            "item_id": item_id,
            "invites": []
        }),
    )
    .await;

    let resp = server
        .post(&format!(
            "/api/v1/watch-party/rooms/{video_room_id}/reconfigure"
        ))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "room_mode": "screen"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["room_mode"], "screen");

    let resp = server
        .get(&format!("/api/v1/watch-party/rooms/{video_room_id}"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["room_mode"], "screen");

    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn watch_party_screen_websocket_enforces_presenter_locking() {
    let server = test_app_http().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let controller_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "screen_controller",
        "screen_controller_pass_123",
        "user",
        &[],
    )
    .await;
    let viewer_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "screen_viewer",
        "screen_viewer_pass_123",
        "user",
        &[],
    )
    .await;
    let outsider_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "screen_outsider",
        "screen_outsider_pass_123",
        "user",
        &[],
    )
    .await;

    let room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "room_mode": "screen",
            "invites": [
                { "user_id": controller_id, "role": "controller" },
                { "user_id": viewer_id, "role": "viewer" }
            ]
        }),
    )
    .await;

    let controller_token = login(&server, "screen_controller", "screen_controller_pass_123").await;
    let viewer_token = login(&server, "screen_viewer", "screen_viewer_pass_123").await;
    let outsider_token = login(&server, "screen_outsider", "screen_outsider_pass_123").await;
    let controller_hdr = auth_hdr(&controller_token);
    let viewer_hdr = auth_hdr(&viewer_token);
    assert_ne!(outsider_id, viewer_id);

    server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(controller_hdr.0.clone(), controller_hdr.1.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();
    server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(viewer_hdr.0.clone(), viewer_hdr.1.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();

    let ws_host_header = axum::http::header::HOST;
    let ws_host_value = "watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();
    let ws_origin_header = axum::http::header::ORIGIN;
    let ws_origin_value = "http://watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();

    let mut host_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    host_ws
        .send_json(&json!({ "type": "auth", "token": admin_token }))
        .await;
    let host_initial = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(host_initial["active"], false);

    let mut controller_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    controller_ws
        .send_json(&json!({ "type": "auth", "token": controller_token }))
        .await;
    let controller_initial = receive_ws_json_of_type(&mut controller_ws, "screen_state").await;
    assert_eq!(controller_initial["active"], false);

    let mut viewer_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    viewer_ws
        .send_json(&json!({ "type": "auth", "token": viewer_token }))
        .await;
    let viewer_initial = receive_ws_json_of_type(&mut viewer_ws, "screen_state").await;
    assert_eq!(viewer_initial["active"], false);

    let mut outsider_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    outsider_ws
        .send_json(&json!({ "type": "auth", "token": outsider_token }))
        .await;
    let outsider_error = receive_ws_json_of_type(&mut outsider_ws, "error").await;
    assert!(
        outsider_error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("room membership not found")
    );

    viewer_ws
        .send_json(&json!({
            "type": "screen_claim"
        }))
        .await;
    let viewer_error = receive_ws_json_of_type(&mut viewer_ws, "error").await;
    assert!(
        viewer_error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("host or controller")
    );

    controller_ws
        .send_json(&json!({
            "type": "screen_claim"
        }))
        .await;
    let host_claimed = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(host_claimed["active"], false);
    assert_eq!(host_claimed["presenter_user_id"], controller_id);
    assert_eq!(host_claimed["presenter_state"], "requesting_capture");
    let controller_claimed = receive_ws_json_of_type(&mut controller_ws, "screen_state").await;
    assert_eq!(controller_claimed["presenter_user_id"], controller_id);
    let viewer_claimed = receive_ws_json_of_type(&mut viewer_ws, "screen_state").await;
    assert_eq!(viewer_claimed["presenter_user_id"], controller_id);

    host_ws
        .send_json(&json!({
            "type": "screen_claim"
        }))
        .await;
    let host_lock_error = receive_ws_json_of_type(&mut host_ws, "error").await;
    assert!(
        host_lock_error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("locked by another presenter")
    );

    controller_ws
        .send_json(&json!({
            "type": "screen_start",
            "surface_type": "window",
            "audio_enabled": true,
            "quality_profile": "auto"
        }))
        .await;
    let host_live = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(host_live["active"], true);
    let controller_live = receive_ws_json_of_type(&mut controller_ws, "screen_state").await;
    assert_eq!(controller_live["active"], true);
    let viewer_live = receive_ws_json_of_type(&mut viewer_ws, "screen_state").await;
    assert_eq!(viewer_live["active"], true);
    let session_id = viewer_live["session_id"].as_str().unwrap().to_string();

    viewer_ws
        .send_json(&json!({
            "type": "screen_offer",
            "to_user_id": controller_id,
            "session_id": session_id,
            "sdp": "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\n"
        }))
        .await;
    let controller_offer = receive_ws_json_of_type(&mut controller_ws, "screen_offer").await;
    assert_eq!(controller_offer["from_user_id"], viewer_id);

    host_ws
        .send_json(&json!({
            "type": "screen_force_stop"
        }))
        .await;
    let host_force_stop_error = receive_ws_json_of_type(&mut host_ws, "error").await;
    assert!(
        host_force_stop_error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("force-stop is disabled")
    );

    controller_ws
        .send_json(&json!({
            "type": "screen_stop"
        }))
        .await;
    let host_stopped = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(host_stopped["active"], false);
    let controller_stopped = receive_ws_json_of_type(&mut controller_ws, "screen_state").await;
    assert_eq!(controller_stopped["active"], false);
    let viewer_stopped = receive_ws_json_of_type(&mut viewer_ws, "screen_state").await;
    assert_eq!(viewer_stopped["active"], false);

    host_ws.close().await;
    controller_ws.close().await;
    viewer_ws.close().await;
    outsider_ws.close().await;
}

#[tokio::test]
async fn watch_party_screen_session_resets_when_presenter_disconnects() {
    let server = test_app_http().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    let controller_id = create_user_with_libraries(
        &server,
        &admin_hdr,
        "screen_disconnect_controller",
        "screen_disconnect_controller_pass_123",
        "user",
        &[],
    )
    .await;

    let room_id = create_watch_party_room(
        &server,
        &admin_hdr,
        json!({
            "room_mode": "screen",
            "invites": [
                { "user_id": controller_id, "role": "controller" }
            ]
        }),
    )
    .await;

    let controller_token = login(
        &server,
        "screen_disconnect_controller",
        "screen_disconnect_controller_pass_123",
    )
    .await;
    let controller_hdr = auth_hdr(&controller_token);
    server
        .post(&format!("/api/v1/watch-party/rooms/{room_id}/join"))
        .add_header(controller_hdr.0.clone(), controller_hdr.1.clone())
        .json(&json!({}))
        .await
        .assert_status_ok();

    let ws_host_header = axum::http::header::HOST;
    let ws_host_value = "watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();
    let ws_origin_header = axum::http::header::ORIGIN;
    let ws_origin_value = "http://watchparty.test"
        .parse::<axum::http::HeaderValue>()
        .unwrap();

    let mut host_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    host_ws
        .send_json(&json!({ "type": "auth", "token": admin_token }))
        .await;
    let _ = receive_ws_json_of_type(&mut host_ws, "screen_state").await;

    let mut controller_ws = server
        .get_websocket(&format!("/api/v1/watch-party/rooms/{room_id}/ws"))
        .add_header(ws_host_header.clone(), ws_host_value.clone())
        .add_header(ws_origin_header.clone(), ws_origin_value.clone())
        .await
        .into_websocket()
        .await;
    controller_ws
        .send_json(&json!({ "type": "auth", "token": controller_token }))
        .await;
    let _ = receive_ws_json_of_type(&mut controller_ws, "screen_state").await;

    controller_ws
        .send_json(&json!({
            "type": "screen_start",
            "surface_type": "monitor",
            "audio_enabled": false,
            "quality_profile": "motion"
        }))
        .await;
    let live_state = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(live_state["active"], true);

    controller_ws.close().await;

    let reset_state = receive_ws_json_of_type(&mut host_ws, "screen_state").await;
    assert_eq!(reset_state["active"], false);
    assert!(reset_state["presenter_user_id"].is_null());

    host_ws.close().await;
}

// ---------------------------------------------------------------------------
// Setup wizard tests
// ---------------------------------------------------------------------------

/// Create a test server in fresh (uncompleted setup) state.
async fn test_app_fresh() -> TestServer {
    let pool = create_test_pool().await;
    rustfin_db::repo::settings::insert_defaults(&pool)
        .await
        .unwrap();

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: std::env::temp_dir().join(format!("rf_setup_{}", std::process::id())),
        max_concurrent: 2,
        ..Default::default()
    };
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = build_test_state(
        pool,
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        events_tx,
        std::env::temp_dir().join(format!("rf_cache_setup_{}", std::process::id())),
        std::env::temp_dir().join(format!("rf_watch_audio_setup_{}", std::process::id())),
    );

    let app = build_router(state);
    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn user_library_access_is_enforced() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    // Create two libraries
    let tmp_a = std::env::temp_dir().join(format!("rf_access_a_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_a).unwrap();
    let tmp_b = std::env::temp_dir().join(format!("rf_access_b_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_b).unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "name": "Movies A", "kind": "movies", "paths": [tmp_a.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let lib_a: Value = resp.json();
    let lib_a_id = lib_a["id"].as_str().unwrap().to_string();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "name": "Movies B", "kind": "movies", "paths": [tmp_b.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let lib_b: Value = resp.json();
    let lib_b_id = lib_b["id"].as_str().unwrap().to_string();

    // Create simple user with access only to library A
    let resp = server
        .post("/api/v1/users")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "username": "viewer",
            "password": "viewerpass_sec",
            "role": "user",
            "library_ids": [lib_a_id]
        }))
        .await;
    resp.assert_status_ok();

    let viewer_token = login(&server, "viewer", "viewerpass_sec").await;
    let viewer_hdr = auth_hdr(&viewer_token);

    // Viewer sees only one library
    let resp = server
        .get("/api/v1/libraries")
        .add_header(viewer_hdr.0.clone(), viewer_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let libs: Vec<Value> = resp.json();
    assert_eq!(libs.len(), 1);
    assert_eq!(libs[0]["id"], lib_a["id"]);

    // Viewer can access assigned library
    let resp = server
        .get(&format!("/api/v1/libraries/{lib_a_id}"))
        .add_header(viewer_hdr.0.clone(), viewer_hdr.1.clone())
        .await;
    resp.assert_status_ok();

    // Viewer cannot access unassigned library
    let resp = server
        .get(&format!("/api/v1/libraries/{lib_b_id}"))
        .add_header(viewer_hdr.0.clone(), viewer_hdr.1.clone())
        .await;
    resp.assert_status(axum::http::StatusCode::FORBIDDEN);

    std::fs::remove_dir_all(&tmp_a).ok();
    std::fs::remove_dir_all(&tmp_b).ok();
}

#[tokio::test]
async fn admin_can_modify_user_permissions() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let admin_hdr = auth_hdr(&admin_token);

    // Create two libraries
    let tmp_1 = std::env::temp_dir().join(format!("rf_perm_1_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_1).unwrap();
    let tmp_2 = std::env::temp_dir().join(format!("rf_perm_2_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_2).unwrap();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "name": "Lib 1", "kind": "movies", "paths": [tmp_1.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let lib1: Value = resp.json();
    let lib1_id = lib1["id"].as_str().unwrap().to_string();

    let resp = server
        .post("/api/v1/libraries")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "name": "Lib 2", "kind": "movies", "paths": [tmp_2.to_str().unwrap()] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let lib2: Value = resp.json();
    let lib2_id = lib2["id"].as_str().unwrap().to_string();

    // Create user with Lib1 access
    let resp = server
        .post("/api/v1/users")
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "username": "limited",
            "password": "limitedpass_sec",
            "role": "user",
            "library_ids": [lib1_id]
        }))
        .await;
    resp.assert_status_ok();
    let created: Value = resp.json();
    let user_id = created["id"].as_str().unwrap().to_string();

    // Move user access to Lib2
    let resp = server
        .patch(&format!("/api/v1/users/{user_id}"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({
            "role": "user",
            "library_ids": [lib2_id]
        }))
        .await;
    resp.assert_status_ok();
    let patched: Value = resp.json();
    assert_eq!(patched["role"], "user");
    assert_eq!(patched["library_ids"][0], lib2["id"]);

    let limited_token = login(&server, "limited", "limitedpass_sec").await;
    let limited_hdr = auth_hdr(&limited_token);
    let resp = server
        .get("/api/v1/libraries")
        .add_header(limited_hdr.0.clone(), limited_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let libs: Vec<Value> = resp.json();
    assert_eq!(libs.len(), 1);
    assert_eq!(libs[0]["id"], lib2["id"]);

    // Promote to admin; admin should see both libraries
    let resp = server
        .patch(&format!("/api/v1/users/{user_id}"))
        .add_header(admin_hdr.0.clone(), admin_hdr.1.clone())
        .json(&json!({ "role": "admin" }))
        .await;
    resp.assert_status_ok();
    let patched: Value = resp.json();
    assert_eq!(patched["role"], "admin");

    let limited_token = login(&server, "limited", "limitedpass_sec").await;
    let limited_hdr = auth_hdr(&limited_token);
    let resp = server
        .get("/api/v1/libraries")
        .add_header(limited_hdr.0.clone(), limited_hdr.1.clone())
        .await;
    resp.assert_status_ok();
    let libs: Vec<Value> = resp.json();
    assert_eq!(libs.len(), 2);

    std::fs::remove_dir_all(&tmp_1).ok();
    std::fs::remove_dir_all(&tmp_2).ok();
}

#[tokio::test]
async fn public_info_shows_setup_incomplete_on_fresh_db() {
    let server = test_app_fresh().await;
    let resp = server.get("/api/v1/system/info/public").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["setup_completed"], false);
    assert_eq!(body["setup_state"], "NotStarted");
    assert_eq!(body["server_name"], "Rustyfin");
    assert!(body["version"].as_str().is_some());
}

#[tokio::test]
async fn public_info_shows_completed_on_existing_install() {
    let server = test_app().await;
    let resp = server.get("/api/v1/system/info/public").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["setup_completed"], true);
    assert_eq!(body["setup_state"], "Completed");
}

#[tokio::test]
async fn setup_claim_and_release_session() {
    let server = test_app_fresh().await;

    // Claim session
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "TestUI",
            "force": false,
            "confirm_takeover": false
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let token = body["owner_token"].as_str().unwrap().to_string();
    assert_eq!(body["claimed_by"], "TestUI");
    assert!(!token.is_empty());
    let owner_hdr = axum::http::HeaderName::from_static("x-setup-owner-token");
    let remote_hdr = axum::http::HeaderName::from_static("x-setup-remote-token");
    let token_val = token.parse::<axum::http::HeaderValue>().unwrap();

    // Second claim without force should 409
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "OtherUI",
            "force": false,
            "confirm_takeover": false
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CONFLICT);
    let body: Value = resp.json();
    assert_eq!(body["error"]["code"], "setup_claimed");

    // Release session
    let resp = server
        .post("/api/v1/setup/session/release")
        .add_header(owner_hdr, token_val.clone())
        .add_header(remote_hdr, token_val)
        .add_header(localhost_header().0, localhost_header().1)
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["released"], true);
}

#[tokio::test]
async fn setup_full_wizard_flow() {
    let server = test_app_fresh().await;

    // Step 1: Claim session
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "TestUI",
            "force": false,
            "confirm_takeover": false
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let token = body["owner_token"].as_str().unwrap().to_string();

    let owner_hdr = axum::http::HeaderName::from_static("x-setup-owner-token");
    let remote_hdr = axum::http::HeaderName::from_static("x-setup-remote-token");
    let owner_val: axum::http::HeaderValue = token.parse().unwrap();
    let remote_val = owner_val.clone();
    let (host_hdr, host_val) = localhost_header();

    // Step 2: PUT config
    let resp = server
        .put("/api/v1/setup/config")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .json(&json!({
            "server_name": "My Rustyfin",
            "default_ui_locale": "en-US",
            "default_region": "US",
            "default_time_zone": "America/New_York"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["setup_state"], "ServerConfigSaved");

    // Step 3: Create admin
    let resp = server
        .post("/api/v1/setup/admin")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .add_header(
            axum::http::HeaderName::from_static("idempotency-key"),
            "test-idem-key-12345678"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "username": "myadmin",
            "password": "supersecurepassword123"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = resp.json();
    assert!(body["user_id"].as_str().is_some());
    assert_eq!(body["setup_state"], "AdminCreated");

    // Step 3b: Idempotent replay with same key
    let resp = server
        .post("/api/v1/setup/admin")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .add_header(
            axum::http::HeaderName::from_static("idempotency-key"),
            "test-idem-key-12345678"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "username": "myadmin",
            "password": "supersecurepassword123"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);

    // Step 4: PUT metadata (skipping libraries since they're optional)
    let resp = server
        .put("/api/v1/setup/metadata")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .json(&json!({
            "metadata_language": "en",
            "metadata_region": "US"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["setup_state"], "MetadataSaved");

    // Step 5: PUT network
    let resp = server
        .put("/api/v1/setup/network")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .json(&json!({
            "allow_remote_access": false,
            "trusted_proxies": []
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["setup_state"], "NetworkSaved");

    // Step 6: Complete
    let resp = server
        .post("/api/v1/setup/complete")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .json(&json!({ "confirm": true }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["setup_completed"], true);
    assert_eq!(body["setup_state"], "Completed");

    // Verify: public info now shows completed
    let resp = server.get("/api/v1/system/info/public").await;
    let body: Value = resp.json();
    assert_eq!(body["setup_completed"], true);
    assert_eq!(body["server_name"], "My Rustyfin");

    // Verify: admin can login
    let resp = server
        .post("/api/v1/auth/login")
        .json(&json!({ "username": "myadmin", "password": "supersecurepassword123" }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["role"], "admin");
}

#[tokio::test]
async fn setup_state_machine_enforces_order() {
    let server = test_app_fresh().await;

    // Try to put config without claiming session first — should fail (no token)
    let resp = server
        .put("/api/v1/setup/config")
        .json(&json!({
            "server_name": "Test",
            "default_ui_locale": "en",
            "default_region": "US"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_validation_rejects_weak_password() {
    let server = test_app_fresh().await;

    // Claim session
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "TestUI",
            "force": false,
            "confirm_takeover": false
        }))
        .await;
    let body: Value = resp.json();
    let token = body["owner_token"].as_str().unwrap().to_string();
    let owner_hdr = axum::http::HeaderName::from_static("x-setup-owner-token");
    let remote_hdr = axum::http::HeaderName::from_static("x-setup-remote-token");
    let owner_val: axum::http::HeaderValue = token.parse().unwrap();
    let remote_val = owner_val.clone();
    let (host_hdr, host_val) = localhost_header();

    // Put config first
    let resp = server
        .put("/api/v1/setup/config")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .json(&json!({
            "server_name": "Test",
            "default_ui_locale": "en",
            "default_region": "US"
        }))
        .await;
    resp.assert_status_ok();

    // Try to create admin with short password
    let resp = server
        .post("/api/v1/setup/admin")
        .add_header(owner_hdr.clone(), owner_val.clone())
        .add_header(remote_hdr.clone(), remote_val.clone())
        .add_header(host_hdr.clone(), host_val.clone())
        .add_header(
            axum::http::HeaderName::from_static("idempotency-key"),
            "validate-test-key123"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "username": "admin",
            "password": "short"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    let body: Value = resp.json();
    assert_eq!(body["error"]["code"], "validation_failed");
    assert!(body["error"]["details"]["fields"]["password"].is_array());
}

#[tokio::test]
async fn setup_force_takeover() {
    let server = test_app_fresh().await;

    // First claim
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "Browser1",
            "force": false,
            "confirm_takeover": false
        }))
        .await;
    resp.assert_status_ok();

    // Force takeover
    let resp = server
        .post("/api/v1/setup/session/claim")
        .json(&json!({
            "client_name": "Browser2",
            "force": true,
            "confirm_takeover": true
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["claimed_by"], "Browser2");
}

#[tokio::test]
async fn vault_bootstrap_persists_and_returns_enabled_config() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    let session = create_vault_session(&server, &admin_token, "Config Test Vault").await;
    let access_token = session["session"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    let mut initial = server.get("/api/v1/vault/config");
    for (name, value) in auth_and_vault_headers(&admin_token, &access_token).await {
        initial = initial.add_header(name, value);
    }
    let initial = initial.await;
    initial.assert_status_ok();
    let initial_body: Value = initial.json();
    assert_eq!(initial_body["enabled"], false);
    assert!(initial_body["active_wrapped_key"].is_null());

    bootstrap_vault_for_user_with_access(&server, &admin_token, &access_token).await;

    let mut persisted = server.get("/api/v1/vault/config");
    for (name, value) in auth_and_vault_headers(&admin_token, &access_token).await {
        persisted = persisted.add_header(name, value);
    }
    let persisted = persisted.await;
    persisted.assert_status_ok();
    let persisted_body: Value = persisted.json();
    assert_eq!(persisted_body["enabled"], true);
    assert_eq!(persisted_body["schema_version"], 1);
    assert_eq!(persisted_body["item_count"], 0);
    assert_eq!(
        persisted_body["active_wrapped_key"]["key_version"],
        serde_json::Value::from(1)
    );
}

#[tokio::test]
async fn vault_bootstrap_requires_rustyvault_session() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;

    let resp = server
        .post("/api/v1/vault/bootstrap")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {admin_token}")
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .json(&json!({
            "wrapped_key": {
                "key_version": 1,
                "kdf_algorithm": "argon2id",
                "kdf_memory_kib": 65536,
                "kdf_iterations": 3,
                "kdf_parallelism": 4,
                "kdf_salt_hex": "00112233445566778899aabbccddeeff",
                "hkdf_algorithm": "hkdf-sha-256",
                "wrap_algorithm": "aes-256-gcm",
                "wrap_nonce_hex": "00112233445566778899aabb",
                "wrapped_vault_key_hex": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                "created_ts": 0
            }
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn vault_protected_action_challenge_rejects_mismatched_auth_and_session() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    create_user_as_admin(&server, &admin_token, "vault_user_d", "vault_pass_d_123").await;
    let user_d_token = login(&server, "vault_user_d", "vault_pass_d_123").await;

    let user_d_session = create_vault_session(&server, &user_d_token, "User D Guard Session").await;
    let user_d_access = user_d_session["session"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    let mut request = server.post("/api/v1/vault/protected-actions/challenge");
    for (name, value) in auth_and_vault_headers(&admin_token, &user_d_access).await {
        request = request.add_header(name, value);
    }
    let resp = request
        .json(&json!({
            "action_kind": "approve_device",
            "current_password": "admin_secure_123"
        }))
        .await;

    resp.assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn vault_item_endpoints_enforce_user_ownership() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    create_user_as_admin(&server, &admin_token, "vault_user_b", "vault_pass_b_123").await;
    let user_b_token = login(&server, "vault_user_b", "vault_pass_b_123").await;

    bootstrap_vault_for_user(&server, &admin_token).await;
    bootstrap_vault_for_user(&server, &user_b_token).await;

    let admin_session = create_vault_session(&server, &admin_token, "Admin Web Vault").await;
    let user_b_session = create_vault_session(&server, &user_b_token, "User B Web Vault").await;
    let admin_access = admin_session["session"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();
    let user_b_access = user_b_session["session"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    create_vault_item(
        &server,
        &admin_token,
        &admin_access,
        "vault-item-admin-1",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .await;

    let mut get_request = server.get("/api/v1/vault/items/vault-item-admin-1");
    for (name, value) in auth_and_vault_headers(&user_b_token, &user_b_access).await {
        get_request = get_request.add_header(name, value);
    }
    let get_resp = get_request.await;
    get_resp.assert_status(axum::http::StatusCode::NOT_FOUND);

    let mut put_request = server.put("/api/v1/vault/items/vault-item-admin-1");
    for (name, value) in auth_and_vault_headers(&user_b_token, &user_b_access).await {
        put_request = put_request.add_header(name, value);
    }
    let put_resp = put_request
        .json(&json!({
            "id": "vault-item-admin-1",
            "item_type": "login",
            "key_version": 1,
            "summary_version": 1,
            "summary_nonce_hex": "00112233445566778899aabb",
            "summary_ciphertext_hex": "00112233445566778899aabbccddeeff",
            "payload_version": 1,
            "payload_nonce_hex": "00112233445566778899aabb",
            "payload_ciphertext_hex": "00112233445566778899aabbccddeeff0011223344556677",
            "favorite": false,
            "revision": 2,
            "uri_indexes": []
        }))
        .await;
    put_resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn vault_lookup_is_user_scoped() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    create_user_as_admin(&server, &admin_token, "vault_user_c", "vault_pass_c_123").await;
    let user_c_token = login(&server, "vault_user_c", "vault_pass_c_123").await;

    bootstrap_vault_for_user(&server, &admin_token).await;
    bootstrap_vault_for_user(&server, &user_c_token).await;

    let admin_session = create_vault_session(&server, &admin_token, "Admin Web Vault").await;
    let user_c_session = create_vault_session(&server, &user_c_token, "User C Web Vault").await;
    let admin_access = admin_session["session"]["access_token"].as_str().unwrap();
    let user_c_access = user_c_session["session"]["access_token"].as_str().unwrap();

    create_vault_item(
        &server,
        &admin_token,
        admin_access,
        "vault-item-admin-lookup",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .await;

    let mut lookup_request = server.post("/api/v1/vault/lookup");
    for (name, value) in auth_and_vault_headers(&user_c_token, user_c_access).await {
        lookup_request = lookup_request.add_header(name, value);
    }
    let lookup_resp = lookup_request
        .json(&json!({
            "match_hashes_hex": [
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            ]
        }))
        .await;
    lookup_resp.assert_status_ok();
    let lookup_body: Value = lookup_resp.json();
    assert_eq!(lookup_body["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn vault_refresh_token_replay_revokes_the_session_family() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    bootstrap_vault_for_user(&server, &admin_token).await;
    let session = create_vault_session(&server, &admin_token, "Replay Test Vault").await;
    let first_refresh = session["session"]["refresh_token"]
        .as_str()
        .unwrap()
        .to_string();

    let rotate_resp = server
        .post("/api/v1/vault/device-sessions/refresh")
        .json(&json!({ "refresh_token": first_refresh }))
        .await;
    rotate_resp.assert_status_ok();
    let rotated_body: Value = rotate_resp.json();
    let second_refresh = rotated_body["refresh_token"].as_str().unwrap().to_string();

    let replay_resp = server
        .post("/api/v1/vault/device-sessions/refresh")
        .json(&json!({ "refresh_token": first_refresh }))
        .await;
    replay_resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let post_replay_resp = server
        .post("/api/v1/vault/device-sessions/refresh")
        .json(&json!({ "refresh_token": second_refresh }))
        .await;
    post_replay_resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn vault_responses_include_security_headers_on_unauthorized_requests() {
    let server = test_app().await;

    let resp = server.get("/api/v1/vault/config").await;

    assert_eq!(resp.status_code(), axum::http::StatusCode::UNAUTHORIZED);
    assert_eq!(
        resp.maybe_header(axum::http::header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store, max-age=0, must-revalidate"
    );
    assert_eq!(
        resp.maybe_header(axum::http::header::PRAGMA)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-cache"
    );
    assert_eq!(
        resp.maybe_header(axum::http::header::REFERRER_POLICY)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-referrer"
    );
    assert_eq!(
        resp.maybe_header(axum::http::header::X_CONTENT_TYPE_OPTIONS)
            .unwrap()
            .to_str()
            .unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn vault_lookup_requests_are_rate_limited_per_session() {
    let server = test_app().await;
    let admin_token = login(&server, "admin", "admin_secure_123").await;
    bootstrap_vault_for_user(&server, &admin_token).await;
    let session = create_vault_session(&server, &admin_token, "Lookup Rate Limit Vault").await;
    let access_token = session["session"]["access_token"]
        .as_str()
        .unwrap()
        .to_string();

    for _ in 0..rustfin_server::rustyvault_host::middleware::VAULT_LOOKUP_RATE_LIMIT_REQUESTS {
        let mut lookup_request = server.post("/api/v1/vault/lookup");
        for (name, value) in auth_and_vault_headers(&admin_token, &access_token).await {
            lookup_request = lookup_request.add_header(name, value);
        }
        let lookup_resp = lookup_request
            .json(&json!({
                "match_hashes_hex": [
                    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                ]
            }))
            .await;
        lookup_resp.assert_status_ok();
    }

    let mut limited_request = server.post("/api/v1/vault/lookup");
    for (name, value) in auth_and_vault_headers(&admin_token, &access_token).await {
        limited_request = limited_request.add_header(name, value);
    }
    let limited_resp = limited_request
        .json(&json!({
            "match_hashes_hex": [
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            ]
        }))
        .await;

    limited_resp.assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);
    let retry_after_seconds = limited_resp
        .maybe_header(axum::http::header::RETRY_AFTER)
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!((1..=60).contains(&retry_after_seconds));
    assert_eq!(
        limited_resp
            .maybe_header(axum::http::header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store, max-age=0, must-revalidate"
    );
}
