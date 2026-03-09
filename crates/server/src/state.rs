use std::sync::Arc;

use rustfin_db::DbPool;

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
    MetadataRefresh { item_id: String, status: String },
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
    pub db: DbPool,
    pub jwt_secret: String,
    pub http: reqwest::Client,
    pub tmdb_agent_url: String,
    pub tmdb_agent_token: Option<String>,
    pub youtube_agent_url: String,
    pub youtube_agent_token: Option<String>,
    pub transcription_agent_url: String,
    pub transcription_agent_token: Option<String>,
    pub servers_agent_url: Option<String>,
    pub servers_agent_token: Option<String>,
    pub transcoder: Arc<rustfin_transcoder::session::SessionManager>,
    pub ffmpeg_path: std::path::PathBuf,
    pub ffprobe_path: std::path::PathBuf,
    pub transcoder_hw_accel: Option<rustfin_transcoder::HwAccel>,
    pub transcoder_hw_accel_required: bool,
    pub cache_dir: std::path::PathBuf,
    pub watch_party_audio_dir: std::path::PathBuf,
    pub events: tokio::sync::broadcast::Sender<ServerEvent>,
    pub watch_party: Arc<crate::watch_party::manager::WatchPartyManager>,
    pub channel_manager: Arc<crate::channels::manager::ChannelManager>,
}
