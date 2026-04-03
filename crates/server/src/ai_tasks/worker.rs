use serde_json::json;

use crate::state::AppState;

use super::job_types::{compact_grounding_summary, run_grounded_query};
use super::types::{TaskUserContext, WorkerFinding, WorkerResult};
use super::worker_profiles::{MAX_WORKER_DEPTH, MAX_WORKER_PROMPT_BUDGET_CHARS, WorkerProfile};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkerPlan {
    pub worker_profile: WorkerProfile,
    pub objective: String,
}

impl WorkerPlan {
    pub fn serializable(&self) -> serde_json::Value {
        json!({
            "worker_profile": self.worker_profile.as_str(),
            "objective": self.objective,
        })
    }
}

pub async fn execute_worker(
    state: &AppState,
    task_id: &str,
    user: &TaskUserContext,
    plan: &WorkerPlan,
) -> Result<WorkerResult, String> {
    if plan.objective.chars().count() > MAX_WORKER_PROMPT_BUDGET_CHARS {
        return Err("worker objective exceeded the prompt budget".to_string());
    }

    match plan.worker_profile {
        WorkerProfile::SourceScoutWorker => {
            let profile = plan.worker_profile.tool_profile(user);
            let planned_tools =
                crate::ai_assistant::plan_tool_calls_with_history(&plan.objective, &[])
                    .into_iter()
                    .filter(|call| profile.denial_reason(call.tool, call.tool.spec()).is_none())
                    .take(profile.max_tool_calls)
                    .collect::<Vec<_>>();

            Ok(WorkerResult {
                worker_profile: plan.worker_profile.as_str().to_string(),
                objective: plan.objective.clone(),
                findings: planned_tools
                    .iter()
                    .map(|call| WorkerFinding {
                        claim: format!("Suggested grounded query via {}", call.tool.as_str()),
                        evidence_refs: vec![call.tool.as_str().to_string()],
                        confidence: 0.5,
                        open_questions: Vec::new(),
                    })
                    .collect(),
                summary: planned_tools
                    .iter()
                    .map(|call| call.tool.as_str().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
        }
        WorkerProfile::GroundingWorker => {
            let profile = plan.worker_profile.tool_profile(user);
            let execution =
                run_grounded_query(state, task_id, user, &plan.objective, &[], &profile).await;
            Ok(WorkerResult {
                worker_profile: plan.worker_profile.as_str().to_string(),
                objective: plan.objective.clone(),
                findings: execution
                    .grounding_chunks
                    .iter()
                    .take(profile.max_tool_calls)
                    .map(|chunk| WorkerFinding {
                        claim: chunk.excerpt.clone(),
                        evidence_refs: vec![chunk.id.clone()],
                        confidence: chunk.score as f32,
                        open_questions: Vec::new(),
                    })
                    .collect(),
                summary: compact_grounding_summary(&execution.grounding_chunks),
            })
        }
        WorkerProfile::VerifierWorker => Ok(WorkerResult {
            worker_profile: plan.worker_profile.as_str().to_string(),
            objective: plan.objective.clone(),
            findings: Vec::new(),
            summary: format!(
                "Verifier workers run only at depth {} and do not execute grounded tools directly.",
                MAX_WORKER_DEPTH
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkerPlan, execute_worker};
    use crate::ai_tasks::types::TaskUserContext;
    use crate::ai_tasks::worker_profiles::{DEFAULT_MAX_TOOL_CALLS_PER_WORKER, WorkerProfile};
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
            cache_dir: std::env::temp_dir().join("rustyfin-ai-worker-cache"),
            watch_party_audio_dir: std::env::temp_dir().join("rustyfin-ai-worker-audio"),
            events: events_tx,
            watch_party: std::sync::Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: std::sync::Arc::new(crate::channels::manager::ChannelManager::new()),
        }
    }

    #[tokio::test]
    async fn worker_cannot_exceed_tool_budget() {
        let result = execute_worker(
            &test_state(),
            "task-1",
            &user(),
            &WorkerPlan {
                worker_profile: WorkerProfile::SourceScoutWorker,
                objective: "Weather and birthdays and network and rooms".to_string(),
            },
        )
        .await
        .expect("worker executed");
        assert!(result.findings.len() <= DEFAULT_MAX_TOOL_CALLS_PER_WORKER);
    }
}
