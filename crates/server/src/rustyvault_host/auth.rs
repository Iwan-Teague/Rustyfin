use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::request::Parts;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

pub const RUSTYVAULT_ACCESS_HEADER: &str = "x-rustyvault-access";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RustyVaultSessionClaims {
    pub sub: String,
    pub sid: String,
    pub kind: String,
    pub aud: String,
    pub exp: usize,
}

pub fn issue_rustyvault_session_access_token(
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

    let claims = RustyVaultSessionClaims {
        sub: user_id.to_string(),
        sid: session_id.to_string(),
        kind: client_kind.to_string(),
        aud: "rustyvault_session".to_string(),
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        ApiError::Internal(format!("rustyvault session token encoding failed: {e}")).into()
    })
}

pub fn validate_rustyvault_session_access_token(
    token: &str,
    secret: &str,
) -> Result<RustyVaultSessionClaims, ApiError> {
    let mut validation = Validation::default();
    validation.set_audience(&["rustyvault_session"]);

    let data = decode::<RustyVaultSessionClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| ApiError::Unauthorized(format!("invalid rustyvault session token: {e}")))?;

    Ok(data.claims)
}

pub fn rustyvault_access_token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(RUSTYVAULT_ACCESS_HEADER)
        .and_then(|value| value.to_str().ok())
}

async fn resolve_rustyvault_session_row(
    state: &AppState,
    token: &str,
) -> Result<rustfin_db::repo::rustyvault::RustyVaultDeviceSessionRow, AppError> {
    let claims = validate_rustyvault_session_access_token(token, &state.jwt_secret)?;
    let session =
        rustfin_db::repo::rustyvault::get_device_session(&state.db, &claims.sub, &claims.sid)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let session = session
        .ok_or_else(|| ApiError::Unauthorized("rustyvault device session not found".into()))?;
    if session.revoked_ts.is_some() {
        return Err(ApiError::Unauthorized("rustyvault device session revoked".into()).into());
    }

    let now_ts = chrono::Utc::now().timestamp();
    if session.expires_ts <= now_ts {
        return Err(ApiError::Unauthorized("rustyvault device session expired".into()).into());
    }
    if session.client_kind != claims.kind {
        return Err(
            ApiError::Unauthorized("rustyvault device session kind mismatch".into()).into(),
        );
    }

    Ok(session)
}

pub async fn resolve_optional_rustyvault_session_row(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<rustfin_db::repo::rustyvault::RustyVaultDeviceSessionRow>, AppError> {
    let Some(token) = rustyvault_access_token_from_headers(headers) else {
        return Ok(None);
    };

    resolve_rustyvault_session_row(state, token).await.map(Some)
}

#[derive(Debug, Clone)]
pub struct RustyVaultSessionUser {
    pub user_id: String,
    pub session_id: String,
    pub client_kind: String,
    pub device_name: String,
}

impl FromRequestParts<AppState> for RustyVaultSessionUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = rustyvault_access_token_from_headers(&parts.headers)
            .ok_or_else(|| ApiError::Unauthorized("missing x-rustyvault-access header".into()))?;
        let session = resolve_rustyvault_session_row(state, token).await?;

        Ok(RustyVaultSessionUser {
            user_id: session.user_id.clone(),
            session_id: session.id,
            client_kind: session.client_kind,
            device_name: session.device_name,
        })
    }
}
