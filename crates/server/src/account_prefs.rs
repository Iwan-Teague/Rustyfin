use serde::{Deserialize, Serialize};

const PREFS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserPreferences {
    pub version: u32,
    pub audio: AudioPreferences,
    pub activity: ActivityPreferences,
    pub privacy: PrivacyPreferences,
    pub vault: VaultPreferences,
    pub notifications: NotificationPreferences,
    pub accessibility: AccessibilityPreferences,
    pub appearance: AppearancePreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            version: PREFS_VERSION,
            audio: AudioPreferences::default(),
            activity: ActivityPreferences::default(),
            privacy: PrivacyPreferences::default(),
            vault: VaultPreferences::default(),
            notifications: NotificationPreferences::default(),
            accessibility: AccessibilityPreferences::default(),
            appearance: AppearancePreferences::default(),
        }
    }
}

impl UserPreferences {
    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let parsed = serde_json::from_str::<Self>(raw)?;
        Ok(parsed.normalized())
    }

    pub fn normalized(mut self) -> Self {
        self.version = PREFS_VERSION;
        self.audio.input_device_id = normalize_optional_id(self.audio.input_device_id);
        self.audio.output_device_id = normalize_optional_id(self.audio.output_device_id);
        self.activity.default_range = normalize_activity_range(&self.activity.default_range);
        self.vault.default_match_mode = normalize_vault_match_mode(&self.vault.default_match_mode);
        self.vault.excluded_domains = normalize_domains(self.vault.excluded_domains);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AudioPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityPreferences {
    pub default_range: String,
}

impl Default for ActivityPreferences {
    fn default() -> Self {
        Self {
            default_range: "7d".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyPreferences {
    pub personal_activity_enabled: bool,
}

impl Default for PrivacyPreferences {
    fn default() -> Self {
        Self {
            personal_activity_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultPreferences {
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

impl Default for VaultPreferences {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NotificationPreferences {
    pub desktop_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AccessibilityPreferences {
    pub reduce_motion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppearancePreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<String>,
}

fn normalize_optional_id(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_activity_range(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "7d" | "30d" | "all" => raw.trim().to_ascii_lowercase(),
        _ => "7d".to_string(),
    }
}

fn normalize_vault_match_mode(raw: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::UserPreferences;

    #[test]
    fn normalizes_legacy_preference_payloads() {
        let prefs = UserPreferences::from_json_str(
            r#"{"audio":{"input_device_id":"  mic-1 "},"activity":{"default_range":"year"},"privacy":{"personal_activity_enabled":false}}"#,
        )
        .unwrap();
        assert_eq!(prefs.version, 1);
        assert_eq!(prefs.audio.input_device_id.as_deref(), Some("mic-1"));
        assert_eq!(prefs.activity.default_range, "7d");
        assert!(!prefs.privacy.personal_activity_enabled);
    }
}
