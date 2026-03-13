use rand::Rng;
use rand::distributions::{Alphanumeric, DistString};
use rustfin_core::error::ApiError;
use sha2::{Digest, Sha256};

pub const RUSTYVAULT_SCHEMA_VERSION: i32 = 1;
pub const RUSTYVAULT_KDF_ALGORITHM: &str = "argon2id";
pub const RUSTYVAULT_HKDF_ALGORITHM: &str = "hkdf-sha-256";
pub const RUSTYVAULT_WRAP_ALGORITHM: &str = "aes-256-gcm";
pub const RUSTYVAULT_ENCRYPTION_ALGORITHM: &str = "aes-256-gcm";
pub const RUSTYVAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 15 * 60;
pub const RUSTYVAULT_REFRESH_TOKEN_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const RUSTYVAULT_PAIRING_TTL_SECONDS: i64 = 10 * 60;
pub const RUSTYVAULT_PROTECTED_ACTION_TTL_SECONDS: i64 = 60;
pub const RUSTYVAULT_LIST_MAX_LIMIT: i64 = 100;
pub const RUSTYVAULT_AUDIT_LIMIT: i64 = 100;
pub const RUSTYVAULT_MAX_MATCH_HASHES: usize = 16;
pub const RUSTYVAULT_MAX_IMPORT_ITEMS: usize = 2_000;
pub const RUSTYVAULT_MAX_URI_INDEXES_PER_ITEM: usize = 32;
pub const RUSTYVAULT_MAX_BLOB_BYTES: usize = 128 * 1024;
pub const RUSTYVAULT_MAX_DEVICE_NAME_CHARS: usize = 120;
pub const RUSTYVAULT_MAX_DEVICE_PLATFORM_CHARS: usize = 80;

pub fn current_timestamp() -> i64 {
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
    let adjective = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
    let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
    format!("{adjective}-{noun}")
}

pub fn decode_hex_field(field_name: &str, value: &str) -> Result<Vec<u8>, ApiError> {
    let decoded = hex::decode(value.trim())
        .map_err(|_| ApiError::BadRequest(format!("{field_name} must be valid hex")))?;
    if decoded.len() > RUSTYVAULT_MAX_BLOB_BYTES {
        return Err(ApiError::BadRequest(format!(
            "{field_name} exceeds size limit"
        )));
    }
    Ok(decoded)
}

pub fn sanitize_limit(limit: Option<i64>, default: i64) -> i64 {
    limit.unwrap_or(default).clamp(1, RUSTYVAULT_LIST_MAX_LIMIT)
}

pub fn sanitize_offset(offset: Option<i64>) -> i64 {
    offset.unwrap_or(0).max(0)
}

pub fn sanitize_device_name(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("device_name is required".into()));
    }
    if trimmed.chars().count() > RUSTYVAULT_MAX_DEVICE_NAME_CHARS {
        return Err(ApiError::BadRequest("device_name is too long".into()));
    }
    Ok(trimmed.to_string())
}

pub fn sanitize_device_platform(raw: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(value) = raw else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > RUSTYVAULT_MAX_DEVICE_PLATFORM_CHARS {
        return Err(ApiError::BadRequest("device_platform is too long".into()));
    }
    Ok(Some(trimmed.to_string()))
}
