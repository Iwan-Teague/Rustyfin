use rustfin_ai_agent::ChatMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantChatRequest {
    pub model: String,
    pub message: String,
    #[serde(default)]
    pub history: Vec<AssistantHistoryMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantHistoryMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub grounding_tools: Vec<String>,
    #[serde(default)]
    pub follow_up_contexts: Vec<AssistantFollowUpContext>,
}

#[derive(Debug, Clone)]
pub enum AssistantToolInput {
    None,
    CalendarWindow {
        from_date: String,
        to_date: String,
        label: String,
    },
    DownloadsFilter {
        query: Option<String>,
        availability: Option<String>,
    },
    LibrarySearch {
        query: String,
    },
    WebSearch {
        query: String,
    },
    WebFetch {
        url: String,
    },
    RoomsFilter {
        room_mode: Option<String>,
        query: Option<String>,
    },
    ServerFilter {
        query: Option<String>,
        availability: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct PlannedToolCall {
    pub tool: super::registry::AssistantToolName,
    pub input: AssistantToolInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantPlannerMode {
    ModelStructured,
    DeterministicFallback,
    DeterministicEntityFollowUp,
}

impl AssistantPlannerMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelStructured => "model_structured",
            Self::DeterministicFallback => "deterministic_fallback",
            Self::DeterministicEntityFollowUp => "deterministic_entity_follow_up",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlannedToolSet {
    pub mode: AssistantPlannerMode,
    pub calls: Vec<PlannedToolCall>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccessMode {
    ReadOnly,
    Write,
    DestructiveWrite,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskTier {
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRoleRequirement {
    AnyAuthenticatedUser,
    AdminOnly,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConfirmationPolicy {
    None,
    ExplicitUserConfirm,
    ProtectedAction,
}

#[derive(Debug, Clone, Copy)]
pub struct AssistantToolSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub access_mode: ToolAccessMode,
    pub risk_tier: ToolRiskTier,
    pub required_role: ToolRoleRequirement,
    pub confirmation: ToolConfirmationPolicy,
    pub timeout_ms: u64,
    pub max_result_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantGroundingSource {
    pub tool: &'static str,
    pub label: String,
    pub access_mode: ToolAccessMode,
    pub risk_tier: ToolRiskTier,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantFollowUpEntity {
    pub ordinal: usize,
    pub label: String,
    pub identifier: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantFollowUpInputHint {
    pub calendar_label: Option<String>,
    pub calendar_from_date: Option<String>,
    pub calendar_to_date: Option<String>,
    pub downloads_query: Option<String>,
    pub downloads_availability: Option<String>,
    pub room_mode: Option<String>,
    pub room_query: Option<String>,
    pub server_query: Option<String>,
    pub server_availability: Option<String>,
    pub library_query: Option<String>,
    pub web_search_query: Option<String>,
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantFollowUpContext {
    pub tool: String,
    pub label: String,
    #[serde(default)]
    pub input_hint: AssistantFollowUpInputHint,
    #[serde(default)]
    pub entities: Vec<AssistantFollowUpEntity>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantStatusKind {
    Checking,
    Complete,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantStatusEvent {
    pub tool: &'static str,
    pub label: String,
    pub kind: AssistantStatusKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantToolContextBlock {
    pub tool: &'static str,
    pub label: String,
    pub status: &'static str,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct PreparedAssistantTurn {
    pub messages: Vec<ChatMessage>,
    pub sources: Vec<AssistantGroundingSource>,
    pub immediate_response: Option<String>,
}
