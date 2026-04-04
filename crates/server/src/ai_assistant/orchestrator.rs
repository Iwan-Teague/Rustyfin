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
use crate::auth::AuthUser;
use crate::state::AppState;

const MAX_TOOL_CALLS_PER_TURN: usize = 3;
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
            availability: args
                .availability
                .clone()
                .or_else(|| legacy.availability.clone()),
            room_mode: args.room_mode.clone().or_else(|| legacy.room_mode.clone()),
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
    availability: Option<String>,
    #[serde(default)]
    room_mode: Option<String>,
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
    if is_tool_inventory_query(&message.to_ascii_lowercase()) {
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
                AssistantToolName::WeatherGetCurrent
                    | AssistantToolName::WeatherGetForecast
                    | AssistantToolName::WeatherGetHistory
                    | AssistantToolName::SystemGetCurrentDateTime
                    | AssistantToolName::SystemGetAiRuntimeSummary
                    | AssistantToolName::NetworkGetTopologySummary
                    | AssistantToolName::CalendarGetNextEvent
                    | AssistantToolName::CalendarUpcomingBirthdays
            )
        })
}

pub async fn prepare_assistant_turn(
    state: &AppState,
    user: &AuthUser,
    request: AssistantChatRequest,
) -> PreparedAssistantTurn {
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
                "You are the Rustyfin assistant tool planner. Choose zero to three grounded read-only tools. \
Return JSON only with no markdown, no prose, and no code fences.\n\
Schema:\n\
{{\"mode\":\"tool_plan\",\"tools\":[{{\"tool\":\"tool_name\",\"args\":{{\"query\":\"optional\",\"url\":\"optional\",\"availability\":\"optional\",\"room_mode\":\"optional\"}}}}]}}\n\
or\n\
{{\"mode\":\"none\",\"tools\":[]}}\n\
Rules:\n\
- Never use a tool not listed below.\n\
- Never exceed {MAX_TOOL_CALLS_PER_TURN} tools.\n\
- Use detail tools only when the user is asking about one specific room, one specific server, or one specific library item.\n\
- Use libraries_list_accessible for generic library access questions.\n\
- Use library_search_titles for searching by title.\n\
- Use libraries_get_recently_added for recently added or newest library items.\n\
- Use calendar_upcoming_birthdays only for birthday requests, including named questions like \"When is Rachel's birthday?\".\n\
- Use calendar_get_next_event when the user asks for the next or nearest upcoming calendar event.\n\
- Use calendar_get_event_details when the user wants more detail about one specific calendar event.\n\
- Use channels_list_unread_activity for recent visible channel activity; exact unread counts are not available.\n\
- Use channels_get_transcript_summary when the user asks what a transcribed voice call was about or wants a transcript-based call summary.\n\
- Use network_get_topology_summary for Rustyfin network, interface, IP address, hostname, remote-access, proxy, or topology questions.\n\
- Use weather_get_current for current weather, temperature, wind, or conditions right now.\n\
- Use weather_get_forecast for forecast, tomorrow, weekend, this week, next few days, rain chance, or weather planning questions.\n\
- Use weather_get_history for recent past-weather questions such as yesterday, last night, or a specific earlier date.\n\
- Use rooms_list_joinable for invites or rooms the user can join now.\n\
- Use system_get_current_datetime for current date/time questions or when the user asks what calendar date a relative day like next Tuesday lands on.\n\
- Use system_get_ai_runtime_summary for current AI model, backend, role-routing, queue, or warm-pool questions.\n\
- Use system_get_host_runtime_summary only for host/runtime resource questions.\n\
- Use system_get_backup_summary for backup or restore capability questions.\n\
- Use system_get_service_health for internal service or agent health questions.\n\
- Use system_get_transcode_summary for transcoding, ffmpeg, hardware acceleration, or transcode-failure questions.\n\
- Use system_get_storage_summary for storage, disk, cache, model directory, or free-space questions.\n\
- Use system_get_recent_errors for recent failures, problem summaries, or error overviews.\n\
- Use web_fetch_public_page_summary only for explicit public URLs.\n\
- Use web_search_public_web only for current public web information not already covered by a Rustyfin tool or curated public-weather tools.\n\
- If the request is unsupported, casual chat, or a write action, return mode none.\n\
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
        AssistantToolName::CalendarCreateBirthday => {
            " Args: required person/date/year/scope; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::CalendarDeleteEvent => {
            " Args: required event id/title/date metadata; explicit user confirmation is required before the backend will execute it."
        }
        AssistantToolName::DocumentCreateDownload => {
            " Args: required title/file name/format/content request; explicit user confirmation is required before the backend will execute it."
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
        AssistantToolName::NetworkGetTopologySummary => " Args: none.",
        AssistantToolName::LibrarySearchTitles => " Args: required query.",
        AssistantToolName::LibraryGetItemSummary => " Args: required query.",
        AssistantToolName::LibrariesGetRecentlyAdded => " Args: optional query.",
        AssistantToolName::WeatherGetCurrent => " Args: required location.",
        AssistantToolName::WeatherGetForecast => {
            " Args: required location; the backend derives a short forecast window from the message."
        }
        AssistantToolName::WeatherGetHistory => {
            " Args: required location; the backend derives the recent history date window from the message."
        }
        AssistantToolName::WebSearchPublicWeb => " Args: required query.",
        AssistantToolName::WebFetchPublicPageSummary => " Args: required url.",
        AssistantToolName::RoomsListActive => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsListJoinable => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsGetRoomSummary => " Args: required query, optional room_mode.",
        AssistantToolName::SystemGetCurrentDateTime => " Args: none.",
        AssistantToolName::SystemGetAiRuntimeSummary => " Args: none.",
        AssistantToolName::ServersListMinecraftStatus => {
            " Args: optional query, optional availability."
        }
        AssistantToolName::ServersGetMinecraftServerSummary => {
            " Args: required query, optional availability."
        }
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => " Args: none.",
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
        AssistantToolName::WebSearchPublicWeb | AssistantToolName::WebFetchPublicPageSummary
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
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::NetworkGetTopologySummary
        | AssistantToolName::CalendarGetNextEvent
        | AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetAiRuntimeSummary
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => Ok(AssistantToolInput::None),
        AssistantToolName::CalendarListEvents => Ok(extract_calendar_window(message, 7, None)),
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
        AssistantToolName::LibrariesGetRecentlyAdded => Ok(AssistantToolInput::LibraryRecent {
            query: normalize_optional_query(args.query.clone())
                .or_else(|| extract_recent_library_query(message)),
        }),
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory => normalize_optional_query(args.query.clone())
            .or_else(|| extract_weather_location(message))
            .and_then(|location| weather_tool_input_for_location(message, location))
            .ok_or_else(|| {
                planner_issue(
                    "missing_required_argument",
                    "weather tools require a location",
                    Some("args.query"),
                )
            }),
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
        }),
        AssistantToolName::WebFetchPublicPageSummary => Ok(AssistantToolInput::WebFetch {
            url: validated_public_web_url(
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
            )?,
        }),
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
        | AssistantToolName::CalendarUpcomingBirthdays
        | AssistantToolName::CalendarGetEventDetails => Some("calendar"),
        AssistantToolName::LibrarySearchTitles
        | AssistantToolName::LibraryGetItemSummary
        | AssistantToolName::LibrariesGetRecentlyAdded
        | AssistantToolName::LibrariesListAccessible => Some("library"),
        AssistantToolName::RoomsListActive
        | AssistantToolName::RoomsListJoinable
        | AssistantToolName::RoomsGetRoomSummary => Some("rooms"),
        AssistantToolName::ServersListMinecraftStatus
        | AssistantToolName::ServersGetMinecraftServerSummary => Some("servers"),
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory => Some("weather"),
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
Do not claim to have created, updated, deleted, or changed anything in Rustyfin unless a confirmed server-side write tool actually ran and the backend verified the result."
        .to_string()
}

pub fn immediate_response_for_message(message: &str) -> Option<String> {
    clarification_for_message(message)
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

pub fn plan_tool_calls(message: &str) -> Vec<PlannedToolCall> {
    plan_tool_calls_with_history(message, &[])
}

pub fn plan_tool_calls_with_history(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Vec<PlannedToolCall> {
    let lower = message.to_ascii_lowercase();
    if is_tool_inventory_query(&lower) {
        return Vec::new();
    }
    let mut planned = Vec::new();
    let mut seen = HashSet::new();

    if has_any(
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
    } else if is_next_calendar_event_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarGetNextEvent,
            AssistantToolInput::None,
        );
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

    if has_any(
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
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::DownloadsListAvailableArtifacts,
            extract_downloads_filter(message),
        );
    }

    if is_weather_query(&lower)
        && let Some(location) = extract_weather_location(message)
        && let Some((weather_tool, weather_input)) =
            weather_tool_call_for_location(message, location)
    {
        push_tool(&mut planned, &mut seen, weather_tool, weather_input);
    }

    if public_web_tools_enabled() {
        if let Some(url) = extract_public_web_url(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::WebFetchPublicPageSummary,
                AssistantToolInput::WebFetch { url },
            );
        } else if let Some(query) = extract_public_web_search_query(message) {
            push_tool(
                &mut planned,
                &mut seen,
                AssistantToolName::WebSearchPublicWeb,
                AssistantToolInput::WebSearch { query },
            );
        }
    }

    if is_network_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::NetworkGetTopologySummary,
            AssistantToolInput::None,
        );
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

    if is_host_runtime_query(&lower) && !is_ai_runtime_query(&lower) {
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

    if is_service_health_query(&lower) {
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

    if is_storage_query(&lower) {
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

    if let Some(query) = extract_library_search_query(message) {
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
        (AssistantToolName::NetworkGetTopologySummary, _) => {
            "Checking network topology and interface state".to_string()
        }
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
            AssistantToolName::WeatherGetHistory,
            AssistantToolInput::WeatherHistory {
                location, label, ..
            },
        ) => format!("Checking recent weather history for {label} in \"{location}\""),
        (AssistantToolName::LibrariesListAccessible, _) => {
            "Checking accessible libraries".to_string()
        }
        (AssistantToolName::SystemGetCurrentDateTime, _) => {
            "Checking the Rustyfin host date and time".to_string()
        }
        (AssistantToolName::SystemGetAiRuntimeSummary, _) => {
            "Checking the Rustyfin AI runtime and loaded model".to_string()
        }
        (AssistantToolName::SystemGetHostRuntimeSummary, _) => {
            "Checking Rustyfin host runtime stats".to_string()
        }
        (AssistantToolName::LibrarySearchTitles, AssistantToolInput::LibrarySearch { query }) => {
            format!("Searching libraries for \"{query}\"")
        }
        (AssistantToolName::LibraryGetItemSummary, AssistantToolInput::LibrarySearch { query }) => {
            format!("Loading library item details for \"{query}\"")
        }
        (
            AssistantToolName::LibrariesGetRecentlyAdded,
            AssistantToolInput::LibraryRecent { query: Some(query) },
        ) => format!("Checking recently added library items matching \"{query}\""),
        (AssistantToolName::LibrariesGetRecentlyAdded, _) => {
            "Checking recently added library items".to_string()
        }
        (AssistantToolName::WebSearchPublicWeb, AssistantToolInput::WebSearch { query }) => {
            format!("Searching the public web for \"{query}\"")
        }
        (AssistantToolName::WebFetchPublicPageSummary, AssistantToolInput::WebFetch { url }) => {
            format!("Fetching public page {}", truncate_for_planner(url, 80))
        }
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
        (AssistantToolName::SystemGetTranscodeSummary, _) => {
            "Checking transcoding health and hardware acceleration".to_string()
        }
        (AssistantToolName::SystemGetStorageSummary, _) => {
            "Checking storage paths and free space".to_string()
        }
        (AssistantToolName::SystemGetRecentErrors, _) => {
            "Checking recent failures and errors".to_string()
        }
        _ => format!("Checking {}", call.tool.spec().summary.to_ascii_lowercase()),
    }
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
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
                } else if is_next_calendar_event_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarGetNextEvent,
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
            AssistantToolName::DownloadsListAvailableArtifacts => {
                if message_has_downloads_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::DownloadsListAvailableArtifacts,
                        extract_downloads_follow_up_filter(message),
                    );
                }
            }
            AssistantToolName::NetworkGetTopologySummary => {
                if message_has_network_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::NetworkGetTopologySummary,
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
            | AssistantToolName::LibrarySearchTitles
            | AssistantToolName::LibraryGetItemSummary
            | AssistantToolName::LibrariesGetRecentlyAdded => {
                if let Some(query) = extract_library_follow_up_query(message) {
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
            | AssistantToolName::WeatherGetHistory => {
                if let Some((tool, input)) = extract_weather_follow_up_call(message, history) {
                    push_tool(planned, seen, tool, input);
                }
            }
            AssistantToolName::WebSearchPublicWeb
            | AssistantToolName::WebFetchPublicPageSummary => {
                if let Some(url) = extract_public_web_url(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::WebFetchPublicPageSummary,
                        AssistantToolInput::WebFetch { url },
                    );
                } else if let Some(query) = extract_public_web_search_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::WebSearchPublicWeb,
                        AssistantToolInput::WebSearch { query },
                    );
                }
            }
            AssistantToolName::AccountGetProfileSummary => {}
            AssistantToolName::SystemGetAiRuntimeSummary => {
                if is_ai_runtime_query(&message.to_ascii_lowercase())
                    || message_has_ai_runtime_follow_up_hint(message)
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
                if message_has_host_runtime_follow_up_hint(message) {
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
            AssistantToolName::SystemGetServiceHealth => {
                if is_service_health_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetServiceHealth,
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
            AssistantToolName::SystemGetStorageSummary => {
                if is_storage_query(&message.to_ascii_lowercase()) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::SystemGetStorageSummary,
                        AssistantToolInput::None,
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
        "calendar_list_events" | "calendar_get_next_event" | "calendar_get_event_details" => {
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
        "library_search_titles" | "library_get_item_summary" | "libraries_get_recently_added" => {
            push_tool(
                planned,
                seen,
                AssistantToolName::LibraryGetItemSummary,
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
                AssistantToolName::DownloadsListAvailableArtifacts,
                AssistantToolInput::DownloadsFilter {
                    query: Some(entity.label.clone()),
                    availability: extract_downloads_availability(message)
                        .or_else(|| context.input_hint.downloads_availability.clone()),
                },
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
            push_tool(
                planned,
                seen,
                AssistantToolName::WebFetchPublicPageSummary,
                AssistantToolInput::WebFetch { url },
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

fn follow_up_context_matches_message(context: &AssistantFollowUpContext, message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    match context.tool.as_str() {
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details" => {
            message_has_calendar_follow_up_hint(message)
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
        "library_search_titles" | "library_get_item_summary" | "libraries_get_recently_added" => {
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
            )
        }
        "downloads_list_available_artifacts" => has_any(
            &lower,
            &[
                "download",
                "downloads",
                "extension",
                "browser extension",
                "app",
                "planned",
                "available",
                "unavailable",
                "companion",
            ],
        ),
        "weather_get_current" | "weather_get_forecast" | "weather_get_history" => {
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
        "system_get_current_datetime" => {
            is_current_datetime_query(&lower)
                || message_has_current_datetime_tool_follow_up_hint(message)
        }
        "system_get_ai_runtime_summary" => {
            is_ai_runtime_query(&lower) || message_has_ai_runtime_follow_up_hint(message)
        }
        "system_get_host_runtime_summary" => is_host_runtime_query(&lower),
        "system_get_backup_summary" => is_backup_query(&lower),
        "system_get_service_health" => is_service_health_query(&lower),
        "system_get_transcode_summary" => is_transcode_query(&lower),
        "system_get_storage_summary" => is_storage_query(&lower),
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
            "remote access",
            "trusted proxy",
            "trusted proxies",
            "proxy",
            "proxies",
            "lan",
        ],
    )
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
    extract_downloads_query(message).is_some()
        || extract_downloads_follow_up_query(message).is_some()
        || extract_downloads_availability(message).is_some()
        || has_any(
            &lower,
            &[
                "download",
                "downloads",
                "extension",
                "app",
                "planned",
                "available",
                "unavailable",
                "companion",
            ],
        )
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
    let detail_hint = has_any(
        &lower,
        &[
            "details",
            "detail",
            "tell me more",
            "tell me about",
            "more about",
            "what time is",
            "when is",
            "who created",
            "describe",
        ],
    );
    if !detail_hint && !has_any(&lower, &["calendar", "event", "events", "schedule"]) {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        return Some(quoted);
    }

    for marker in [
        "event called ",
        "event named ",
        "details for ",
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

    if let Some((date, label)) = extract_single_calendar_date(message, today) {
        if date < today {
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
        return Some((
            AssistantToolName::WeatherGetForecast,
            AssistantToolInput::Weather {
                location,
                forecast_days: Some(((date - today).num_days() + 1).clamp(1, 7) as u8),
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
    if is_weather_query(&lower)
        || lower.contains("today")
        || lower.contains("tomorrow")
        || lower.contains("yesterday")
        || lower.contains("week")
        || lower.contains("weekend")
    {
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
            "rain",
            "wind",
            "humidity",
            "hot",
            "cold",
            "sunny",
            "cloudy",
            "storm",
        ],
    )
}

fn recent_weather_hint(history: &[AssistantHistoryMessage]) -> Option<RecentWeatherHint> {
    for context in recent_follow_up_contexts(history) {
        let tool = match context.tool.as_str() {
            "weather_get_current" => AssistantToolName::WeatherGetCurrent,
            "weather_get_forecast" => AssistantToolName::WeatherGetForecast,
            "weather_get_history" => AssistantToolName::WeatherGetHistory,
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
            "sunny",
            "cloudy",
            "storm",
            "hot in ",
            "cold in ",
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
    if let Some((date, label)) = extract_single_calendar_date(message, today)
        && date < today
    {
        return Some((date, date, label));
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
            "remote access",
            "trusted proxy",
            "trusted proxies",
            "hostname",
            "host name",
            "lan ip",
            "local ip",
            "ip address",
            "ip addresses",
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
        AssistantToolName, PlannerAst, clarification_for_message,
        deterministic_current_datetime_reply, deterministic_tool_inventory_reply,
        extract_birthday_query, parse_planner_ast, plan_tool_calls, plan_tool_calls_with_history,
        plan_tool_calls_with_model_assist, status_label_for_tool_call,
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
    use std::sync::Mutex;

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
        assert_eq!(tools.len(), 3);
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
    fn planner_detects_network_query() {
        let tools = plan_tool_calls("What network interfaces are active right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetTopologySummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
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
    fn planner_keeps_library_access_question_as_library_listing() {
        let tools = plan_tool_calls("Do I have access to any libraries?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesListAccessible);
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
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"calendar_list_events\"},{\"tool\":\"library_search_titles\",\"args\":{\"query\":\"Dune\"}},{\"tool\":\"rooms_list_active\"},{\"tool\":\"servers_list_minecraft_status\"}]}",
        )
        .expect("expected parsed planner AST");
        let issues = validate_planner_ast(
            &ast,
            &auth_user("admin"),
            "What events are this week, do I have Dune, what rooms are active, and what servers are online?",
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
        assert_eq!(planned.mode, AssistantPlannerMode::ModelStructured);
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
    fn planner_routes_next_event_queries_to_deterministic_tool() {
        let tools = plan_tool_calls("What's my next event?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarGetNextEvent);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
    }

    #[test]
    fn planner_routes_next_thing_coming_up_queries_to_deterministic_tool() {
        let tools = plan_tool_calls("What is the next thing coming up in my calendar?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarGetNextEvent);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
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
            AssistantToolName::DownloadsListAvailableArtifacts
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
    fn planner_routes_calendar_detail_queries() {
        let tools = plan_tool_calls("Tell me more about the \"Team Meeting\" event.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarGetEventDetails);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { query, .. } => {
                assert_eq!(query.as_deref(), Some("Team Meeting"));
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
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetServiceHealth);

        let tools = plan_tool_calls("Summarize recent errors.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetRecentErrors);

        let tools = plan_tool_calls("How much free space is left on disk?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::SystemGetStorageSummary);
    }
}
