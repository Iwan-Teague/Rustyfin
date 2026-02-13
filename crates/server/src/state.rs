use std::sync::Arc;

use sqlx::SqlitePool;

/// Server-sent event types.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerEvent {
    #[serde(rename = "scan_progress")]
    ScanProgress {
        library_id: String,
        job_id: String,
        progress: f64,
        message: String,
    },
    #[serde(rename = "scan_complete")]
    ScanComplete {
        library_id: String,
        job_id: String,
        items_added: u64,
    },
    #[serde(rename = "metadata_refresh")]
    MetadataRefresh {
        item_id: String,
        status: String,
    },
    #[serde(rename = "job_update")]
    JobUpdate {
        job_id: String,
        status: String,
        progress: f64,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat { seq: u64 },
}

/// Shared application state passed to all handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub jwt_secret: String,
    pub transcoder: Arc<rustfin_transcoder::session::SessionManager>,
    pub cache_dir: std::path::PathBuf,
    pub events: tokio::sync::broadcast::Sender<ServerEvent>,
}
