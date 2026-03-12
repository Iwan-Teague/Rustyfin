use axum::http::HeaderMap;
use rustfin_core::error::ApiError;
use rustfin_core::vault::{
    ConsumeVaultPairingCodeRequest, CreateVaultDeviceSessionRequest,
    CreateVaultDeviceSessionResponse, VaultClientKind, VaultDeviceSessionTokens,
    VaultPairingCodeResponse, VaultSessionRefreshRequest,
};

use crate::error::AppError;
use crate::state::AppState;
use crate::vault::{audit, service};

async fn create_session_with_refresh_token(
    state: &AppState,
    user_id: &str,
    client_kind: VaultClientKind,
    device_name: String,
    device_platform: Option<String>,
    headers: &HeaderMap,
) -> Result<(rustfin_db::repo::vault::VaultDeviceSessionRow, String), AppError> {
    let now_ts = service::now_ts();
    let refresh_token = service::generate_secret_token(64);
    let refresh_token_hash = service::hash_secret(&refresh_token);
    let refresh_token_family_id = uuid::Uuid::new_v4().to_string();
    let session = rustfin_db::repo::vault::create_device_session(
        &state.db,
        &rustfin_db::repo::vault::CreateVaultDeviceSessionInput {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            client_kind: client_kind.as_str().to_string(),
            device_name,
            device_platform,
            device_fingerprint_hash: None,
            refresh_token_family_id: refresh_token_family_id.clone(),
            refresh_token_hash: refresh_token_hash.clone(),
            created_ts: now_ts,
            last_used_ts: now_ts,
            expires_ts: now_ts + service::VAULT_REFRESH_TOKEN_TTL_SECONDS,
            ip_summary: service::forwarded_for_summary(headers),
            user_agent_summary: service::user_agent_summary(headers),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    rustfin_db::repo::vault::create_device_session_refresh_token(
        &state.db,
        &rustfin_db::repo::vault::CreateVaultDeviceSessionRefreshTokenInput {
            id: uuid::Uuid::new_v4().to_string(),
            device_session_id: session.id.clone(),
            user_id: user_id.to_string(),
            refresh_token_family_id,
            token_hash: refresh_token_hash,
            created_ts: now_ts,
            expires_ts: session.expires_ts,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok((session, refresh_token))
}

pub async fn create_device_session(
    state: &AppState,
    user_id: &str,
    headers: &HeaderMap,
    request: CreateVaultDeviceSessionRequest,
) -> Result<CreateVaultDeviceSessionResponse, AppError> {
    let device_name = service::sanitize_device_name(&request.device_name)?;
    let device_platform = service::sanitize_device_platform(request.device_platform)?;
    let now_ts = service::now_ts();
    match request.client_kind {
        VaultClientKind::WebVault => {
            let (session, refresh_token) = create_session_with_refresh_token(
                state,
                user_id,
                request.client_kind,
                device_name,
                device_platform,
                headers,
            )
            .await?;
            let mut tokens = service::issue_device_session_tokens(&session, &state.jwt_secret)?;
            tokens.refresh_token = refresh_token;
            audit::record_event(
                state,
                user_id,
                Some(&session.id),
                "vault_device_session_created",
                None,
                serde_json::json!({ "client_kind": session.client_kind }),
            )
            .await?;
            Ok(CreateVaultDeviceSessionResponse {
                session: Some(tokens),
                pairing: None,
            })
        }
        VaultClientKind::BrowserExtension => {
            let protected_action_token = request.protected_action_token.ok_or_else(|| {
                ApiError::BadRequest(
                    "protected_action_token is required for browser extension pairing".into(),
                )
            })?;
            let session = service::resolve_optional_vault_session(state, headers).await?;
            service::consume_protected_action_token(
                state,
                user_id,
                session.as_ref().map(|ctx| ctx.session_id.as_str()),
                rustfin_core::vault::VaultProtectedActionKind::ApproveDevice,
                None,
                &protected_action_token,
            )
            .await?;

            let pairing_code = service::generate_pairing_code();
            let fingerprint_phrase = service::generate_fingerprint_phrase();
            let pending = rustfin_db::repo::vault::create_pending_device_approval(
                &state.db,
                &rustfin_db::repo::vault::CreatePendingDeviceApprovalInput {
                    id: uuid::Uuid::new_v4().to_string(),
                    user_id: user_id.to_string(),
                    client_kind: request.client_kind.as_str().to_string(),
                    device_name,
                    fingerprint_phrase: fingerprint_phrase.clone(),
                    pairing_code_hash: service::hash_secret(&pairing_code),
                    created_ts: now_ts,
                    expires_ts: now_ts + service::VAULT_PAIRING_TTL_SECONDS,
                },
            )
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
            audit::record_event(
                state,
                user_id,
                session.as_ref().map(|ctx| ctx.session_id.as_str()),
                "vault_device_pairing_created",
                None,
                serde_json::json!({ "device_name": pending.device_name }),
            )
            .await?;
            Ok(CreateVaultDeviceSessionResponse {
                session: None,
                pairing: Some(VaultPairingCodeResponse {
                    pairing_code,
                    fingerprint_phrase,
                    expires_ts: pending.expires_ts,
                }),
            })
        }
    }
}

pub async fn consume_pairing_code(
    state: &AppState,
    headers: &HeaderMap,
    request: ConsumeVaultPairingCodeRequest,
) -> Result<VaultDeviceSessionTokens, AppError> {
    let pending = rustfin_db::repo::vault::get_pending_device_approval_by_code_hash(
        &state.db,
        &service::hash_secret(&request.pairing_code),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let pending = pending.ok_or_else(|| ApiError::Unauthorized("invalid pairing code".into()))?;
    let now_ts = service::now_ts();
    if pending.approved_ts.is_some() || pending.denied_ts.is_some() || pending.expires_ts <= now_ts
    {
        return Err(ApiError::Unauthorized("pairing code expired or already used".into()).into());
    }

    let marked = rustfin_db::repo::vault::mark_pending_device_approval_consumed(
        &state.db,
        &pending.id,
        now_ts,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !marked {
        return Err(ApiError::Unauthorized("pairing code already used".into()).into());
    }

    let (session, refresh_token) = create_session_with_refresh_token(
        state,
        &pending.user_id,
        service::parse_client_kind(&pending.client_kind)?,
        service::sanitize_device_name(&request.device_name)?,
        service::sanitize_device_platform(request.device_platform)?,
        headers,
    )
    .await?;
    let mut tokens = service::issue_device_session_tokens(&session, &state.jwt_secret)?;
    tokens.refresh_token = refresh_token;
    audit::record_event(
        state,
        &pending.user_id,
        Some(&session.id),
        "vault_device_session_created",
        None,
        serde_json::json!({ "client_kind": session.client_kind }),
    )
    .await?;
    Ok(tokens)
}

pub async fn refresh_device_session(
    state: &AppState,
    request: VaultSessionRefreshRequest,
) -> Result<VaultDeviceSessionTokens, AppError> {
    let request_refresh_hash = service::hash_secret(&request.refresh_token);
    let current_token = rustfin_db::repo::vault::get_device_session_refresh_token_by_hash(
        &state.db,
        &request_refresh_hash,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let current_token =
        current_token.ok_or_else(|| ApiError::Unauthorized("invalid refresh token".into()))?;
    let now_ts = service::now_ts();
    let current = rustfin_db::repo::vault::get_device_session(
        &state.db,
        &current_token.user_id,
        &current_token.device_session_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let current =
        current.ok_or_else(|| ApiError::Unauthorized("vault device session not found".into()))?;

    if current_token.revoked_ts.is_some() || current_token.consumed_ts.is_some() {
        let _ = rustfin_db::repo::vault::revoke_device_session_refresh_family(
            &state.db,
            &current_token.refresh_token_family_id,
            now_ts,
        )
        .await;
        let _ = rustfin_db::repo::vault::revoke_device_session(
            &state.db,
            &current.user_id,
            &current.id,
            now_ts,
        )
        .await;
        let _ = audit::record_event(
            state,
            &current.user_id,
            Some(&current.id),
            "vault_refresh_replay_detected",
            None,
            serde_json::json!({ "device_name": current.device_name }),
        )
        .await;
        return Err(ApiError::Unauthorized(
            "refresh token replay detected; session revoked".into(),
        )
        .into());
    }

    if current.revoked_ts.is_some()
        || current.expires_ts <= now_ts
        || current_token.expires_ts <= now_ts
    {
        return Err(ApiError::Unauthorized("vault device session expired".into()).into());
    }

    let next_refresh_token = service::generate_secret_token(64);
    let next_expires_ts = now_ts + service::VAULT_REFRESH_TOKEN_TTL_SECONDS;
    let rotated = rustfin_db::repo::vault::rotate_device_session_refresh_token(
        &state.db,
        rustfin_db::repo::vault::RotateDeviceSessionRefreshTokenParams {
            session_id: &current.id,
            current_token_id: &current_token.id,
            current_token_hash: &request_refresh_hash,
            family_id: &current.refresh_token_family_id,
            user_id: &current.user_id,
            next_refresh_token_hash: &service::hash_secret(&next_refresh_token),
            now_ts,
            expires_ts: next_expires_ts,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    if !rotated {
        return Err(ApiError::Unauthorized("vault device session refresh failed".into()).into());
    }
    let updated =
        rustfin_db::repo::vault::get_device_session(&state.db, &current.user_id, &current.id)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?
            .ok_or_else(|| ApiError::Unauthorized("vault device session not found".into()))?;
    let mut tokens = service::issue_device_session_tokens(&updated, &state.jwt_secret)?;
    tokens.refresh_token = next_refresh_token;
    audit::record_event(
        state,
        &updated.user_id,
        Some(&updated.id),
        "vault_device_session_refreshed",
        None,
        serde_json::json!({ "client_kind": updated.client_kind }),
    )
    .await?;
    Ok(tokens)
}
