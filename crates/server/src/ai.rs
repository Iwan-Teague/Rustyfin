#[cfg(not(feature = "ai"))]
use axum::Json;
#[cfg(not(feature = "ai"))]
use axum::Router;
#[cfg(not(feature = "ai"))]
use axum::http::StatusCode;

use crate::state::AppState;

#[cfg(feature = "ai")]
pub use crate::ai_enabled::{EngineState, ai_router};

#[cfg(not(feature = "ai"))]
use axum::response::IntoResponse;
#[cfg(not(feature = "ai"))]
use axum::routing::any;
#[cfg(not(feature = "ai"))]
use serde_json::json;

pub fn inference_available() -> bool {
    cfg!(feature = "ai")
}

#[cfg(feature = "ai")]
pub async fn clear_loaded_model_state(state: &AppState) {
    let mut guard = state.engine.lock().await;
    guard.loaded_model = None;
    guard.engine = None;
}

#[cfg(not(feature = "ai"))]
pub async fn clear_loaded_model_state(_state: &AppState) {}

#[cfg(feature = "ai")]
pub async fn clear_loaded_model_if_matching(state: &AppState, model_name: &str) {
    let mut guard = state.engine.lock().await;
    if guard.loaded_model.as_deref() == Some(model_name) {
        guard.loaded_model = None;
        guard.engine = None;
    }
}

#[cfg(not(feature = "ai"))]
pub async fn clear_loaded_model_if_matching(_state: &AppState, _model_name: &str) {}

#[cfg(not(feature = "ai"))]
#[derive(Default)]
pub struct EngineState;

#[cfg(not(feature = "ai"))]
pub fn ai_router() -> Router<AppState> {
    async fn ai_unavailable() -> impl IntoResponse {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "service_unavailable",
                    "message": "AI is unavailable on this host."
                },
                "inference_available": false,
                "models": []
            })),
        )
    }

    Router::new()
        .route("/models", any(ai_unavailable))
        .route("/chat", any(ai_unavailable))
        .fallback(any(ai_unavailable))
}
