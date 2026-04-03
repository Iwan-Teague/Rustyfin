use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskType {
    DeepResearchReport,
    GroundedDocumentGeneration,
    AiEvalRun,
}

impl AiTaskType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeepResearchReport => "deep_research_report",
            Self::GroundedDocumentGeneration => "grounded_document_generation",
            Self::AiEvalRun => "ai_eval_run",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "deep_research_report" => Some(Self::DeepResearchReport),
            "grounded_document_generation" => Some(Self::GroundedDocumentGeneration),
            "ai_eval_run" => Some(Self::AiEvalRun),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskStatus {
    Queued,
    Running,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl AiTaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "verifying" => Some(Self::Verifying),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskPhase {
    Queued,
    Planning,
    WorkerFanout,
    Grounding,
    Drafting,
    Merging,
    Verifying,
    PersistingArtifacts,
    Completed,
    Failed,
    Cancelled,
}

impl AiTaskPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Planning => "planning",
            Self::WorkerFanout => "worker_fanout",
            Self::Grounding => "grounding",
            Self::Drafting => "drafting",
            Self::Merging => "merging",
            Self::Verifying => "verifying",
            Self::PersistingArtifacts => "persisting_artifacts",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "planning" => Some(Self::Planning),
            "worker_fanout" => Some(Self::WorkerFanout),
            "grounding" => Some(Self::Grounding),
            "drafting" => Some(Self::Drafting),
            "merging" => Some(Self::Merging),
            "verifying" => Some(Self::Verifying),
            "persisting_artifacts" => Some(Self::PersistingArtifacts),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

pub fn valid_status_transition(from: AiTaskStatus, to: AiTaskStatus) -> bool {
    matches!(
        (from, to),
        (AiTaskStatus::Queued, AiTaskStatus::Running)
            | (AiTaskStatus::Running, AiTaskStatus::Verifying)
            | (AiTaskStatus::Verifying, AiTaskStatus::Completed)
            | (AiTaskStatus::Queued, AiTaskStatus::Cancelled)
            | (AiTaskStatus::Running, AiTaskStatus::Cancelled)
            | (AiTaskStatus::Running, AiTaskStatus::Failed)
            | (AiTaskStatus::Verifying, AiTaskStatus::Failed)
            | (AiTaskStatus::Failed, AiTaskStatus::Queued)
            | (AiTaskStatus::Cancelled, AiTaskStatus::Queued)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskArtifactFormat {
    Markdown,
    Text,
}

impl AiTaskArtifactFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }

    pub const fn file_extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown; charset=utf-8",
            Self::Text => "text/plain; charset=utf-8",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "task_type", rename_all = "snake_case")]
pub enum AiTaskInput {
    DeepResearchReport {
        objective: String,
        #[serde(default)]
        max_workers: Option<u8>,
    },
    GroundedDocumentGeneration {
        prompt: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        format: Option<AiTaskArtifactFormat>,
    },
    AiEvalRun {
        #[serde(default)]
        suite: Option<String>,
    },
}

impl AiTaskInput {
    pub const fn task_type(&self) -> AiTaskType {
        match self {
            Self::DeepResearchReport { .. } => AiTaskType::DeepResearchReport,
            Self::GroundedDocumentGeneration { .. } => AiTaskType::GroundedDocumentGeneration,
            Self::AiEvalRun { .. } => AiTaskType::AiEvalRun,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAiTaskRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(flatten)]
    pub input: AiTaskInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUserContext {
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub is_admin: bool,
}

impl From<&AuthUser> for TaskUserContext {
    fn from(value: &AuthUser) -> Self {
        Self {
            user_id: value.user_id.clone(),
            username: value.username.clone(),
            role: value.role.clone(),
            is_admin: value.role == "admin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskRecord {
    pub id: String,
    pub owner_user_id: String,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub task_type: AiTaskType,
    pub status: AiTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_answer_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_planner_model: Option<String>,
    pub input_json: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_json: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_json: Option<serde_json::Value>,
    pub progress_pct: f64,
    pub phase: AiTaskPhase,
    pub cancel_requested: bool,
    pub checkpoint_version: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checkpoint_json: Option<serde_json::Value>,
    #[serde(default)]
    pub artifacts: Vec<AiTaskArtifactRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskEventRecord {
    pub id: i64,
    pub task_id: String,
    pub created_ts: i64,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskCheckpointRecord {
    pub id: i64,
    pub task_id: String,
    pub version: i32,
    pub phase: AiTaskPhase,
    pub payload: serde_json::Value,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskArtifactRecord {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub file_name: String,
    pub media_type: String,
    pub storage_path: String,
    pub size_bytes: i64,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskListResponse {
    pub tasks: Vec<AiTaskRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTaskEventsResponse {
    pub task_id: String,
    pub events: Vec<AiTaskEventRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiTaskEventsQuery {
    #[serde(default)]
    pub after_id: Option<i64>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFinding {
    pub claim: String,
    pub evidence_refs: Vec<String>,
    pub confidence: f32,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub worker_profile: String,
    pub objective: String,
    pub findings: Vec<WorkerFinding>,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use super::{AiTaskStatus, valid_status_transition};

    #[test]
    fn task_status_transitions_match_the_allowed_graph() {
        assert!(valid_status_transition(
            AiTaskStatus::Queued,
            AiTaskStatus::Running
        ));
        assert!(valid_status_transition(
            AiTaskStatus::Running,
            AiTaskStatus::Verifying
        ));
        assert!(valid_status_transition(
            AiTaskStatus::Failed,
            AiTaskStatus::Queued
        ));
        assert!(!valid_status_transition(
            AiTaskStatus::Queued,
            AiTaskStatus::Completed
        ));
        assert!(!valid_status_transition(
            AiTaskStatus::Cancelled,
            AiTaskStatus::Completed
        ));
    }
}
