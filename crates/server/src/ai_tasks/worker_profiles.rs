use std::time::Duration;

use crate::ai_assistant::provider::ToolExecutionProfile;
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::types::ToolAccessMode;
use crate::ai_assistant::web::public_web_tools_enabled;

use super::types::TaskUserContext;

pub const MAX_WORKERS_PER_TASK: usize = 4;
pub const MAX_WORKER_DEPTH: u8 = 1;
pub const MAX_WORKER_RUNTIME_BUDGET: Duration = Duration::from_secs(90);
pub const MAX_WORKER_PROMPT_BUDGET_CHARS: usize = 8_192;
pub const DEFAULT_MAX_TOOL_CALLS_PER_WORKER: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerProfile {
    SourceScoutWorker,
    GroundingWorker,
    VerifierWorker,
}

impl WorkerProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceScoutWorker => "source_scout_worker",
            Self::GroundingWorker => "grounding_worker",
            Self::VerifierWorker => "verifier_worker",
        }
    }

    pub fn tool_profile(self, user: &TaskUserContext) -> ToolExecutionProfile {
        let mut allowed_tools = AssistantToolName::all()
            .iter()
            .copied()
            .filter(|tool| tool.spec().access_mode == ToolAccessMode::ReadOnly)
            .collect::<Vec<_>>();

        if !user.is_admin || !public_web_tools_enabled() {
            allowed_tools.retain(|tool| {
                !matches!(
                    tool,
                    AssistantToolName::WebSearchPublicWeb
                        | AssistantToolName::WebFetchPublicPageSummary
                )
            });
        }

        match self {
            Self::SourceScoutWorker => {}
            Self::GroundingWorker => {}
            Self::VerifierWorker => allowed_tools.clear(),
        }

        ToolExecutionProfile::restricted(allowed_tools, true, DEFAULT_MAX_TOOL_CALLS_PER_WORKER)
    }
}

pub fn capped_worker_count(requested: Option<u8>) -> usize {
    requested
        .map(|value| value as usize)
        .unwrap_or(3)
        .clamp(1, MAX_WORKERS_PER_TASK)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_TOOL_CALLS_PER_WORKER, WorkerProfile, capped_worker_count};
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_tasks::types::TaskUserContext;

    fn user() -> TaskUserContext {
        TaskUserContext {
            user_id: "user-1".to_string(),
            username: "tester".to_string(),
            role: "user".to_string(),
            is_admin: false,
        }
    }

    #[test]
    fn worker_profiles_enforce_tool_budget() {
        let profile = WorkerProfile::GroundingWorker.tool_profile(&user());
        assert_eq!(profile.max_tool_calls, DEFAULT_MAX_TOOL_CALLS_PER_WORKER);
    }

    #[test]
    fn worker_profiles_deny_write_tools() {
        let profile = WorkerProfile::GroundingWorker.tool_profile(&user());
        let denial = profile
            .denial_reason(
                AssistantToolName::CalendarCreateEvent,
                AssistantToolName::CalendarCreateEvent.spec(),
            )
            .expect("write tool should be denied");
        assert!(denial.contains("read-only") || denial.contains("not available"));
    }

    #[test]
    fn worker_count_is_capped() {
        assert_eq!(capped_worker_count(Some(9)), 4);
    }
}
