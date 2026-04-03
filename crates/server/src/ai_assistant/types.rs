use rustfin_ai_agent::ChatMessage;
use serde::{Deserialize, Serialize};

pub const ASSISTANT_CLARIFICATION_PREFIX: &str = "clarification:";

pub fn encode_assistant_clarification_message(message: &str) -> String {
    format!("{ASSISTANT_CLARIFICATION_PREFIX}{}", message.trim())
}

pub fn decode_assistant_clarification_message(message: &str) -> Option<&str> {
    message
        .strip_prefix(ASSISTANT_CLARIFICATION_PREFIX)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantResponseMode {
    #[default]
    Instant,
    Thinking,
    Extended,
}

impl AssistantResponseMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Thinking => "thinking",
            Self::Extended => "extended",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantChatRequest {
    pub model: String,
    pub message: String,
    #[serde(default)]
    pub response_mode: AssistantResponseMode,
    #[serde(default)]
    pub confirmation_token: Option<String>,
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
    #[serde(default)]
    pub grounding_chunks: Vec<AssistantGroundingChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssistantToolInput {
    None,
    CalendarWindow {
        from_date: String,
        to_date: String,
        label: String,
        query: Option<String>,
    },
    CalendarCreateEvent {
        scope: String,
        title: String,
        description: Option<String>,
        event_date: String,
    },
    CalendarCreateBirthday {
        scope: String,
        title: String,
        description: Option<String>,
        event_date: String,
        birthday_year: i32,
    },
    CalendarDeleteEvent {
        event_id: String,
        title: String,
        event_date: String,
        scope: String,
        event_type: String,
        recurrence: String,
    },
    DocumentCreateDownload {
        title: String,
        file_name: String,
        format: String,
        request_prompt: String,
        model_name: String,
    },
    ChannelsFilter {
        query: Option<String>,
    },
    DownloadsFilter {
        query: Option<String>,
        availability: Option<String>,
    },
    LibrarySearch {
        query: String,
    },
    LibraryRecent {
        query: Option<String>,
    },
    Weather {
        location: String,
        forecast_days: Option<u8>,
    },
    WeatherHistory {
        location: String,
        start_date: String,
        end_date: String,
        label: String,
    },
    WebSearch {
        query: String,
    },
    WebFetch {
        url: String,
    },
    CurrentDateTime {
        location: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantPlannerDebug {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub raw_response: Option<String>,
    #[serde(default)]
    pub repaired_response: Option<String>,
    #[serde(default)]
    pub validation_errors: Vec<String>,
    #[serde(default)]
    pub repair_attempt_count: u32,
    #[serde(default)]
    pub used_repaired_response: bool,
    #[serde(default)]
    pub validated_call_count: u32,
}

#[derive(Debug, Clone)]
pub struct PlannedToolSet {
    pub mode: AssistantPlannerMode,
    pub calls: Vec<PlannedToolCall>,
    pub debug: AssistantPlannerDebug,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantArtifactVerificationDebug {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub revision_count: u32,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccessMode {
    ReadOnly,
    Write,
    DestructiveWrite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantGroundingSource {
    pub tool: String,
    pub label: String,
    pub access_mode: ToolAccessMode,
    pub risk_tier: ToolRiskTier,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_size_bytes: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantGroundingVisibility {
    User,
    Shared,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantGroundingCitation {
    pub citation_id: String,
    pub source_kind: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ts_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ts_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantGroundingChunk {
    pub id: String,
    pub source_kind: String,
    pub title: String,
    pub excerpt: String,
    pub score: f64,
    pub visibility: AssistantGroundingVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<AssistantGroundingCitation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantTurnStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_duration_ms: u64,
    pub generation_duration_ms: u64,
    pub planner_duration_ms: u64,
    pub tool_duration_ms: u64,
    pub end_to_end_duration_ms: u64,
    pub queue_duration_ms: u64,
    pub model_load_duration_ms: u64,
    pub tokens_per_second: f64,
    #[serde(default)]
    pub context_length_tokens: u32,
    #[serde(default)]
    pub prompt_budget_tokens: u32,
    #[serde(default)]
    pub reserved_completion_tokens: u32,
    #[serde(default)]
    pub completion_budget_tokens: u32,
    #[serde(default)]
    pub loaded_history_turns: u32,
    #[serde(default)]
    pub retained_raw_turns: u32,
    #[serde(default)]
    pub summarized_turns: u32,
    #[serde(default)]
    pub recent_grounded_context_count: u32,
    #[serde(default)]
    pub memory_turn_index: i64,
    #[serde(default)]
    pub compact_boundary_count: u32,
    #[serde(default)]
    pub planner_validation_error_count: u32,
    #[serde(default)]
    pub planner_repair_count: u32,
    #[serde(default)]
    pub journal_persisted: bool,
    #[serde(default)]
    pub overload: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overload_reason: Option<String>,
    #[serde(default)]
    pub artifact_verification_attempts: u32,
    #[serde(default)]
    pub artifact_revision_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantFollowUpEntity {
    pub ordinal: usize,
    pub label: String,
    pub identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chunk_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantFollowUpInputHint {
    pub calendar_label: Option<String>,
    pub calendar_from_date: Option<String>,
    pub calendar_to_date: Option<String>,
    pub calendar_query: Option<String>,
    pub channels_query: Option<String>,
    pub downloads_query: Option<String>,
    pub downloads_availability: Option<String>,
    pub room_mode: Option<String>,
    pub room_query: Option<String>,
    pub server_query: Option<String>,
    pub server_availability: Option<String>,
    pub library_query: Option<String>,
    pub weather_location: Option<String>,
    pub weather_days: Option<u8>,
    pub weather_start_date: Option<String>,
    pub weather_end_date: Option<String>,
    pub weather_label: Option<String>,
    pub current_datetime_location: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPendingActionKind {
    CalendarCreateEvent,
    CalendarCreateBirthday,
    CalendarDeleteEvent,
    DocumentCreateDownload,
}

impl AssistantPendingActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CalendarCreateEvent => "calendar_create_event",
            Self::CalendarCreateBirthday => "calendar_create_birthday",
            Self::CalendarDeleteEvent => "calendar_delete_event",
            Self::DocumentCreateDownload => "document_create_download",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPendingActionStatus {
    Pending,
    Confirmed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantPendingAction {
    pub token: String,
    pub action_kind: AssistantPendingActionKind,
    pub summary: String,
    pub expires_ts: i64,
    pub status: AssistantPendingActionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantConfirmationPayload {
    pub action_kind: AssistantPendingActionKind,
    pub call: PlannedToolCall,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantConfirmationRequiredEvent {
    pub token: String,
    pub action_kind: AssistantPendingActionKind,
    pub summary: String,
    pub expires_ts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRuntimePhase {
    Idle,
    LoadingModel,
    Planning,
    Grounding,
    Generating,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Planning,
    Generating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantPhaseEvent {
    pub phase: AssistantPhase,
    pub label: String,
    pub started_ts_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ts_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantToolActivityState {
    Running,
    Complete,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantToolActivityEvent {
    pub id: String,
    pub tool: String,
    pub label: String,
    pub state: AssistantToolActivityState,
    pub started_ts_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ts_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssistantActivityTraceItem {
    Phase {
        phase: AssistantPhase,
        label: String,
        started_ts_ms: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        finished_ts_ms: Option<i64>,
    },
    Tool {
        id: String,
        tool: String,
        label: String,
        state: AssistantToolActivityState,
        started_ts_ms: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        finished_ts_ms: Option<i64>,
    },
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
