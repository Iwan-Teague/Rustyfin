use std::sync::{Mutex, OnceLock};

use llama_cpp_2::llama_backend::LlamaBackend;

use crate::error::AiError;

static LLAMA_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
static LLAMA_BACKEND_INIT: Mutex<()> = Mutex::new(());

pub(crate) fn shared_backend() -> Result<&'static LlamaBackend, AiError> {
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend);
    }

    let _guard = LLAMA_BACKEND_INIT
        .lock()
        .map_err(|_| AiError::ContextError("failed to acquire backend init lock".to_string()))?;

    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(backend);
    }

    let backend = LlamaBackend::init().map_err(|error| {
        AiError::ContextError(format!("failed to initialize llama backend: {error}"))
    })?;
    let _ = LLAMA_BACKEND.set(backend);

    LLAMA_BACKEND
        .get()
        .ok_or_else(|| AiError::ContextError("llama backend initialization failed".to_string()))
}
