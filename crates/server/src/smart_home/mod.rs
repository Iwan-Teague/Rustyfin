use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartDeviceType {
    Camera,
    Light,
    DoorLock,
    Alarm,
    Generic,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartDevice {
    pub id: String,
    pub name: String,
    pub device_type: SmartDeviceType,
    pub room: Option<String>,
    pub status: String, // "online", "unavailable", "locked", "on", "off"
    pub battery_level: Option<u8>,
    pub last_seen_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartHomeSummary {
    pub available: bool,
    pub provider: Option<String>,
    pub devices: Vec<SmartDevice>,
}

pub async fn get_smart_home_state(
    _auth: AuthUser,
    State(_state): State<AppState>,
) -> Result<Json<SmartHomeSummary>, AppError> {
    // Check if Home Assistant is configured (e.g. via env var or settings)
    let ha_url = std::env::var("RUSTFIN_SMART_HOME_URL").ok();

    if ha_url.is_none() {
        return Ok(Json(SmartHomeSummary {
            available: false,
            provider: None,
            devices: Vec::new(),
        }));
    }

    // In a real implementation, we would fetch from Home Assistant here.
    // For MVP/placeholder, we'll return "available" but empty, or mock data if needed.
    // Given I cannot connect to a real HA instance, I will return a stub that indicates
    // the feature is technically enabled but has no devices yet, or unavailable.

    Ok(Json(SmartHomeSummary {
        available: true,
        provider: Some("Home Assistant".to_string()),
        devices: Vec::new(),
    }))
}
