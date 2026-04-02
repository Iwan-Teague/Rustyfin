use std::collections::HashSet;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use futures::{StreamExt, future::join_all};
use rustfin_ai_agent::{ChatChunk, ChatMessage, LlamaEngine, SamplingParams};
use serde::Deserialize;

use super::confirmation::{
    is_supported_calendar_create_intent, pending_action_request_for_message,
};
use super::context::AssistantContext;
use super::dates::{assistant_local_now, assistant_local_today, extract_single_calendar_date};
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
    if let Some(result) = pending_action_request_for_message(user, &request.message, None) {
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
{{\"mode\":\"tool_plan\",\"tools\":[{{\"tool\":\"tool_name\",\"query\":\"optional\",\"url\":\"optional\",\"availability\":\"optional\",\"room_mode\":\"optional\"}}]}}\n\
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
- Use weather_get_forecast for forecast, tomorrow, weekend, next few days, rain chance, or weather planning questions.\n\
- Use rooms_list_joinable for invites or rooms the user can join now.\n\
- Use system_get_current_datetime for current date/time questions or when the user asks what calendar date a relative day like next Tuesday lands on.\n\
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
        AssistantToolName::WebSearchPublicWeb => " Args: required query.",
        AssistantToolName::WebFetchPublicPageSummary => " Args: required url.",
        AssistantToolName::RoomsListActive => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsListJoinable => " Args: optional room_mode, optional query.",
        AssistantToolName::RoomsGetRoomSummary => " Args: required query, optional room_mode.",
        AssistantToolName::SystemGetCurrentDateTime => " Args: none.",
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
        AssistantToolName::CalendarCreateEvent | AssistantToolName::CalendarCreateBirthday => None,
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::NetworkGetTopologySummary
        | AssistantToolName::CalendarGetNextEvent
        | AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => Some(AssistantToolInput::None),
        AssistantToolName::CalendarListEvents => Some(extract_calendar_window(message, 7, None)),
        AssistantToolName::CalendarUpcomingBirthdays => Some(extract_calendar_window(
            message,
            30,
            normalize_optional_query(response.query.clone())
                .or_else(|| extract_birthday_query(message)),
        )),
        AssistantToolName::CalendarGetEventDetails => Some(extract_calendar_window(
            message,
            30,
            normalize_optional_query(response.query.clone())
                .or_else(|| extract_calendar_event_detail_query(message)),
        )),
        AssistantToolName::ChannelsListUnreadActivity => Some(AssistantToolInput::ChannelsFilter {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_channel_query(message)),
        }),
        AssistantToolName::ChannelsGetTranscriptSummary => {
            Some(AssistantToolInput::ChannelsFilter {
                query: normalize_optional_query(response.query.clone())
                    .or_else(|| extract_transcript_channel_query(message)),
            })
        }
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
        AssistantToolName::LibrariesGetRecentlyAdded => Some(AssistantToolInput::LibraryRecent {
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_recent_library_query(message)),
        }),
        AssistantToolName::WeatherGetCurrent => Some(AssistantToolInput::Weather {
            location: normalize_optional_query(response.query.clone())
                .or_else(|| extract_weather_location(message))?,
            forecast_days: None,
        }),
        AssistantToolName::WeatherGetForecast => Some(AssistantToolInput::Weather {
            location: normalize_optional_query(response.query.clone())
                .or_else(|| extract_weather_location(message))?,
            forecast_days: Some(extract_weather_forecast_days(message)),
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
        AssistantToolName::RoomsListJoinable => Some(AssistantToolInput::RoomsFilter {
            room_mode: normalize_room_mode(response.room_mode.as_deref())
                .or_else(|| detect_room_mode(message)),
            query: normalize_optional_query(response.query.clone())
                .or_else(|| extract_room_query(message)),
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
Do not claim to have created, updated, deleted, or changed anything in Rustyfin unless a confirmed server-side write tool actually ran and the backend verified the result."
        .to_string()
}

pub fn immediate_response_for_message(message: &str) -> Option<String> {
    clarification_for_message(message)
}

pub fn unsupported_write_response_for_message(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if is_supported_calendar_create_intent(&lower) {
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
        let calendar_input = extract_calendar_window(message, 30, extract_birthday_query(message));
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

    if let Some(weather_input) = extract_weather_input(message) {
        let weather_tool = match weather_input {
            AssistantToolInput::Weather {
                forecast_days: Some(_),
                ..
            } => AssistantToolName::WeatherGetForecast,
            AssistantToolInput::Weather {
                forecast_days: None,
                ..
            } => AssistantToolName::WeatherGetCurrent,
            _ => AssistantToolName::WeatherGetCurrent,
        };
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

    if is_host_runtime_query(&lower) {
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
    grounding_blocks: &[AssistantToolContextBlock],
) -> Vec<ChatMessage> {
    let local_now = assistant_local_now();
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
        (AssistantToolName::LibrariesListAccessible, _) => {
            "Checking accessible libraries".to_string()
        }
        (AssistantToolName::SystemGetCurrentDateTime, _) => {
            "Checking the Rustyfin host date and time".to_string()
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
            AssistantToolName::CalendarCreateEvent | AssistantToolName::CalendarCreateBirthday => {}
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
                        extract_calendar_window(message, 30, birthday_query),
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
            AssistantToolName::WeatherGetCurrent | AssistantToolName::WeatherGetForecast => {
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
                if is_current_datetime_query(&message.to_ascii_lowercase()) {
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
        "weather_get_current" | "weather_get_forecast" => {
            is_weather_query(&lower)
                || extract_weather_location(message).is_some()
                || has_any(
                    &lower,
                    &[
                        "today",
                        "tomorrow",
                        "weekend",
                        "this week",
                        "next week",
                        "rain",
                        "wind",
                        "temperature",
                    ],
                )
        }
        "web_search_public_web" | "web_fetch_public_page_summary" => {
            extract_public_web_url(message).is_some()
                || extract_public_web_search_query(message).is_some()
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
            "coming up next on my calendar",
            "what is my next event",
            "what's my next event",
            "whats my next event",
            "what is the next event",
            "what's the next event",
            "whats the next event",
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

fn extract_weather_input(message: &str) -> Option<AssistantToolInput> {
    let lower = message.to_ascii_lowercase();
    if !is_weather_query(&lower) {
        return None;
    }

    let location = extract_weather_location(message)?;
    let forecast_days = if weather_prefers_forecast(&lower) {
        Some(extract_weather_forecast_days(message))
    } else {
        None
    };
    Some(AssistantToolInput::Weather {
        location,
        forecast_days,
    })
}

fn extract_weather_follow_up_call(
    message: &str,
    history: &[AssistantHistoryMessage],
) -> Option<(AssistantToolName, AssistantToolInput)> {
    let lower = message.to_ascii_lowercase();
    let (default_tool, fallback_location, fallback_days) = recent_weather_hint(history)?;
    let has_hint = is_weather_query(&lower)
        || extract_weather_location(message).is_some()
        || has_any(
            &lower,
            &[
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
            ],
        );
    if !has_hint {
        return None;
    }

    let location = extract_weather_location(message).unwrap_or(fallback_location);
    let tool = weather_follow_up_tool(&lower, default_tool);
    let explicit_forecast_days = if weather_prefers_forecast(&lower) {
        Some(extract_weather_forecast_days(message))
    } else {
        None
    };
    let forecast_days = match tool {
        AssistantToolName::WeatherGetForecast => {
            explicit_forecast_days.or(fallback_days).or(Some(3))
        }
        _ => None,
    };

    Some((
        tool,
        AssistantToolInput::Weather {
            location,
            forecast_days,
        },
    ))
}

fn recent_weather_hint(
    history: &[AssistantHistoryMessage],
) -> Option<(AssistantToolName, String, Option<u8>)> {
    for context in recent_follow_up_contexts(history) {
        let tool = match context.tool.as_str() {
            "weather_get_current" => AssistantToolName::WeatherGetCurrent,
            "weather_get_forecast" => AssistantToolName::WeatherGetForecast,
            _ => continue,
        };
        let location = context.input_hint.weather_location.clone()?;
        return Some((tool, location, context.input_hint.weather_days));
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

fn weather_follow_up_tool(
    message_lower: &str,
    default_tool: AssistantToolName,
) -> AssistantToolName {
    if weather_prefers_forecast(message_lower) {
        AssistantToolName::WeatherGetForecast
    } else if weather_prefers_current(message_lower) {
        AssistantToolName::WeatherGetCurrent
    } else {
        default_tool
    }
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
    ] {
        if let Some(candidate) = extract_location_after_marker(message, &lower, marker) {
            return Some(candidate);
        }
    }
    None
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

    let lower = trimmed.to_ascii_lowercase();
    let stop_markers = [
        " right now",
        " currently",
        " today",
        " tomorrow",
        " this week",
        " next week",
        " this weekend",
        " weekend",
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
    has_any(
        message_lower,
        &[
            "what date is it",
            "what day is it",
            "what time is it",
            "what's the date",
            "whats the date",
            "what's the time",
            "whats the time",
            "today's date",
            "todays date",
            "current date",
            "current time",
            "current day",
            "date today",
            "time right now",
            "date right now",
        ],
    ) || ((message_lower.contains("what date is next ")
        || message_lower.contains("what day is next ")
        || message_lower.contains("when is next ")
        || message_lower.contains("what date is this ")
        || message_lower.contains("what day is this "))
        && has_any(
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
        ))
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
            "what network",
            "what interfaces",
        ],
    ) || (has_any(
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

fn extract_birthday_query(message: &str) -> Option<String> {
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

    for suffix in [
        "'s birthday",
        "’s birthday",
        " birthday",
        " birthdays",
        "'s",
        "’s",
    ] {
        if normalized.to_ascii_lowercase().ends_with(suffix) {
            let keep_len = normalized.len().saturating_sub(suffix.len());
            normalized.truncate(keep_len);
            normalized = normalized.trim().to_string();
            break;
        }
    }

    if normalized.is_empty() {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    if looks_like_calendar_window_phrase(&lower)
        || matches!(
            lower.as_str(),
            "it" | "them" | "those" | "these" | "ones" | "one"
        )
    {
        return None;
    }

    Some(normalized)
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
        AssistantToolName, ModelPlannerResponse, clarification_for_message, normalize_model_plan,
        parse_model_planner_response, plan_tool_calls, plan_tool_calls_with_history,
        status_label_for_tool_call, unsupported_write_response_for_message,
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
            AssistantToolInput::None => panic!("expected library search input"),
            AssistantToolInput::CalendarWindow { .. } => panic!("unexpected calendar window"),
            AssistantToolInput::ChannelsFilter { .. } => panic!("unexpected channel filter"),
            AssistantToolInput::DownloadsFilter { .. } => panic!("unexpected downloads filter"),
            AssistantToolInput::LibraryRecent { .. } => panic!("unexpected recent library input"),
            AssistantToolInput::Weather { .. } => panic!("unexpected weather input"),
            AssistantToolInput::WebSearch { .. } => panic!("unexpected web search"),
            AssistantToolInput::WebFetch { .. } => panic!("unexpected web fetch"),
            AssistantToolInput::RoomsFilter { .. } => panic!("unexpected room filter"),
            AssistantToolInput::ServerFilter { .. } => panic!("unexpected server filter"),
            AssistantToolInput::CalendarCreateEvent { .. } => {
                panic!("unexpected calendar create event input")
            }
            AssistantToolInput::CalendarCreateBirthday { .. } => {
                panic!("unexpected calendar create birthday input")
            }
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
    fn planner_detects_network_query() {
        let tools = plan_tool_calls("What network interfaces are active right now?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::NetworkGetTopologySummary);
        assert!(matches!(tools[0].input, AssistantToolInput::None));
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
    fn model_planner_parser_extracts_json_from_surrounding_prose() {
        let parsed = parse_model_planner_response(
            "I will use a grounded tool.\n{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"calendar_list_events\"}]}\nThanks!",
        )
        .expect("expected parsed planner response");
        assert_eq!(parsed.mode.as_deref(), Some("tool_plan"));
        assert_eq!(parsed.tools.len(), 1);
        assert_eq!(parsed.tools[0].tool, "calendar_list_events");
    }

    #[test]
    fn model_planner_parser_rejects_non_json_output() {
        assert!(parse_model_planner_response("no usable planner json here").is_none());
    }

    #[test]
    fn model_planner_mode_none_discards_tool_entries() {
        let response: ModelPlannerResponse = serde_json::from_str(
            "{\"mode\":\"none\",\"tools\":[{\"tool\":\"library_search_titles\",\"query\":\"Dune\"}]}",
        )
        .expect("expected planner response");
        let tools = normalize_model_plan(&response, &auth_user("admin"), "Do I have Dune?");
        assert!(tools.is_empty());
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
    fn model_planner_normalization_deduplicates_duplicate_tools() {
        let response: ModelPlannerResponse = serde_json::from_str(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"calendar_list_events\"},{\"tool\":\"calendar_list_events\"},{\"tool\":\"calendar_list_events\"}]}",
        )
        .expect("expected planner response");
        let tools =
            normalize_model_plan(&response, &auth_user("user"), "What events are this week?");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool, AssistantToolName::CalendarListEvents);
    }

    #[test]
    fn model_planner_normalization_ignores_tools_without_required_inputs() {
        let response: ModelPlannerResponse = serde_json::from_str(
            "{\"mode\":\"tool_plan\",\"tools\":[{\"tool\":\"library_search_titles\"},{\"tool\":\"weather_get_current\"}]}",
        )
        .expect("expected planner response");
        let tools = normalize_model_plan(&response, &auth_user("user"), "Hello there");
        assert!(tools.is_empty());
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
    fn planner_routes_next_event_queries_to_deterministic_tool() {
        let tools = plan_tool_calls("What's my next event?");
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
