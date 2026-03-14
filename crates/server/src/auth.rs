use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
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
    pub room_id: Option<String>,
    pub track_id: Option<String>,
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
        room_id: None,
        track_id: None,
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("stream token encoding failed: {e}")).into())
}

/// Issue a short-lived token scoped to an online-audio room track stream.
pub fn issue_room_track_stream_token(
    room_id: &str,
    track_id: &str,
    ttl_seconds: i64,
    secret: &str,
) -> Result<String, AppError> {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::seconds(ttl_seconds))
        .ok_or_else(|| ApiError::Internal("time overflow".into()))?
        .timestamp() as usize;

    let claims = StreamClaims {
        sub: room_id.to_string(),
        role: "room".to_string(),
        aud: "stream".to_string(),
        file_id: None,
        session_id: None,
        room_id: Some(room_id.to_string()),
        track_id: Some(track_id.to_string()),
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

const AUTH_COOKIE_NAME: &str = "rustfin_token";

fn extract_token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))?;
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let cookie_header = headers.get(header::COOKIE).and_then(|v| v.to_str().ok())?;
    for cookie in cookie_header.split(';') {
        let mut parts = cookie.trim().splitn(2, '=');
        let name = parts.next().unwrap_or_default().trim();
        let value = parts.next().unwrap_or_default().trim();
        if name != AUTH_COOKIE_NAME || value.is_empty() {
            continue;
        }

        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        if !unquoted.is_empty() {
            return Some(unquoted.to_string());
        }
    }

    None
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token_from_headers(&parts.headers)
            .ok_or_else(|| ApiError::Unauthorized("missing authorization token".into()))?;

        let claims = validate_token(&token, &state.jwt_secret)?;

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

#[cfg(test)]
mod tests {
    use super::extract_token_from_headers;
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn extracts_bearer_token_from_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer header-token"),
        );
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("rustfin_token=cookie-token"),
        );

        let token = extract_token_from_headers(&headers);

        assert_eq!(token.as_deref(), Some("header-token"));
    }

    #[test]
    fn extracts_token_from_cookie_when_authorization_is_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("foo=bar; rustfin_token=cookie-token ; baz=qux"),
        );

        let token = extract_token_from_headers(&headers);

        assert_eq!(token.as_deref(), Some("cookie-token"));
    }

    #[test]
    fn returns_none_when_no_supported_auth_token_is_present() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("foo=bar; baz=qux"));

        let token = extract_token_from_headers(&headers);

        assert!(token.is_none());
    }
}
