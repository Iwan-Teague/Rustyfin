use serde::Serialize;
use thiserror::Error;

/// Unified API error type.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("validation failed")]
    UnprocessableEntity {
        message: String,
        details: serde_json::Value,
    },

    #[error("too many requests")]
    TooManyRequests {
        retry_after_seconds: u64,
    },

    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::UnprocessableEntity { .. } => "validation_failed",
            Self::TooManyRequests { .. } => "too_many_requests",
            Self::Internal(_) => "internal_error",
        }
    }

    /// Return a custom error code (for setup-specific errors like setup_claimed, etc.)
    pub fn with_code(code: &str, message: String, details: serde_json::Value) -> ApiErrorWithCode {
        ApiErrorWithCode {
            code: code.to_string(),
            message,
            details,
            status: 409,
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::UnprocessableEntity { .. } => 422,
            Self::TooManyRequests { .. } => 429,
            Self::Internal(_) => 500,
        }
    }

    pub fn details(&self) -> serde_json::Value {
        match self {
            Self::UnprocessableEntity { details, .. } => details.clone(),
            Self::TooManyRequests { retry_after_seconds } => {
                serde_json::json!({ "retry_after_seconds": retry_after_seconds })
            }
            _ => serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Create a validation error with field-level details.
    pub fn validation(fields: serde_json::Value) -> Self {
        Self::UnprocessableEntity {
            message: "validation failed".to_string(),
            details: serde_json::json!({ "fields": fields }),
        }
    }
}

/// An API error with a custom error code string (for setup-specific codes).
#[derive(Debug)]
pub struct ApiErrorWithCode {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
    pub status: u16,
}

impl ApiErrorWithCode {
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}

impl std::fmt::Display for ApiErrorWithCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiErrorWithCode {}

/// JSON error envelope per spec: `{ "error": { "code": "…", "message": "…", "details": {} } }`
#[derive(Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

impl From<&ApiError> for ErrorEnvelope {
    fn from(e: &ApiError) -> Self {
        Self {
            error: ErrorBody {
                code: e.code().to_string(),
                message: e.to_string(),
                details: e.details(),
            },
        }
    }
}

impl From<&ApiErrorWithCode> for ErrorEnvelope {
    fn from(e: &ApiErrorWithCode) -> Self {
        Self {
            error: ErrorBody {
                code: e.code.clone(),
                message: e.message.clone(),
                details: e.details.clone(),
            },
        }
    }
}
