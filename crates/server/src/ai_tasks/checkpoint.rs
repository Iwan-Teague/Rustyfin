use serde::Serialize;

use super::events::append_task_event;
use super::store::AiTaskStore;
use super::types::AiTaskPhase;

pub async fn write_task_checkpoint<S, T>(
    store: &S,
    task_id: &str,
    phase: AiTaskPhase,
    payload: &T,
    event_type: &str,
) -> Result<(), String>
where
    S: AiTaskStore,
    T: Serialize + ?Sized,
{
    let payload = serde_json::to_value(payload)
        .map_err(|e| format!("failed to serialize ai task checkpoint payload: {e}"))?;
    store
        .write_checkpoint(task_id, phase, payload.clone())
        .await?;
    append_task_event(store, task_id, event_type, &payload).await
}
