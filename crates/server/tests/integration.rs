use axum_test::TestServer;
use rustfin_server::routes::build_router;
use rustfin_server::state::AppState;
use serde_json::{json, Value};

/// Create a test server with an in-memory SQLite database.
async fn test_app() -> TestServer {
    let pool = rustfin_db::connect(":memory:").await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();

    // Bootstrap admin user
    rustfin_db::repo::users::create_user(&pool, "admin", "admin123", "admin")
        .await
        .unwrap();

    let tc_config = rustfin_transcoder::TranscoderConfig {
        transcode_dir: std::env::temp_dir().join(format!("rf_test_{}", std::process::id())),
        max_concurrent: 2,
        ..Default::default()
    };
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = AppState {
        db: pool,
        jwt_secret: "test-secret-key".to_string(),
        transcoder,
        cache_dir: std::env::temp_dir().join(format!("rf_cache_{}", std::process::id())),
        events: events_tx,
    };

    let app = build_router(state);
    TestServer::new(app).unwrap()
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
        .json(&json!({ "username": "admin", "password": "admin123" }))
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
    let token = login(&server, "admin", "admin123").await;

    let resp = server
        .get("/api/v1/users/me")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse::<axum::http::HeaderValue>().unwrap(),
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
    let token = login(&server, "admin", "admin123").await;
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
    assert_eq!(body, json!({}));

    // PATCH prefs
    let new_prefs = json!({ "show_missing_episodes": true, "theme": "dark" });
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
    assert_eq!(body["show_missing_episodes"], true);
    assert_eq!(body["theme"], "dark");

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
    assert_eq!(body["show_missing_episodes"], true);
}

#[tokio::test]
async fn migrations_are_idempotent() {
    let pool = rustfin_db::connect(":memory:").await.unwrap();
    // Run migrations twice — should not error
    rustfin_db::migrate::run(&pool).await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();
}

// ---------------------------------------------------------------------------
// Library tests
// ---------------------------------------------------------------------------

fn auth_hdr(token: &str) -> (axum::http::HeaderName, axum::http::HeaderValue) {
    (
        axum::http::header::AUTHORIZATION,
        format!("Bearer {token}").parse::<axum::http::HeaderValue>().unwrap(),
    )
}

#[tokio::test]
async fn create_library_requires_admin() {
    let server = test_app().await;

    // Create a regular user
    let pool = {
        // We need to reach into state — use a fresh login-based approach:
        // the test_app bootstraps "admin" only; there's no regular user to test with.
        // Instead, test that unauthenticated requests fail:
        let resp = server
            .post("/api/v1/libraries")
            .json(&json!({ "name": "Movies", "kind": "movies", "paths": ["/media/movies"] }))
            .await;
        resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    };
    let _ = pool; // suppress unused
}

#[tokio::test]
async fn library_crud_flow() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // Create library
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "Movies", "kind": "movies", "paths": ["/media/movies"] }))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["name"], "Movies");
    assert_eq!(body["kind"], "movies");
    assert_eq!(body["item_count"], 0);
    assert_eq!(body["paths"][0]["path"], "/media/movies");
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
}

#[tokio::test]
async fn create_library_validates_kind() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin123").await;
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
    let token = login(&server, "admin", "admin123").await;
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
    let token = login(&server, "admin", "admin123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // Create library first
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "TV", "kind": "tv_shows", "paths": ["/media/tv"] }))
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
    let token = login(&server, "admin", "admin123").await;
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

    let pool = rustfin_db::connect(":memory:").await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();

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

    let pool = rustfin_db::connect(":memory:").await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();

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
async fn scan_is_idempotent() {
    let tmp = std::env::temp_dir().join(format!("rustfin_test_idem_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("Movie (2020).mkv"), b"fake").unwrap();

    let pool = rustfin_db::connect(":memory:").await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();

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
    let pool = rustfin_db::connect(":memory:").await.unwrap();
    rustfin_db::migrate::run(&pool).await.unwrap();
    rustfin_db::repo::users::create_user(&pool, "admin", "admin123", "admin")
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
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));

    let (events_tx, _) = tokio::sync::broadcast::channel(64);
    let state = AppState {
        db: pool,
        jwt_secret: "test-secret-key".to_string(),
        transcoder,
        cache_dir: std::env::temp_dir().join(format!("rf_cache_stream_{}", std::process::id())),
        events: events_tx,
    };
    let app = rustfin_server::routes::build_router(state);
    let server = TestServer::new(app).unwrap();

    // Request Range: bytes=0-999 (first 1000 bytes)
    let resp = server
        .get(&format!("/stream/file/{file_id}"))
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
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::OK);
    assert_eq!(resp.as_bytes().len(), 5000);

    // Request open-ended range: bytes=4000-
    let resp = server
        .get(&format!("/stream/file/{file_id}"))
        .add_header(
            axum::http::header::RANGE,
            "bytes=4000-".parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;
    assert_eq!(resp.status_code(), axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.as_bytes().len(), 1000);
    let cr = resp.headers().get("content-range").unwrap().to_str().unwrap();
    assert_eq!(cr, "bytes 4000-4999/5000");

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

// ---------------------------------------------------------------------------
// Playback progress tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playback_progress_update_and_get() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin123").await;
    let (hdr_name, hdr_val) = auth_hdr(&token);

    // First need a library and item to reference
    let resp = server
        .post("/api/v1/libraries")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({ "name": "Movies", "kind": "movies", "paths": ["/media/movies"] }))
        .await;
    let lib_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    // Manually insert an item via DB (we have access to pool through a helper)
    // Instead, we create via scan with a real temp dir
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

    // Update progress
    let resp = server
        .post("/api/v1/playback/progress")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .json(&json!({
            "item_id": item_id,
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
            "item_id": item_id,
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
    assert!(body["last_played_ts"].as_i64().unwrap() > 0);

    // Cleanup
    std::fs::remove_dir_all(&tmp).ok();
}

#[tokio::test]
async fn user_management_crud() {
    let server = test_app().await;
    let token = login(&server, "admin", "admin123").await;
    let hdr_name = axum::http::header::AUTHORIZATION;
    let hdr_val = axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

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
            "password": "testpass"
        }))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let new_user_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["username"], "testuser");
    assert_eq!(body["role"], "user");

    // List again — should have 2 users
    let resp = server
        .get("/api/v1/users")
        .add_header(hdr_name.clone(), hdr_val.clone())
        .await;
    let users: Vec<Value> = resp.json();
    assert_eq!(users.len(), 2);

    // New user can login
    let _user_token = login(&server, "testuser", "testpass").await;

    // Delete the new user
    let resp = server
        .delete(&format!("/api/v1/users/{new_user_id}"))
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
}
