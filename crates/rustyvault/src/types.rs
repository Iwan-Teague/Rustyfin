use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RustyVaultClientKind {
    #[serde(
        rename = "rustyvault_web",
        alias = "web_client",
        alias = "WebClient",
        alias = "RustyVault_web"
    )]
    WebClient,
    #[serde(rename = "browser_extension", alias = "BrowserExtension")]
    BrowserExtension,
}

impl RustyVaultClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebClient => "rustyvault_web",
            Self::BrowserExtension => "browser_extension",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RustyVaultUriMatchMode {
    Exact,
    Host,
    BaseDomain,
    Never,
}

impl RustyVaultUriMatchMode {
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
pub enum RustyVaultProtectedActionKind {
    Rekey,
    Export,
    ImportOverwrite,
    DestroyRustyVault,
    ApproveDevice,
    RevokeOtherSessions,
}

impl RustyVaultProtectedActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rekey => "rekey",
            Self::Export => "export",
            Self::ImportOverwrite => "import_overwrite",
            Self::DestroyRustyVault => "destroy_rustyvault",
            Self::ApproveDevice => "approve_device",
            Self::RevokeOtherSessions => "revoke_other_sessions",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultWrappedKeyMetadata {
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
pub struct RustyVaultConfigResponse {
    pub enabled: bool,
    pub schema_version: i32,
    pub supported_kdf_algorithms: Vec<String>,
    pub supported_encryption_algorithms: Vec<String>,
    pub active_wrapped_key: Option<RustyVaultWrappedKeyMetadata>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RustyVaultPreferences {
    pub auto_lock_minutes: u32,
    pub clipboard_clear_seconds: u32,
    pub inline_save_prompt_enabled: bool,
    pub inline_autofill_enabled: bool,
    pub default_match_mode: String,
    pub warn_on_http: bool,
    pub warn_on_untrusted_iframe: bool,
    pub excluded_domains: Vec<String>,
    pub allow_manual_http_fill: bool,
}

impl Default for RustyVaultPreferences {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 15,
            clipboard_clear_seconds: 30,
            inline_save_prompt_enabled: true,
            inline_autofill_enabled: true,
            default_match_mode: "base_domain".to_string(),
            warn_on_http: true,
            warn_on_untrusted_iframe: true,
            excluded_domains: Vec::new(),
            allow_manual_http_fill: false,
        }
    }
}

impl RustyVaultPreferences {
    pub fn normalized(mut self) -> Self {
        self.default_match_mode = normalize_match_mode(&self.default_match_mode);
        self.excluded_domains = normalize_domains(self.excluded_domains);
        self
    }
}

fn normalize_match_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "exact" | "host" | "base_domain" | "never" => raw.trim().to_ascii_lowercase(),
        _ => "base_domain".to_string(),
    }
}

fn normalize_domains(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedRustyVaultItemSummary {
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
pub struct EncryptedRustyVaultItem {
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
pub struct UpsertRustyVaultItemRequest {
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
    pub uri_indexes: Vec<RustyVaultUriIndexInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultUriIndexInput {
    pub match_hash_hex: String,
    pub match_type: RustyVaultUriMatchMode,
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultItemListResponse {
    pub items: Vec<EncryptedRustyVaultItemSummary>,
    pub next_offset: Option<i64>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultLookupRequest {
    pub match_hashes_hex: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultLookupResponse {
    pub items: Vec<EncryptedRustyVaultItemSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultDeviceSessionResponse {
    pub id: String,
    pub client_kind: RustyVaultClientKind,
    pub device_name: String,
    pub device_platform: Option<String>,
    pub created_ts: i64,
    pub last_used_ts: i64,
    pub expires_ts: i64,
    pub revoked_ts: Option<i64>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateRustyVaultDeviceSessionRequest {
    pub client_kind: RustyVaultClientKind,
    pub device_name: String,
    #[serde(default)]
    pub device_platform: Option<String>,
    #[serde(default)]
    pub protected_action_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateRustyVaultDeviceSessionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<RustyVaultDeviceSessionTokens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<RustyVaultPairingCodeResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultDeviceSessionTokens {
    pub session_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_ts: i64,
    pub refresh_expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultPairingCodeResponse {
    pub pairing_code: String,
    pub fingerprint_phrase: String,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumeRustyVaultPairingCodeRequest {
    pub pairing_code: String,
    pub device_name: String,
    #[serde(default)]
    pub device_platform: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        CreateRustyVaultDeviceSessionRequest, RustyVaultClientKind, RustyVaultPreferences,
    };

    #[test]
    fn normalizes_rustyvault_preferences() {
        let normalized = RustyVaultPreferences {
            default_match_mode: " HOST ".to_string(),
            excluded_domains: vec![
                " Example.com ".to_string(),
                "example.com".to_string(),
                "".to_string(),
                "Sub.EXAMPLE.com".to_string(),
            ],
            ..RustyVaultPreferences::default()
        }
        .normalized();

        assert_eq!(normalized.default_match_mode, "host");
        assert_eq!(
            normalized.excluded_domains,
            vec!["example.com".to_string(), "sub.example.com".to_string()]
        );
    }

    #[test]
    fn rustyvault_client_kind_uses_canonical_wire_values() {
        let serialized = serde_json::to_string(&RustyVaultClientKind::WebClient).unwrap();
        assert_eq!(serialized, "\"rustyvault_web\"");

        let serialized = serde_json::to_string(&RustyVaultClientKind::BrowserExtension).unwrap();
        assert_eq!(serialized, "\"browser_extension\"");
    }

    #[test]
    fn rustyvault_client_kind_accepts_rollout_aliases() {
        let parsed: RustyVaultClientKind = serde_json::from_str("\"rustyvault_web\"").unwrap();
        assert_eq!(parsed, RustyVaultClientKind::WebClient);

        let parsed: RustyVaultClientKind = serde_json::from_str("\"web_client\"").unwrap();
        assert_eq!(parsed, RustyVaultClientKind::WebClient);

        let parsed: RustyVaultClientKind = serde_json::from_str("\"WebClient\"").unwrap();
        assert_eq!(parsed, RustyVaultClientKind::WebClient);

        let parsed: RustyVaultClientKind = serde_json::from_str("\"RustyVault_web\"").unwrap();
        assert_eq!(parsed, RustyVaultClientKind::WebClient);
    }

    #[test]
    fn create_device_session_request_deserializes_rustyvault_web_client_kind() {
        let request: CreateRustyVaultDeviceSessionRequest = serde_json::from_str(
            r#"{
                "client_kind": "rustyvault_web",
                "device_name": "RustyVault Web Vault"
            }"#,
        )
        .unwrap();

        assert_eq!(request.client_kind, RustyVaultClientKind::WebClient);
        assert_eq!(request.device_name, "RustyVault Web Vault");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultSessionRefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultProtectedActionChallengeRequest {
    pub action_kind: RustyVaultProtectedActionKind,
    #[serde(default)]
    pub target_item_id: Option<String>,
    pub current_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultProtectedActionChallengeResponse {
    pub action_token: String,
    pub action_kind: RustyVaultProtectedActionKind,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultAuditEventResponse {
    pub id: String,
    pub event_kind: String,
    pub target_item_id: Option<String>,
    pub created_ts: i64,
    pub event_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultAuditListResponse {
    pub events: Vec<RustyVaultAuditEventResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultExportRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultExportResponse {
    pub config: RustyVaultConfigResponse,
    pub items: Vec<EncryptedRustyVaultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultImportBitwardenRequest {
    pub protected_action_token: String,
    #[serde(default)]
    pub clear_existing: bool,
    pub items: Vec<UpsertRustyVaultItemRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultImportBitwardenResponse {
    pub imported_count: usize,
    pub cleared_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultBootstrapRequest {
    pub wrapped_key: RustyVaultWrappedKeyMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultRevokeOtherSessionsRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultRevokeOtherSessionsResponse {
    pub revoked_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultDestroyRequest {
    pub protected_action_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RustyVaultDestroyResponse {
    pub destroyed: bool,
}
