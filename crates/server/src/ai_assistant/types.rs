use std::collections::BTreeMap;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_response_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub execution: PlannerExecutionStats,
    #[serde(default)]
    pub repair_records: Vec<PlannerRepairRecord>,
    #[serde(default)]
    pub final_selected_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_trace: Option<AssistantExecutionTrace>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantDomainFamily {
    #[default]
    System,
    Account,
    Calendar,
    Channels,
    Transcript,
    Downloads,
    Library,
    Network,
    Weather,
    Web,
    Rooms,
    AiRuntime,
    Servers,
    Documents,
}

impl AssistantDomainFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Calendar => "calendar",
            Self::Channels => "channels",
            Self::Transcript => "transcript",
            Self::Downloads => "downloads",
            Self::Library => "library",
            Self::Network => "network",
            Self::Weather => "weather",
            Self::Web => "web",
            Self::Rooms => "rooms",
            Self::System => "system",
            Self::AiRuntime => "ai_runtime",
            Self::Servers => "servers",
            Self::Documents => "documents",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantToolOutcomeKind {
    #[default]
    Answer,
    Partial,
    Empty,
    Ambiguous,
    ClarificationNeeded,
    NotFound,
    WeakMatch,
    ValidationFailed,
    Stale,
    Conflicting,
    Denied,
    TransientError,
    FatalError,
}

impl AssistantToolOutcomeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Partial => "partial",
            Self::Empty => "empty",
            Self::Ambiguous => "ambiguous",
            Self::ClarificationNeeded => "clarification_needed",
            Self::NotFound => "not_found",
            Self::WeakMatch => "weak_match",
            Self::ValidationFailed => "validation_failed",
            Self::Stale => "stale",
            Self::Conflicting => "conflicting",
            Self::Denied => "denied",
            Self::TransientError => "transient_error",
            Self::FatalError => "fatal_error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantSynthesisMode {
    None,
    DeterministicReply,
    DeterministicSynthesis,
    ModelAnswer,
    Clarification,
    BoundedFailure,
}

impl AssistantSynthesisMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DeterministicReply => "deterministic_reply",
            Self::DeterministicSynthesis => "deterministic_synthesis",
            Self::ModelAnswer => "model_answer",
            Self::Clarification => "clarification",
            Self::BoundedFailure => "bounded_failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantExecutionStopReason {
    DeterministicReply,
    SufficientAnswer,
    ClarificationRequired,
    BudgetExhausted,
    NoPermittedFallback,
    DuplicateSignature,
    WeakEvidenceOnly,
    ConflictUnresolved,
    AclDenied,
    ConfirmationRequired,
    FatalError,
    ModelAnswerCompleted,
}

impl AssistantExecutionStopReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeterministicReply => "deterministic_reply",
            Self::SufficientAnswer => "sufficient_answer",
            Self::ClarificationRequired => "clarification_required",
            Self::BudgetExhausted => "budget_exhausted",
            Self::NoPermittedFallback => "no_permitted_fallback",
            Self::DuplicateSignature => "duplicate_signature",
            Self::WeakEvidenceOnly => "weak_evidence_only",
            Self::ConflictUnresolved => "conflict_unresolved",
            Self::AclDenied => "acl_denied",
            Self::ConfirmationRequired => "confirmation_required",
            Self::FatalError => "fatal_error",
            Self::ModelAnswerCompleted => "model_answer_completed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantExecutionBudget {
    pub max_planner_passes: u8,
    pub max_tool_steps: u8,
    pub max_alternate_steps: u8,
    pub max_parallel_tools: u8,
    pub max_evidence_items: u8,
    pub max_grounding_chars: usize,
    pub max_recovery_depth: u8,
    pub max_same_signature_repeats: u8,
    pub allow_verifier: bool,
    pub allow_parallel_read_fanout: bool,
}

impl AssistantExecutionBudget {
    pub fn for_mode(mode: AssistantResponseMode) -> Self {
        match mode {
            AssistantResponseMode::Instant => Self {
                max_planner_passes: 1,
                max_tool_steps: 1,
                max_alternate_steps: 0,
                max_parallel_tools: 1,
                max_evidence_items: 4,
                max_grounding_chars: 2_400,
                max_recovery_depth: 0,
                max_same_signature_repeats: 1,
                allow_verifier: false,
                allow_parallel_read_fanout: false,
            },
            AssistantResponseMode::Thinking => Self {
                max_planner_passes: 2,
                max_tool_steps: 3,
                max_alternate_steps: 2,
                max_parallel_tools: 2,
                max_evidence_items: 6,
                max_grounding_chars: 3_600,
                max_recovery_depth: 2,
                max_same_signature_repeats: 1,
                allow_verifier: false,
                allow_parallel_read_fanout: true,
            },
            AssistantResponseMode::Extended => Self {
                max_planner_passes: 3,
                max_tool_steps: 5,
                max_alternate_steps: 4,
                max_parallel_tools: 2,
                max_evidence_items: 8,
                max_grounding_chars: 4_800,
                max_recovery_depth: 4,
                max_same_signature_repeats: 1,
                allow_verifier: true,
                allow_parallel_read_fanout: true,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantClarificationRequest {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_field: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantEvidenceItem {
    pub id: String,
    pub tool: String,
    pub domain_family: AssistantDomainFamily,
    pub title: String,
    pub excerpt: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chunk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantToolOutcome {
    pub tool: String,
    pub label: String,
    pub domain_family: AssistantDomainFamily,
    pub kind: AssistantToolOutcomeKind,
    #[serde(default)]
    pub confidence: f32,
    pub block: AssistantToolContextBlock,
    #[serde(default)]
    pub evidence_items: Vec<AssistantEvidenceItem>,
    #[serde(default)]
    pub ambiguity_keys: Vec<String>,
    #[serde(default)]
    pub recovery_hints: Vec<String>,
    pub args_hash: String,
    pub result_signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssistantRecoveryDecision {
    Stop {
        reason: AssistantExecutionStopReason,
    },
    AskClarification {
        request: AssistantClarificationRequest,
    },
    RunNext {
        call: PlannedToolCall,
        edge_label: String,
        recovery_depth: u8,
        is_alternate: bool,
    },
    SynthesizeNow,
    DeterministicReplyNow,
    VerifierPass,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantExecutionAttempt {
    pub step_index: u32,
    pub tool: String,
    pub label: String,
    pub domain_family: AssistantDomainFamily,
    pub status: String,
    pub outcome_kind: AssistantToolOutcomeKind,
    pub latency_ms: u64,
    pub args_hash: String,
    pub result_signature: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub ambiguity_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_edge: Option<String>,
    #[serde(default)]
    pub used_alternate: bool,
    #[serde(default)]
    pub recovery_depth: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantExecutionTrace {
    pub response_mode: AssistantResponseMode,
    pub budget: AssistantExecutionBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_mode: Option<String>,
    #[serde(default)]
    pub attempts: Vec<AssistantExecutionAttempt>,
    #[serde(default)]
    pub retained_evidence: Vec<AssistantEvidenceItem>,
    pub stop_reason: AssistantExecutionStopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_outcome_kind: Option<AssistantToolOutcomeKind>,
    pub final_answer_path: AssistantSynthesisMode,
    #[serde(default)]
    pub planner_pass_count: u8,
    #[serde(default)]
    pub tool_step_count: u32,
    #[serde(default)]
    pub alternate_tool_count: u32,
    #[serde(default)]
    pub recovery_step_count: u32,
    #[serde(default)]
    pub clarification_count: u32,
    #[serde(default)]
    pub conflict_count: u32,
    #[serde(default)]
    pub deterministic_answer_used: bool,
    #[serde(default)]
    pub synthesis_used: bool,
    #[serde(default)]
    pub used_role_backends: Vec<String>,
    #[serde(default)]
    pub outcome_counts: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantExecutionCandidateStep {
    pub call: PlannedToolCall,
    pub domain_family: AssistantDomainFamily,
    #[serde(default)]
    pub preferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantExecutionPlanCandidate {
    pub primary_domain_family: AssistantDomainFamily,
    pub requested_response_mode: AssistantResponseMode,
    pub candidate_steps: Vec<AssistantExecutionCandidateStep>,
    pub expected_answer_shape: String,
    #[serde(default)]
    pub clarification_preferred: bool,
    #[serde(default)]
    pub requires_entity_resolution: bool,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerFallbackReason {
    ParseFailed,
    ValidationFailed,
    RepairExhausted,
    ToolNotAllowed,
    ArgumentInvalid,
    ToolCountExceeded,
    UnsupportedCombination,
}

impl PlannerFallbackReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ParseFailed => "parse_failed",
            Self::ValidationFailed => "validation_failed",
            Self::RepairExhausted => "repair_exhausted",
            Self::ToolNotAllowed => "tool_not_allowed",
            Self::ArgumentInvalid => "argument_invalid",
            Self::ToolCountExceeded => "tool_count_exceeded",
            Self::UnsupportedCombination => "unsupported_combination",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlannerIssue {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlannerRepairRecord {
    pub attempt_index: u8,
    #[serde(default)]
    pub issues: Vec<PlannerIssue>,
    pub repaired_successfully: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlannerExecutionStats {
    pub parse_attempts: u8,
    pub validation_failures: u8,
    pub repair_attempts: u8,
    pub repair_successes: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<PlannerFallbackReason>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationPromptDebug {
    #[serde(default)]
    pub system_message_count: u32,
    #[serde(default)]
    pub history_message_count: u32,
    #[serde(default)]
    pub grounding_chunk_count: u32,
    #[serde(default)]
    pub response_mode: String,
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
    pub planner_parse_attempts: u8,
    #[serde(default)]
    pub planner_validation_failures: u8,
    #[serde(default)]
    pub planner_repair_attempts: u8,
    #[serde(default)]
    pub planner_repair_successes: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_fallback_reason: Option<String>,
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
    #[serde(default)]
    pub tool_step_count: u32,
    #[serde(default)]
    pub alternate_tool_count: u32,
    #[serde(default)]
    pub recovery_step_count: u32,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default)]
    pub clarification_count: u32,
    #[serde(default)]
    pub conflict_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_outcome_kind: Option<String>,
    #[serde(default)]
    pub deterministic_answer_used: bool,
    #[serde(default)]
    pub synthesis_used: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_backend_usage: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_trace: Option<AssistantExecutionTrace>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    Grounding,
    Recovering,
    Synthesizing,
    Clarifying,
    Verifying,
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
