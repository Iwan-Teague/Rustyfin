use axum::http::HeaderMap;
use rustfin_core::error::ApiError;
use rustyvault::types::{
    EncryptedRustyVaultItem, EncryptedRustyVaultItemSummary, RustyVaultClientKind,
    RustyVaultConfigResponse, RustyVaultDeviceSessionResponse, RustyVaultDeviceSessionTokens,
    RustyVaultPreferences, RustyVaultProtectedActionKind, RustyVaultWrappedKeyMetadata,
};

use crate::error::AppError;
use crate::rustyvault_host::auth::{
    issue_rustyvault_session_access_token, resolve_optional_rustyvault_session_row,
};
use crate::state::AppState;

pub const RUSTYVAULT_SCHEMA_VERSION: i32 = rustyvault::service::RUSTYVAULT_SCHEMA_VERSION;
pub const RUSTYVAULT_KDF_ALGORITHM: &str = rustyvault::service::RUSTYVAULT_KDF_ALGORITHM;
pub const RUSTYVAULT_HKDF_ALGORITHM: &str = rustyvault::service::RUSTYVAULT_HKDF_ALGORITHM;
pub const RUSTYVAULT_WRAP_ALGORITHM: &str = rustyvault::service::RUSTYVAULT_WRAP_ALGORITHM;
pub const RUSTYVAULT_ENCRYPTION_ALGORITHM: &str =
    rustyvault::service::RUSTYVAULT_ENCRYPTION_ALGORITHM;
pub const RUSTYVAULT_ACCESS_TOKEN_TTL_SECONDS: i64 =
    rustyvault::service::RUSTYVAULT_ACCESS_TOKEN_TTL_SECONDS;
pub const RUSTYVAULT_REFRESH_TOKEN_TTL_SECONDS: i64 =
    rustyvault::service::RUSTYVAULT_REFRESH_TOKEN_TTL_SECONDS;
pub const RUSTYVAULT_PAIRING_TTL_SECONDS: i64 = rustyvault::service::RUSTYVAULT_PAIRING_TTL_SECONDS;
pub const RUSTYVAULT_PROTECTED_ACTION_TTL_SECONDS: i64 =
    rustyvault::service::RUSTYVAULT_PROTECTED_ACTION_TTL_SECONDS;
pub const RUSTYVAULT_LIST_MAX_LIMIT: i64 = rustyvault::service::RUSTYVAULT_LIST_MAX_LIMIT;
pub const RUSTYVAULT_AUDIT_LIMIT: i64 = rustyvault::service::RUSTYVAULT_AUDIT_LIMIT;
pub const RUSTYVAULT_MAX_MATCH_HASHES: usize = rustyvault::service::RUSTYVAULT_MAX_MATCH_HASHES;
pub const RUSTYVAULT_MAX_IMPORT_ITEMS: usize = rustyvault::service::RUSTYVAULT_MAX_IMPORT_ITEMS;
pub const RUSTYVAULT_MAX_URI_INDEXES_PER_ITEM: usize =
    rustyvault::service::RUSTYVAULT_MAX_URI_INDEXES_PER_ITEM;
pub const RUSTYVAULT_MAX_BLOB_BYTES: usize = rustyvault::service::RUSTYVAULT_MAX_BLOB_BYTES;
pub const RUSTYVAULT_MAX_DEVICE_NAME_CHARS: usize =
    rustyvault::service::RUSTYVAULT_MAX_DEVICE_NAME_CHARS;
pub const RUSTYVAULT_MAX_DEVICE_PLATFORM_CHARS: usize =
    rustyvault::service::RUSTYVAULT_MAX_DEVICE_PLATFORM_CHARS;

#[derive(Debug, Clone)]
pub struct OptionalRustyVaultSessionContext {
    pub user_id: String,
    pub session_id: String,
}

pub fn now_ts() -> i64 {
    rustyvault::service::current_timestamp()
}

pub fn hash_secret(value: &str) -> String {
    rustyvault::service::hash_secret(value)
}

pub fn generate_secret_token(len: usize) -> String {
    rustyvault::service::generate_secret_token(len)
}

pub fn generate_pairing_code() -> String {
    rustyvault::service::generate_pairing_code()
}

pub fn generate_fingerprint_phrase() -> String {
    rustyvault::service::generate_fingerprint_phrase()
}

pub fn parse_client_kind(raw: &str) -> Result<RustyVaultClientKind, AppError> {
    match raw {
        "rustyvault_web" => Ok(RustyVaultClientKind::WebClient),
        "browser_extension" => Ok(RustyVaultClientKind::BrowserExtension),
        _ => Err(ApiError::BadRequest("invalid rustyvault client kind".into()).into()),
    }
}

pub fn parse_protected_action_kind(raw: &str) -> Result<RustyVaultProtectedActionKind, AppError> {
    match raw {
        "rekey" => Ok(RustyVaultProtectedActionKind::Rekey),
        "export" => Ok(RustyVaultProtectedActionKind::Export),
        "import_overwrite" => Ok(RustyVaultProtectedActionKind::ImportOverwrite),
        "destroy_rustyvault" => Ok(RustyVaultProtectedActionKind::DestroyRustyVault),
        "approve_device" => Ok(RustyVaultProtectedActionKind::ApproveDevice),
        "revoke_other_sessions" => Ok(RustyVaultProtectedActionKind::RevokeOtherSessions),
        _ => Err(ApiError::BadRequest("invalid rustyvault protected action kind".into()).into()),
    }
}

pub fn decode_hex_field(field_name: &str, value: &str) -> Result<Vec<u8>, AppError> {
    rustyvault::service::decode_hex_field(field_name, value).map_err(AppError::from)
}

pub fn sanitize_limit(limit: Option<i64>, default: i64) -> i64 {
    rustyvault::service::sanitize_limit(limit, default)
}

pub fn sanitize_offset(offset: Option<i64>) -> i64 {
    rustyvault::service::sanitize_offset(offset)
}

pub fn map_wrapped_key_metadata(
    row: rustfin_db::repo::rustyvault::RustyVaultWrappedKeyRow,
) -> RustyVaultWrappedKeyMetadata {
    RustyVaultWrappedKeyMetadata {
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
    row: rustfin_db::repo::rustyvault::RustyVaultItemSummaryRow,
) -> EncryptedRustyVaultItemSummary {
    EncryptedRustyVaultItemSummary {
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

pub fn map_item(row: rustfin_db::repo::rustyvault::RustyVaultItemRow) -> EncryptedRustyVaultItem {
    EncryptedRustyVaultItem {
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

pub fn build_rustyvault_config_response(
    account: Option<rustfin_db::repo::rustyvault::RustyVaultAccountRow>,
    wrapped_key: Option<rustfin_db::repo::rustyvault::RustyVaultWrappedKeyRow>,
    item_count: i64,
) -> RustyVaultConfigResponse {
    let display_name = account
        .and_then(|row| row.display_name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Personal Vault".to_string());
    RustyVaultConfigResponse {
        enabled: wrapped_key.is_some(),
        schema_version: RUSTYVAULT_SCHEMA_VERSION,
        supported_kdf_algorithms: vec![RUSTYVAULT_KDF_ALGORITHM.to_string()],
        supported_encryption_algorithms: vec![RUSTYVAULT_ENCRYPTION_ALGORITHM.to_string()],
        display_name,
        active_wrapped_key: wrapped_key.map(map_wrapped_key_metadata),
        item_count,
    }
}

pub fn map_device_session_response(
    row: rustfin_db::repo::rustyvault::RustyVaultDeviceSessionRow,
    current_session_id: Option<&str>,
) -> Result<RustyVaultDeviceSessionResponse, AppError> {
    Ok(RustyVaultDeviceSessionResponse {
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

pub async fn load_rustyvault_preferences(
    state: &AppState,
    user_id: &str,
) -> Result<RustyVaultPreferences, AppError> {
    let prefs = rustfin_db::repo::rustyvault::get_rustyvault_preferences(&state.db, user_id)
        .await
        .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(prefs
        .map(map_preferences_row)
        .unwrap_or_default()
        .normalized())
}

pub async fn save_rustyvault_preferences(
    state: &AppState,
    user_id: &str,
    prefs: RustyVaultPreferences,
) -> Result<RustyVaultPreferences, AppError> {
    let prefs = prefs.normalized();
    let saved = rustfin_db::repo::rustyvault::upsert_rustyvault_preferences(
        &state.db,
        &rustfin_db::repo::rustyvault::UpsertRustyVaultPreferenceInput {
            user_id: user_id.to_string(),
            auto_lock_minutes: i32::try_from(prefs.auto_lock_minutes)
                .map_err(|_| ApiError::BadRequest("auto_lock_minutes is too large".into()))?,
            clipboard_clear_seconds: i32::try_from(prefs.clipboard_clear_seconds)
                .map_err(|_| ApiError::BadRequest("clipboard_clear_seconds is too large".into()))?,
            inline_save_prompt_enabled: prefs.inline_save_prompt_enabled,
            inline_autofill_enabled: prefs.inline_autofill_enabled,
            default_match_mode: prefs.default_match_mode,
            warn_on_http: prefs.warn_on_http,
            warn_on_untrusted_iframe: prefs.warn_on_untrusted_iframe,
            excluded_domains: prefs.excluded_domains,
            allow_manual_http_fill: prefs.allow_manual_http_fill,
            password_generator_default_preset: prefs.password_generator_default_preset,
            password_generator_default_length: i32::try_from(
                prefs.password_generator_default_length,
            )
            .map_err(|_| {
                ApiError::BadRequest("password_generator_default_length is too large".into())
            })?,
            password_generator_include_uppercase: prefs.password_generator_include_uppercase,
            password_generator_include_lowercase: prefs.password_generator_include_lowercase,
            password_generator_include_numbers: prefs.password_generator_include_numbers,
            password_generator_include_symbols: prefs.password_generator_include_symbols,
            password_generator_exclude_ambiguous: prefs.password_generator_exclude_ambiguous,
            updated_ts: now_ts(),
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;
    Ok(map_preferences_row(saved).normalized())
}

pub fn normalize_rustyvault_display_name(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("vault display name is required".into()).into());
    }
    if trimmed.chars().count() > 80 {
        return Err(ApiError::BadRequest("vault display name is too long".into()).into());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_rustyvault_display_name;

    #[test]
    fn normalize_rustyvault_display_name_trims_valid_names() {
        let normalized = normalize_rustyvault_display_name("  Personal Vault  ").unwrap();
        assert_eq!(normalized, "Personal Vault");
    }

    #[test]
    fn normalize_rustyvault_display_name_rejects_blank_names() {
        assert!(normalize_rustyvault_display_name("   ").is_err());
    }
}

fn map_preferences_row(
    row: rustfin_db::repo::rustyvault::RustyVaultPreferenceRow,
) -> RustyVaultPreferences {
    let defaults = RustyVaultPreferences::default();
    RustyVaultPreferences {
        auto_lock_minutes: u32::try_from(row.auto_lock_minutes)
            .unwrap_or(defaults.auto_lock_minutes),
        clipboard_clear_seconds: u32::try_from(row.clipboard_clear_seconds)
            .unwrap_or(defaults.clipboard_clear_seconds),
        inline_save_prompt_enabled: row.inline_save_prompt_enabled,
        inline_autofill_enabled: row.inline_autofill_enabled,
        default_match_mode: row.default_match_mode,
        warn_on_http: row.warn_on_http,
        warn_on_untrusted_iframe: row.warn_on_untrusted_iframe,
        excluded_domains: row.excluded_domains,
        allow_manual_http_fill: row.allow_manual_http_fill,
        password_generator_default_preset: row.password_generator_default_preset,
        password_generator_default_length: u32::try_from(row.password_generator_default_length)
            .unwrap_or(defaults.password_generator_default_length),
        password_generator_include_uppercase: row.password_generator_include_uppercase,
        password_generator_include_lowercase: row.password_generator_include_lowercase,
        password_generator_include_numbers: row.password_generator_include_numbers,
        password_generator_include_symbols: row.password_generator_include_symbols,
        password_generator_exclude_ambiguous: row.password_generator_exclude_ambiguous,
    }
}

pub fn issue_device_session_tokens(
    session: &rustfin_db::repo::rustyvault::RustyVaultDeviceSessionRow,
    secret: &str,
) -> Result<RustyVaultDeviceSessionTokens, AppError> {
    let access_expires_ts = now_ts() + RUSTYVAULT_ACCESS_TOKEN_TTL_SECONDS;
    let access_token = issue_rustyvault_session_access_token(
        &session.user_id,
        &session.id,
        &session.client_kind,
        RUSTYVAULT_ACCESS_TOKEN_TTL_SECONDS,
        secret,
    )?;
    let refresh_token = generate_secret_token(64);
    Ok(RustyVaultDeviceSessionTokens {
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
    rustfin_db::repo::rustyvault::create_audit_event(
        &state.db,
        &rustfin_db::repo::rustyvault::CreateRustyVaultAuditEventInput {
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

pub async fn resolve_optional_rustyvault_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<OptionalRustyVaultSessionContext>, AppError> {
    Ok(resolve_optional_rustyvault_session_row(state, headers)
        .await?
        .map(|session| OptionalRustyVaultSessionContext {
            user_id: session.user_id,
            session_id: session.id,
        }))
}

pub fn ensure_rustyvault_session_user_matches(
    auth_user_id: &str,
    rustyvault_user_id: &str,
) -> Result<(), AppError> {
    if auth_user_id != rustyvault_user_id {
        return Err(
            ApiError::Forbidden("rustyvault session does not belong to caller".into()).into(),
        );
    }
    Ok(())
}

pub async fn consume_protected_action_token(
    state: &AppState,
    user_id: &str,
    device_session_id: Option<&str>,
    expected_kind: RustyVaultProtectedActionKind,
    expected_target_item_id: Option<&str>,
    token: &str,
) -> Result<(), AppError> {
    let token_hash = hash_secret(token);
    let stored =
        rustfin_db::repo::rustyvault::get_protected_action_token_by_hash(&state.db, &token_hash)
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

    let consumed = rustfin_db::repo::rustyvault::consume_protected_action_token(
        &state.db,
        &stored.id,
        now_ts(),
    )
    .await
    .map_err(|e| AppError::from(ApiError::Internal(format!("db error: {e}"))))?;
    if !consumed {
        return Err(ApiError::Unauthorized("protected action token already used".into()).into());
    }
    Ok(())
}

pub fn sanitize_device_name(raw: &str) -> Result<String, AppError> {
    rustyvault::service::sanitize_device_name(raw).map_err(AppError::from)
}

pub fn sanitize_device_platform(raw: Option<String>) -> Result<Option<String>, AppError> {
    rustyvault::service::sanitize_device_platform(raw).map_err(AppError::from)
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
