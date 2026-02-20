#![allow(
    clippy::collapsible_if,
    clippy::redundant_closure,
    clippy::unused_async
)]
pub mod decision;
pub mod ffprobe;
pub mod gpu;
pub mod hls;
pub mod session;

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TranscodeError {
    #[error("ffmpeg/ffprobe not found at {0}")]
    BinaryNotFound(PathBuf),
    #[error("ffprobe failed: {0}")]
    ProbeFailed(String),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("max transcodes reached ({0})")]
    MaxTranscodesReached(usize),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Global transcoder configuration.
#[derive(Debug, Clone)]
pub struct TranscoderConfig {
    pub ffmpeg_path: PathBuf,
    pub ffprobe_path: PathBuf,
    pub transcode_dir: PathBuf,
    pub max_concurrent: usize,
    pub segment_secs: u32,
    pub idle_timeout_secs: u64,
    pub hw_accel: Option<HwAccel>,
}

impl Default for TranscoderConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: PathBuf::from("ffmpeg"),
            ffprobe_path: PathBuf::from("ffprobe"),
            transcode_dir: PathBuf::from("/tmp/rustfin_transcode"),
            max_concurrent: 4,
            segment_secs: 4,
            idle_timeout_secs: 60,
            hw_accel: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum HwAccel {
    Nvenc,
    Vaapi,
    Qsv,
    VideoToolbox,
}
