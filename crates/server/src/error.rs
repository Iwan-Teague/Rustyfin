use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rustfin_core::error::{ApiError, ApiErrorWithCode, ErrorEnvelope};

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

/// Wrapper for setup-specific errors with custom codes.
pub struct AppErrorWithCode(pub ApiErrorWithCode);

impl IntoResponse for AppErrorWithCode {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
}

impl From<ApiErrorWithCode> for AppErrorWithCode {
    fn from(e: ApiErrorWithCode) -> Self {
        Self(e)
    }
}
