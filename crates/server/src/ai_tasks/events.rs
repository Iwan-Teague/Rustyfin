use serde::Serialize;

use super::store::AiTaskStore;

pub async fn append_task_event<S, T>(
    store: &S,
    task_id: &str,
    event_type: &str,
    payload: &T,
) -> Result<(), String>
where
    S: AiTaskStore,
    T: Serialize + ?Sized,
{
    let payload = serde_json::to_value(payload)
        .map_err(|e| format!("failed to serialize ai task event payload: {e}"))?;
    store.append_event(task_id, event_type, payload).await?;
    Ok(())
}
