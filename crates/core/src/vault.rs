use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultClientKind {
    WebVault,
    BrowserExtension,
}

impl VaultClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebVault => "web_vault",
            Self::BrowserExtension => "browser_extension",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultUriMatchMode {
    Exact,
    Host,
    BaseDomain,
    Never,
}

impl VaultUriMatchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Host => "host",
            Self::BaseDomain => "base_domain",
            Self::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultProtectedActionKind {
    Rekey,
    Export,
    ImportOverwrite,
    DestroyVault,
    ApproveDevice,
    RevokeOtherSessions,
}

impl VaultProtectedActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rekey => "rekey",
            Self::Export => "export",
            Self::ImportOverwrite => "import_overwrite",
            Self::DestroyVault => "destroy_vault",
            Self::ApproveDevice => "approve_device",
            Self::RevokeOtherSessions => "revoke_other_sessions",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultWrappedKeyMetadata {
    pub key_version: i32,
    pub kdf_algorithm: String,
    pub kdf_memory_kib: i32,
    pub kdf_iterations: i32,
    pub kdf_parallelism: i32,
    pub kdf_salt_hex: String,
    pub hkdf_algorithm: String,
    pub wrap_algorithm: String,
    pub wrap_nonce_hex: String,
    pub wrapped_vault_key_hex: String,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultConfigResponse {
    pub enabled: bool,
    pub schema_version: i32,
    pub supported_kdf_algorithms: Vec<String>,
    pub supported_encryption_algorithms: Vec<String>,
    pub active_wrapped_key: Option<VaultWrappedKeyMetadata>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedVaultItemSummary {
    pub id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_version: i32,
    pub summary_nonce_hex: String,
    pub summary_ciphertext_hex: String,
    pub favorite: bool,
    pub revision: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub deleted_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedVaultItem {
    pub id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_version: i32,
    pub summary_nonce_hex: String,
    pub summary_ciphertext_hex: String,
    pub payload_version: i32,
    pub payload_nonce_hex: String,
    pub payload_ciphertext_hex: String,
    pub favorite: bool,
    pub revision: i64,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub deleted_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpsertVaultItemRequest {
    pub id: String,
    pub item_type: String,
    pub key_version: i32,
    pub summary_version: i32,
    pub summary_nonce_hex: String,
    pub summary_ciphertext_hex: String,
    pub payload_version: i32,
    pub payload_nonce_hex: String,
    pub payload_ciphertext_hex: String,
    pub favorite: bool,
    pub revision: i64,
    #[serde(default)]
    pub uri_indexes: Vec<VaultUriIndexInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultUriIndexInput {
    pub match_hash_hex: String,
    pub match_type: VaultUriMatchMode,
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultItemListResponse {
    pub items: Vec<EncryptedVaultItemSummary>,
    pub next_offset: Option<i64>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultLookupRequest {
    pub match_hashes_hex: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultLookupResponse {
    pub items: Vec<EncryptedVaultItemSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultSyncResponse {
    pub cursor: i64,
    pub items: Vec<EncryptedVaultItemSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultDeviceSessionResponse {
    pub id: String,
    pub client_kind: VaultClientKind,
    pub device_name: String,
    pub device_platform: Option<String>,
    pub created_ts: i64,
    pub last_used_ts: i64,
    pub expires_ts: i64,
    pub revoked_ts: Option<i64>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateVaultDeviceSessionRequest {
    pub client_kind: VaultClientKind,
    pub device_name: String,
    #[serde(default)]
    pub device_platform: Option<String>,
    #[serde(default)]
    pub protected_action_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateVaultDeviceSessionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<VaultDeviceSessionTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<VaultPairingCodeResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultDeviceSessionTokens {
    pub session_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_ts: i64,
    pub refresh_expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultPairingCodeResponse {
    pub pairing_code: String,
    pub fingerprint_phrase: String,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumeVaultPairingCodeRequest {
    pub pairing_code: String,
    pub device_name: String,
    #[serde(default)]
    pub device_platform: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultSessionRefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultProtectedActionChallengeRequest {
    pub action_kind: VaultProtectedActionKind,
    #[serde(default)]
    pub target_item_id: Option<String>,
    pub current_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultProtectedActionChallengeResponse {
    pub action_token: String,
    pub action_kind: VaultProtectedActionKind,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultProtectedActionCompleteRequest {
    pub action_token: String,
    pub action_kind: VaultProtectedActionKind,
    #[serde(default)]
    pub target_item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultProtectedActionCompleteResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultAuditEventResponse {
    pub id: String,
    pub event_kind: String,
    pub target_item_id: Option<String>,
    pub created_ts: i64,
    pub event_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultAuditListResponse {
    pub events: Vec<VaultAuditEventResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultExportRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultExportResponse {
    pub config: VaultConfigResponse,
    pub items: Vec<EncryptedVaultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultImportBitwardenRequest {
    pub protected_action_token: String,
    #[serde(default)]
    pub clear_existing: bool,
    pub items: Vec<UpsertVaultItemRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultImportBitwardenResponse {
    pub imported_count: usize,
    pub cleared_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultBootstrapRequest {
    pub wrapped_key: VaultWrappedKeyMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultRevokeOtherSessionsRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultRevokeOtherSessionsResponse {
    pub revoked_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultDestroyRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultDestroyResponse {
    pub destroyed: bool,
}
