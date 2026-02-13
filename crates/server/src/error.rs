use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rustfin_core::error::{ApiError, ErrorEnvelope};

/// Newtype wrapper so we can implement `IntoResponse` in this crate.
pub struct AppError(pub ApiError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
}

impl From<ApiError> for AppError {
    fn from(e: ApiError) -> Self {
        Self(e)
    }
}
