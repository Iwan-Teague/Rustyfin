use crate::error::AppError;
use crate::state::AppState;
use crate::vault::service;

pub async fn record_event(
    state: &AppState,
    user_id: &str,
    device_session_id: Option<&str>,
    event_kind: &str,
    target_item_id: Option<&str>,
    event_json: serde_json::Value,
) -> Result<(), AppError> {
    service::create_audit_event(
        state,
        user_id,
        device_session_id,
        event_kind,
        target_item_id,
        event_json,
    )
    .await
}
