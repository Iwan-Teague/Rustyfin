use serde::{Deserialize, Serialize};

const PREFS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserPreferences {
    pub version: u32,
    pub audio: AudioPreferences,
    pub activity: ActivityPreferences,
    pub privacy: PrivacyPreferences,
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
