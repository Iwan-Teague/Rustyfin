use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use futures::{StreamExt, future::join_all};
use rustfin_ai_agent::{ChatChunk, ChatMessage, PromptCacheHint, SamplingParams};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use rustfin_ai_agent::backends::PromptBackend;

use super::confirmation::{
    is_supported_calendar_create_intent, is_supported_calendar_delete_intent,
    is_supported_conversation_manage_intent, is_supported_document_create_intent,
    pending_action_request_for_message_with_state,
};
use super::context::AssistantContext;
use super::dates::{assistant_local_now, assistant_local_today, extract_single_calendar_date};
use super::memory::{augment_history_with_entity_graph, build_grounding_chunks_for_turn};
use super::registry::AssistantToolName;
use super::replies::{compact_text, rank_and_compress_grounding_chunks};
use super::tools::{execute_tool, source_from_block};
use super::types::{
    AssistantChatRequest, AssistantExecutionCandidateStep, AssistantExecutionPlanCandidate,
    AssistantFollowUpContext, AssistantFollowUpEntity, AssistantGroundingChunk,
    AssistantHistoryMessage, AssistantPlannerDebug, AssistantPlannerMode, AssistantResponseMode,
    AssistantToolContextBlock, AssistantToolInput, ConversationPromptDebug, PlannedToolCall,
    PlannedToolSet, PlannerFallbackReason, PlannerIssue, PlannerRepairRecord,
    PreparedAssistantTurn,
};
use super::web::{normalize_public_url, public_web_tools_enabled};
use super::web_sources::{curated_web_category_for_url, curated_web_category_label};
use crate::auth::AuthUser;
use crate::state::AppState;

const MAX_TOOL_CALLS_PER_TURN: usize = 8;
const PLANNER_HISTORY_MESSAGE_LIMIT: usize = 6;
const DEFAULT_ASSISTANT_CONTEXT_LENGTH_TOKENS: u32 = 4096;
const NORMAL_HISTORY_RAW_MESSAGE_LIMIT: usize = 6;
const EXTENDED_HISTORY_RAW_MESSAGE_LIMIT: usize = 8;
const INSTANT_HISTORY_RAW_MESSAGE_LIMIT: usize = 4;
const EMERGENCY_HISTORY_RAW_MESSAGE_LIMIT: usize = 2;
const NORMAL_HISTORY_COMPACT_CHARS: usize = 220;
const EMERGENCY_HISTORY_COMPACT_CHARS: usize = 120;
const NORMAL_GROUNDING_PROMPT_CHARS: usize = super::replies::MAX_GROUNDING_PROMPT_CHARS;
const EMERGENCY_GROUNDING_PROMPT_CHARS: usize = 1_200;
const EMERGENCY_GROUNDING_CHUNK_LIMIT: usize = 4;

const MAX_PLANNER_REPAIR_ATTEMPTS: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum PlannerAst {
    None {
        #[serde(default)]
        tools: Vec<PlannerAstToolCall>,
    },
    ToolPlan {
        #[serde(default)]
        tools: Vec<PlannerAstToolCall>,
    },
}

impl PlannerAst {
    fn mode_label(&self) -> &'static str {
        match self {
            Self::None { .. } => "none",
            Self::ToolPlan { .. } => "tool_plan",
        }
    }

    fn tools(&self) -> &[PlannerAstToolCall] {
        match self {
            Self::None { tools } | Self::ToolPlan { tools } => tools,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlannerAstToolCall {
    #[serde(alias = "name")]
    tool: String,
    #[serde(default)]
    args: PlannerAstArgs,
    #[serde(flatten)]
    legacy_args: PlannerAstArgs,
}

impl PlannerAstToolCall {
    fn merged_args(&self) -> PlannerAstArgs {
        let args = &self.args;
        let legacy = &self.legacy_args;
        PlannerAstArgs {
            query: args.query.clone().or_else(|| legacy.query.clone()),
            url: args.url.clone().or_else(|| legacy.url.clone()),
            category: args.category.clone().or_else(|| legacy.category.clone()),
            availability: args
                .availability
                .clone()
                .or_else(|| legacy.availability.clone()),
            room_mode: args.room_mode.clone().or_else(|| legacy.room_mode.clone()),
            workspace_id: args
                .workspace_id
                .clone()
                .or_else(|| legacy.workspace_id.clone()),
            person_id: args.person_id.clone().or_else(|| legacy.person_id.clone()),
            reference: args.reference.clone().or_else(|| legacy.reference.clone()),
            limit: args.limit.or(legacy.limit),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PlannerAstArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    availability: Option<String>,
    #[serde(default)]
    room_mode: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    person_id: Option<String>,
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

pub async fn plan_tool_calls_with_model_assist<B: PromptBackend>(
    backend: &B,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> PlannedToolSet {
    let mut debug = AssistantPlannerDebug {
        schema_version: 2,
        ..AssistantPlannerDebug::default()
    };
    let lower = message.to_ascii_lowercase();
    if is_tool_inventory_query(&lower)
        || is_direct_model_chat_request(message, history)
        || is_unsafe_destructive_host_request(&lower)
    {
        debug.planner_mode = Some(
            AssistantPlannerMode::DeterministicFallback
                .as_str()
                .to_string(),
        );
        return PlannedToolSet {
            mode: AssistantPlannerMode::DeterministicFallback,
            calls: Vec::new(),
            debug,
        };
    }

    if extract_follow_up_entity_reference(message).is_some() {
        let calls = plan_tool_calls_with_history(message, history);
        if !calls.is_empty() {
            debug.planner_mode = Some(
                AssistantPlannerMode::DeterministicEntityFollowUp
                    .as_str()
                    .to_string(),
            );
            debug.validated_call_count = calls.len() as u32;
            debug.final_selected_tools = calls
                .iter()
                .map(|call| call.tool.as_str().to_string())
                .collect();
            return PlannedToolSet {
                mode: AssistantPlannerMode::DeterministicEntityFollowUp,
                calls,
                debug,
            };
        }
    }

    let deterministic = plan_tool_calls_with_history(message, history);
    let model_plan =
        resolve_model_plan_with_repair(backend, user, message, history, &mut debug).await;

    if should_prefer_deterministic_plan(&deterministic) {
        debug.planner_mode = Some(
            AssistantPlannerMode::DeterministicFallback
                .as_str()
                .to_string(),
        );
        debug.validated_call_count = deterministic.len() as u32;
        debug.final_selected_tools = deterministic
            .iter()
            .map(|call| call.tool.as_str().to_string())
            .collect();
        return PlannedToolSet {
            mode: AssistantPlannerMode::DeterministicFallback,
            calls: deterministic,
            debug,
        };
    }

    if let Some(model_calls) = model_plan {
        debug.planner_mode = Some(AssistantPlannerMode::ModelStructured.as_str().to_string());
        debug.validated_call_count = model_calls.len() as u32;
        debug.final_selected_tools = model_calls
            .iter()
            .map(|call| call.tool.as_str().to_string())
            .collect();
        return PlannedToolSet {
            mode: AssistantPlannerMode::ModelStructured,
            calls: model_calls,
            debug,
        };
    }

    debug.planner_mode = Some(
        AssistantPlannerMode::DeterministicFallback
            .as_str()
            .to_string(),
    );
    debug.validated_call_count = deterministic.len() as u32;
    debug.final_selected_tools = deterministic
        .iter()
        .map(|call| call.tool.as_str().to_string())
        .collect();
    PlannedToolSet {
        mode: AssistantPlannerMode::DeterministicFallback,
        calls: deterministic,
        debug,
    }
}

pub fn plan_execution_candidates(
    response_mode: AssistantResponseMode,
    planned: &PlannedToolSet,
) -> Vec<AssistantExecutionPlanCandidate> {
    if planned.calls.is_empty() {
        return Vec::new();
    }

    let steps = planned
        .calls
        .iter()
        .enumerate()
        .map(|(index, call)| AssistantExecutionCandidateStep {
            call: call.clone(),
            domain_family: call.tool.domain_family(),
            preferred: index == 0,
        })
        .collect::<Vec<_>>();
    let primary_domain_family = planned
        .calls
        .first()
        .map(|call| call.tool.domain_family())
        .unwrap_or(super::types::AssistantDomainFamily::System);

    vec![AssistantExecutionPlanCandidate {
        primary_domain_family,
        requested_response_mode: response_mode,
        candidate_steps: steps,
        expected_answer_shape: primary_domain_family.as_str().to_string(),
        clarification_preferred: planned
            .calls
            .first()
            .map(|call| call.tool.ambiguity_prone())
            .unwrap_or(false),
        requires_entity_resolution: false,
    }]
}

async fn resolve_model_plan_with_repair<B: PromptBackend>(
    backend: &B,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
    debug: &mut AssistantPlannerDebug,
) -> Option<Vec<PlannedToolCall>> {
    let raw_response = run_model_planner(backend, user, message, history).await?;
    debug.raw_response_hash = Some(hash_planner_text(&raw_response));
    debug.raw_response = Some(raw_response.clone());

    let mut current_raw = raw_response;
    let mut parse_failed = false;

    for attempt in 0..=MAX_PLANNER_REPAIR_ATTEMPTS {
        debug.execution.parse_attempts = debug.execution.parse_attempts.saturating_add(1);
        let ast = match parse_planner_ast(&current_raw) {
            Ok(ast) => ast,
            Err(issues) => {
                parse_failed = true;
                debug.validation_errors = planner_issue_messages(&issues);
                if attempt == MAX_PLANNER_REPAIR_ATTEMPTS {
                    let fallback_reason = fallback_reason_for_issues(&issues, true, attempt > 0);
                    debug.execution.fallback_reason = Some(fallback_reason);
                    debug.fallback_reason = Some(fallback_reason.as_str().to_string());
                    return None;
                }
                let repaired = run_planner_repair(
                    backend,
                    user,
                    message,
                    &current_raw,
                    history,
                    &issues,
                    attempt + 1,
                )
                .await?;
                debug.execution.repair_attempts = debug.execution.repair_attempts.saturating_add(1);
                debug.repair_attempt_count = u32::from(debug.execution.repair_attempts);
                debug.repaired_response = Some(repaired.clone());
                debug.used_repaired_response = true;
                debug.repair_records.push(PlannerRepairRecord {
                    attempt_index: attempt + 1,
                    issues: issues.clone(),
                    repaired_successfully: false,
                });
                current_raw = repaired;
                continue;
            }
        };

        debug.planner_mode = Some(ast.mode_label().to_string());
        match validate_planner_ast(&ast, user, message, history) {
            Ok(calls) => {
                if attempt > 0 {
                    debug.execution.repair_successes =
                        debug.execution.repair_successes.saturating_add(1);
                    if let Some(record) = debug.repair_records.last_mut() {
                        record.repaired_successfully = true;
                    }
                }
                return Some(calls);
            }
            Err(issues) => {
                debug.execution.validation_failures =
                    debug.execution.validation_failures.saturating_add(1);
                debug.validation_errors = planner_issue_messages(&issues);
                if attempt == MAX_PLANNER_REPAIR_ATTEMPTS {
                    let fallback_reason =
                        fallback_reason_for_issues(&issues, parse_failed, attempt > 0);
                    debug.execution.fallback_reason = Some(fallback_reason);
                    debug.fallback_reason = Some(fallback_reason.as_str().to_string());
                    return None;
                }
                let repaired = run_planner_repair(
                    backend,
                    user,
                    message,
                    &current_raw,
                    history,
                    &issues,
                    attempt + 1,
                )
                .await?;
                debug.execution.repair_attempts = debug.execution.repair_attempts.saturating_add(1);
                debug.repair_attempt_count = u32::from(debug.execution.repair_attempts);
                debug.repaired_response = Some(repaired.clone());
                debug.used_repaired_response = true;
                debug.repair_records.push(PlannerRepairRecord {
                    attempt_index: attempt + 1,
                    issues: issues.clone(),
                    repaired_successfully: false,
                });
                current_raw = repaired;
            }
        }
    }

    debug.execution.fallback_reason = Some(PlannerFallbackReason::RepairExhausted);
    debug.fallback_reason = Some("repair_exhausted".to_string());
    None
}

fn planner_issue_messages(issues: &[PlannerIssue]) -> Vec<String> {
    issues
        .iter()
        .map(|issue| match issue.path.as_deref() {
            Some(path) => format!("{} ({path})", issue.message),
            None => issue.message.clone(),
        })
        .collect()
}

fn hash_planner_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn fallback_reason_for_issues(
    issues: &[PlannerIssue],
    parse_failed: bool,
    repair_attempted: bool,
) -> PlannerFallbackReason {
    if issues
        .iter()
        .any(|issue| issue.code == "tool_count_exceeded")
    {
        PlannerFallbackReason::ToolCountExceeded
    } else if issues.iter().any(|issue| issue.code == "tool_not_allowed") {
        PlannerFallbackReason::ToolNotAllowed
    } else if issues.iter().any(|issue| {
        matches!(
            issue.code.as_str(),
            "missing_required_argument" | "invalid_argument" | "invalid_enum"
        )
    }) {
        PlannerFallbackReason::ArgumentInvalid
    } else if issues.iter().any(|issue| {
        matches!(
            issue.code.as_str(),
            "unsupported_combination" | "duplicate_tool"
        )
    }) {
        PlannerFallbackReason::UnsupportedCombination
    } else if repair_attempted {
        PlannerFallbackReason::RepairExhausted
    } else if parse_failed {
        PlannerFallbackReason::ParseFailed
    } else {
        PlannerFallbackReason::ValidationFailed
    }
}

fn should_prefer_deterministic_plan(calls: &[PlannedToolCall]) -> bool {
    !calls.is_empty()
        && calls.iter().all(|call| {
            matches!(
                call.tool,
                AssistantToolName::DictionaryGetAccountIdentity
                    | AssistantToolName::DictionaryListVisibleWorkspaces
                    | AssistantToolName::DictionaryBrowseWorkspacePeople
                    | AssistantToolName::DictionarySearchPeople
                    | AssistantToolName::DictionaryGetPersonBundle
                    | AssistantToolName::DictionaryResolveRelationshipReference
                    | AssistantToolName::WeatherGetCurrent
                    | AssistantToolName::WeatherGetForecast
                    | AssistantToolName::WeatherGetHistory
                    | AssistantToolName::WeatherGetHourlyWindow
                    | AssistantToolName::WeatherResolveLocationAlias
                    | AssistantToolName::WeatherGetForecastForDate
                    | AssistantToolName::WeatherGetRecentHistoryForDate
                    | AssistantToolName::SystemGetCurrentDateTime
                    | AssistantToolName::SystemGetAiRuntimeSummary
                    | AssistantToolName::NetworkGetTopologySummary
                    | AssistantToolName::NetworkGetInterfaceDetails
                    | AssistantToolName::NetworkGetInterfaceByIp
                    | AssistantToolName::NetworkGetDefaultRoute
                    | AssistantToolName::NetworkGetHostnameAliases
                    | AssistantToolName::NetworkGetDnsServers
                    | AssistantToolName::NetworkGetRouteTable
                    | AssistantToolName::NetworkGetActiveConnections
                    | AssistantToolName::NetworkGetInterfaceCounters
                    | AssistantToolName::NetworkGetWifiStatus
                    | AssistantToolName::NetworkGetVpnStatus
                    | AssistantToolName::SystemGetServiceHealth
                    | AssistantToolName::SystemGetServiceDetail
                    | AssistantToolName::SystemGetMountDetail
                    | AssistantToolName::SystemGetStoragePathDetail
                    | AssistantToolName::SystemGetProcessDetail
                    | AssistantToolName::SystemGetListenerDetail
                    | AssistantToolName::SystemGetDiskUsageDetail
                    | AssistantToolName::SystemGetPortConflicts
                    | AssistantToolName::SystemGetPortConflictDetail
                    | AssistantToolName::SystemGetFailedUnits
                    | AssistantToolName::SystemGetFailedUnitDetail
                    | AssistantToolName::DownloadsListAvailableArtifacts
                    | AssistantToolName::DownloadsGetArtifactDetails
                    | AssistantToolName::DownloadsGetArtifactSource
                    | AssistantToolName::DownloadsGetReleaseNotes
                    | AssistantToolName::DownloadsGetArtifactChecksum
                    | AssistantToolName::DownloadsGetArtifactInstallSteps
                    | AssistantToolName::DownloadsGetArtifactCompatibility
                    | AssistantToolName::LibrariesListAccessible
                    | AssistantToolName::LibrariesGetLibrarySummary
                    | AssistantToolName::LibrarySearchTitles
                    | AssistantToolName::LibraryGetItemSummary
                    | AssistantToolName::LibraryGetItemMediaDetails
                    | AssistantToolName::LibraryGetItemSourcePaths
                    | AssistantToolName::LibrariesGetRecentlyAdded
                    | AssistantToolName::LibrariesFindDuplicateTitles
                    | AssistantToolName::LibrariesListMissingMetadata
                    | AssistantToolName::CalendarGetNextEvent
                    | AssistantToolName::CalendarGetNextEventTiming
                    | AssistantToolName::CalendarCountEvents
                    | AssistantToolName::CalendarListBusyDays
                    | AssistantToolName::CalendarUpcomingBirthdays
                    | AssistantToolName::CalendarListDateConflicts
                    | AssistantToolName::CalendarListFreeDays
                    | AssistantToolName::MemoryListRecentFacts
                    | AssistantToolName::MemoryListRecentEntities
                    | AssistantToolName::MemoryListRecentChanges
                    | AssistantToolName::MemoryListConflictingFacts
                    | AssistantToolName::MemorySearchFacts
                    | AssistantToolName::MemorySearchEntities
                    | AssistantToolName::MemoryGetEntityRelations
                    | AssistantToolName::MemoryGetPersonSummary
                    | AssistantToolName::MemoryGetEntityProvenance
                    | AssistantToolName::SystemGetKernelInfo
                    | AssistantToolName::SystemGetCpuTopology
                    | AssistantToolName::SystemGetTemperatureSensors
                    | AssistantToolName::SystemGetBlockDeviceInventory
                    | AssistantToolName::SystemGetFilesystemTable
                    | AssistantToolName::SystemGetGpuInventory
                    | AssistantToolName::SystemGetPciDevices
                    | AssistantToolName::SystemGetUsbDevices
                    | AssistantToolName::SystemGetBootLogSummary
                    | AssistantToolName::SystemGetJournalSummary
            )
        })
}

pub async fn prepare_assistant_turn(
    state: &AppState,
    user: &AuthUser,
    request: AssistantChatRequest,
) -> PreparedAssistantTurn {
    if let Some(refusal) = unsafe_action_response_for_message(&request.message) {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(refusal),
        };
    }

    if let Some(result) = pending_action_request_for_message_with_state(
        state,
        user,
        &request.message,
        None,
        &request.model,
    )
    .await
    {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(match result {
                Ok(parsed) => format!(
                    "{} Reply with \"Confirm\" to continue.",
                    parsed.payload.summary
                ),
                Err(message) => message,
            }),
        };
    }

    if let Some(refusal) = unsupported_write_response_for_message(&request.message) {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(refusal),
        };
    }

    if let Some(tool_inventory) = deterministic_tool_inventory_reply(user, &request.message) {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(tool_inventory),
        };
    }

    if let Some(clarification) = immediate_response_for_message(&request.message) {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(clarification),
        };
    }

    let context = AssistantContext::new(user, uuid::Uuid::new_v4().to_string());
    let planned_tools = plan_tool_calls_with_history(&request.message, &request.history);

    let tool_results = join_all(planned_tools.iter().map(|call| {
        let context = context.clone();
        async move {
            let block = execute_tool(state, &context, call).await;
            let source = source_from_block(call.tool, &block);
            (block, source)
        }
    }))
    .await;
    let grounding_blocks: Vec<_> = tool_results
        .iter()
        .map(|(block, _)| block.clone())
        .collect();
    let grounding_sources: Vec<_> = tool_results.into_iter().map(|(_, source)| source).collect();

    let augmented_history =
        augment_history_with_entity_graph(state, &context, &request.history, &request.message)
            .await;
    let grounding_chunks = build_grounding_chunks_for_turn(
        state,
        &context,
        &request,
        &planned_tools,
        &grounding_blocks,
        &grounding_sources,
        &augmented_history,
    )
    .await;
    let prompt_request = AssistantChatRequest {
        history: augmented_history,
        ..request
    };
    let messages = build_assistant_messages(prompt_request, &grounding_chunks);

    PreparedAssistantTurn {
        messages,
        sources: grounding_sources,
        immediate_response: None,
    }
}

async fn run_model_planner<B: PromptBackend>(
    backend: &B,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Option<String> {
    let planner_messages = build_model_planner_messages(user, message, history);
    let prompt_cache = planner_prompt_cache_hint(
        backend.capabilities().supports_prompt_cache,
        user,
        message,
        history,
    );
    collect_backend_text(
        backend,
        planner_messages,
        SamplingParams {
            temperature: 0.1,
            top_p: 0.9,
            top_k: 20,
            repeat_penalty: 1.05,
            max_tokens: 320,
        },
        prompt_cache,
    )
    .await
}

async fn run_planner_repair<B: PromptBackend>(
    backend: &B,
    user: &AuthUser,
    message: &str,
    raw_invalid_json: &str,
    history: &[AssistantHistoryMessage],
    issues: &[PlannerIssue],
    _attempt_index: u8,
) -> Option<String> {
    collect_backend_text(
        backend,
        build_planner_repair_messages(user, message, raw_invalid_json, history, issues),
        SamplingParams {
            temperature: 0.0,
            top_p: 0.9,
            top_k: 20,
            repeat_penalty: 1.0,
            max_tokens: 320,
        },
        None,
    )
    .await
}

async fn collect_backend_text<B: PromptBackend>(
    backend: &B,
    messages: Vec<ChatMessage>,
    sampling: SamplingParams,
    prompt_cache: Option<PromptCacheHint>,
) -> Option<String> {
    let planner_stream = backend.chat_stream_boxed(messages, sampling, prompt_cache);
    futures::pin_mut!(planner_stream);

    let mut response = String::new();
    while let Some(chunk) = planner_stream.next().await {
        match chunk.ok()? {
            ChatChunk::Token(text) => response.push_str(&text),
            ChatChunk::Done => return Some(response),
            ChatChunk::Stats { .. } => {}
        }
    }

    Some(response)
}

fn build_planner_repair_messages(
    user: &AuthUser,
    message: &str,
    raw_invalid_json: &str,
    history: &[AssistantHistoryMessage],
    issues: &[PlannerIssue],
) -> Vec<ChatMessage> {
    let role_label = if user.role == "admin" {
        "admin"
    } else {
        "authenticated_user"
    };
    let issue_lines = issues
        .iter()
        .map(|issue| match issue.path.as_deref() {
            Some(path) => format!("- {}: {} ({path})", issue.code, issue.message),
            None => format!("- {}: {}", issue.code, issue.message),
        })
        .collect::<Vec<_>>()
        .join("\n");

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You repair invalid Rustyfin planner JSON.\nReturn JSON only.\nPreserve user intent but obey the allowed schema.\nDo not add tools not already justified by the original request.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Current user role: {role_label}\nAllowed tool inventory:\n{}\nRecent conversation:\n{}\nOriginal user message:\n{}\nValidation issues:\n{}\nRaw invalid JSON:\n{}",
                planner_tool_inventory(user),
                planner_history_summary(history),
                message.trim(),
                issue_lines,
                raw_invalid_json.trim(),
            ),
        },
    ]
}

fn planner_prompt_cache_hint(
    enabled: bool,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Option<PromptCacheHint> {
    if !enabled {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(user.user_id.as_bytes());
    hasher.update(b"|");
    hasher.update(user.role.as_bytes());
    hasher.update(b"|");
    hasher.update(message.trim().as_bytes());
    hasher.update(b"|");
    hasher.update(planner_history_summary(history).as_bytes());
    let cache_key = format!("{:x}", hasher.finalize());

    Some(PromptCacheHint {
        cache_key: Some(cache_key),
        cache_scope: Some("planner".to_string()),
        enabled: true,
    })
}

fn build_model_planner_messages(
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Vec<ChatMessage> {
    let allowed_tools = planner_tool_inventory(user);
    let recent_tools = recent_grounded_tools(history);
    let recent_history = planner_history_summary(history);
    let local_now = assistant_local_now();
    let role_label = if user.role == "admin" {
        "admin"
    } else {
        "authenticated_user"
    };

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: format!(
                "You are the Rustyfin assistant tool planner. Choose zero to {MAX_TOOL_CALLS_PER_TURN} grounded read-only tools. \
Return JSON only with no markdown, no prose, and no code fences.\n\
Schema:\n\
{{\"mode\":\"tool_plan\",\"tools\":[{{\"tool\":\"tool_name\",\"args\":{{\"query\":\"optional\",\"url\":\"optional\",\"category\":\"optional\",\"availability\":\"optional\",\"room_mode\":\"optional\",\"workspace_id\":\"optional\",\"person_id\":\"optional\",\"reference\":\"optional\",\"limit\":\"optional integer\"}}}}]}}\n\
or\n\
{{\"mode\":\"none\",\"tools\":[]}}\n\
Rules:\n\
- Never use a tool not listed below.\n\
- Never exceed {MAX_TOOL_CALLS_PER_TURN} tools.\n\
- Use detail tools only when the user is asking about one specific room, one specific server, one specific download artifact, or one specific library item.\n\
- Use libraries_list_accessible for generic library access questions.\n\
- Use dictionary_get_account_identity when the user asks who they are linked to in the Human Dictionary.\n\
- Use dictionary_list_visible_workspaces when the user wants to browse or choose among Human Dictionary workspaces.\n\
- Use dictionary_browse_workspace_people when the user wants visible people in one chosen Human Dictionary workspace, with or without a search query.\n\
- Use dictionary_search_people for visible Human Dictionary search inside one known workspace.\n\
- Use dictionary_get_person_bundle when you already know the workspace and one specific visible person id.\n\
- Use dictionary_resolve_relationship_reference for relationship-relative Human Dictionary queries such as \"my mother\", \"my brother\", or \"my co-workers\".\n\
- Use library_search_titles for searching by title.\n\
- Use libraries_get_library_summary when the user wants exact metadata, item counts, paths, or settings for one accessible library.\n\
- Use library_get_item_media_details when the user wants file path, artwork, poster/backdrop/logo, or storage details for one specific library item.\n\
- Use library_get_item_source_paths when the user wants the source file paths for one specific library item.\n\
- Use libraries_get_recently_added for recently added or newest library items.\n\
- Use libraries_find_duplicate_titles when the user wants duplicate titles or collisions across accessible libraries.\n\
- Use libraries_list_missing_metadata when the user wants library items with missing metadata.\n\
- Use downloads_list_available_artifacts for generic host-published download questions.\n\
- Use downloads_get_artifact_details when the user wants exact metadata for one specific download artifact or package.\n\
- Use downloads_get_artifact_source when the user wants the source URL or package path for one specific download artifact.\n\
- Use downloads_get_release_notes when the user wants the release-note text for one specific download artifact.\n\
- Use downloads_get_artifact_checksum when the user wants only the checksum or verification hash for one specific download artifact.\n\
- Use downloads_get_artifact_install_steps when the user wants install steps or setup instructions for one specific download artifact.\n\
- Use downloads_get_artifact_compatibility when the user wants platform or architecture compatibility for one specific download artifact.\n\
- Use web_search_public_web with category=technology for technology, developer, engineering, or AI sources.\n\
- Use web_search_public_web with category=business for business, company, market, earnings, or startup sources.\n\
- Use web_search_public_web with category=economics for macroeconomics, inflation, labor, or official-data sources.\n\
- Use web_fetch_public_page_summary with category when the URL belongs to one of the curated source sets.\n\
- Use memory_list_recent_facts for recent stored facts.\n\
- Use memory_list_recent_entities for recent stored entities or people.\n\
- Use memory_list_recent_changes when the user asks what changed recently in memory, optionally narrowed to a subject.\n\
- Use memory_list_conflicting_facts when the user asks about conflicting or contradictory stored facts, optionally narrowed to a subject.\n\
- Use memory_search_facts for searching stored facts by subject.\n\
- Use memory_search_entities for searching stored entities or people by subject.\n\
- Use memory_find_exact_entity for exact stored entity lookups.\n\
- Use memory_get_entity_relations when the user asks who or what is related to a stored entity.\n\
- Use memory_get_person_summary when the user wants a grounded summary of one specific person or profile entity.\n\
- Use memory_get_entity_relation_path when the user asks how two stored entities are connected.\n\
- Use memory_get_entity_provenance when the user asks where a stored entity or fact came from or what source grounded it.\n\
- Use calendar_upcoming_birthdays only for birthday requests, including named questions like \"When is Rachel's birthday?\".\n\
- Use calendar_get_next_event when the user asks for the next or nearest upcoming calendar event.\n\
- Use calendar_get_next_event_timing when the user asks how long until the next upcoming calendar event.\n\
- Use calendar_list_date_conflicts when the user asks whether a date or window has overlapping calendar events.\n\
- Use calendar_list_free_days when the user asks which dates in a window are free or open.\n\
- Use calendar_count_events when the user asks how many events are in a window or how busy a calendar window is.\n\
- Use calendar_list_busy_days when the user asks which dates in a window are the busiest.\n\
- Use calendar_get_event_details when the user wants more detail about one specific calendar event.\n\
- Use channels_list_unread_activity for recent visible channel activity; exact unread counts are not available.\n\
- Use channels_get_transcript_summary when the user asks what a transcribed voice call was about or wants a transcript-based call summary.\n\
- Use network_get_topology_summary for Rustyfin network, interface, IP address, hostname, remote-access, proxy, or topology questions.\n\
- Use network_get_interface_details when the user wants one specific interface or IP address.\n\
- Use network_get_interface_by_ip when the user asks which interface owns a specific IP address.\n\
- Use network_get_default_route when the user asks for the host default route, gateway, or outbound path.\n\
- Use network_get_hostname_aliases when the user asks about hostname aliases, host naming, or /etc/hosts style mappings.\n\
- Use network_get_dns_servers when the user asks about DNS servers, resolvers, nameservers, or resolver configuration.\n\
- Use network_get_route_table when the user asks about routing tables, routes, or gateway paths.\n\
- Use network_get_active_connections when the user asks about active sockets, listeners, or established connections.\n\
- Use network_get_interface_counters when the user asks about interface traffic counters, bytes, packets, or link stats.\n\
- Use network_get_wifi_status when the user asks about Wi-Fi, wireless, SSID, or signal information.\n\
- Use network_get_vpn_status when the user asks about VPN, WireGuard, tunnel, or TUN/TAP interfaces.\n\
- Use weather_get_current for current weather, temperature, wind, or conditions right now.\n\
- Use weather_get_forecast for forecast, tomorrow, weekend, this week, next few days, rain chance, or weather planning questions.\n\
- Use weather_get_history for recent past-weather questions such as yesterday, last night, or a specific earlier date.\n\
- Use weather_resolve_location_alias when the user wants a canonical location or timezone.\n\
- Use weather_get_forecast_for_date when the user asks about weather on one exact calendar day.\n\
- Use weather_get_hourly_window when the user asks for hour-by-hour weather on one exact day.\n\
- Use weather_get_recent_history_for_date when the user asks about weather on one exact recent past day.\n\
- Use rooms_list_joinable for invites or rooms the user can join now.\n\
- Use system_get_current_datetime for current date/time questions or when the user asks what calendar date a relative day like next Tuesday lands on.\n\
- Use system_get_ai_runtime_summary for current AI model, backend, role-routing, queue, or warm-pool questions.\n\
- Use system_get_host_runtime_summary only for host/runtime resource questions.\n\
- Use system_get_backup_summary for backup or restore capability questions.\n\
- Use system_get_service_health for internal service or agent health questions.\n\
- Use system_get_service_detail for one specific internal service or agent health question.\n\
- Use system_get_transcode_summary for transcoding, ffmpeg, hardware acceleration, or transcode-failure questions.\n\
- Use system_get_storage_path_detail for one specific storage path, mount point, cache directory, model directory, or disk-usage location.\n\
- Use system_get_mount_detail for one specific mount point, mounted filesystem, volume, or backing mount query.\n\
- Use system_get_process_detail for one specific host process, pid, or command-line query.\n\
- Use system_get_listener_detail for one specific listener or socket query.\n\
- Use system_get_disk_usage_detail for one specific path or mount when the user wants exact disk usage.\n\
- Use system_get_storage_summary for general storage, disk, cache, or free-space questions.\n\
- Use system_get_recent_errors for recent failures, problem summaries, or error overviews.\n\
- Use system_get_kernel_info for kernel, OS release, or base platform questions.\n\
- Use system_get_cpu_topology for CPU socket, core, thread, or topology questions.\n\
- Use system_get_temperature_sensors for temperature, thermal zone, hardware sensor, or heat questions.\n\
- Use system_get_block_device_inventory for block devices, disks, partitions, or lsblk-style inventory questions.\n\
- Use system_get_filesystem_table for mounted filesystems or mount-table questions.\n\
- Use system_get_gpu_inventory for GPUs, graphics adapters, VRAM, or display-controller inventory questions.\n\
- Use system_get_pci_devices for PCI devices or PCI inventory questions.\n\
- Use system_get_usb_devices for USB device inventory questions.\n\
- Use system_get_boot_log_summary for boot logs or early boot summary questions.\n\
- Use system_get_journal_summary for current boot journal or warning/error journal questions.\n\
- Use system_get_port_conflicts for listening sockets, bound ports, or port-in-use questions.\n\
- Use system_get_port_conflict_detail for one specific port, socket, or process/listener question.\n\
- Use system_get_failed_units for failed systemd units or service failures.\n\
- Use system_get_failed_unit_detail for one specific failed systemd unit or service failure.\n\
- Use web_list_curated_sources when the user asks what trusted public-web source categories are available.\n\
- Use web_search_public_web with an optional curated category when the user wants technology, business, or economics sources.\n\
- Use web_fetch_public_page_summary with an optional curated category for explicit public URLs from those source sets.\n\
- Use web_search_public_web only for current public web information not already covered by a Rustyfin tool, the curated source catalog, or curated public-weather tools.\n\
- If the request is unsupported, casual chat, a joke, simple math, roleplay, a tone/style instruction, a reset request, or a write action, return mode none.\n\
- Allowed availability values for downloads: available, planned, unavailable.\n\
- Allowed availability values for servers: online, offline, healthy, problem.\n\
- Allowed room_mode values: video, audio, youtube, web, screen, create, play.\n\
Allowed tools for this user:\n{}",
                allowed_tools
            ),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Current user role: {role_label}\nRustyfin host local date/time: {}\nRecent grounded tools: {}\nRecent conversation:\n{}\nCurrent user message:\n{}",
                local_now.format("%Y-%m-%d %H:%M:%S %:z (%A)"),
                if recent_tools.is_empty() {
                    "none".to_string()
                } else {
                    recent_tools
                        .iter()
                        .map(|tool| tool.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                recent_history,
                message.trim()
            ),
        },
    ]
}

fn planner_tool_inventory(user: &AuthUser) -> String {
    AssistantToolName::all()
        .iter()
        .copied()
        .filter(|tool| tool_visible_to_user(*tool, user))
        .filter(|tool| {
            matches!(
                tool.spec().access_mode,
                super::types::ToolAccessMode::ReadOnly
            )
        })
        .map(|tool| {
            format!(
                "- {}: {}{}",
                tool.as_str(),
                tool.spec().summary,
                planner_tool_argument_hint(tool)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn planner_tool_argument_hint(tool: AssistantToolName) -> &'static str {
    match tool {
        AssistantToolName::CalendarCreateEvent => {
            " Args: required title/date/scope; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::DictionaryGetAccountIdentity => " Args: none.",
        AssistantToolName::DictionaryListVisibleWorkspaces => " Args: none.",
        AssistantToolName::DictionaryBrowseWorkspacePeople => {
            " Args: required workspace_id selector; optional query and limit."
        }
        AssistantToolName::DictionarySearchPeople => {
            " Args: required workspace_id/query; optional limit."
        }
        AssistantToolName::DictionaryGetPersonBundle => " Args: required workspace_id/person_id.",
        AssistantToolName::DictionaryResolveRelationshipReference => {
            " Args: required reference such as \"my mother\"; optional workspace_id override."
        }
        AssistantToolName::CalendarCreateBirthday => {
            " Args: required person/date/year/scope; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::CalendarDeleteEvent => {
            " Args: required event id/title/date metadata; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::DocumentCreateDownload => {
            " Args: required title/file name/format/content request; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::SystemGetFailedUnitDetail => {
            " Args: required failed unit or service reference."
        }
        AssistantToolName::ConversationsArchiveSelection => {
            " Args: required resolved conversation ids/titles; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::ConversationsDeleteSelection => {
            " Args: required resolved conversation ids/titles; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::ConversationsMoveToGroupSelection => {
            " Args: required resolved conversation ids/titles/group name; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::CalendarListEvents => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarGetNextEvent => " Args: none.",
        AssistantToolName::CalendarGetNextEventTiming => " Args: none.",
        AssistantToolName::CalendarListDateConflicts => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarListFreeDays => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarGetNextFreeDay => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarGetEventByExactDateAndTitle => {
            " Args: required query; the backend derives the visible calendar window from the message or follow-up context."
        }
        AssistantToolName::CalendarGetEventSeriesSummary => {
            " Args: required query; the backend derives the visible calendar window from the message."
        }
        AssistantToolName::CalendarGetNextFreeSlot => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarListBusySlots => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarCountEvents => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarListBusyDays => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarListOverlappingEvents => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarUpcomingBirthdays => {
            " Args: optional query; the backend derives the birthday time window from the message and can narrow results to a named person."
        }
        AssistantToolName::CalendarGetEventDetails => {
            " Args: required query; the backend derives the visible calendar window from the message or follow-up context."
        }
        AssistantToolName::ChannelsListUnreadActivity => {
            " Args: optional query; exact unread counts are unavailable."
        }
        AssistantToolName::ChannelsGetTranscriptSummary => {
            " Args: optional query; the backend picks the latest accessible completed voice transcript for a matching channel."
        }
        AssistantToolName::DownloadsListAvailableArtifacts => {
            " Args: optional query, optional availability."
        }
        AssistantToolName::DownloadsGetArtifactDetails => {
            " Args: required query; the backend resolves one download artifact and returns its exact metadata."
        }
        AssistantToolName::DownloadsGetArtifactSource => {
            " Args: required query; the backend resolves one download artifact and returns its source URL or package path."
        }
        AssistantToolName::DownloadsGetReleaseNotes => {
            " Args: required query; the backend resolves one download artifact and returns its release-note text."
        }
        AssistantToolName::DownloadsGetArtifactChecksum => {
            " Args: required query; the backend resolves one download artifact and returns its checksum details."
        }
        AssistantToolName::DownloadsGetArtifactInstallSteps => {
            " Args: required query; the backend resolves one download artifact and returns install guidance."
        }
        AssistantToolName::DownloadsGetArtifactCompatibility => {
            " Args: required query; the backend resolves one download artifact and returns platform compatibility details."
        }
        AssistantToolName::NetworkGetTopologySummary => " Args: none.",
        AssistantToolName::NetworkGetInterfaceDetails => {
            " Args: required query; the backend resolves one network interface or IP address."
        }
        AssistantToolName::NetworkGetInterfaceByIp => {
            " Args: required query; the backend resolves one network interface from an exact IP address."
        }
        AssistantToolName::NetworkGetDefaultRoute => {
            " Args: optional query; the backend resolves the host default route, gateway, or outbound path."
        }
        AssistantToolName::NetworkGetHostnameAliases => {
            " Args: optional query; the backend resolves hostname aliases or /etc/hosts style host names."
        }
        AssistantToolName::NetworkGetDnsServers => {
            " Args: optional query; the backend resolves DNS servers, resolvers, or nameserver configuration."
        }
        AssistantToolName::NetworkGetRouteTable => " Args: none.",
        AssistantToolName::NetworkGetActiveConnections => " Args: none.",
        AssistantToolName::NetworkGetInterfaceCounters => " Args: none.",
        AssistantToolName::NetworkGetWifiStatus => " Args: none.",
        AssistantToolName::NetworkGetVpnStatus => " Args: none.",
        AssistantToolName::LibrariesListAccessible => " Args: none.",
        AssistantToolName::LibrariesGetLibrarySummary => {
            " Args: required query; the backend resolves one accessible library and returns its exact metadata, paths, item count, and settings."
        }
        AssistantToolName::LibrarySearchTitles => " Args: required query.",
        AssistantToolName::LibraryGetItemSummary => " Args: required query.",
        AssistantToolName::LibraryGetItemMediaDetails => {
            " Args: required query; the backend resolves one accessible item and returns artwork and media-path details."
        }
        AssistantToolName::LibraryGetItemSourcePaths => {
            " Args: required query; the backend resolves one accessible item and returns source-path details."
        }
        AssistantToolName::LibrariesGetRecentlyAdded => " Args: optional query.",
        AssistantToolName::LibrariesFindDuplicateTitles => {
            " Args: none; the backend scans accessible libraries for duplicate titles."
        }
        AssistantToolName::LibrariesListMissingMetadata => {
            " Args: none; the backend scans accessible libraries for items missing metadata."
        }
        AssistantToolName::WeatherGetCurrent => " Args: required location.",
        AssistantToolName::WeatherGetForecast => {
            " Args: required location; the backend derives a short forecast window from the message."
        }
        AssistantToolName::WeatherGetHistory => {
            " Args: required location; the backend derives the recent history date window from the message."
        }
        AssistantToolName::WeatherGetHourlyWindow => {
            " Args: required location; the backend derives the exact hourly date window from the message."
        }
        AssistantToolName::WeatherResolveLocationAlias => {
            " Args: required location; the backend resolves a canonical location and timezone."
        }
        AssistantToolName::WeatherGetForecastForDate => {
            " Args: required location; the backend derives the target forecast date from the message."
        }
        AssistantToolName::WeatherGetRecentHistoryForDate => {
            " Args: required location; the backend derives the target history date from the message."
        }
        AssistantToolName::WebListCuratedSources => " Args: none.",
        AssistantToolName::WebSearchPublicWeb => " Args: required query; optional category.",
        AssistantToolName::WebFetchPublicPageSummary => " Args: required url; optional category.",
        AssistantToolName::RoomsListActive => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsListJoinable => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsGetRoomSummary => " Args: required query, optional room_mode.",
        AssistantToolName::SystemGetCurrentDateTime => " Args: none.",
        AssistantToolName::SystemGetAiRuntimeSummary => " Args: none.",
        AssistantToolName::SystemGetKernelInfo => " Args: none.",
        AssistantToolName::SystemGetCpuTopology => " Args: none.",
        AssistantToolName::SystemGetTemperatureSensors => " Args: none.",
        AssistantToolName::SystemGetBlockDeviceInventory => " Args: none.",
        AssistantToolName::SystemGetFilesystemTable => " Args: none.",
        AssistantToolName::SystemGetGpuInventory => " Args: none.",
        AssistantToolName::SystemGetPciDevices => " Args: none.",
        AssistantToolName::SystemGetUsbDevices => " Args: none.",
        AssistantToolName::SystemGetBootLogSummary => " Args: none.",
        AssistantToolName::SystemGetJournalSummary => " Args: none.",
        AssistantToolName::SystemGetProcessDetail => {
            " Args: required query; the backend resolves one process by pid, name, or command line."
        }
        AssistantToolName::SystemGetListenerDetail => {
            " Args: required query; the backend resolves one listening socket or listener."
        }
        AssistantToolName::SystemGetDiskUsageDetail => {
            " Args: required query; the backend resolves one path or mount and returns exact disk usage."
        }
        AssistantToolName::SystemGetPortConflicts => {
            " Args: optional query; the backend lists listening sockets or port-in-use details."
        }
        AssistantToolName::SystemGetPortConflictDetail => {
            " Args: required query; the backend resolves one listening socket, port, or process listener."
        }
        AssistantToolName::SystemGetFailedUnits => {
            " Args: optional query; the backend lists failed systemd units or service failures."
        }
        AssistantToolName::SystemGetStoragePathDetail => {
            " Args: required query; the backend resolves one storage path, mount, or disk usage location."
        }
        AssistantToolName::SystemGetMountDetail => {
            " Args: required query; the backend resolves one mount point, filesystem, or backing mount."
        }
        AssistantToolName::MemoryListRecentFacts => {
            " Args: none; the backend lists recent stored memory facts for the signed-in user."
        }
        AssistantToolName::MemorySearchFacts => {
            " Args: required query; the backend resolves one stored memory fact subject."
        }
        AssistantToolName::MemorySearchEntities => {
            " Args: required query; the backend resolves one stored entity or person subject."
        }
        AssistantToolName::MemoryFindExactEntity => {
            " Args: required query; the backend resolves one exact stored entity match."
        }
        AssistantToolName::MemoryListRecentChanges => {
            " Args: optional query; the backend can return broad recent changes or narrow them to a subject."
        }
        AssistantToolName::MemoryListConflictingFacts => {
            " Args: optional query; the backend lists conflicting stored facts for a subject or topic."
        }
        AssistantToolName::MemoryGetEntityProvenance => {
            " Args: required query; the backend resolves one stored entity and returns its source provenance."
        }
        AssistantToolName::ServersListMinecraftStatus => {
            " Args: optional query, optional availability."
        }
        AssistantToolName::ServersGetMinecraftServerSummary => {
            " Args: required query, optional availability."
        }
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::AiListBackgroundJobs
        | AssistantToolName::AiGetJobStatus
        | AssistantToolName::AiGetToolRegistry
        | AssistantToolName::AiGetGroundingSummary
        | AssistantToolName::AiGetLastToolFailureReason
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetServiceDetail
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => " Args: none.",
        AssistantToolName::MemoryListRecentEntities => {
            " Args: none; the backend lists the signed-in user's recent stored entities."
        }
        AssistantToolName::MemoryGetPersonSummary => {
            " Args: required query; the backend resolves one stored person or profile entity and returns a grounded summary."
        }
        AssistantToolName::MemoryGetEntityRelations => {
            " Args: required query; the backend resolves one stored entity and loads its immediate relations."
        }
        AssistantToolName::MemoryGetEntityRelationPath => {
            " Args: required source and target entity queries joined with ||; the backend resolves a bounded path between them."
        }
        _ => " Args: none.",
    }
}

fn planner_history_summary(history: &[AssistantHistoryMessage]) -> String {
    let recent = history
        .iter()
        .rev()
        .take(PLANNER_HISTORY_MESSAGE_LIMIT)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        return "none".to_string();
    }

    recent
        .into_iter()
        .rev()
        .map(|message| {
            let content = message.content.replace('\n', " ");
            format!(
                "- {}: {}",
                message.role,
                truncate_for_planner(&content, 220)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_for_planner(content: &str, max_chars: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    trimmed.chars().take(max_chars).collect::<String>() + "..."
}

fn parse_planner_ast(raw: &str) -> Result<PlannerAst, Vec<PlannerIssue>> {
    let cleaned = strip_markdown_code_fence(raw);
    if let Ok(ast) = serde_json::from_str::<PlannerAst>(cleaned) {
        return Ok(ast);
    }
    let Some(candidate) = extract_json_object(cleaned) else {
        return Err(vec![planner_issue(
            "parse_failed",
            "planner output did not contain a JSON object",
            None,
        )]);
    };
    serde_json::from_str::<PlannerAst>(&candidate).map_err(|error| {
        vec![planner_issue(
            "parse_failed",
            &format!("planner JSON did not match the required schema: {error}"),
            None,
        )]
    })
}

fn strip_markdown_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix("```") {
        let stripped = stripped.strip_prefix("json").unwrap_or(stripped);
        let stripped = stripped.strip_prefix('\n').unwrap_or(stripped);
        return stripped.strip_suffix("```").unwrap_or(stripped).trim();
    }
    trimmed
}

fn extract_json_object(raw: &str) -> Option<String> {
    let mut start = None;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if start.is_none() {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    let start = start?;
                    return Some(raw[start..=index].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

fn validate_planner_ast(
    ast: &PlannerAst,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Result<Vec<PlannedToolCall>, Vec<PlannerIssue>> {
    if is_tool_inventory_query(&message.to_ascii_lowercase()) {
        return Ok(Vec::new());
    }

    if matches!(ast, PlannerAst::None { tools } if !tools.is_empty()) {
        return Err(vec![planner_issue(
            "unsupported_combination",
            "mode none cannot include tool calls",
            Some("tools"),
        )]);
    }

    if ast.tools().len() > MAX_TOOL_CALLS_PER_TURN {
        return Err(vec![planner_issue(
            "tool_count_exceeded",
            &format!(
                "planner proposed {} tools but the limit is {MAX_TOOL_CALLS_PER_TURN}",
                ast.tools().len()
            ),
            Some("tools"),
        )]);
    }

    if matches!(ast, PlannerAst::None { .. }) {
        return Ok(Vec::new());
    }

    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    let mut seen_domains = HashSet::new();
    let mut issues = Vec::new();
    for (index, tool) in ast.tools().iter().enumerate() {
        let path = format!("tools[{index}]");
        let Some(parsed_tool) = AssistantToolName::from_str(&tool.tool.to_ascii_lowercase()) else {
            issues.push(planner_issue(
                "unknown_tool",
                &format!("unknown planner tool `{}`", tool.tool),
                Some(&format!("{path}.tool")),
            ));
            continue;
        };
        if !tool_visible_to_user(parsed_tool, user) {
            issues.push(planner_issue(
                "tool_not_allowed",
                &format!(
                    "tool `{}` is not allowed for this user",
                    parsed_tool.as_str()
                ),
                Some(&format!("{path}.tool")),
            ));
            continue;
        }
        if !matches!(
            parsed_tool.spec().access_mode,
            super::types::ToolAccessMode::ReadOnly
        ) {
            issues.push(planner_issue(
                "tool_not_allowed",
                &format!(
                    "tool `{}` is confirmation-gated and cannot be planned by the model",
                    parsed_tool.as_str()
                ),
                Some(&format!("{path}.tool")),
            ));
            continue;
        }
        if matches!(
            parsed_tool,
            AssistantToolName::WeatherGetCurrent
                | AssistantToolName::WeatherGetForecast
                | AssistantToolName::WeatherGetHistory
        ) {
            if !message_allows_weather_tool(message, history) {
                issues.push(planner_issue(
                    "unsupported_combination",
                    "weather tools are not valid for this message",
                    Some(&format!("{path}.tool")),
                ));
                continue;
            }
            let merged_args = tool.merged_args();
            let Some(location) = normalize_optional_query(merged_args.query.clone())
                .or_else(|| extract_weather_location(message))
            else {
                issues.push(planner_issue(
                    "missing_required_argument",
                    "weather tool requires a location",
                    Some(&format!("{path}.args.query")),
                ));
                continue;
            };
            let Some((weather_tool, weather_input)) =
                weather_tool_call_for_location(message, location)
            else {
                issues.push(planner_issue(
                    "invalid_argument",
                    "weather tool arguments could not be normalized",
                    Some(&format!("{path}.args.query")),
                ));
                continue;
            };
            let dedupe_key = format!(
                "{}:{}",
                weather_tool.as_str(),
                serde_json::to_string(&weather_input).unwrap_or_default()
            );
            if !seen.insert(dedupe_key) {
                issues.push(planner_issue(
                    "duplicate_tool",
                    "duplicate tool calls are not allowed",
                    Some(&path),
                ));
                continue;
            }
            planned.push(PlannedToolCall {
                tool: weather_tool,
                input: weather_input,
            });
            continue;
        }
        match normalize_planner_tool_input(parsed_tool, &tool.merged_args(), message) {
            Ok(input) => {
                let dedupe_key = format!(
                    "{}:{}",
                    parsed_tool.as_str(),
                    serde_json::to_string(&input).unwrap_or_default()
                );
                if !seen.insert(dedupe_key) {
                    issues.push(planner_issue(
                        "duplicate_tool",
                        "duplicate tool calls are not allowed",
                        Some(&path),
                    ));
                    continue;
                }
                if let Some(domain) = unsupported_combo_domain(parsed_tool) {
                    if !seen_domains.insert(domain) {
                        issues.push(planner_issue(
                            "unsupported_combination",
                            "planner proposed multiple overlapping tools for the same domain",
                            Some(&path),
                        ));
                        continue;
                    }
                }
                planned.push(PlannedToolCall {
                    tool: parsed_tool,
                    input,
                });
            }
            Err(issue) => issues.push(issue.with_path_prefix(&path)),
        }
    }

    if issues.is_empty() {
        Ok(planned)
    } else {
        Err(issues)
    }
}

fn message_allows_weather_tool(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    let lower = message.to_ascii_lowercase();
    if is_current_datetime_query(&lower) || message_has_current_datetime_follow_up_hint(message) {
        return false;
    }

    let has_recent_weather = recent_weather_hint(history).is_some();
    is_weather_query(&lower)
        || extract_weather_location(message).is_some()
        || (has_recent_weather
            && (extract_standalone_weather_location(message).is_some()
                || message_has_weather_follow_up_hint(&lower)))
}

fn tool_visible_to_user(tool: AssistantToolName, user: &AuthUser) -> bool {
    if matches!(
        tool,
        AssistantToolName::WebListCuratedSources
            | AssistantToolName::WebSearchPublicWeb
            | AssistantToolName::WebFetchPublicPageSummary
    ) && !public_web_tools_enabled()
    {
        return false;
    }

    match tool.spec().required_role {
        super::types::ToolRoleRequirement::AnyAuthenticatedUser => true,
        super::types::ToolRoleRequirement::AdminOnly => user.role == "admin",
    }
}

fn normalize_planner_tool_input(
    tool: AssistantToolName,
    args: &PlannerAstArgs,
    message: &str,
) -> Result<AssistantToolInput, PlannerIssue> {
    match tool {
        AssistantToolName::CalendarCreateEvent
        | AssistantToolName::CalendarCreateBirthday
        | AssistantToolName::CalendarDeleteEvent
        | AssistantToolName::DocumentCreateDownload
        | AssistantToolName::ConversationsArchiveSelection
        | AssistantToolName::ConversationsDeleteSelection
        | AssistantToolName::ConversationsMoveToGroupSelection => Err(planner_issue(
            "tool_not_allowed",
            "write tools are not allowed in model planning",
            None,
        )),
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::AiListBackgroundJobs
        | AssistantToolName::AiGetJobStatus
        | AssistantToolName::AiGetToolRegistry
        | AssistantToolName::AiGetGroundingSummary
        | AssistantToolName::AiGetLastToolFailureReason
        | AssistantToolName::MemoryListRecentFacts
        | AssistantToolName::MemoryListRecentEntities
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::NetworkGetTopologySummary
        | AssistantToolName::NetworkGetDefaultRoute
        | AssistantToolName::NetworkGetHostnameAliases
        | AssistantToolName::CalendarGetNextEvent
        | AssistantToolName::CalendarGetNextEventTiming
        | AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetAiRuntimeSummary
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => Ok(AssistantToolInput::None),
        AssistantToolName::DictionaryListVisibleWorkspaces => {
            Ok(AssistantToolInput::DictionaryListVisibleWorkspaces)
        }
        AssistantToolName::DictionaryBrowseWorkspacePeople => {
            let workspace_id = args
                .workspace_id
                .clone()
                .or_else(|| extract_dictionary_workspace_selector(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "dictionary workspace browse requires a workspace_id selector such as family, friends, or work",
                        Some("args.workspace_id"),
                    )
                })?;
            Ok(AssistantToolInput::DictionaryBrowseWorkspacePeople {
                workspace_id,
                query: normalize_optional_query(args.query.clone())
                    .or_else(|| extract_dictionary_workspace_people_query(message)),
                limit: args.limit,
            })
        }
        AssistantToolName::DictionarySearchPeople => {
            let workspace_id = args
                .workspace_id
                .clone()
                .or_else(|| extract_dictionary_workspace_selector(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "dictionary people search requires a workspace_id",
                        Some("args.workspace_id"),
                    )
                })?;
            let query = normalize_optional_query(args.query.clone())
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "dictionary people search requires a query",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::DictionarySearchPeople {
                workspace_id,
                query,
                limit: args.limit,
            })
        }
        AssistantToolName::DictionaryGetPersonBundle => {
            let workspace_id = args.workspace_id.clone().ok_or_else(|| {
                planner_issue(
                    "missing_required_argument",
                    "dictionary person bundle requires a workspace_id",
                    Some("args.workspace_id"),
                )
            })?;
            let person_id = args.person_id.clone().ok_or_else(|| {
                planner_issue(
                    "missing_required_argument",
                    "dictionary person bundle requires a person_id",
                    Some("args.person_id"),
                )
            })?;
            Ok(AssistantToolInput::DictionaryGetPersonBundle {
                workspace_id,
                person_id,
            })
        }
        AssistantToolName::DictionaryResolveRelationshipReference => {
            let reference = args
                .reference
                .clone()
                .or_else(|| extract_dictionary_relationship_reference(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "dictionary relationship resolution requires a relationship reference such as my mother",
                        Some("args.reference"),
                    )
                })?;
            Ok(AssistantToolInput::DictionaryResolveRelationshipReference {
                reference,
                workspace_id: args.workspace_id.clone(),
            })
        }
        AssistantToolName::CalendarGetEventByExactDateAndTitle
        | AssistantToolName::CalendarGetEventSeriesSummary => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_calendar_event_detail_query(message))
                .or_else(|| extract_quoted_phrase(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "calendar series and exact-date detail queries require a specific event reference",
                        Some("args.query"),
                    )
                })?;
            Ok(extract_calendar_window(message, 30, Some(query)))
        }
        AssistantToolName::CalendarGetNextFreeSlot | AssistantToolName::CalendarListBusySlots => {
            Ok(extract_calendar_window(message, 7, None))
        }
        AssistantToolName::SystemGetPortConflicts => Ok(AssistantToolInput::SystemPortConflicts {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_port_conflicts_query(message)),
        }),
        AssistantToolName::SystemGetFailedUnits => Ok(AssistantToolInput::SystemFailedUnits {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_failed_units_query(message)),
        }),
        AssistantToolName::SystemGetFailedUnitDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_failed_unit_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "failed unit detail queries require a specific failed unit or service reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemFailedUnits { query: Some(query) })
        }
        AssistantToolName::MemoryListRecentChanges => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_recent_changes_query(message));
            Ok(match query {
                Some(query) => AssistantToolInput::SystemService { query },
                None => AssistantToolInput::None,
            })
        }
        AssistantToolName::MemoryListConflictingFacts => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_conflict_query(message));
            Ok(match query {
                Some(query) => AssistantToolInput::SystemService { query },
                None => AssistantToolInput::None,
            })
        }
        AssistantToolName::MemoryGetPersonSummary => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_exact_entity_query(message))
                .or_else(|| extract_memory_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "person summary queries require a specific person or profile reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::MemorySearchFacts | AssistantToolName::MemorySearchEntities => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "memory search queries require a subject to search for",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::MemoryFindExactEntity => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_exact_entity_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "exact memory entity queries require a specific subject to look up",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::MemoryGetEntityRelations => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_relation_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "memory relation queries require a subject to load",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::MemoryGetEntityRelationPath => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| {
                    extract_memory_relation_path_query(message)
                        .map(|(source, target)| format!("{source} || {target}"))
                })
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "memory relation path queries require two related subjects",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::MemoryGetEntityProvenance => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_memory_provenance_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "memory provenance queries require a specific subject to look up",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::SystemGetServiceDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_service_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "service detail queries require a service reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::SystemGetMountDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_mount_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "mount detail queries require a mount point, filesystem, or backing path reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::SystemGetStoragePathDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_storage_path_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "storage path detail queries require a storage path or mount reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::SystemGetPortConflictDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_port_conflict_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "port conflict detail queries require a specific port or listener reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemPortConflicts { query: Some(query) })
        }
        AssistantToolName::SystemGetProcessDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_process_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "process detail queries require a process, pid, or command line reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::SystemGetListenerDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_listener_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "listener detail queries require a socket, port, or listener reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemPortConflicts { query: Some(query) })
        }
        AssistantToolName::SystemGetDiskUsageDetail => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_disk_usage_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "disk usage detail queries require a path, mount, or filesystem reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::SystemService { query })
        }
        AssistantToolName::CalendarListEvents
        | AssistantToolName::CalendarListDateConflicts
        | AssistantToolName::CalendarListFreeDays
        | AssistantToolName::CalendarCountEvents
        | AssistantToolName::CalendarListBusyDays
        | AssistantToolName::CalendarListOverlappingEvents => {
            Ok(extract_calendar_window(message, 7, None))
        }
        AssistantToolName::CalendarGetNextFreeDay => Ok(extract_calendar_window(message, 30, None)),
        AssistantToolName::CalendarUpcomingBirthdays => Ok(birthday_calendar_window_input(
            message,
            args.query
                .clone()
                .and_then(|query| normalize_birthday_query_candidate(&query))
                .or_else(|| extract_birthday_query(message)),
        )),
        AssistantToolName::CalendarGetEventDetails => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_calendar_event_detail_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "calendar detail queries require a specific event reference",
                        Some("args.query"),
                    )
                })?;
            Ok(extract_calendar_window(message, 30, Some(query)))
        }
        AssistantToolName::ChannelsListUnreadActivity => Ok(AssistantToolInput::ChannelsFilter {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_channel_query(message)),
        }),
        AssistantToolName::ChannelsGetTranscriptSummary => Ok(AssistantToolInput::ChannelsFilter {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_transcript_channel_query(message)),
        }),
        AssistantToolName::DownloadsListAvailableArtifacts => {
            let availability = validated_downloads_availability(args.availability.as_deref())?
                .or_else(|| extract_downloads_availability(message));
            Ok(AssistantToolInput::DownloadsFilter {
                query: normalize_optional_query(args.query.clone())
                    .or_else(|| extract_downloads_follow_up_query(message))
                    .or_else(|| extract_downloads_query(message)),
                availability,
            })
        }
        AssistantToolName::DownloadsGetArtifactDetails => Ok(AssistantToolInput::DownloadsFilter {
            query: Some(
                normalize_optional_query(args.query.clone())
                    .or_else(|| extract_download_artifact_detail_query(message))
                    .or_else(|| extract_downloads_follow_up_query(message))
                    .or_else(|| extract_downloads_query(message))
                    .ok_or_else(|| {
                        planner_issue(
                            "missing_required_argument",
                            "download artifact detail queries require an artifact reference",
                            Some("args.query"),
                        )
                    })?,
            ),
            availability: validated_downloads_availability(args.availability.as_deref())?
                .or_else(|| extract_downloads_availability(message)),
        }),
        AssistantToolName::DownloadsGetArtifactSource
        | AssistantToolName::DownloadsGetReleaseNotes => {
            Ok(AssistantToolInput::DownloadsFilter {
                query: Some(
                    normalize_optional_query(args.query.clone())
                        .or_else(|| extract_download_artifact_source_query(message))
                        .or_else(|| extract_download_artifact_release_notes_query(message))
                        .or_else(|| extract_download_artifact_detail_query(message))
                        .or_else(|| extract_downloads_follow_up_query(message))
                        .or_else(|| extract_downloads_query(message))
                        .ok_or_else(|| {
                            planner_issue(
                                "missing_required_argument",
                                "download artifact source and release-note queries require an artifact reference",
                                Some("args.query"),
                            )
                        })?,
                ),
                availability: validated_downloads_availability(args.availability.as_deref())?
                    .or_else(|| extract_downloads_availability(message)),
            })
        }
        AssistantToolName::DownloadsGetArtifactChecksum
        | AssistantToolName::DownloadsGetArtifactInstallSteps
        | AssistantToolName::DownloadsGetArtifactCompatibility => {
            Ok(AssistantToolInput::DownloadsFilter {
                query: Some(
                    normalize_optional_query(args.query.clone())
                        .or_else(|| extract_download_artifact_detail_query(message))
                        .or_else(|| extract_downloads_follow_up_query(message))
                        .or_else(|| extract_downloads_query(message))
                        .ok_or_else(|| {
                            planner_issue(
                                "missing_required_argument",
                                "download artifact detail queries require an artifact reference",
                                Some("args.query"),
                            )
                        })?,
                ),
                availability: validated_downloads_availability(args.availability.as_deref())?
                    .or_else(|| extract_downloads_availability(message)),
            })
        }
        AssistantToolName::NetworkGetInterfaceByIp => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_network_interface_ip_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "network interface by IP queries require an exact IP address",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::NetworkInterface { query })
        }
        AssistantToolName::NetworkGetInterfaceDetails => {
            let query = normalize_optional_query(args.query.clone())
                .or_else(|| extract_network_interface_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "network interface detail queries require an interface reference",
                        Some("args.query"),
                    )
                })?;
            Ok(AssistantToolInput::NetworkInterface { query })
        }
        AssistantToolName::NetworkGetDnsServers => Ok(AssistantToolInput::NetworkDnsServers {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_network_dns_servers_query(message)),
        }),
        AssistantToolName::LibrariesGetLibrarySummary => Ok(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_library_detail_query(message))
                .or_else(|| extract_library_follow_up_query(message))
                .or_else(|| extract_library_search_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "library summary queries require a library reference",
                        Some("args.query"),
                    )
                })?,
        }),
        AssistantToolName::LibrarySearchTitles => Ok(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_library_search_query(message))
                .or_else(|| extract_library_follow_up_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "library search requires a title query",
                        Some("args.query"),
                    )
                })?,
        }),
        AssistantToolName::LibraryGetItemSummary => Ok(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_quoted_phrase(message))
                .or_else(|| extract_library_follow_up_query(message))
                .or_else(|| extract_library_search_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "library detail queries require a title",
                        Some("args.query"),
                    )
                })?,
        }),
        AssistantToolName::LibraryGetItemMediaDetails => Ok(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_quoted_phrase(message))
                .or_else(|| extract_library_media_detail_query(message))
                .or_else(|| extract_library_follow_up_query(message))
                .or_else(|| extract_library_search_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "library media detail queries require a title",
                        Some("args.query"),
                    )
                })?,
        }),
        AssistantToolName::LibraryGetItemSourcePaths => Ok(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_quoted_phrase(message))
                .or_else(|| extract_library_source_paths_query(message))
                .or_else(|| extract_library_media_detail_query(message))
                .or_else(|| extract_library_follow_up_query(message))
                .or_else(|| extract_library_search_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "library source path queries require a title",
                        Some("args.query"),
                    )
                })?,
        }),
        AssistantToolName::LibrariesGetRecentlyAdded => Ok(AssistantToolInput::LibraryRecent {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_recent_library_query(message)),
        }),
        AssistantToolName::LibrariesFindDuplicateTitles
        | AssistantToolName::LibrariesListMissingMetadata => Ok(AssistantToolInput::None),
        AssistantToolName::WeatherGetHourlyWindow => {
            let location = normalize_optional_query(args.query.clone())
                .or_else(|| extract_weather_location(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "weather hourly window queries require a location",
                        Some("args.query"),
                    )
                })?;
            let today = assistant_local_today();
            let (date, label) = extract_single_calendar_date(message, today).unwrap_or((
                today,
                "today".to_string(),
            ));
            Ok(AssistantToolInput::WeatherHistory {
                location,
                start_date: date.format("%F").to_string(),
                end_date: date.format("%F").to_string(),
                label,
            })
        }
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory
        | AssistantToolName::WeatherResolveLocationAlias
        | AssistantToolName::WeatherGetForecastForDate
        | AssistantToolName::WeatherGetRecentHistoryForDate => {
            normalize_optional_query(args.query.clone())
                .or_else(|| extract_weather_location(message))
                .and_then(|location| weather_tool_input_for_location(message, location))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "weather tools require a location",
                        Some("args.query"),
                    )
                })
        }
        AssistantToolName::WebListCuratedSources => Ok(AssistantToolInput::None),
        AssistantToolName::WebSearchPublicWeb => Ok(AssistantToolInput::WebSearch {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_public_web_search_query(message))
                .ok_or_else(|| {
                    planner_issue(
                        "missing_required_argument",
                        "web search requires a query",
                        Some("args.query"),
                    )
                })?,
            category: normalize_optional_query(args.category.clone())
                .and_then(|category| validate_curated_web_category_slug(&category))
                .or_else(|| infer_curated_web_category_slug(message)),
        }),
        AssistantToolName::WebFetchPublicPageSummary => {
            let url = validated_public_web_url(
                normalize_optional_query(args.url.clone())
                    .or_else(|| normalize_optional_query(args.query.clone()))
                    .or_else(|| extract_public_web_url(message))
                    .ok_or_else(|| {
                        planner_issue(
                            "missing_required_argument",
                            "web fetch requires an explicit public URL",
                            Some("args.url"),
                        )
                    })?,
            )?;
            let category = normalize_optional_query(args.category.clone())
                .and_then(|category| validate_curated_web_category_slug(&category))
                .or_else(|| infer_curated_web_category_for_url(&url));
            Ok(AssistantToolInput::WebFetch { url, category })
        }
        AssistantToolName::RoomsListActive => Ok(AssistantToolInput::RoomsFilter {
            room_mode: validated_room_mode(args.room_mode.as_deref())?
                .or_else(|| detect_room_mode(message)),
            query: normalize_optional_query(args.query.clone()),
        }),
        AssistantToolName::RoomsListJoinable => Ok(AssistantToolInput::RoomsFilter {
            room_mode: validated_room_mode(args.room_mode.as_deref())?
                .or_else(|| detect_room_mode(message)),
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_room_query(message)),
        }),
        AssistantToolName::RoomsGetRoomSummary => Ok(AssistantToolInput::RoomsFilter {
            room_mode: validated_room_mode(args.room_mode.as_deref())?
                .or_else(|| detect_room_mode(message)),
            query: Some(
                normalize_optional_query(args.query.clone())
                    .or_else(|| extract_room_query(message))
                    .or_else(|| extract_quoted_phrase(message))
                    .ok_or_else(|| {
                        planner_issue(
                            "missing_required_argument",
                            "room detail queries require a room reference",
                            Some("args.query"),
                        )
                    })?,
            ),
        }),
        AssistantToolName::ServersListMinecraftStatus => Ok(AssistantToolInput::ServerFilter {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_server_query(message)),
            availability: validated_server_availability(args.availability.as_deref())?
                .or_else(|| extract_server_availability(message)),
        }),
        AssistantToolName::ServersGetMinecraftServerSummary => {
            Ok(AssistantToolInput::ServerFilter {
                query: Some(
                    normalize_optional_query(args.query.clone())
                        .or_else(|| extract_server_query(message))
                        .or_else(|| extract_quoted_phrase(message))
                        .filter(|query| !query.is_empty())
                        .ok_or_else(|| {
                            planner_issue(
                                "missing_required_argument",
                                "server detail queries require a server reference",
                                Some("args.query"),
                            )
                        })?,
                ),
                availability: validated_server_availability(args.availability.as_deref())?
                    .or_else(|| extract_server_availability(message)),
            })
        }
        _ => Err(planner_issue(
            "unsupported_tool",
            "tool input normalization is not implemented for this tool",
            None,
        )),
    }
}

fn validated_room_mode(room_mode: Option<&str>) -> Result<Option<String>, PlannerIssue> {
    match room_mode {
        Some(raw) => normalize_room_mode(Some(raw))
            .ok_or_else(|| {
                planner_issue(
                    "invalid_enum",
                    "room_mode must be one of video, audio, youtube, web, screen, create, or play",
                    Some("args.room_mode"),
                )
            })
            .map(Some),
        None => Ok(None),
    }
}

fn validated_downloads_availability(
    availability: Option<&str>,
) -> Result<Option<String>, PlannerIssue> {
    match availability {
        Some(raw) => normalize_downloads_availability(Some(raw))
            .ok_or_else(|| {
                planner_issue(
                    "invalid_enum",
                    "availability must be one of available, planned, or unavailable",
                    Some("args.availability"),
                )
            })
            .map(Some),
        None => Ok(None),
    }
}

fn validated_server_availability(
    availability: Option<&str>,
) -> Result<Option<String>, PlannerIssue> {
    match availability {
        Some(raw) => normalize_server_availability(Some(raw))
            .ok_or_else(|| {
                planner_issue(
                    "invalid_enum",
                    "availability must be one of online, offline, healthy, or problem",
                    Some("args.availability"),
                )
            })
            .map(Some),
        None => Ok(None),
    }
}

fn validated_public_web_url(raw_url: String) -> Result<String, PlannerIssue> {
    normalize_public_url(&raw_url)
        .map(|url| url.to_string())
        .map_err(|error| planner_issue("invalid_argument", &error, Some("args.url")))
}

fn unsupported_combo_domain(tool: AssistantToolName) -> Option<&'static str> {
    match tool {
        AssistantToolName::CalendarListEvents
        | AssistantToolName::CalendarGetNextEvent
        | AssistantToolName::CalendarGetNextEventTiming
        | AssistantToolName::CalendarCountEvents
        | AssistantToolName::CalendarListBusyDays
        | AssistantToolName::CalendarUpcomingBirthdays
        | AssistantToolName::CalendarGetEventDetails
        | AssistantToolName::CalendarListDateConflicts
        | AssistantToolName::CalendarListFreeDays => Some("calendar"),
        AssistantToolName::DownloadsListAvailableArtifacts
        | AssistantToolName::DownloadsGetArtifactDetails => Some("downloads"),
        AssistantToolName::NetworkGetTopologySummary
        | AssistantToolName::NetworkGetInterfaceDetails
        | AssistantToolName::NetworkGetInterfaceByIp
        | AssistantToolName::NetworkGetDnsServers
        | AssistantToolName::NetworkGetDefaultRoute
        | AssistantToolName::NetworkGetHostnameAliases
        | AssistantToolName::NetworkGetRouteTable
        | AssistantToolName::NetworkGetActiveConnections
        | AssistantToolName::NetworkGetInterfaceCounters
        | AssistantToolName::NetworkGetWifiStatus
        | AssistantToolName::NetworkGetVpnStatus => Some("network"),
        AssistantToolName::LibrarySearchTitles
        | AssistantToolName::LibraryGetItemSummary
        | AssistantToolName::LibraryGetItemMediaDetails
        | AssistantToolName::LibrariesGetRecentlyAdded
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::LibrariesGetLibrarySummary => Some("library"),
        AssistantToolName::MemoryListRecentFacts
        | AssistantToolName::MemoryListRecentEntities
        | AssistantToolName::MemorySearchFacts
        | AssistantToolName::MemorySearchEntities
        | AssistantToolName::MemoryGetEntityRelations
        | AssistantToolName::MemoryListRecentChanges
        | AssistantToolName::MemoryListConflictingFacts
        | AssistantToolName::MemoryGetEntityProvenance => Some("memory"),
        AssistantToolName::RoomsListActive
        | AssistantToolName::RoomsListJoinable
        | AssistantToolName::RoomsGetRoomSummary => Some("rooms"),
        AssistantToolName::ServersListMinecraftStatus
        | AssistantToolName::ServersGetMinecraftServerSummary => Some("servers"),
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory
        | AssistantToolName::WeatherGetHourlyWindow => Some("weather"),
        AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetAiRuntimeSummary
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetServiceDetail
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetStoragePathDetail
        | AssistantToolName::SystemGetRecentErrors
        | AssistantToolName::SystemGetKernelInfo
        | AssistantToolName::SystemGetCpuTopology
        | AssistantToolName::SystemGetTemperatureSensors
        | AssistantToolName::SystemGetBlockDeviceInventory
        | AssistantToolName::SystemGetFilesystemTable
        | AssistantToolName::SystemGetGpuInventory
        | AssistantToolName::SystemGetPciDevices
        | AssistantToolName::SystemGetUsbDevices
        | AssistantToolName::SystemGetBootLogSummary
        | AssistantToolName::SystemGetJournalSummary
        | AssistantToolName::SystemGetPortConflicts
        | AssistantToolName::SystemGetFailedUnits
        | AssistantToolName::SystemGetFailedUnitDetail => Some("system"),
        _ => None,
    }
}

fn planner_issue(code: &str, message: &str, path: Option<&str>) -> PlannerIssue {
    PlannerIssue {
        code: code.to_string(),
        message: message.to_string(),
        path: path.map(str::to_string),
    }
}

trait PlannerIssuePathExt {
    fn with_path_prefix(self, prefix: &str) -> Self;
}

impl PlannerIssuePathExt for PlannerIssue {
    fn with_path_prefix(mut self, prefix: &str) -> Self {
        self.path = match self.path {
            Some(path) if path.starts_with("tools[") => Some(path),
            Some(path) => Some(format!("{prefix}.{path}")),
            None => Some(prefix.to_string()),
        };
        self
    }
}

fn normalize_optional_query(query: Option<String>) -> Option<String> {
    query
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_room_mode(room_mode: Option<&str>) -> Option<String> {
    match room_mode?.trim().to_ascii_lowercase().as_str() {
        "video" | "watch" => Some("video".to_string()),
        "audio" | "music" => Some("audio".to_string()),
        "youtube" => Some("youtube".to_string()),
        "web" => Some("web".to_string()),
        "screen" => Some("screen".to_string()),
        "create" => Some("create".to_string()),
        "play" => Some("play".to_string()),
        _ => None,
    }
}

fn normalize_downloads_availability(availability: Option<&str>) -> Option<String> {
    match availability?.trim().to_ascii_lowercase().as_str() {
        "available" => Some("available".to_string()),
        "planned" | "coming_soon" | "future" => Some("planned".to_string()),
        "unavailable" => Some("unavailable".to_string()),
        _ => None,
    }
}

fn normalize_server_availability(availability: Option<&str>) -> Option<String> {
    match availability?.trim().to_ascii_lowercase().as_str() {
        "online" | "running" => Some("online".to_string()),
        "offline" | "stopped" => Some("offline".to_string()),
        "healthy" => Some("healthy".to_string()),
        "problem" | "failed" | "error" | "errors" | "broken" | "unhealthy" => {
            Some("problem".to_string())
        }
        _ => None,
    }
}

fn build_system_prompt() -> String {
    "You are the Rustyfin assistant — a helpful AI built into a personal home media server. \
Be concise and genuinely helpful. Respond in plain text unless code or markdown lists add real clarity. \
If authoritative Rustyfin grounding is supplied in another system message, treat it as the source of truth for this turn. \
Do not invent data that was not grounded. If a grounded tool reports an error or missing data, say so plainly. \
Do not claim to have created, updated, deleted, or changed anything in Rustyfin unless a confirmed server-side write tool actually ran and the backend verified the result. \
If the user is just chatting, joking, roleplaying, asking for tone changes, or doing simple math, answer directly instead of pretending Rustyfin data was used. \
Do not dump extremely long walls of text, digits, or copied content when a concise answer or a narrower follow-up would be better."
        .to_string()
}

pub fn immediate_response_for_message(message: &str) -> Option<String> {
    oversized_numeric_dump_response(message).or_else(|| clarification_for_message(message))
}

pub fn unsafe_action_response_for_message(message: &str) -> Option<String> {
    if !is_unsafe_destructive_host_request(&message.to_ascii_lowercase()) {
        return None;
    }

    Some(
        "I can’t help delete system files, format a computer, wipe disks, or perform other destructive host actions."
            .to_string(),
    )
}

pub fn unsupported_write_response_for_message(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if is_supported_calendar_create_intent(&lower)
        || is_supported_calendar_delete_intent(&lower)
        || is_supported_document_create_intent(&lower)
        || is_supported_conversation_manage_intent(&lower)
    {
        return None;
    }
    if !is_unsupported_write_intent(&lower) {
        return None;
    }

    if has_any(
        &lower,
        &["calendar", "event", "events", "birthday", "birthdays"],
    ) {
        return Some(
            "I can view your calendar right now, but I can't create or edit calendar entries yet through Rustyfin AI.".to_string(),
        );
    }
    if has_any(&lower, &["room", "rooms"]) {
        return Some(
            "I can inspect Rustyfin rooms right now, but I can't create, rename, or delete rooms yet through Rustyfin AI.".to_string(),
        );
    }
    if has_any(&lower, &["server", "servers", "minecraft"]) {
        return Some(
            "I can read Minecraft server state right now, but I can't change server records or runtime state yet through Rustyfin AI.".to_string(),
        );
    }
    if has_any(&lower, &["channel", "channels"]) {
        return Some(
            "I can read channel activity right now, but I can't create, rename, or delete channels yet through Rustyfin AI.".to_string(),
        );
    }
    if has_any(&lower, &["conversation", "conversations", "chat", "chats"]) {
        return Some(
            "I can archive, delete, or move your AI conversations into groups after explicit confirmation, but I can't rename them through Rustyfin AI yet.".to_string(),
        );
    }

    Some(
        "I can read Rustyfin data right now, but I can't create, edit, or delete data yet through Rustyfin AI.".to_string(),
    )
}

fn clarification_for_message(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if is_non_birthday_calendar_query(&lower)
        && !is_next_calendar_event_query(&lower)
        && !calendar_query_has_explicit_window(&lower)
    {
        return Some(
            "What time window should I check for your calendar? Try today, tomorrow, this week, next week, this month, or a specific date like 2026-03-22.".to_string(),
        );
    }
    if is_weather_query(&lower) && extract_weather_location(message).is_none() {
        return Some(
            "Which location should I check the weather for? Try a place name like Dublin, Cork, or Galway.".to_string(),
        );
    }
    if is_ambiguous_server_query(&lower, message) {
        return Some(
            "Which Minecraft server should I check? Say the server name, for example \"Survival\", or ask for all servers that are online or offline.".to_string(),
        );
    }
    None
}

fn is_unsupported_write_intent(message_lower: &str) -> bool {
    if has_any(
        message_lower,
        &[
            "how do i ",
            "how can i ",
            "can i ",
            "is it possible to ",
            "do you support ",
            "does rustyfin ai support ",
        ],
    ) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "add ",
            "create ",
            "make ",
            "save ",
            "schedule ",
            "update ",
            "edit ",
            "change ",
            "modify ",
            "rename ",
            "delete ",
            "remove ",
            "archive ",
            "unarchive ",
            "restore ",
            "cancel ",
        ],
    ) && has_any(
        message_lower,
        &[
            "calendar",
            "event",
            "events",
            "birthday",
            "birthdays",
            "room",
            "rooms",
            "server",
            "servers",
            "channel",
            "channels",
            "library",
            "libraries",
            "download",
            "downloads",
        ],
    )
}

fn is_non_birthday_calendar_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "calendar",
            "event",
            "events",
            "schedule",
            "scheduled",
            "what do i have on",
            "what's on my calendar",
            "whats on my calendar",
        ],
    ) && !has_any(
        message_lower,
        &[
            "birthday",
            "birthdays",
            "who has a birthday",
            "upcoming birthday",
        ],
    )
}

fn calendar_query_has_explicit_window(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "today",
            "tomorrow",
            "this week",
            "next week",
            "this month",
            "next month",
            "this weekend",
            "weekend",
            "coming up",
            "upcoming",
            "soon",
        ],
    ) || extract_next_numbered_window(message_lower, "day", "days").is_some()
        || extract_next_numbered_window(message_lower, "week", "weeks").is_some()
        || extract_next_numbered_window(message_lower, "month", "months").is_some()
        || extract_single_calendar_date(message_lower, assistant_local_today()).is_some()
}

fn is_ambiguous_server_query(message_lower: &str, message: &str) -> bool {
    has_any(
        message_lower,
        &["the server", "my server", "that server", "is server online"],
    ) && has_any(
        message_lower,
        &[
            "server",
            "minecraft",
            "online",
            "offline",
            "running",
            "stopped",
            "healthy",
            "failed",
        ],
    ) && !message_lower.contains("servers")
        && extract_server_query(message).is_none()
}

fn is_system_kernel_info_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "kernel",
            "kernel info",
            "kernel and os",
            "operating system",
            "os release",
            "uname",
            "distribution",
            "distro",
            "platform",
            "base platform",
            "linux version",
        ],
    )
}

fn is_system_cpu_topology_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "cpu topology",
            "cpu sockets",
            "logical cpu",
            "logical cpus",
            "physical core",
            "physical cores",
            "socket",
            "sockets",
            "core count",
            "thread count",
            "lscpu",
        ],
    )
}

fn is_system_temperature_sensors_query(message_lower: &str) -> bool {
    if has_any(
        message_lower,
        &[
            "temperature sensors",
            "thermal",
            "thermal sensors",
            "thermal zone",
            "hwmon",
            "overheating",
        ],
    ) {
        return true;
    }

    if is_weather_query(message_lower) {
        return false;
    }

    has_any(message_lower, &["temperature", "temperatures"])
}

fn is_system_block_device_inventory_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "block device",
            "block devices",
            "disk inventory",
            "disks",
            "drives",
            "partitions",
            "storage devices",
            "lsblk",
        ],
    )
}

fn is_system_filesystem_table_query(message_lower: &str) -> bool {
    if extract_mount_detail_query(message_lower).is_some()
        || extract_storage_path_detail_query(message_lower).is_some()
    {
        return false;
    }

    has_any(
        message_lower,
        &[
            "filesystem table",
            "mount table",
            "mounted filesystems",
            "mounts",
            "mount points",
            "what is mounted",
            "what's mounted",
            "mounted on",
            "fstab",
        ],
    )
}

fn is_system_gpu_inventory_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "gpu",
            "gpus",
            "graphics",
            "graphics card",
            "graphics cards",
            "display adapter",
            "display adapters",
            "vram",
            "opencl",
            "cuda",
            "nvidia-smi",
        ],
    )
}

fn is_system_pci_devices_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &["pci", "pci devices", "pci inventory", "pci bus"],
    )
}

fn is_system_usb_devices_query(message_lower: &str) -> bool {
    has_any(message_lower, &["usb", "usb devices", "usb inventory"])
}

fn is_system_boot_log_summary_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "boot log",
            "boot logs",
            "early boot",
            "startup log",
            "startup logs",
            "system boot",
        ],
    )
}

fn is_system_journal_summary_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "journal",
            "journalctl",
            "warning log",
            "warning logs",
            "error log",
            "error logs",
            "boot journal",
            "system journal",
            "warnings and errors",
        ],
    )
}

fn is_system_process_detail_query(message_lower: &str) -> bool {
    if extract_port_conflict_detail_query(message_lower).is_some()
        || extract_listener_detail_query(message_lower).is_some()
    {
        return false;
    }

    has_any(
        message_lower,
        &[
            "process detail",
            "process details",
            "process info",
            "process information",
            "which process",
            "what process",
            "find process",
            "lookup process",
            "ps ",
            "pid ",
        ],
    )
}

fn is_system_listener_detail_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "listener detail",
            "listener details",
            "listener info",
            "listener information",
            "socket detail",
            "socket details",
            "which listener",
            "what listener",
            "which socket",
            "what socket",
            "who is listening",
            "what is listening on",
            "what's listening on",
        ],
    )
}

fn is_system_disk_usage_detail_query(message_lower: &str) -> bool {
    if extract_storage_path_detail_query(message_lower).is_some()
        && extract_posix_path_candidate(message_lower).is_none()
    {
        return false;
    }

    extract_disk_usage_detail_query(message_lower).is_some()
}

fn is_network_route_table_query(message_lower: &str) -> bool {
    if has_any(
        message_lower,
        &["default route", "default gateway", "gateway", "gateways"],
    ) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "route table",
            "routing table",
            "routes",
            "routing",
            "ip route",
        ],
    )
}

fn is_network_active_connections_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "active connections",
            "listeners",
            "listening ports",
            "socket",
            "sockets",
            "established connections",
            "what is listening",
            "who is connected",
        ],
    )
}

fn is_network_interface_counters_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "interface counters",
            "traffic counters",
            "interface stats",
            "link state",
            "link status",
            "rx bytes",
            "tx bytes",
            "packets",
            "network traffic",
        ],
    )
}

fn is_network_wifi_status_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &["wifi", "wi-fi", "wireless", "ssid", "wlan"],
    )
}

fn is_network_vpn_status_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "vpn",
            "wireguard",
            "wg0",
            "tun",
            "tap",
            "tunnel",
            "tunnels",
            "tailscale",
            "zerotier",
            "zt",
        ],
    )
}

fn push_system_diagnostics_tools(
    message: &str,
    planned: &mut Vec<PlannedToolCall>,
    seen: &mut HashSet<&'static str>,
) {
    let lower = message.to_ascii_lowercase();
    if is_system_kernel_info_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetKernelInfo,
            AssistantToolInput::None,
        );
    }
    if is_system_cpu_topology_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetCpuTopology,
            AssistantToolInput::None,
        );
    }
    if is_system_temperature_sensors_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetTemperatureSensors,
            AssistantToolInput::None,
        );
    }
    if is_system_block_device_inventory_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetBlockDeviceInventory,
            AssistantToolInput::None,
        );
    }
    if is_system_filesystem_table_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetFilesystemTable,
            AssistantToolInput::None,
        );
    }
    if is_system_gpu_inventory_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetGpuInventory,
            AssistantToolInput::None,
        );
    }
    if is_system_pci_devices_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetPciDevices,
            AssistantToolInput::None,
        );
    }
    if is_system_usb_devices_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetUsbDevices,
            AssistantToolInput::None,
        );
    }
    if is_system_boot_log_summary_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetBootLogSummary,
            AssistantToolInput::None,
        );
    }
    if is_system_journal_summary_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetJournalSummary,
            AssistantToolInput::None,
        );
    }
    if is_system_process_detail_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetProcessDetail,
            AssistantToolInput::SystemService {
                query: extract_process_detail_query(message)
                    .unwrap_or_else(|| message.trim().to_string()),
            },
        );
    }
    if is_system_listener_detail_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetListenerDetail,
            AssistantToolInput::SystemPortConflicts {
                query: extract_listener_detail_query(message),
            },
        );
    }
    if is_system_disk_usage_detail_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::SystemGetDiskUsageDetail,
            AssistantToolInput::SystemService {
                query: extract_disk_usage_detail_query(message)
                    .unwrap_or_else(|| message.trim().to_string()),
            },
        );
    }
}

fn push_network_diagnostics_tools(
    message: &str,
    planned: &mut Vec<PlannedToolCall>,
    seen: &mut HashSet<&'static str>,
) {
    let lower = message.to_ascii_lowercase();
    if let Some(query) = extract_network_interface_ip_query(message) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetInterfaceByIp,
            AssistantToolInput::NetworkInterface { query },
        );
    }
    if is_network_route_table_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetRouteTable,
            AssistantToolInput::None,
        );
    }
    if is_network_active_connections_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetActiveConnections,
            AssistantToolInput::None,
        );
    }
    if is_network_interface_counters_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetInterfaceCounters,
            AssistantToolInput::None,
        );
    }
    if is_network_wifi_status_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetWifiStatus,
            AssistantToolInput::None,
        );
    }
    if is_network_vpn_status_query(&lower) {
        push_tool(
            planned,
            seen,
            AssistantToolName::NetworkGetVpnStatus,
            AssistantToolInput::None,
        );
    }
}

pub fn plan_tool_calls(message: &str) -> Vec<PlannedToolCall> {
    plan_tool_calls_with_history(message, &[])
}

pub fn plan_tool_calls_with_history(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Vec<PlannedToolCall> {
    let lower = message.to_ascii_lowercase();
    if is_tool_inventory_query(&lower)
        || is_direct_model_chat_request(message, history)
        || is_unsafe_destructive_host_request(&lower)
    {
        return Vec::new();
    }
    let mut planned = Vec::new();
    let mut seen = HashSet::new();

    if let Some(reference) = extract_dictionary_relationship_reference(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DictionaryResolveRelationshipReference,
            AssistantToolInput::DictionaryResolveRelationshipReference {
                reference,
                workspace_id: None,
            },
        );
    } else if is_dictionary_workspace_listing_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DictionaryListVisibleWorkspaces,
            AssistantToolInput::DictionaryListVisibleWorkspaces,
        );
    } else if let Some(workspace_id) = extract_dictionary_workspace_selector(message) {
        if is_dictionary_workspace_people_browse_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::DictionaryBrowseWorkspacePeople,
                AssistantToolInput::DictionaryBrowseWorkspacePeople {
                    workspace_id,
                    query: extract_dictionary_workspace_people_query(message),
                    limit: Some(12),
                },
            );
        }
    } else if has_any(
        &lower,
        &[
            "birthday",
            "birthdays",
            "who has a birthday",
            "upcoming birthday",
        ],
    ) {
        let calendar_input =
            birthday_calendar_window_input(message, extract_birthday_query(message));
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarUpcomingBirthdays,
            calendar_input,
        );
    } else if is_calendar_overlapping_events_query(&lower) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListOverlappingEvents,
            calendar_input,
        );
    } else if is_calendar_conflict_query(&lower) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListDateConflicts,
            calendar_input,
        );
    } else if is_calendar_next_free_day_query(&lower) {
        let calendar_input = extract_calendar_window(message, 30, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarGetNextFreeDay,
            calendar_input,
        );
    } else if is_calendar_free_days_query(&lower) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListFreeDays,
            calendar_input,
        );
    } else if is_calendar_busy_days_query(&lower) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListBusyDays,
            calendar_input,
        );
    } else if is_calendar_event_count_query(&lower) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarCountEvents,
            calendar_input,
        );
    } else if is_next_calendar_event_query(&lower) {
        if is_next_calendar_event_timing_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::CalendarGetNextEventTiming,
                AssistantToolInput::None,
            );
        } else {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::CalendarGetNextEvent,
                AssistantToolInput::None,
            );
        }
    } else if let Some(query) = extract_calendar_event_detail_query(message) {
        let calendar_input = extract_calendar_window(message, 30, Some(query));
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarGetEventDetails,
            calendar_input,
        );
    } else if has_any(
        &lower,
        &[
            "calendar",
            "event",
            "events",
            "schedule",
            "upcoming this week",
            "coming up",
        ],
    ) {
        let calendar_input = extract_calendar_window(message, 7, None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListEvents,
            calendar_input,
        );
    }

    if is_channel_activity_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::ChannelsListUnreadActivity,
            AssistantToolInput::ChannelsFilter {
                query: extract_channel_query(message),
            },
        );
    }

    if is_transcript_summary_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::ChannelsGetTranscriptSummary,
            AssistantToolInput::ChannelsFilter {
                query: extract_transcript_channel_query(message)
                    .or_else(|| recent_transcript_query_hint(history)),
            },
        );
    }

    if is_joinable_rooms_query(&lower) {
        let room_input = extract_room_filter(message);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::RoomsListJoinable,
            room_input,
        );
    } else if has_any(
        &lower,
        &[
            "room",
            "rooms",
            "watch party",
            "listen together",
            "youtube party",
            "screen share",
            "create together",
        ],
    ) {
        let room_input = extract_room_filter(message);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::RoomsListActive,
            room_input,
        );
    }

    if let Some(query) = extract_download_artifact_checksum_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DownloadsGetArtifactChecksum,
            AssistantToolInput::DownloadsFilter {
                query: Some(query),
                availability: extract_downloads_availability(message),
            },
        );
    } else if let Some(query) = extract_download_artifact_install_steps_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DownloadsGetArtifactInstallSteps,
            AssistantToolInput::DownloadsFilter {
                query: Some(query),
                availability: extract_downloads_availability(message),
            },
        );
    } else if let Some(query) = extract_download_artifact_compatibility_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DownloadsGetArtifactCompatibility,
            AssistantToolInput::DownloadsFilter {
                query: Some(query),
                availability: extract_downloads_availability(message),
            },
        );
    } else if has_any(
        &lower,
        &[
            "download",
            "downloads",
            "browser extension",
            "extension",
            "client app",
            "companion tools",
        ],
    ) {
        if let Some(query) = extract_download_artifact_detail_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::DownloadsGetArtifactDetails,
                AssistantToolInput::DownloadsFilter {
                    query: Some(query),
                    availability: extract_downloads_availability(message),
                },
            );
        } else {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::DownloadsListAvailableArtifacts,
                extract_downloads_filter(message),
            );
        }
    }

    if is_weather_query(&lower)
        && let Some(location) = extract_weather_location(message)
        && let Some((weather_tool, weather_input)) =
            weather_tool_call_for_location(message, location)
    {
        push_tool(&mut planned, &mut seen, weather_tool, weather_input);
    }

    if public_web_tools_enabled() {
        if is_curated_web_catalog_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::WebListCuratedSources,
                AssistantToolInput::None,
            );
        } else if let Some(url) = extract_public_web_url(message) {
            let category =
                infer_curated_web_category_for_url(&url).or_else(|| recent_web_category(history));
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::WebFetchPublicPageSummary,
                AssistantToolInput::WebFetch { url, category },
            );
        } else if let Some(query) = extract_public_web_search_query(message) {
            let category =
                infer_curated_web_category_slug(message).or_else(|| recent_web_category(history));
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::WebSearchPublicWeb,
                AssistantToolInput::WebSearch { query, category },
            );
        }
    }

    if let Some(query) = extract_port_conflict_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetPortConflictDetail,
            AssistantToolInput::SystemPortConflicts { query: Some(query) },
        );
    } else if is_port_conflicts_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetPortConflicts,
            AssistantToolInput::SystemPortConflicts {
                query: extract_port_conflicts_query(message),
            },
        );
    }

    if is_failed_unit_detail_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetFailedUnitDetail,
            AssistantToolInput::SystemFailedUnits {
                query: extract_failed_unit_detail_query(message),
            },
        );
    } else if is_failed_units_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetFailedUnits,
            AssistantToolInput::SystemFailedUnits {
                query: extract_failed_units_query(message),
            },
        );
    }

    push_system_diagnostics_tools(message, &mut planned, &mut seen);
    push_network_diagnostics_tools(message, &mut planned, &mut seen);

    let network_diagnostics_requested = is_network_route_table_query(&lower)
        || is_network_active_connections_query(&lower)
        || is_network_interface_counters_query(&lower)
        || is_network_wifi_status_query(&lower)
        || is_network_vpn_status_query(&lower);

    if is_network_query(&lower) && !network_diagnostics_requested {
        if is_network_default_route_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetDefaultRoute,
                AssistantToolInput::NetworkDefaultRoute {
                    query: extract_network_default_route_query(message),
                },
            );
        } else if is_network_hostname_aliases_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetHostnameAliases,
                AssistantToolInput::NetworkHostnameAliases {
                    query: extract_network_hostname_aliases_query(message),
                },
            );
        } else if is_network_dns_servers_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetDnsServers,
                AssistantToolInput::NetworkDnsServers {
                    query: extract_network_dns_servers_query(message),
                },
            );
        } else if let Some(query) = extract_network_interface_ip_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetInterfaceByIp,
                AssistantToolInput::NetworkInterface { query },
            );
        } else if let Some(query) = extract_network_interface_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetInterfaceDetails,
                AssistantToolInput::NetworkInterface { query },
            );
        } else {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::NetworkGetTopologySummary,
                AssistantToolInput::None,
            );
        }
    }

    if is_current_datetime_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetCurrentDateTime,
            AssistantToolInput::None,
        );
    }

    if is_ai_runtime_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetAiRuntimeSummary,
            AssistantToolInput::None,
        );
    }

    let host_diagnostics_requested = is_system_kernel_info_query(&lower)
        || is_system_cpu_topology_query(&lower)
        || is_system_temperature_sensors_query(&lower)
        || is_system_block_device_inventory_query(&lower)
        || is_system_filesystem_table_query(&lower)
        || is_system_gpu_inventory_query(&lower)
        || is_system_pci_devices_query(&lower)
        || is_system_usb_devices_query(&lower)
        || is_system_boot_log_summary_query(&lower)
        || is_system_journal_summary_query(&lower);

    if is_host_runtime_query(&lower) && !is_ai_runtime_query(&lower) && !host_diagnostics_requested
    {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetHostRuntimeSummary,
            AssistantToolInput::None,
        );
    }

    if is_backup_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetBackupSummary,
            AssistantToolInput::None,
        );
    }

    if let Some(query) = extract_service_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetServiceDetail,
            AssistantToolInput::SystemService { query },
        );
    } else if is_service_health_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetServiceHealth,
            AssistantToolInput::None,
        );
    }

    if is_transcode_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetTranscodeSummary,
            AssistantToolInput::None,
        );
    }

    if let Some(query) = extract_mount_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetMountDetail,
            AssistantToolInput::SystemService { query },
        );
    } else if let Some(query) = extract_storage_path_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetStoragePathDetail,
            AssistantToolInput::SystemService { query },
        );
    } else if is_storage_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetStorageSummary,
            AssistantToolInput::None,
        );
    }

    if is_recent_errors_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetRecentErrors,
            AssistantToolInput::None,
        );
    }

    if is_library_duplicate_titles_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrariesFindDuplicateTitles,
            AssistantToolInput::None,
        );
    } else if is_library_missing_metadata_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrariesListMissingMetadata,
            AssistantToolInput::None,
        );
    } else if let Some(query) = extract_library_media_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibraryGetItemMediaDetails,
            AssistantToolInput::LibrarySearch { query },
        );
    } else if let Some(query) = extract_library_detail_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrariesGetLibrarySummary,
            AssistantToolInput::LibrarySearch { query },
        );
    } else if let Some(query) = extract_library_search_query(message) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrarySearchTitles,
            AssistantToolInput::LibrarySearch { query },
        );
    } else if is_recent_library_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrariesGetRecentlyAdded,
            AssistantToolInput::LibraryRecent {
                query: extract_recent_library_query(message),
            },
        );
    } else if has_any(
        &lower,
        &[
            "library",
            "libraries",
            "media library",
            "media libraries",
            "what media",
            "what libraries",
        ],
    ) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::LibrariesListAccessible,
            AssistantToolInput::None,
        );
    }

    if !message_has_other_domain_context(&lower)
        && extract_memory_relation_path_query(message).is_some()
    {
        if let Some((source, target)) = extract_memory_relation_path_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemoryGetEntityRelationPath,
                AssistantToolInput::SystemService {
                    query: format!("{source} || {target}"),
                },
            );
        }
    } else if !message_has_other_domain_context(&lower) && is_memory_relation_query(&lower) {
        if let Some(query) = extract_memory_relation_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemoryGetEntityRelations,
                AssistantToolInput::SystemService { query },
            );
        }
    } else if !message_has_other_domain_context(&lower) && is_memory_person_summary_query(&lower) {
        if let Some(query) = extract_memory_person_summary_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemoryGetPersonSummary,
                AssistantToolInput::SystemService { query },
            );
        }
    } else if !message_has_other_domain_context(&lower) && is_memory_recent_changes_query(&lower) {
        let input = extract_memory_recent_changes_query(message)
            .map(|query| AssistantToolInput::SystemService { query })
            .unwrap_or(AssistantToolInput::None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::MemoryListRecentChanges,
            input,
        );
    } else if !message_has_other_domain_context(&lower) && is_memory_conflict_query(&lower) {
        let input = extract_memory_conflict_query(message)
            .map(|query| AssistantToolInput::SystemService { query })
            .unwrap_or(AssistantToolInput::None);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::MemoryListConflictingFacts,
            input,
        );
    } else if !message_has_other_domain_context(&lower) && is_memory_provenance_query(&lower) {
        if let Some(query) = extract_memory_provenance_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemoryGetEntityProvenance,
                AssistantToolInput::SystemService { query },
            );
        }
    } else if !message_has_other_domain_context(&lower) && is_memory_recent_entity_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::MemoryListRecentEntities,
            AssistantToolInput::None,
        );
    } else if !message_has_other_domain_context(&lower) && is_memory_recent_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::MemoryListRecentFacts,
            AssistantToolInput::None,
        );
    } else if !message_has_other_domain_context(&lower) && is_memory_exact_entity_query(&lower) {
        if let Some(query) = extract_memory_exact_entity_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemoryFindExactEntity,
                AssistantToolInput::SystemService { query },
            );
        }
    } else if let Some(query) = extract_memory_query(message) {
        if is_memory_entity_query(&lower) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemorySearchEntities,
                AssistantToolInput::SystemService { query },
            );
        } else {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::MemorySearchFacts,
                AssistantToolInput::SystemService { query },
            );
        }
    }

    if !is_host_runtime_query(&lower)
        && !is_service_health_query(&lower)
        && !is_recent_errors_query(&lower)
        && !is_transcode_query(&lower)
        && has_any(
            &lower,
            &["server", "servers", "minecraft", "minecraft server"],
        )
    {
        let server_input = extract_server_filter(message);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::ServersListMinecraftStatus,
            server_input,
        );
    }

    if has_any(
        &lower,
        &[
            "who am i",
            "my account",
            "my profile",
            "what can i access",
            "what is my role",
        ],
    ) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::AccountGetProfileSummary,
            AssistantToolInput::None,
        );
    }

    if extract_follow_up_entity_reference(message).is_some() {
        let mut referenced = Vec::new();
        let mut referenced_seen = HashSet::new();
        if apply_follow_up_entity_reference(message, history, &mut referenced, &mut referenced_seen)
        {
            planned = referenced;
        }
    } else if planned.is_empty() {
        apply_follow_up_tool_hints(message, history, &mut planned, &mut seen);
    }

    planned.truncate(MAX_TOOL_CALLS_PER_TURN);
    planned
}

pub fn build_assistant_messages(
    request: AssistantChatRequest,
    grounding_chunks: &[AssistantGroundingChunk],
) -> Vec<ChatMessage> {
    build_assistant_messages_with_budget(
        request,
        grounding_chunks,
        DEFAULT_ASSISTANT_CONTEXT_LENGTH_TOKENS,
        false,
    )
    .0
}

pub fn build_assistant_messages_with_budget(
    request: AssistantChatRequest,
    grounding_chunks: &[AssistantGroundingChunk],
    context_length_tokens: u32,
    emergency_compaction: bool,
) -> (Vec<ChatMessage>, ConversationPromptDebug) {
    let local_now = assistant_local_now();
    let context_length_tokens = context_length_tokens.max(1);
    let reserved_completion_tokens =
        response_mode_completion_reserve_tokens(request.response_mode, context_length_tokens);
    let prompt_budget_tokens =
        prompt_budget_tokens(request.response_mode, context_length_tokens) as u64;

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: build_system_prompt(),
    }];

    messages.push(ChatMessage {
        role: "system".to_string(),
        content: format!(
            "Current Rustyfin host local date/time for this turn: {}. Use this when interpreting relative dates like today, tomorrow, and next Tuesday.",
            local_now.format("%Y-%m-%d %H:%M:%S %:z (%A)")
        ),
    });

    let prompt_grounding_chunks = select_grounding_chunks_for_prompt(
        grounding_chunks,
        prompt_budget_tokens,
        emergency_compaction,
    );
    if !prompt_grounding_chunks.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding for this turn:\n{}",
                super::replies::grounding_chunks_prompt(&prompt_grounding_chunks)
            ),
        });
    }

    let system_message_count = messages.len() as u32;
    let mut prompt_debug = ConversationPromptDebug {
        system_message_count,
        grounding_chunk_count: prompt_grounding_chunks.len() as u32,
        response_mode: request.response_mode.as_str().to_string(),
        context_length_tokens,
        prompt_budget_tokens: prompt_budget_tokens as u32,
        reserved_completion_tokens,
        completion_budget_tokens: reserved_completion_tokens,
        emergency_compaction,
        ..ConversationPromptDebug::default()
    };

    let current_user_message = ChatMessage {
        role: "user".to_string(),
        content: request.message,
    };
    let base_prompt_tokens = estimate_chat_message_tokens(&messages).saturating_add(
        estimate_chat_message_tokens(std::slice::from_ref(&current_user_message)),
    );
    let raw_history_limit = match (request.response_mode, emergency_compaction) {
        (_, true) => EMERGENCY_HISTORY_RAW_MESSAGE_LIMIT,
        (AssistantResponseMode::Instant, false) => INSTANT_HISTORY_RAW_MESSAGE_LIMIT,
        (AssistantResponseMode::Extended, false) => EXTENDED_HISTORY_RAW_MESSAGE_LIMIT,
        _ => NORMAL_HISTORY_RAW_MESSAGE_LIMIT,
    };
    let compact_chars = if emergency_compaction {
        EMERGENCY_HISTORY_COMPACT_CHARS
    } else {
        NORMAL_HISTORY_COMPACT_CHARS
    };

    let mut selected_history = Vec::new();
    let mut selected_history_tokens = 0_u64;
    let mut retained_raw_turns = 0_u32;
    let mut summarized_turns = 0_u32;
    let mut compact_boundary_count = 0_u32;

    for history in request.history.iter().rev() {
        let raw_message = ChatMessage {
            role: history.role.clone(),
            content: history.content.trim().to_string(),
        };
        let raw_tokens = estimate_chat_message_tokens(std::slice::from_ref(&raw_message));
        let can_keep_raw = retained_raw_turns < raw_history_limit as u32
            && base_prompt_tokens
                .saturating_add(selected_history_tokens)
                .saturating_add(raw_tokens)
                <= prompt_budget_tokens;

        if can_keep_raw {
            selected_history.push(raw_message);
            selected_history_tokens = selected_history_tokens.saturating_add(raw_tokens);
            retained_raw_turns += 1;
            continue;
        }

        let compact_message = ChatMessage {
            role: history.role.clone(),
            content: compact_history_message(history, compact_chars),
        };
        let compact_tokens = estimate_chat_message_tokens(std::slice::from_ref(&compact_message));
        let can_keep_compact = !compact_message.content.is_empty()
            && base_prompt_tokens
                .saturating_add(selected_history_tokens)
                .saturating_add(compact_tokens)
                <= prompt_budget_tokens;

        if can_keep_compact {
            selected_history.push(compact_message);
            selected_history_tokens = selected_history_tokens.saturating_add(compact_tokens);
            summarized_turns += 1;
            compact_boundary_count = compact_boundary_count.saturating_add(1);
        } else {
            compact_boundary_count = compact_boundary_count.saturating_add(1);
        }
    }

    selected_history.reverse();
    prompt_debug.history_message_count = selected_history.len() as u32;
    prompt_debug.loaded_history_turns = selected_history.len() as u32;
    prompt_debug.retained_raw_turns = retained_raw_turns;
    prompt_debug.summarized_turns = summarized_turns;
    prompt_debug.compact_boundary_count = compact_boundary_count;

    messages.extend(selected_history);

    messages.push(current_user_message);

    (messages, prompt_debug)
}

fn select_grounding_chunks_for_prompt(
    grounding_chunks: &[AssistantGroundingChunk],
    prompt_budget_tokens: u64,
    emergency_compaction: bool,
) -> Vec<AssistantGroundingChunk> {
    if grounding_chunks.is_empty() {
        return Vec::new();
    }

    let max_chars = if emergency_compaction {
        EMERGENCY_GROUNDING_PROMPT_CHARS
    } else {
        NORMAL_GROUNDING_PROMPT_CHARS.min((prompt_budget_tokens as usize).saturating_mul(4) / 3)
    };
    let max_chunks = if emergency_compaction {
        EMERGENCY_GROUNDING_CHUNK_LIMIT
    } else {
        super::replies::MAX_GROUNDING_CHUNKS
    };

    rank_and_compress_grounding_chunks(grounding_chunks, max_chunks, max_chars.max(400))
}

fn compact_history_message(message: &AssistantHistoryMessage, max_chars: usize) -> String {
    let normalized = message
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return String::new();
    }

    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let head_chars = max_chars.saturating_mul(2) / 3;
    let tail_chars = max_chars
        .saturating_sub(head_chars)
        .saturating_sub(5)
        .max(24);
    let head = compact_text(&normalized, head_chars);
    let tail = normalized
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    match message.role.as_str() {
        "assistant" => format!("Earlier assistant reply (truncated): {head} ... {tail}"),
        "user" => format!("Earlier user message (truncated): {head} ... {tail}"),
        _ => format!("{head} ... {tail}"),
    }
}

fn estimate_chat_message_tokens(messages: &[ChatMessage]) -> u64 {
    messages
        .iter()
        .fold(0_u64, |total, message| {
            let role_cost = (message.role.len() / 4).saturating_add(4) as u64;
            let content_cost = (message.content.len() / 4).saturating_add(1) as u64;
            total.saturating_add(role_cost).saturating_add(content_cost)
        })
        .max(1)
}

fn response_mode_completion_reserve_tokens(
    response_mode: AssistantResponseMode,
    context_window_tokens: u32,
) -> u32 {
    let dynamic = match response_mode {
        AssistantResponseMode::Instant => context_window_tokens / 6,
        AssistantResponseMode::Thinking => context_window_tokens / 4,
        AssistantResponseMode::Extended => context_window_tokens / 3,
    };

    match response_mode {
        AssistantResponseMode::Instant => dynamic.clamp(384, 640),
        AssistantResponseMode::Thinking => dynamic.clamp(768, 1536),
        AssistantResponseMode::Extended => dynamic.clamp(1024, 4096),
    }
}

fn prompt_budget_tokens(response_mode: AssistantResponseMode, context_window_tokens: u32) -> u32 {
    const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 192;

    context_window_tokens.saturating_sub(
        response_mode_completion_reserve_tokens(response_mode, context_window_tokens)
            + CONTEXT_SAFETY_MARGIN_TOKENS,
    )
}

pub fn status_label_for_tool_call(call: &PlannedToolCall) -> String {
    match (&call.tool, &call.input) {
        (AssistantToolName::AccountGetProfileSummary, _) => {
            "Checking your account context".to_string()
        }
        (
            AssistantToolName::CalendarListEvents,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking calendar events for {label}"),
        (AssistantToolName::CalendarListEvents, _) => "Checking calendar events".to_string(),
        (
            AssistantToolName::CalendarCountEvents,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Counting calendar events for {label}"),
        (AssistantToolName::CalendarCountEvents, _) => "Counting calendar events".to_string(),
        (
            AssistantToolName::CalendarListBusyDays,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking busy calendar days for {label}"),
        (AssistantToolName::CalendarListBusyDays, _) => "Checking busy calendar days".to_string(),
        (
            AssistantToolName::CalendarGetNextFreeDay,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking the next free calendar day for {label}"),
        (AssistantToolName::CalendarGetNextFreeDay, _) => {
            "Checking the next free calendar day".to_string()
        }
        (
            AssistantToolName::CalendarListOverlappingEvents,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking overlapping calendar events for {label}"),
        (AssistantToolName::CalendarListOverlappingEvents, _) => {
            "Checking overlapping calendar events".to_string()
        }
        (AssistantToolName::CalendarGetNextEvent, _) => {
            "Checking your next calendar event".to_string()
        }
        (
            AssistantToolName::CalendarUpcomingBirthdays,
            AssistantToolInput::CalendarWindow {
                label,
                query: Some(query),
                ..
            },
        ) => format!("Checking birthdays matching \"{query}\" for {label}"),
        (
            AssistantToolName::CalendarUpcomingBirthdays,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking birthdays for {label}"),
        (AssistantToolName::CalendarUpcomingBirthdays, _) => {
            "Checking upcoming birthdays".to_string()
        }
        (
            AssistantToolName::CalendarGetEventDetails,
            AssistantToolInput::CalendarWindow {
                query: Some(query), ..
            },
        ) => format!("Loading calendar details for \"{query}\""),
        (AssistantToolName::CalendarGetEventDetails, _) => {
            "Loading calendar event details".to_string()
        }
        (
            AssistantToolName::ChannelsListUnreadActivity,
            AssistantToolInput::ChannelsFilter { query: Some(query) },
        ) => format!("Checking recent channel activity in \"{query}\""),
        (AssistantToolName::ChannelsListUnreadActivity, _) => {
            "Checking recent channel activity".to_string()
        }
        (
            AssistantToolName::ChannelsGetTranscriptSummary,
            AssistantToolInput::ChannelsFilter { query: Some(query) },
        ) => format!("Checking the latest transcript summary for \"{query}\""),
        (AssistantToolName::ChannelsGetTranscriptSummary, _) => {
            "Checking the latest completed call transcript".to_string()
        }
        (
            AssistantToolName::DownloadsListAvailableArtifacts,
            AssistantToolInput::DownloadsFilter {
                query: Some(query),
                availability: Some(availability),
            },
        ) => format!("Checking {availability} downloads matching \"{query}\""),
        (
            AssistantToolName::DownloadsListAvailableArtifacts,
            AssistantToolInput::DownloadsFilter {
                query: Some(query),
                availability: None,
            },
        ) => format!("Checking downloads matching \"{query}\""),
        (
            AssistantToolName::DownloadsListAvailableArtifacts,
            AssistantToolInput::DownloadsFilter {
                query: None,
                availability: Some(availability),
            },
        ) => format!("Checking {availability} downloads"),
        (AssistantToolName::DownloadsListAvailableArtifacts, _) => {
            "Checking available downloads".to_string()
        }
        (
            AssistantToolName::DownloadsGetArtifactDetails,
            AssistantToolInput::DownloadsFilter {
                query: Some(query), ..
            },
        ) => format!("Loading download artifact details for \"{query}\""),
        (AssistantToolName::DownloadsGetArtifactDetails, _) => {
            "Loading download artifact details".to_string()
        }
        (AssistantToolName::NetworkGetTopologySummary, _) => {
            "Checking network topology and interface state".to_string()
        }
        (
            AssistantToolName::NetworkGetInterfaceDetails,
            AssistantToolInput::NetworkInterface { query },
        ) => {
            format!("Loading network interface details for \"{query}\"")
        }
        (
            AssistantToolName::NetworkGetInterfaceByIp,
            AssistantToolInput::NetworkInterface { query },
        ) => {
            if query.parse::<std::net::IpAddr>().is_ok() {
                format!("Resolving network interface for IP \"{query}\"")
            } else {
                format!("Loading network interface details for \"{query}\"")
            }
        }
        (AssistantToolName::NetworkGetInterfaceDetails, _) => {
            "Loading network interface details".to_string()
        }
        (AssistantToolName::NetworkGetInterfaceByIp, _) => {
            "Resolving network interface for an IP address".to_string()
        }
        (
            AssistantToolName::NetworkGetDefaultRoute,
            AssistantToolInput::NetworkDefaultRoute { query: Some(query) },
        ) => format!("Checking default route for \"{query}\""),
        (AssistantToolName::NetworkGetDefaultRoute, _) => {
            "Checking the default network route".to_string()
        }
        (
            AssistantToolName::NetworkGetHostnameAliases,
            AssistantToolInput::NetworkHostnameAliases { query: Some(query) },
        ) => format!("Checking hostname aliases for \"{query}\""),
        (AssistantToolName::NetworkGetHostnameAliases, _) => {
            "Checking hostname aliases".to_string()
        }
        (
            AssistantToolName::NetworkGetDnsServers,
            AssistantToolInput::NetworkDnsServers { query: Some(query) },
        ) => format!("Checking DNS servers for \"{query}\""),
        (AssistantToolName::NetworkGetDnsServers, _) => "Checking DNS servers".to_string(),
        (AssistantToolName::NetworkGetRouteTable, _) => "Checking route table".to_string(),
        (AssistantToolName::NetworkGetActiveConnections, _) => {
            "Checking active connections".to_string()
        }
        (AssistantToolName::NetworkGetInterfaceCounters, _) => {
            "Checking interface counters".to_string()
        }
        (AssistantToolName::NetworkGetWifiStatus, _) => "Checking Wi-Fi status".to_string(),
        (AssistantToolName::NetworkGetVpnStatus, _) => "Checking VPN status".to_string(),
        (AssistantToolName::WeatherGetCurrent, AssistantToolInput::Weather { location, .. }) => {
            format!("Checking current weather for \"{location}\"")
        }
        (
            AssistantToolName::WeatherGetForecast,
            AssistantToolInput::Weather {
                location,
                forecast_days: Some(days),
            },
        ) => format!("Checking the next {days} days of weather for \"{location}\""),
        (AssistantToolName::WeatherGetForecast, AssistantToolInput::Weather { location, .. }) => {
            format!("Checking weather forecast for \"{location}\"")
        }
        (
            AssistantToolName::WeatherGetHourlyWindow,
            AssistantToolInput::WeatherHistory {
                location, label, ..
            },
        ) => format!("Checking hourly weather window for {label} in \"{location}\""),
        (
            AssistantToolName::WeatherGetHistory,
            AssistantToolInput::WeatherHistory {
                location, label, ..
            },
        ) => format!("Checking recent weather history for {label} in \"{location}\""),
        (AssistantToolName::LibrariesListAccessible, _) => {
            "Checking accessible libraries".to_string()
        }
        (AssistantToolName::MemoryListRecentFacts, AssistantToolInput::None) => {
            "Checking recent stored memory facts".to_string()
        }
        (AssistantToolName::MemoryListRecentEntities, AssistantToolInput::None) => {
            "Checking recent stored entities".to_string()
        }
        (AssistantToolName::MemorySearchFacts, AssistantToolInput::SystemService { query }) => {
            format!("Searching stored memory facts for \"{query}\"")
        }
        (AssistantToolName::MemorySearchEntities, AssistantToolInput::SystemService { query }) => {
            format!("Searching stored entities for \"{query}\"")
        }
        (AssistantToolName::MemoryFindExactEntity, AssistantToolInput::SystemService { query }) => {
            format!("Finding exact stored entity match for \"{query}\"")
        }
        (
            AssistantToolName::MemoryListRecentChanges,
            AssistantToolInput::SystemService { query },
        ) => format!("Checking recent stored memory changes for \"{query}\""),
        (AssistantToolName::MemoryListRecentChanges, AssistantToolInput::None) => {
            "Checking recent stored memory changes".to_string()
        }
        (
            AssistantToolName::MemoryListConflictingFacts,
            AssistantToolInput::SystemService { query },
        ) => format!("Checking conflicting stored memory facts for \"{query}\""),
        (AssistantToolName::MemoryListConflictingFacts, AssistantToolInput::None) => {
            "Checking conflicting stored memory facts".to_string()
        }
        (
            AssistantToolName::MemoryGetEntityProvenance,
            AssistantToolInput::SystemService { query },
        ) => format!("Loading stored entity provenance for \"{query}\""),
        (AssistantToolName::MemoryGetEntityProvenance, AssistantToolInput::None) => {
            "Loading stored entity provenance".to_string()
        }
        (
            AssistantToolName::MemoryGetEntityRelations,
            AssistantToolInput::SystemService { query },
        ) => format!("Loading stored entity relations for \"{query}\""),
        (
            AssistantToolName::MemoryGetPersonSummary,
            AssistantToolInput::SystemService { query },
        ) => format!("Checking stored person summary for \"{query}\""),
        (
            AssistantToolName::MemoryGetEntityRelationPath,
            AssistantToolInput::SystemService { query },
        ) => {
            if let Some((source, target)) = query.split_once("||") {
                format!(
                    "Loading stored entity relation path between \"{}\" and \"{}\"",
                    source.trim(),
                    target.trim()
                )
            } else {
                "Loading stored entity relation path".to_string()
            }
        }
        (AssistantToolName::MemoryListRecentFacts, _) => {
            "Checking recent stored memory facts".to_string()
        }
        (AssistantToolName::MemoryListRecentEntities, _) => {
            "Checking recent stored entities".to_string()
        }
        (AssistantToolName::MemorySearchFacts, _) => "Searching stored memory facts".to_string(),
        (AssistantToolName::MemorySearchEntities, _) => "Searching stored entities".to_string(),
        (AssistantToolName::MemoryFindExactEntity, _) => {
            "Finding exact stored entity match".to_string()
        }
        (AssistantToolName::MemoryListRecentChanges, _) => {
            "Checking recent stored memory changes".to_string()
        }
        (AssistantToolName::MemoryListConflictingFacts, _) => {
            "Checking conflicting stored memory facts".to_string()
        }
        (AssistantToolName::MemoryGetEntityProvenance, _) => {
            "Loading stored entity provenance".to_string()
        }
        (AssistantToolName::MemoryGetEntityRelations, _) => {
            "Loading stored entity relations".to_string()
        }
        (AssistantToolName::MemoryGetEntityRelationPath, _) => {
            "Loading stored entity relation path".to_string()
        }
        (AssistantToolName::SystemGetCurrentDateTime, _) => {
            "Checking the Rustyfin host date and time".to_string()
        }
        (AssistantToolName::SystemGetAiRuntimeSummary, _) => {
            "Checking the Rustyfin AI runtime and loaded model".to_string()
        }
        (AssistantToolName::SystemGetKernelInfo, _) => {
            "Checking host kernel and OS details".to_string()
        }
        (AssistantToolName::SystemGetCpuTopology, _) => "Checking CPU topology".to_string(),
        (AssistantToolName::SystemGetTemperatureSensors, _) => {
            "Checking temperature sensors".to_string()
        }
        (AssistantToolName::SystemGetBlockDeviceInventory, _) => {
            "Checking block devices".to_string()
        }
        (AssistantToolName::SystemGetFilesystemTable, _) => "Checking filesystem table".to_string(),
        (AssistantToolName::SystemGetGpuInventory, _) => "Checking GPU inventory".to_string(),
        (AssistantToolName::SystemGetPciDevices, _) => "Checking PCI devices".to_string(),
        (AssistantToolName::SystemGetUsbDevices, _) => "Checking USB devices".to_string(),
        (AssistantToolName::SystemGetBootLogSummary, _) => "Checking boot logs".to_string(),
        (AssistantToolName::SystemGetJournalSummary, _) => "Checking journal summary".to_string(),
        (AssistantToolName::SystemGetProcessDetail, _) => "Checking host processes".to_string(),
        (AssistantToolName::SystemGetListenerDetail, _) => "Checking host listeners".to_string(),
        (AssistantToolName::SystemGetDiskUsageDetail, _) => {
            "Checking disk usage details".to_string()
        }
        (AssistantToolName::SystemGetHostRuntimeSummary, _) => {
            "Checking Rustyfin host runtime stats".to_string()
        }
        (
            AssistantToolName::SystemGetStoragePathDetail,
            AssistantToolInput::SystemService { query },
        ) => format!("Loading storage path details for \"{query}\""),
        (AssistantToolName::SystemGetStoragePathDetail, _) => {
            "Loading storage path details".to_string()
        }
        (AssistantToolName::SystemGetMountDetail, AssistantToolInput::SystemService { query }) => {
            format!("Loading mount details for \"{query}\"")
        }
        (AssistantToolName::SystemGetMountDetail, _) => "Loading mount details".to_string(),
        (
            AssistantToolName::LibrariesGetLibrarySummary,
            AssistantToolInput::LibrarySearch { query },
        ) => format!("Loading library summary for \"{query}\""),
        (AssistantToolName::LibrariesGetLibrarySummary, _) => "Loading library summary".to_string(),
        (AssistantToolName::LibrarySearchTitles, AssistantToolInput::LibrarySearch { query }) => {
            format!("Searching libraries for \"{query}\"")
        }
        (AssistantToolName::LibraryGetItemSummary, AssistantToolInput::LibrarySearch { query }) => {
            format!("Loading library item details for \"{query}\"")
        }
        (
            AssistantToolName::LibraryGetItemMediaDetails,
            AssistantToolInput::LibrarySearch { query },
        ) => format!("Loading library media details for \"{query}\""),
        (AssistantToolName::LibraryGetItemMediaDetails, _) => {
            "Loading library media details".to_string()
        }
        (
            AssistantToolName::LibrariesGetRecentlyAdded,
            AssistantToolInput::LibraryRecent { query: Some(query) },
        ) => format!("Checking recently added library items matching \"{query}\""),
        (AssistantToolName::LibrariesGetRecentlyAdded, _) => {
            "Checking recently added library items".to_string()
        }
        (AssistantToolName::WebListCuratedSources, AssistantToolInput::None) => {
            "Listing curated public web sources".to_string()
        }
        (
            AssistantToolName::WebSearchPublicWeb,
            AssistantToolInput::WebSearch { query, category },
        ) => match category.as_deref().and_then(curated_web_category_label) {
            Some(category_label) => {
                format!("Searching {category_label} sources for \"{query}\"")
            }
            None => format!("Searching the public web for \"{query}\""),
        },
        (
            AssistantToolName::WebFetchPublicPageSummary,
            AssistantToolInput::WebFetch { url, category },
        ) => match category.as_deref().and_then(curated_web_category_label) {
            Some(category_label) => format!(
                "Fetching {category_label} page {}",
                truncate_for_planner(url, 80)
            ),
            None => format!("Fetching public page {}", truncate_for_planner(url, 80)),
        },
        (
            AssistantToolName::RoomsListActive,
            AssistantToolInput::RoomsFilter {
                room_mode: Some(room_mode),
                ..
            },
        ) => format!(
            "Checking active {} rooms",
            room_mode_status_label(room_mode)
        ),
        (
            AssistantToolName::RoomsListActive,
            AssistantToolInput::RoomsFilter {
                room_mode: None,
                query: Some(query),
            },
        ) => format!("Checking room \"{query}\""),
        (AssistantToolName::RoomsListActive, _) => "Checking active rooms".to_string(),
        (
            AssistantToolName::RoomsGetRoomSummary,
            AssistantToolInput::RoomsFilter {
                query: Some(query), ..
            },
        ) => format!("Loading room details for \"{query}\""),
        (AssistantToolName::RoomsGetRoomSummary, _) => "Loading room details".to_string(),
        (
            AssistantToolName::RoomsListJoinable,
            AssistantToolInput::RoomsFilter {
                room_mode: Some(room_mode),
                ..
            },
        ) => format!(
            "Checking joinable {} rooms",
            room_mode_status_label(room_mode)
        ),
        (
            AssistantToolName::RoomsListJoinable,
            AssistantToolInput::RoomsFilter {
                room_mode: None,
                query: Some(query),
            },
        ) => format!("Checking joinable rooms matching \"{query}\""),
        (AssistantToolName::RoomsListJoinable, _) => "Checking joinable rooms".to_string(),
        (
            AssistantToolName::ServersListMinecraftStatus,
            AssistantToolInput::ServerFilter {
                query: Some(query),
                availability: Some(availability),
            },
        ) => format!("Checking whether Minecraft server \"{query}\" is {availability}"),
        (
            AssistantToolName::ServersListMinecraftStatus,
            AssistantToolInput::ServerFilter {
                query: Some(query),
                availability: None,
            },
        ) => format!("Checking Minecraft server \"{query}\""),
        (
            AssistantToolName::ServersListMinecraftStatus,
            AssistantToolInput::ServerFilter {
                query: None,
                availability: Some(availability),
            },
        ) => format!("Checking Minecraft servers that are {availability}"),
        (AssistantToolName::ServersListMinecraftStatus, _) => {
            "Checking Minecraft server status".to_string()
        }
        (
            AssistantToolName::ServersGetMinecraftServerSummary,
            AssistantToolInput::ServerFilter {
                query: Some(query), ..
            },
        ) => format!("Loading Minecraft server details for \"{query}\""),
        (AssistantToolName::ServersGetMinecraftServerSummary, _) => {
            "Loading Minecraft server details".to_string()
        }
        (AssistantToolName::SystemGetBackupSummary, _) => "Checking backup capability".to_string(),
        (AssistantToolName::SystemGetServiceHealth, _) => "Checking service health".to_string(),
        (
            AssistantToolName::SystemGetServiceDetail,
            AssistantToolInput::SystemService { query },
        ) => {
            format!("Loading service details for \"{query}\"")
        }
        (AssistantToolName::SystemGetServiceDetail, _) => "Loading service details".to_string(),
        (AssistantToolName::SystemGetTranscodeSummary, _) => {
            "Checking transcoding health and hardware acceleration".to_string()
        }
        (AssistantToolName::SystemGetStorageSummary, _) => {
            "Checking storage paths and free space".to_string()
        }
        (AssistantToolName::SystemGetRecentErrors, _) => {
            "Checking recent failures and errors".to_string()
        }
        (
            AssistantToolName::SystemGetPortConflicts,
            AssistantToolInput::SystemPortConflicts { query: Some(query) },
        ) => format!("Checking port conflicts for \"{query}\""),
        (AssistantToolName::SystemGetPortConflicts, _) => "Checking port conflicts".to_string(),
        (
            AssistantToolName::SystemGetPortConflictDetail,
            AssistantToolInput::SystemPortConflicts { query: Some(query) },
        ) => format!("Loading port conflict details for \"{query}\""),
        (AssistantToolName::SystemGetPortConflictDetail, _) => {
            "Loading port conflict details".to_string()
        }
        (
            AssistantToolName::SystemGetFailedUnits,
            AssistantToolInput::SystemFailedUnits { query: Some(query) },
        ) => format!("Checking failed units for \"{query}\""),
        (AssistantToolName::SystemGetFailedUnits, _) => "Checking failed systemd units".to_string(),
        (
            AssistantToolName::SystemGetFailedUnitDetail,
            AssistantToolInput::SystemFailedUnits { query: Some(query) },
        ) => format!("Loading failed unit details for \"{query}\""),
        (AssistantToolName::SystemGetFailedUnitDetail, _) => {
            "Loading failed unit details".to_string()
        }
        _ => format!("Checking {}", call.tool.spec().summary.to_ascii_lowercase()),
    }
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn has_any_token(haystack: &str, needles: &[&str]) -> bool {
    haystack
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| {
            needles
                .iter()
                .any(|needle| token.eq_ignore_ascii_case(needle))
        })
}

fn room_mode_status_label(room_mode: &str) -> &'static str {
    match room_mode {
        "video" => "watch",
        "audio" => "audio",
        "youtube" => "YouTube",
        "web" => "web",
        "screen" => "screen share",
        "create" => "create",
        "play" => "play",
        _ => "filtered",
    }
}

fn push_tool(
    planned: &mut Vec<PlannedToolCall>,
    seen: &mut HashSet<&'static str>,
    tool: AssistantToolName,
    input: AssistantToolInput,
) {
    if seen.insert(tool.as_str()) {
        planned.push(PlannedToolCall { tool, input });
    }
}

fn apply_follow_up_tool_hints(
    message: &str,
    history: &[AssistantHistoryMessage],
    planned: &mut Vec<PlannedToolCall>,
    seen: &mut HashSet<&'static str>,
) {
    if apply_follow_up_entity_reference(message, history, planned, seen) {
        return;
    }

    let recent_tools = recent_grounded_tools(history);
    if recent_tools.is_empty() {
        return;
    }

    for tool in recent_tools {
        match tool {
            AssistantToolName::CalendarCreateEvent
            | AssistantToolName::CalendarCreateBirthday
            | AssistantToolName::CalendarDeleteEvent
            | AssistantToolName::DocumentCreateDownload
            | AssistantToolName::ConversationsArchiveSelection
            | AssistantToolName::ConversationsDeleteSelection
            | AssistantToolName::ConversationsMoveToGroupSelection => {}
            AssistantToolName::CalendarListEvents => {
                if let Some(query) = extract_calendar_event_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetEventDetails,
                        extract_calendar_window(message, 30, Some(query)),
                    );
                } else if message_has_calendar_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarListEvents,
                        extract_calendar_window(message, 7, None),
                    );
                }
            }
            AssistantToolName::CalendarGetNextFreeDay => {
                if message_has_calendar_follow_up_hint(message)
                    || is_calendar_free_days_query(&message.to_ascii_lowercase())
                    || is_calendar_conflict_query(&message.to_ascii_lowercase())
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetNextFreeDay,
                        extract_calendar_window(message, 30, None),
                    );
                }
            }
            AssistantToolName::CalendarCountEvents => {
                if let Some(query) = extract_calendar_event_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetEventDetails,
                        extract_calendar_window(message, 30, Some(query)),
                    );
                } else if message_has_calendar_follow_up_hint(message)
                    || is_calendar_event_count_query(&message.to_ascii_lowercase())
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarCountEvents,
                        extract_calendar_window(message, 7, None),
                    );
                }
            }
            AssistantToolName::CalendarListBusyDays => {
                if let Some(query) = extract_calendar_event_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetEventDetails,
                        extract_calendar_window(message, 30, Some(query)),
                    );
                } else if message_has_calendar_follow_up_hint(message)
                    || is_calendar_busy_days_query(&message.to_ascii_lowercase())
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarListBusyDays,
                        extract_calendar_window(message, 7, None),
                    );
                }
            }
            AssistantToolName::CalendarGetNextEvent => {
                if let Some(query) = extract_calendar_event_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetEventDetails,
                        extract_calendar_window(message, 30, Some(query)),
                    );
                } else if message_has_calendar_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarListEvents,
                        extract_calendar_window(message, 7, None),
                    );
                } else if is_single_event_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_calendar_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::CalendarGetEventDetails,
                            extract_calendar_window(message, 30, Some(entity_label)),
                        );
                    }
                } else if is_next_calendar_event_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetNextEvent,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::CalendarGetNextEventTiming => {
                if is_next_calendar_event_timing_query(&message.to_ascii_lowercase())
                    || message_has_calendar_follow_up_hint(message)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetNextEventTiming,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::CalendarUpcomingBirthdays => {
                let birthday_query = extract_birthday_query(message);
                if message_has_calendar_follow_up_hint(message) || birthday_query.is_some() {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarUpcomingBirthdays,
                        birthday_calendar_window_input(message, birthday_query),
                    );
                }
            }
            AssistantToolName::CalendarGetEventDetails => {
                if let Some(query) = extract_calendar_event_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetEventDetails,
                        extract_calendar_window(message, 30, Some(query)),
                    );
                } else if message_has_calendar_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarListEvents,
                        extract_calendar_window(message, 7, None),
                    );
                } else if is_single_event_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_calendar_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::CalendarGetEventDetails,
                            extract_calendar_window(message, 30, Some(entity_label)),
                        );
                    }
                }
            }
            AssistantToolName::CalendarListDateConflicts
            | AssistantToolName::CalendarListFreeDays
            | AssistantToolName::CalendarListOverlappingEvents => {
                if message_has_calendar_follow_up_hint(message)
                    || is_calendar_conflict_query(&message.to_ascii_lowercase())
                    || is_calendar_free_days_query(&message.to_ascii_lowercase())
                {
                    push_tool(
                        planned,
                        seen,
                        tool,
                        extract_calendar_window(message, 7, None),
                    );
                }
            }
            AssistantToolName::ChannelsListUnreadActivity => {
                if message_has_channel_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::ChannelsListUnreadActivity,
                        AssistantToolInput::ChannelsFilter {
                            query: extract_channel_query(message),
                        },
                    );
                }
            }
            AssistantToolName::ChannelsGetTranscriptSummary => {
                if is_transcript_summary_query(&message.to_ascii_lowercase())
                    || message_has_transcript_follow_up_hint(message)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::ChannelsGetTranscriptSummary,
                        AssistantToolInput::ChannelsFilter {
                            query: extract_transcript_channel_query(message)
                                .or_else(|| extract_channel_query(message))
                                .or_else(|| recent_transcript_query_hint(history)),
                        },
                    );
                }
            }
            AssistantToolName::DownloadsListAvailableArtifacts
            | AssistantToolName::DownloadsGetArtifactDetails
            | AssistantToolName::DownloadsGetArtifactSource
            | AssistantToolName::DownloadsGetReleaseNotes
            | AssistantToolName::DownloadsGetArtifactChecksum
            | AssistantToolName::DownloadsGetArtifactInstallSteps
            | AssistantToolName::DownloadsGetArtifactCompatibility => {
                if let Some(query) = extract_download_artifact_source_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetArtifactSource,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(query) = extract_download_artifact_release_notes_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetReleaseNotes,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(query) = extract_download_artifact_checksum_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetArtifactChecksum,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(query) = extract_download_artifact_install_steps_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetArtifactInstallSteps,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(query) = extract_download_artifact_compatibility_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetArtifactCompatibility,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(query) = extract_download_artifact_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsGetArtifactDetails,
                        AssistantToolInput::DownloadsFilter {
                            query: Some(query),
                            availability: extract_downloads_availability(message),
                        },
                    );
                } else if let Some(entity_label) = recent_single_download_entity_label(history) {
                    if message_has_download_checksum_hint(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::DownloadsGetArtifactChecksum,
                            AssistantToolInput::DownloadsFilter {
                                query: Some(entity_label),
                                availability: extract_downloads_availability(message),
                            },
                        );
                    } else if message_has_download_install_steps_hint(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::DownloadsGetArtifactInstallSteps,
                            AssistantToolInput::DownloadsFilter {
                                query: Some(entity_label),
                                availability: extract_downloads_availability(message),
                            },
                        );
                    } else if message_has_download_compatibility_hint(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::DownloadsGetArtifactCompatibility,
                            AssistantToolInput::DownloadsFilter {
                                query: Some(entity_label),
                                availability: extract_downloads_availability(message),
                            },
                        );
                    } else if is_single_download_detail_follow_up(message, history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::DownloadsGetArtifactDetails,
                            AssistantToolInput::DownloadsFilter {
                                query: Some(entity_label),
                                availability: extract_downloads_availability(message),
                            },
                        );
                    }
                } else if message_has_downloads_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsListAvailableArtifacts,
                        extract_downloads_follow_up_filter(message),
                    );
                }
            }
            AssistantToolName::NetworkGetTopologySummary => {
                let before = planned.len();
                push_network_diagnostics_tools(message, planned, seen);
                if planned.len() == before {
                    if let Some(query) = extract_network_interface_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::NetworkGetInterfaceDetails,
                            AssistantToolInput::NetworkInterface { query },
                        );
                    } else if is_single_network_detail_follow_up(message, history) {
                        if let Some(entity_label) = recent_single_network_entity_label(history) {
                            push_tool(
                                planned,
                                seen,
                                AssistantToolName::NetworkGetInterfaceDetails,
                                AssistantToolInput::NetworkInterface {
                                    query: entity_label,
                                },
                            );
                        }
                    } else if message_has_network_follow_up_hint(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::NetworkGetTopologySummary,
                            AssistantToolInput::None,
                        );
                    }
                }
            }
            AssistantToolName::NetworkGetInterfaceDetails
            | AssistantToolName::NetworkGetInterfaceByIp => {
                let before = planned.len();
                push_network_diagnostics_tools(message, planned, seen);
                if planned.len() == before {
                    if let Some(query) = extract_network_interface_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::NetworkGetInterfaceDetails,
                            AssistantToolInput::NetworkInterface { query },
                        );
                    } else if is_single_network_detail_follow_up(message, history) {
                        if let Some(entity_label) = recent_single_network_entity_label(history) {
                            push_tool(
                                planned,
                                seen,
                                AssistantToolName::NetworkGetInterfaceDetails,
                                AssistantToolInput::NetworkInterface {
                                    query: entity_label,
                                },
                            );
                        }
                    } else if message_has_network_follow_up_hint(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::NetworkGetTopologySummary,
                            AssistantToolInput::None,
                        );
                    }
                }
            }
            AssistantToolName::NetworkGetDefaultRoute => {
                let before = planned.len();
                push_network_diagnostics_tools(message, planned, seen);
                if planned.len() == before
                    && (is_network_default_route_query(&message.to_ascii_lowercase())
                        || message_has_network_follow_up_hint(message))
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::NetworkGetDefaultRoute,
                        AssistantToolInput::NetworkDefaultRoute {
                            query: extract_network_default_route_query(message),
                        },
                    );
                }
            }
            AssistantToolName::NetworkGetHostnameAliases => {
                let before = planned.len();
                push_network_diagnostics_tools(message, planned, seen);
                if planned.len() == before
                    && (is_network_hostname_aliases_query(&message.to_ascii_lowercase())
                        || message_has_network_follow_up_hint(message))
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::NetworkGetHostnameAliases,
                        AssistantToolInput::NetworkHostnameAliases {
                            query: extract_network_hostname_aliases_query(message),
                        },
                    );
                }
            }
            AssistantToolName::NetworkGetDnsServers => {
                let before = planned.len();
                push_network_diagnostics_tools(message, planned, seen);
                if planned.len() == before
                    && (is_network_dns_servers_query(&message.to_ascii_lowercase())
                        || message_has_network_follow_up_hint(message))
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::NetworkGetDnsServers,
                        AssistantToolInput::NetworkDnsServers {
                            query: extract_network_dns_servers_query(message),
                        },
                    );
                }
            }
            AssistantToolName::NetworkGetRouteTable
            | AssistantToolName::NetworkGetActiveConnections
            | AssistantToolName::NetworkGetInterfaceCounters
            | AssistantToolName::NetworkGetWifiStatus
            | AssistantToolName::NetworkGetVpnStatus => {
                push_network_diagnostics_tools(message, planned, seen);
            }
            AssistantToolName::SystemGetServiceHealth => {
                if let Some(query) = extract_service_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceDetail,
                        AssistantToolInput::SystemService { query },
                    );
                } else if is_single_service_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_service_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::SystemGetServiceDetail,
                            AssistantToolInput::SystemService {
                                query: entity_label,
                            },
                        );
                    }
                } else if is_service_health_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceHealth,
                        AssistantToolInput::None,
                    );
                } else if message_has_service_health_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceHealth,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetKernelInfo
            | AssistantToolName::SystemGetCpuTopology
            | AssistantToolName::SystemGetTemperatureSensors
            | AssistantToolName::SystemGetBlockDeviceInventory
            | AssistantToolName::SystemGetFilesystemTable
            | AssistantToolName::SystemGetGpuInventory
            | AssistantToolName::SystemGetPciDevices
            | AssistantToolName::SystemGetUsbDevices
            | AssistantToolName::SystemGetBootLogSummary
            | AssistantToolName::SystemGetJournalSummary
            | AssistantToolName::SystemGetProcessDetail
            | AssistantToolName::SystemGetListenerDetail
            | AssistantToolName::SystemGetDiskUsageDetail => {
                push_system_diagnostics_tools(message, planned, seen);
            }
            AssistantToolName::SystemGetPortConflicts
            | AssistantToolName::SystemGetPortConflictDetail => {
                if let Some(query) = extract_port_conflict_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetPortConflictDetail,
                        AssistantToolInput::SystemPortConflicts { query: Some(query) },
                    );
                } else if is_port_conflicts_query(&message.to_ascii_lowercase())
                    || message_has_port_conflicts_follow_up_hint(message)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetPortConflicts,
                        AssistantToolInput::SystemPortConflicts {
                            query: extract_port_conflicts_query(message),
                        },
                    );
                }
            }
            AssistantToolName::SystemGetFailedUnits => {
                if is_failed_units_query(&message.to_ascii_lowercase())
                    || message_has_failed_units_follow_up_hint(message)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetFailedUnits,
                        AssistantToolInput::SystemFailedUnits {
                            query: extract_failed_units_query(message),
                        },
                    );
                }
            }
            AssistantToolName::SystemGetFailedUnitDetail => {
                if let Some(query) = extract_failed_unit_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetFailedUnitDetail,
                        AssistantToolInput::SystemFailedUnits { query: Some(query) },
                    );
                } else if message_has_failed_units_follow_up_hint(message) {
                    if let Some(entity_label) = recent_single_failed_unit_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::SystemGetFailedUnitDetail,
                            AssistantToolInput::SystemFailedUnits {
                                query: Some(entity_label),
                            },
                        );
                    }
                }
            }
            AssistantToolName::SystemGetServiceDetail => {
                if let Some(query) = extract_service_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceDetail,
                        AssistantToolInput::SystemService { query },
                    );
                } else if is_single_service_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_service_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::SystemGetServiceDetail,
                            AssistantToolInput::SystemService {
                                query: entity_label,
                            },
                        );
                    }
                } else if message_has_service_health_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceHealth,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::RoomsListActive | AssistantToolName::RoomsGetRoomSummary => {
                if message_has_room_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::RoomsListActive,
                        extract_room_filter(message),
                    );
                }
            }
            AssistantToolName::RoomsListJoinable => {
                if message_has_room_follow_up_hint(message) || is_joinable_rooms_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::RoomsListJoinable,
                        extract_room_filter(message),
                    );
                }
            }
            AssistantToolName::ServersListMinecraftStatus
            | AssistantToolName::ServersGetMinecraftServerSummary => {
                if message_has_server_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::ServersListMinecraftStatus,
                        extract_server_filter(message),
                    );
                }
            }
            AssistantToolName::LibrariesListAccessible
            | AssistantToolName::LibrariesGetLibrarySummary
            | AssistantToolName::LibrarySearchTitles
            | AssistantToolName::LibraryGetItemSummary
            | AssistantToolName::LibraryGetItemMediaDetails
            | AssistantToolName::LibraryGetItemSourcePaths
            | AssistantToolName::LibrariesGetRecentlyAdded
            | AssistantToolName::LibrariesFindDuplicateTitles
            | AssistantToolName::LibrariesListMissingMetadata => {
                let lower = message.to_ascii_lowercase();
                if is_library_duplicate_titles_query(&lower) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrariesFindDuplicateTitles,
                        AssistantToolInput::None,
                    );
                } else if is_library_missing_metadata_query(&lower) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrariesListMissingMetadata,
                        AssistantToolInput::None,
                    );
                } else if let Some(query) = extract_library_media_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibraryGetItemMediaDetails,
                        AssistantToolInput::LibrarySearch { query },
                    );
                } else if let Some(query) = extract_library_source_paths_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibraryGetItemSourcePaths,
                        AssistantToolInput::LibrarySearch { query },
                    );
                } else if is_single_library_media_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_library_item_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::LibraryGetItemMediaDetails,
                            AssistantToolInput::LibrarySearch {
                                query: entity_label,
                            },
                        );
                    }
                } else if is_single_library_source_paths_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_library_item_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::LibraryGetItemSourcePaths,
                            AssistantToolInput::LibrarySearch {
                                query: entity_label,
                            },
                        );
                    }
                } else if let Some(query) = extract_library_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrariesGetLibrarySummary,
                        AssistantToolInput::LibrarySearch { query },
                    );
                } else if is_single_library_detail_follow_up(message, history) {
                    if let Some(entity_label) = recent_single_library_entity_label(history) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::LibrariesGetLibrarySummary,
                            AssistantToolInput::LibrarySearch {
                                query: entity_label,
                            },
                        );
                    }
                } else if let Some(query) = extract_library_follow_up_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrarySearchTitles,
                        AssistantToolInput::LibrarySearch { query },
                    );
                } else if is_recent_library_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrariesGetRecentlyAdded,
                        AssistantToolInput::LibraryRecent {
                            query: extract_recent_library_query(message),
                        },
                    );
                } else if message_has_library_listing_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrariesListAccessible,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::WeatherGetCurrent
            | AssistantToolName::WeatherGetForecast
            | AssistantToolName::WeatherGetHistory
            | AssistantToolName::WeatherResolveLocationAlias
            | AssistantToolName::WeatherGetHourlyWindow
            | AssistantToolName::WeatherGetForecastForDate
            | AssistantToolName::WeatherGetRecentHistoryForDate => {
                if let Some((tool, input)) = extract_weather_follow_up_call(message, history) {
                    push_tool(planned, seen, tool, input);
                }
            }
            AssistantToolName::WebSearchPublicWeb
            | AssistantToolName::WebFetchPublicPageSummary => {
                if let Some(url) = extract_public_web_url(message) {
                    let category = infer_curated_web_category_for_url(&url)
                        .or_else(|| recent_web_category(history));
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::WebFetchPublicPageSummary,
                        AssistantToolInput::WebFetch { url, category },
                    );
                } else if let Some(query) = extract_public_web_search_query(message) {
                    let category = infer_curated_web_category_slug(message)
                        .or_else(|| recent_web_category(history));
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::WebSearchPublicWeb,
                        AssistantToolInput::WebSearch { query, category },
                    );
                }
            }
            AssistantToolName::AccountGetProfileSummary => {}
            AssistantToolName::SystemGetAiRuntimeSummary => {
                let before = planned.len();
                push_system_diagnostics_tools(message, planned, seen);
                if planned.len() == before
                    && (is_ai_runtime_query(&message.to_ascii_lowercase())
                        || message_has_ai_runtime_follow_up_hint(message))
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetAiRuntimeSummary,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetHostRuntimeSummary => {
                let before = planned.len();
                push_system_diagnostics_tools(message, planned, seen);
                if planned.len() == before && message_has_host_runtime_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetHostRuntimeSummary,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetCurrentDateTime => {
                if is_current_datetime_query(&message.to_ascii_lowercase())
                    || message_has_current_datetime_tool_follow_up_hint(message)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetCurrentDateTime,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetBackupSummary => {
                if is_backup_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetBackupSummary,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetTranscodeSummary => {
                if is_transcode_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetTranscodeSummary,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::SystemGetStorageSummary
            | AssistantToolName::SystemGetStoragePathDetail
            | AssistantToolName::SystemGetMountDetail => {
                if let Some(query) = extract_mount_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetMountDetail,
                        AssistantToolInput::SystemService { query },
                    );
                } else if let Some(query) = extract_storage_path_detail_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetStoragePathDetail,
                        AssistantToolInput::SystemService { query },
                    );
                } else if is_storage_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetStorageSummary,
                        AssistantToolInput::None,
                    );
                }
            }
            AssistantToolName::MemoryListRecentFacts
            | AssistantToolName::MemoryListRecentEntities
            | AssistantToolName::MemorySearchFacts
            | AssistantToolName::MemorySearchEntities
            | AssistantToolName::MemoryFindExactEntity
            | AssistantToolName::MemoryGetEntityRelations
            | AssistantToolName::MemoryGetPersonSummary
            | AssistantToolName::MemoryGetEntityRelationPath
            | AssistantToolName::MemoryListRecentChanges
            | AssistantToolName::MemoryListConflictingFacts
            | AssistantToolName::MemoryGetEntityProvenance => {
                let lower = message.to_ascii_lowercase();
                if !message_has_other_domain_context(&lower)
                    && extract_memory_relation_path_query(message).is_some()
                {
                    if let Some((source, target)) = extract_memory_relation_path_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemoryGetEntityRelationPath,
                            AssistantToolInput::SystemService {
                                query: format!("{source} || {target}"),
                            },
                        );
                    }
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_relation_query(&lower)
                {
                    if let Some(query) = extract_memory_relation_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemoryGetEntityRelations,
                            AssistantToolInput::SystemService { query },
                        );
                    }
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_recent_changes_query(&lower)
                {
                    let input = extract_memory_recent_changes_query(message)
                        .map(|query| AssistantToolInput::SystemService { query })
                        .unwrap_or(AssistantToolInput::None);
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::MemoryListRecentChanges,
                        input,
                    );
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_conflict_query(&lower)
                {
                    let input = extract_memory_conflict_query(message)
                        .map(|query| AssistantToolInput::SystemService { query })
                        .unwrap_or(AssistantToolInput::None);
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::MemoryListConflictingFacts,
                        input,
                    );
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_provenance_query(&lower)
                {
                    if let Some(query) = extract_memory_provenance_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemoryGetEntityProvenance,
                            AssistantToolInput::SystemService { query },
                        );
                    }
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_person_summary_query(&lower)
                {
                    if let Some(query) = extract_memory_person_summary_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemoryGetPersonSummary,
                            AssistantToolInput::SystemService { query },
                        );
                    }
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_recent_entity_query(&lower)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::MemoryListRecentEntities,
                        AssistantToolInput::None,
                    );
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_recent_query(&lower)
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::MemoryListRecentFacts,
                        AssistantToolInput::None,
                    );
                } else if !message_has_other_domain_context(&lower)
                    && is_memory_exact_entity_query(&lower)
                {
                    if let Some(query) = extract_memory_exact_entity_query(message) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemoryFindExactEntity,
                            AssistantToolInput::SystemService { query },
                        );
                    }
                } else if let Some(query) = extract_memory_query(message) {
                    if is_memory_entity_query(&lower) {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemorySearchEntities,
                            AssistantToolInput::SystemService { query },
                        );
                    } else {
                        push_tool(
                            planned,
                            seen,
                            AssistantToolName::MemorySearchFacts,
                            AssistantToolInput::SystemService { query },
                        );
                    }
                } else if let Some(query) = recent_memory_person_label(history)
                    .filter(|_| message_has_memory_person_follow_up_hint(message))
                {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::MemoryGetPersonSummary,
                        AssistantToolInput::SystemService { query },
                    );
                }
            }
            AssistantToolName::SystemGetRecentErrors => {
                if is_recent_errors_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetRecentErrors,
                        AssistantToolInput::None,
                    );
                }
            }
            _ => {}
        }
    }
}

fn apply_follow_up_entity_reference(
    message: &str,
    history: &[AssistantHistoryMessage],
    planned: &mut Vec<PlannedToolCall>,
    seen: &mut HashSet<&'static str>,
) -> bool {
    let Some(reference) = extract_follow_up_entity_reference(message) else {
        return false;
    };
    let contexts = recent_follow_up_contexts(history);
    if contexts.is_empty() {
        return false;
    }

    let matching_contexts: Vec<_> = contexts
        .iter()
        .filter(|context| follow_up_context_matches_message(context, message))
        .copied()
        .collect();

    let context = if matching_contexts.len() == 1 {
        matching_contexts[0]
    } else if contexts.len() == 1 {
        contexts[0]
    } else {
        return false;
    };

    let Some(entity) = resolve_follow_up_entity(context, &reference) else {
        return false;
    };

    match context.tool.as_str() {
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_get_next_event_timing"
        | "calendar_get_event_details" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::CalendarGetEventDetails,
                AssistantToolInput::CalendarWindow {
                    from_date: context
                        .input_hint
                        .calendar_from_date
                        .clone()
                        .unwrap_or_else(|| assistant_local_today().format("%F").to_string()),
                    to_date: context
                        .input_hint
                        .calendar_to_date
                        .clone()
                        .unwrap_or_else(|| {
                            (assistant_local_today() + Duration::days(30))
                                .format("%F")
                                .to_string()
                        }),
                    label: context
                        .input_hint
                        .calendar_label
                        .clone()
                        .unwrap_or_else(|| "the current calendar window".to_string()),
                    query: Some(entity.label.clone()),
                },
            );
            true
        }
        "calendar_count_events" | "calendar_list_busy_days" => {
            let selected_date = entity
                .identifier
                .clone()
                .or_else(|| context.input_hint.calendar_from_date.clone())
                .unwrap_or_else(|| assistant_local_today().format("%F").to_string());
            push_tool(
                planned,
                seen,
                AssistantToolName::CalendarListEvents,
                AssistantToolInput::CalendarWindow {
                    from_date: selected_date.clone(),
                    to_date: selected_date,
                    label: entity.label.clone(),
                    query: None,
                },
            );
            true
        }
        "memory_list_recent_entities"
        | "memory_search_entities"
        | "memory_find_exact_entity"
        | "memory_get_entity_relations"
        | "memory_get_entity_relation_path"
        | "memory_get_entity_provenance"
        | "memory_get_person_summary" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::MemoryGetPersonSummary,
                AssistantToolInput::SystemService {
                    query: follow_up_entity_query_text(entity),
                },
            );
            true
        }
        "channels_get_transcript_summary" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::ChannelsGetTranscriptSummary,
                AssistantToolInput::ChannelsFilter {
                    query: Some(entity.label.clone()),
                },
            );
            true
        }
        "servers_list_minecraft_status" | "servers_get_minecraft_server_summary" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::ServersGetMinecraftServerSummary,
                AssistantToolInput::ServerFilter {
                    query: Some(entity.label.clone()),
                    availability: extract_server_availability(message)
                        .or_else(|| context.input_hint.server_availability.clone()),
                },
            );
            true
        }
        "rooms_list_active" | "rooms_list_joinable" | "rooms_get_room_summary" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::RoomsGetRoomSummary,
                AssistantToolInput::RoomsFilter {
                    room_mode: detect_room_mode(message)
                        .or_else(|| context.input_hint.room_mode.clone()),
                    query: Some(entity.label.clone()),
                },
            );
            true
        }
        "libraries_list_accessible" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::LibrariesGetLibrarySummary,
                AssistantToolInput::LibrarySearch {
                    query: entity.label.clone(),
                },
            );
            true
        }
        "libraries_get_library_summary" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::LibrariesGetLibrarySummary,
                AssistantToolInput::LibrarySearch {
                    query: entity.label.clone(),
                },
            );
            true
        }
        "library_search_titles"
        | "library_get_item_summary"
        | "library_get_item_media_details"
        | "library_get_item_source_paths"
        | "libraries_get_recently_added" => {
            let tool = if extract_library_source_paths_query(message).is_some()
                || is_single_library_source_paths_follow_up(message, history)
                || context.tool == "library_get_item_source_paths"
            {
                AssistantToolName::LibraryGetItemSourcePaths
            } else if message_has_library_media_follow_up_hint(message)
                || extract_library_media_detail_query(message).is_some()
                || is_single_library_media_detail_follow_up(message, history)
            {
                AssistantToolName::LibraryGetItemMediaDetails
            } else {
                AssistantToolName::LibraryGetItemSummary
            };
            push_tool(
                planned,
                seen,
                tool,
                AssistantToolInput::LibrarySearch {
                    query: entity.label.clone(),
                },
            );
            true
        }
        "downloads_list_available_artifacts" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::DownloadsGetArtifactDetails,
                AssistantToolInput::DownloadsFilter {
                    query: Some(entity.label.clone()),
                    availability: extract_downloads_availability(message)
                        .or_else(|| context.input_hint.downloads_availability.clone()),
                },
            );
            true
        }
        "downloads_get_artifact_details"
        | "downloads_get_artifact_source"
        | "downloads_get_release_notes"
        | "downloads_get_artifact_checksum"
        | "downloads_get_artifact_install_steps"
        | "downloads_get_artifact_compatibility" => {
            let tool = if extract_download_artifact_source_query(message).is_some()
                || message_has_download_source_hint(message)
                || context.tool == "downloads_get_artifact_source"
            {
                AssistantToolName::DownloadsGetArtifactSource
            } else if extract_download_artifact_release_notes_query(message).is_some()
                || message_has_download_release_notes_hint(message)
                || context.tool == "downloads_get_release_notes"
            {
                AssistantToolName::DownloadsGetReleaseNotes
            } else if extract_download_artifact_checksum_query(message).is_some()
                || message_has_download_checksum_hint(message)
                || context.tool == "downloads_get_artifact_checksum"
            {
                AssistantToolName::DownloadsGetArtifactChecksum
            } else if extract_download_artifact_install_steps_query(message).is_some()
                || message_has_download_install_steps_hint(message)
                || context.tool == "downloads_get_artifact_install_steps"
            {
                AssistantToolName::DownloadsGetArtifactInstallSteps
            } else if extract_download_artifact_compatibility_query(message).is_some()
                || message_has_download_compatibility_hint(message)
                || context.tool == "downloads_get_artifact_compatibility"
            {
                AssistantToolName::DownloadsGetArtifactCompatibility
            } else {
                AssistantToolName::DownloadsGetArtifactDetails
            };
            push_tool(
                planned,
                seen,
                tool,
                AssistantToolInput::DownloadsFilter {
                    query: Some(entity.label.clone()),
                    availability: extract_downloads_availability(message)
                        .or_else(|| context.input_hint.downloads_availability.clone()),
                },
            );
            true
        }
        "network_get_topology_summary" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::NetworkGetInterfaceDetails,
                AssistantToolInput::NetworkInterface { query },
            );
            true
        }
        "network_get_interface_details" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::NetworkGetInterfaceDetails,
                AssistantToolInput::NetworkInterface { query },
            );
            true
        }
        "network_get_default_route" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::NetworkGetDefaultRoute,
                AssistantToolInput::NetworkDefaultRoute { query: Some(query) },
            );
            true
        }
        "network_get_hostname_aliases" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::NetworkGetHostnameAliases,
                AssistantToolInput::NetworkHostnameAliases { query: Some(query) },
            );
            true
        }
        "system_get_service_health" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetServiceDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_service_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetServiceDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_mount_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetMountDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_process_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetProcessDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_listener_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetListenerDetail,
                AssistantToolInput::SystemPortConflicts { query: Some(query) },
            );
            true
        }
        "system_get_disk_usage_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetDiskUsageDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_port_conflicts" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetPortConflicts,
                AssistantToolInput::SystemPortConflicts { query: Some(query) },
            );
            true
        }
        "system_get_port_conflict_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetPortConflictDetail,
                AssistantToolInput::SystemPortConflicts { query: Some(query) },
            );
            true
        }
        "system_get_storage_summary" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetStoragePathDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_storage_path_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetStoragePathDetail,
                AssistantToolInput::SystemService { query },
            );
            true
        }
        "system_get_failed_units" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetFailedUnitDetail,
                AssistantToolInput::SystemFailedUnits { query: Some(query) },
            );
            true
        }
        "system_get_failed_unit_detail" => {
            let query = entity
                .identifier
                .clone()
                .or_else(|| Some(entity.label.clone()))
                .unwrap_or_else(|| entity.label.clone());
            push_tool(
                planned,
                seen,
                AssistantToolName::SystemGetFailedUnitDetail,
                AssistantToolInput::SystemFailedUnits { query: Some(query) },
            );
            true
        }
        "web_search_public_web" | "web_fetch_public_page_summary" => {
            let url = entity
                .identifier
                .clone()
                .or_else(|| extract_public_web_url(&entity.label));
            let Some(url) = url else {
                return false;
            };
            let category = web_category_from_topic_key(entity.topic_key.as_deref())
                .or_else(|| infer_curated_web_category_for_url(&url));
            push_tool(
                planned,
                seen,
                AssistantToolName::WebFetchPublicPageSummary,
                AssistantToolInput::WebFetch { url, category },
            );
            true
        }
        _ => false,
    }
}

fn recent_grounded_tools(history: &[AssistantHistoryMessage]) -> Vec<AssistantToolName> {
    history
        .iter()
        .rev()
        .find(|message| {
            message.role.eq_ignore_ascii_case("assistant") && !message.grounding_tools.is_empty()
        })
        .map(|message| {
            message
                .grounding_tools
                .iter()
                .filter_map(|tool| AssistantToolName::from_str(tool))
                .collect()
        })
        .unwrap_or_default()
}

fn recent_follow_up_contexts(
    history: &[AssistantHistoryMessage],
) -> Vec<&AssistantFollowUpContext> {
    history
        .iter()
        .rev()
        .find(|message| {
            message.role.eq_ignore_ascii_case("assistant") && !message.follow_up_contexts.is_empty()
        })
        .map(|message| message.follow_up_contexts.iter().collect())
        .unwrap_or_default()
}

fn recent_single_calendar_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "calendar_list_events"
                | "calendar_get_next_event"
                | "calendar_get_next_event_timing"
                | "calendar_list_date_conflicts"
                | "calendar_list_free_days"
                | "calendar_count_events"
                | "calendar_list_busy_days"
                | "calendar_get_event_details"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_download_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "downloads_list_available_artifacts"
                | "downloads_get_artifact_details"
                | "downloads_get_artifact_source"
                | "downloads_get_release_notes"
                | "downloads_get_artifact_checksum"
                | "downloads_get_artifact_install_steps"
                | "downloads_get_artifact_compatibility"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_network_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "network_get_topology_summary"
                | "network_get_interface_details"
                | "network_get_interface_by_ip"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_service_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_service_health" | "system_get_service_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_failed_unit_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_failed_units" | "system_get_failed_unit_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_storage_path_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_storage_summary"
                | "system_get_storage_path_detail"
                | "system_get_disk_usage_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_library_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "libraries_list_accessible"
                | "libraries_get_library_summary"
                | "library_search_titles"
                | "library_get_item_summary"
                | "library_get_item_media_details"
                | "libraries_get_recently_added"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_library_item_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "library_search_titles"
                | "library_get_item_summary"
                | "library_get_item_media_details"
                | "library_get_item_source_paths"
                | "libraries_get_recently_added"
                | "libraries_list_missing_metadata"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_process_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(context.tool.as_str(), "system_get_process_detail") && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_listener_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_listener_detail"
                | "system_get_port_conflicts"
                | "system_get_port_conflict_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn follow_up_entity_query_text(entity: &AssistantFollowUpEntity) -> String {
    let label = entity.label.trim();
    if let Some((base, suffix)) = label.rsplit_once(" (") {
        if suffix.ends_with(')') {
            let candidate = base.trim();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }
    label.to_string()
}

fn recent_memory_person_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "memory_list_recent_entities"
                | "memory_search_entities"
                | "memory_find_exact_entity"
                | "memory_get_entity_relations"
                | "memory_get_entity_relation_path"
                | "memory_get_entity_provenance"
                | "memory_get_person_summary"
        )
    })?;
    let entity = context.entities.first()?;
    Some(follow_up_entity_query_text(entity))
}

fn message_has_library_media_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_library_media_detail_query(message).is_some()
        || has_any(
            &lower,
            &[
                "its path",
                "it path",
                "the path",
                "file path",
                "media path",
                "what is its path",
                "what's its path",
                "what is its file path",
                "what's its file path",
                "what is its media path",
                "what's its media path",
                "what is its poster",
                "what's its poster",
                "what is its artwork",
                "what's its artwork",
                "where is it stored",
                "where's it stored",
                "where is this stored",
                "where's this stored",
                "show me the artwork",
                "tell me the artwork",
                "tell me more about the artwork",
            ],
        )
}

fn message_has_storage_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_storage_path_detail_query(message).is_some()
        || has_any(
            &lower,
            &[
                "its path",
                "it path",
                "the path",
                "file path",
                "storage path",
                "mount point",
                "where is it stored",
                "where's it stored",
                "where is this stored",
                "where's this stored",
                "disk usage",
                "disk space",
                "storage usage",
                "how much space is on it",
                "how much free space is on it",
                "how full is it",
                "that path",
                "that directory",
                "that folder",
                "storage details",
            ],
        )
}

fn message_has_process_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_process_detail_query(message).is_some()
        || has_any(
            &lower,
            &[
                "process",
                "processes",
                "pid",
                "command line",
                "command-line",
                "cmdline",
                "who is using it",
                "what process is using it",
                "tell me more about it",
                "more about it",
                "describe it",
            ],
        )
}

fn message_has_listener_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_listener_detail_query(message).is_some()
        || has_any(
            &lower,
            &[
                "listener",
                "listeners",
                "socket",
                "sockets",
                "listening on",
                "what is listening on it",
                "what is listening on that port",
                "what port is it listening on",
                "tell me more about it",
                "more about it",
                "describe it",
            ],
        )
}

fn is_single_library_media_detail_follow_up(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> bool {
    if recent_single_library_item_entity_label(history).is_none() {
        return false;
    }

    message_has_library_media_follow_up_hint(message)
}

fn is_single_storage_path_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_storage_path_entity_label(history).is_none() {
        return false;
    }

    message_has_storage_follow_up_hint(message)
}

fn is_single_network_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_network_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "what is its ip",
            "what's its ip",
            "what is its address",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn is_single_service_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_service_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "what is its status",
            "what's its status",
            "what is its health",
            "what's its health",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn recent_single_mount_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_storage_summary"
                | "system_get_storage_path_detail"
                | "system_get_mount_detail"
                | "system_get_disk_usage_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn recent_single_port_conflict_entity_label(history: &[AssistantHistoryMessage]) -> Option<String> {
    let contexts = recent_follow_up_contexts(history);
    let context = contexts.iter().find(|context| {
        matches!(
            context.tool.as_str(),
            "system_get_port_conflicts"
                | "system_get_port_conflict_detail"
                | "system_get_listener_detail"
        ) && context.entities.len() == 1
    })?;
    Some(context.entities.first()?.label.clone())
}

fn is_single_process_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_process_entity_label(history).is_none() {
        return false;
    }

    message_has_process_follow_up_hint(message)
}

fn is_single_listener_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_listener_entity_label(history).is_none() {
        return false;
    }

    message_has_listener_follow_up_hint(message)
        || message_has_port_conflicts_follow_up_hint(message)
}

fn is_single_mount_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_mount_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "what is it mounted on",
            "what's it mounted on",
            "what filesystem is it on",
            "what file system is it on",
            "what is it on",
            "what is it mounted on",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn is_single_port_conflict_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_port_conflict_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "who is using it",
            "what process is using it",
            "what is listening on it",
            "what is using it",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn follow_up_context_matches_message(context: &AssistantFollowUpContext, message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    match context.tool.as_str() {
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_get_next_event_timing"
        | "calendar_list_date_conflicts"
        | "calendar_list_free_days"
        | "calendar_count_events"
        | "calendar_list_busy_days"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details" => {
            message_has_calendar_follow_up_hint(message)
                || is_calendar_conflict_query(&lower)
                || is_calendar_free_days_query(&lower)
                || is_calendar_busy_days_query(&lower)
                || is_calendar_event_count_query(&lower)
                || is_next_calendar_event_timing_query(&lower)
                || extract_calendar_event_detail_query(message).is_some()
                || extract_birthday_query(message).is_some()
                || has_any(
                    &lower,
                    &["calendar", "event", "events", "birthday", "birthdays"],
                )
        }
        "channels_list_unread_activity" => is_channel_activity_query(&lower),
        "channels_get_transcript_summary" => {
            is_transcript_summary_query(&lower) || message_has_transcript_follow_up_hint(message)
        }
        "servers_list_minecraft_status" | "servers_get_minecraft_server_summary" => {
            extract_server_availability(message).is_some()
                || extract_server_query(message).is_some()
                || has_any(&lower, &["server", "servers"])
        }
        "rooms_list_active" | "rooms_list_joinable" | "rooms_get_room_summary" => {
            detect_room_mode(message).is_some() || has_any(&lower, &["room", "rooms"])
        }
        "libraries_list_accessible"
        | "libraries_get_library_summary"
        | "library_search_titles"
        | "library_get_item_summary"
        | "library_get_item_media_details"
        | "library_get_item_source_paths"
        | "libraries_get_recently_added"
        | "libraries_find_duplicate_titles"
        | "libraries_list_missing_metadata" => {
            has_any(
                &lower,
                &[
                    "movie",
                    "movies",
                    "show",
                    "shows",
                    "song",
                    "songs",
                    "album",
                    "artist",
                    "library",
                    "libraries",
                ],
            ) || extract_library_detail_query(message).is_some()
                || message_has_library_media_follow_up_hint(message)
                || extract_library_media_detail_query(message).is_some()
                || extract_library_source_paths_query(message).is_some()
                || is_library_duplicate_titles_query(&lower)
                || is_library_missing_metadata_query(&lower)
        }
        "downloads_list_available_artifacts"
        | "downloads_get_artifact_details"
        | "downloads_get_artifact_source"
        | "downloads_get_release_notes"
        | "downloads_get_artifact_checksum"
        | "downloads_get_artifact_install_steps"
        | "downloads_get_artifact_compatibility" => {
            has_any(
                &lower,
                &[
                    "download",
                    "downloads",
                    "artifact",
                    "artifacts",
                    "extension",
                    "browser extension",
                    "app",
                    "planned",
                    "available",
                    "unavailable",
                    "companion",
                    "checksum",
                    "hash",
                    "install",
                    "installation",
                    "setup",
                    "compatible",
                    "compatibility",
                    "platform",
                    "architecture",
                ],
            ) || extract_download_artifact_detail_query(message).is_some()
                || extract_download_artifact_source_query(message).is_some()
                || extract_download_artifact_release_notes_query(message).is_some()
                || extract_download_artifact_checksum_query(message).is_some()
                || extract_download_artifact_install_steps_query(message).is_some()
                || extract_download_artifact_compatibility_query(message).is_some()
        }
        "memory_list_recent_facts"
        | "memory_list_recent_entities"
        | "memory_search_facts"
        | "memory_search_entities"
        | "memory_find_exact_entity"
        | "memory_get_entity_relations"
        | "memory_get_person_summary"
        | "memory_get_entity_relation_path"
        | "memory_list_recent_changes"
        | "memory_list_conflicting_facts"
        | "memory_get_entity_provenance" => {
            extract_follow_up_entity_reference(message).is_some()
                || is_memory_query(&lower)
                || message_has_memory_person_follow_up_hint(message)
        }
        "weather_get_current"
        | "weather_get_forecast"
        | "weather_get_history"
        | "weather_resolve_location_alias"
        | "weather_get_hourly_window"
        | "weather_get_forecast_for_date"
        | "weather_get_recent_history_for_date" => {
            !is_current_datetime_query(&lower)
                && !message_has_current_datetime_follow_up_hint(message)
                && (is_weather_query(&lower)
                    || extract_weather_location(message).is_some()
                    || extract_standalone_weather_location(message).is_some()
                    || message_has_weather_follow_up_hint(&lower))
        }
        "web_search_public_web" | "web_fetch_public_page_summary" => {
            extract_public_web_url(message).is_some()
                || extract_public_web_search_query(message).is_some()
        }
        "network_get_topology_summary"
        | "network_get_interface_details"
        | "network_get_interface_by_ip" => {
            extract_network_interface_query(message).is_some()
                || extract_network_interface_ip_query(message).is_some()
                || message_has_network_follow_up_hint(message)
        }
        "network_get_default_route" => {
            extract_network_default_route_query(message).is_some()
                || message_has_network_follow_up_hint(message)
        }
        "network_get_hostname_aliases" => {
            extract_network_hostname_aliases_query(message).is_some()
                || message_has_network_follow_up_hint(message)
        }
        "network_get_dns_servers" => {
            extract_network_dns_servers_query(message).is_some()
                || message_has_network_follow_up_hint(message)
        }
        "system_get_service_health" | "system_get_service_detail" => {
            extract_service_detail_query(message).is_some()
                || message_has_service_health_follow_up_hint(message)
        }
        "system_get_process_detail" => {
            extract_process_detail_query(message).is_some()
                || message_has_process_follow_up_hint(message)
        }
        "system_get_listener_detail" => {
            extract_listener_detail_query(message).is_some()
                || message_has_listener_follow_up_hint(message)
                || message_has_port_conflicts_follow_up_hint(message)
        }
        "system_get_port_conflicts" | "system_get_port_conflict_detail" => {
            extract_port_conflicts_query(message).is_some()
                || extract_port_conflict_detail_query(message).is_some()
                || message_has_port_conflicts_follow_up_hint(message)
        }
        "system_get_failed_units" | "system_get_failed_unit_detail" => {
            extract_failed_units_query(message).is_some()
                || extract_failed_unit_detail_query(message).is_some()
                || message_has_failed_units_follow_up_hint(message)
        }
        "system_get_storage_summary"
        | "system_get_storage_path_detail"
        | "system_get_mount_detail"
        | "system_get_disk_usage_detail" => {
            is_storage_query(&lower)
                || extract_mount_detail_query(message).is_some()
                || extract_storage_path_detail_query(message).is_some()
                || extract_disk_usage_detail_query(message).is_some()
                || message_has_storage_follow_up_hint(message)
        }
        "system_get_current_datetime" => {
            is_current_datetime_query(&lower)
                || message_has_current_datetime_tool_follow_up_hint(message)
        }
        "system_get_ai_runtime_summary" => {
            is_ai_runtime_query(&lower) || message_has_ai_runtime_follow_up_hint(message)
        }
        "system_get_host_runtime_summary" => is_host_runtime_query(&lower),
        "system_get_backup_summary" => is_backup_query(&lower),
        "system_get_transcode_summary" => is_transcode_query(&lower),
        "system_get_recent_errors" => is_recent_errors_query(&lower),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
enum FollowUpEntityReference {
    Ordinal(usize),
    Last,
    Demonstrative,
}

fn extract_follow_up_entity_reference(message: &str) -> Option<FollowUpEntityReference> {
    let lower = message.to_ascii_lowercase();
    if has_any(
        &lower,
        &[
            "that one",
            "that server",
            "that room",
            "that movie",
            "that show",
            "that event",
            "that process",
            "that listener",
            "that socket",
            "that download",
            "that artifact",
            "that path",
            "that mount",
            "that library item",
        ],
    ) {
        return Some(FollowUpEntityReference::Demonstrative);
    }
    if lower.contains("last one") || lower.contains("the last one") {
        return Some(FollowUpEntityReference::Last);
    }
    for (needle, ordinal) in [
        ("first", 1),
        ("1st", 1),
        ("second", 2),
        ("2nd", 2),
        ("third", 3),
        ("3rd", 3),
        ("fourth", 4),
        ("4th", 4),
        ("fifth", 5),
        ("5th", 5),
    ] {
        if lower.contains(needle) {
            return Some(FollowUpEntityReference::Ordinal(ordinal));
        }
    }
    None
}

fn resolve_follow_up_entity<'a>(
    context: &'a AssistantFollowUpContext,
    reference: &FollowUpEntityReference,
) -> Option<&'a AssistantFollowUpEntity> {
    match reference {
        FollowUpEntityReference::Ordinal(ordinal) => context
            .entities
            .iter()
            .find(|entity| entity.ordinal == *ordinal),
        FollowUpEntityReference::Last => context.entities.last(),
        FollowUpEntityReference::Demonstrative => {
            if context.entities.len() == 1 {
                context.entities.first()
            } else {
                None
            }
        }
    }
}

fn message_has_calendar_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    calendar_query_has_explicit_window(&lower)
        || is_calendar_next_free_day_query(&lower)
        || is_calendar_overlapping_events_query(&lower)
        || has_any(
            &lower,
            &[
                "what about",
                "how about",
                "and then",
                "and what about",
                "anything else",
                "what else",
            ],
        )
}

fn is_single_event_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_calendar_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its description",
            "it description",
            "the description",
            "what is its description",
            "what's its description",
            "whats its description",
            "describe it",
            "tell me more about it",
            "more about it",
        ],
    )
}

fn is_single_download_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_download_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "what is its package",
            "what's its package",
            "what does it include",
            "what is its checksum",
            "what's its checksum",
            "what is its install steps",
            "what's its install steps",
            "what is its compatibility",
            "what's its compatibility",
            "what is its source",
            "what's its source",
            "what is its release notes",
            "what's its release notes",
            "what platform is it for",
            "what architecture is it for",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn message_has_download_checksum_hint(message: &str) -> bool {
    has_any(
        &message.to_ascii_lowercase(),
        &[
            "checksum",
            "sha256",
            "sha-256",
            "sha1",
            "hash",
            "verify",
            "verification",
        ],
    )
}

fn message_has_download_install_steps_hint(message: &str) -> bool {
    has_any(
        &message.to_ascii_lowercase(),
        &[
            "install steps",
            "installation",
            "install it",
            "how do i install",
            "how to install",
            "setup",
            "setup steps",
        ],
    )
}

fn message_has_download_compatibility_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "compatible",
            "compatibility",
            "platform",
            "architecture",
            "supported on",
            "works on",
        ],
    ) || has_any_token(
        &lower,
        &[
            "linux", "windows", "mac", "macos", "arm64", "aarch64", "amd64", "x86_64",
        ],
    )
}

fn message_has_download_source_hint(message: &str) -> bool {
    has_any(
        &message.to_ascii_lowercase(),
        &[
            "source",
            "source url",
            "download source",
            "package path",
            "where from",
            "origin",
            "url",
        ],
    )
}

fn message_has_download_release_notes_hint(message: &str) -> bool {
    has_any(
        &message.to_ascii_lowercase(),
        &[
            "release notes",
            "release note",
            "notes",
            "changelog",
            "changes",
            "what changed",
            "update notes",
        ],
    )
}

fn message_has_memory_person_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "tell me more about it",
            "more about it",
            "describe it",
            "what about it",
            "what about them",
            "what about him",
            "what about her",
            "who is that",
            "who is this",
            "who are they",
            "their profile",
            "its profile",
            "their summary",
            "its summary",
            "profile details",
            "person details",
        ],
    )
}

fn is_single_library_detail_follow_up(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    if recent_single_library_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "its details",
            "it details",
            "the details",
            "what are its details",
            "what is its details",
            "what's its details",
            "whats its details",
            "what is its summary",
            "what's its summary",
            "what is its path",
            "what are its paths",
            "what are its settings",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn is_single_library_source_paths_follow_up(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> bool {
    if recent_single_library_item_entity_label(history).is_none() {
        return false;
    }

    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "source path",
            "source paths",
            "its paths",
            "it paths",
            "the paths",
            "what are its paths",
            "what is its paths",
            "what's its paths",
            "whats its paths",
            "what is its file path",
            "what are its file paths",
            "where is it stored",
            "where are its files",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    )
}

fn is_next_calendar_event_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "next event",
            "next calendar event",
            "next upcoming event",
            "next thing on my calendar",
            "next thing coming up",
            "coming up next in my calendar",
            "coming up next on my calendar",
            "what is my next event",
            "what's my next event",
            "whats my next event",
            "what is the next event",
            "what's the next event",
            "whats the next event",
            "what is the next thing coming up in my calendar",
            "what's the next thing coming up in my calendar",
            "whats the next thing coming up in my calendar",
        ],
    ) && !has_any(message_lower, &["birthday", "birthdays"])
}

fn is_next_calendar_event_timing_query(message_lower: &str) -> bool {
    is_next_calendar_event_query(message_lower)
        && has_any(
            message_lower,
            &[
                "how long",
                "time until",
                "days until",
                "how far until",
                "when is it",
                "how many days",
            ],
        )
}

fn is_calendar_conflict_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "conflict",
            "conflicts",
            "clash",
            "clashes",
            "double booked",
            "double-booked",
            "overlap",
            "overlapping",
            "busy on",
            "same day",
            "same date",
        ],
    )
}

fn is_calendar_free_days_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "free day",
            "free days",
            "open day",
            "open days",
            "available day",
            "available days",
            "when am i free",
            "what days am i free",
            "what days are free",
            "which days are free",
            "what dates are free",
            "which dates are free",
            "when are we free",
            "calendar gaps",
            "open slots",
        ],
    )
}

fn is_calendar_next_free_day_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "next free day",
            "next open day",
            "first free day",
            "next available day",
            "next available date",
            "when am i next free",
            "when are we next free",
            "when is the next free day",
            "what is the next free day",
            "what's the next free day",
            "whats the next free day",
        ],
    )
}

fn is_calendar_overlapping_events_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "overlapping events",
            "overlapping event",
            "calendar overlaps",
            "calendar overlap",
            "overlap events",
            "overlap calendar",
        ],
    )
}

fn is_calendar_busy_days_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "busy day",
            "busy days",
            "busiest day",
            "busiest days",
            "busiest",
            "how busy",
            "what days are busy",
            "which days are busy",
            "when am i busy",
            "when are we busy",
            "days with the most events",
            "most booked",
            "most packed",
        ],
    )
}

fn is_calendar_event_count_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "how many events",
            "event count",
            "count my events",
            "count events",
            "how full is my calendar",
            "how full is the calendar",
            "how busy is my calendar",
            "how busy is the calendar",
            "how packed is my calendar",
            "how packed is the calendar",
        ],
    )
}

fn message_has_channel_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    is_channel_activity_query(&lower)
        || extract_channel_query(message).is_some()
        || has_any(&lower, &["what about", "how about", "and in", "and on"])
}

fn message_has_transcript_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_transcript_channel_query(message).is_some()
        || has_any(
            &lower,
            &[
                "transcript",
                "transcription",
                "call summary",
                "summarize the call",
                "summarise the call",
                "what was it about",
                "what did they talk about",
                "what was discussed",
            ],
        )
}

fn message_has_room_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    detect_room_mode(message).is_some() || has_any(&lower, &["room", "rooms"])
}

fn message_has_server_follow_up_hint(message: &str) -> bool {
    extract_server_query(message).is_some() || extract_server_availability(message).is_some()
}

fn message_has_ai_runtime_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "ai model",
            "loaded model",
            "model loaded",
            "inference model",
            "backend",
            "warm pool",
            "scheduler",
            "queue depth",
            "queued",
            "overload",
            "planner",
            "summarizer",
            "answer model",
            "verifier",
            "worker",
            "role routing",
            "runtime",
        ],
    )
}

fn message_has_host_runtime_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "ram",
            "memory",
            "swap",
            "cpu",
            "load",
            "uptime",
            "thread",
            "threads",
            "resource",
            "resources",
            "host",
            "system",
            "runtime",
            "gigabyte",
            "gigabytes",
            "gib",
            "gb",
            "megabyte",
            "megabytes",
            "mib",
            "mb",
            "byte",
            "bytes",
        ],
    )
}

fn message_has_network_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "network",
            "topology",
            "interface",
            "interfaces",
            "ip",
            "ip address",
            "ip addresses",
            "address",
            "addresses",
            "hostname",
            "hostname aliases",
            "host aliases",
            "dns",
            "dns servers",
            "nameserver",
            "nameservers",
            "resolver",
            "resolvers",
            "resolv.conf",
            "default route",
            "default gateway",
            "gateway",
            "remote access",
            "trusted proxy",
            "trusted proxies",
            "proxy",
            "proxies",
            "lan",
            "/etc/hosts",
            "what about it",
            "tell me more about it",
            "more about it",
            "describe it",
        ],
    ) || extract_network_interface_query(message).is_some()
        || extract_network_interface_ip_query(message).is_some()
        || extract_network_default_route_query(message).is_some()
        || extract_network_hostname_aliases_query(message).is_some()
        || extract_network_dns_servers_query(message).is_some()
}

fn message_has_service_health_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "service", "services", "agent", "agents", "healthy", "health", "status", "up", "down",
            "runtime",
        ],
    ) || extract_service_detail_query(message).is_some()
        || message_has_failed_units_follow_up_hint(message)
        || message_has_port_conflicts_follow_up_hint(message)
}

fn message_has_port_conflicts_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "port conflict",
            "port conflicts",
            "conflicting port",
            "conflicting ports",
            "port in use",
            "ports in use",
            "listening socket",
            "listening sockets",
            "listener",
            "listeners",
            "socket",
            "sockets",
            "bound port",
            "bound ports",
            "who is using it",
            "what process is using it",
            "what is listening on it",
            "what is using it",
            "what about it",
            "tell me more about it",
            "more about it",
        ],
    )
}

fn message_has_failed_units_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "failed unit",
            "failed units",
            "failed service",
            "failed services",
            "failed systemd",
            "systemd failed",
            "systemd units failed",
            "systemd units are failed",
            "what about it",
            "tell me more about it",
            "more about it",
        ],
    ) || extract_failed_unit_detail_query(message).is_some()
}

fn extract_network_default_route_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &["default route", "default gateway", "gateway", "route"],
    ) {
        return None;
    }

    extract_quoted_phrase(message).or_else(|| {
        for marker in [
            "for the default route to ",
            "for default route to ",
            "for the default route on ",
            "for default route on ",
            "for the gateway to ",
            "for gateway to ",
            "for the gateway on ",
            "for gateway on ",
            "for the route to ",
            "for route to ",
            "for the route on ",
            "for route on ",
        ] {
            if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
                let candidate = candidate
                    .trim()
                    .trim_matches(|ch: char| {
                        ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch)
                    })
                    .to_string();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn extract_network_hostname_aliases_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "hostname aliases",
            "host aliases",
            "hostnames",
            "/etc/hosts",
        ],
    ) {
        return None;
    }

    extract_quoted_phrase(message).or_else(|| {
        for marker in [
            "for the hostname aliases of ",
            "for hostname aliases of ",
            "for the host aliases of ",
            "for host aliases of ",
            "for the hostnames of ",
            "for hostnames of ",
        ] {
            if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
                let candidate = candidate
                    .trim()
                    .trim_matches(|ch: char| {
                        ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch)
                    })
                    .to_string();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn extract_network_dns_servers_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "dns",
            "dns servers",
            "nameserver",
            "nameservers",
            "resolver",
            "resolvers",
        ],
    ) {
        return None;
    }

    extract_quoted_phrase(message).or_else(|| {
        for marker in [
            "for the dns servers on ",
            "for dns servers on ",
            "for the dns servers of ",
            "for dns servers of ",
            "for the nameservers on ",
            "for nameservers on ",
            "for the resolvers on ",
            "for resolvers on ",
            "on interface ",
            "for interface ",
            "on the interface ",
            "for the interface ",
            "on ",
            "for ",
        ] {
            if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
                let candidate = candidate
                    .trim()
                    .trim_matches(|ch: char| {
                        ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch)
                    })
                    .to_string();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn extract_port_conflicts_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "port conflict",
            "port conflicts",
            "port in use",
            "ports in use",
            "listening socket",
            "listening sockets",
            "bound port",
            "bound ports",
            "what port",
            "which port",
            "port ",
            "ports ",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "port ",
        "port:",
        "ports ",
        "ports:",
        "using port ",
        "held on port ",
        "held by port ",
        "listening on port ",
        "conflict on port ",
        "conflicts on port ",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    None
}

fn extract_port_conflict_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "who is using port",
            "what is using port",
            "what's using port",
            "what process is using port",
            "which process is using port",
            "what is listening on port",
            "what's listening on port",
            "what is on port",
            "what's on port",
            "who is listening on port",
            "what listens on port",
            "which service is using port",
            "which listener is using port",
            "port listener",
            "port detail",
            "listener on port",
            "socket on port",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        let quoted = quoted
            .trim()
            .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
            .to_string();
        if !quoted.is_empty() {
            return Some(quoted);
        }
    }

    if let Some(port) = extract_port_number_candidate(message) {
        return Some(port);
    }

    for marker in [
        "using port ",
        "listening on port ",
        "on port ",
        "port ",
        "port:",
        "socket ",
        "socket:",
        "listener ",
        "listener:",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                if candidate.chars().any(|ch| ch.is_ascii_digit()) {
                    return Some(candidate);
                }
                if has_any(
                    &lower,
                    &["socket", "listener", "listening", "using", "process"],
                ) {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn extract_process_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "process detail",
            "process details",
            "process info",
            "process information",
            "which process",
            "what process",
            "find process",
            "lookup process",
            "pid ",
            "ps ",
            "command line",
            "command-line",
            "cmdline",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "pid ",
        "pid:",
        "process ",
        "process:",
        "process named ",
        "process called ",
        "command line ",
        "command-line ",
        "cmdline ",
        "cmd ",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    None
}

fn extract_listener_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "listener detail",
            "listener details",
            "listener info",
            "listener information",
            "socket detail",
            "socket details",
            "which listener",
            "what listener",
            "which socket",
            "what socket",
            "who is listening",
            "what is listening on",
            "what's listening on",
            "listening on port",
            "port listener",
            "listener on port",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "port ",
        "port:",
        "socket ",
        "socket:",
        "listener ",
        "listener:",
        "listening on port ",
        "listening on ",
        "listening at ",
        "bound to ",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    None
}

fn extract_disk_usage_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "disk usage detail",
            "disk usage",
            "disk space",
            "storage usage",
            "filesystem usage",
            "how much space",
            "how much free space",
            "how full is",
            "how full are",
            "used space",
            "free space",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        let quoted_lower = quoted.to_ascii_lowercase();
        if !matches!(
            quoted_lower.as_str(),
            "disk" | "disks" | "storage" | "filesystem" | "file system" | "space"
        ) {
            return Some(quoted);
        }
    }

    for marker in [
        "on ",
        "for ",
        "path ",
        "path:",
        "mount ",
        "mount:",
        "filesystem ",
        "filesystem:",
        "directory ",
        "directory:",
        "folder ",
        "folder:",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            let candidate_lower = candidate.to_ascii_lowercase();
            if !candidate.is_empty()
                && !matches!(
                    candidate_lower.as_str(),
                    "disk" | "disks" | "storage" | "filesystem" | "file system" | "space"
                )
            {
                return Some(candidate);
            }
        }
    }

    None
}

fn extract_port_number_candidate(message: &str) -> Option<String> {
    for token in message.split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '(' | ')'))
    {
        let token =
            token.trim_matches(|ch: char| ['"', '\'', '.', '?', '!', ':', ']'].contains(&ch));
        if token.is_empty() {
            continue;
        }

        let port_candidate = token.rsplit(':').next().unwrap_or(token);
        let port_candidate = port_candidate
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim();
        if port_candidate.is_empty() || port_candidate.len() > 5 {
            continue;
        }
        if !port_candidate.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if port_candidate.parse::<u16>().is_ok() {
            return Some(port_candidate.to_string());
        }
    }
    None
}

fn extract_failed_units_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "failed unit",
            "failed units",
            "failed service",
            "failed services",
            "systemd units failed",
            "systemd units are failed",
            "which systemd units are failed",
            "what systemd units are failed",
        ],
    ) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "failed unit ",
        "failed units ",
        "failed service ",
        "failed services ",
        "systemd failed unit ",
        "systemd failed services ",
        "systemctl failed unit ",
        "systemctl failed units ",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    None
}

fn is_failed_unit_detail_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "unit status",
            "service status",
            "status of",
            "logs for",
            "log for",
            "why is",
            "why did",
            "what happened to",
            "inspect",
            "detail for",
            "details for",
        ],
    )
}

fn extract_failed_unit_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !is_failed_unit_detail_query(&lower) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "details for the ",
        "details for ",
        "detail for the ",
        "detail for ",
        "status of the ",
        "status of ",
        "logs for the ",
        "logs for ",
        "log for the ",
        "log for ",
        "why is the ",
        "why is ",
        "why did the ",
        "why did ",
        "what happened to the ",
        "what happened to ",
        "inspect the ",
        "inspect ",
        "show me the ",
        "show me ",
    ] {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            let candidate = candidate
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
                .to_string();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    extract_failed_units_query(message)
}

fn extract_network_interface_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let network_context = has_any(
        &lower,
        &[
            "network",
            "interface",
            "interfaces",
            "ip",
            "ip address",
            "address",
            "hostname",
            "host name",
            "lan",
            "local network",
            "rustyfin",
        ],
    );
    if !network_context {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        let quoted = normalize_download_artifact_query_candidate(&quoted);
        if !quoted.is_empty() {
            return Some(quoted);
        }
    }

    for marker in [
        "details for the ",
        "details for ",
        "summary of the ",
        "summary of ",
        "interface ",
        "network interface ",
        "ip of the ",
        "ip of ",
        "ip for the ",
        "ip for ",
        "address of the ",
        "address of ",
        "address for the ",
        "address for ",
        "hostname of the ",
        "hostname of ",
        "host name of the ",
        "host name of ",
        "host of the ",
        "host of ",
        "for the interface ",
        "for interface ",
        "what about ",
        "how about ",
        "and ",
        "what else about ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker)
            .and_then(|candidate| normalize_network_interface_query_candidate(&candidate))
        else {
            continue;
        };
        return Some(candidate);
    }

    None
}

fn extract_network_interface_ip_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let network_context = has_any(
        &lower,
        &[
            "network",
            "interface",
            "interfaces",
            "ip",
            "ip address",
            "address",
            "hostname",
            "host name",
            "lan",
            "local network",
            "rustyfin",
        ],
    );
    if !network_context {
        return None;
    }

    for token in message.split_whitespace() {
        let candidate = token.trim_matches(|ch: char| {
            ['"', '\'', '(', ')', '[', ']', ',', '.', '?', '!', ';', ':'].contains(&ch)
        });
        if candidate.is_empty() {
            continue;
        }
        if candidate.parse::<std::net::IpAddr>().is_ok() {
            return Some(candidate.to_string());
        }
    }

    None
}

fn normalize_network_interface_query_candidate(candidate: &str) -> Option<String> {
    let mut normalized = candidate
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .to_string();
    if normalized.is_empty() {
        return None;
    }

    for prefix in [
        "the ",
        "network ",
        "interface ",
        "ip address ",
        "ip ",
        "address ",
        "hostname ",
        "host name ",
        "host ",
        "lan ",
        "local ",
    ] {
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim().to_string();
        }
    }

    loop {
        let lower = normalized.to_ascii_lowercase();
        let mut stripped = false;
        for suffix in [
            " interface",
            " network",
            " ip address",
            " ip",
            " address",
            " hostname",
            " host name",
            " host",
            " lan",
            " local network",
        ] {
            if lower.ends_with(suffix) {
                let keep_len = normalized.len().saturating_sub(suffix.len());
                normalized.truncate(keep_len);
                normalized = normalized.trim().to_string();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    let normalized_lower = normalized.to_ascii_lowercase();
    if normalized.is_empty()
        || matches!(
            normalized_lower.as_str(),
            "network"
                | "networks"
                | "interface"
                | "interfaces"
                | "network interface"
                | "network interfaces"
                | "ip"
                | "ips"
                | "ip address"
                | "ip addresses"
                | "address"
                | "addresses"
                | "hostname"
                | "hostnames"
                | "host alias"
                | "host aliases"
                | "hostname alias"
                | "hostname aliases"
                | "dns"
                | "dns server"
                | "dns servers"
                | "nameserver"
                | "nameservers"
                | "resolver"
                | "resolvers"
                | "default route"
                | "default gateway"
                | "gateway"
        )
    {
        None
    } else {
        Some(normalized)
    }
}

fn extract_service_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let service_context = has_any(
        &lower,
        &[
            "service",
            "services",
            "agent",
            "agents",
            "healthy",
            "health",
            "status",
            "core api",
            "backend api",
            "backend service",
            "api backend",
        ],
    );
    if !service_context {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        let quoted = normalize_download_artifact_query_candidate(&quoted);
        if !quoted.is_empty() {
            return Some(quoted);
        }
    }

    for marker in [
        "service health for the ",
        "service health for ",
        "service health of the ",
        "service health of ",
        "service status for the ",
        "service status for ",
        "service status of the ",
        "service status of ",
        "health of the ",
        "health of ",
        "status of the ",
        "status of ",
        "is the ",
        "is my ",
        "is ",
        "how is the ",
        "how is ",
        "what about the ",
        "what about ",
        "how about the ",
        "how about ",
        "and ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker)
            .and_then(|candidate| normalize_service_detail_query_candidate(&candidate))
        else {
            continue;
        };
        if marker == "and " {
            let candidate_lower = candidate.to_ascii_lowercase();
            if has_any(
                &candidate_lower,
                &[
                    "what ", "who ", "where ", "when ", "why ", "how ", "is ", "are ", "do ",
                    "does ", "did ", "can ", "could ", "should ", "would ", "will ",
                ],
            ) {
                continue;
            }
        }
        return Some(candidate);
    }

    None
}

fn normalize_service_detail_query_candidate(candidate: &str) -> Option<String> {
    let mut normalized = candidate
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }

    for prefix in ["the ", "my ", "service ", "agent ", "component "] {
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim().to_string();
        }
    }

    loop {
        let lower = normalized.to_ascii_lowercase();
        let mut stripped = false;
        for suffix in [
            " healthy",
            " health",
            " status",
            " service",
            " agent",
            " component",
            " service health",
            " service status",
            " up",
            " down",
        ] {
            if lower.ends_with(suffix) {
                let keep_len = normalized.len().saturating_sub(suffix.len());
                normalized.truncate(keep_len);
                normalized = normalized.trim().to_string();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn extract_storage_path_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if let Some(path) = extract_posix_path_candidate(message) {
        return Some(path);
    }

    for (needle, canonical) in [
        ("ai model dir", "ai_model_dir"),
        ("ai model directory", "ai_model_dir"),
        ("model dir", "ai_model_dir"),
        ("model directory", "ai_model_dir"),
        ("model folder", "ai_model_dir"),
        ("cache dir", "cache_dir"),
        ("cache directory", "cache_dir"),
        ("watch party audio dir", "watch_party_audio_dir"),
        ("watch party audio directory", "watch_party_audio_dir"),
        ("media root", "media_root"),
        ("media path", "media_root"),
        ("storage path", "storage"),
    ] {
        if lower.contains(needle) {
            return Some(canonical.to_string());
        }
    }

    None
}

fn extract_mount_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "mount point",
            "mounted on",
            "mounted at",
            "filesystem",
            "file system",
            "volume",
            "partition",
            "mount details",
            "mount detail",
        ],
    ) {
        return None;
    }

    if let Some(path) = extract_posix_path_candidate(message) {
        return Some(path);
    }

    for (needle, canonical) in [
        ("ai model dir", "ai_model_dir"),
        ("ai model directory", "ai_model_dir"),
        ("model dir", "ai_model_dir"),
        ("model directory", "ai_model_dir"),
        ("model folder", "ai_model_dir"),
        ("cache dir", "cache_dir"),
        ("cache directory", "cache_dir"),
        ("watch party audio dir", "watch_party_audio_dir"),
        ("watch party audio directory", "watch_party_audio_dir"),
        ("media root", "media_root"),
        ("media path", "media_root"),
        ("mount point", "mount"),
        ("mounted on", "mount"),
        ("mounted at", "mount"),
        ("filesystem", "mount"),
        ("file system", "mount"),
        ("volume", "mount"),
        ("partition", "mount"),
    ] {
        if lower.contains(needle) {
            return Some(canonical.to_string());
        }
    }

    None
}

fn extract_posix_path_candidate(message: &str) -> Option<String> {
    for token in message.split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '(' | ')'))
    {
        let token =
            token.trim_matches(|ch: char| ['"', '\'', '.', '?', '!', ':', ']'].contains(&ch));
        if token.is_empty() || token.contains("://") {
            continue;
        }
        if token.starts_with('/') || token.starts_with('~') || token.contains('/') {
            let candidate = token.trim_end_matches(|ch: char| {
                ['"', '\'', '.', '?', '!', ':', ',', ';'].contains(&ch)
            });
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

fn message_has_library_listing_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    has_any(
        &lower,
        &[
            "what libraries",
            "which libraries",
            "what about music",
            "what about movies",
            "what about shows",
            "library",
            "libraries",
        ],
    )
}

fn is_channel_activity_query(message_lower: &str) -> bool {
    let channel_scope = has_any(
        message_lower,
        &[
            "channel",
            "channels",
            "chat",
            "messages",
            "message activity",
            "general chat",
        ],
    );
    let activity_hint = has_any(
        message_lower,
        &[
            "unread",
            "recent",
            "latest",
            "activity",
            "new",
            "happening",
            "going on",
        ],
    );
    channel_scope && activity_hint
}

fn is_transcript_summary_query(message_lower: &str) -> bool {
    let transcript_scope = has_any(
        message_lower,
        &[
            "transcript",
            "transcription",
            "voice call",
            "call",
            "voice chat",
            "call summary",
        ],
    );
    let summary_hint = has_any(
        message_lower,
        &[
            "summarize",
            "summarise",
            "summary",
            "what was",
            "what were",
            "what did",
            "what was it about",
            "what was the call about",
            "what did they talk about",
            "what was discussed",
            "recap",
        ],
    );

    transcript_scope && summary_hint
}

fn message_has_downloads_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_download_artifact_detail_query(message).is_some()
        || extract_download_artifact_checksum_query(message).is_some()
        || extract_download_artifact_install_steps_query(message).is_some()
        || extract_download_artifact_compatibility_query(message).is_some()
        || extract_downloads_query(message).is_some()
        || extract_downloads_follow_up_query(message).is_some()
        || extract_downloads_availability(message).is_some()
        || has_any(
            &lower,
            &[
                "download",
                "downloads",
                "artifact",
                "artifacts",
                "extension",
                "app",
                "planned",
                "available",
                "unavailable",
                "companion",
                "source",
                "release notes",
                "changelog",
            ],
        )
}

fn extract_download_artifact_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let detail_hint = has_any(
        &lower,
        &[
            "details",
            "detail",
            "summary",
            "tell me more",
            "tell me about",
            "more about",
            "what is",
            "what's",
            "what does",
            "what comes with",
            "what is included",
            "included",
            "install",
            "package",
        ],
    );
    let download_context = has_any(
        &lower,
        &[
            "download",
            "downloads",
            "artifact",
            "artifacts",
            "extension",
            "app",
            "companion",
            "browser extension",
        ],
    );
    if !detail_hint && !download_context {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        if detail_hint || download_context {
            return Some(quoted);
        }
    }

    for marker in [
        "tell me about the ",
        "tell me about ",
        "tell me more about the ",
        "tell me more about ",
        "details for the ",
        "details for ",
        "summary of the ",
        "summary of ",
        "what is the ",
        "what is this ",
        "what does the ",
        "what does this ",
        "what comes with the ",
        "what comes with ",
        "package for the ",
        "package for ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [
            " package",
            " download",
            " downloads",
            " artifact",
            " artifacts",
            " extension",
            " app",
            " companion",
        ] {
            let candidate_lower = candidate.to_ascii_lowercase();
            if suffix == " extension" && candidate_lower.contains("browser extension") {
                continue;
            }
            if candidate_lower.ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn normalize_download_artifact_query_candidate(candidate: &str) -> String {
    let mut normalized = candidate.trim().trim_matches(['"', '\'', '`']).to_string();
    for suffix in [
        " package",
        " download",
        " downloads",
        " artifact",
        " artifacts",
        " extension",
        " app",
        " companion",
        " install steps",
        " installation",
        " setup",
        " compatibility",
    ] {
        let normalized_lower = normalized.to_ascii_lowercase();
        if suffix == " extension" && normalized_lower.contains("browser extension") {
            continue;
        }
        if normalized_lower.ends_with(suffix) {
            let keep_len = normalized.len().saturating_sub(suffix.len());
            normalized.truncate(keep_len);
            normalized = normalized.trim().to_string();
        }
    }
    normalized
}

fn extract_download_artifact_checksum_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "checksum",
            "sha256",
            "sha-256",
            "sha1",
            "hash",
            "verify",
            "verification",
        ],
    ) {
        return None;
    }
    if let Some(quoted) = extract_quoted_phrase(message) {
        let quoted = normalize_download_artifact_query_candidate(&quoted);
        if !quoted.is_empty() {
            return Some(quoted);
        }
    }
    for marker in [
        "checksum for the ",
        "checksum for ",
        "sha256 for the ",
        "sha256 for ",
        "sha-256 for the ",
        "sha-256 for ",
        "hash for the ",
        "hash for ",
        "verify the ",
        "verify ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = normalize_download_artifact_query_candidate(&rest[..end]);
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    extract_download_artifact_detail_query(message)
        .or_else(|| extract_downloads_follow_up_query(message))
        .or_else(|| extract_downloads_query(message))
}

fn extract_download_artifact_install_steps_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "install steps",
            "installation",
            "install it",
            "how do i install",
            "how to install",
            "setup",
            "setup steps",
        ],
    ) {
        return None;
    }
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }
    for marker in [
        "install steps for the ",
        "install steps for ",
        "installation steps for the ",
        "installation steps for ",
        "how do i install the ",
        "how do i install ",
        "how to install the ",
        "how to install ",
        "setup for the ",
        "setup for ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = normalize_download_artifact_query_candidate(&rest[..end]);
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    extract_download_artifact_detail_query(message)
        .or_else(|| extract_downloads_follow_up_query(message))
        .or_else(|| extract_downloads_query(message))
}

fn extract_download_artifact_compatibility_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !has_any(
        &lower,
        &[
            "compatible",
            "compatibility",
            "platform",
            "architecture",
            "supported on",
            "works on",
        ],
    ) && !has_any_token(
        &lower,
        &[
            "linux", "windows", "mac", "macos", "arm64", "aarch64", "amd64", "x86_64",
        ],
    ) {
        return None;
    }
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }
    if let Some(idx) = lower.find(" compatible with ") {
        let candidate = strip_download_question_prefix(message[..idx].trim());
        let candidate = normalize_download_artifact_query_candidate(candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    for marker in [
        "compatible with the ",
        "compatible with ",
        "compatibility for the ",
        "compatibility for ",
        "compatibility of the ",
        "compatibility of ",
        "compatible for the ",
        "compatible for ",
        "works on the ",
        "works on ",
        "platform for the ",
        "platform for ",
        "architecture for the ",
        "architecture for ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = normalize_download_artifact_query_candidate(&rest[..end]);
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    extract_download_artifact_detail_query(message)
        .or_else(|| extract_downloads_follow_up_query(message))
        .or_else(|| extract_downloads_query(message))
}

fn extract_download_artifact_source_query(message: &str) -> Option<String> {
    if !message_has_download_source_hint(message) {
        return None;
    }

    extract_download_artifact_detail_query(message)
        .or_else(|| extract_downloads_follow_up_query(message))
        .or_else(|| extract_downloads_query(message))
}

fn extract_download_artifact_release_notes_query(message: &str) -> Option<String> {
    if !message_has_download_release_notes_hint(message) {
        return None;
    }

    extract_download_artifact_detail_query(message)
        .or_else(|| extract_downloads_follow_up_query(message))
        .or_else(|| extract_downloads_query(message))
}

fn strip_download_question_prefix(candidate: &str) -> &str {
    let trimmed = candidate.trim();
    let lower = trimmed.to_ascii_lowercase();
    for prefix in [
        "is the ",
        "is a ",
        "is an ",
        "is ",
        "are the ",
        "are a ",
        "are an ",
        "are ",
        "what is the ",
        "what is a ",
        "what is an ",
        "what is ",
        "what's the ",
        "what's a ",
        "what's an ",
        "what's ",
        "tell me about the ",
        "tell me about ",
        "show me the ",
        "show me ",
    ] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim();
        }
    }
    trimmed
}

fn extract_downloads_filter(message: &str) -> AssistantToolInput {
    AssistantToolInput::DownloadsFilter {
        query: extract_downloads_query(message),
        availability: extract_downloads_availability(message),
    }
}

fn extract_downloads_follow_up_filter(message: &str) -> AssistantToolInput {
    AssistantToolInput::DownloadsFilter {
        query: extract_downloads_follow_up_query(message)
            .or_else(|| extract_downloads_query(message)),
        availability: extract_downloads_availability(message),
    }
}

fn extract_downloads_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();

    if has_any(&lower, &["browser extension", "extension"]) {
        return Some("extension".to_string());
    }
    if lower.contains("rustyvault") {
        return Some("rustyvault".to_string());
    }
    if lower.contains("client app") || (lower.contains("app") && lower.contains("download")) {
        return Some("app".to_string());
    }
    if lower.contains("companion") {
        return Some("companion".to_string());
    }

    None
}

fn extract_downloads_follow_up_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    let lower = message.to_ascii_lowercase();
    for needle in ["what about ", "how about ", "and ", "what else about "] {
        let Some(idx) = lower.find(needle) else {
            continue;
        };
        let rest = message[idx + needle.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() {
            continue;
        }
        if has_any(
            &candidate.to_ascii_lowercase(),
            &["planned", "available", "unavailable", "now", "soon"],
        ) {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn extract_downloads_availability(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if has_any(&lower, &["planned", "coming soon", "future"]) {
        Some("planned".to_string())
    } else if has_any(&lower, &["unavailable", "not available"]) {
        Some("unavailable".to_string())
    } else if has_any(&lower, &["available", "available now", "download now"]) {
        Some("available".to_string())
    } else {
        None
    }
}

fn extract_calendar_event_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let calendar_context = has_any(
        &lower,
        &[
            "calendar",
            "event",
            "events",
            "schedule",
            "appointment",
            "meeting",
            "birthday",
        ],
    );
    if has_any(
        &lower,
        &[
            "failed unit",
            "failed units",
            "failed service",
            "failed services",
            "systemd failed",
            "systemctl failed",
            "systemd unit",
            "systemd service",
        ],
    ) {
        return None;
    }
    if !calendar_context {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "event called ",
        "event named ",
        "details for ",
        "description for ",
        "tell me about ",
        "tell me more about ",
        "more about ",
        "what time is ",
        "when is ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn extract_channel_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    let lower = message.to_ascii_lowercase();
    for marker in ["channel called ", "channel named ", "in ", "on "] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [" about", " again", " please"] {
            if candidate.to_ascii_lowercase().ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if candidate.is_empty() {
            continue;
        }
        let candidate_lower = candidate.to_ascii_lowercase();
        if matches!(
            candidate_lower.as_str(),
            "channels" | "channel" | "chat" | "messages" | "latest" | "recent" | "unread"
        ) {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn extract_transcript_channel_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    let lower = message.to_ascii_lowercase();
    for marker in [
        "call in ",
        "call on ",
        "transcript for ",
        "transcript in ",
        "transcript on ",
        "transcription for ",
        "transcription in ",
        "voice channel ",
        "channel called ",
        "channel named ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [" about", " again", " please"] {
            if candidate.to_ascii_lowercase().ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if candidate.is_empty() {
            continue;
        }
        let candidate_lower = candidate.to_ascii_lowercase();
        if matches!(
            candidate_lower.as_str(),
            "call" | "voice" | "voice call" | "transcript" | "transcription"
        ) {
            continue;
        }
        return Some(candidate);
    }

    extract_channel_query(message)
}

#[derive(Debug, Clone)]
struct RecentWeatherHint {
    tool: AssistantToolName,
    location: String,
    forecast_days: Option<u8>,
    start_date: Option<String>,
    end_date: Option<String>,
    label: Option<String>,
}

fn weather_tool_input_for_location(message: &str, location: String) -> Option<AssistantToolInput> {
    weather_tool_call_for_location(message, location).map(|(_, input)| input)
}

fn weather_tool_call_for_location(
    message: &str,
    location: String,
) -> Option<(AssistantToolName, AssistantToolInput)> {
    let lower = message.to_ascii_lowercase();
    let today = assistant_local_today();

    if let Some((start_date, end_date, label)) = extract_weather_history_window(message, today) {
        return Some((
            AssistantToolName::WeatherGetHistory,
            AssistantToolInput::WeatherHistory {
                location,
                start_date: start_date.format("%F").to_string(),
                end_date: end_date.format("%F").to_string(),
                label,
            },
        ));
    }

    if is_weather_timezone_query(&lower) {
        return Some((
            AssistantToolName::WeatherResolveLocationAlias,
            AssistantToolInput::Weather {
                location,
                forecast_days: None,
            },
        ));
    }

    if weather_prefers_hourly(&lower) {
        return weather_hourly_tool_call_for_location(message, location, false);
    }

    if let Some((date, label)) = extract_single_calendar_date(message, today) {
        let matched_text = label.to_ascii_lowercase();
        let is_relative_day = matches!(
            matched_text.as_str(),
            "today" | "tomorrow" | "yesterday" | "day after tomorrow"
        );
        if date < today {
            if !is_relative_day {
                return Some((
                    AssistantToolName::WeatherGetRecentHistoryForDate,
                    AssistantToolInput::WeatherHistory {
                        location,
                        start_date: date.format("%F").to_string(),
                        end_date: date.format("%F").to_string(),
                        label,
                    },
                ));
            }
            return Some((
                AssistantToolName::WeatherGetHistory,
                AssistantToolInput::WeatherHistory {
                    location,
                    start_date: date.format("%F").to_string(),
                    end_date: date.format("%F").to_string(),
                    label,
                },
            ));
        }
        if date == today {
            return Some((
                AssistantToolName::WeatherGetCurrent,
                AssistantToolInput::Weather {
                    location,
                    forecast_days: None,
                },
            ));
        }
        if !is_relative_day {
            return Some((
                AssistantToolName::WeatherGetForecastForDate,
                AssistantToolInput::Weather {
                    location,
                    forecast_days: Some(((date - today).num_days() + 1).clamp(1, 7) as u8),
                },
            ));
        }
    }

    if weather_prefers_current(&lower) {
        return Some((
            AssistantToolName::WeatherGetCurrent,
            AssistantToolInput::Weather {
                location,
                forecast_days: None,
            },
        ));
    }

    if weather_prefers_forecast(&lower) {
        return Some((
            AssistantToolName::WeatherGetForecast,
            AssistantToolInput::Weather {
                location,
                forecast_days: Some(extract_weather_forecast_days(message)),
            },
        ));
    }

    Some((
        AssistantToolName::WeatherGetCurrent,
        AssistantToolInput::Weather {
            location,
            forecast_days: None,
        },
    ))
}

fn weather_hourly_tool_call_for_location(
    message: &str,
    location: String,
    force_hourly: bool,
) -> Option<(AssistantToolName, AssistantToolInput)> {
    let today = assistant_local_today();
    let (date, label) =
        extract_single_calendar_date(message, today).unwrap_or((today, "today".to_string()));

    if date < today {
        return Some((
            AssistantToolName::WeatherGetRecentHistoryForDate,
            AssistantToolInput::WeatherHistory {
                location,
                start_date: date.format("%F").to_string(),
                end_date: date.format("%F").to_string(),
                label,
            },
        ));
    }

    if !force_hourly && !weather_prefers_hourly(&message.to_ascii_lowercase()) {
        return None;
    }

    Some((
        AssistantToolName::WeatherGetHourlyWindow,
        AssistantToolInput::WeatherHistory {
            location,
            start_date: date.format("%F").to_string(),
            end_date: date.format("%F").to_string(),
            label,
        },
    ))
}

fn extract_weather_follow_up_call(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Option<(AssistantToolName, AssistantToolInput)> {
    let lower = message.to_ascii_lowercase();
    if is_current_datetime_query(&lower) || message_has_current_datetime_follow_up_hint(message) {
        return None;
    }
    let hint = recent_weather_hint(history)?;
    let explicit_location = extract_weather_location(message);
    let standalone_location = extract_standalone_weather_location(message);
    let has_hint = is_weather_query(&lower)
        || explicit_location.is_some()
        || standalone_location.is_some()
        || message_has_weather_follow_up_hint(&lower);
    if !has_hint {
        return None;
    }

    let location = explicit_location
        .or(standalone_location)
        .map(|location| merge_weather_follow_up_location(&location, &hint.location))
        .unwrap_or_else(|| hint.location.clone());
    if is_weather_timezone_query(&lower) {
        return Some((
            AssistantToolName::WeatherResolveLocationAlias,
            AssistantToolInput::Weather {
                location,
                forecast_days: None,
            },
        ));
    }
    if is_weather_query(&lower)
        || lower.contains("today")
        || lower.contains("tomorrow")
        || lower.contains("yesterday")
        || lower.contains("week")
        || lower.contains("weekend")
        || extract_single_calendar_date(message, assistant_local_today()).is_some()
    {
        if hint.tool == AssistantToolName::WeatherGetHourlyWindow
            && let Some(call) =
                weather_hourly_tool_call_for_location(message, location.clone(), true)
        {
            return Some(call);
        }
        return weather_tool_call_for_location(message, location);
    }

    match hint.tool {
        AssistantToolName::WeatherGetCurrent => Some((
            AssistantToolName::WeatherGetCurrent,
            AssistantToolInput::Weather {
                location,
                forecast_days: None,
            },
        )),
        AssistantToolName::WeatherGetForecast => Some((
            AssistantToolName::WeatherGetForecast,
            AssistantToolInput::Weather {
                location,
                forecast_days: hint.forecast_days.or(Some(3)),
            },
        )),
        AssistantToolName::WeatherGetForecastForDate => Some((
            AssistantToolName::WeatherGetForecastForDate,
            AssistantToolInput::Weather {
                location,
                forecast_days: hint.forecast_days.or(Some(3)),
            },
        )),
        AssistantToolName::WeatherGetHourlyWindow => Some((
            AssistantToolName::WeatherGetHourlyWindow,
            AssistantToolInput::WeatherHistory {
                location,
                start_date: hint
                    .start_date
                    .clone()
                    .unwrap_or_else(|| assistant_local_today().format("%F").to_string()),
                end_date: hint
                    .end_date
                    .clone()
                    .unwrap_or_else(|| assistant_local_today().format("%F").to_string()),
                label: hint.label.clone().unwrap_or_else(|| "today".to_string()),
            },
        )),
        AssistantToolName::WeatherGetHistory => Some((
            AssistantToolName::WeatherGetHistory,
            AssistantToolInput::WeatherHistory {
                location,
                start_date: hint.start_date?,
                end_date: hint.end_date?,
                label: hint
                    .label
                    .unwrap_or_else(|| "the same recent weather window".to_string()),
            },
        )),
        AssistantToolName::WeatherGetRecentHistoryForDate => Some((
            AssistantToolName::WeatherGetRecentHistoryForDate,
            AssistantToolInput::WeatherHistory {
                location,
                start_date: hint.start_date?,
                end_date: hint.end_date?,
                label: hint
                    .label
                    .unwrap_or_else(|| "the same recent weather window".to_string()),
            },
        )),
        AssistantToolName::WeatherResolveLocationAlias => Some((
            AssistantToolName::WeatherResolveLocationAlias,
            AssistantToolInput::Weather {
                location,
                forecast_days: None,
            },
        )),
        _ => None,
    }
}

fn merge_weather_follow_up_location(location: &str, hint_location: &str) -> String {
    let candidate = location.trim();
    if candidate.is_empty() {
        return hint_location.to_string();
    }
    if candidate.contains(',')
        || candidate.to_ascii_lowercase().contains(" in ")
        || candidate.eq_ignore_ascii_case(hint_location)
    {
        return candidate.to_string();
    }

    let hint_primary = hint_location
        .split(',')
        .next()
        .map(str::trim)
        .unwrap_or(hint_location);
    if candidate.eq_ignore_ascii_case(hint_primary) {
        return hint_location.to_string();
    }

    if candidate.split_whitespace().count() <= 3
        && let Some(country) = hint_location
            .split(',')
            .next_back()
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .filter(|segment| !segment.eq_ignore_ascii_case(candidate))
    {
        return format!("{candidate}, {country}");
    }

    candidate.to_string()
}

fn message_has_weather_follow_up_hint(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "yesterday",
            "today",
            "tomorrow",
            "weekend",
            "this week",
            "next week",
            "weather",
            "forecast",
            "temperature",
            "hourly",
            "hour by hour",
            "hour-by-hour",
            "rain",
            "wind",
            "humidity",
            "hot",
            "cold",
            "sunny",
            "cloudy",
            "storm",
            "timezone",
            "time zone",
            "utc offset",
            "location alias",
        ],
    )
}

fn recent_weather_hint(history: &[AssistantHistoryMessage]) -> Option<RecentWeatherHint> {
    for context in recent_follow_up_contexts(history) {
        let tool = match context.tool.as_str() {
            "weather_get_current" => AssistantToolName::WeatherGetCurrent,
            "weather_get_forecast" => AssistantToolName::WeatherGetForecast,
            "weather_get_history" => AssistantToolName::WeatherGetHistory,
            "weather_get_hourly_window" => AssistantToolName::WeatherGetHourlyWindow,
            "weather_resolve_location_alias" => AssistantToolName::WeatherResolveLocationAlias,
            "weather_get_forecast_for_date" => AssistantToolName::WeatherGetForecastForDate,
            "weather_get_recent_history_for_date" => {
                AssistantToolName::WeatherGetRecentHistoryForDate
            }
            _ => continue,
        };
        let location = context.input_hint.weather_location.clone()?;
        return Some(RecentWeatherHint {
            tool,
            location,
            forecast_days: context.input_hint.weather_days,
            start_date: context.input_hint.weather_start_date.clone(),
            end_date: context.input_hint.weather_end_date.clone(),
            label: context.input_hint.weather_label.clone(),
        });
    }
    None
}

fn recent_transcript_query_hint(history: &[AssistantHistoryMessage]) -> Option<String> {
    for context in recent_follow_up_contexts(history) {
        if context.tool != "channels_get_transcript_summary" {
            continue;
        }
        if let Some(query) = context.input_hint.channels_query.clone() {
            return Some(query);
        }
        if let Some(entity) = context.entities.first() {
            return Some(entity.label.clone());
        }
    }
    None
}

fn is_weather_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "weather",
            "forecast",
            "temperature",
            "rain",
            "raining",
            "precipitation",
            "wind",
            "windy",
            "humidity",
            "humid",
            "hourly",
            "hour by hour",
            "hour-by-hour",
            "sunny",
            "cloudy",
            "storm",
            "hot in ",
            "cold in ",
            "timezone",
            "time zone",
            "utc offset",
        ],
    )
}

fn weather_prefers_current(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "right now",
            "currently",
            "current",
            "at the moment",
            "temperature",
            "how hot",
            "how cold",
            "how warm",
        ],
    ) && !weather_prefers_forecast(message_lower)
        && !weather_prefers_history(message_lower)
}

fn weather_prefers_history(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &["yesterday", "last night", "last week", "earlier today"],
    )
}

fn is_weather_timezone_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "timezone",
            "time zone",
            "utc offset",
            "what time zone",
            "which timezone",
            "which time zone",
            "resolve location",
            "canonical location",
        ],
    )
}

fn weather_prefers_forecast(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "forecast",
            "tomorrow",
            "weekend",
            "this week",
            "next week",
            "next few days",
            "coming days",
            "will it",
            "rain chance",
            "expected",
        ],
    ) || extract_next_numbered_window(message_lower, "day", "days").is_some()
}

fn weather_prefers_hourly(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "hourly",
            "hour by hour",
            "hour-by-hour",
            "by hour",
            "by the hour",
            "every hour",
            "hourly forecast",
        ],
    )
}

fn extract_weather_forecast_days(message: &str) -> u8 {
    let lower = message.to_ascii_lowercase();
    let today = assistant_local_today();
    if let Some((date, _label)) = extract_single_calendar_date(message, today)
        && date >= today
    {
        return ((date - today).num_days() + 1).clamp(1, 7) as u8;
    }
    if lower.contains("tomorrow") {
        2
    } else if lower.contains("today") {
        1
    } else if lower.contains("weekend")
        || lower.contains("this week")
        || lower.contains("next week")
    {
        7
    } else if let Some(days) = extract_next_numbered_window(&lower, "day", "days") {
        days.clamp(1, 7) as u8
    } else {
        3
    }
}

fn extract_weather_location(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    for marker in [
        "weather in ",
        "weather for ",
        "forecast in ",
        "forecast for ",
        "temperature in ",
        "temperature for ",
        "conditions in ",
        "conditions for ",
        "rain in ",
        "rain for ",
        "raining in ",
        "wind in ",
        "wind for ",
        "windy in ",
        "humidity in ",
        "humidity for ",
        "humid in ",
        "humid for ",
        "hot in ",
        "cold in ",
        " for ",
        " in ",
    ] {
        if matches!(marker, " for " | " in ")
            && !weather_prefix_allows_generic_location_marker(&lower, marker)
        {
            continue;
        }
        if let Some(candidate) = extract_location_after_marker(message, &lower, marker) {
            return Some(candidate);
        }
    }
    None
}

fn weather_prefix_allows_generic_location_marker(message_lower: &str, marker: &str) -> bool {
    let Some(idx) = message_lower.find(marker) else {
        return false;
    };
    let prefix = message_lower[..idx].trim();
    if prefix.is_empty() {
        return false;
    }
    has_any(
        prefix,
        &[
            "weather",
            "forecast",
            "temperature",
            "conditions",
            "rain",
            "raining",
            "wind",
            "windy",
            "humidity",
            "humid",
            "hot",
            "cold",
            "today",
            "tomorrow",
            "yesterday",
            "week",
            "weekend",
        ],
    )
}

fn extract_location_after_marker(message: &str, lower: &str, marker: &str) -> Option<String> {
    let idx = lower.find(marker)?;
    let raw = message[idx + marker.len()..].trim();
    normalize_weather_location_candidate(raw)
}

fn normalize_weather_location_candidate(raw: &str) -> Option<String> {
    let trimmed =
        raw.trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch));
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed
        .strip_prefix("for ")
        .or_else(|| trimmed.strip_prefix("For "))
        .or_else(|| trimmed.strip_prefix("in "))
        .or_else(|| trimmed.strip_prefix("In "))
        .unwrap_or(trimmed)
        .trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = strip_weather_temporal_preamble(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let stop_markers = [
        " right now",
        " currently",
        " today",
        " tomorrow",
        " yesterday",
        " this week",
        " next week",
        " this weekend",
        " weekend",
        " last week",
        " last night",
        " next few days",
        " in the next ",
        " over the next ",
        " please",
    ];

    let mut end = trimmed.len();
    for marker in stop_markers {
        if let Some(idx) = lower.find(marker) {
            end = end.min(idx);
        }
    }

    let candidate = trimmed[..end]
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .trim();
    if candidate.is_empty() {
        return None;
    }
    Some(candidate.to_string())
}

fn strip_weather_temporal_preamble(candidate: &str) -> &str {
    let lower = candidate.to_ascii_lowercase();
    if !starts_with_weather_temporal_preamble(&lower) {
        return candidate;
    }

    let for_index = lower.find(" for ");
    let in_index = lower.find(" in ");
    let split_index = match (for_index, in_index) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    };
    split_index
        .and_then(|index| candidate.get(index + 4..))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(candidate)
}

fn starts_with_weather_temporal_preamble(lower: &str) -> bool {
    [
        "the next ",
        "next ",
        "this week",
        "next week",
        "this weekend",
        "weekend",
        "today",
        "tomorrow",
        "yesterday",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
        || starts_with_numbered_weather_window(lower)
}

fn starts_with_numbered_weather_window(lower: &str) -> bool {
    let mut parts = lower.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    first.parse::<u8>().is_ok() && matches!(second, "day" | "days" | "week" | "weeks")
}

fn extract_standalone_weather_location(message: &str) -> Option<String> {
    let candidate = normalize_weather_location_candidate(message)?;
    let lower = candidate.to_ascii_lowercase();
    if has_any(
        &lower,
        &[
            "weather",
            "forecast",
            "temperature",
            "rain",
            "wind",
            "humidity",
            "today",
            "tomorrow",
            "yesterday",
            "weekend",
            "this week",
            "next week",
        ],
    ) || [
        "what ", "when ", "where ", "why ", "how ", "will ", "did ", "does ", "is ", "are ",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
    {
        return None;
    }
    candidate
        .chars()
        .any(|ch| ch.is_ascii_alphabetic())
        .then_some(candidate)
}

fn extract_weather_history_window(
    message: &str,
    today: NaiveDate,
) -> Option<(NaiveDate, NaiveDate, String)> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("yesterday") || lower.contains("last night") {
        let date = today - Duration::days(1);
        return Some((date, date, "yesterday".to_string()));
    }
    if lower.contains("last week") {
        let start = today - Duration::days(7);
        let end = today - Duration::days(1);
        return Some((start, end, "last week".to_string()));
    }
    None
}

fn extract_public_web_url(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        })
        .find(|token| token.starts_with("http://") || token.starts_with("https://"))
        .map(str::to_string)
}

fn extract_public_web_search_query(message: &str) -> Option<String> {
    if !public_web_tools_enabled() {
        return None;
    }

    if extract_public_web_url(message).is_some() {
        return None;
    }

    let lower = message.to_ascii_lowercase();
    if is_weather_query(&lower) {
        return None;
    }

    for marker in [
        "search the web for ",
        "search the internet for ",
        "search online for ",
        "look up ",
        "look online for ",
        "find online ",
    ] {
        if let Some(idx) = lower.find(marker) {
            let query = message[idx + marker.len()..].trim();
            if !query.is_empty() {
                return Some(query.to_string());
            }
        }
    }

    if has_any(
        &lower,
        &[
            "latest news about ",
            "latest on ",
            "online about ",
            "on the web",
            "on the internet",
        ],
    ) {
        return Some(message.trim().to_string());
    }

    None
}

fn is_curated_web_catalog_query(message_lower: &str) -> bool {
    if !public_web_tools_enabled() {
        return false;
    }

    has_any(
        message_lower,
        &[
            "what sites",
            "what websites",
            "what sources",
            "which sites",
            "which websites",
            "which sources",
            "curated sources",
            "source catalog",
            "source list",
            "trusted sources",
            "recommended sites",
            "sites do you use",
            "websites do you use",
        ],
    ) || (has_any(message_lower, &["technology", "business", "economics"])
        && has_any(
            message_lower,
            &["sites", "websites", "sources", "source", "catalog", "list"],
        ))
}

fn validate_curated_web_category_slug(raw: &str) -> Option<String> {
    super::web_sources::CuratedWebCategory::from_slug(raw)
        .map(|category| category.slug().to_string())
}

fn infer_curated_web_category_slug(message: &str) -> Option<String> {
    if !public_web_tools_enabled() {
        return None;
    }

    super::web_sources::CuratedWebCategory::from_message(&message.to_ascii_lowercase())
        .map(|category| category.slug().to_string())
}

fn infer_curated_web_category_for_url(raw_url: &str) -> Option<String> {
    if !public_web_tools_enabled() {
        return None;
    }

    curated_web_category_for_url(raw_url).map(str::to_string)
}

fn recent_web_category(history: &[AssistantHistoryMessage]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| {
            message.role.eq_ignore_ascii_case("assistant") && !message.follow_up_contexts.is_empty()
        })
        .and_then(|message| {
            message.follow_up_contexts.iter().rev().find_map(|context| {
                context
                    .input_hint
                    .web_category
                    .clone()
                    .and_then(|raw| validate_curated_web_category_slug(&raw))
            })
        })
}

fn web_category_from_topic_key(topic_key: Option<&str>) -> Option<String> {
    topic_key
        .and_then(|value| value.trim().strip_prefix("web:"))
        .and_then(validate_curated_web_category_slug)
}

fn extract_library_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("what libraries")
        || lower.contains("which libraries")
        || lower.contains("do i have access to any libraries")
        || lower.contains("what libraries do i have")
    {
        return None;
    }

    let detail_hint = has_any(
        &lower,
        &[
            "tell me about",
            "tell me more",
            "details",
            "detail",
            "summary",
            "what is my",
            "what are my",
            "what kind of",
            "paths",
            "settings",
            "item count",
            "items count",
            "how many items",
            "collection",
        ],
    );
    let library_context = has_any(
        &lower,
        &[
            "library",
            "libraries",
            "collection",
            "collections",
            "movie",
            "movies",
            "show",
            "shows",
            "song",
            "songs",
            "track",
            "tracks",
            "album",
            "albums",
            "artist",
            "artists",
        ],
    );
    if !library_context {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        if detail_hint {
            return Some(quoted);
        }
    }

    for marker in [
        "tell me about the ",
        "tell me about my ",
        "tell me about ",
        "tell me more about the ",
        "tell me more about my ",
        "tell me more about ",
        "details for the ",
        "details for my ",
        "details for ",
        "summary of the ",
        "summary of my ",
        "summary of ",
        "what is the ",
        "what are the ",
        "what is my ",
        "what are my ",
        "what kind of library is the ",
        "what kind of library is my ",
        "what kind of library is ",
        "paths for the ",
        "paths for my ",
        "paths for ",
        "settings for the ",
        "settings for my ",
        "settings for ",
        "how many items in the ",
        "how many items in my ",
        "how many items in ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [" library", " libraries", " collection", " collections"] {
            if candidate.to_ascii_lowercase().ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn extract_library_search_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("access to") {
        return None;
    }

    let mentions_library_context = has_any(
        &lower,
        &[
            "library",
            "libraries",
            "movie",
            "movies",
            "show",
            "shows",
            "song",
            "songs",
            "track",
            "tracks",
            "album",
            "albums",
            "artist",
            "artists",
        ],
    );

    if let Some(quoted) = extract_quoted_phrase(message) {
        if has_any(
            &lower,
            &["do i have", "find", "search", "look for", "is there"],
        ) || mentions_library_context
        {
            return Some(quoted);
        }
    }

    let needles = [
        "do i have ",
        "find ",
        "search for ",
        "look for ",
        "is there ",
    ];

    for needle in needles {
        let Some(idx) = lower.find(needle) else {
            continue;
        };
        let rest = message[idx + needle.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [
            " in my library",
            " in the library",
            " in my libraries",
            " in the libraries",
            " on rustyfin",
        ] {
            if candidate.to_ascii_lowercase().ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if !candidate.is_empty() {
            if !mentions_library_context
                && !rest.to_ascii_lowercase().contains(" in my library")
                && !rest.to_ascii_lowercase().contains(" in the library")
                && !rest.to_ascii_lowercase().contains(" in my libraries")
                && !rest.to_ascii_lowercase().contains(" in the libraries")
            {
                continue;
            }
            return Some(candidate);
        }
    }

    None
}

fn is_recent_library_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "recently added",
            "recent additions",
            "new in my library",
            "newest in my library",
            "latest additions",
            "latest in my library",
            "what was added recently",
        ],
    )
}

fn extract_recent_library_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    let lower = message.to_ascii_lowercase();
    for marker in ["recently added ", "new in my library ", "latest additions "] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn extract_library_follow_up_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    let lower = message.to_ascii_lowercase();
    for needle in ["what about ", "how about ", "and ", "what else about "] {
        let Some(idx) = lower.find(needle) else {
            continue;
        };
        let rest = message[idx + needle.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() {
            continue;
        }
        if has_any(
            &candidate.to_ascii_lowercase(),
            &[
                "next week",
                "this week",
                "tomorrow",
                "today",
                "online",
                "offline",
                "healthy",
                "failed",
            ],
        ) {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn is_library_duplicate_titles_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "duplicate titles",
            "duplicate title",
            "duplicate library titles",
            "duplicate library title",
            "title collisions",
            "title collision",
            "same title",
            "duplicates",
            "duplicate items",
        ],
    ) && has_any(
        message_lower,
        &[
            "library",
            "libraries",
            "movie",
            "movies",
            "show",
            "shows",
            "song",
            "songs",
            "album",
            "albums",
            "artist",
            "artists",
            "collection",
            "collections",
            "title",
            "titles",
            "item",
            "items",
            "media",
        ],
    )
}

fn is_library_missing_metadata_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "missing metadata",
            "incomplete metadata",
            "metadata gaps",
            "missing metadata fields",
            "missing fields",
            "metadata audit",
            "incomplete library records",
            "missing information",
        ],
    ) && has_any(
        message_lower,
        &[
            "library",
            "libraries",
            "movie",
            "movies",
            "show",
            "shows",
            "song",
            "songs",
            "album",
            "albums",
            "artist",
            "artists",
            "collection",
            "collections",
            "metadata",
            "item",
            "items",
            "media",
        ],
    )
}

fn extract_library_media_detail_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let media_hint = has_any(
        &lower,
        &[
            "file path",
            "media path",
            "poster",
            "backdrop",
            "thumbnail",
            "thumb",
            "logo",
            "artwork",
            "cover",
            "stored",
            "location",
            "where is",
            "where's",
        ],
    );
    if !media_hint {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "what is the file path for the ",
        "what's the file path for the ",
        "what is the file path for ",
        "what's the file path for ",
        "what is the media path for the ",
        "what's the media path for the ",
        "what is the media path for ",
        "what's the media path for ",
        "what is the poster for the ",
        "what's the poster for the ",
        "what is the poster for ",
        "what's the poster for ",
        "what is the artwork for the ",
        "what's the artwork for the ",
        "what is the artwork for ",
        "what's the artwork for ",
        "what is the backdrop for the ",
        "what's the backdrop for the ",
        "what is the backdrop for ",
        "what's the backdrop for ",
        "what is the thumbnail for the ",
        "what's the thumbnail for the ",
        "what is the thumbnail for ",
        "what's the thumbnail for ",
        "what is the logo for the ",
        "what's the logo for the ",
        "what is the logo for ",
        "what's the logo for ",
        "tell me the artwork for the ",
        "tell me the artwork for ",
        "show me the artwork for the ",
        "show me the artwork for ",
        "where is the ",
        "where's the ",
        "where is ",
        "where's ",
        "where is my ",
        "where's my ",
        "where is this ",
        "where's this ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate_text = rest[..end].trim().to_string();
        if let Some(and_idx) = candidate_text.to_ascii_lowercase().find(" and ") {
            let after_and = candidate_text[and_idx + 5..].trim_start();
            if has_any(
                &after_and.to_ascii_lowercase(),
                &[
                    "what ", "who ", "where ", "when ", "why ", "how ", "is ", "are ", "do ",
                    "does ", "did ", "can ", "could ", "should ", "would ", "will ",
                ],
            ) {
                candidate_text.truncate(and_idx);
                candidate_text = candidate_text.trim().to_string();
            }
        }
        let candidate = normalize_library_media_detail_query_candidate(&candidate_text);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    extract_library_follow_up_query(message).or_else(|| extract_library_search_query(message))
}

fn normalize_library_media_detail_query_candidate(candidate: &str) -> String {
    let mut normalized = candidate.trim().trim_matches(['"', '\'', '`']).to_string();
    for suffix in [
        " file path",
        " media path",
        " poster",
        " backdrop",
        " thumbnail",
        " thumb",
        " logo",
        " artwork",
        " cover",
        " movie",
        " show",
        " song",
        " track",
        " album",
        " artist",
        " item",
        " stored",
    ] {
        if normalized.to_ascii_lowercase().ends_with(suffix) {
            let keep_len = normalized.len().saturating_sub(suffix.len());
            normalized.truncate(keep_len);
            normalized = normalized.trim().to_string();
        }
    }

    normalized
}

fn extract_memory_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        if is_memory_query(&lower) {
            return Some(quoted);
        }
    }

    for marker in [
        "what do you know about ",
        "what do you remember about ",
        "what can you tell me about ",
        "tell me about ",
        "remember that ",
        "remember ",
        "who is in my ",
        "who are in my ",
        "who is my ",
        "who are my ",
        "who is ",
        "who's ",
        "whos ",
        "who are ",
        "what is my ",
        "what are my ",
        "what's my ",
        "whats my ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn extract_library_source_paths_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let source_hint = has_any(
        &lower,
        &[
            "source path",
            "source paths",
            "file path",
            "file paths",
            "media path",
            "media paths",
            "where is the file",
            "where is the media",
            "where are the files",
            "where are the paths",
        ],
    );
    if !source_hint {
        return None;
    }

    extract_library_media_detail_query(message)
        .or_else(|| extract_library_detail_query(message))
        .or_else(|| extract_library_follow_up_query(message))
        .or_else(|| extract_library_search_query(message))
}

fn extract_memory_exact_entity_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }
    if !has_any(
        &lower,
        &[
            "exact",
            "exactly",
            "specific",
            "precise",
            "full name",
            "spelled",
            "spelling",
            "exact match",
        ],
    ) {
        return None;
    }

    extract_memory_query(message)
        .map(|candidate| normalize_memory_exact_entity_query_candidate(&candidate))
        .filter(|candidate| !candidate.is_empty())
}

fn normalize_memory_exact_entity_query_candidate(candidate: &str) -> String {
    let mut normalized = normalize_memory_query_candidate(candidate);
    for prefix in [
        "the exact ",
        "the specific ",
        "the precise ",
        "the full name ",
        "exact ",
        "specific ",
        "precise ",
    ] {
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim().to_string();
        }
    }
    normalized
}

fn extract_memory_relation_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    for marker in [
        "who is related to ",
        "who are related to ",
        "who is connected to ",
        "who are connected to ",
        "who is linked to ",
        "who are linked to ",
        "who is associated with ",
        "who are associated with ",
        "what is the relation of ",
        "what are the relations of ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_relation_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    extract_memory_query(message)
        .map(|candidate| normalize_memory_relation_query_candidate(&candidate))
        .filter(|candidate| !candidate.is_empty())
}

fn extract_memory_relation_path_query(message: &str) -> Option<(String, String)> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    for (marker, separator) in [
        ("how is ", " related to "),
        ("how are ", " related to "),
        ("what is the relation between ", " and "),
        ("what are the relations between ", " and "),
        ("what is the relationship between ", " and "),
        ("what is the connection between ", " and "),
        ("relationship between ", " and "),
        ("relation between ", " and "),
        ("connection between ", " and "),
        ("path between ", " and "),
        ("between ", " and "),
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        if let Some((source, target)) = split_memory_relation_path_candidate(&candidate, separator)
        {
            return Some((
                normalize_memory_relation_path_query_candidate(&source),
                normalize_memory_relation_path_query_candidate(&target),
            ));
        }
    }

    None
}

fn normalize_memory_query_candidate(candidate: &str) -> String {
    candidate
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '`', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_memory_relation_path_candidate(
    candidate: &str,
    separator: &str,
) -> Option<(String, String)> {
    let (left, right) = candidate.split_once(separator)?;
    let source = left.trim();
    let target = right.trim();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    Some((source.to_string(), target.to_string()))
}

fn normalize_memory_relation_path_query_candidate(candidate: &str) -> String {
    let mut normalized = normalize_memory_query_candidate(candidate);
    let lower = normalized.to_ascii_lowercase();
    for suffix in [
        " related",
        " related to",
        " connected",
        " connected to",
        " linked",
        " linked to",
        " associated",
        " associated with",
        " relation",
        " relations",
        " connection",
        " connections",
    ] {
        if lower.ends_with(suffix) {
            let keep_len = normalized.len().saturating_sub(suffix.len());
            normalized.truncate(keep_len);
            normalized = normalized.trim().to_string();
            break;
        }
    }
    normalized
}

fn normalize_memory_relation_query_candidate(candidate: &str) -> String {
    let mut normalized = normalize_memory_query_candidate(candidate);
    let lower = normalized.to_ascii_lowercase();
    for suffix in [
        " related to",
        " connected to",
        " linked to",
        " associated with",
        " relation",
        " relations",
        " connection",
        " connections",
        " family tree",
        " family relation",
        " family relations",
    ] {
        if lower.ends_with(suffix) {
            let keep_len = normalized.len().saturating_sub(suffix.len());
            normalized.truncate(keep_len);
            normalized = normalized.trim().to_string();
            break;
        }
    }
    normalized
}

fn extract_room_filter(message: &str) -> AssistantToolInput {
    AssistantToolInput::RoomsFilter {
        room_mode: detect_room_mode(message),
        query: extract_room_query(message),
    }
}

fn extract_room_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();

    if let Some(quoted) = extract_quoted_phrase(message) {
        if has_any(
            &lower,
            &["room", "rooms", "party", "screen share", "watch together"],
        ) {
            return Some(quoted);
        }
    }

    for marker in ["room called ", "room named ", "room "] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() {
            continue;
        }
        let candidate_lower = candidate.to_ascii_lowercase();
        if has_any(
            &candidate_lower,
            &["active", "inactive", "open", "closed", "youtube", "screen"],
        ) {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn detect_room_mode(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let room_mode = if has_any(&lower, &["youtube", "yt", "youtube party"]) {
        Some("youtube")
    } else if has_any(&lower, &["screen share", "screen", "share my screen"]) {
        Some("screen")
    } else if has_any(
        &lower,
        &[
            "listen together",
            "audio room",
            "music room",
            "audio",
            "music",
        ],
    ) {
        Some("audio")
    } else if has_any(&lower, &["web room", "website", "browser room", "web"]) {
        Some("web")
    } else if has_any(
        &lower,
        &[
            "create together",
            "canvas",
            "document",
            "whiteboard",
            "create",
        ],
    ) {
        Some("create")
    } else if has_any(
        &lower,
        &["play together", "game room", "gaming room", "play"],
    ) {
        Some("play")
    } else if has_any(&lower, &["watch party", "watch", "video room", "video"]) {
        Some("video")
    } else {
        None
    };
    room_mode.map(str::to_string)
}

fn is_joinable_rooms_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "joinable room",
            "joinable rooms",
            "can i join",
            "what rooms can i join",
            "what room can i join",
            "my invites",
            "room invites",
            "invites to rooms",
        ],
    )
}

fn extract_server_filter(message: &str) -> AssistantToolInput {
    AssistantToolInput::ServerFilter {
        query: extract_server_query(message),
        availability: extract_server_availability(message),
    }
}

fn is_ai_runtime_query(message_lower: &str) -> bool {
    if is_storage_query(message_lower) {
        return false;
    }

    let explicit = has_any(
        message_lower,
        &[
            "what ai model",
            "which ai model",
            "what model are you",
            "which model are you",
            "what model is loaded",
            "which model is loaded",
            "loaded ai model",
            "currently loaded model",
            "ai runtime",
            "inference backend",
            "what backend is ai using",
            "which backend is ai using",
            "what backend are you using",
            "which backend are you using",
            "warm pool",
            "queue depth",
            "hot model",
            "hot models",
            "role routing",
            "vram",
            "gpu memory",
            "video memory",
            "graphics memory",
            "gpu ram",
            "cuda memory",
        ],
    );
    if explicit {
        return true;
    }

    let mentions_model = message_lower.contains("model");
    let ai_scope = has_any(
        message_lower,
        &[
            "ai",
            "assistant",
            "inference",
            "loaded",
            "backend",
            "planner",
            "summarizer",
            "answer",
            "verifier",
            "worker",
            "warm pool",
            "scheduler",
            "queue",
        ],
    );

    mentions_model && ai_scope
}

fn is_host_runtime_query(message_lower: &str) -> bool {
    if is_ai_runtime_query(message_lower) {
        return false;
    }
    if message_lower.contains("minecraft") {
        return false;
    }

    let explicit_host_stats = has_any(
        message_lower,
        &[
            "host stats",
            "system stats",
            "runtime stats",
            "runtime diagnostics",
            "server stats",
            "server diagnostics",
        ],
    );
    let resource_keywords = has_any(
        message_lower,
        &[
            "ram",
            "memory",
            "swap",
            "cpu",
            "load average",
            "load",
            "uptime",
            "thread",
            "threads",
            "resource usage",
            "resources",
            "utilization",
            "usage",
        ],
    );
    let host_scope = has_any(
        message_lower,
        &["host", "system", "server", "machine", "rustyfin"],
    );
    let standalone_host_usage = has_any(
        message_lower,
        &[
            "how much ram",
            "how much memory",
            "memory usage",
            "cpu usage",
            "cpu threads",
            "how many threads",
        ],
    );

    explicit_host_stats || (resource_keywords && (host_scope || standalone_host_usage))
}

fn is_backup_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "backup",
            "backups",
            "restore",
            "restores",
            "recovery",
            "snapshot",
            "snapshots",
        ],
    )
}

fn is_memory_recent_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "what do you remember",
            "what memories",
            "recent memory",
            "recent memories",
            "recent facts",
            "show me my memories",
            "show me recent memories",
            "what stored facts",
            "what facts do you remember",
        ],
    )
}

fn is_memory_recent_changes_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "what changed recently",
            "what changed in my memory",
            "what changed in memory",
            "what changed about",
            "what updated about",
            "recent changes",
            "recent updates",
            "what's new",
            "whats new",
            "what's new in my memory",
            "what's new in memory",
        ],
    )
}

fn extract_memory_recent_changes_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    for marker in [
        "what changed recently about ",
        "what changed about ",
        "what updated about ",
        "what's new about ",
        "whats new about ",
        "what's new in my memory about ",
        "what's new in memory about ",
        "what changed in my memory about ",
        "what changed in memory about ",
        "recent changes about ",
        "recent updates about ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn is_memory_recent_entity_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "what people do you remember",
            "who do you remember",
            "recent people",
            "recent entities",
            "show me remembered people",
            "show me recent people",
            "list remembered people",
            "list remembered entities",
            "what entities do you remember",
            "people you remember",
            "entities you remember",
        ],
    )
}

fn is_memory_conflict_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "conflicting facts",
            "contradictory facts",
            "inconsistent facts",
            "memory conflicts",
            "what conflicts do you have about",
            "what conflicting facts do you have about",
            "what contradictory facts do you have about",
            "what inconsistent facts do you have about",
            "conflicts about",
            "conflicting facts about",
            "contradictory facts about",
            "inconsistent facts about",
        ],
    )
}

fn extract_memory_conflict_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    for marker in [
        "what conflicting facts do you have about ",
        "what contradictory facts do you have about ",
        "what inconsistent facts do you have about ",
        "what conflicts do you have about ",
        "conflicting facts about ",
        "contradictory facts about ",
        "inconsistent facts about ",
        "conflicts about ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn is_memory_provenance_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "where did you learn about",
            "where did you learn ",
            "what is the source of",
            "what's the source of",
            "what is the source for",
            "what's the source for",
            "what is the provenance of",
            "what's the provenance of",
            "source of",
            "source for",
            "origin of",
            "origin for",
            "where did you get the information about",
            "where did that come from",
        ],
    )
}

fn extract_memory_provenance_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    for marker in [
        "where did you learn about ",
        "where did you learn ",
        "what is the source of ",
        "what's the source of ",
        "what is the source for ",
        "what's the source for ",
        "what is the provenance of ",
        "what's the provenance of ",
        "source of ",
        "source for ",
        "origin of ",
        "origin for ",
        "where did you get the information about ",
        "where did that come from ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    None
}

fn is_memory_person_summary_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "person summary",
            "profile summary",
            "person profile",
            "profile details",
            "profile info",
            "profile information",
            "person details",
            "person info",
            "person information",
        ],
    )
}

fn extract_memory_person_summary_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if message_has_other_domain_context(&lower) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "person summary for ",
        "person summary of ",
        "person summary ",
        "profile summary for ",
        "profile summary of ",
        "profile summary ",
        "person profile for ",
        "person profile of ",
        "person profile ",
        "profile details for ",
        "profile details of ",
        "profile details ",
        "profile info for ",
        "profile info of ",
        "profile info ",
        "profile information for ",
        "profile information of ",
        "profile information ",
        "person details for ",
        "person details of ",
        "person details ",
        "person info for ",
        "person info of ",
        "person info ",
        "person information for ",
        "person information of ",
        "person information ",
    ] {
        let Some(candidate) = extract_tail_after_marker(message, &lower, marker) else {
            continue;
        };
        let candidate = normalize_memory_query_candidate(&candidate);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    extract_memory_exact_entity_query(message).or_else(|| extract_memory_query(message))
}

fn is_memory_fact_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "what do you know about",
            "what do you remember about",
            "what can you tell me about",
            "tell me about",
            "remember that",
            "what is my ",
            "what are my ",
            "what's my ",
            "whats my ",
            "my favorite",
            "my preference",
            "my preferences",
            "my allergy",
            "my allergies",
            "my memory",
            "my memories",
            "my note",
            "my notes",
        ],
    )
}

fn is_memory_exact_entity_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "exact",
            "exactly",
            "specific",
            "precise",
            "full name",
            "spelled",
            "spelling",
            "exact match",
        ],
    ) && is_memory_entity_query(message_lower)
        || has_any(
            message_lower,
            &[
                "exact",
                "exactly",
                "specific",
                "precise",
                "full name",
                "spelled",
                "spelling",
                "exact match",
            ],
        ) && is_memory_fact_query(message_lower)
}

fn is_memory_entity_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "who is ",
            "who's ",
            "whos ",
            "who are ",
            "who is in my",
            "who are in my",
            "who is my",
            "who are my",
            "family group",
            "my family",
            "my group",
            "my people",
            "my person",
            "mother",
            "father",
            "brother",
            "sister",
            "sibling",
            "partner",
            "spouse",
            "friend",
            "coworker",
            "colleague",
        ],
    )
}

fn is_memory_relation_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    has_any(
        message_lower,
        &[
            "related to",
            "connected to",
            "linked to",
            "associated with",
            "relation",
            "relations",
            "family tree",
            "family relation",
            "family relations",
            "who is related to",
            "who are related to",
            "who is connected to",
            "who are connected to",
            "who is linked to",
            "who are linked to",
            "who is associated with",
            "who are associated with",
            "what is the relation of",
            "what are the relations of",
        ],
    )
}

fn is_memory_relation_path_query(message_lower: &str) -> bool {
    if message_has_other_domain_context(message_lower) {
        return false;
    }

    (has_any(
        message_lower,
        &[
            "related to",
            "connected to",
            "linked to",
            "associated with",
            "relation between",
            "relationship between",
            "connection between",
            "path between",
        ],
    ) && has_any(
        message_lower,
        &[
            " and ",
            " between ",
            " how is ",
            " how are ",
            "what is the relation between",
            "what are the relations between",
        ],
    )) || has_any(message_lower, &["||"])
}

fn is_memory_query(message_lower: &str) -> bool {
    is_memory_recent_query(message_lower)
        || is_memory_recent_entity_query(message_lower)
        || is_memory_fact_query(message_lower)
        || is_memory_entity_query(message_lower)
        || is_memory_relation_query(message_lower)
        || is_memory_exact_entity_query(message_lower)
        || is_memory_person_summary_query(message_lower)
        || is_memory_relation_path_query(message_lower)
        || is_memory_recent_changes_query(message_lower)
        || is_memory_conflict_query(message_lower)
        || is_memory_provenance_query(message_lower)
}

fn extract_dictionary_relationship_reference(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    [
        "my mother",
        "my mum",
        "my mom",
        "my father",
        "my dad",
        "my parents",
        "my parent",
        "my brother",
        "my sister",
        "my siblings",
        "my sibling",
        "my spouse",
        "my partner",
        "my wife",
        "my husband",
        "my friends",
        "my friend",
        "my co-workers",
        "my co-worker",
        "my coworkers",
        "my coworker",
        "my colleagues",
        "my colleague",
        "my child",
        "my children",
        "my son",
        "my daughter",
        "my grandparents",
        "my grandparent",
    ]
    .iter()
    .find(|needle| lower.contains(**needle))
    .map(|needle| (*needle).to_string())
}

fn is_dictionary_workspace_listing_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "dictionary workspaces",
            "human dictionary workspaces",
            "show me my dictionary workspaces",
            "list my dictionary workspaces",
            "which dictionary workspaces",
            "what dictionary workspaces",
        ],
    )
}

fn extract_dictionary_workspace_selector(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if has_any(
        &lower,
        &[
            "family dictionary",
            "family workspace",
            "family root",
            "my family",
        ],
    ) {
        Some("family".to_string())
    } else if has_any(
        &lower,
        &[
            "friends dictionary",
            "friend dictionary",
            "friends workspace",
            "friends root",
            "my friends",
            "my friend dictionary",
        ],
    ) {
        Some("friends".to_string())
    } else if has_any(
        &lower,
        &[
            "work dictionary",
            "work workspace",
            "work root",
            "my work",
            "co-worker dictionary",
        ],
    ) {
        Some("work".to_string())
    } else {
        None
    }
}

fn is_dictionary_workspace_people_browse_query(message_lower: &str) -> bool {
    has_any(message_lower, &["dictionary", "workspace", "root"])
        && has_any(
            message_lower,
            &[
                "who is in",
                "who's in",
                "show me",
                "list people",
                "browse people",
                "find ",
                "search for ",
                "look for ",
            ],
        )
}

fn extract_dictionary_workspace_people_query(message: &str) -> Option<String> {
    if let Some(quoted) = extract_quoted_phrase(message) {
        let trimmed = quoted.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let source = message.trim();
    let lower = source.to_ascii_lowercase();
    for needle in ["find ", "search for ", "look for "] {
        if let Some(start) = lower.find(needle) {
            let remainder = &source[start + needle.len()..];
            let remainder_lower = &lower[start + needle.len()..];
            let stop = [" in my ", " in the ", " inside my ", " inside the "]
                .iter()
                .filter_map(|delimiter| remainder_lower.find(delimiter))
                .min()
                .unwrap_or(remainder.len());
            let candidate = remainder[..stop]
                .trim()
                .trim_matches(|ch: char| matches!(ch, '.' | '?' | '!' | '"' | '\''));
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

fn message_has_other_domain_context(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "calendar",
            "birthday",
            "download",
            "library",
            "movie",
            "show",
            "weather",
            "network",
            "server",
            "room",
            "transcript",
            "service health",
            "backup",
            "storage",
            "ai runtime",
            "current date",
            "current time",
        ],
    )
}

fn is_service_health_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "service health",
            "services healthy",
            "services health",
            "agent health",
            "agents healthy",
            "what services are up",
            "what services are down",
            "is the system healthy",
            "internal services",
        ],
    )
}

fn is_failed_units_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "failed unit",
            "failed units",
            "failed service",
            "failed services",
            "systemd failed",
            "systemctl failed",
            "systemd units failed",
            "systemd units are failed",
            "which systemd units are failed",
            "what systemd units are failed",
            "what failed",
            "what services failed",
            "what units failed",
            "which services failed",
            "which units failed",
        ],
    )
}

fn is_port_conflicts_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "port conflict",
            "port conflicts",
            "port in use",
            "ports in use",
            "listening socket",
            "listening sockets",
            "bound port",
            "bound ports",
            "what port is in use",
            "which port is in use",
            "what is using port",
            "which process is using port",
        ],
    )
}

fn is_network_default_route_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "default route",
            "default gateway",
            "gateway",
            "outbound route",
            "outbound gateway",
            "which route",
            "what route",
            "how does traffic leave",
            "route to the internet",
        ],
    )
}

fn is_network_hostname_aliases_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "hostname aliases",
            "host aliases",
            "hostnames",
            "/etc/hosts",
            "host name aliases",
            "what names does this host have",
            "what host names does this host have",
        ],
    )
}

fn is_network_dns_servers_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "dns",
            "dns servers",
            "dns server",
            "nameserver",
            "nameservers",
            "resolver",
            "resolvers",
            "resolv.conf",
            "resolvectl",
        ],
    )
}

fn is_direct_model_chat_request(message: &str, history: &[AssistantHistoryMessage]) -> bool {
    let lower = message.to_ascii_lowercase();
    let trimmed = lower.trim();
    if trimmed.is_empty() {
        return true;
    }

    if is_tool_inventory_query(trimmed)
        || is_supported_calendar_create_intent(trimmed)
        || is_supported_calendar_delete_intent(trimmed)
        || is_supported_document_create_intent(trimmed)
        || is_supported_conversation_manage_intent(trimmed)
        || is_unsupported_write_intent(trimmed)
        || clarification_for_message(message).is_some()
        || is_non_birthday_calendar_query(trimmed)
        || is_next_calendar_event_query(trimmed)
        || is_next_calendar_event_timing_query(trimmed)
        || is_calendar_conflict_query(trimmed)
        || is_calendar_free_days_query(trimmed)
        || is_calendar_busy_days_query(trimmed)
        || is_calendar_event_count_query(trimmed)
        || extract_calendar_event_detail_query(message).is_some()
        || extract_birthday_query(message).is_some()
        || is_channel_activity_query(trimmed)
        || is_transcript_summary_query(trimmed)
        || is_joinable_rooms_query(trimmed)
        || is_network_query(trimmed)
        || is_current_datetime_query(trimmed)
        || is_ai_runtime_query(trimmed)
        || is_host_runtime_query(trimmed)
        || is_backup_query(trimmed)
        || is_memory_query(trimmed)
        || is_service_health_query(trimmed)
        || is_failed_units_query(trimmed)
        || is_failed_unit_detail_query(trimmed)
        || is_transcode_query(trimmed)
        || is_storage_query(trimmed)
        || is_recent_errors_query(trimmed)
        || is_weather_query(trimmed)
        || extract_weather_location(message).is_some()
        || extract_public_web_url(message).is_some()
        || extract_public_web_search_query(message).is_some()
        || is_curated_web_catalog_query(trimmed)
        || infer_curated_web_category_slug(trimmed).is_some()
        || extract_download_artifact_detail_query(message).is_some()
        || extract_download_artifact_source_query(message).is_some()
        || extract_download_artifact_release_notes_query(message).is_some()
        || extract_download_artifact_checksum_query(message).is_some()
        || extract_download_artifact_install_steps_query(message).is_some()
        || extract_download_artifact_compatibility_query(message).is_some()
        || message_has_downloads_follow_up_hint(message)
        || extract_network_interface_query(message).is_some()
        || is_network_default_route_query(trimmed)
        || is_network_hostname_aliases_query(trimmed)
        || is_network_dns_servers_query(trimmed)
        || extract_network_dns_servers_query(message).is_some()
        || extract_service_detail_query(message).is_some()
        || extract_failed_unit_detail_query(message).is_some()
        || extract_process_detail_query(message).is_some()
        || extract_listener_detail_query(message).is_some()
        || extract_disk_usage_detail_query(message).is_some()
        || extract_library_detail_query(message).is_some()
        || extract_library_media_detail_query(message).is_some()
        || extract_library_source_paths_query(message).is_some()
        || message_has_library_media_follow_up_hint(message)
        || is_single_library_media_detail_follow_up(message, history)
        || extract_library_search_query(message).is_some()
        || is_recent_library_query(trimmed)
        || is_library_duplicate_titles_query(trimmed)
        || is_library_missing_metadata_query(trimmed)
        || message_has_library_listing_follow_up_hint(message)
        || extract_server_query(message).is_some()
        || extract_server_availability(message).is_some()
        || extract_follow_up_entity_reference(message).is_some()
    {
        return false;
    }

    is_reset_conversation_request(trimmed)
        || is_tone_or_style_request(trimmed)
        || is_smalltalk_message(trimmed)
        || is_joke_message(trimmed, history)
        || is_general_math_or_pi_message(trimmed)
        || is_self_introduction_message(trimmed)
}

fn is_reset_conversation_request(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "reset chat",
            "reset conversation",
            "reset this conversation",
            "start fresh",
            "start afresh",
            "start over",
            "fresh start",
            "new conversation",
            "stop looking",
            "stop searching",
            "nevermind",
            "never mind",
        ],
    ) || matches!(
        message_lower.trim(),
        "reset" | "clear chat" | "clear conversation"
    )
}

fn is_tone_or_style_request(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "pirate talk",
            "talk like a pirate",
            "speak like a pirate",
            "use pirate",
            "from now on",
            "only use",
            "make this change permanent",
            "keep this permanent",
            "keep talking like",
            "change your tone",
            "change your style",
        ],
    )
}

fn is_smalltalk_message(message_lower: &str) -> bool {
    let trimmed = message_lower.trim();
    matches!(
        trimmed,
        "hi" | "hello"
            | "hello?"
            | "hey"
            | "hey there"
            | "yo"
            | "test"
            | "thanks"
            | "thank you"
            | "cheers"
            | "yarrr"
            | "yarrrr"
            | "yarr"
            | ":("
    ) || has_any(
        trimmed,
        &[
            "do you love",
            "are you mates with",
            "are u mates with",
            "who are you",
            "what are you",
            "can you chat",
            "let's start afresh",
            "lets start afresh",
        ],
    )
}

fn is_joke_message(message_lower: &str, history: &[AssistantHistoryMessage]) -> bool {
    if has_any(
        message_lower,
        &[
            "tell me a joke",
            "funny joke",
            "what do you call ",
            "make me laugh",
            "say something funny",
        ],
    ) {
        return true;
    }

    let trimmed = message_lower.trim();
    if trimmed.split_whitespace().count() <= 3
        && history
            .iter()
            .rev()
            .find(|message| message.role.eq_ignore_ascii_case("user"))
            .is_some_and(|message| {
                let previous = message.content.to_ascii_lowercase();
                previous.contains("what do you call ")
                    || previous.contains("tell me a joke")
                    || previous.contains("funny joke")
            })
    {
        return true;
    }

    false
}

fn is_general_math_or_pi_message(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "plus ",
            " minus ",
            " times ",
            " multiplied by ",
            " divided by ",
            "digit of pi",
            "digits of pi",
            "give me pi",
            "value of pi",
        ],
    )
}

fn is_self_introduction_message(message_lower: &str) -> bool {
    let trimmed = message_lower.trim();
    trimmed.starts_with("i am ")
        || trimmed.starts_with("i'm ")
        || trimmed.starts_with("im ")
        || trimmed.starts_with("my name is ")
        || trimmed.starts_with("call me ")
}

fn oversized_numeric_dump_response(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("pi")
        && has_any(
            &lower,
            &[
                "all numbers",
                "all digits",
                "every digit",
                "10000",
                "10,000",
                "1000 digits",
                "10,000 digits",
            ],
        )
    {
        return Some(
            "I can give a short excerpt of pi or help with a specific digit or range, but I won’t dump an enormous wall of digits into the chat. Tell me the position or range you want."
                .to_string(),
        );
    }

    None
}

fn is_unsafe_destructive_host_request(message_lower: &str) -> bool {
    let destructive_verb = has_any(
        message_lower,
        &[
            "delete ", "remove ", "format ", "wipe ", "erase ", "destroy ", "nuke ", "rm -rf",
        ],
    );
    let destructive_target = has_any(
        message_lower,
        &[
            "system32",
            "system 32",
            "operating system",
            "os drive",
            "boot drive",
            "hard drive",
            "disk",
            "computer",
            "pc",
            "laptop",
            "filesystem",
            "file system",
            "windows folder",
        ],
    ) || has_any(
        message_lower,
        &[
            "format system",
            "format my computer",
            "wipe my computer",
            "wipe the disk",
            "erase the disk",
        ],
    );

    destructive_verb && destructive_target
}

fn is_transcode_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "transcode",
            "transcoding",
            "ffmpeg",
            "ffprobe",
            "hardware acceleration",
            "hw accel",
            "transcoder",
            "transcode session",
            "transcode sessions",
        ],
    )
}

fn is_storage_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "storage",
            "disk",
            "disks",
            "free space",
            "space left",
            "cache dir",
            "cache directory",
            "model dir",
            "model directory",
            "media path",
            "media root",
        ],
    )
}

fn is_current_datetime_query(message_lower: &str) -> bool {
    let simple_current_datetime = has_any(
        message_lower,
        &[
            "what date is it",
            "what day is it",
            "what time is it",
            "what is the date",
            "what is the day",
            "what is the time",
            "what's the date",
            "whats the date",
            "what's the time",
            "whats the time",
            "what's today's date",
            "whats today's date",
            "today's date",
            "todays date",
            "current date",
            "current time",
            "current day",
            "date today",
            "time right now",
            "date right now",
            "server time",
            "host time",
            "fetch the time",
            "get the time",
        ],
    );
    if simple_current_datetime {
        return true;
    }

    if has_any(
        message_lower,
        &["calendar", "event", "events", "birthday", "birthdays"],
    ) {
        return false;
    }

    contains_weekday_name(message_lower)
        && has_any(
            message_lower,
            &[
                "what date",
                "what day",
                "which date",
                "which day",
                "when is",
                "when would",
                "what date would",
                "what day would",
                "what date does",
                "what day does",
                "would be",
                "falls on",
                "fall on",
                "lands on",
                "land on",
                "calendar date",
            ],
        )
}

fn contains_weekday_name(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ],
    )
}

fn message_has_current_datetime_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    extract_single_calendar_date(message, assistant_local_today())
        .map(|(_, matched_text)| {
            !matches!(
                matched_text.to_ascii_lowercase().as_str(),
                "today" | "tomorrow" | "day after tomorrow"
            )
        })
        .unwrap_or(false)
        || contains_weekday_name(&lower)
        || contains_day_of_month_reference(&lower)
        || has_any(
            &lower,
            &[
                "surely",
                "that would",
                "it would",
                "so that would",
                "so it would",
                "isn't it",
                "isnt it",
                "correct date",
                "correct day",
                "wrong date",
                "wrong day",
            ],
        )
}

fn message_has_current_datetime_tool_follow_up_hint(message: &str) -> bool {
    message_has_current_datetime_follow_up_hint(message)
        || extract_single_calendar_date(message, assistant_local_today()).is_some()
}

fn contains_day_of_month_reference(message_lower: &str) -> bool {
    message_lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| {
            let suffix = if token.len() > 2 {
                &token[token.len() - 2..]
            } else {
                ""
            };
            matches!(suffix, "st" | "nd" | "rd" | "th")
                && token[..token.len().saturating_sub(2)]
                    .parse::<u32>()
                    .ok()
                    .is_some_and(|day| (1..=31).contains(&day))
        })
}

#[derive(Debug, Deserialize)]
struct GroundedCurrentDateTimeSummary {
    local_date: String,
    local_time: String,
    timezone_offset: String,
}

pub fn deterministic_current_datetime_reply(
    message: &str,
    history: &[AssistantHistoryMessage],
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    if grounding_blocks.len() != 1 {
        return None;
    }
    let block = grounding_blocks.first()?;
    if block.tool != "system_get_current_datetime" {
        return None;
    }
    if block.status != "ok" {
        return Some("I couldn't load the current Rustyfin host date and time.".to_string());
    }

    let summary =
        serde_json::from_value::<GroundedCurrentDateTimeSummary>(block.data.clone()).ok()?;
    let today = NaiveDate::parse_from_str(&summary.local_date, "%Y-%m-%d").ok()?;
    if let Some((resolved_date, phrase)) =
        resolve_current_datetime_reference(message, history, today)
    {
        return Some(format!(
            "From Rustyfin's current local date, {}, {} is {}.",
            format_with_weekday(today),
            phrase,
            format_with_weekday(resolved_date),
        ));
    }

    let lower = message.to_ascii_lowercase();
    if lower.contains("time") && !lower.contains("date") && !lower.contains("day") {
        return Some(format!(
            "The current Rustyfin host local time is {} on {} (UTC{}).",
            summary.local_time,
            format_without_weekday(today),
            summary.timezone_offset,
        ));
    }

    Some(format!(
        "Today is {}. The current Rustyfin host local time is {} (UTC{}).",
        format_with_weekday(today),
        summary.local_time,
        summary.timezone_offset,
    ))
}

fn resolve_current_datetime_reference(
    message: &str,
    history: &[AssistantHistoryMessage],
    today: NaiveDate,
) -> Option<(NaiveDate, String)> {
    extract_single_calendar_date(message, today)
        .or_else(|| recent_current_datetime_reference(history, today))
}

fn recent_current_datetime_reference(
    history: &[AssistantHistoryMessage],
    today: NaiveDate,
) -> Option<(NaiveDate, String)> {
    history.iter().rev().find_map(|message| {
        if !message.role.eq_ignore_ascii_case("user") {
            return None;
        }

        let lower = message.content.to_ascii_lowercase();
        if !is_current_datetime_query(&lower) {
            return None;
        }

        extract_single_calendar_date(&message.content, today)
    })
}

fn format_with_weekday(date: NaiveDate) -> String {
    date.format("%A, %B %-d, %Y").to_string()
}

fn format_without_weekday(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

fn is_recent_errors_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "recent errors",
            "recent failures",
            "what failed",
            "what is failing",
            "what's failing",
            "problems lately",
            "issues lately",
            "error summary",
            "failure summary",
        ],
    )
}

fn is_tool_inventory_query(message_lower: &str) -> bool {
    if message_lower.contains("list of all the functions")
        || message_lower.contains("list all the functions")
        || message_lower.contains("available functions")
        || message_lower.contains("available tools")
        || message_lower.contains("tool list")
        || message_lower.contains("function list")
        || message_lower.contains("grounded tools")
        || message_lower.contains("what tools do you have access")
        || message_lower.contains("what functions do you have access")
        || message_lower.contains("what tools can you use")
        || message_lower.contains("what functions can you use")
    {
        return true;
    }

    let mentions_inventory_target = has_any(
        message_lower,
        &["tool", "tools", "function", "functions", "capabilities"],
    );
    let asks_for_inventory = has_any(
        message_lower,
        &[
            "what are",
            "which are",
            "list",
            "show",
            "available",
            "access to",
            "have access",
            "can you use",
            "can you access",
        ],
    );
    let points_at_assistant = has_any(
        message_lower,
        &[
            "you",
            "your",
            "this environment",
            "rustyfin ai",
            "assistant",
        ],
    );

    mentions_inventory_target && asks_for_inventory && points_at_assistant
}

pub fn deterministic_tool_inventory_reply(user: &AuthUser, message: &str) -> Option<String> {
    if !is_tool_inventory_query(&message.to_ascii_lowercase()) {
        return None;
    }

    let visible_tools = AssistantToolName::all()
        .iter()
        .copied()
        .filter(|tool| tool_visible_to_user(*tool, user))
        .collect::<Vec<_>>();

    let mut lines = vec![format!(
        "I can use these grounded Rustyfin functions in this environment for your account ({} total):",
        visible_tools.len()
    )];
    if user.role == "admin" {
        lines.push(
            "Admin-only host diagnostics are included because your account has admin access."
                .to_string(),
        );
    }

    for tool in visible_tools {
        let spec = tool.spec();
        let mut notes = vec![match spec.access_mode {
            super::types::ToolAccessMode::ReadOnly => "read-only".to_string(),
            super::types::ToolAccessMode::Write => "write".to_string(),
            super::types::ToolAccessMode::DestructiveWrite => "destructive-write".to_string(),
        }];
        match spec.confirmation {
            super::types::ToolConfirmationPolicy::None => {}
            super::types::ToolConfirmationPolicy::ExplicitUserConfirm => {
                notes.push("confirmation required".to_string())
            }
            super::types::ToolConfirmationPolicy::ProtectedAction => {
                notes.push("protected action".to_string())
            }
        }
        lines.push(format!(
            "- {}: {} [{}]",
            spec.name,
            spec.summary,
            notes.join(", ")
        ));
    }

    lines.push(
        "I do not have arbitrary shell, database, or filesystem access. I can only use the backend-owned Rustyfin functions listed above."
            .to_string(),
    );

    Some(lines.join("\n"))
}

fn is_network_query(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "network topology",
            "network map",
            "network status",
            "network interfaces",
            "network interface",
            "dns",
            "dns servers",
            "nameserver",
            "nameservers",
            "resolver",
            "resolvers",
            "resolv.conf",
            "remote access",
            "trusted proxy",
            "trusted proxies",
            "hostname",
            "host name",
            "lan ip",
            "local ip",
            "ip address",
            "ip addresses",
            "default route",
            "default gateway",
            "hostname aliases",
            "host aliases",
            "local network",
            "connect to rustyfin",
            "open rustyfin",
            "which ip would i use",
            "what ip would i use",
            "what url should i use",
            "which url should i use",
            "what port should i use",
            "which port should i use",
            "what network",
            "what interfaces",
        ],
    ) || ((has_any(
        message_lower,
        &[
            "network",
            "topology",
            "interfaces",
            "interface",
            "proxy",
            "proxies",
        ],
    ) && !message_lower.contains("internet"))
        || ((message_lower.contains("connect") || message_lower.contains("open"))
            && message_lower.contains("rustyfin"))
        || (message_lower.contains("local network") && message_lower.contains("rustyfin"))
        || ((message_lower.contains("ip")
            || message_lower.contains("url")
            || message_lower.contains("port"))
            && message_lower.contains("rustyfin")))
}

fn extract_server_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();

    if let Some(quoted) = extract_quoted_phrase(message) {
        if has_any(&lower, &["server", "servers", "minecraft"]) {
            return Some(quoted);
        }
    }

    for marker in [
        "server called ",
        "server named ",
        "server ",
        "minecraft server ",
    ] {
        let Some(idx) = lower.find(marker) else {
            continue;
        };
        let rest = message[idx + marker.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let mut candidate = rest[..end].trim().to_string();
        for suffix in [
            " online",
            " offline",
            " running",
            " stopped",
            " healthy",
            " failed",
            " with errors",
            " in error",
        ] {
            if candidate.to_ascii_lowercase().ends_with(suffix) {
                let keep_len = candidate.len().saturating_sub(suffix.len());
                candidate.truncate(keep_len);
                candidate = candidate.trim().to_string();
            }
        }
        if candidate.is_empty() {
            continue;
        }
        let candidate_lower = candidate.to_ascii_lowercase();
        if has_any(
            &candidate_lower,
            &[
                "online", "offline", "running", "stopped", "healthy", "failed", "error", "errors",
            ],
        ) {
            continue;
        }
        return Some(candidate);
    }

    None
}

fn extract_server_availability(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if has_any(
        &lower,
        &[
            "failed",
            "failing",
            "error",
            "errors",
            "broken",
            "unhealthy",
        ],
    ) {
        Some("problem".to_string())
    } else if has_any(&lower, &["healthy", "ready"]) {
        Some("healthy".to_string())
    } else if has_any(&lower, &["offline", "stopped", "down"]) {
        Some("offline".to_string())
    } else if has_any(&lower, &["online", "running", "up"]) {
        Some("online".to_string())
    } else {
        None
    }
}

fn extract_quoted_phrase(message: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let mut parts = message.split(quote);
        let _before = parts.next()?;
        let candidate = parts.next()?.trim();
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    None
}

pub(crate) fn extract_birthday_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let has_birthday_context = has_any(
        &lower,
        &["birthday", "birthdays", "born", "turning", "turns"],
    );

    if has_birthday_context {
        if let Some(quoted) = extract_quoted_phrase(message) {
            return normalize_birthday_query_candidate(&quoted);
        }

        for suffix in ["'s birthday", "’s birthday", " birthday", " birthdays"] {
            if let Some(idx) = lower.find(suffix) {
                let prefix = message[..idx].trim();
                if let Some(candidate) = extract_named_tail_after_marker(
                    prefix,
                    &[
                        "when is ",
                        "when's ",
                        "whens ",
                        "what is ",
                        "what's ",
                        "whats ",
                        "tell me about ",
                        "tell me ",
                        "check ",
                        "is ",
                    ],
                )
                .and_then(|candidate| normalize_birthday_query_candidate(&candidate))
                {
                    return Some(candidate);
                }
            }
        }

        for marker in [
            "birthday of ",
            "birthday for ",
            "when is ",
            "when's ",
            "whens ",
            "what is ",
            "what's ",
            "whats ",
        ] {
            if let Some(candidate) = extract_tail_after_marker(message, &lower, marker)
                .and_then(|candidate| normalize_birthday_query_candidate(&candidate))
            {
                return Some(candidate);
            }
        }
    }

    extract_follow_up_subject_query(message)
        .and_then(|candidate| normalize_birthday_query_candidate(&candidate))
}

fn extract_follow_up_subject_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    for needle in [
        "what about ",
        "how about ",
        "and what about ",
        "and ",
        "what else about ",
    ] {
        let Some(idx) = lower.find(needle) else {
            continue;
        };
        let rest = message[idx + needle.len()..].trim();
        if rest.is_empty() {
            continue;
        }
        let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
        let candidate = rest[..end].trim();
        if candidate.is_empty() || looks_like_calendar_window_phrase(candidate) {
            continue;
        }
        return Some(candidate.to_string());
    }

    None
}

fn extract_named_tail_after_marker(message: &str, markers: &[&str]) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    for marker in markers {
        if let Some(candidate) = extract_tail_after_marker(message, &lower, marker) {
            return Some(candidate);
        }
    }
    None
}

fn extract_tail_after_marker(message: &str, lower: &str, marker: &str) -> Option<String> {
    let idx = lower.find(marker)?;
    let rest = message[idx + marker.len()..].trim();
    if rest.is_empty() {
        return None;
    }
    let end = rest.find(['?', '!', '.', ',']).unwrap_or(rest.len());
    let candidate = rest[..end].trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

fn normalize_birthday_query_candidate(candidate: &str) -> Option<String> {
    let mut normalized = candidate
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .to_string();
    if normalized.is_empty() {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    if looks_like_calendar_window_phrase(&lower) {
        return None;
    }

    for prefix in ["the birthday of ", "birthday of ", "birthday for ", "the "] {
        if lower.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim().to_string();
            break;
        }
    }

    loop {
        let lower = normalized.to_ascii_lowercase();
        let mut stripped = false;
        for suffix in [
            "'s birthday event",
            "’s birthday event",
            " birthday event",
            " birthday date",
            " birthday day",
            "'s birthday",
            "’s birthday",
            " birthday",
            " birthdays",
            "'s",
            "’s",
            " event",
            " events",
        ] {
            if lower.ends_with(suffix) {
                let keep_len = normalized.len().saturating_sub(suffix.len());
                normalized.truncate(keep_len);
                normalized = normalized.trim().to_string();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    if normalized.is_empty() {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    let words = lower.split_whitespace().collect::<Vec<_>>();
    let is_generic_qualifier = |word: &str| {
        matches!(
            word,
            "next" | "upcoming" | "nearest" | "coming" | "soon" | "one" | "ones"
        )
    };
    let is_generic_birthday_word = |word: &str| {
        is_generic_qualifier(word)
            || matches!(
                word,
                "birthday"
                    | "birthdays"
                    | "event"
                    | "events"
                    | "calendar"
                    | "in"
                    | "my"
                    | "the"
                    | "a"
                    | "an"
            )
    };
    if matches!(lower.as_str(), "my" | "me" | "mine")
        || (words.first() == Some(&"my")
            && words.iter().skip(1).all(|word| is_generic_qualifier(word)))
    {
        return Some("my".to_string());
    }
    if looks_like_calendar_window_phrase(&lower)
        || words.iter().all(|word| is_generic_birthday_word(word))
        || words.iter().all(|word| is_generic_qualifier(word))
        || matches!(
            lower.as_str(),
            "it" | "them" | "those" | "these" | "ones" | "one" | "the next"
        )
    {
        return None;
    }

    Some(normalized)
}

pub(crate) fn is_next_birthday_request(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if !lower.contains("birthday") {
        return false;
    }

    let singular_birthday = lower.contains("birthday") && !lower.contains("birthdays");
    (singular_birthday
        && (lower.contains("next birthday")
            || lower.contains("nearest birthday")
            || lower.contains("upcoming birthday")
            || lower.contains("next birthday event")
            || (lower.contains("next") && lower.contains("calendar"))))
        || lower.contains("my birthday")
        || lower.contains("my next")
}

fn birthday_fallback_days(message: &str, query: Option<&str>) -> i64 {
    if query.is_some() || is_next_birthday_request(message) {
        366
    } else {
        30
    }
}

pub(crate) fn birthday_calendar_window_input(
    message: &str,
    query: Option<String>,
) -> AssistantToolInput {
    extract_calendar_window(
        message,
        birthday_fallback_days(message, query.as_deref()),
        query,
    )
}

fn looks_like_calendar_window_phrase(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    calendar_query_has_explicit_window(&lower)
        || has_any(
            &lower,
            &[
                "coming up",
                "upcoming",
                "next week",
                "this week",
                "next month",
                "this month",
                "today",
                "tomorrow",
                "soon",
            ],
        )
}

fn extract_calendar_window(
    message: &str,
    fallback_days: i64,
    query: Option<String>,
) -> AssistantToolInput {
    let today = assistant_local_today();
    let lower = message.to_ascii_lowercase();

    if let Some((date, matched_text)) = extract_single_calendar_date(message, today) {
        return AssistantToolInput::CalendarWindow {
            from_date: date.format("%F").to_string(),
            to_date: date.format("%F").to_string(),
            label: calendar_date_label(date, &matched_text),
            query,
        };
    }

    let (from, to, label) = if lower.contains("tomorrow") {
        let day = today + Duration::days(1);
        (day, day, "tomorrow".to_string())
    } else if lower.contains("today") {
        (today, today, "today".to_string())
    } else if lower.contains("next week") {
        next_week_window(today)
    } else if lower.contains("this week") {
        this_week_window(today)
    } else if lower.contains("next month") {
        next_month_window(today)
    } else if lower.contains("this month") {
        this_month_window(today)
    } else if lower.contains("this weekend") || lower.contains("weekend") {
        weekend_window(today)
    } else if let Some(days) = extract_next_numbered_window(&lower, "day", "days") {
        let to = today + Duration::days(days);
        (today, to, format!("the next {days} days"))
    } else if let Some(weeks) = extract_next_numbered_window(&lower, "week", "weeks") {
        let to = today + Duration::days(weeks * 7);
        (today, to, format!("the next {weeks} weeks"))
    } else if let Some(months) = extract_next_numbered_window(&lower, "month", "months") {
        let (from, to) = next_n_months_window(today, months);
        (from, to, format!("the next {months} months"))
    } else {
        let to = today + Duration::days(fallback_days);
        (today, to, format!("the next {fallback_days} days"))
    };

    AssistantToolInput::CalendarWindow {
        from_date: from.format("%F").to_string(),
        to_date: to.format("%F").to_string(),
        label,
        query,
    }
}

fn calendar_date_label(date: NaiveDate, matched_text: &str) -> String {
    let normalized = strip_calendar_date_prefix(matched_text);
    let lower = normalized.to_ascii_lowercase();
    if lower == "today"
        || lower == "tomorrow"
        || lower == "day after tomorrow"
        || lower.starts_with("next ")
        || lower.starts_with("this ")
    {
        normalized.to_string()
    } else {
        format!("{} ({normalized})", date.format("%F"))
    }
}

fn strip_calendar_date_prefix(matched_text: &str) -> &str {
    let trimmed = matched_text.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("on ") {
        trimmed[3..].trim_start()
    } else if lower.starts_with("for ") {
        trimmed[4..].trim_start()
    } else {
        trimmed
    }
}

fn extract_next_numbered_window(message_lower: &str, singular: &str, plural: &str) -> Option<i64> {
    let marker = "next ";
    let idx = message_lower.find(marker)?;
    let rest = &message_lower[idx + marker.len()..];
    let mut parts = rest.split_whitespace();
    let number = parts.next()?.parse::<i64>().ok()?;
    let unit = parts
        .next()?
        .trim_matches(|c: char| !c.is_ascii_alphabetic());
    if unit == singular || unit == plural {
        Some(number.clamp(1, 90))
    } else {
        None
    }
}

fn this_week_window(today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    let days_from_monday = today.weekday().num_days_from_monday() as i64;
    let start = today - Duration::days(days_from_monday);
    let end = start + Duration::days(6);
    (start, end, "this week".to_string())
}

fn next_week_window(today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    let (this_week_start, _, _) = this_week_window(today);
    let start = this_week_start + Duration::days(7);
    let end = start + Duration::days(6);
    (start, end, "next week".to_string())
}

fn weekend_window(today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    let target_saturday = match today.weekday() {
        Weekday::Sat => today,
        Weekday::Sun => today - Duration::days(1),
        weekday => {
            let days_until_saturday =
                Weekday::Sat.num_days_from_monday() as i64 - weekday.num_days_from_monday() as i64;
            today + Duration::days(days_until_saturday)
        }
    };
    let end = target_saturday + Duration::days(1);
    (target_saturday, end, "this weekend".to_string())
}

fn this_month_window(today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    let start = today.with_day(1).unwrap_or(today);
    let next_month_start = if today.month() == 12 {
        NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap_or(today)
    } else {
        NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1).unwrap_or(today)
    };
    let end = next_month_start - Duration::days(1);
    (start, end, "this month".to_string())
}

fn next_month_window(today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    let (_, this_month_end, _) = this_month_window(today);
    let start = this_month_end + Duration::days(1);
    let end = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1).unwrap_or(start) - Duration::days(1)
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1).unwrap_or(start)
            - Duration::days(1)
    };
    (start, end, "next month".to_string())
}

fn next_n_months_window(today: NaiveDate, months: i64) -> (NaiveDate, NaiveDate) {
    let start = today;
    let mut year = today.year();
    let mut month = today.month() as i64;
    month += months;
    while month > 12 {
        month -= 12;
        year += 1;
    }
    let next_boundary = NaiveDate::from_ymd_opt(year, month as u32, 1)
        .unwrap_or(today + Duration::days(30 * months));
    let end = next_boundary - Duration::days(1);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::{
        AssistantToolName, MAX_TOOL_CALLS_PER_TURN, PlannedToolCall, PlannerAst,
        clarification_for_message, deterministic_current_datetime_reply,
        deterministic_tool_inventory_reply, extract_birthday_query, parse_planner_ast,
        plan_tool_calls, plan_tool_calls_with_history, plan_tool_calls_with_model_assist,
        status_label_for_tool_call, unsafe_action_response_for_message,
        unsupported_write_response_for_message, validate_planner_ast,
    };
    use crate::ai_assistant::dates::assistant_local_today;
    use crate::ai_assistant::types::{
        AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
        AssistantHistoryMessage, AssistantPlannerMode, AssistantToolContextBlock,
        AssistantToolInput,
    };
    use crate::auth::AuthUser;
    use futures::stream::{self, BoxStream};
    use rustfin_ai_agent::{
        BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptBackend, PromptCacheHint,
        SamplingParams,
    };
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{Mutex, OnceLock};

    fn auth_user(role: &str) -> AuthUser {
        AuthUser {
            user_id: "user-1".to_string(),
            username: "tester".to_string(),
            role: role.to_string(),
        }
    }

    fn grounded_history(tool_names: &[&str]) -> Vec<AssistantHistoryMessage> {
        vec![AssistantHistoryMessage {
            role: "assistant".to_string(),
            content: "Grounded answer".to_string(),
            grounding_tools: tool_names.iter().map(|tool| (*tool).to_string()).collect(),
            follow_up_contexts: Vec::new(),
            grounding_chunks: Vec::new(),
        }]
    }

    fn history_with_follow_up_context(
        tool: &str,
        entities: &[&str],
        input_hint: AssistantFollowUpInputHint,
    ) -> Vec<AssistantHistoryMessage> {
        vec![AssistantHistoryMessage {
            role: "assistant".to_string(),
            content: "Grounded answer".to_string(),
            grounding_tools: vec![tool.to_string()],
            follow_up_contexts: vec![AssistantFollowUpContext {
                tool: tool.to_string(),
                label: "Context".to_string(),
                input_hint,
                entities: entities
                    .iter()
                    .enumerate()
                    .map(|(index, label)| AssistantFollowUpEntity {
                        ordinal: index + 1,
                        label: (*label).to_string(),
                        identifier: None,
                        ..Default::default()
                    })
                    .collect(),
            }],
            grounding_chunks: Vec::new(),
        }]
    }

    fn assert_tools_include(tools: &[PlannedToolCall], expected: &[AssistantToolName]) {
        for tool_name in expected {
            assert!(
                tools.iter().any(|tool| tool.tool == *tool_name),
                "expected tool {:?} to be planned, got {:?}",
                tool_name,
                tools.iter().map(|tool| tool.tool).collect::<Vec<_>>()
            );
        }
    }

    struct PublicWebToolsEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl PublicWebToolsEnvGuard {
        fn enable() -> Self {
            let previous = std::env::var_os(crate::ai_assistant::web::AI_PUBLIC_WEB_ENABLE_ENV);
            unsafe {
                std::env::set_var(crate::ai_assistant::web::AI_PUBLIC_WEB_ENABLE_ENV, "1");
            }
            Self { previous }
        }
    }

    impl Drop for PublicWebToolsEnvGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(previous) => unsafe {
                    std::env::set_var(crate::ai_assistant::web::AI_PUBLIC_WEB_ENABLE_ENV, previous);
                },
                None => unsafe {
                    std::env::remove_var(crate::ai_assistant::web::AI_PUBLIC_WEB_ENABLE_ENV);
                },
            }
        }
    }

    fn with_public_web_tools_enabled<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("public web tools test lock");
        let _env_guard = PublicWebToolsEnvGuard::enable();
        f()
    }

    fn grounded_datetime_block(local_date: &str, weekday: &str) -> AssistantToolContextBlock {
        AssistantToolContextBlock {
            tool: "system_get_current_datetime",
            label: format!("Rustyfin host local date and time: {local_date} ({weekday})"),
            status: "ok",
            data: json!({
                "local_timestamp": format!("{local_date} 12:00:00 +00:00"),
                "local_date": local_date,
                "local_time": "12:00:00",
                "weekday": weekday,
                "timezone_offset": "+00:00",
                "unix_timestamp": 1775121600_i64,
            }),
        }
    }

    struct MockPromptBackend {
        responses: Mutex<VecDeque<String>>,
    }

    impl MockPromptBackend {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(
                    responses
                        .into_iter()
                        .map(str::to_string)
                        .collect::<VecDeque<_>>(),
                ),
            }
        }
    }

    impl PromptBackend for MockPromptBackend {
        fn backend_kind(&self) -> BackendKind {
            BackendKind::Local
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                kind: BackendKind::Local,
                supports_streaming: true,
                supports_prompt_cache: false,
                supports_structured_output: true,
                can_degrade: true,
                max_parallel_requests: 1,
            }
        }

        fn chat_stream_boxed(
            &self,
            _messages: Vec<ChatMessage>,
            _sampling: SamplingParams,
            _prompt_cache: Option<PromptCacheHint>,
        ) -> BoxStream<'static, Result<ChatChunk, rustfin_ai_agent::AiError>> {
            let response = self
                .responses
                .lock()
                .expect("mock planner response lock")
                .pop_front()
                .unwrap_or_default();
            Box::pin(stream::iter(vec![
                Ok(ChatChunk::Token(response)),
                Ok(ChatChunk::Done),
            ]))
        }
    }

    #[test]
    fn planner_detects_calendar_birthday_queries() {
        let tools = plan_tool_calls("Who has a birthday coming up soon?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
    }

    #[test]
    fn planner_extracts_named_birthday_query() {
        let tools = plan_tool_calls("When is Rachel's birthday?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { query, .. } => {
                assert_eq!(query.as_deref(), Some("Rachel"));
            }
            _ => panic!("expected birthday calendar window"),
        }
    }

    #[test]
    fn clarification_triggers_for_ambiguous_calendar_question() {
        let clarification = clarification_for_message("What's on my calendar?");
        assert!(clarification.is_some());
    }

    #[test]
    fn clarification_does_not_trigger_for_explicit_calendar_window() {
        let message = "What events do I have this week?";
        assert!(clarification_for_message(message).is_none());
    }

    #[test]
    fn clarification_does_not_trigger_for_relative_weekday_calendar_window() {
        let message = "What events do I have next Tuesday?";
        assert!(clarification_for_message(message).is_none());
    }

    #[test]
    fn planner_detects_multiple_grounding_domains() {
        let tools = plan_tool_calls("What rooms are active and what Minecraft servers are online?");
        assert_eq!(
            tools.iter().map(|tool| tool.tool).collect::<Vec<_>>(),
            vec![
                AssistantToolName::RoomsListActive,
                AssistantToolName::ServersListMinecraftStatus
            ]
        );
    }

    #[test]
    fn planner_caps_tool_count() {
        let tools = plan_tool_calls(
            "Who am I, what events are coming up, what rooms are active, what libraries do I have, and what servers are online?",
        );
        assert!(tools.len() <= MAX_TOOL_CALLS_PER_TURN);
        assert_tools_include(
            &tools,
            &[
                AssistantToolName::AccountGetProfileSummary,
                AssistantToolName::CalendarListEvents,
                AssistantToolName::RoomsListActive,
                AssistantToolName::LibrariesListAccessible,
                AssistantToolName::ServersListMinecraftStatus,
            ],
        );
    }

    #[test]
    fn planner_extracts_library_search_query() {
        let tools = plan_tool_calls("Do I have \"Interstellar\" in my library?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrarySearchTitles);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Interstellar"),
            _ => panic!("expected library search input"),
        }
    }

    #[test]
    fn planner_extracts_library_summary_query() {
        let tools = plan_tool_calls("Tell me about my Movies library.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesGetLibrarySummary);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Movies"),
            _ => panic!("expected library summary input"),
        }
    }

    #[test]
    fn planner_detects_host_runtime_query() {
        let tools = plan_tool_calls("How much RAM is the server using right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::SystemGetHostRuntimeSummary
        );
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_ai_runtime_model_queries() {
        let tools = plan_tool_calls("What AI model is loaded right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetAiRuntimeSummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_ai_runtime_identity_queries() {
        let tools = plan_tool_calls("What AI model are you?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetAiRuntimeSummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_vram_questions_to_ai_runtime_tool_direct() {
        let tools = plan_tool_calls("How much VRAM are the GPUs using right now?");
        let ai_runtime = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::SystemGetAiRuntimeSummary)
            .expect("expected AI runtime tool to be planned");
        assert!(matches!(ai_runtime.input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_host_diagnostics_follow_up_to_gpu_inventory() {
        let history = grounded_history(&["system_get_host_runtime_summary"]);
        let tools = plan_tool_calls_with_history("What about the GPU?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetGpuInventory);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_network_follow_up_to_vpn_status() {
        let history = grounded_history(&["network_get_topology_summary"]);
        let tools = plan_tool_calls_with_history("What about the VPN?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetVpnStatus);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_extracts_downloads_filter() {
        let tools = plan_tool_calls("What downloads are available right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsListAvailableArtifacts
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter {
                query,
                availability,
            } => {
                assert_eq!(query, &None);
                assert_eq!(availability.as_deref(), Some("available"));
            }
            _ => panic!("expected downloads filter"),
        }
    }

    #[test]
    fn planner_extracts_download_artifact_detail_query() {
        let tools = plan_tool_calls("Tell me more about the RustyVault browser extension package.");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsGetArtifactDetails
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter { query, .. } => {
                assert_eq!(query.as_deref(), Some("RustyVault browser extension"));
            }
            _ => panic!("expected download detail filter"),
        }
    }

    #[test]
    fn planner_detects_network_query() {
        let tools = plan_tool_calls("What network interfaces are active right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetTopologySummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_detects_network_interface_by_ip_query() {
        let tools = plan_tool_calls("Which interface owns 192.168.0.36?");
        assert!(!tools.is_empty());
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetInterfaceByIp);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::NetworkInterface { .. }
        ));
    }

    #[test]
    fn planner_detects_default_route_query() {
        let tools = plan_tool_calls("What is the default route on this machine?");
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetDefaultRoute);
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::NetworkGetDefaultRoute)
        );
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::NetworkDefaultRoute { .. }
        ));
    }

    #[test]
    fn planner_detects_hostname_aliases_query() {
        let tools = plan_tool_calls("What hostname aliases does this host have?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetHostnameAliases);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::NetworkHostnameAliases { .. }
        ));
    }

    #[test]
    fn planner_detects_dns_servers_query() {
        let tools = plan_tool_calls("What DNS servers does this host use?");
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetDnsServers);
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::NetworkGetDnsServers)
        );
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::NetworkDnsServers { .. }
        ));
    }

    #[test]
    fn planner_routes_network_diagnostics_queries() {
        let tools = plan_tool_calls(
            "Show me the route table, active connections, interface counters, Wi-Fi status, and VPN status on this host.",
        );
        assert_tools_include(
            &tools,
            &[
                AssistantToolName::NetworkGetRouteTable,
                AssistantToolName::NetworkGetActiveConnections,
                AssistantToolName::NetworkGetInterfaceCounters,
                AssistantToolName::NetworkGetWifiStatus,
                AssistantToolName::NetworkGetVpnStatus,
            ],
        );
    }

    #[test]
    fn planner_routes_system_diagnostics_queries() {
        let tools = plan_tool_calls(
            "Show me the kernel info, CPU topology, temperature sensors, GPU inventory, PCI devices, USB devices, boot logs, and journal summary on this host.",
        );
        assert_tools_include(
            &tools,
            &[
                AssistantToolName::SystemGetKernelInfo,
                AssistantToolName::SystemGetCpuTopology,
                AssistantToolName::SystemGetTemperatureSensors,
                AssistantToolName::SystemGetGpuInventory,
                AssistantToolName::SystemGetPciDevices,
                AssistantToolName::SystemGetUsbDevices,
                AssistantToolName::SystemGetBootLogSummary,
                AssistantToolName::SystemGetJournalSummary,
            ],
        );
    }

    #[test]
    fn planner_routes_local_network_connect_query() {
        let tools = plan_tool_calls(
            "If I was on the local network, what IP would I use to connect to Rustyfin?",
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetTopologySummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_keeps_tool_inventory_queries_off_grounded_tools() {
        let tools = plan_tool_calls(
            "Give me a list of all the functions you have access to in this environment",
        );
        assert!(tools.is_empty());
    }

    #[test]
    fn planner_detects_current_weather_query() {
        let tools = plan_tool_calls("What is the temperature in Dublin right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetCurrent);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Dublin");
                assert_eq!(*forecast_days, None);
            }
            _ => panic!("expected weather input"),
        }
    }

    #[test]
    fn planner_detects_weather_forecast_query() {
        let tools = plan_tool_calls("What is the weather forecast for Cork tomorrow?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Cork");
                assert_eq!(*forecast_days, Some(2));
            }
            _ => panic!("expected weather input"),
        }
    }

    #[test]
    fn planner_detects_qualified_weekday_weather_forecast_query() {
        let message = "What's the weather next Tuesday in Campile, Ireland?";
        let tools = plan_tool_calls(message);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecastForDate);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                let today = assistant_local_today();
                let (date, _) =
                    crate::ai_assistant::dates::extract_single_calendar_date(message, today)
                        .expect("expected qualified weekday date");
                assert_eq!(location, "Campile, Ireland");
                assert_eq!(
                    *forecast_days,
                    Some(((date - today).num_days() + 1).clamp(1, 7) as u8)
                );
            }
            _ => panic!("expected weather input"),
        }
    }

    #[test]
    fn planner_detects_exact_weather_history_date_query() {
        let today = assistant_local_today();
        let target_date = today - chrono::Duration::days(1);
        let message = format!(
            "What was the weather on {} in Galway?",
            target_date.format("%F")
        );
        let tools = plan_tool_calls(&message);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::WeatherGetRecentHistoryForDate
        );
        match &tools[0].input {
            AssistantToolInput::WeatherHistory {
                location,
                start_date,
                end_date,
                label,
            } => {
                let expected = target_date.format("%F").to_string();
                assert_eq!(location, "Galway");
                assert_eq!(start_date, &expected);
                assert_eq!(end_date, &expected);
                assert_eq!(label, &expected);
            }
            _ => panic!("expected weather history input"),
        }
    }

    #[test]
    fn planner_detects_calendar_conflict_query() {
        let message = "Do I have any calendar conflicts next Tuesday?";
        let tools = plan_tool_calls(message);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListDateConflicts);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow {
                from_date,
                to_date,
                label,
                ..
            } => {
                let today = assistant_local_today();
                let (date, _) =
                    crate::ai_assistant::dates::extract_single_calendar_date(message, today)
                        .expect("expected qualified weekday date");
                let expected = date.format("%F").to_string();
                assert_eq!(from_date, &expected);
                assert_eq!(to_date, &expected);
                assert!(label.contains("next Tuesday"));
            }
            _ => panic!("expected calendar window input"),
        }
    }

    #[test]
    fn planner_detects_calendar_free_days_query() {
        let tools = plan_tool_calls("What days are free next week?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListFreeDays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow {
                from_date,
                to_date,
                label,
                ..
            } => {
                let today = assistant_local_today();
                let (expected_from, expected_to, expected_label) = super::next_week_window(today);
                assert_eq!(from_date, &expected_from.format("%F").to_string());
                assert_eq!(to_date, &expected_to.format("%F").to_string());
                assert_eq!(label, &expected_label);
            }
            _ => panic!("expected calendar window input"),
        }
    }

    #[test]
    fn planner_detects_calendar_event_count_query() {
        let tools = plan_tool_calls("How many events do I have next week?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarCountEvents);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow {
                from_date,
                to_date,
                label,
                ..
            } => {
                let today = assistant_local_today();
                let (expected_from, expected_to, expected_label) = super::next_week_window(today);
                assert_eq!(from_date, &expected_from.format("%F").to_string());
                assert_eq!(to_date, &expected_to.format("%F").to_string());
                assert_eq!(label, &expected_label);
            }
            _ => panic!("expected calendar window input"),
        }
    }

    #[test]
    fn planner_detects_calendar_busy_days_query() {
        let tools = plan_tool_calls("Which days are busiest next week?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListBusyDays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow {
                from_date,
                to_date,
                label,
                ..
            } => {
                let today = assistant_local_today();
                let (expected_from, expected_to, expected_label) = super::next_week_window(today);
                assert_eq!(from_date, &expected_from.format("%F").to_string());
                assert_eq!(to_date, &expected_to.format("%F").to_string());
                assert_eq!(label, &expected_label);
            }
            _ => panic!("expected calendar window input"),
        }
    }

    #[test]
    fn planner_detects_next_event_timing_query() {
        let tools = plan_tool_calls("How long until my next event?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarGetNextEventTiming);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_detects_weather_history_query() {
        let tools = plan_tool_calls("Did it rain yesterday in Galway?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetHistory);
        match &tools[0].input {
            AssistantToolInput::WeatherHistory {
                location,
                start_date,
                end_date,
                label,
            } => {
                let expected = (assistant_local_today() - chrono::Duration::days(1))
                    .format("%F")
                    .to_string();
                assert_eq!(location, "Galway");
                assert_eq!(start_date, &expected);
                assert_eq!(end_date, &expected);
                assert_eq!(label, "yesterday");
            }
            _ => panic!("expected weather history input"),
        }
    }

    #[test]
    fn planner_extracts_weather_location_with_county_phrase() {
        let tools = plan_tool_calls(
            "What is the weather going to be like this week for Campile in County Wexford?",
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Campile in County Wexford");
                assert_eq!(*forecast_days, Some(7));
            }
            _ => panic!("expected weather forecast input"),
        }
    }

    #[test]
    fn planner_extracts_weather_location_after_numbered_window_phrase() {
        let tools =
            plan_tool_calls("what is the weather for the next 7 days for Campile in Ireland?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Campile in Ireland");
                assert_eq!(*forecast_days, Some(7));
            }
            _ => panic!("expected weather forecast input"),
        }
    }

    #[test]
    fn clarification_triggers_for_weather_without_location() {
        let clarification = clarification_for_message("What's the weather like?");
        assert!(clarification.is_some());
    }

    #[test]
    fn planner_detects_curated_web_catalog_query() {
        with_public_web_tools_enabled(|| {
            let tools = plan_tool_calls("What sites do you use for technology?");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, AssistantToolName::WebListCuratedSources);
            assert!(matches!(tools[0].input, AssistantToolInput::None));
        });
    }

    #[test]
    fn planner_detects_technology_web_search_query() {
        with_public_web_tools_enabled(|| {
            let tools = plan_tool_calls("search the web for Rust compiler release notes");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, AssistantToolName::WebSearchPublicWeb);
            match &tools[0].input {
                AssistantToolInput::WebSearch { query, category } => {
                    assert_eq!(query, "Rust compiler release notes");
                    assert_eq!(category.as_deref(), Some("technology"));
                }
                _ => panic!("expected web search input"),
            }
        });
    }

    #[test]
    fn planner_detects_economics_web_search_query() {
        with_public_web_tools_enabled(|| {
            let tools = plan_tool_calls("search the web for CPI inflation update");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, AssistantToolName::WebSearchPublicWeb);
            match &tools[0].input {
                AssistantToolInput::WebSearch { query, category } => {
                    assert_eq!(query, "CPI inflation update");
                    assert_eq!(category.as_deref(), Some("economics"));
                }
                _ => panic!("expected web search input"),
            }
        });
    }

    #[test]
    fn planner_detects_business_web_fetch_query() {
        with_public_web_tools_enabled(|| {
            let tools = plan_tool_calls("Fetch https://www.reuters.com/markets/");
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, AssistantToolName::WebFetchPublicPageSummary);
            match &tools[0].input {
                AssistantToolInput::WebFetch { url, category } => {
                    assert_eq!(url, "https://www.reuters.com/markets/");
                    assert_eq!(category.as_deref(), Some("business"));
                }
                _ => panic!("expected web fetch input"),
            }
        });
    }

    #[test]
    fn planner_keeps_library_access_question_as_library_listing() {
        let tools = plan_tool_calls("Do I have access to any libraries?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesListAccessible);
    }

    #[test]
    fn planner_keeps_joke_queries_off_grounded_tools() {
        let tools = plan_tool_calls("Tell me a funny joke");
        assert!(tools.is_empty());
    }

    #[test]
    fn planner_keeps_style_requests_off_grounded_tools() {
        let tools = plan_tool_calls("From now on, only use pirate talk. Yarrrrr");
        assert!(tools.is_empty());
    }

    #[test]
    fn planner_keeps_greetings_off_library_follow_up_history() {
        let history = grounded_history(&["library_search_titles"]);
        let tools = plan_tool_calls_with_history("hello", &history);
        assert!(tools.is_empty());
    }

    #[test]
    fn planner_ast_parses_and_validates_valid_tool_plan() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"Dune\"}}]}",
        )
        .expect("expected parsed planner AST");
        let tools = validate_planner_ast(&ast, &auth_user("user"), "Do I have Dune?", &[])
            .expect("expected validated tool plan");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrarySearchTitles);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Dune"),
            _ => panic!("expected normalized library search"),
        }
    }

    #[test]
    fn planner_ast_accepts_markdown_fenced_json() {
        let ast = parse_planner_ast(
            "```json\n{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"Dune\"}}]}\n```",
        )
        .expect("expected parsed planner AST");
        assert!(matches!(ast, PlannerAst::ToolPlan { .. }));
    }

    #[test]
    fn planner_ast_rejects_unknown_tool_names() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"totally_unknown_tool\"}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(&ast, &auth_user("user"), "hello", &[])
            .expect_err("expected unknown tool rejection");
        assert!(issues.iter().any(|issue| issue.code == "unknown_tool"));
    }

    #[test]
    fn planner_ast_rejects_excessive_tool_count() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"calendar_get_next_event\"},{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"Dune\"}},{\"tool\":\"rooms_list_active\"},{\"tool\":\"servers_list_minecraft_status\"},{\"tool\":\"libraries_list_accessible\"},{\"tool\":\"system_get_ai_runtime_summary\"},{\"tool\":\"network_get_topology_summary\"},{\"tool\":\"calendar_get_next_event_timing\"},{\"tool\":\"downloads_list_available_artifacts\",\"args\":{\"availability\":\"available\"}}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(
            &ast,
            &auth_user("admin"),
            "What is next on my calendar, do I have Dune, what rooms are active, what servers are online, which libraries do I have, what AI model is loaded, what does my network look like, how long until my next event, and what downloads are available?",
            &[],
        )
        .expect_err("expected tool-count rejection");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "tool_count_exceeded")
        );
    }

    #[test]
    fn planner_ast_rejects_invalid_enum_values() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"rooms_list_active\",\"args\":{\"room_mode\":\"party\"}}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(&ast, &auth_user("user"), "What rooms are active?", &[])
            .expect_err("expected invalid enum rejection");
        assert!(issues.iter().any(|issue| issue.code == "invalid_enum"));
    }

    #[test]
    fn planner_ast_rejects_missing_required_args() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\"}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(&ast, &auth_user("user"), "Hello there", &[])
            .expect_err("expected missing arg rejection");
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "missing_required_argument")
        );
    }

    #[test]
    fn planner_ast_rejects_write_tools_from_model_output() {
        let ast = parse_planner_ast(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"calendar_create_event\",\"args\":{\"query\":\"Team sync\"}}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(&ast, &auth_user("user"), "Add Team sync tomorrow", &[])
            .expect_err("expected write-tool rejection");
        assert!(issues.iter().any(|issue| issue.code == "tool_not_allowed"));
    }

    #[tokio::test]
    async fn planner_repair_succeeds_after_one_failed_parse() {
        let backend = MockPromptBackend::new(vec![
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\"}]",
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"Dune\"}}]}",
        ]);
        let planned = plan_tool_calls_with_model_assist(
            &backend,
            &auth_user("user"),
            "Do I have \"Dune\" in my library?",
            &[],
        )
        .await;
        assert_eq!(planned.mode, AssistantPlannerMode::DeterministicFallback);
        assert_eq!(planned.calls.len(), 1);
        assert_eq!(
            planned.calls[0].tool,
            AssistantToolName::LibrarySearchTitles
        );
        assert_eq!(planned.debug.execution.parse_attempts, 2);
        assert_eq!(planned.debug.execution.repair_attempts, 1);
        assert_eq!(planned.debug.execution.repair_successes, 1);
        assert_eq!(planned.debug.repair_records.len(), 1);
        assert!(planned.debug.repair_records[0].repaired_successfully);
    }

    #[tokio::test]
    async fn planner_repair_exhaustion_triggers_deterministic_fallback() {
        let backend = MockPromptBackend::new(vec![
            "not valid json",
            "still not valid json",
            "still not valid json again",
        ]);
        let planned = plan_tool_calls_with_model_assist(
            &backend,
            &auth_user("user"),
            "What is the weather in Dublin right now?",
            &[],
        )
        .await;
        assert_eq!(planned.mode, AssistantPlannerMode::DeterministicFallback);
        assert_eq!(planned.calls.len(), 1);
        assert_eq!(planned.calls[0].tool, AssistantToolName::WeatherGetCurrent);
        assert_eq!(planned.debug.execution.repair_attempts, 2);
        assert_eq!(
            planned.debug.fallback_reason.as_deref(),
            Some("repair_exhausted")
        );
        assert_eq!(
            planned
                .debug
                .execution
                .fallback_reason
                .map(|reason| reason.as_str()),
            Some("repair_exhausted")
        );
    }

    #[tokio::test]
    async fn planner_model_assist_skips_tools_for_casual_chat() {
        let backend = MockPromptBackend::new(vec![
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"funny joke\"}}]}",
        ]);
        let planned = plan_tool_calls_with_model_assist(
            &backend,
            &auth_user("user"),
            "Tell me a funny joke",
            &[],
        )
        .await;
        assert_eq!(planned.mode, AssistantPlannerMode::DeterministicFallback);
        assert!(planned.calls.is_empty());
    }

    #[test]
    fn planner_extracts_calendar_today_window() {
        let tools = plan_tool_calls("What events do I have today?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, .. } => assert_eq!(label, "today"),
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn planner_extracts_relative_weekday_calendar_window() {
        let tools = plan_tool_calls("What events do I have next Tuesday?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow {
                from_date,
                to_date,
                label,
                ..
            } => {
                assert_eq!(from_date, to_date);
                assert!(label.contains("next Tuesday"));
            }
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn planner_routes_current_datetime_queries() {
        let tools = plan_tool_calls("What date is next Tuesday?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetCurrentDateTime);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_fetch_time_queries() {
        let tools = plan_tool_calls("Fetch the time on the Rustyfin host");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetCurrentDateTime);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_natural_language_current_datetime_queries() {
        let tools = plan_tool_calls("What day next Tuesday would be?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetCurrentDateTime);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_datetime_follow_up_corrections() {
        let history = grounded_history(&["system_get_current_datetime"]);
        let tools = plan_tool_calls_with_history("Surely it would be the 7th", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetCurrentDateTime);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn deterministic_current_datetime_reply_uses_grounded_relative_date() {
        let reply = deterministic_current_datetime_reply(
            "What day next Tuesday would be?",
            &[],
            &[grounded_datetime_block("2026-04-02", "Thursday")],
        )
        .expect("expected deterministic reply");
        assert!(reply.contains("Thursday, April 2, 2026"));
        assert!(reply.contains("next Tuesday"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
    }

    #[test]
    fn deterministic_current_datetime_reply_uses_recent_datetime_question_for_corrections() {
        let history = vec![AssistantHistoryMessage {
            role: "user".to_string(),
            content: "What day next Tuesday would be?".to_string(),
            grounding_tools: Vec::new(),
            follow_up_contexts: Vec::new(),
            grounding_chunks: Vec::new(),
        }];
        let reply = deterministic_current_datetime_reply(
            "Surely it would be the 7th",
            &history,
            &[grounded_datetime_block("2026-04-02", "Thursday")],
        )
        .expect("expected deterministic reply");
        assert!(reply.contains("next Tuesday"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
    }

    #[test]
    fn deterministic_tool_inventory_reply_lists_user_visible_tools() {
        let reply = deterministic_tool_inventory_reply(
            &auth_user("user"),
            "Give me a list of all the functions you have access to in this environment",
        )
        .expect("expected deterministic tool inventory reply");
        assert!(reply.contains("network_get_topology_summary"));
        assert!(reply.contains("system_get_current_datetime"));
        assert!(reply.contains("calendar_create_event"));
        assert!(!reply.contains("system_get_service_health"));
    }

    #[test]
    fn deterministic_tool_inventory_reply_lists_admin_tools_for_admins() {
        let reply = deterministic_tool_inventory_reply(
            &auth_user("admin"),
            "What tools can you use in this environment?",
        )
        .expect("expected deterministic tool inventory reply");
        assert!(reply.contains("system_get_service_health"));
        assert!(reply.contains("system_get_host_runtime_summary"));
    }

    #[test]
    fn planner_routes_family_relationship_queries_to_dictionary() {
        let tools = plan_tool_calls("When is my mother's birthday?");
        let tool = tools
            .iter()
            .find(|call| call.tool == AssistantToolName::DictionaryResolveRelationshipReference)
            .expect("expected dictionary relationship reference tool");
        match &tool.input {
            AssistantToolInput::DictionaryResolveRelationshipReference {
                reference,
                workspace_id,
            } => {
                assert_eq!(reference, "my mother");
                assert_eq!(workspace_id, &None);
            }
            _ => panic!("expected dictionary relationship reference"),
        }
    }

    #[test]
    fn planner_routes_work_relationship_queries_to_dictionary() {
        let tools = plan_tool_calls("Who are my co-workers?");
        let tool = tools
            .iter()
            .find(|call| call.tool == AssistantToolName::DictionaryResolveRelationshipReference)
            .expect("expected dictionary relationship reference tool");
        match &tool.input {
            AssistantToolInput::DictionaryResolveRelationshipReference {
                reference,
                workspace_id,
            } => {
                assert_eq!(reference, "my co-workers");
                assert_eq!(workspace_id, &None);
            }
            _ => panic!("expected dictionary relationship reference"),
        }
    }

    #[test]
    fn planner_routes_dictionary_workspace_listing_queries() {
        let tools = plan_tool_calls("Show me my dictionary workspaces");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DictionaryListVisibleWorkspaces
        );
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::DictionaryListVisibleWorkspaces
        ));
    }

    #[test]
    fn planner_routes_dictionary_workspace_browse_queries() {
        let tools = plan_tool_calls("Who is in my family dictionary?");
        let tool = tools
            .iter()
            .find(|call| call.tool == AssistantToolName::DictionaryBrowseWorkspacePeople)
            .expect("expected dictionary workspace browse tool");
        match &tool.input {
            AssistantToolInput::DictionaryBrowseWorkspacePeople {
                workspace_id,
                query,
                limit,
            } => {
                assert_eq!(workspace_id, "family");
                assert_eq!(query, &None);
                assert_eq!(limit, &Some(12));
            }
            _ => panic!("expected dictionary workspace browse input"),
        }
    }

    #[test]
    fn planner_routes_dictionary_find_in_workspace_queries() {
        let tools = plan_tool_calls("Find Rachel in my work dictionary");
        let tool = tools
            .iter()
            .find(|call| call.tool == AssistantToolName::DictionaryBrowseWorkspacePeople)
            .expect("expected dictionary workspace browse tool");
        match &tool.input {
            AssistantToolInput::DictionaryBrowseWorkspacePeople {
                workspace_id,
                query,
                limit,
            } => {
                assert_eq!(workspace_id, "work");
                assert_eq!(query.as_deref(), Some("Rachel"));
                assert_eq!(limit, &Some(12));
            }
            _ => panic!("expected dictionary workspace browse input"),
        }
    }

    #[test]
    fn planner_extracts_next_week_birthday_window() {
        let tools = plan_tool_calls("Which birthdays are next week?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, .. } => assert_eq!(label, "next week"),
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn planner_uses_year_window_for_named_birthday_lookup() {
        let tools = plan_tool_calls("When is Rachel's birthday?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, query, .. } => {
                assert_eq!(label, "the next 366 days");
                assert_eq!(query.as_deref(), Some("Rachel"));
            }
            _ => panic!("expected birthday calendar window"),
        }
    }

    #[test]
    fn planner_drops_generic_next_birthday_query_filters() {
        let tools = plan_tool_calls("What's the next birthday in my calendar?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, query, .. } => {
                assert_eq!(label, "the next 366 days");
                assert_eq!(query, &None);
            }
            _ => panic!("expected birthday calendar window"),
        }
    }

    #[test]
    fn birthday_query_normalizes_my_next_birthday_event_to_self() {
        assert_eq!(
            extract_birthday_query("When is my next birthday event?").as_deref(),
            Some("my")
        );
    }

    #[test]
    fn planner_extracts_numbered_day_window() {
        let tools = plan_tool_calls("What events are in the next 10 days?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, .. } => {
                assert_eq!(label, "the next 10 days")
            }
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn clarification_triggers_for_ambiguous_server_question() {
        let clarification = clarification_for_message("Is the server online?");
        assert!(clarification.is_some());
    }

    #[test]
    fn planner_extracts_room_mode_filter() {
        let tools = plan_tool_calls("Are any YouTube rooms active right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::RoomsListActive);
        match &tools[0].input {
            AssistantToolInput::RoomsFilter { room_mode, query } => {
                assert_eq!(room_mode.as_deref(), Some("youtube"));
                assert_eq!(query, &None);
            }
            _ => panic!("expected room filter"),
        }
    }

    #[test]
    fn planner_extracts_online_server_filter() {
        let tools = plan_tool_calls("What Minecraft servers are online?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::ServersListMinecraftStatus);
        match &tools[0].input {
            AssistantToolInput::ServerFilter {
                availability,
                query,
            } => {
                assert_eq!(availability.as_deref(), Some("online"));
                assert_eq!(query, &None);
            }
            _ => panic!("expected server filter"),
        }
    }

    #[test]
    fn planner_extracts_named_server_query() {
        let tools = plan_tool_calls("Is the Minecraft server called Survival online?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::ServersListMinecraftStatus);
        match &tools[0].input {
            AssistantToolInput::ServerFilter {
                availability,
                query,
            } => {
                assert_eq!(availability.as_deref(), Some("online"));
                assert_eq!(query.as_deref(), Some("Survival"));
            }
            _ => panic!("expected server filter"),
        }
    }

    #[test]
    fn status_label_describes_server_filter() {
        let tools = plan_tool_calls("What Minecraft servers are online?");
        assert_eq!(
            status_label_for_tool_call(&tools[0]),
            "Checking Minecraft servers that are online"
        );
    }

    #[test]
    fn planner_uses_calendar_follow_up_history() {
        let history = grounded_history(&["calendar_list_events"]);
        let tools = plan_tool_calls_with_history("What about next week?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, .. } => assert_eq!(label, "next week"),
            _ => panic!("expected calendar window"),
        }
    }

    #[test]
    fn planner_uses_birthday_follow_up_history_for_named_person() {
        let history = grounded_history(&["calendar_upcoming_birthdays"]);
        let tools = plan_tool_calls_with_history("What about Rachel?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { query, .. } => {
                assert_eq!(query.as_deref(), Some("Rachel"));
            }
            _ => panic!("expected birthday calendar window"),
        }
    }

    #[test]
    fn planner_uses_server_follow_up_history() {
        let history = grounded_history(&["servers_list_minecraft_status"]);
        let tools = plan_tool_calls_with_history("Which ones are healthy?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::ServersListMinecraftStatus);
        match &tools[0].input {
            AssistantToolInput::ServerFilter {
                availability,
                query,
            } => {
                assert_eq!(availability.as_deref(), Some("healthy"));
                assert_eq!(query, &None);
            }
            _ => panic!("expected server filter"),
        }
    }

    #[test]
    fn planner_uses_room_follow_up_history() {
        let history = grounded_history(&["rooms_list_active"]);
        let tools = plan_tool_calls_with_history("What about YouTube ones?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::RoomsListActive);
        match &tools[0].input {
            AssistantToolInput::RoomsFilter { room_mode, query } => {
                assert_eq!(room_mode.as_deref(), Some("youtube"));
                assert_eq!(query, &None);
            }
            _ => panic!("expected room filter"),
        }
    }

    #[test]
    fn planner_uses_library_follow_up_history() {
        let history = grounded_history(&["library_search_titles"]);
        let tools = plan_tool_calls_with_history("What about Dune?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrarySearchTitles);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Dune"),
            _ => panic!("expected library search"),
        }
    }

    #[test]
    fn planner_uses_library_summary_follow_up_history() {
        let history = history_with_follow_up_context(
            "libraries_get_library_summary",
            &["Movies"],
            AssistantFollowUpInputHint::default(),
        );
        let tools = plan_tool_calls_with_history("Tell me more about it.", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesGetLibrarySummary);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Movies"),
            _ => panic!("expected library summary"),
        }
    }

    #[test]
    fn planner_uses_downloads_follow_up_history() {
        let history = grounded_history(&["downloads_list_available_artifacts"]);
        let tools = plan_tool_calls_with_history("What about the planned ones?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsListAvailableArtifacts
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter {
                query,
                availability,
            } => {
                assert_eq!(query, &None);
                assert_eq!(availability.as_deref(), Some("planned"));
            }
            _ => panic!("expected downloads filter"),
        }
    }

    #[test]
    fn planner_uses_network_follow_up_history() {
        let history = grounded_history(&["network_get_topology_summary"]);
        let tools = plan_tool_calls_with_history("What about the IP addresses?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetTopologySummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_uses_weather_follow_up_history() {
        let history = history_with_follow_up_context(
            "weather_get_forecast",
            &[],
            AssistantFollowUpInputHint {
                weather_location: Some("Dublin".to_string()),
                weather_days: Some(3),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What about tomorrow?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Dublin");
                assert_eq!(*forecast_days, Some(2));
            }
            _ => panic!("expected weather input"),
        }
    }

    #[test]
    fn planner_uses_weather_follow_up_history_for_bare_location() {
        let history = history_with_follow_up_context(
            "weather_get_forecast",
            &[],
            AssistantFollowUpInputHint {
                weather_location: Some("Dublin".to_string()),
                weather_days: Some(7),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("Campile, County Wexford, Ireland", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Campile, County Wexford, Ireland");
                assert_eq!(*forecast_days, Some(7));
            }
            _ => panic!("expected weather forecast input"),
        }
    }

    #[test]
    fn planner_uses_weather_follow_up_history_for_location_with_in_country_phrase() {
        let history = history_with_follow_up_context(
            "weather_get_current",
            &[],
            AssistantFollowUpInputHint {
                weather_location: Some("Campile, County Wexford, Ireland".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("campile in ireland", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetCurrent);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "campile in ireland");
                assert_eq!(*forecast_days, None);
            }
            _ => panic!("expected weather current input"),
        }
    }

    #[test]
    fn planner_strips_leading_for_from_weather_follow_up_location() {
        let history = history_with_follow_up_context(
            "weather_get_current",
            &[],
            AssistantFollowUpInputHint {
                weather_location: Some("Campile, County Wexford, Ireland".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("for Campile, Ireland?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetCurrent);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Campile, Ireland");
                assert_eq!(*forecast_days, None);
            }
            _ => panic!("expected weather current input"),
        }
    }

    #[test]
    fn planner_merges_short_weather_follow_up_with_hint_location() {
        let history = history_with_follow_up_context(
            "weather_get_forecast",
            &[],
            AssistantFollowUpInputHint {
                weather_location: Some("Campile, County Wexford, Leinster, Ireland".to_string()),
                weather_days: Some(7),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("for Campile", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::WeatherGetForecast);
        match &tools[0].input {
            AssistantToolInput::Weather {
                location,
                forecast_days,
            } => {
                assert_eq!(location, "Campile, County Wexford, Leinster, Ireland");
                assert_eq!(*forecast_days, Some(7));
            }
            _ => panic!("expected weather forecast input"),
        }
    }

    #[test]
    fn planner_uses_host_runtime_follow_up_history() {
        let history = grounded_history(&["system_get_host_runtime_summary"]);
        let tools = plan_tool_calls_with_history("What about memory usage?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::SystemGetHostRuntimeSummary
        );
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_uses_ai_runtime_follow_up_history() {
        let history = grounded_history(&["system_get_ai_runtime_summary"]);
        let tools = plan_tool_calls_with_history("What about the backend?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetAiRuntimeSummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_vram_questions_to_ai_runtime_tool_follow_up() {
        let tools = plan_tool_calls("How much VRAM are the GPUs using right now?");
        let ai_runtime = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::SystemGetAiRuntimeSummary)
            .expect("expected AI runtime tool to be planned");
        assert!(matches!(ai_runtime.input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_next_event_queries_to_deterministic_tool() {
        let tools = plan_tool_calls("What's my next event?");
        let next_event = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::CalendarGetNextEvent)
            .expect("expected next event tool to be planned");
        assert!(matches!(next_event.input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_next_thing_coming_up_queries_to_deterministic_tool() {
        let tools = plan_tool_calls("What is the next thing coming up in my calendar?");
        let next_event = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::CalendarGetNextEvent)
            .expect("expected next event tool to be planned");
        assert!(matches!(next_event.input, AssistantToolInput::None));
    }

    #[test]
    fn supported_calendar_create_prompts_skip_server_refusal() {
        let refusal =
            unsupported_write_response_for_message("Add Rachel's birthday to my calendar");
        assert_eq!(refusal, None);
    }

    #[test]
    fn supported_calendar_delete_prompts_skip_server_refusal() {
        let refusal = unsupported_write_response_for_message(
            "Delete dentist appointment on 2026-06-09 from my calendar",
        );
        assert_eq!(refusal, None);
    }

    #[test]
    fn supported_document_create_prompts_skip_server_refusal() {
        let refusal = unsupported_write_response_for_message(
            "Create a markdown document summarizing the local Rustyfin IP and login URL",
        );
        assert_eq!(refusal, None);
    }

    #[test]
    fn unsafe_host_action_requests_return_refusal() {
        let refusal = unsafe_action_response_for_message("Delete System32 on my computer");
        assert!(refusal.is_some());
    }

    #[test]
    fn planner_does_not_overfire_server_follow_up_for_calendar_window() {
        let history = grounded_history(&["calendar_list_events", "servers_list_minecraft_status"]);
        let tools = plan_tool_calls_with_history("What about next week?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
    }

    #[test]
    fn planner_resolves_server_ordinal_follow_up() {
        let history = history_with_follow_up_context(
            "servers_list_minecraft_status",
            &["Alpha", "Beta", "Gamma"],
            AssistantFollowUpInputHint {
                server_availability: Some("online".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What about the second one?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::ServersGetMinecraftServerSummary
        );
        match &tools[0].input {
            AssistantToolInput::ServerFilter {
                query,
                availability,
            } => {
                assert_eq!(query.as_deref(), Some("Beta"));
                assert_eq!(availability.as_deref(), Some("online"));
            }
            _ => panic!("expected server filter"),
        }
    }

    #[test]
    fn planner_resolves_room_entity_follow_up() {
        let history = history_with_follow_up_context(
            "rooms_list_active",
            &["YouTube Party", "Screen Share"],
            AssistantFollowUpInputHint {
                room_mode: Some("youtube".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What about the first room?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::RoomsGetRoomSummary);
        match &tools[0].input {
            AssistantToolInput::RoomsFilter { room_mode, query } => {
                assert_eq!(room_mode.as_deref(), Some("youtube"));
                assert_eq!(query.as_deref(), Some("YouTube Party"));
            }
            _ => panic!("expected room filter"),
        }
    }

    #[test]
    fn planner_resolves_library_entity_follow_up_to_detail_tool() {
        let history = history_with_follow_up_context(
            "library_search_titles",
            &["Interstellar", "Dune", "Arrival"],
            AssistantFollowUpInputHint {
                library_query: Some("science fiction".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What about the second one?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibraryGetItemSummary);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Dune"),
            _ => panic!("expected library detail query"),
        }
    }

    #[test]
    fn planner_detects_library_media_detail_query() {
        let tools = plan_tool_calls("Where is Interstellar stored and what artwork does it have?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibraryGetItemMediaDetails);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Interstellar"),
            _ => panic!("expected library media detail query"),
        }
    }

    #[test]
    fn planner_detects_storage_path_detail_query() {
        let tools = plan_tool_calls("How much space is on the AI model dir?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetStoragePathDetail);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "ai_model_dir"),
            _ => panic!("expected storage path detail query"),
        }
    }

    #[test]
    fn planner_detects_mount_detail_query() {
        let tools = plan_tool_calls("What filesystem is mounted on /srv/media?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetMountDetail);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "/srv/media"),
            _ => panic!("expected mount detail query"),
        }
    }

    #[test]
    fn planner_detects_port_conflicts_query() {
        let tools = plan_tool_calls("What ports in use?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetPortConflicts);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::SystemPortConflicts { .. }
        ));
    }

    #[test]
    fn planner_detects_port_conflict_detail_query() {
        let tools = plan_tool_calls("What process is using port 3008?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::SystemGetPortConflictDetail
        );
        match &tools[0].input {
            AssistantToolInput::SystemPortConflicts { query } => {
                assert_eq!(query.as_deref(), Some("3008"))
            }
            _ => panic!("expected port conflict detail query"),
        }
    }

    #[test]
    fn planner_routes_process_detail_query() {
        let tools = plan_tool_calls("Show me process detail for pid 1234");
        let process_detail = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::SystemGetProcessDetail)
            .expect("expected process detail tool to be planned");
        match &process_detail.input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "1234"),
            _ => panic!("expected process detail query"),
        }
    }

    #[test]
    fn planner_routes_listener_detail_query() {
        let tools = plan_tool_calls("What listener is using port 3008?");
        let listener_detail = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::SystemGetListenerDetail)
            .expect("expected listener detail tool to be planned");
        match &listener_detail.input {
            AssistantToolInput::SystemPortConflicts { query } => {
                assert_eq!(query.as_deref(), Some("3008"))
            }
            _ => panic!("expected listener detail query"),
        }
    }

    #[test]
    fn planner_routes_disk_usage_detail_query() {
        let tools = plan_tool_calls("How much disk space is on /srv/media?");
        let disk_usage_detail = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::SystemGetDiskUsageDetail)
            .expect("expected disk usage detail tool to be planned");
        match &disk_usage_detail.input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "/srv/media"),
            _ => panic!("expected disk usage detail query"),
        }
    }

    #[test]
    fn planner_detects_failed_units_query() {
        let tools = plan_tool_calls("Which systemd units are failed?");
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetFailedUnits);
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::SystemGetFailedUnits)
        );
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::SystemFailedUnits { .. }
        ));
    }

    #[test]
    fn planner_detects_failed_unit_detail_query() {
        let tools = plan_tool_calls("Show me details for the failed unit rustfin.service.");
        assert!(!tools.is_empty());
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetFailedUnitDetail);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::SystemFailedUnits { .. }
        ));
    }

    #[test]
    fn planner_detects_memory_fact_query() {
        let tools = plan_tool_calls("What do you know about Rachel?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemorySearchFacts);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "Rachel"),
            _ => panic!("expected memory fact query"),
        }
    }

    #[test]
    fn planner_detects_memory_entity_query() {
        let tools = plan_tool_calls("Who is Rachel in my family?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemorySearchEntities);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => {
                assert_eq!(query, "Rachel in my family")
            }
            _ => panic!("expected memory entity query"),
        }
    }

    #[test]
    fn planner_detects_memory_recent_entities_query() {
        let tools = plan_tool_calls("Who do you remember?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryListRecentEntities);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_detects_memory_recent_changes_query() {
        let tools = plan_tool_calls("What's new in my memory?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryListRecentChanges);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_detects_memory_conflicting_facts_query() {
        let tools = plan_tool_calls("What conflicting facts do you have about Rachel?");
        let conflict_tool = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::MemoryListConflictingFacts)
            .expect("expected memory conflict tool");
        match &conflict_tool.input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "Rachel"),
            _ => panic!("expected memory conflict query"),
        }
    }

    #[test]
    fn planner_detects_memory_provenance_query() {
        let tools = plan_tool_calls("Where did you learn about Rachel?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryGetEntityProvenance);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "Rachel"),
            _ => panic!("expected memory provenance query"),
        }
    }

    #[test]
    fn planner_detects_memory_relation_query() {
        let tools = plan_tool_calls("Who is Rachel related to?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryGetEntityRelations);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "Rachel"),
            _ => panic!("expected memory relation query"),
        }
    }

    #[test]
    fn planner_detects_memory_exact_entity_query() {
        let tools = plan_tool_calls("Who is the exact Rachel in my family?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryFindExactEntity);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => {
                assert_eq!(query, "Rachel in my family")
            }
            _ => panic!("expected memory exact entity query"),
        }
    }

    #[test]
    fn planner_detects_memory_person_summary_query() {
        let tools = plan_tool_calls("Give me a person summary for Rachel.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::MemoryGetPersonSummary);
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => assert_eq!(query, "Rachel"),
            _ => panic!("expected memory person summary query"),
        }
    }

    #[test]
    fn planner_detects_memory_relation_path_query() {
        let tools = plan_tool_calls("How is Rachel related to Bob?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::MemoryGetEntityRelationPath
        );
        match &tools[0].input {
            AssistantToolInput::SystemService { query } => {
                assert_eq!(query, "Rachel || Bob")
            }
            _ => panic!("expected memory relation path query"),
        }
    }

    #[test]
    fn planner_detects_calendar_next_free_day_query() {
        let tools = plan_tool_calls("When is the next free day?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarGetNextFreeDay);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::CalendarWindow { .. }
        ));
    }

    #[test]
    fn planner_detects_calendar_overlapping_events_query() {
        let tools = plan_tool_calls("Are there any overlapping events next week?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::CalendarListOverlappingEvents
        );
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::CalendarWindow { .. }
        ));
    }

    #[test]
    fn planner_resolves_library_media_follow_up_to_media_tool() {
        let history = history_with_follow_up_context(
            "library_get_item_summary",
            &["Interstellar"],
            AssistantFollowUpInputHint {
                library_query: Some("science fiction".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What is its file path?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibraryGetItemMediaDetails);
        match &tools[0].input {
            AssistantToolInput::LibrarySearch { query } => assert_eq!(query, "Interstellar"),
            _ => panic!("expected library media detail query"),
        }
    }

    #[test]
    fn planner_resolves_download_entity_follow_up() {
        let history = history_with_follow_up_context(
            "downloads_list_available_artifacts",
            &[
                "RustyVault Browser Extension",
                "Rustyfin App",
                "Additional Companion Tools",
            ],
            AssistantFollowUpInputHint {
                downloads_availability: Some("planned".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What about the second one?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsGetArtifactDetails
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter {
                query,
                availability,
            } => {
                assert_eq!(query.as_deref(), Some("Rustyfin App"));
                assert_eq!(availability.as_deref(), Some("planned"));
            }
            _ => panic!("expected downloads filter"),
        }
    }

    #[test]
    fn planner_detects_download_artifact_checksum_query() {
        let tools = plan_tool_calls("What is the checksum for the Rustyfin App package?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsGetArtifactChecksum
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter { query, .. } => {
                assert_eq!(query.as_deref(), Some("Rustyfin"));
            }
            _ => panic!("expected download checksum filter"),
        }
    }

    #[test]
    fn planner_detects_download_artifact_install_steps_query() {
        let tools = plan_tool_calls("How do I install the Rustyfin App?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsGetArtifactInstallSteps
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter { query, .. } => {
                assert_eq!(query.as_deref(), Some("Rustyfin"));
            }
            _ => panic!("expected download install steps filter"),
        }
    }

    #[test]
    fn planner_detects_download_artifact_compatibility_query() {
        let tools = plan_tool_calls("Is the Rustyfin App compatible with Linux?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::DownloadsGetArtifactCompatibility
        );
        match &tools[0].input {
            AssistantToolInput::DownloadsFilter { query, .. } => {
                assert_eq!(query.as_deref(), Some("Rustyfin"));
            }
            _ => panic!("expected download compatibility filter"),
        }
    }

    #[test]
    fn planner_detects_library_duplicate_titles_query() {
        let tools = plan_tool_calls("Do I have duplicate titles in my libraries?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::LibrariesFindDuplicateTitles
        );
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_detects_library_missing_metadata_query() {
        let tools = plan_tool_calls("What library items are missing metadata?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::LibrariesListMissingMetadata
        );
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_calendar_detail_queries() {
        let tools = plan_tool_calls("Tell me more about the \"Team Meeting\" event.");
        let event_details = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::CalendarGetEventDetails)
            .expect("expected calendar detail tool to be planned");
        match &event_details.input {
            AssistantToolInput::CalendarWindow { query, .. } => {
                assert_eq!(query.as_deref(), Some("Team Meeting"));
            }
            _ => panic!("expected calendar detail input"),
        }
    }

    #[test]
    fn planner_uses_single_event_detail_follow_up_history() {
        let history = history_with_follow_up_context(
            "calendar_get_next_event",
            &["Iwans birthday (2026-06-09)"],
            AssistantFollowUpInputHint {
                calendar_label: Some("your next calendar event".to_string()),
                calendar_from_date: Some("2026-06-09".to_string()),
                calendar_to_date: Some("2026-06-09".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("what is its description?", &history);
        let event_details = tools
            .iter()
            .find(|tool| tool.tool == AssistantToolName::CalendarGetEventDetails)
            .expect("expected calendar detail tool to be planned");
        match &event_details.input {
            AssistantToolInput::CalendarWindow { query, .. } => {
                assert_eq!(query.as_deref(), Some("Iwans birthday (2026-06-09)"));
            }
            _ => panic!("expected calendar detail input"),
        }
    }

    #[test]
    fn planner_routes_channel_activity_queries() {
        let tools = plan_tool_calls("Any unread activity in general chat?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::ChannelsListUnreadActivity);
        match &tools[0].input {
            AssistantToolInput::ChannelsFilter { query } => {
                assert_eq!(query.as_deref(), Some("general chat"));
            }
            _ => panic!("expected channel filter"),
        }
    }

    #[test]
    fn planner_routes_transcript_summary_queries() {
        let tools = plan_tool_calls("What was the call in general voice about?");
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::ChannelsGetTranscriptSummary
        );
        match &tools[0].input {
            AssistantToolInput::ChannelsFilter { query } => {
                assert_eq!(query.as_deref(), Some("general voice"));
            }
            _ => panic!("expected transcript channel filter"),
        }
    }

    #[test]
    fn planner_uses_transcript_follow_up_history() {
        let history = history_with_follow_up_context(
            "channels_get_transcript_summary",
            &["General Voice"],
            AssistantFollowUpInputHint {
                channels_query: Some("General Voice".to_string()),
                ..AssistantFollowUpInputHint::default()
            },
        );
        let tools = plan_tool_calls_with_history("What was that call about again?", &history);
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].tool,
            AssistantToolName::ChannelsGetTranscriptSummary
        );
        match &tools[0].input {
            AssistantToolInput::ChannelsFilter { query } => {
                assert_eq!(query.as_deref(), Some("General Voice"));
            }
            _ => panic!("expected transcript follow-up filter"),
        }
    }

    #[test]
    fn planner_routes_recently_added_library_queries() {
        let tools = plan_tool_calls("What was recently added to my library?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesGetRecentlyAdded);
        assert!(matches!(
            tools[0].input,
            AssistantToolInput::LibraryRecent { .. }
        ));
    }

    #[test]
    fn planner_routes_joinable_rooms_queries() {
        let tools = plan_tool_calls("What rooms can I join right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::RoomsListJoinable);
    }

    #[test]
    fn planner_routes_admin_ops_queries() {
        let tools = plan_tool_calls("What services are down right now?");
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::SystemGetServiceHealth)
        );

        let tools = plan_tool_calls("Summarize recent errors.");
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::SystemGetRecentErrors)
        );

        let tools = plan_tool_calls("How much free space is left on disk?");
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == AssistantToolName::SystemGetStorageSummary)
        );
    }
}
