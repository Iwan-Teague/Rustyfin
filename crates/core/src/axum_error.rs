use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::error::{ApiError, ApiErrorWithCode, ErrorEnvelope};

#[derive(Debug)]
pub struct AppError(pub ApiError);

impl From<ApiError> for AppError {
    fn from(value: ApiError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
}

#[derive(Debug)]
pub struct AppErrorWithCode(pub ApiErrorWithCode);

impl From<ApiErrorWithCode> for AppErrorWithCode {
    fn from(value: ApiErrorWithCode) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppErrorWithCode {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let envelope = ErrorEnvelope::from(&self.0);
        (status, Json(envelope)).into_response()
    }
}
