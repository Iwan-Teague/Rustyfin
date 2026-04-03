use serde_json::json;

use crate::ai_assistant::provider::ToolExecutionProfile;
use crate::ai_eval_harness;
use crate::ai_tasks::checkpoint::write_task_checkpoint;
use crate::ai_tasks::coordinator::execute_deep_research_task;
use crate::ai_tasks::events::append_task_event;
use crate::ai_tasks::job_types::{
    compact_grounding_summary, default_file_name, effective_models_for_task,
    persist_task_artifact_file, render_document, run_grounded_query, verify_document,
};
use crate::ai_tasks::store::{AiTaskStore, DbAiTaskStore};
use crate::ai_tasks::types::{
    AiTaskArtifactFormat, AiTaskInput, AiTaskPhase, AiTaskStatus, AiTaskType,
};
use crate::state::AppState;

pub async fn run_task(state: AppState, task_id: String) -> Result<(), String> {
    let store = DbAiTaskStore::new(state.db.clone());
    run_task_with_store(state, store, &task_id).await
}

pub async fn run_task_with_store<S: AiTaskStore>(
    state: AppState,
    store: S,
    task_id: &str,
) -> Result<(), String> {
    let Some(mut task) = store.get_task(task_id).await? else {
        return Ok(());
    };
    let Some(user) = store.get_task_user_context(task_id).await? else {
        return Err(format!("ai task {task_id} is missing its owner context"));
    };

    if task.status == AiTaskStatus::Queued {
        task = store
            .transition_status(
                task_id,
                &[AiTaskStatus::Queued],
                AiTaskStatus::Running,
                AiTaskPhase::Planning,
                None,
                None,
                true,
            )
            .await?
            .ok_or_else(|| format!("ai task {task_id} could not enter running state"))?;
        append_task_event(
            &store,
            task_id,
            "task_started",
            &json!({ "status": task.status, "phase": task.phase }),
        )
        .await?;
    } else if matches!(
        task.status,
        AiTaskStatus::Completed | AiTaskStatus::Failed | AiTaskStatus::Cancelled
    ) {
        return Ok(());
    }

    let (effective_answer_model, effective_planner_model) =
        effective_models_for_task(&state, task.requested_model.as_deref()).await;
    store
        .update_progress(
            task_id,
            task.phase,
            task.progress_pct,
            effective_answer_model.as_deref(),
            effective_planner_model.as_deref(),
        )
        .await?;

    ensure_not_cancelled(&store, task_id).await?;

    let input: AiTaskInput = serde_json::from_value(task.input_json.clone())
        .map_err(|e| format!("failed to parse ai task input: {e}"))?;
    match input.task_type() {
        AiTaskType::GroundedDocumentGeneration => {
            run_grounded_document_task(
                &state,
                &store,
                task_id,
                &user,
                input,
                effective_answer_model.as_deref(),
                effective_planner_model.as_deref(),
            )
            .await
        }
        AiTaskType::DeepResearchReport => {
            execute_deep_research_task(
                &state,
                &store,
                task_id,
                &user,
                match &input {
                    AiTaskInput::DeepResearchReport { objective, .. } => objective,
                    _ => "",
                },
                match &input {
                    AiTaskInput::DeepResearchReport { max_workers, .. } => *max_workers,
                    _ => None,
                },
                effective_answer_model.as_deref(),
                effective_planner_model.as_deref(),
            )
            .await
        }
        AiTaskType::AiEvalRun => {
            run_eval_task(
                &state,
                &store,
                task_id,
                &user,
                input,
                effective_answer_model.as_deref(),
                effective_planner_model.as_deref(),
            )
            .await
        }
    }
}

async fn ensure_not_cancelled<S: AiTaskStore>(store: &S, task_id: &str) -> Result<(), String> {
    let Some(task) = store.get_task(task_id).await? else {
        return Ok(());
    };
    if !task.cancel_requested {
        return Ok(());
    }

    let expected = match task.status {
        AiTaskStatus::Queued => vec![AiTaskStatus::Queued],
        AiTaskStatus::Running => vec![AiTaskStatus::Running],
        AiTaskStatus::Verifying => vec![AiTaskStatus::Verifying],
        AiTaskStatus::Completed | AiTaskStatus::Failed | AiTaskStatus::Cancelled => return Ok(()),
    };

    let cancelled = store
        .transition_status(
            task_id,
            &expected,
            AiTaskStatus::Cancelled,
            AiTaskPhase::Cancelled,
            None,
            None,
            true,
        )
        .await?;
    if cancelled.is_some() {
        append_task_event(
            store,
            task_id,
            "task_cancelled",
            &json!({ "reason": "cancel_requested" }),
        )
        .await?;
    }
    Err("ai task cancelled".to_string())
}

async fn run_grounded_document_task<S: AiTaskStore>(
    state: &AppState,
    store: &S,
    task_id: &str,
    user: &crate::ai_tasks::types::TaskUserContext,
    input: AiTaskInput,
    effective_answer_model: Option<&str>,
    effective_planner_model: Option<&str>,
) -> Result<(), String> {
    let AiTaskInput::GroundedDocumentGeneration {
        prompt,
        title,
        format,
    } = input
    else {
        return Err("invalid grounded_document_generation input".to_string());
    };
    let format = format.unwrap_or(AiTaskArtifactFormat::Markdown);
    let title = title.unwrap_or_else(|| "Grounded Rustyfin Document".to_string());

    let planned_tools = crate::ai_assistant::plan_tool_calls_with_history(&prompt, &[]);
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Planning,
        &json!({
            "prompt": prompt,
            "planned_tools": planned_tools.iter().map(|call| call.tool.as_str()).collect::<Vec<_>>(),
        }),
        "planner_result",
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

    ensure_not_cancelled(store, task_id).await?;

    let profile = ToolExecutionProfile::restricted(
        crate::ai_assistant::registry::AssistantToolName::all()
            .iter()
            .copied(),
        true,
        3,
    );
    let execution = run_grounded_query(state, task_id, user, &prompt, &[], &profile).await;

    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Grounding,
        &json!({
            "planned_tools": execution.planned_tools.iter().map(|call| call.tool.as_str()).collect::<Vec<_>>(),
            "grounding_sources": execution.grounding_sources,
            "grounding_summary": compact_grounding_summary(&execution.grounding_chunks),
        }),
        "grounding_complete",
    )
    .await?;
    store
        .update_progress(
            task_id,
            AiTaskPhase::Grounding,
            45.0,
            effective_answer_model,
            effective_planner_model,
        )
        .await?;

    ensure_not_cancelled(store, task_id).await?;

    let draft = render_document(&title, &prompt, format, &execution.grounding_chunks);
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Drafting,
        &json!({ "title": title, "draft_preview": draft }),
        "draft_generated",
    )
    .await?;
    store
        .update_progress(
            task_id,
            AiTaskPhase::Drafting,
            70.0,
            effective_answer_model,
            effective_planner_model,
        )
        .await?;

    ensure_not_cancelled(store, task_id).await?;

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
    let (verified_content, issues) =
        verify_document(&title, &prompt, format, &draft, &execution.grounding_chunks);
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Verifying,
        &json!({ "issues": issues, "verified_preview": verified_content }),
        "verification_complete",
    )
    .await?;
    store
        .update_progress(
            task_id,
            AiTaskPhase::Verifying,
            85.0,
            effective_answer_model,
            effective_planner_model,
        )
        .await?;

    ensure_not_cancelled(store, task_id).await?;

    let file_name = default_file_name(&title, format);
    let (path, size_bytes) =
        persist_task_artifact_file(state, task_id, &file_name, &verified_content)?;
    let artifact = store
        .create_artifact(
            task_id,
            &user.user_id,
            "document",
            &file_name,
            format.media_type(),
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
                "file_name": artifact.file_name,
                "issue_count": issues.len(),
                "grounding_chunk_count": execution.grounding_chunks.len(),
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

async fn run_eval_task<S: AiTaskStore>(
    state: &AppState,
    store: &S,
    task_id: &str,
    user: &crate::ai_tasks::types::TaskUserContext,
    input: AiTaskInput,
    effective_answer_model: Option<&str>,
    effective_planner_model: Option<&str>,
) -> Result<(), String> {
    let AiTaskInput::AiEvalRun { suite } = input else {
        return Err("invalid ai_eval_run input".to_string());
    };
    let suite = suite.unwrap_or_else(|| "default".to_string());
    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::Planning,
        &json!({ "suite": suite }),
        "eval_plan_created",
    )
    .await?;
    store
        .update_progress(
            task_id,
            AiTaskPhase::Planning,
            25.0,
            effective_answer_model,
            effective_planner_model,
        )
        .await?;

    ensure_not_cancelled(store, task_id).await?;

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

    let fixtures_dir = ai_eval_harness::corpus::fixtures_dir();
    let report = Box::pin(ai_eval_harness::run_mode_for_task(&suite, &fixtures_dir))
        .await
        .map_err(|e| format!("failed to run AI eval harness: {e}"))?;
    let markdown_report = ai_eval_harness::report::render_markdown_report(&report);
    let file_name = "ai-eval-run.md".to_string();
    let (path, size_bytes) =
        persist_task_artifact_file(state, task_id, &file_name, &markdown_report)?;
    let summary_artifact = store
        .create_artifact(
            task_id,
            &user.user_id,
            "eval_report",
            &file_name,
            AiTaskArtifactFormat::Markdown.media_type(),
            &path.to_string_lossy(),
            size_bytes,
        )
        .await?;

    let json_file_name = "ai-eval-run.json".to_string();
    let json_path = state
        .cache_dir
        .join("ai_tasks")
        .join(task_id)
        .join(&json_file_name);
    ai_eval_harness::write_report_json(&json_path, &report)
        .map_err(|e| format!("failed to persist AI eval JSON report: {e}"))?;
    let json_metadata = tokio::fs::metadata(&json_path)
        .await
        .map_err(|e| format!("failed to read AI eval JSON report metadata: {e}"))?;
    let json_artifact = store
        .create_artifact(
            task_id,
            &user.user_id,
            "eval_report_json",
            &json_file_name,
            "application/json; charset=utf-8",
            &json_path.to_string_lossy(),
            json_metadata.len() as i64,
        )
        .await?;

    write_task_checkpoint(
        store,
        task_id,
        AiTaskPhase::PersistingArtifacts,
        &json!({
            "artifact_ids": [summary_artifact.id.clone(), json_artifact.id.clone()],
            "file_names": [summary_artifact.file_name.clone(), json_artifact.file_name.clone()],
            "overall_pass": report.overall_pass,
        }),
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
                "suite": suite,
                "overall_pass": report.overall_pass,
                "summary_artifact_id": summary_artifact.id,
                "json_artifact_id": json_artifact.id,
                "suite_count": report.suites.len(),
            })),
            None,
            true,
        )
        .await?;
    append_task_event(
        store,
        task_id,
        "task_completed",
        &json!({
            "artifact_ids": [summary_artifact.id, json_artifact.id],
            "file_names": [file_name, json_file_name],
            "overall_pass": report.overall_pass,
        }),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run_task_with_store;
    use crate::ai_tasks::store::{AiTaskStore, MemoryAiTaskStore};
    use crate::ai_tasks::types::{AiTaskInput, AiTaskStatus, CreateAiTaskRequest, TaskUserContext};
    use crate::state::AppState;

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
        let tc_config = rustfin_transcoder::TranscoderConfig {
            transcode_dir: std::env::temp_dir().join(format!(
                "rustyfin-ai-task-executor-{}",
                uuid::Uuid::new_v4()
            )),
            max_concurrent: 1,
            ..Default::default()
        };
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
            cache_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-task-cache-{}", uuid::Uuid::new_v4())),
            watch_party_audio_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-task-audio-{}", uuid::Uuid::new_v4())),
            events: events_tx,
            watch_party: std::sync::Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: std::sync::Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    #[tokio::test]
    async fn restart_simulation_uses_persisted_checkpointed_task_state() {
        let store = MemoryAiTaskStore::default();
        let state = test_state();
        let created = store
            .create_task(
                &user(),
                &CreateAiTaskRequest {
                    requested_model: None,
                    input: AiTaskInput::AiEvalRun { suite: None },
                },
            )
            .await
            .expect("task created");

        run_task_with_store(state.clone(), store.clone(), &created.id)
            .await
            .expect("task run");

        let completed = store
            .get_task(&created.id)
            .await
            .expect("task load")
            .expect("task exists");
        assert_eq!(completed.status, AiTaskStatus::Completed);
        assert!(completed.last_checkpoint_json.is_some());

        run_task_with_store(state, store.clone(), &created.id)
            .await
            .expect("rerun should be a no-op");
        let checkpoints = store
            .list_checkpoints(&created.id)
            .await
            .expect("checkpoints listed");
        assert!(!checkpoints.is_empty());
    }
}
