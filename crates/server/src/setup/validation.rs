use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

static REGION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{2}$").unwrap());

/// Validate create admin request fields. Returns field errors or None.
pub fn validate_admin(username: &str, password: &str) -> Option<Value> {
    crate::user_pipeline::validate_username_password(username, password)
}

/// Validate setup config fields.
pub fn validate_config(
    server_name: &str,
    locale: &str,
    region: &str,
    time_zone: &Option<String>,
) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if server_name.is_empty() || server_name.len() > 64 {
        fields.insert(
            "server_name".to_string(),
            json!(["must be between 1 and 64 characters"]),
        );
    }

    if locale.len() < 2 || locale.len() > 32 {
        fields.insert(
            "default_ui_locale".to_string(),
            json!(["must be between 2 and 32 characters (BCP-47)"]),
        );
    }

    if !REGION_RE.is_match(region) {
        fields.insert(
            "default_region".to_string(),
            json!(["must be ISO 3166-1 alpha-2 (two uppercase letters)"]),
        );
    }

    if let Some(tz) = time_zone {
        if !tz.is_empty() && tz.len() > 64 {
            fields.insert(
                "default_time_zone".to_string(),
                json!(["must be at most 64 characters"]),
            );
        }
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

/// Validate metadata fields.
pub fn validate_metadata(language: &str, region: &str) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if language.len() < 2 || language.len() > 32 {
        fields.insert(
            "metadata_language".to_string(),
            json!(["must be between 2 and 32 characters"]),
        );
    }

    if !REGION_RE.is_match(region) {
        fields.insert(
            "metadata_region".to_string(),
            json!(["must be ISO 3166-1 alpha-2 (two uppercase letters)"]),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

/// Validate network config.
pub fn validate_network(trusted_proxies: &[String]) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if trusted_proxies.len() > 64 {
        fields.insert(
            "trusted_proxies".to_string(),
            json!(["must have at most 64 entries"]),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

/// Validate a library spec.
pub fn validate_library_spec(name: &str, kind: &str, paths: &[String]) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if name.is_empty() || name.len() > 64 {
        fields.insert(
            "name".to_string(),
            json!(["must be between 1 and 64 characters"]),
        );
    }

    let valid_kinds = ["movie", "show", "music", "mixed"];
    if !valid_kinds.contains(&kind) {
        fields.insert(
            "kind".to_string(),
            json!(["must be one of: movie, show, music, mixed"]),
        );
    }

    if paths.is_empty() || paths.len() > 32 {
        fields.insert(
            "paths".to_string(),
            json!(["must have between 1 and 32 paths"]),
        );
    }

    for (i, p) in paths.iter().enumerate() {
        if p.is_empty() || p.len() > 4096 {
            fields.insert(
                format!("paths[{i}]"),
                json!(["must be between 1 and 4096 characters"]),
            );
        }
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}

/// Validate path for validate endpoint.
pub fn validate_path_input(path: &str) -> Option<Value> {
    let mut fields = serde_json::Map::new();

    if path.is_empty() || path.len() > 4096 {
        fields.insert(
            "path".to_string(),
            json!(["must be between 1 and 4096 characters"]),
        );
    }

    if fields.is_empty() {
        None
    } else {
        Some(Value::Object(fields))
    }
}
