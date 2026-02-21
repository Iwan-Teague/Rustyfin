use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

/// JWT claims payload.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // user ID
    pub username: String,
    pub role: String,
    pub exp: usize,
}

/// Short-lived token used only for streaming URLs.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StreamClaims {
    pub sub: String, // user ID
    pub role: String,
    pub aud: String, // "stream"
    pub file_id: Option<String>,
    pub session_id: Option<String>,
    pub exp: usize,
}

/// Issue a JWT token for a user.
pub fn issue_token(
    user_id: &str,
    username: &str,
    role: &str,
    secret: &str,
) -> Result<String, AppError> {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .ok_or_else(|| ApiError::Internal("time overflow".into()))?
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("token encoding failed: {e}")).into())
}

/// Validate a JWT token and return claims.
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, ApiError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| ApiError::Unauthorized(format!("invalid token: {e}")))?;

    Ok(data.claims)
}

/// Issue a short-lived, scoped token for stream URLs.
pub fn issue_stream_token(
    user_id: &str,
    role: &str,
    file_id: Option<&str>,
    session_id: Option<&str>,
    ttl_seconds: i64,
    secret: &str,
) -> Result<String, AppError> {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::seconds(ttl_seconds))
        .ok_or_else(|| ApiError::Internal("time overflow".into()))?
        .timestamp() as usize;

    let claims = StreamClaims {
        sub: user_id.to_string(),
        role: role.to_string(),
        aud: "stream".to_string(),
        file_id: file_id.map(str::to_string),
        session_id: session_id.map(str::to_string),
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("stream token encoding failed: {e}")).into())
}

/// Validate a short-lived stream token.
pub fn validate_stream_token(token: &str, secret: &str) -> Result<StreamClaims, ApiError> {
    let mut validation = Validation::default();
    validation.set_audience(&["stream"]);

    let data = decode::<StreamClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| ApiError::Unauthorized(format!("invalid stream token: {e}")))?;

    Ok(data.claims)
}

/// Authenticated user extractor — pulls Bearer token from Authorization header.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub username: String,
    pub role: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("missing authorization header".into()))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("invalid authorization scheme".into()))?;

        let claims = validate_token(token, &state.jwt_secret)?;

        Ok(AuthUser {
            user_id: claims.sub,
            username: claims.username,
            role: claims.role,
        })
    }
}

/// Admin-only extractor — rejects non-admin users with 403.
#[derive(Debug, Clone)]
pub struct AdminUser {
    pub user_id: String,
    pub username: String,
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        if user.role != "admin" {
            return Err(ApiError::Forbidden("admin access required".into()).into());
        }
        Ok(AdminUser {
            user_id: user.user_id,
            username: user.username,
        })
    }
}
