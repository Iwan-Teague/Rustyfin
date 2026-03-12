use axum::http::HeaderMap;
use rand::distributions::{Alphanumeric, DistString};
use rustfin_core::error::ApiError;
use rustfin_core::vault::{
    EncryptedVaultItem, EncryptedVaultItemSummary, VaultClientKind, VaultConfigResponse,
    VaultDeviceSessionResponse, VaultDeviceSessionTokens, VaultProtectedActionKind,
    VaultWrappedKeyMetadata,
};
use sha2::{Digest, Sha256};

use crate::auth::{
    issue_vault_session_access_token, validate_token, validate_vault_session_access_token,
};
use crate::error::AppError;
use crate::state::AppState;

pub const VAULT_SCHEMA_VERSION: i32 = 1;
pub const VAULT_KDF_ALGORITHM: &str = "argon2id";
pub const VAULT_HKDF_ALGORITHM: &str = "hkdf-sha-256";
pub const VAULT_WRAP_ALGORITHM: &str = "aes-256-gcm";
pub const VAULT_ENCRYPTION_ALGORITHM: &str = "aes-256-gcm";
pub const VAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;
pub const VAULT_REFRESH_TOKEN_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const VAULT_PAIRING_TTL_SECONDS: i64 = 10 * 60;
pub const VAULT_PROTECTED_ACTION_TTL_SECONDS: i64 = 60;
pub const VAULT_LIST_MAX_LIMIT: i64 = 100;
pub const VAULT_AUDIT_LIMIT: i64 = 100;
pub const VAULT_MAX_MATCH_HASHES: usize = 16;
pub const VAULT_MAX_IMPORT_ITEMS: usize = 2_000;
pub const VAULT_MAX_URI_INDEXES_PER_ITEM: usize = 32;
pub const VAULT_MAX_BLOB_BYTES: usize = 128 * 1024;
pub const VAULT_MAX_DEVICE_NAME_CHARS: usize = 120;
pub const VAULT_MAX_DEVICE_PLATFORM_CHARS: usize = 80;

#[derive(Debug, Clone)]
pub struct OptionalVaultSessionContext {
    pub session_id: String,
}

pub fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn generate_secret_token(len: usize) -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), len)
}

pub fn generate_pairing_code() -> String {
    let raw = generate_secret_token(24).to_ascii_uppercase();
    format!(
        "RFVLT-{}-{}-{}-{}",
        &raw[0..6],
        &raw[6..12],
        &raw[12..18],
        &raw[18..24]
    )
}

pub fn generate_fingerprint_phrase() -> String {
    const ADJECTIVES: &[&str] = &[
        "amber", "brisk", "cinder", "delta", "ember", "frost", "gloss", "harbor",
    ];
    const NOUNS: &[&str] = &[
        "anchor", "beacon", "canyon", "drift", "echo", "forge", "grove", "harvest",
    ];
    let mut rng = rand::thread_rng();
    let adjective = ADJECTIVES[rand::Rng::gen_range(&mut rng, 0..ADJECTIVES.len())];
    let noun = NOUNS[rand::Rng::gen_range(&mut rng, 0..NOUNS.len())];
    format!("{adjective}-{noun}")
}

pub fn parse_client_kind(raw: &str) -> Result<VaultClientKind, AppError> {
    match raw {
        "web_vault" => Ok(VaultClientKind::WebVault),
        "browser_extension" => Ok(VaultClientKind::BrowserExtension),
        _ => Err(ApiError::BadRequest("invalid vault client kind".into()).into()),
    }
}

pub fn parse_protected_action_kind(raw: &str) -> Result<VaultProtectedActionKind, AppError> {
    match raw {
        "rekey" => Ok(VaultProtectedActionKind::Rekey),
        "export" => Ok(VaultProtectedActionKind::Export),
        "import_overwrite" => Ok(VaultProtectedActionKind::ImportOverwrite),
        "destroy_vault" => Ok(VaultProtectedActionKind::DestroyVault),
        "approve_device" => Ok(VaultProtectedActionKind::ApproveDevice),
        "revoke_other_sessions" => Ok(VaultProtectedActionKind::RevokeOtherSessions),
        _ => Err(ApiError::BadRequest("invalid vault protected action kind".into()).into()),
    }
}

pub fn decode_hex_field(field_name: &str, value: &str) -> Result<Vec<u8>, AppError> {
    let decoded = hex::decode(value.trim()).map_err(|_| {
        AppError::from(ApiError::BadRequest(format!(
            "{field_name} must be valid hex"
        )))
    })?;
    if decoded.len() > VAULT_MAX_BLOB_BYTES {
        return Err(ApiError::BadRequest(format!("{field_name} exceeds size limit")).into());
    }
    Ok(decoded)
}

pub fn sanitize_limit(limit: Option<i64>, default: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, VAULT_LIST_MAX_LIMIT)
}

pub fn sanitize_offset(offset: Option<i64>) -> i64 {
    offset.unwrap_or(0).max(0)
}

pub fn map_wrapped_key_metadata(
    row: rustfin_db::repo::vault::VaultWrappedKeyRow,
) -> VaultWrappedKeyMetadata {
    VaultWrappedKeyMetadata {
        key_version: row.key_version,
        kdf_algorithm: row.kdf_algorithm,
        kdf_memory_kib: row.kdf_memory_kib,
        kdf_iterations: row.kdf_iterations,
        kdf_parallelism: row.kdf_parallelism,
        kdf_salt_hex: hex::encode(row.kdf_salt),
        hkdf_algorithm: row.hkdf_algorithm,
        wrap_algorithm: row.wrap_algorithm,
        wrap_nonce_hex: hex::encode(row.wrap_nonce),
        wrapped_vault_key_hex: hex::encode(row.wrapped_vault_key),
        created_ts: row.created_ts,
    }
}

pub fn map_item_summary(
    row: rustfin_db::repo::vault::VaultItemSummaryRow,
) -> EncryptedVaultItemSummary {
    EncryptedVaultItemSummary {
        id: row.id,
        item_type: row.item_type,
        key_version: row.key_version,
        summary_version: row.summary_version,
        summary_nonce_hex: hex::encode(row.summary_nonce),
        summary_ciphertext_hex: hex::encode(row.summary_ciphertext),
        favorite: row.favorite,
        revision: row.revision,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
        deleted_ts: row.deleted_ts,
    }
}

pub fn map_item(row: rustfin_db::repo::vault::VaultItemRow) -> EncryptedVaultItem {
    EncryptedVaultItem {
        id: row.id,
        item_type: row.item_type,
        key_version: row.key_version,
        summary_version: row.summary_version,
        summary_nonce_hex: hex::encode(row.summary_nonce),
        summary_ciphertext_hex: hex::encode(row.summary_ciphertext),
        payload_version: row.payload_version,
        payload_nonce_hex: hex::encode(row.payload_nonce),
        payload_ciphertext_hex: hex::encode(row.payload_ciphertext),
        favorite: row.favorite,
        revision: row.revision,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
        deleted_ts: row.deleted_ts,
    }
}

pub fn build_vault_config_response(
    wrapped_key: Option<rustfin_db::repo::vault::VaultWrappedKeyRow>,
    item_count: i64,
) -> VaultConfigResponse {
    VaultConfigResponse {
        enabled: wrapped_key.is_some(),
        schema_version: VAULT_SCHEMA_VERSION,
        supported_kdf_algorithms: vec![VAULT_KDF_ALGORITHM.to_string()],
        supported_encryption_algorithms: vec![VAULT_ENCRYPTION_ALGORITHM.to_string()],
        active_wrapped_key: wrapped_key.map(map_wrapped_key_metadata),
        item_count,
    }
}

pub fn map_device_session_response(
    row: rustfin_db::repo::vault::VaultDeviceSessionRow,
    current_session_id: Option<&str>,
) -> Result<VaultDeviceSessionResponse, AppError> {
    Ok(VaultDeviceSessionResponse {
        id: row.id.clone(),
        client_kind: parse_client_kind(&row.client_kind)?,
        device_name: row.device_name,
        device_platform: row.device_platform,
        created_ts: row.created_ts,
        last_used_ts: row.last_used_ts,
        expires_ts: row.expires_ts,
        revoked_ts: row.revoked_ts,
        current: current_session_id == Some(row.id.as_str()),
    })
}

pub fn issue_device_session_tokens(
    session: &rustfin_db::repo::vault::VaultDeviceSessionRow,
    secret: &str,
) -> Result<VaultDeviceSessionTokens, AppError> {
    let access_expires_ts = now_ts() + VAULT_ACCESS_TOKEN_TTL_SECONDS;
    let access_token = issue_vault_session_access_token(
        &session.user_id,
        &session.id,
        &session.client_kind,
        VAULT_ACCESS_TOKEN_TTL_SECONDS,
        secret,
    )?;
    let refresh_token = generate_secret_token(64);
    Ok(VaultDeviceSessionTokens {
        session_id: session.id.clone(),
        access_token,
        refresh_token,
        access_expires_ts,
        refresh_expires_ts: session.expires_ts,
    })
}

pub async fn create_audit_event(
    state: &AppState,
    user_id: &str,
    device_session_id: Option<&str>,
    event_kind: &str,
    target_item_id: Option<&str>,
    event_json: serde_json::Value,
) -> Result<(), AppError> {
    rustfin_db::repo::vault::create_audit_event(
        &state.db,
        &rustfin_db::repo::vault::CreateVaultAuditEventInput {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            device_session_id: device_session_id.map(str::to_string),
            event_kind: event_kind.to_string(),
            target_item_id: target_item_id.map(str::to_string),
            event_json,
            created_ts: now_ts(),
        },
    )
    .await
    .map_err(|e| AppError::from(ApiError::Internal(format!("db error: {e}"))))
}

pub async fn resolve_optional_vault_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<OptionalVaultSessionContext>, AppError> {
    let Some(token) = headers
        .get("x-rustfin-vault-access")
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(None);
    };

    let claims = validate_vault_session_access_token(token, &state.jwt_secret)?;
    let session = rustfin_db::repo::vault::get_device_session(&state.db, &claims.sub, &claims.sid)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let session =
        session.ok_or_else(|| ApiError::Unauthorized("vault device session not found".into()))?;
    if session.revoked_ts.is_some() || session.expires_ts <= now_ts() {
        return Err(ApiError::Unauthorized("vault device session inactive".into()).into());
    }
    Ok(Some(OptionalVaultSessionContext {
        session_id: session.id,
    }))
}

pub async fn resolve_vault_owner_user_id(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, AppError> {
    if let Some(token) = headers
        .get("x-rustfin-vault-access")
        .and_then(|value| value.to_str().ok())
    {
        let claims = validate_vault_session_access_token(token, &state.jwt_secret)?;
        let session =
            rustfin_db::repo::vault::get_device_session(&state.db, &claims.sub, &claims.sid)
                .await
                .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
        let session = session
            .ok_or_else(|| ApiError::Unauthorized("vault device session not found".into()))?;
        if session.revoked_ts.is_none() && session.expires_ts > now_ts() {
            return Ok(session.user_id);
        }
    }

    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .ok_or_else(|| ApiError::Unauthorized("missing authorization or vault session".into()))?;
    let claims = validate_token(token, &state.jwt_secret)?;
    Ok(claims.sub)
}

pub async fn consume_protected_action_token(
    state: &AppState,
    user_id: &str,
    device_session_id: Option<&str>,
    expected_kind: VaultProtectedActionKind,
    expected_target_item_id: Option<&str>,
    token: &str,
) -> Result<(), AppError> {
    let token_hash = hash_secret(token);
    let stored =
        rustfin_db::repo::vault::get_protected_action_token_by_hash(&state.db, &token_hash)
            .await
            .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    let stored =
        stored.ok_or_else(|| ApiError::Unauthorized("invalid protected action token".into()))?;
    if stored.user_id != user_id {
        return Err(
            ApiError::Forbidden("protected action token does not belong to caller".into()).into(),
        );
    }
    if stored.action_kind != expected_kind.as_str() {
        return Err(ApiError::BadRequest("protected action token kind mismatch".into()).into());
    }
    if stored.target_item_id.as_deref() != expected_target_item_id {
        return Err(ApiError::BadRequest("protected action token target mismatch".into()).into());
    }
    if stored.expires_ts <= now_ts() {
        return Err(ApiError::Unauthorized("protected action token expired".into()).into());
    }
    if stored.consumed_ts.is_some() {
        return Err(ApiError::Unauthorized("protected action token already used".into()).into());
    }
    if stored.device_session_id.as_deref() != device_session_id {
        return Err(ApiError::Forbidden("protected action token session mismatch".into()).into());
    }

    let consumed =
        rustfin_db::repo::vault::consume_protected_action_token(&state.db, &stored.id, now_ts())
            .await
            .map_err(|e| AppError::from(ApiError::Internal(format!("db error: {e}"))))?;
    if !consumed {
        return Err(ApiError::Unauthorized("protected action token already used".into()).into());
    }
    Ok(())
}

pub fn sanitize_device_name(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("device_name is required".into()).into());
    }
    if trimmed.chars().count() > VAULT_MAX_DEVICE_NAME_CHARS {
        return Err(ApiError::BadRequest("device_name is too long".into()).into());
    }
    Ok(trimmed.to_string())
}

pub fn sanitize_device_platform(raw: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > VAULT_MAX_DEVICE_PLATFORM_CHARS {
        return Err(ApiError::BadRequest("device_platform is too long".into()).into());
    }
    Ok(Some(trimmed.to_string()))
}

pub fn forwarded_for_summary(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn user_agent_summary(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let mut owned = value.to_string();
            if owned.len() > 200 {
                owned.truncate(200);
            }
            owned
        })
}
