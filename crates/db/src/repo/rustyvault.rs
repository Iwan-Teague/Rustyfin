use crate::DbPool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustyVaultAccountRow {
    pub user_id: String,
    pub status: String,
    pub schema_version: i32,
    pub active_key_version: i32,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub last_unlock_required_ts: Option<i64>,
    pub last_rekey_ts: Option<i64>,
}

type RustyVaultAccountTuple = (String, String, i32, i32, i64, i64, Option<i64>, Option<i64>);

fn map_rustyvault_account_row(
    (
        user_id,
        status,
        schema_version,
        active_key_version,
        created_ts,
        updated_ts,
        last_unlock_required_ts,
        last_rekey_ts,
    ): RustyVaultAccountTuple,
) -> RustyVaultAccountRow {
    RustyVaultAccountRow {
        user_id,
        status,
        schema_version,
        active_key_version,
        created_ts,
        updated_ts,
        last_unlock_required_ts,
        last_rekey_ts,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultPreferenceRow {
    pub user_id: String,
    pub auto_lock_minutes: i32,
    pub clipboard_clear_seconds: i32,
    pub inline_save_prompt_enabled: bool,
    pub inline_autofill_enabled: bool,
    pub default_match_mode: String,
    pub warn_on_http: bool,
    pub warn_on_untrusted_iframe: bool,
    pub excluded_domains: Vec<String>,
    pub allow_manual_http_fill: bool,
    pub updated_ts: i64,
}

type RustyVaultPreferenceTuple = (
    String,
    i32,
    i32,
    bool,
    bool,
    String,
    bool,
    bool,
    Vec<String>,
    bool,
    i64,
);

fn map_rustyvault_preference_row(
    (
        user_id,
        auto_lock_minutes,
        clipboard_clear_seconds,
        inline_save_prompt_enabled,
        inline_autofill_enabled,
        default_match_mode,
        warn_on_http,
        warn_on_untrusted_iframe,
        excluded_domains,
        allow_manual_http_fill,
        updated_ts,
    ): RustyVaultPreferenceTuple,
) -> RustyVaultPreferenceRow {
    RustyVaultPreferenceRow {
        user_id,
        auto_lock_minutes,
        clipboard_clear_seconds,
        inline_save_prompt_enabled,
        inline_autofill_enabled,
        default_match_mode,
        warn_on_http,
        warn_on_untrusted_iframe,
        excluded_domains,
        allow_manual_http_fill,
        updated_ts,
    }
}

#[derive(Debug, Clone)]
pub struct UpsertRustyVaultPreferenceInput {
    pub user_id: String,
    pub auto_lock_minutes: i32,
    pub clipboard_clear_seconds: i32,
    pub inline_save_prompt_enabled: bool,
    pub inline_autofill_enabled: bool,
    pub default_match_mode: String,
    pub warn_on_http: bool,
    pub warn_on_untrusted_iframe: bool,
    pub excluded_domains: Vec<String>,
    pub allow_manual_http_fill: bool,
    pub updated_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultWrappedKeyRow {
    pub id: String,
    pub user_id: String,
    pub key_version: i32,
    pub kdf_algorithm: String,
    pub kdf_memory_kib: i32,
    pub kdf_iterations: i32,
    pub kdf_parallelism: i32,
    pub kdf_salt: Vec<u8>,
    pub hkdf_algorithm: String,
    pub wrap_algorithm: String,
    pub wrap_nonce: Vec<u8>,
    pub wrapped_vault_key: Vec<u8>,
    pub created_ts: i64,
    pub superseded_ts: Option<i64>,
}

type RustyVaultWrappedKeyTuple = (
    String,
    String,
    i32,
    String,
    i32,
    i32,
    i32,
    Vec<u8>,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    i64,
    Option<i64>,
);

fn map_rustyvault_wrapped_key_row(
    (
        id,
        user_id,
        key_version,
        kdf_algorithm,
        kdf_memory_kib,
        kdf_iterations,
        kdf_parallelism,
        kdf_salt,
        hkdf_algorithm,
        wrap_algorithm,
        wrap_nonce,
        wrapped_vault_key,
        created_ts,
        superseded_ts,
    ): RustyVaultWrappedKeyTuple,
) -> RustyVaultWrappedKeyRow {
    RustyVaultWrappedKeyRow {
        id,
        user_id,
        key_version,
        kdf_algorithm,
        kdf_memory_kib,
        kdf_iterations,
        kdf_parallelism,
        kdf_salt,
        hkdf_algorithm,
        wrap_algorithm,
        wrap_nonce,
        wrapped_vault_key,
        created_ts,
        superseded_ts,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultItemSummaryRow {
    pub id: String,
    pub user_id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_ciphertext: Vec<u8>,
    pub summary_nonce: Vec<u8>,
    pub summary_version: i32,
    pub favorite: bool,
    pub revision: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub deleted_ts: Option<i64>,
}

type RustyVaultItemSummaryTuple = (
    String,
    String,
    String,
    i32,
    Vec<u8>,
    Vec<u8>,
    i32,
    bool,
    i64,
    i64,
    i64,
    Option<i64>,
);

fn map_rustyvault_item_summary_row(
    (
        user_id,
        id,
        item_type,
        key_version,
        summary_ciphertext,
        summary_nonce,
        summary_version,
        favorite,
        revision,
        created_ts,
        updated_ts,
        deleted_ts,
    ): RustyVaultItemSummaryTuple,
) -> RustyVaultItemSummaryRow {
    RustyVaultItemSummaryRow {
        id,
        user_id,
        item_type,
        key_version,
        summary_ciphertext,
        summary_nonce,
        summary_version,
        favorite,
        revision,
        created_ts,
        updated_ts,
        deleted_ts,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultItemRow {
    pub id: String,
    pub user_id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_ciphertext: Vec<u8>,
    pub summary_nonce: Vec<u8>,
    pub summary_version: i32,
    pub payload_ciphertext: Vec<u8>,
    pub payload_nonce: Vec<u8>,
    pub payload_version: i32,
    pub favorite: bool,
    pub revision: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub deleted_ts: Option<i64>,
}

type RustyVaultItemTuple = (
    String,
    String,
    String,
    i32,
    Vec<u8>,
    Vec<u8>,
    i32,
    Vec<u8>,
    Vec<u8>,
    i32,
    bool,
    i64,
    i64,
    i64,
    Option<i64>,
);

fn map_rustyvault_item_row(
    (
        user_id,
        id,
        item_type,
        key_version,
        summary_ciphertext,
        summary_nonce,
        summary_version,
        payload_ciphertext,
        payload_nonce,
        payload_version,
        favorite,
        revision,
        created_ts,
        updated_ts,
        deleted_ts,
    ): RustyVaultItemTuple,
) -> RustyVaultItemRow {
    RustyVaultItemRow {
        id,
        user_id,
        item_type,
        key_version,
        summary_ciphertext,
        summary_nonce,
        summary_version,
        payload_ciphertext,
        payload_nonce,
        payload_version,
        favorite,
        revision,
        created_ts,
        updated_ts,
        deleted_ts,
    }
}

#[derive(Debug, Clone)]
pub struct RustyVaultUriIndexInput {
    pub id: String,
    pub match_hash: Vec<u8>,
    pub match_type: String,
    pub rank: i32,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct RustyVaultWrappedKeyInsert {
    pub id: String,
    pub key_version: i32,
    pub kdf_algorithm: String,
    pub kdf_memory_kib: i32,
    pub kdf_iterations: i32,
    pub kdf_parallelism: i32,
    pub kdf_salt: Vec<u8>,
    pub hkdf_algorithm: String,
    pub wrap_algorithm: String,
    pub wrap_nonce: Vec<u8>,
    pub wrapped_vault_key: Vec<u8>,
    pub created_ts: i64,
}

#[derive(Debug, Clone)]
pub struct RustyVaultItemUpsert {
    pub id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_ciphertext: Vec<u8>,
    pub summary_nonce: Vec<u8>,
    pub summary_version: i32,
    pub payload_ciphertext: Vec<u8>,
    pub payload_nonce: Vec<u8>,
    pub payload_version: i32,
    pub favorite: bool,
    pub revision: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub uri_indexes: Vec<RustyVaultUriIndexInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustyVaultDeviceSessionRow {
    pub id: String,
    pub user_id: String,
    pub client_kind: String,
    pub device_name: String,
    pub device_platform: Option<String>,
    pub device_fingerprint_hash: Option<String>,
    pub refresh_token_family_id: String,
    pub refresh_token_hash: String,
    pub created_ts: i64,
    pub last_used_ts: i64,
    pub expires_ts: i64,
    pub revoked_ts: Option<i64>,
    pub ip_summary: Option<String>,
    pub user_agent_summary: Option<String>,
}

type RustyVaultDeviceSessionTuple = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<String>,
    Option<String>,
);

fn map_rustyvault_device_session_row(
    (
        id,
        user_id,
        client_kind,
        device_name,
        device_platform,
        device_fingerprint_hash,
        refresh_token_family_id,
        refresh_token_hash,
        created_ts,
        last_used_ts,
        expires_ts,
        revoked_ts,
        ip_summary,
        user_agent_summary,
    ): RustyVaultDeviceSessionTuple,
) -> RustyVaultDeviceSessionRow {
    RustyVaultDeviceSessionRow {
        id,
        user_id,
        client_kind,
        device_name,
        device_platform,
        device_fingerprint_hash,
        refresh_token_family_id,
        refresh_token_hash,
        created_ts,
        last_used_ts,
        expires_ts,
        revoked_ts,
        ip_summary,
        user_agent_summary,
    }
}

#[derive(Debug, Clone)]
pub struct CreateRustyVaultDeviceSessionInput {
    pub id: String,
    pub user_id: String,
    pub client_kind: String,
    pub device_name: String,
    pub device_platform: Option<String>,
    pub device_fingerprint_hash: Option<String>,
    pub refresh_token_family_id: String,
    pub refresh_token_hash: String,
    pub created_ts: i64,
    pub last_used_ts: i64,
    pub expires_ts: i64,
    pub ip_summary: Option<String>,
    pub user_agent_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultDeviceSessionRefreshTokenRow {
    pub id: String,
    pub device_session_id: String,
    pub user_id: String,
    pub refresh_token_family_id: String,
    pub token_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
    pub consumed_ts: Option<i64>,
    pub revoked_ts: Option<i64>,
}

type RustyVaultDeviceSessionRefreshTokenTuple = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
);

fn map_rustyvault_device_session_refresh_token_row(
    (
        id,
        device_session_id,
        user_id,
        refresh_token_family_id,
        token_hash,
        created_ts,
        expires_ts,
        consumed_ts,
        revoked_ts,
    ): RustyVaultDeviceSessionRefreshTokenTuple,
) -> RustyVaultDeviceSessionRefreshTokenRow {
    RustyVaultDeviceSessionRefreshTokenRow {
        id,
        device_session_id,
        user_id,
        refresh_token_family_id,
        token_hash,
        created_ts,
        expires_ts,
        consumed_ts,
        revoked_ts,
    }
}

#[derive(Debug, Clone)]
pub struct CreateRustyVaultDeviceSessionRefreshTokenInput {
    pub id: String,
    pub device_session_id: String,
    pub user_id: String,
    pub refresh_token_family_id: String,
    pub token_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultPendingDeviceApprovalRow {
    pub id: String,
    pub user_id: String,
    pub client_kind: String,
    pub device_name: String,
    pub fingerprint_phrase: String,
    pub pairing_code_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
    pub approved_ts: Option<i64>,
    pub denied_ts: Option<i64>,
}

type RustyVaultPendingDeviceApprovalTuple = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
);

fn map_rustyvault_pending_device_approval_row(
    (
        id,
        user_id,
        client_kind,
        device_name,
        fingerprint_phrase,
        pairing_code_hash,
        created_ts,
        expires_ts,
        approved_ts,
        denied_ts,
    ): RustyVaultPendingDeviceApprovalTuple,
) -> RustyVaultPendingDeviceApprovalRow {
    RustyVaultPendingDeviceApprovalRow {
        id,
        user_id,
        client_kind,
        device_name,
        fingerprint_phrase,
        pairing_code_hash,
        created_ts,
        expires_ts,
        approved_ts,
        denied_ts,
    }
}

#[derive(Debug, Clone)]
pub struct CreateRustyVaultPendingDeviceApprovalInput {
    pub id: String,
    pub user_id: String,
    pub client_kind: String,
    pub device_name: String,
    pub fingerprint_phrase: String,
    pub pairing_code_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustyVaultProtectedActionTokenRow {
    pub id: String,
    pub user_id: String,
    pub device_session_id: Option<String>,
    pub action_kind: String,
    pub target_item_id: Option<String>,
    pub token_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
    pub consumed_ts: Option<i64>,
}

type RustyVaultProtectedActionTokenTuple = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    i64,
    Option<i64>,
);

fn map_rustyvault_protected_action_token_row(
    (
        id,
        user_id,
        device_session_id,
        action_kind,
        target_item_id,
        token_hash,
        created_ts,
        expires_ts,
        consumed_ts,
    ): RustyVaultProtectedActionTokenTuple,
) -> RustyVaultProtectedActionTokenRow {
    RustyVaultProtectedActionTokenRow {
        id,
        user_id,
        device_session_id,
        action_kind,
        target_item_id,
        token_hash,
        created_ts,
        expires_ts,
        consumed_ts,
    }
}

#[derive(Debug, Clone)]
pub struct CreateRustyVaultProtectedActionTokenInput {
    pub id: String,
    pub user_id: String,
    pub device_session_id: Option<String>,
    pub action_kind: String,
    pub target_item_id: Option<String>,
    pub token_hash: String,
    pub created_ts: i64,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustyVaultAuditEventRow {
    pub id: String,
    pub user_id: String,
    pub device_session_id: Option<String>,
    pub event_kind: String,
    pub target_item_id: Option<String>,
    pub event_json: serde_json::Value,
    pub created_ts: i64,
}

type RustyVaultAuditEventTuple = (
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    serde_json::Value,
    i64,
);

fn map_rustyvault_audit_event_row(
    (id, user_id, device_session_id, event_kind, target_item_id, event_json, created_ts): RustyVaultAuditEventTuple,
) -> RustyVaultAuditEventRow {
    RustyVaultAuditEventRow {
        id,
        user_id,
        device_session_id,
        event_kind,
        target_item_id,
        event_json,
        created_ts,
    }
}

#[derive(Debug, Clone)]
pub struct CreateRustyVaultAuditEventInput {
    pub id: String,
    pub user_id: String,
    pub device_session_id: Option<String>,
    pub event_kind: String,
    pub target_item_id: Option<String>,
    pub event_json: serde_json::Value,
    pub created_ts: i64,
}

const RUSTYVAULT_ACCOUNT_COLUMNS: &str = "user_id, status, schema_version, active_key_version, created_ts, updated_ts, last_unlock_required_ts, last_rekey_ts";
const RUSTYVAULT_PREFERENCE_COLUMNS: &str = "user_id, auto_lock_minutes, clipboard_clear_seconds, inline_save_prompt_enabled, inline_autofill_enabled, default_match_mode, warn_on_http, warn_on_untrusted_iframe, excluded_domains, allow_manual_http_fill, updated_ts";
const RUSTYVAULT_WRAPPED_KEY_COLUMNS: &str = "id, user_id, key_version, kdf_algorithm, kdf_memory_kib, kdf_iterations, kdf_parallelism, kdf_salt, hkdf_algorithm, wrap_algorithm, wrap_nonce, wrapped_vault_key, created_ts, superseded_ts";
const RUSTYVAULT_ITEM_SUMMARY_COLUMNS: &str = "user_id, id, item_type, key_version, summary_ciphertext, summary_nonce, summary_version, favorite, revision, created_ts, updated_ts, deleted_ts";
const RUSTYVAULT_ITEM_SUMMARY_COLUMNS_QUALIFIED: &str = "v.user_id, v.id, v.item_type, v.key_version, v.summary_ciphertext, v.summary_nonce, v.summary_version, v.favorite, v.revision, v.created_ts, v.updated_ts, v.deleted_ts";
const RUSTYVAULT_ITEM_COLUMNS: &str = "user_id, id, item_type, key_version, summary_ciphertext, summary_nonce, summary_version, payload_ciphertext, payload_nonce, payload_version, favorite, revision, created_ts, updated_ts, deleted_ts";
const RUSTYVAULT_DEVICE_SESSION_COLUMNS: &str = "id, user_id, client_kind, device_name, device_platform, device_fingerprint_hash, refresh_token_family_id, refresh_token_hash, created_ts, last_used_ts, expires_ts, revoked_ts, ip_summary, user_agent_summary";
const RUSTYVAULT_DEVICE_SESSION_REFRESH_TOKEN_COLUMNS: &str = "id, device_session_id, user_id, refresh_token_family_id, token_hash, created_ts, expires_ts, consumed_ts, revoked_ts";
const RUSTYVAULT_PENDING_DEVICE_APPROVAL_COLUMNS: &str = "id, user_id, client_kind, device_name, fingerprint_phrase, pairing_code_hash, created_ts, expires_ts, approved_ts, denied_ts";
const RUSTYVAULT_PROTECTED_ACTION_COLUMNS: &str = "id, user_id, device_session_id, action_kind, target_item_id, token_hash, created_ts, expires_ts, consumed_ts";
const RUSTYVAULT_AUDIT_COLUMNS: &str =
    "id, user_id, device_session_id, event_kind, target_item_id, event_json, created_ts";

pub async fn get_rustyvault_account(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<RustyVaultAccountRow>, sqlx::Error> {
    let sql =
        format!("SELECT {RUSTYVAULT_ACCOUNT_COLUMNS} FROM rustyvault_account WHERE user_id = $1");
    let row = sqlx::query_as::<_, RustyVaultAccountTuple>(&sql)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_account_row))
}

pub async fn get_rustyvault_preferences(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<RustyVaultPreferenceRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_PREFERENCE_COLUMNS} FROM rustyvault_preference WHERE user_id = $1"
    );
    let row = sqlx::query_as::<_, RustyVaultPreferenceTuple>(&sql)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_preference_row))
}

pub async fn upsert_rustyvault_preferences(
    pool: &DbPool,
    input: &UpsertRustyVaultPreferenceInput,
) -> Result<RustyVaultPreferenceRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO rustyvault_preference \
         (user_id, auto_lock_minutes, clipboard_clear_seconds, inline_save_prompt_enabled, inline_autofill_enabled, default_match_mode, warn_on_http, warn_on_untrusted_iframe, excluded_domains, allow_manual_http_fill, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT (user_id) DO UPDATE SET \
            auto_lock_minutes = EXCLUDED.auto_lock_minutes, \
            clipboard_clear_seconds = EXCLUDED.clipboard_clear_seconds, \
            inline_save_prompt_enabled = EXCLUDED.inline_save_prompt_enabled, \
            inline_autofill_enabled = EXCLUDED.inline_autofill_enabled, \
            default_match_mode = EXCLUDED.default_match_mode, \
            warn_on_http = EXCLUDED.warn_on_http, \
            warn_on_untrusted_iframe = EXCLUDED.warn_on_untrusted_iframe, \
            excluded_domains = EXCLUDED.excluded_domains, \
            allow_manual_http_fill = EXCLUDED.allow_manual_http_fill, \
            updated_ts = EXCLUDED.updated_ts \
         RETURNING {RUSTYVAULT_PREFERENCE_COLUMNS}"
    );
    let row = sqlx::query_as::<_, RustyVaultPreferenceTuple>(&sql)
        .bind(&input.user_id)
        .bind(input.auto_lock_minutes)
        .bind(input.clipboard_clear_seconds)
        .bind(input.inline_save_prompt_enabled)
        .bind(input.inline_autofill_enabled)
        .bind(&input.default_match_mode)
        .bind(input.warn_on_http)
        .bind(input.warn_on_untrusted_iframe)
        .bind(&input.excluded_domains)
        .bind(input.allow_manual_http_fill)
        .bind(input.updated_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_rustyvault_preference_row(row))
}

pub async fn delete_rustyvault_preferences(
    pool: &DbPool,
    user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM rustyvault_preference WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn count_items(pool: &DbPool, user_id: &str) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM rustyvault_item WHERE user_id = $1 AND deleted_ts IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn bootstrap_rustyvault(
    pool: &DbPool,
    user_id: &str,
    account_status: &str,
    schema_version: i32,
    active_key_version: i32,
    wrapped_key: &RustyVaultWrappedKeyInsert,
    now_ts: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO rustyvault_account (user_id, status, schema_version, active_key_version, created_ts, updated_ts, last_unlock_required_ts, last_rekey_ts) \
         VALUES ($1, $2, $3, $4, $5, $5, $5, $5)",
    )
    .bind(user_id)
    .bind(account_status)
    .bind(schema_version)
    .bind(active_key_version)
    .bind(now_ts)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO rustyvault_wrapped_key \
         (id, user_id, key_version, kdf_algorithm, kdf_memory_kib, kdf_iterations, kdf_parallelism, kdf_salt, hkdf_algorithm, wrap_algorithm, wrap_nonce, wrapped_vault_key, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(&wrapped_key.id)
    .bind(user_id)
    .bind(wrapped_key.key_version)
    .bind(&wrapped_key.kdf_algorithm)
    .bind(wrapped_key.kdf_memory_kib)
    .bind(wrapped_key.kdf_iterations)
    .bind(wrapped_key.kdf_parallelism)
    .bind(&wrapped_key.kdf_salt)
    .bind(&wrapped_key.hkdf_algorithm)
    .bind(&wrapped_key.wrap_algorithm)
    .bind(&wrapped_key.wrap_nonce)
    .bind(&wrapped_key.wrapped_vault_key)
    .bind(wrapped_key.created_ts)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn rekey_rustyvault(
    pool: &DbPool,
    user_id: &str,
    active_key_version: i32,
    wrapped_key: &RustyVaultWrappedKeyInsert,
    now_ts: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE rustyvault_wrapped_key SET superseded_ts = $1 WHERE user_id = $2 AND superseded_ts IS NULL")
        .bind(now_ts)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "INSERT INTO rustyvault_wrapped_key \
         (id, user_id, key_version, kdf_algorithm, kdf_memory_kib, kdf_iterations, kdf_parallelism, kdf_salt, hkdf_algorithm, wrap_algorithm, wrap_nonce, wrapped_vault_key, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(&wrapped_key.id)
    .bind(user_id)
    .bind(wrapped_key.key_version)
    .bind(&wrapped_key.kdf_algorithm)
    .bind(wrapped_key.kdf_memory_kib)
    .bind(wrapped_key.kdf_iterations)
    .bind(wrapped_key.kdf_parallelism)
    .bind(&wrapped_key.kdf_salt)
    .bind(&wrapped_key.hkdf_algorithm)
    .bind(&wrapped_key.wrap_algorithm)
    .bind(&wrapped_key.wrap_nonce)
    .bind(&wrapped_key.wrapped_vault_key)
    .bind(wrapped_key.created_ts)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE rustyvault_account SET active_key_version = $1, updated_ts = $2, last_rekey_ts = $2, last_unlock_required_ts = $2 WHERE user_id = $3",
    )
    .bind(active_key_version)
    .bind(now_ts)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn get_active_wrapped_key(
    pool: &DbPool,
    user_id: &str,
) -> Result<Option<RustyVaultWrappedKeyRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_WRAPPED_KEY_COLUMNS} FROM rustyvault_wrapped_key \
         WHERE user_id = $1 AND superseded_ts IS NULL ORDER BY key_version DESC LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultWrappedKeyTuple>(&sql)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_wrapped_key_row))
}

pub async fn list_item_summaries(
    pool: &DbPool,
    user_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<RustyVaultItemSummaryRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_ITEM_SUMMARY_COLUMNS} FROM rustyvault_item \
         WHERE user_id = $1 AND deleted_ts IS NULL ORDER BY updated_ts DESC, id ASC LIMIT $2 OFFSET $3"
    );
    let rows = sqlx::query_as::<_, RustyVaultItemSummaryTuple>(&sql)
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(map_rustyvault_item_summary_row)
        .collect())
}

pub async fn get_item(
    pool: &DbPool,
    user_id: &str,
    item_id: &str,
) -> Result<Option<RustyVaultItemRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_ITEM_COLUMNS} FROM rustyvault_item \
             WHERE user_id = $1 AND id = $2 AND deleted_ts IS NULL LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultItemTuple>(&sql)
        .bind(user_id)
        .bind(item_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_item_row))
}

pub async fn upsert_item(
    pool: &DbPool,
    user_id: &str,
    item: &RustyVaultItemUpsert,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO rustyvault_item \
         (user_id, id, item_type, key_version, summary_ciphertext, summary_nonce, summary_version, payload_ciphertext, payload_nonce, payload_version, favorite, revision, created_ts, updated_ts, deleted_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NULL) \
         ON CONFLICT (user_id, id) DO UPDATE SET \
           item_type = EXCLUDED.item_type, \
           key_version = EXCLUDED.key_version, \
           summary_ciphertext = EXCLUDED.summary_ciphertext, \
           summary_nonce = EXCLUDED.summary_nonce, \
           summary_version = EXCLUDED.summary_version, \
           payload_ciphertext = EXCLUDED.payload_ciphertext, \
           payload_nonce = EXCLUDED.payload_nonce, \
           payload_version = EXCLUDED.payload_version, \
           favorite = EXCLUDED.favorite, \
           revision = EXCLUDED.revision, \
           updated_ts = EXCLUDED.updated_ts, \
           deleted_ts = NULL",
    )
    .bind(user_id)
    .bind(&item.id)
    .bind(&item.item_type)
    .bind(item.key_version)
    .bind(&item.summary_ciphertext)
    .bind(&item.summary_nonce)
    .bind(item.summary_version)
    .bind(&item.payload_ciphertext)
    .bind(&item.payload_nonce)
    .bind(item.payload_version)
    .bind(item.favorite)
    .bind(item.revision)
    .bind(item.created_ts)
    .bind(item.updated_ts)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM rustyvault_item_uri_index WHERE user_id = $1 AND item_id = $2")
        .bind(user_id)
        .bind(&item.id)
        .execute(&mut *tx)
        .await?;

    for uri_index in &item.uri_indexes {
        sqlx::query(
            "INSERT INTO rustyvault_item_uri_index (id, user_id, item_id, match_hash, match_type, rank, created_ts) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&uri_index.id)
        .bind(user_id)
        .bind(&item.id)
        .bind(&uri_index.match_hash)
        .bind(&uri_index.match_type)
        .bind(uri_index.rank)
        .bind(uri_index.created_ts)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn soft_delete_item(
    pool: &DbPool,
    user_id: &str,
    item_id: &str,
    now_ts: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE rustyvault_item SET deleted_ts = $3, updated_ts = $3 \
         WHERE user_id = $1 AND id = $2 AND deleted_ts IS NULL",
    )
    .bind(user_id)
    .bind(item_id)
    .bind(now_ts)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query("DELETE FROM rustyvault_item_uri_index WHERE user_id = $1 AND item_id = $2")
        .bind(user_id)
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn lookup_item_summaries(
    pool: &DbPool,
    user_id: &str,
    match_hashes: &[Vec<u8>],
) -> Result<Vec<RustyVaultItemSummaryRow>, sqlx::Error> {
    if match_hashes.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(2, match_hashes.len());
    let sql = format!(
        "SELECT DISTINCT {RUSTYVAULT_ITEM_SUMMARY_COLUMNS_QUALIFIED} \
         FROM rustyvault_item v \
         INNER JOIN rustyvault_item_uri_index i ON i.user_id = v.user_id AND i.item_id = v.id \
         WHERE v.user_id = $1 AND v.deleted_ts IS NULL AND i.match_hash IN ({placeholders}) \
         ORDER BY v.updated_ts DESC, v.id ASC \
         LIMIT 25"
    );
    let mut query = sqlx::query_as::<_, RustyVaultItemSummaryTuple>(&sql).bind(user_id);
    for hash in match_hashes {
        query = query.bind(hash);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(map_rustyvault_item_summary_row)
        .collect())
}

pub async fn list_all_items(
    pool: &DbPool,
    user_id: &str,
) -> Result<Vec<RustyVaultItemRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_ITEM_COLUMNS} FROM rustyvault_item \
         WHERE user_id = $1 AND deleted_ts IS NULL ORDER BY updated_ts DESC, id ASC"
    );
    let rows = sqlx::query_as::<_, RustyVaultItemTuple>(&sql)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(map_rustyvault_item_row).collect())
}

pub async fn clear_all_items(pool: &DbPool, user_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM rustyvault_item_uri_index WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_item WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn destroy_rustyvault(pool: &DbPool, user_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM rustyvault_preference WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_item_uri_index WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_item WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_device_session_refresh_token WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_device_session WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_pending_device_approval WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_protected_action_token WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_audit_event WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_wrapped_key WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM rustyvault_account WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create_device_session(
    pool: &DbPool,
    input: &CreateRustyVaultDeviceSessionInput,
) -> Result<RustyVaultDeviceSessionRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO rustyvault_device_session \
         (id, user_id, client_kind, device_name, device_platform, device_fingerprint_hash, refresh_token_family_id, refresh_token_hash, created_ts, last_used_ts, expires_ts, ip_summary, user_agent_summary) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
         RETURNING {RUSTYVAULT_DEVICE_SESSION_COLUMNS}"
    );
    let row = sqlx::query_as::<_, RustyVaultDeviceSessionTuple>(&sql)
        .bind(&input.id)
        .bind(&input.user_id)
        .bind(&input.client_kind)
        .bind(&input.device_name)
        .bind(&input.device_platform)
        .bind(&input.device_fingerprint_hash)
        .bind(&input.refresh_token_family_id)
        .bind(&input.refresh_token_hash)
        .bind(input.created_ts)
        .bind(input.last_used_ts)
        .bind(input.expires_ts)
        .bind(&input.ip_summary)
        .bind(&input.user_agent_summary)
        .fetch_one(pool)
        .await?;
    Ok(map_rustyvault_device_session_row(row))
}

pub async fn create_device_session_refresh_token(
    pool: &DbPool,
    input: &CreateRustyVaultDeviceSessionRefreshTokenInput,
) -> Result<RustyVaultDeviceSessionRefreshTokenRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO rustyvault_device_session_refresh_token \
         (id, device_session_id, user_id, refresh_token_family_id, token_hash, created_ts, expires_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING {RUSTYVAULT_DEVICE_SESSION_REFRESH_TOKEN_COLUMNS}"
    );
    let row = sqlx::query_as::<_, RustyVaultDeviceSessionRefreshTokenTuple>(&sql)
        .bind(&input.id)
        .bind(&input.device_session_id)
        .bind(&input.user_id)
        .bind(&input.refresh_token_family_id)
        .bind(&input.token_hash)
        .bind(input.created_ts)
        .bind(input.expires_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_rustyvault_device_session_refresh_token_row(row))
}

pub async fn list_device_sessions(
    pool: &DbPool,
    user_id: &str,
) -> Result<Vec<RustyVaultDeviceSessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_DEVICE_SESSION_COLUMNS} FROM rustyvault_device_session \
         WHERE user_id = $1 ORDER BY last_used_ts DESC, created_ts DESC"
    );
    let rows = sqlx::query_as::<_, RustyVaultDeviceSessionTuple>(&sql)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(map_rustyvault_device_session_row)
        .collect())
}

pub async fn get_device_session(
    pool: &DbPool,
    user_id: &str,
    session_id: &str,
) -> Result<Option<RustyVaultDeviceSessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_DEVICE_SESSION_COLUMNS} FROM rustyvault_device_session WHERE user_id = $1 AND id = $2 LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultDeviceSessionTuple>(&sql)
        .bind(user_id)
        .bind(session_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_device_session_row))
}

pub async fn get_device_session_by_refresh_hash(
    pool: &DbPool,
    refresh_token_hash: &str,
) -> Result<Option<RustyVaultDeviceSessionRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_DEVICE_SESSION_COLUMNS} FROM rustyvault_device_session WHERE refresh_token_hash = $1 LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultDeviceSessionTuple>(&sql)
        .bind(refresh_token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_device_session_row))
}

pub async fn get_device_session_refresh_token_by_hash(
    pool: &DbPool,
    token_hash: &str,
) -> Result<Option<RustyVaultDeviceSessionRefreshTokenRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_DEVICE_SESSION_REFRESH_TOKEN_COLUMNS} \
         FROM rustyvault_device_session_refresh_token WHERE token_hash = $1 LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultDeviceSessionRefreshTokenTuple>(&sql)
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_device_session_refresh_token_row))
}

pub struct RotateRustyVaultDeviceSessionRefreshTokenParams<'a> {
    pub session_id: &'a str,
    pub current_token_id: &'a str,
    pub current_token_hash: &'a str,
    pub family_id: &'a str,
    pub user_id: &'a str,
    pub next_refresh_token_hash: &'a str,
    pub now_ts: i64,
    pub expires_ts: i64,
}

pub async fn rotate_device_session_refresh_token(
    pool: &DbPool,
    params: RotateRustyVaultDeviceSessionRefreshTokenParams<'_>,
) -> Result<bool, sqlx::Error> {
    let RotateRustyVaultDeviceSessionRefreshTokenParams {
        session_id,
        current_token_id,
        current_token_hash,
        family_id,
        user_id,
        next_refresh_token_hash,
        now_ts,
        expires_ts,
    } = params;
    let mut tx = pool.begin().await?;
    let token_result = sqlx::query(
        "UPDATE rustyvault_device_session_refresh_token SET consumed_ts = $2 \
         WHERE id = $1 AND token_hash = $3 AND consumed_ts IS NULL AND revoked_ts IS NULL",
    )
    .bind(current_token_id)
    .bind(now_ts)
    .bind(current_token_hash)
    .execute(&mut *tx)
    .await?;
    if token_result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }

    let session_result = sqlx::query(
        "UPDATE rustyvault_device_session SET refresh_token_hash = $2, last_used_ts = $3, expires_ts = $4 \
         WHERE id = $1 AND revoked_ts IS NULL",
    )
    .bind(session_id)
    .bind(next_refresh_token_hash)
    .bind(now_ts)
    .bind(expires_ts)
    .execute(&mut *tx)
    .await?;
    if session_result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }

    sqlx::query(
        "INSERT INTO rustyvault_device_session_refresh_token \
         (id, device_session_id, user_id, refresh_token_family_id, token_hash, created_ts, expires_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(user_id)
    .bind(family_id)
    .bind(next_refresh_token_hash)
    .bind(now_ts)
    .bind(expires_ts)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}

pub async fn touch_device_session(
    pool: &DbPool,
    session_id: &str,
    now_ts: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE rustyvault_device_session SET last_used_ts = $2 WHERE id = $1 AND revoked_ts IS NULL",
    )
    .bind(session_id)
    .bind(now_ts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn revoke_device_session(
    pool: &DbPool,
    user_id: &str,
    session_id: &str,
    now_ts: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE rustyvault_device_session SET revoked_ts = COALESCE(revoked_ts, $3) WHERE user_id = $1 AND id = $2",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(now_ts)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query(
        "UPDATE rustyvault_device_session_refresh_token SET revoked_ts = COALESCE(revoked_ts, $2) \
         WHERE user_id = $1 AND device_session_id = $3",
    )
    .bind(user_id)
    .bind(now_ts)
    .bind(session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn revoke_other_device_sessions(
    pool: &DbPool,
    user_id: &str,
    keep_session_id: Option<&str>,
    now_ts: i64,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = if let Some(keep) = keep_session_id {
        sqlx::query(
            "UPDATE rustyvault_device_session SET revoked_ts = COALESCE(revoked_ts, $3) \
             WHERE user_id = $1 AND id <> $2 AND revoked_ts IS NULL",
        )
        .bind(user_id)
        .bind(keep)
        .bind(now_ts)
        .execute(&mut *tx)
        .await?
    } else {
        sqlx::query(
            "UPDATE rustyvault_device_session SET revoked_ts = COALESCE(revoked_ts, $2) \
             WHERE user_id = $1 AND revoked_ts IS NULL",
        )
        .bind(user_id)
        .bind(now_ts)
        .execute(&mut *tx)
        .await?
    };

    if let Some(keep) = keep_session_id {
        sqlx::query(
            "UPDATE rustyvault_device_session_refresh_token SET revoked_ts = COALESCE(revoked_ts, $3) \
             WHERE user_id = $1 AND device_session_id <> $2 AND revoked_ts IS NULL",
        )
        .bind(user_id)
        .bind(keep)
        .bind(now_ts)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE rustyvault_device_session_refresh_token SET revoked_ts = COALESCE(revoked_ts, $2) \
             WHERE user_id = $1 AND revoked_ts IS NULL",
        )
        .bind(user_id)
        .bind(now_ts)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn revoke_device_session_refresh_family(
    pool: &DbPool,
    family_id: &str,
    now_ts: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE rustyvault_device_session_refresh_token SET revoked_ts = COALESCE(revoked_ts, $2) \
         WHERE refresh_token_family_id = $1 AND revoked_ts IS NULL",
    )
    .bind(family_id)
    .bind(now_ts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn create_pending_device_approval(
    pool: &DbPool,
    input: &CreateRustyVaultPendingDeviceApprovalInput,
) -> Result<RustyVaultPendingDeviceApprovalRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO rustyvault_pending_device_approval \
         (id, user_id, client_kind, device_name, fingerprint_phrase, pairing_code_hash, created_ts, expires_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING {RUSTYVAULT_PENDING_DEVICE_APPROVAL_COLUMNS}"
    );
    let row = sqlx::query_as::<_, RustyVaultPendingDeviceApprovalTuple>(&sql)
        .bind(&input.id)
        .bind(&input.user_id)
        .bind(&input.client_kind)
        .bind(&input.device_name)
        .bind(&input.fingerprint_phrase)
        .bind(&input.pairing_code_hash)
        .bind(input.created_ts)
        .bind(input.expires_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_rustyvault_pending_device_approval_row(row))
}

pub async fn get_pending_device_approval_by_code_hash(
    pool: &DbPool,
    pairing_code_hash: &str,
) -> Result<Option<RustyVaultPendingDeviceApprovalRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_PENDING_DEVICE_APPROVAL_COLUMNS} FROM rustyvault_pending_device_approval \
         WHERE pairing_code_hash = $1 LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultPendingDeviceApprovalTuple>(&sql)
        .bind(pairing_code_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_pending_device_approval_row))
}

pub async fn mark_pending_device_approval_consumed(
    pool: &DbPool,
    approval_id: &str,
    approved_ts: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE rustyvault_pending_device_approval SET approved_ts = $2 WHERE id = $1 AND approved_ts IS NULL AND denied_ts IS NULL",
    )
    .bind(approval_id)
    .bind(approved_ts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn create_protected_action_token(
    pool: &DbPool,
    input: &CreateRustyVaultProtectedActionTokenInput,
) -> Result<RustyVaultProtectedActionTokenRow, sqlx::Error> {
    let sql = format!(
        "INSERT INTO rustyvault_protected_action_token \
         (id, user_id, device_session_id, action_kind, target_item_id, token_hash, created_ts, expires_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING {RUSTYVAULT_PROTECTED_ACTION_COLUMNS}"
    );
    let row = sqlx::query_as::<_, RustyVaultProtectedActionTokenTuple>(&sql)
        .bind(&input.id)
        .bind(&input.user_id)
        .bind(&input.device_session_id)
        .bind(&input.action_kind)
        .bind(&input.target_item_id)
        .bind(&input.token_hash)
        .bind(input.created_ts)
        .bind(input.expires_ts)
        .fetch_one(pool)
        .await?;
    Ok(map_rustyvault_protected_action_token_row(row))
}

pub async fn get_protected_action_token_by_hash(
    pool: &DbPool,
    token_hash: &str,
) -> Result<Option<RustyVaultProtectedActionTokenRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_PROTECTED_ACTION_COLUMNS} FROM rustyvault_protected_action_token WHERE token_hash = $1 LIMIT 1"
    );
    let row = sqlx::query_as::<_, RustyVaultProtectedActionTokenTuple>(&sql)
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(map_rustyvault_protected_action_token_row))
}

pub async fn consume_protected_action_token(
    pool: &DbPool,
    token_id: &str,
    consumed_ts: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE rustyvault_protected_action_token SET consumed_ts = $2 WHERE id = $1 AND consumed_ts IS NULL",
    )
    .bind(token_id)
    .bind(consumed_ts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn create_audit_event(
    pool: &DbPool,
    input: &CreateRustyVaultAuditEventInput,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO rustyvault_audit_event (id, user_id, device_session_id, event_kind, target_item_id, event_json, created_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&input.id)
    .bind(&input.user_id)
    .bind(&input.device_session_id)
    .bind(&input.event_kind)
    .bind(&input.target_item_id)
    .bind(&input.event_json)
    .bind(input.created_ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_audit_events(
    pool: &DbPool,
    user_id: &str,
    limit: i64,
) -> Result<Vec<RustyVaultAuditEventRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {RUSTYVAULT_AUDIT_COLUMNS} FROM rustyvault_audit_event WHERE user_id = $1 ORDER BY created_ts DESC, id DESC LIMIT $2"
    );
    let rows = sqlx::query_as::<_, RustyVaultAuditEventTuple>(&sql)
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(map_rustyvault_audit_event_row)
        .collect())
}
