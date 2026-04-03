use serde_json::json;

use crate::state::AppState;

use super::checkpoint::write_task_checkpoint;
use super::events::append_task_event;
use super::job_types::{
    default_file_name, persist_task_artifact_file, split_objective_into_questions,
};
use super::research_merge::merge_worker_results;
use super::research_verify::verify_research_report;
use super::store::AiTaskStore;
use super::types::{AiTaskArtifactFormat, AiTaskPhase, AiTaskStatus, TaskUserContext};
use super::worker::{WorkerPlan, execute_worker};
use super::worker_profiles::{MAX_WORKERS_PER_TASK, WorkerProfile, capped_worker_count};

#[allow(clippy::too_many_arguments)]
pub async fn execute_deep_research_task<S: AiTaskStore>(
    state: &AppState,
    store: &S,
    task_id: &str,
    user: &TaskUserContext,
    objective: &str,
    max_workers: Option<u8>,
    effective_answer_model: Option<&str>,
    effective_planner_model: Option<&str>,
) -> Result<(), String> {
    let plan = build_research_plan(objective, max_workers);

    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Planning,
        &json!({
            "objective": objective,
            "worker_plan": plan.iter().map(WorkerPlan::serializable).collect::<Vec<_>>(),
        }),
        "research_plan_created",
    )
    .await?;
    store
        .update_progress(
            task_id,
            AiTaskPhase::Planning,
            10.0,
            effective_answer_model,
            effective_planner_model,
        )
        .await?;

    let worker_plan_json = plan
        .iter()
        .map(WorkerPlan::serializable)
        .collect::<Vec<_>>();
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::WorkerFanout,
        &json!({ "workers": worker_plan_json }),
        "worker_plan_created",
    )
    .await?;

    let mut worker_results = Vec::new();
    for (index, worker_plan) in plan.iter().enumerate() {
        if let Some(task) = store.get_task(task_id).await? {
            if task.cancel_requested {
                store
                    .transition_status(
                        task_id,
                        &[AiTaskStatus::Running, AiTaskStatus::Verifying],
                        AiTaskStatus::Cancelled,
                        AiTaskPhase::Cancelled,
                        None,
                        None,
                        true,
                    )
                    .await?;
                append_task_event(
                    store,
                    task_id,
                    "task_cancelled",
                    &json!({ "worker_index": index + 1 }),
                )
                .await?;
                return Err("ai task cancelled".to_string());
            }
        }

        let worker_result = execute_worker(state, task_id, user, worker_plan).await?;
        write_task_checkpoint(
            store,
            task_id,
            AiTaskPhase::WorkerFanout,
            &json!({ "worker_index": index + 1, "result": worker_result }),
            "worker_result_recorded",
        )
        .await?;
        worker_results.push(worker_result);
        let progress = 20.0 + ((index + 1) as f64 / plan.len().max(1) as f64) * 45.0;
        store
            .update_progress(
                task_id,
                AiTaskPhase::WorkerFanout,
                progress,
                effective_answer_model,
                effective_planner_model,
            )
            .await?;
    }

    let merged_report = merge_worker_results(objective, &worker_results);
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Merging,
        &json!({ "worker_count": worker_results.len(), "merged_report": merged_report }),
        "research_merge_complete",
    )
    .await?;

    store
        .transition_status(
            task_id,
            &[AiTaskStatus::Running],
            AiTaskStatus::Verifying,
            AiTaskPhase::Verifying,
            None,
            None,
            false,
        )
        .await?;

    let verification = verify_research_report(&merged_report, &worker_results);
    let final_report = verification.revised_report.clone().unwrap_or(merged_report);
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Verifying,
        &json!({ "issues": verification.issues, "final_report": final_report }),
        "research_verification_complete",
    )
    .await?;

    let file_name = default_file_name("deep-research-report", AiTaskArtifactFormat::Markdown);
    let (path, size_bytes) = persist_task_artifact_file(state, task_id, &file_name, &final_report)?;
    let artifact = store
        .create_artifact(
            task_id,
            &user.user_id,
            "report",
            &file_name,
            AiTaskArtifactFormat::Markdown.media_type(),
            &path.to_string_lossy(),
            size_bytes,
        )
        .await?;
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::PersistingArtifacts,
        &json!({ "artifact_id": artifact.id, "file_name": artifact.file_name }),
        "artifact_persisted",
    )
    .await?;

    store
        .transition_status(
            task_id,
            &[AiTaskStatus::Verifying],
            AiTaskStatus::Completed,
            AiTaskPhase::Completed,
            Some(json!({
                "artifact_id": artifact.id,
                "worker_count": worker_results.len(),
                "verifier_issue_count": verification.issues.len(),
            })),
            None,
            true,
        )
        .await?;
    append_task_event(
        store,
        task_id,
        "task_completed",
        &json!({ "artifact_id": artifact.id, "file_name": artifact.file_name }),
    )
    .await?;
    Ok(())
}

pub fn build_research_plan(objective: &str, max_workers: Option<u8>) -> Vec<WorkerPlan> {
    let capped = capped_worker_count(max_workers).min(MAX_WORKERS_PER_TASK);
    let mut questions = split_objective_into_questions(objective, capped.saturating_sub(1).max(1));
    if questions.is_empty() {
        questions.push(objective.trim().to_string());
    }

    let mut plan = Vec::new();
    if questions.len() < capped {
        plan.push(WorkerPlan {
            worker_profile: WorkerProfile::SourceScoutWorker,
            objective: objective.to_string(),
        });
    }
    for question in questions
        .into_iter()
        .take(capped.saturating_sub(plan.len()))
    {
        plan.push(WorkerPlan {
            worker_profile: WorkerProfile::GroundingWorker,
            objective: question,
        });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::{build_research_plan, execute_deep_research_task};
    use crate::ai_tasks::store::{AiTaskStore, MemoryAiTaskStore};
    use crate::ai_tasks::types::{AiTaskInput, AiTaskStatus, CreateAiTaskRequest, TaskUserContext};
    use crate::state::AppState;

    #[test]
    fn build_research_plan_stays_within_worker_limit() {
        let plan = build_research_plan("Weather and rooms and storage and servers", Some(8));
        assert!(plan.len() <= 4);
    }

    fn user() -> TaskUserContext {
        TaskUserContext {
            user_id: "user-1".to_string(),
            username: "tester".to_string(),
            role: "user".to_string(),
            is_admin: false,
        }
    }

    fn test_state() -> AppState {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig::default();
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder =
            std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(8);

        AppState {
            db: pool,
            rustyvault: crate::state::RustyVaultRuntimeState::available(),
            jwt_secret: "test-secret".to_string(),
            http: reqwest::Client::builder().build().unwrap(),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url: "http://127.0.0.1:8102".to_string(),
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            model_dir: std::sync::Arc::new(tokio::sync::RwLock::new(std::env::temp_dir())),
            engine: std::sync::Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: std::env::temp_dir().join("rustyfin-ai-coordinator-cache"),
            watch_party_audio_dir: std::env::temp_dir().join("rustyfin-ai-coordinator-audio"),
            events: events_tx,
            watch_party: std::sync::Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: std::sync::Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    #[tokio::test]
    async fn cancelled_coordinator_task_leaves_consistent_checkpoints() {
        let state = test_state();
        let store = MemoryAiTaskStore::default();
        let task = store
            .create_task(
                &user(),
                &CreateAiTaskRequest {
                    requested_model: None,
                    input: AiTaskInput::DeepResearchReport {
                        objective: "".to_string(),
                        max_workers: Some(2),
                    },
                },
            )
            .await
            .expect("task created");
        store
            .transition_status(
                &task.id,
                &[AiTaskStatus::Queued],
                AiTaskStatus::Running,
                crate::ai_tasks::types::AiTaskPhase::Planning,
                None,
                None,
                true,
            )
            .await
            .expect("transition ok");
        store
            .request_cancel(&task.id, &user().user_id)
            .await
            .expect("cancel requested");

        let result =
            execute_deep_research_task(&state, &store, &task.id, &user(), "", Some(2), None, None)
                .await;
        assert!(result.is_err());

        let updated = store
            .get_task(&task.id)
            .await
            .expect("load task")
            .expect("task exists");
        assert_eq!(updated.status, AiTaskStatus::Cancelled);
        let checkpoints = store
            .list_checkpoints(&task.id)
            .await
            .expect("list checkpoints");
        assert!(!checkpoints.is_empty());
    }
}
