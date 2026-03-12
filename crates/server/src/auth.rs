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
    pub room_id: Option<String>,
    pub track_id: Option<String>,
    pub exp: usize,
}

/// Short-lived token used for vault device-session access.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VaultSessionClaims {
    pub sub: String, // user ID
    pub sid: String, // vault device session ID
    pub kind: String,
    pub aud: String, // "vault_session"
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

pub fn issue_vault_session_access_token(
    user_id: &str,
    session_id: &str,
    client_kind: &str,
    ttl_seconds: i64,
    secret: &str,
) -> Result<String, AppError> {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::seconds(ttl_seconds))
        .ok_or_else(|| ApiError::Internal("time overflow".into()))?
        .timestamp() as usize;

    let claims = VaultSessionClaims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        kind: client_kind.to_string(),
        aud: "vault_session".to_string(),
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::Internal(format!("vault session token encoding failed: {e}")).into())
}

pub fn validate_vault_session_access_token(
    token: &str,
    secret: &str,
) -> Result<VaultSessionClaims, ApiError> {
    let mut validation = Validation::default();
    validation.set_audience(&["vault_session"]);

    let data = decode::<VaultSessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| ApiError::Unauthorized(format!("invalid vault session token: {e}")))?;

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

#[derive(Debug, Clone)]
pub struct VaultSessionUser {
    pub user_id: String,
    pub session_id: String,
    pub client_kind: String,
    pub device_name: String,
}

impl FromRequestParts<AppState> for VaultSessionUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("x-rustfin-vault-access")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                ApiError::Unauthorized("missing x-rustfin-vault-access header".into())
            })?;

        let claims = validate_vault_session_access_token(token, &state.jwt_secret)?;
        let session =
            rustfin_db::repo::vault::get_device_session(&state.db, &claims.sub, &claims.sid)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

        let session = session
            .ok_or_else(|| ApiError::Unauthorized("vault device session not found".into()))?;
        if session.revoked_ts.is_some() {
            return Err(ApiError::Unauthorized("vault device session revoked".into()).into());
        }

        let now_ts = chrono::Utc::now().timestamp();
        if session.expires_ts <= now_ts {
            return Err(ApiError::Unauthorized("vault device session expired".into()).into());
        }
        if session.client_kind != claims.kind {
            return Err(ApiError::Unauthorized("vault device session kind mismatch".into()).into());
        }

        Ok(VaultSessionUser {
            user_id: claims.sub,
            session_id: claims.sid,
            client_kind: claims.kind,
            device_name: session.device_name,
        })
    }
}
