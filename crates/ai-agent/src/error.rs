use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("model directory error: {0}")]
    ModelDirError(String),
    #[error("inference error: {0}")]
    InferenceError(String),
    #[error("download error: {0}")]
    DownloadError(String),
    #[error("context build error: {0}")]
    ContextError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
