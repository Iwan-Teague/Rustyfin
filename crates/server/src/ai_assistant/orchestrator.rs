use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use futures::{StreamExt, future::join_all};
use rustfin_ai_agent::{ChatChunk, ChatMessage, LlamaEngine, SamplingParams};
use serde::Deserialize;

use super::context::AssistantContext;
use super::registry::AssistantToolName;
use super::tools::{execute_tool, source_from_block};
use super::types::{
    AssistantChatRequest, AssistantFollowUpContext, AssistantFollowUpEntity,
    AssistantHistoryMessage, AssistantPlannerMode, AssistantToolContextBlock, AssistantToolInput,
    PlannedToolCall, PlannedToolSet, PreparedAssistantTurn,
};
use super::web::public_web_tools_enabled;
use crate::auth::AuthUser;
use crate::state::AppState;

const MAX_TOOL_CALLS_PER_TURN: usize = 3;
const PLANNER_HISTORY_MESSAGE_LIMIT: usize = 6;

#[derive(Debug, Deserialize)]
struct ModelPlannerResponse {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    tools: Vec<ModelPlannerTool>,
}

#[derive(Debug, Deserialize)]
struct ModelPlannerTool {
    #[serde(alias = "name")]
    tool: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    availability: Option<String>,
    #[serde(default)]
    room_mode: Option<String>,
}

pub async fn plan_tool_calls_with_model_assist(
    engine: &LlamaEngine,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> PlannedToolSet {
    if extract_follow_up_entity_reference(message).is_some() {
        let calls = plan_tool_calls_with_history(message, history);
        if !calls.is_empty() {
            return PlannedToolSet {
                mode: AssistantPlannerMode::DeterministicEntityFollowUp,
                calls,
            };
        }
    }

    let deterministic = plan_tool_calls_with_history(message, history);
    let raw_response = run_model_planner(engine, user, message, history).await;
    let model_calls = raw_response
        .as_deref()
        .and_then(parse_model_planner_response)
        .map(|response| normalize_model_plan(&response, user, message))
        .unwrap_or_default();

    if !model_calls.is_empty() || (raw_response.is_some() && deterministic.is_empty()) {
        return PlannedToolSet {
            mode: AssistantPlannerMode::ModelStructured,
            calls: model_calls,
        };
    }

    PlannedToolSet {
        mode: AssistantPlannerMode::DeterministicFallback,
        calls: deterministic,
    }
}

pub async fn prepare_assistant_turn(
    state: &AppState,
    user: &AuthUser,
    request: AssistantChatRequest,
) -> PreparedAssistantTurn {
    if let Some(clarification) = immediate_response_for_message(&request.message) {
        return PreparedAssistantTurn {
            messages: Vec::new(),
            sources: Vec::new(),
            immediate_response: Some(clarification),
        };
    }

    let context = AssistantContext::new(user, uuid::Uuid::new_v4().to_string());
    let planned_tools = plan_tool_calls_with_history(&request.message, &request.history);

    let tool_results = join_all(planned_tools.iter().cloned().map(|call| {
        let context = context.clone();
        async move {
            let block = execute_tool(state, &context, &call).await;
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

    let messages = build_assistant_messages(request, &grounding_blocks);

    PreparedAssistantTurn {
        messages,
        sources: grounding_sources,
        immediate_response: None,
    }
}

async fn run_model_planner(
    engine: &LlamaEngine,
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Option<String> {
    let planner_messages = build_model_planner_messages(user, message, history);
    let planner_stream = engine.chat_stream(
        planner_messages,
        SamplingParams {
            temperature: 0.1,
            top_p: 0.9,
            top_k: 20,
            repeat_penalty: 1.05,
            max_tokens: 320,
        },
    );
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

fn build_model_planner_messages(
    user: &AuthUser,
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Vec<ChatMessage> {
    let allowed_tools = planner_tool_inventory(user);
    let recent_tools = recent_grounded_tools(history);
    let recent_history = planner_history_summary(history);
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
{{\"mode\":\"tool_plan\",\"tools\":[{{\"tool\":\"tool_name\",\"query\":\"optional\",\"url\":\"optional\",\"availability\":\"optional\",\"room_mode\":\"optional\"}}]}}\n\
or\n\
{{\"mode\":\"none\",\"tools\":[]}}\n\
Rules:\n\
- Never use a tool not listed below.\n\
- Never exceed {MAX_TOOL_CALLS_PER_TURN} tools.\n\
- Use detail tools only when the user is asking about one specific room, one specific server, or one specific library item.\n\
- Use libraries_list_accessible for generic library access questions.\n\
- Use library_search_titles for searching by title.\n\
- Use calendar_upcoming_birthdays only for birthday requests.\n\
- Use system_get_host_runtime_summary only for host/runtime resource questions.\n\
- Use web_fetch_public_page_summary only for explicit public URLs.\n\
- Use web_search_public_web only for current public web information not already covered by a Rustyfin tool.\n\
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
                "Current user role: {role_label}\nRecent grounded tools: {}\nRecent conversation:\n{}\nCurrent user message:\n{}",
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
        AssistantToolName::CalendarListEvents => {
            " Args: none; the backend derives the calendar time window from the message."
        }
        AssistantToolName::CalendarUpcomingBirthdays => {
            " Args: none; the backend derives the birthday time window from the message."
        }
        AssistantToolName::DownloadsListAvailableArtifacts => {
            " Args: optional query, optional availability."
        }
        AssistantToolName::LibrarySearchTitles => " Args: required query.",
        AssistantToolName::LibraryGetItemSummary => " Args: required query.",
        AssistantToolName::WebSearchPublicWeb => " Args: required query.",
        AssistantToolName::WebFetchPublicPageSummary => " Args: required url.",
        AssistantToolName::RoomsListActive => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsGetRoomSummary => " Args: required query, optional room_mode.",
        AssistantToolName::ServersListMinecraftStatus => {
            " Args: optional query, optional availability."
        }
        AssistantToolName::ServersGetMinecraftServerSummary => {
            " Args: required query, optional availability."
        }
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::SystemGetHostRuntimeSummary => " Args: none.",
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

fn parse_model_planner_response(raw: &str) -> Option<ModelPlannerResponse> {
    let cleaned = strip_markdown_code_fence(raw);
    if let Ok(response) = serde_json::from_str::<ModelPlannerResponse>(cleaned) {
        return Some(response);
    }
    let candidate = extract_json_object(cleaned)?;
    serde_json::from_str::<ModelPlannerResponse>(&candidate).ok()
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

fn normalize_model_plan(
    response: &ModelPlannerResponse,
    user: &AuthUser,
    message: &str,
) -> Vec<PlannedToolCall> {
    if response.mode.as_deref() == Some("none") {
        return Vec::new();
    }

    let mut planned = Vec::new();
    let mut seen = HashSet::new();
    for tool in response.tools.iter().take(MAX_TOOL_CALLS_PER_TURN) {
        let Some(parsed_tool) = AssistantToolName::from_str(&tool.tool.to_ascii_lowercase()) else {
            continue;
        };
        if !tool_visible_to_user(parsed_tool, user) {
            continue;
        }
        let Some(input) = normalize_model_tool_input(parsed_tool, tool, message) else {
            continue;
        };
        push_tool(&mut planned, &mut seen, parsed_tool, input);
    }

    planned
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

fn normalize_model_tool_input(
    tool: AssistantToolName,
    response: &ModelPlannerTool,
    message: &str,
) -> Option<AssistantToolInput> {
    match tool {
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::SystemGetHostRuntimeSummary => Some(AssistantToolInput::None),
        AssistantToolName::CalendarListEvents => Some(extract_calendar_window(message, 7)),
        AssistantToolName::CalendarUpcomingBirthdays => Some(extract_calendar_window(message, 30)),
        AssistantToolName::DownloadsListAvailableArtifacts => {
            Some(AssistantToolInput::DownloadsFilter {
                query: normalize_optional_query(response.query.clone())
                    .or_else(|| extract_downloads_follow_up_query(message))
                    .or_else(|| extract_downloads_query(message)),
                availability: normalize_downloads_availability(response.availability.as_deref())
                    .or_else(|| extract_downloads_availability(message)),
            })
        }
        AssistantToolName::LibrarySearchTitles => Some(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_library_search_query(message))
                .or_else(|| extract_library_follow_up_query(message))?,
        }),
        AssistantToolName::LibraryGetItemSummary => Some(AssistantToolInput::LibrarySearch {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_quoted_phrase(message))
                .or_else(|| extract_library_follow_up_query(message))
                .or_else(|| extract_library_search_query(message))?,
        }),
        AssistantToolName::WebSearchPublicWeb => Some(AssistantToolInput::WebSearch {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_public_web_search_query(message))?,
        }),
        AssistantToolName::WebFetchPublicPageSummary => Some(AssistantToolInput::WebFetch {
            url: normalize_optional_query(response.url.clone())
                .or_else(|| normalize_optional_query(response.query.clone()))
                .or_else(|| extract_public_web_url(message))?,
        }),
        AssistantToolName::RoomsListActive => Some(AssistantToolInput::RoomsFilter {
            room_mode: normalize_room_mode(response.room_mode.as_deref())
                .or_else(|| detect_room_mode(message)),
            query: normalize_optional_query(response.query.clone()),
        }),
        AssistantToolName::RoomsGetRoomSummary => Some(AssistantToolInput::RoomsFilter {
            room_mode: normalize_room_mode(response.room_mode.as_deref())
                .or_else(|| detect_room_mode(message)),
            query: Some(
                normalize_optional_query(response.query.clone())
                    .or_else(|| extract_room_query(message))
                    .or_else(|| extract_quoted_phrase(message))?,
            ),
        }),
        AssistantToolName::ServersListMinecraftStatus => Some(AssistantToolInput::ServerFilter {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_server_query(message)),
            availability: normalize_server_availability(response.availability.as_deref())
                .or_else(|| extract_server_availability(message)),
        }),
        AssistantToolName::ServersGetMinecraftServerSummary => {
            Some(AssistantToolInput::ServerFilter {
                query: Some(
                    normalize_optional_query(response.query.clone())
                        .or_else(|| extract_server_query(message))
                        .or_else(|| extract_quoted_phrase(message))
                        .filter(|query| !query.is_empty())?,
                ),
                availability: normalize_server_availability(response.availability.as_deref())
                    .or_else(|| extract_server_availability(message)),
            })
        }
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
Do not claim to have created, updated, deleted, or changed anything in Rustyfin because write actions are not enabled through this assistant yet."
        .to_string()
}

pub fn immediate_response_for_message(message: &str) -> Option<String> {
    clarification_for_message(message)
}

fn clarification_for_message(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if is_non_birthday_calendar_query(&lower) && !calendar_query_has_explicit_window(&lower) {
        return Some(
            "What time window should I check for your calendar? Try today, tomorrow, this week, next week, this month, or a specific date like 2026-03-22.".to_string(),
        );
    }
    if is_ambiguous_server_query(&lower, message) {
        return Some(
            "Which Minecraft server should I check? Say the server name, for example \"Survival\", or ask for all servers that are online or offline.".to_string(),
        );
    }
    None
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
        || extract_iso_date(message_lower).is_some()
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
        let calendar_input = extract_calendar_window(message, 30);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarUpcomingBirthdays,
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
        let calendar_input = extract_calendar_window(message, 7);
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::CalendarListEvents,
            calendar_input,
        );
    }

    if has_any(
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

    if is_host_runtime_query(&lower) {
        push_tool(
            &mut planned,
            &mut seen,
            AssistantToolName::SystemGetHostRuntimeSummary,
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
    grounding_blocks: &[AssistantToolContextBlock],
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: build_system_prompt(),
    }];

    if !grounding_blocks.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding for this turn:\n{}",
                serde_json::to_string(grounding_blocks).unwrap_or_else(|_| "[]".to_string())
            ),
        });
    }

    for history in request.history {
        messages.push(ChatMessage {
            role: history.role,
            content: history.content,
        });
    }

    messages.push(ChatMessage {
        role: "user".to_string(),
        content: request.message,
    });

    messages
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
            AssistantToolName::CalendarUpcomingBirthdays,
            AssistantToolInput::CalendarWindow { label, .. },
        ) => format!("Checking birthdays for {label}"),
        (AssistantToolName::CalendarUpcomingBirthdays, _) => {
            "Checking upcoming birthdays".to_string()
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
        (AssistantToolName::LibrariesListAccessible, _) => {
            "Checking accessible libraries".to_string()
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
            AssistantToolName::CalendarListEvents => {
                if message_has_calendar_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarListEvents,
                        extract_calendar_window(message, 7),
                    );
                }
            }
            AssistantToolName::CalendarUpcomingBirthdays => {
                if message_has_calendar_follow_up_hint(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::CalendarUpcomingBirthdays,
                        extract_calendar_window(message, 30),
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
            | AssistantToolName::LibraryGetItemSummary => {
                if let Some(query) = extract_library_follow_up_query(message) {
                    push_tool(
                        planned,
                        seen,
                        AssistantToolName::LibrarySearchTitles,
                        AssistantToolInput::LibrarySearch { query },
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
        "rooms_list_active" | "rooms_get_room_summary" => {
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
        "library_search_titles" | "library_get_item_summary" => {
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
        "servers_list_minecraft_status" | "servers_get_minecraft_server_summary" => {
            extract_server_availability(message).is_some()
                || extract_server_query(message).is_some()
                || has_any(&lower, &["server", "servers"])
        }
        "rooms_list_active" | "rooms_get_room_summary" => {
            detect_room_mode(message).is_some() || has_any(&lower, &["room", "rooms"])
        }
        "library_search_titles" | "library_get_item_summary" => has_any(
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
        ),
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
        "web_search_public_web" | "web_fetch_public_page_summary" => {
            extract_public_web_url(message).is_some()
                || extract_public_web_search_query(message).is_some()
        }
        "system_get_host_runtime_summary" => is_host_runtime_query(&lower),
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

fn message_has_room_follow_up_hint(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    detect_room_mode(message).is_some() || has_any(&lower, &["room", "rooms"])
}

fn message_has_server_follow_up_hint(message: &str) -> bool {
    extract_server_query(message).is_some() || extract_server_availability(message).is_some()
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
    if has_any(&lower, &["weather", "forecast", "temperature", "rain in "]) {
        return Some(message.trim().to_string());
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
        query: None,
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

fn extract_server_filter(message: &str) -> AssistantToolInput {
    AssistantToolInput::ServerFilter {
        query: extract_server_query(message),
        availability: extract_server_availability(message),
    }
}

fn is_host_runtime_query(message_lower: &str) -> bool {
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

fn extract_calendar_window(message: &str, fallback_days: i64) -> AssistantToolInput {
    let today = Utc::now().date_naive();
    let lower = message.to_ascii_lowercase();

    if let Some(date) = extract_iso_date(&lower) {
        return AssistantToolInput::CalendarWindow {
            from_date: date.format("%F").to_string(),
            to_date: date.format("%F").to_string(),
            label: format!("{} only", date.format("%F")),
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
    }
}

fn extract_iso_date(message_lower: &str) -> Option<NaiveDate> {
    for token in message_lower.split(|c: char| !(c.is_ascii_digit() || c == '-')) {
        if token.len() == 10 {
            if let Ok(date) = NaiveDate::parse_from_str(token, "%Y-%m-%d") {
                return Some(date);
            }
        }
    }
    None
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
        AssistantToolName, ModelPlannerResponse, clarification_for_message, normalize_model_plan,
        parse_model_planner_response, plan_tool_calls, plan_tool_calls_with_history,
        status_label_for_tool_call,
    };
    use crate::ai_assistant::types::{
        AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
        AssistantHistoryMessage, AssistantToolInput,
    };
    use crate::auth::AuthUser;

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
                    })
                    .collect(),
            }],
        }]
    }

    #[test]
    fn planner_detects_calendar_birthday_queries() {
        let tools = plan_tool_calls("Who has a birthday coming up soon?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
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
            AssistantToolInput::None => panic!("expected library search input"),
            AssistantToolInput::CalendarWindow { .. } => panic!("unexpected calendar window"),
            AssistantToolInput::DownloadsFilter { .. } => panic!("unexpected downloads filter"),
            AssistantToolInput::WebSearch { .. } => panic!("unexpected web search"),
            AssistantToolInput::WebFetch { .. } => panic!("unexpected web fetch"),
            AssistantToolInput::RoomsFilter { .. } => panic!("unexpected room filter"),
            AssistantToolInput::ServerFilter { .. } => panic!("unexpected server filter"),
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
    fn planner_keeps_library_access_question_as_library_listing() {
        let tools = plan_tool_calls("Do I have access to any libraries?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrariesListAccessible);
    }

    #[test]
    fn model_planner_parser_accepts_markdown_fenced_json() {
        let parsed = parse_model_planner_response(
            "```json\n{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\",\"query\":\"Dune\"}]}\n```",
        )
        .expect("expected parsed planner response");
        assert_eq!(parsed.mode.as_deref(), Some("tool_plan"));
        assert_eq!(parsed.tools.len(), 1);
        assert_eq!(parsed.tools[0].tool, "library_search_titles");
        assert_eq!(parsed.tools[0].query.as_deref(), Some("Dune"));
    }

    #[test]
    fn model_planner_normalization_respects_role_visibility() {
        let response: ModelPlannerResponse = serde_json::from_str(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"system_get_host_runtime_summary\"},{\"tool\":\"library_search_titles\",\"query\":\"Dune\"}]}",
        )
        .expect("expected planner response");
        let tools = normalize_model_plan(
            &response,
            &auth_user("user"),
            "How much RAM is the server using and do I have Dune?",
        );
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::LibrarySearchTitles);
    }

    #[test]
    fn model_planner_normalizes_server_filter_values() {
        let response: ModelPlannerResponse = serde_json::from_str(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"servers_list_minecraft_status\",\"availability\":\"running\",\"query\":\"Survival\"}]}",
        )
        .expect("expected planner response");
        let tools = normalize_model_plan(
            &response,
            &auth_user("admin"),
            "Is the Minecraft server called Survival online?",
        );
        assert_eq!(tools.len(), 1);
        match &tools[0].input {
            AssistantToolInput::ServerFilter {
                query,
                availability,
            } => {
                assert_eq!(query.as_deref(), Some("Survival"));
                assert_eq!(availability.as_deref(), Some("online"));
            }
            _ => panic!("expected server filter"),
        }
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
    fn planner_extracts_next_week_birthday_window() {
        let tools = plan_tool_calls("Which birthdays are next week?");
        assert_eq!(tools[0].tool, AssistantToolName::CalendarUpcomingBirthdays);
        match &tools[0].input {
            AssistantToolInput::CalendarWindow { label, .. } => assert_eq!(label, "next week"),
            _ => panic!("expected calendar window"),
        }
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
}
