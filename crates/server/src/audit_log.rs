use serde_json::Value;

use crate::state::AppState;

pub async fn record_event(state: &AppState, kind: &str, payload: Value) {
    let payload_json = serde_json::to_string(&payload).ok();
    let Ok(job) =
        rustfin_db::repo::jobs::create_job(&state.db, kind, payload_json.as_deref()).await
    else {
        return;
    };
    let _ =
        rustfin_db::repo::jobs::update_job_status(&state.db, &job.id, "completed", 1.0, None).await;
}
