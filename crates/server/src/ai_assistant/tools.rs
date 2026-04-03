use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use futures::{StreamExt, future::join_all};
use rustfin_ai_agent::{ChatChunk, ChatMessage, SamplingParams};
use serde::Serialize;
use serde_json::json;

use super::context::AssistantContext;
use super::dates::{assistant_local_now, assistant_local_today, assistant_local_year};
use super::orchestrator::plan_tool_calls_with_model_assist;
use super::registry::AssistantToolName;
use super::types::{
    AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
    AssistantGroundingSource, AssistantHistoryMessage, AssistantToolContextBlock,
    AssistantToolInput, PlannedToolCall, ToolAccessMode, ToolConfirmationPolicy,
    ToolRoleRequirement,
};
use super::weather::{
    fetch_public_weather_current, fetch_public_weather_forecast, fetch_public_weather_history,
    resolve_public_location_timezone,
};
use super::web::{fetch_public_page_summary, public_web_tools_enabled, search_public_web};
use crate::ai_generated_artifacts::artifact_download_path;
use crate::ai_storage::{current_model_dir, model_file_path};
use crate::auth::AuthUser;
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct AccountProfileSummary {
    username: String,
    display_name: String,
    role: String,
    time_zone: Option<String>,
    accessible_library_count: usize,
}

#[derive(Debug, Serialize)]
struct LibrarySummary {
    id: String,
    name: String,
    kind: String,
    item_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct DownloadArtifactSummary {
    id: String,
    title: String,
    availability: String,
    version: Option<String>,
    install_mode: Option<String>,
    summary: String,
    detail: String,
}

#[derive(Debug, Serialize)]
struct GeneratedDocumentArtifactSummary {
    id: String,
    title: String,
    file_name: String,
    media_type: String,
    byte_size: i64,
    download_path: String,
}

#[derive(Debug, Serialize)]
struct LibraryItemMatch {
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct LibraryItemDetailSummary {
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
    overview: Option<String>,
    duration_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
struct RoomSummary {
    room_id: String,
    title: String,
    room_mode: String,
    host_username: String,
    password_required: bool,
    member_count: i64,
}

#[derive(Debug, Serialize)]
struct RoomMemberSummary {
    username: String,
    role: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct RoomDetailSummary {
    room_id: String,
    title: String,
    room_mode: String,
    host_user_id: String,
    status: String,
    member_count: i64,
    password_required: bool,
    members: Vec<RoomMemberSummary>,
}

#[derive(Debug, Serialize)]
struct ServerSummary {
    id: String,
    display_name: String,
    observed_state: String,
    health_state: String,
    current_player_count: i64,
    max_player_count: Option<i64>,
    listen_address: String,
}

#[derive(Debug, Serialize)]
struct ServerDetailSummary {
    id: String,
    display_name: String,
    slug: String,
    observed_state: String,
    health_state: String,
    desired_state: String,
    current_player_count: i64,
    max_player_count: Option<i64>,
    listen_address: String,
    minecraft_version: String,
    world_name: String,
    gamemode: String,
    difficulty: String,
    motd: String,
    owner_display_name: String,
}

#[derive(Debug, Serialize)]
struct CalendarEventSummary {
    title: String,
    event_date: String,
    scope: String,
    event_type: String,
    owner_username: Option<String>,
}

#[derive(Debug, Serialize)]
struct BirthdaySummary {
    title: String,
    event_date: String,
    month_day_display: String,
    next_occurs_on: String,
    scope: String,
    owner_username: Option<String>,
    birthday_year: Option<i32>,
}

#[derive(Debug, Serialize)]
struct CalendarEventDetailSummary {
    id: String,
    title: String,
    description: Option<String>,
    event_date: String,
    scope: String,
    event_type: String,
    recurrence: String,
    owner_username: Option<String>,
    created_by_username: Option<String>,
    birthday_year: Option<i32>,
    month_day_display: Option<String>,
    next_occurs_on: Option<String>,
}

#[derive(Debug, Serialize)]
struct NextCalendarEventSummary {
    id: String,
    title: String,
    event_date: String,
    event_type: String,
    scope: String,
    owner_username: Option<String>,
    recurrence: String,
    next_occurs_on: String,
}

#[derive(Debug, Serialize)]
struct ChannelActivityMessageSummary {
    username: String,
    content_preview: String,
    created_ts: i64,
}

#[derive(Debug, Serialize)]
struct ChannelActivitySummary {
    channel_id: String,
    name: String,
    kind: String,
    is_private: bool,
    latest_message: Option<ChannelActivityMessageSummary>,
}

#[derive(Debug, Serialize)]
struct TranscriptSpeakerSummary {
    username: String,
    segment_count: usize,
    word_count: usize,
    approx_spoken_seconds: i64,
}

#[derive(Debug, Serialize)]
struct TranscriptHighlightSummary {
    username: String,
    started_ts_ms: i64,
    ended_ts_ms: i64,
    relative_start: String,
    relative_end: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct ChannelTranscriptSummary {
    channel_id: String,
    channel_name: String,
    session_id: String,
    started_ts: i64,
    ended_ts: i64,
    duration_seconds: i64,
    started_by_username: String,
    entry_count: i64,
    speaker_count: usize,
    top_terms: Vec<String>,
    speakers: Vec<TranscriptSpeakerSummary>,
    highlights: Vec<TranscriptHighlightSummary>,
    transcript_excerpt: String,
    transcript_excerpt_truncated: bool,
}

#[derive(Debug, Serialize)]
struct LibraryRecentItemSummary {
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
    created_ts: i64,
}

#[derive(Debug, Serialize)]
struct JoinableRoomSummary {
    room_id: String,
    title: String,
    room_mode: String,
    host_username: String,
    password_required: bool,
    joinable_via: String,
    member_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HostRuntimeAssistantSummary {
    host: crate::runtime_diagnostics::HostRuntimeSnapshot,
    rustyfin: HostRuntimeRustyfinSummary,
    memory: Option<HostRuntimeMemorySummary>,
    swap: Option<HostRuntimeSwapSummary>,
}

#[derive(Debug, Serialize)]
struct HostRuntimeRustyfinSummary {
    uptime_seconds: u64,
    active_jobs: u64,
    active_transcode_sessions: usize,
    active_channels_websockets: u64,
    active_watch_party_websockets: u64,
    ai_chat_requests_in_flight: u64,
    ai_tool_calls_in_flight: u64,
}

#[derive(Debug, Serialize)]
struct HostRuntimeMemorySummary {
    used_memory_bytes: u64,
    total_memory_bytes: u64,
    used_memory_gib: f64,
    total_memory_gib: f64,
    used_memory_human: String,
    total_memory_human: String,
    memory_summary: String,
}

#[derive(Debug, Serialize)]
struct HostRuntimeSwapSummary {
    used_swap_bytes: u64,
    total_swap_bytes: u64,
    used_swap_gib: f64,
    total_swap_gib: f64,
    used_swap_human: String,
    total_swap_human: String,
    swap_summary: String,
}

#[derive(Debug, Serialize)]
struct CurrentDateTimeAssistantSummary {
    local_timestamp: String,
    local_date: String,
    local_time: String,
    weekday: String,
    timezone_offset: String,
    timezone_name: Option<String>,
    resolved_location: Option<String>,
    location_query: Option<String>,
    unix_timestamp: i64,
}

#[derive(Debug, Serialize)]
struct PublicWebSearchSummary {
    query: String,
    results: Vec<super::web::PublicWebSearchResult>,
}

#[derive(Debug, Serialize)]
struct BackupAssistantSummary {
    configured: bool,
    restore_supported: bool,
    last_successful_backup_ts: Option<i64>,
    policy_count: i64,
    total_job_count: i64,
    successful_job_count: i64,
    failed_job_count: i64,
    message: String,
}

#[derive(Debug, Serialize)]
struct ServiceHealthComponentSummary {
    name: String,
    status: String,
    configured: bool,
    url: Option<String>,
    detail: String,
}

#[derive(Debug, Serialize)]
struct ServiceHealthAssistantSummary {
    all_healthy: bool,
    components: Vec<ServiceHealthComponentSummary>,
}

#[derive(Debug, Serialize)]
struct TranscodeAssistantDetailedSummary {
    active_sessions: usize,
    active_session_ids: Vec<String>,
    created_total: u64,
    create_failures_total: u64,
    create_failures_last_minute: u64,
    create_failures_last_five_minutes: u64,
    cleaned_total: u64,
    ffmpeg_path: String,
    ffprobe_path: String,
    hw_accel: Option<String>,
    hw_device_path: Option<String>,
    hw_accel_required: bool,
}

#[derive(Debug, Serialize, Clone)]
struct StoragePathSummary {
    name: String,
    path: String,
    exists: bool,
    resolved_path: Option<String>,
    stats_path: Option<String>,
    mount_point: Option<String>,
    mount_file_system: Option<String>,
    mount_source: Option<String>,
    total_bytes: Option<u64>,
    total_human: Option<String>,
    available_bytes: Option<u64>,
    available_human: Option<String>,
    used_bytes: Option<u64>,
    used_human: Option<String>,
    used_percent: Option<f64>,
}

#[derive(Debug, Serialize, Clone)]
struct StorageMountSummary {
    mount_point: String,
    mount_file_system: Option<String>,
    mount_source: Option<String>,
    tracked_paths: Vec<String>,
    total_bytes: Option<u64>,
    total_human: Option<String>,
    available_bytes: Option<u64>,
    available_human: Option<String>,
    used_bytes: Option<u64>,
    used_human: Option<String>,
    used_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
struct StorageAssistantSummary {
    available: bool,
    reason: Option<String>,
    mounts: Vec<StorageMountSummary>,
    paths: Vec<StoragePathSummary>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxMountEntry {
    mount_point: std::path::PathBuf,
    file_system: String,
    source: Option<String>,
}

#[derive(Debug, Serialize)]
struct RecentErrorItemSummary {
    source: String,
    kind: String,
    occurred_ts: Option<i64>,
    message: String,
}

#[derive(Debug, Serialize)]
struct RuntimeFailureWindowSummary {
    failures_last_minute: u64,
    failures_last_five_minutes: u64,
}

#[derive(Debug, Serialize)]
struct RecentErrorsAssistantSummary {
    recent_failed_jobs: Vec<RecentErrorItemSummary>,
    transcode_failures: RuntimeFailureWindowSummary,
    agent_failures: HashMap<String, RuntimeFailureWindowSummary>,
}

pub async fn execute_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let tool = call.tool;
    let spec = tool.spec();
    if let Some(message) = enforce_tool_policy(context, spec) {
        return AssistantToolContextBlock {
            tool: spec.name,
            label: spec.summary.to_string(),
            status: "error",
            data: json!({ "message": message }),
        };
    }

    let result = match tool {
        AssistantToolName::AccountGetProfileSummary => {
            account_get_profile_summary(state, context).await
        }
        AssistantToolName::CalendarListEvents => calendar_list_events(state, context, call).await,
        AssistantToolName::CalendarGetNextEvent => calendar_get_next_event(state, context).await,
        AssistantToolName::CalendarUpcomingBirthdays => {
            calendar_upcoming_birthdays(state, context, call).await
        }
        AssistantToolName::CalendarGetEventDetails => {
            calendar_get_event_details(state, context, call).await
        }
        AssistantToolName::CalendarCreateEvent => calendar_create_event(state, context, call).await,
        AssistantToolName::CalendarCreateBirthday => {
            calendar_create_birthday(state, context, call).await
        }
        AssistantToolName::CalendarDeleteEvent => calendar_delete_event(state, context, call).await,
        AssistantToolName::DocumentCreateDownload => {
            document_create_download(state, context, call).await
        }
        AssistantToolName::ChannelsListUnreadActivity => {
            channels_list_unread_activity(state, context, call).await
        }
        AssistantToolName::ChannelsGetTranscriptSummary => {
            channels_get_transcript_summary(state, context, call).await
        }
        AssistantToolName::DownloadsListAvailableArtifacts => {
            downloads_list_available_artifacts(state, context, call).await
        }
        AssistantToolName::NetworkGetTopologySummary => {
            network_get_topology_summary(state, context).await
        }
        AssistantToolName::LibrariesListAccessible => {
            libraries_list_accessible(state, context).await
        }
        AssistantToolName::LibrarySearchTitles => library_search_titles(state, context, call).await,
        AssistantToolName::LibraryGetItemSummary => {
            library_get_item_summary(state, context, call).await
        }
        AssistantToolName::LibrariesGetRecentlyAdded => {
            libraries_get_recently_added(state, context, call).await
        }
        AssistantToolName::WeatherGetCurrent => weather_get_current(state, context, call).await,
        AssistantToolName::WeatherGetForecast => weather_get_forecast(state, context, call).await,
        AssistantToolName::WeatherGetHistory => weather_get_history(state, context, call).await,
        AssistantToolName::WebSearchPublicWeb => web_search_public_web(state, context, call).await,
        AssistantToolName::WebFetchPublicPageSummary => {
            web_fetch_public_page_summary(state, context, call).await
        }
        AssistantToolName::RoomsListActive => rooms_list_active(state, context, call).await,
        AssistantToolName::RoomsListJoinable => rooms_list_joinable(state, context, call).await,
        AssistantToolName::RoomsGetRoomSummary => room_get_room_summary(state, context, call).await,
        AssistantToolName::SystemGetCurrentDateTime => system_get_current_datetime(call).await,
        AssistantToolName::SystemGetHostRuntimeSummary => {
            system_get_host_runtime_summary(state, context).await
        }
        AssistantToolName::SystemGetBackupSummary => system_get_backup_summary(state).await,
        AssistantToolName::SystemGetServiceHealth => system_get_service_health(state).await,
        AssistantToolName::SystemGetTranscodeSummary => system_get_transcode_summary(state).await,
        AssistantToolName::SystemGetStorageSummary => system_get_storage_summary(state).await,
        AssistantToolName::SystemGetRecentErrors => system_get_recent_errors(state).await,
        AssistantToolName::ServersListMinecraftStatus => {
            servers_list_minecraft_status(state, context, call).await
        }
        AssistantToolName::ServersGetMinecraftServerSummary => {
            server_get_minecraft_server_summary(state, context, call).await
        }
    };

    match result {
        Ok((label, data)) => AssistantToolContextBlock {
            tool: spec.name,
            label,
            status: "ok",
            data,
        },
        Err(message) => AssistantToolContextBlock {
            tool: spec.name,
            label: spec.summary.to_string(),
            status: "error",
            data: json!({ "message": message }),
        },
    }
}

fn enforce_tool_policy(
    context: &AssistantContext,
    spec: super::types::AssistantToolSpec,
) -> Option<String> {
    match spec.required_role {
        ToolRoleRequirement::AnyAuthenticatedUser => {}
        ToolRoleRequirement::AdminOnly if !context.is_admin => {
            return Some(format!("{} requires an admin Rustyfin account.", spec.name));
        }
        ToolRoleRequirement::AdminOnly => {}
    }

    match spec.access_mode {
        ToolAccessMode::ReadOnly => {}
        ToolAccessMode::Write => {
            if context.confirmed_write_tool.as_deref() != Some(spec.name) {
                return Some(format!(
                    "{} requires explicit confirmation before Rustyfin AI can run it.",
                    spec.name
                ));
            }
        }
        ToolAccessMode::DestructiveWrite => {
            return Some(format!(
                "{} requires a protected confirmation flow that is not available yet.",
                spec.name
            ));
        }
    }

    match spec.confirmation {
        ToolConfirmationPolicy::None => None,
        ToolConfirmationPolicy::ExplicitUserConfirm => {
            if context.confirmed_write_tool.as_deref() == Some(spec.name) {
                None
            } else {
                Some(format!(
                    "{} requires explicit confirmation before Rustyfin AI can run it.",
                    spec.name
                ))
            }
        }
        ToolConfirmationPolicy::ProtectedAction => Some(format!(
            "{} is blocked until the protected confirmation flow is implemented.",
            spec.name
        )),
    }
}

pub fn source_from_block(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> AssistantGroundingSource {
    let spec = tool.spec();
    AssistantGroundingSource {
        tool: spec.name.to_string(),
        label: block.label.clone(),
        access_mode: spec.access_mode,
        risk_tier: spec.risk_tier,
        status: block.status.to_string(),
        download_url: block
            .data
            .get("artifact")
            .and_then(|artifact| artifact.get("download_path"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_file_name: block
            .data
            .get("artifact")
            .and_then(|artifact| artifact.get("file_name"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_media_type: block
            .data
            .get("artifact")
            .and_then(|artifact| artifact.get("media_type"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_size_bytes: block
            .data
            .get("artifact")
            .and_then(|artifact| artifact.get("byte_size"))
            .and_then(serde_json::Value::as_i64),
    }
}

pub fn build_follow_up_context(
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
) -> AssistantFollowUpContext {
    let mut input_hint = follow_up_input_hint(call);
    if call.tool == AssistantToolName::CalendarGetNextEvent {
        input_hint.calendar_label = Some("your next calendar event".to_string());
        input_hint.calendar_from_date = block
            .data
            .get("next_event")
            .and_then(|event| event.get("next_occurs_on"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        input_hint.calendar_to_date = input_hint.calendar_from_date.clone();
        input_hint.calendar_query = block
            .data
            .get("next_event")
            .and_then(|event| event.get("title"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
    } else if matches!(
        call.tool,
        AssistantToolName::CalendarCreateEvent | AssistantToolName::CalendarCreateBirthday
    ) {
        input_hint.calendar_label = Some("the created calendar event".to_string());
        input_hint.calendar_from_date = block
            .data
            .get("event")
            .and_then(|event| event.get("event_date"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        input_hint.calendar_to_date = input_hint.calendar_from_date.clone();
        input_hint.calendar_query = block
            .data
            .get("event")
            .and_then(|event| event.get("title"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
    }

    AssistantFollowUpContext {
        tool: call.tool.as_str().to_string(),
        label: block.label.clone(),
        input_hint,
        entities: follow_up_entities(call.tool, block),
    }
}

async fn account_get_profile_summary(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let user = rustfin_db::repo::users::find_by_id(&state.db, &context.user_id)
        .await
        .map_err(|e| format!("failed to load account profile: {e}"))?;

    let accessible_library_count = if context.is_admin {
        rustfin_db::repo::libraries::list_libraries(&state.db)
            .await
            .map_err(|e| format!("failed to load accessible libraries: {e}"))?
            .len()
    } else {
        rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
            .await
            .map_err(|e| format!("failed to load accessible libraries: {e}"))?
            .len()
    };

    let summary = if let Some(user) = user {
        AccountProfileSummary {
            username: user.username,
            display_name: user.display_name,
            role: user.role,
            time_zone: user.time_zone,
            accessible_library_count,
        }
    } else {
        AccountProfileSummary {
            username: context.username.clone(),
            display_name: context.username.clone(),
            role: context.role.clone(),
            time_zone: None,
            accessible_library_count,
        }
    };

    Ok((
        "Signed-in Rustyfin account summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn libraries_list_accessible(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
                .await
                .map_err(|e| format!("failed to load library permissions: {e}"))?
                .into_iter()
                .collect::<HashSet<_>>(),
        )
    };

    let libraries = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to load libraries: {e}"))?;

    let libraries: Vec<_> = libraries
        .into_iter()
        .filter(|lib| {
            allowed_library_ids
                .as_ref()
                .map(|allowed| allowed.contains(&lib.id))
                .unwrap_or(true)
        })
        .collect();

    let library_ids: Vec<String> = libraries.iter().map(|lib| lib.id.clone()).collect();
    let counts =
        rustfin_db::repo::libraries::count_library_items_for_libraries(&state.db, &library_ids)
            .await
            .map_err(|e| format!("failed to count library items: {e}"))?;
    let counts: HashMap<_, _> = counts.into_iter().collect();

    let summaries: Vec<_> = libraries
        .iter()
        .take(12)
        .map(|lib| LibrarySummary {
            id: lib.id.clone(),
            name: lib.name.clone(),
            kind: lib.kind.clone(),
            item_count: counts.get(&lib.id).copied(),
        })
        .collect();

    Ok((
        format!("{} accessible libraries", libraries.len()),
        json!({
            "total_count": libraries.len(),
            "libraries": summaries,
        }),
    ))
}

async fn downloads_list_available_artifacts(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let filtered_items: Vec<_> = catalog
        .items
        .into_iter()
        .filter(|item| downloads_matches_availability(item, availability_filter.as_deref()))
        .filter(|item| downloads_matches_query(item, query.as_deref()))
        .collect();

    let summaries: Vec<_> = filtered_items
        .iter()
        .map(|item| DownloadArtifactSummary {
            id: item.id.clone(),
            title: item.title.clone(),
            availability: match item.availability {
                crate::downloads::DownloadArtifactAvailability::Available => {
                    "available".to_string()
                }
                crate::downloads::DownloadArtifactAvailability::Unavailable => {
                    "unavailable".to_string()
                }
                crate::downloads::DownloadArtifactAvailability::Planned => "planned".to_string(),
            },
            version: item.version.clone(),
            install_mode: item.install_mode.clone(),
            summary: item.summary.clone(),
            detail: item.detail.clone(),
        })
        .collect();

    let label = downloads_status_label(
        filtered_items.len(),
        query.as_deref(),
        availability_filter.as_deref(),
    );

    Ok((
        label,
        json!({
            "total_count": summaries.len(),
            "query": query,
            "availability_filter": availability_filter,
            "artifacts": summaries,
        }),
    ))
}

async fn weather_get_current(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::Weather { location, .. } = &call.input else {
        return Err("missing public weather location".to_string());
    };
    let current = fetch_public_weather_current(location).await?;
    Ok((
        format!("Current weather for {}", current.resolved_location),
        serde_json::to_value(current).unwrap_or_else(|_| json!({})),
    ))
}

async fn weather_get_forecast(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::Weather {
        location,
        forecast_days,
    } = &call.input
    else {
        return Err("missing public weather location".to_string());
    };
    let forecast = fetch_public_weather_forecast(location, *forecast_days).await?;
    let day_count = forecast.forecast_days.len();
    Ok((
        format!(
            "{day_count}-day weather forecast for {}",
            forecast.resolved_location
        ),
        serde_json::to_value(forecast).unwrap_or_else(|_| json!({})),
    ))
}

async fn weather_get_history(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::WeatherHistory {
        location,
        start_date,
        end_date,
        ..
    } = &call.input
    else {
        return Err("missing public weather history input".to_string());
    };
    let start_date = NaiveDate::parse_from_str(start_date, "%F")
        .map_err(|error| format!("invalid public weather history start date: {error}"))?;
    let end_date = NaiveDate::parse_from_str(end_date, "%F")
        .map_err(|error| format!("invalid public weather history end date: {error}"))?;
    let history = fetch_public_weather_history(location, start_date, end_date).await?;
    Ok((
        format!("Recent weather history for {}", history.resolved_location),
        serde_json::to_value(history).unwrap_or_else(|_| json!({})),
    ))
}

async fn web_search_public_web(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    if !public_web_tools_enabled() {
        return Err(format!(
            "public web tools are disabled on this host. Set {}=1 to enable them.",
            super::web::AI_PUBLIC_WEB_ENABLE_ENV
        ));
    }
    let AssistantToolInput::WebSearch { query } = &call.input else {
        return Err("missing public web search query".to_string());
    };
    let results = search_public_web(query, Some(5)).await?;
    let label = format!("{} public web results for \"{}\"", results.len(), query);
    Ok((
        label,
        serde_json::to_value(PublicWebSearchSummary {
            query: query.clone(),
            results,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn web_fetch_public_page_summary(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    if !public_web_tools_enabled() {
        return Err(format!(
            "public web tools are disabled on this host. Set {}=1 to enable them.",
            super::web::AI_PUBLIC_WEB_ENABLE_ENV
        ));
    }
    let AssistantToolInput::WebFetch { url } = &call.input else {
        return Err("missing public web URL".to_string());
    };
    let summary = fetch_public_page_summary(url).await?;
    let label = if let Some(title) = summary.page_title.as_deref() {
        format!("Fetched public page \"{title}\"")
    } else {
        format!("Fetched public page from {}", summary.source_host)
    };
    Ok((
        label,
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn rooms_list_active(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let rooms = rustfin_db::repo::watch_party::list_public_rooms(&state.db)
        .await
        .map_err(|e| format!("failed to load active rooms: {e}"))?;

    let room_mode_filter = room_mode_filter_for_call(call);
    let room_query_filter = room_query_filter_for_call(call);

    let filtered_rooms: Vec<_> = rooms
        .iter()
        .filter(|room| {
            room_mode_filter
                .as_deref()
                .map(|filter| room.room_mode.eq_ignore_ascii_case(filter))
                .unwrap_or(true)
        })
        .filter(|room| room_matches_query(room, room_query_filter.as_deref()))
        .collect();

    let summaries: Vec<_> = filtered_rooms
        .iter()
        .take(12)
        .map(|room| RoomSummary {
            room_id: room.id.clone(),
            title: room_title_for_listing(
                &room.room_name,
                &room.room_mode,
                &room.audio_source,
                &room.item_title,
                &room.audio_library_name,
                &room.web_url,
            ),
            room_mode: room.room_mode.clone(),
            host_username: room.host_username.clone(),
            password_required: room.password_required,
            member_count: room.member_count,
        })
        .collect();

    let label = room_mode_filter
        .as_deref()
        .map(room_mode_label)
        .map(|mode| format!("{} public {mode} rooms active", filtered_rooms.len()))
        .unwrap_or_else(|| format!("{} public rooms active", filtered_rooms.len()));

    Ok((
        label,
        json!({
            "total_count": filtered_rooms.len(),
            "room_mode_filter": room_mode_filter,
            "query": room_query_filter,
            "rooms": summaries,
        }),
    ))
}

async fn room_get_room_summary(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let room_mode_filter = room_mode_filter_for_call(call);
    let room_query_filter = room_query_filter_for_call(call);
    let rooms = rustfin_db::repo::watch_party::list_public_rooms(&state.db)
        .await
        .map_err(|e| format!("failed to load active rooms: {e}"))?;

    let room = rooms
        .iter()
        .filter(|room| {
            room_mode_filter
                .as_deref()
                .map(|filter| room.room_mode.eq_ignore_ascii_case(filter))
                .unwrap_or(true)
        })
        .find(|room| room_matches_query(room, room_query_filter.as_deref()))
        .ok_or_else(|| {
            room_query_filter
                .as_deref()
                .map(|query| format!("no active public room matched \"{query}\""))
                .unwrap_or_else(|| {
                    "no active public room matched the current selection".to_string()
                })
        })?;

    let room_detail = rustfin_db::repo::watch_party::get_room(&state.db, &room.id)
        .await
        .map_err(|e| format!("failed to load room details: {e}"))?
        .ok_or_else(|| format!("room {} is no longer available", room.id))?;
    let members = rustfin_db::repo::watch_party::list_members_with_usernames(&state.db, &room.id)
        .await
        .map_err(|e| format!("failed to load room members: {e}"))?;

    let summary = RoomDetailSummary {
        room_id: room_detail.id,
        title: room_title_for_listing(
            &room.room_name,
            &room.room_mode,
            &room.audio_source,
            &room.item_title,
            &room.audio_library_name,
            &room.web_url,
        ),
        room_mode: room_detail.room_mode,
        host_user_id: room_detail.host_user_id,
        status: room_detail.status,
        member_count: room_detail.joined_member_count,
        password_required: room_detail.join_password_hash.is_some(),
        members: members
            .into_iter()
            .map(|member| RoomMemberSummary {
                username: member.username,
                role: member.role,
                status: member.status,
            })
            .collect(),
    };

    Ok((
        format!("Room summary for \"{}\"", summary.title),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn library_search_titles(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::LibrarySearch { query } = &call.input else {
        return Err("missing library search query".to_string());
    };

    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
                .await
                .map_err(|e| format!("failed to load library permissions: {e}"))?,
        )
    };

    let items = rustfin_db::repo::items::search_items_by_title(
        &state.db,
        query,
        allowed_library_ids.as_deref(),
        10,
    )
    .await
    .map_err(|e| format!("failed to search library titles: {e}"))?;

    let libraries = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to load libraries: {e}"))?;
    let library_names: HashMap<_, _> = libraries
        .into_iter()
        .map(|lib| (lib.id, lib.name))
        .collect();

    let matches: Vec<_> = items
        .into_iter()
        .map(|item| LibraryItemMatch {
            id: item.id,
            title: item.title,
            kind: item.kind,
            year: item.year,
            library_name: library_names.get(&item.library_id).cloned(),
        })
        .collect();

    Ok((
        format!("Library matches for \"{query}\""),
        json!({
            "query": query,
            "match_count": matches.len(),
            "matches": matches,
        }),
    ))
}

async fn library_get_item_summary(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::LibrarySearch { query } = &call.input else {
        return Err("missing library item query".to_string());
    };

    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
                .await
                .map_err(|e| format!("failed to load library permissions: {e}"))?,
        )
    };

    let matches = rustfin_db::repo::items::search_items_by_title(
        &state.db,
        query,
        allowed_library_ids.as_deref(),
        5,
    )
    .await
    .map_err(|e| format!("failed to load library item matches: {e}"))?;
    let Some(best_match) = matches.into_iter().next() else {
        return Err(format!("no accessible library item matched \"{query}\""));
    };

    let item = rustfin_db::repo::items::get_item(&state.db, &best_match.id)
        .await
        .map_err(|e| format!("failed to load library item details: {e}"))?
        .ok_or_else(|| format!("library item {} is no longer available", best_match.id))?;

    let library_name = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to load libraries: {e}"))?
        .into_iter()
        .find(|library| library.id == item.library_id)
        .map(|library| library.name);

    let summary = LibraryItemDetailSummary {
        id: item.id,
        title: item.title,
        kind: item.kind,
        year: item.year,
        library_name,
        overview: item.overview,
        duration_ms: item.duration_ms,
    };

    Ok((
        format!("Library item summary for \"{}\"", summary.title),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn servers_list_minecraft_status(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let servers = rustfin_db::repo::servers::list_accessible_minecraft_servers(
        &state.db,
        &context.user_id,
        context.is_admin,
    )
    .await
    .map_err(|e| format!("failed to load Minecraft server status: {e}"))?;

    let (name_query, availability_filter) = server_filter_for_call(call);
    let filtered_servers: Vec<_> = servers
        .iter()
        .filter(|server| server_matches_availability(server, availability_filter.as_deref()))
        .filter(|server| server_matches_query(server, name_query.as_deref()))
        .collect();

    let summaries: Vec<_> = filtered_servers
        .iter()
        .take(12)
        .map(|server| ServerSummary {
            id: server.id.clone(),
            display_name: server.display_name.clone(),
            observed_state: server.observed_state.clone(),
            health_state: server.health_state.clone(),
            current_player_count: server.current_player_count,
            max_player_count: server.max_player_count,
            listen_address: format!("{}:{}", server.listen_host, server.listen_port),
        })
        .collect();

    let label = server_status_label(
        filtered_servers.len(),
        name_query.as_deref(),
        availability_filter.as_deref(),
    );

    Ok((
        label,
        json!({
            "total_count": filtered_servers.len(),
            "query": name_query,
            "availability_filter": availability_filter,
            "servers": summaries,
        }),
    ))
}

async fn server_get_minecraft_server_summary(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (name_query, availability_filter) = server_filter_for_call(call);
    let servers = rustfin_db::repo::servers::list_accessible_minecraft_servers(
        &state.db,
        &context.user_id,
        context.is_admin,
    )
    .await
    .map_err(|e| format!("failed to load Minecraft server status: {e}"))?;

    let server = servers
        .into_iter()
        .filter(|server| server_matches_availability(server, availability_filter.as_deref()))
        .find(|server| server_matches_query(server, name_query.as_deref()))
        .ok_or_else(|| {
            if let Some(query) = name_query.as_deref() {
                format!("no accessible Minecraft server matched \"{query}\"")
            } else {
                "no accessible Minecraft server matched the current selection".to_string()
            }
        })?;

    let summary = ServerDetailSummary {
        id: server.id,
        display_name: server.display_name,
        slug: server.slug,
        observed_state: server.observed_state,
        health_state: server.health_state,
        desired_state: server.desired_state,
        current_player_count: server.current_player_count,
        max_player_count: server.max_player_count,
        listen_address: format!("{}:{}", server.listen_host, server.listen_port),
        minecraft_version: server.minecraft_version,
        world_name: server.world_name,
        gamemode: server.gamemode,
        difficulty: server.difficulty,
        motd: server.motd,
        owner_display_name: server.owner_display_name,
    };

    Ok((
        format!("Minecraft server summary for \"{}\"", summary.display_name),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn network_get_topology_summary(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let snapshot =
        crate::network_diagnostics::collect_network_topology_snapshot(state, context.is_admin)
            .await;

    Ok((
        "Rustyfin network topology summary".to_string(),
        serde_json::to_value(snapshot).unwrap_or_else(|_| json!({})),
    ))
}

async fn calendar_list_events(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 7);
    let events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from,
        &to,
    )
    .await
    .map_err(|e| format!("failed to load upcoming calendar events: {e}"))?;

    let events: Vec<_> = events
        .into_iter()
        .take(12)
        .map(|event| CalendarEventSummary {
            title: event.title,
            event_date: event.event_date,
            scope: event.scope,
            event_type: event.event_type,
            owner_username: event.owner_username,
        })
        .collect();

    Ok((
        format!("Visible calendar events for {label}"),
        json!({
            "window": {
                "from": from,
                "to": to,
                "label": label,
            },
            "events": events,
        }),
    ))
}

async fn calendar_get_next_event(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let next_event = rustfin_db::repo::calendar::find_next_visible_event(
        &state.db,
        &context.user_id,
        context.is_admin,
        assistant_local_today(),
    )
    .await
    .map_err(|e| format!("failed to load the next visible calendar event: {e}"))?;

    let next_event = next_event.map(|row| NextCalendarEventSummary {
        id: row.event.id,
        title: row.event.title,
        event_date: row.event.event_date,
        event_type: row.event.event_type,
        scope: row.event.scope,
        owner_username: row.event.owner_username,
        recurrence: row.event.recurrence,
        next_occurs_on: row.next_occurs_on,
    });

    Ok((
        "Next visible calendar event".to_string(),
        json!({
            "label": "Next visible calendar event",
            "next_event": next_event,
        }),
    ))
}

async fn calendar_get_event_details(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 30);
    let query = calendar_query_for_call(call)
        .map(|query| normalize_calendar_event_query(&query))
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "missing calendar event query".to_string())?;

    let visible_events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from,
        &to,
    )
    .await
    .map_err(|e| format!("failed to load visible calendar events: {e}"))?;

    let matching_event = visible_events
        .into_iter()
        .find(|event| calendar_event_matches_query(event, &query))
        .ok_or_else(|| format!("no visible calendar event matched \"{query}\" in {label}"))?;

    let event = rustfin_db::repo::calendar::get_event(&state.db, &matching_event.id)
        .await
        .map_err(|e| format!("failed to load calendar event details: {e}"))?
        .ok_or_else(|| {
            format!(
                "calendar event {} is no longer available",
                matching_event.id
            )
        })?;

    let is_birthday = event.event_type == "birthday";
    let summary = CalendarEventDetailSummary {
        id: event.id,
        title: event.title,
        description: event.description,
        event_date: event.event_date.clone(),
        scope: event.scope,
        event_type: event.event_type,
        recurrence: event.recurrence,
        owner_username: event.owner_username,
        created_by_username: event.created_by_username,
        birthday_year: event.birthday_year,
        month_day_display: is_birthday.then(|| birthday_month_day_display(&event.event_date)),
        next_occurs_on: is_birthday.then(|| next_birthday_occurrence(&event.event_date)),
    };

    Ok((
        format!("Calendar event details for \"{}\"", summary.title),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn calendar_create_event(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::CalendarCreateEvent {
        scope,
        title,
        description,
        event_date,
    } = &call.input
    else {
        return Err("missing calendar event payload".to_string());
    };

    let title = validate_calendar_title(title)?;
    let event_date = validate_calendar_date(event_date)?;
    let owner_user_id = calendar_owner_for_scope(context, scope)?;

    let created = rustfin_db::repo::calendar::create_event(
        &state.db,
        &rustfin_db::repo::calendar::NewCalendarEvent {
            scope: scope.clone(),
            owner_user_id,
            title,
            description: normalize_calendar_optional_text(description.as_deref()),
            event_date: event_date.format("%F").to_string(),
            event_type: "event".to_string(),
            recurrence: "none".to_string(),
            birthday_year: None,
            created_by_user_id: context.user_id.clone(),
        },
    )
    .await
    .map_err(|e| format!("failed to create the calendar event: {e}"))?;

    let summary =
        verify_created_calendar_event(state, context, &created.id, "event", "none", None).await?;

    Ok((
        format!("Created calendar event \"{}\"", summary.title),
        json!({
            "verified": true,
            "event": summary,
        }),
    ))
}

async fn calendar_create_birthday(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::CalendarCreateBirthday {
        scope,
        title,
        description,
        event_date,
        birthday_year,
    } = &call.input
    else {
        return Err("missing calendar birthday payload".to_string());
    };

    let title = validate_calendar_title(title)?;
    let event_date = validate_calendar_date(event_date)?;
    validate_calendar_birthday_year(*birthday_year)?;
    if event_date.year() != *birthday_year {
        return Err("birthday event_date must use the same year as birthday_year".to_string());
    }
    let owner_user_id = calendar_owner_for_scope(context, scope)?;

    let created = rustfin_db::repo::calendar::create_event(
        &state.db,
        &rustfin_db::repo::calendar::NewCalendarEvent {
            scope: scope.clone(),
            owner_user_id,
            title,
            description: normalize_calendar_optional_text(description.as_deref()),
            event_date: event_date.format("%F").to_string(),
            event_type: "birthday".to_string(),
            recurrence: "yearly".to_string(),
            birthday_year: Some(*birthday_year),
            created_by_user_id: context.user_id.clone(),
        },
    )
    .await
    .map_err(|e| format!("failed to create the calendar birthday: {e}"))?;

    let summary = verify_created_calendar_event(
        state,
        context,
        &created.id,
        "birthday",
        "yearly",
        Some(*birthday_year),
    )
    .await?;

    Ok((
        format!("Created recurring birthday for \"{}\"", summary.title),
        json!({
            "verified": true,
            "event": summary,
        }),
    ))
}

async fn calendar_delete_event(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::CalendarDeleteEvent {
        event_id,
        title,
        event_date,
        scope,
        event_type,
        recurrence,
    } = &call.input
    else {
        return Err("missing calendar delete payload".to_string());
    };

    let existing = rustfin_db::repo::calendar::get_event(&state.db, event_id)
        .await
        .map_err(|e| format!("failed to reload the calendar event to delete: {e}"))?
        .ok_or_else(|| "That calendar event is no longer available.".to_string())?;

    if !calendar_event_can_manage(context, &existing) {
        return Err(
            "Your Rustyfin account is not allowed to delete that calendar entry.".to_string(),
        );
    }

    if existing.title != *title
        || existing.event_date != *event_date
        || existing.scope != *scope
        || existing.event_type != *event_type
        || existing.recurrence != *recurrence
    {
        return Err(
            "That calendar entry changed after the confirmation card was issued. Ask Rustyfin AI to prepare the delete again."
                .to_string(),
        );
    }

    let deleted = rustfin_db::repo::calendar::delete_event(&state.db, event_id)
        .await
        .map_err(|e| format!("failed to delete the calendar event: {e}"))?;
    if !deleted {
        return Err("That calendar event is no longer available.".to_string());
    }

    let summary = verify_deleted_calendar_event(state, context, &existing).await?;

    Ok((
        format!("Deleted calendar event \"{}\"", summary.title),
        json!({
            "verified": true,
            "event": summary,
        }),
    ))
}

async fn document_create_download(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::DocumentCreateDownload {
        title,
        file_name,
        format,
        request_prompt,
        model_name,
    } = &call.input
    else {
        return Err("missing downloadable document payload".to_string());
    };

    let format = GeneratedDocumentOutputFormat::parse(format)?;
    let title = validate_document_title(title)?;
    let file_name = validate_document_file_name(file_name, format)?;
    let request_prompt = validate_document_request_prompt(request_prompt)?;
    let model_name = validate_document_model_name(model_name)?;
    let history = load_document_generation_history(state, context).await?;

    let model_dir = current_model_dir(state).await;
    let gguf_path = model_file_path(&model_dir, &model_name)
        .map_err(|error| format!("invalid AI model selection for document generation: {error}"))?;
    if !gguf_path.exists() {
        return Err(format!(
            "The selected AI model \"{model_name}\" is not installed on this host."
        ));
    }

    let (engine, _, _) =
        crate::ai_enabled::load_engine_for_chat(state, &model_name, &gguf_path).await?;
    let auth_user = AuthUser {
        user_id: context.user_id.clone(),
        username: context.username.clone(),
        role: context.role.clone(),
    };
    let planned =
        plan_tool_calls_with_model_assist(&engine, &auth_user, &request_prompt, &history).await;
    let planned = planned
        .calls
        .into_iter()
        .filter(|planned| planned.tool.spec().access_mode == ToolAccessMode::ReadOnly)
        .collect::<Vec<_>>();

    let nested_context =
        AssistantContext::new(&auth_user, format!("{}:document", context.trace_id))
            .with_conversation_id(context.conversation_id.as_deref());
    let grounding_blocks = join_all(planned.iter().cloned().map(|nested_call| {
        let nested_context = nested_context.clone();
        async move { execute_tool(state, &nested_context, &nested_call).await }
    }))
    .await;

    let messages =
        build_document_generation_messages(format, &request_prompt, &history, &grounding_blocks);
    let content = collect_generated_document_text(&engine, messages)
        .await
        .and_then(|content| finalize_generated_document_content(&content))?;
    let byte_size = i64::try_from(content.as_bytes().len())
        .map_err(|_| "generated document is too large to store".to_string())?;

    let artifact = rustfin_db::repo::ai_generated_artifacts::create_artifact(
        &state.db,
        rustfin_db::repo::ai_generated_artifacts::CreateAiGeneratedArtifactParams {
            user_id: &context.user_id,
            conversation_id: context.conversation_id.as_deref(),
            title: &title,
            file_name: &file_name,
            media_type: format.media_type(),
            content_text: &content,
            byte_size,
            trace_id: Some(context.trace_id.as_str()),
        },
    )
    .await
    .map_err(|error| format!("failed to store generated document: {error}"))?;

    let artifact = GeneratedDocumentArtifactSummary {
        id: artifact.id.clone(),
        title: artifact.title,
        file_name: artifact.file_name,
        media_type: artifact.media_type,
        byte_size: artifact.byte_size,
        download_path: artifact_download_path(&artifact.id),
    };

    Ok((
        format!(
            "Created downloadable {} document \"{}\"",
            format.label(),
            file_name
        ),
        json!({
            "verified": true,
            "artifact": artifact,
        }),
    ))
}

#[derive(Clone, Copy)]
enum GeneratedDocumentOutputFormat {
    Markdown,
    Text,
}

impl GeneratedDocumentOutputFormat {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "markdown" => Ok(Self::Markdown),
            "text" => Ok(Self::Text),
            other => Err(format!(
                "unsupported document format \"{other}\"; only markdown and text are available"
            )),
        }
    }

    const fn media_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown; charset=utf-8",
            Self::Text => "text/plain; charset=utf-8",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "plain-text",
        }
    }
}

fn validate_document_title(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("generated document title is required".to_string());
    }
    Ok(trimmed.chars().take(80).collect())
}

fn validate_document_file_name(
    raw: &str,
    format: GeneratedDocumentOutputFormat,
) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("generated document file name is required".to_string());
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("generated document file name must not contain path separators".to_string());
    }

    let mut normalized = trimmed.to_string();
    if !normalized.ends_with(".md") && !normalized.ends_with(".txt") {
        normalized.push('.');
        normalized.push_str(format.extension());
    }
    Ok(normalized.chars().take(96).collect())
}

fn validate_document_request_prompt(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("generated document prompt is required".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_document_model_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("generated document model name is required".to_string());
    }
    Ok(trimmed.to_string())
}

async fn load_document_generation_history(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<AssistantHistoryMessage>, String> {
    let Some(conversation_id) = context.conversation_id.as_deref() else {
        return Ok(Vec::new());
    };
    let (_, _, history) = crate::ai_conversations::load_conversation_request_context(
        state,
        &context.user_id,
        conversation_id,
    )
    .await
    .map_err(|error| {
        format!("failed to load conversation history for document generation: {error:?}")
    })?;
    Ok(trim_document_generation_history(&history))
}

fn trim_document_generation_history(
    history: &[AssistantHistoryMessage],
) -> Vec<AssistantHistoryMessage> {
    let len = history.len();
    let start = len.saturating_sub(8);
    history[start..].to_vec()
}

fn build_document_generation_messages(
    format: GeneratedDocumentOutputFormat,
    request_prompt: &str,
    history: &[AssistantHistoryMessage],
    grounding_blocks: &[AssistantToolContextBlock],
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "You generate downloadable Rustyfin user documents. Return only the document body with no surrounding commentary, no markdown code fences, and no explanation about downloads. Produce a {} document.",
            format.label()
        ),
    }];

    messages.push(ChatMessage {
        role: "system".to_string(),
        content: format!(
            "Current Rustyfin host local date/time for this document: {}.",
            assistant_local_now().format("%Y-%m-%d %H:%M:%S %:z (%A)")
        ),
    });

    if !grounding_blocks.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding for this document:\n{}",
                serde_json::to_string(grounding_blocks).unwrap_or_else(|_| "[]".to_string())
            ),
        });
    }

    for history_message in history {
        messages.push(ChatMessage {
            role: history_message.role.clone(),
            content: history_message.content.clone(),
        });
    }

    let format_instruction = match format {
        GeneratedDocumentOutputFormat::Markdown => {
            "Write a clean markdown document with useful headings and concise detail."
        }
        GeneratedDocumentOutputFormat::Text => {
            "Write a clean plain-text document with readable section breaks."
        }
    };
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: format!(
            "{format_instruction}\nUser request for the downloadable document:\n{}",
            request_prompt.trim()
        ),
    });

    messages
}

async fn collect_generated_document_text(
    engine: &rustfin_ai_agent::LlamaEngine,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    let mut output = String::new();
    let stream = engine.chat_stream(
        messages,
        SamplingParams {
            temperature: 0.2,
            top_p: 0.9,
            top_k: 30,
            repeat_penalty: 1.05,
            max_tokens: 1200,
            max_duration_ms: None,
        },
    );
    futures::pin_mut!(stream);

    while let Some(chunk) = stream.next().await {
        match chunk.map_err(|error| format!("document generation failed: {error}"))? {
            ChatChunk::Token(text) => output.push_str(&text),
            ChatChunk::Done => break,
            ChatChunk::Stats { .. } => {}
        }
    }

    Ok(output)
}

fn finalize_generated_document_content(content: &str) -> Result<String, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("the AI generated an empty document".to_string());
    }

    let normalized = trimmed.replace("\r\n", "\n");
    if normalized.len() > 64_000 {
        return Err("the generated document exceeded the maximum allowed size".to_string());
    }
    Ok(normalized)
}

fn validate_calendar_title(raw: &str) -> Result<String, String> {
    let title = raw.trim();
    if title.is_empty() {
        return Err("calendar title cannot be empty".to_string());
    }
    if title.chars().count() > 140 {
        return Err("calendar title must be 140 characters or fewer".to_string());
    }
    Ok(title.to_string())
}

fn normalize_calendar_optional_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_calendar_date(raw: &str) -> Result<chrono::NaiveDate, String> {
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| "calendar event_date must use YYYY-MM-DD".to_string())
}

fn validate_calendar_birthday_year(year: i32) -> Result<(), String> {
    let current_year = assistant_local_year();
    if !(1900..=current_year).contains(&year) {
        return Err(format!(
            "birthday_year must be between 1900 and {current_year}"
        ));
    }
    Ok(())
}

fn calendar_owner_for_scope(
    context: &AssistantContext,
    scope: &str,
) -> Result<Option<String>, String> {
    match scope {
        "personal" => Ok(Some(context.user_id.clone())),
        "global" if context.is_admin => Ok(None),
        "global" => Err("only admins can create shared calendar entries".to_string()),
        _ => Err(format!("unsupported calendar scope \"{scope}\"")),
    }
}

async fn verify_created_calendar_event(
    state: &AppState,
    context: &AssistantContext,
    event_id: &str,
    expected_event_type: &str,
    expected_recurrence: &str,
    expected_birthday_year: Option<i32>,
) -> Result<CalendarEventDetailSummary, String> {
    let event = rustfin_db::repo::calendar::get_event(&state.db, event_id)
        .await
        .map_err(|e| format!("failed to reload the created calendar event: {e}"))?
        .ok_or_else(|| format!("created calendar event {event_id} is missing"))?;

    let visible_events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &event.event_date,
        &event.event_date,
    )
    .await
    .map_err(|e| format!("failed to verify the created calendar event: {e}"))?;

    if !visible_events
        .iter()
        .any(|candidate| candidate.id == event_id)
    {
        return Err(
            "Rustyfin created the calendar entry, but it could not verify that the entry is visible through the normal calendar read path."
                .to_string(),
        );
    }

    if event.event_type != expected_event_type {
        return Err(format!(
            "calendar verification failed because event_type was {} instead of {}",
            event.event_type, expected_event_type
        ));
    }
    if event.recurrence != expected_recurrence {
        return Err(format!(
            "calendar verification failed because recurrence was {} instead of {}",
            event.recurrence, expected_recurrence
        ));
    }
    if event.birthday_year != expected_birthday_year {
        return Err("calendar verification failed because birthday_year did not match".to_string());
    }

    let is_birthday = event.event_type == "birthday";
    Ok(CalendarEventDetailSummary {
        id: event.id,
        title: event.title,
        description: event.description,
        event_date: event.event_date.clone(),
        scope: event.scope,
        event_type: event.event_type,
        recurrence: event.recurrence,
        owner_username: event.owner_username,
        created_by_username: event.created_by_username,
        birthday_year: event.birthday_year,
        month_day_display: is_birthday.then(|| birthday_month_day_display(&event.event_date)),
        next_occurs_on: is_birthday.then(|| next_birthday_occurrence(&event.event_date)),
    })
}

async fn verify_deleted_calendar_event(
    state: &AppState,
    context: &AssistantContext,
    deleted_event: &rustfin_db::repo::calendar::CalendarEventRow,
) -> Result<CalendarEventDetailSummary, String> {
    if rustfin_db::repo::calendar::get_event(&state.db, &deleted_event.id)
        .await
        .map_err(|e| format!("failed to reload the deleted calendar event: {e}"))?
        .is_some()
    {
        return Err(
            "Rustyfin deleted the calendar entry, but it could not verify that the entry disappeared from the normal calendar read path."
                .to_string(),
        );
    }

    let visible_events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &deleted_event.event_date,
        &deleted_event.event_date,
    )
    .await
    .map_err(|e| format!("failed to verify the deleted calendar event: {e}"))?;

    if visible_events
        .iter()
        .any(|candidate| candidate.id == deleted_event.id)
    {
        return Err(
            "Rustyfin deleted the calendar entry, but it could not verify that the entry disappeared from the normal calendar read path."
                .to_string(),
        );
    }

    let is_birthday = deleted_event.event_type == "birthday";
    Ok(CalendarEventDetailSummary {
        id: deleted_event.id.clone(),
        title: deleted_event.title.clone(),
        description: deleted_event.description.clone(),
        event_date: deleted_event.event_date.clone(),
        scope: deleted_event.scope.clone(),
        event_type: deleted_event.event_type.clone(),
        recurrence: deleted_event.recurrence.clone(),
        owner_username: deleted_event.owner_username.clone(),
        created_by_username: deleted_event.created_by_username.clone(),
        birthday_year: deleted_event.birthday_year,
        month_day_display: is_birthday
            .then(|| birthday_month_day_display(&deleted_event.event_date)),
        next_occurs_on: is_birthday.then(|| next_birthday_occurrence(&deleted_event.event_date)),
    })
}

fn calendar_event_can_manage(
    context: &AssistantContext,
    event: &rustfin_db::repo::calendar::CalendarEventRow,
) -> bool {
    context.is_admin
        || (event.scope == "personal"
            && event.owner_user_id.as_deref() == Some(context.user_id.as_str()))
}

async fn channels_list_unread_activity(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let query = channels_query_for_call(call);
    let channels = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .map_err(|e| format!("failed to load channels: {e}"))?;
    let accessible_channels: Vec<_> = channels
        .into_iter()
        .filter(|channel| !channel.is_private || context.is_admin)
        .filter(|channel| channel_matches_query(channel, query.as_deref()))
        .collect();

    let mut activity = Vec::new();
    let before_ts = Utc::now().timestamp().saturating_add(1);
    for channel in accessible_channels.iter().take(12) {
        let messages =
            rustfin_db::repo::channels::list_messages(&state.db, &channel.id, 1, before_ts, None)
                .await
                .map_err(|e| format!("failed to load channel activity: {e}"))?;
        let latest_message = messages
            .last()
            .map(|message| ChannelActivityMessageSummary {
                username: message.username.clone(),
                content_preview: truncate_preview(&message.content, 140),
                created_ts: message.created_ts,
            });
        activity.push(ChannelActivitySummary {
            channel_id: channel.id.clone(),
            name: channel.name.clone(),
            kind: channel.kind.clone(),
            is_private: channel.is_private,
            latest_message,
        });
    }

    activity.sort_by(|left, right| {
        let left_ts = left
            .latest_message
            .as_ref()
            .map(|message| message.created_ts)
            .unwrap_or_default();
        let right_ts = right
            .latest_message
            .as_ref()
            .map(|message| message.created_ts)
            .unwrap_or_default();
        right_ts
            .cmp(&left_ts)
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok((
        match query.as_deref() {
            Some(query) => format!("Recent visible channel activity matching \"{query}\""),
            None => "Recent visible channel activity".to_string(),
        },
        json!({
            "unread_tracking_available": false,
            "query": query,
            "total_count": activity.len(),
            "channels": activity,
        }),
    ))
}

async fn channels_get_transcript_summary(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let query = channels_query_for_call(call);
    let channel_query = query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(str::to_string);

    let channels = rustfin_db::repo::channels::list_channels(&state.db)
        .await
        .map_err(|e| format!("failed to load channels: {e}"))?;
    let accessible_voice_channels: Vec<_> = channels
        .into_iter()
        .filter(|channel| channel.kind.eq_ignore_ascii_case("voice"))
        .filter(|channel| !channel.is_private || context.is_admin)
        .filter(|channel| channel_matches_query(channel, channel_query.as_deref()))
        .collect();

    if accessible_voice_channels.is_empty() {
        return Err(channel_query
            .as_deref()
            .map(|query| format!("no accessible voice channel matched \"{query}\""))
            .unwrap_or_else(|| "no accessible voice channels are available".to_string()));
    }

    let mut selected: Option<(
        rustfin_db::repo::channels::ChannelRow,
        rustfin_db::repo::channel_transcripts::TranscriptSessionRow,
    )> = None;
    let mut latest_non_completed: Option<(
        rustfin_db::repo::channels::ChannelRow,
        rustfin_db::repo::channel_transcripts::TranscriptSessionRow,
    )> = None;

    for channel in accessible_voice_channels {
        let sessions = rustfin_db::repo::channel_transcripts::list_sessions_for_channel(
            &state.db,
            &channel.id,
            8,
        )
        .await
        .map_err(|e| format!("failed to load transcript sessions: {e}"))?;

        if let Some(session) = sessions
            .iter()
            .find(|session| session.status == "completed")
        {
            let replace = selected
                .as_ref()
                .map(|(_, current)| session.started_ts > current.started_ts)
                .unwrap_or(true);
            if replace {
                selected = Some((channel.clone(), session.clone()));
            }
        } else if let Some(session) = sessions.first() {
            let replace = latest_non_completed
                .as_ref()
                .map(|(_, current)| session.started_ts > current.started_ts)
                .unwrap_or(true);
            if replace {
                latest_non_completed = Some((channel.clone(), session.clone()));
            }
        }
    }

    let Some((channel, session)) = selected else {
        if let Some((channel, session)) = latest_non_completed {
            return Err(match session.status.as_str() {
                "running" | "finalizing" => format!(
                    "a transcript exists for voice channel \"{}\", but it is still {}.",
                    channel.name, session.status
                ),
                "failed" => format!(
                    "the latest transcript for voice channel \"{}\" failed: {}",
                    channel.name,
                    session
                        .failure_reason
                        .unwrap_or_else(|| "unknown reason".to_string())
                ),
                "cancelled" => format!(
                    "the latest transcript for voice channel \"{}\" was cancelled and has no saved summary.",
                    channel.name
                ),
                _ => format!(
                    "no completed transcript is available yet for voice channel \"{}\".",
                    channel.name
                ),
            });
        }
        return Err(channel_query
            .as_deref()
            .map(|query| format!("no completed transcript was found for \"{query}\""))
            .unwrap_or_else(|| "no completed call transcripts were found yet".to_string()));
    };

    let entries =
        rustfin_db::repo::channel_transcripts::list_entries_for_session(&state.db, &session.id)
            .await
            .map_err(|e| format!("failed to load transcript entries: {e}"))?;
    if entries.is_empty() {
        return Err(format!(
            "the transcript for voice channel \"{}\" has no saved transcript lines yet.",
            channel.name
        ));
    }

    let summary = summarize_transcript_session(&channel, &session, &entries);
    Ok((
        format!("Transcript summary for \"{}\"", summary.channel_name),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_host_runtime_summary(
    state: &AppState,
    _context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let diagnostics = crate::runtime_diagnostics::collect_runtime_diagnostics(state).await;
    let summary = HostRuntimeAssistantSummary {
        memory: diagnostics
            .host
            .used_memory_bytes
            .zip(diagnostics.host.total_memory_bytes)
            .map(
                |(used_memory_bytes, total_memory_bytes)| HostRuntimeMemorySummary {
                    used_memory_bytes,
                    total_memory_bytes,
                    used_memory_gib: bytes_to_gib(used_memory_bytes),
                    total_memory_gib: bytes_to_gib(total_memory_bytes),
                    used_memory_human: humanize_binary_bytes(used_memory_bytes),
                    total_memory_human: humanize_binary_bytes(total_memory_bytes),
                    memory_summary: format!(
                        "The server is currently using {} of RAM out of {}.",
                        humanize_binary_bytes(used_memory_bytes),
                        humanize_binary_bytes(total_memory_bytes)
                    ),
                },
            ),
        swap: diagnostics
            .host
            .used_swap_bytes
            .zip(diagnostics.host.total_swap_bytes)
            .map(
                |(used_swap_bytes, total_swap_bytes)| HostRuntimeSwapSummary {
                    used_swap_bytes,
                    total_swap_bytes,
                    used_swap_gib: bytes_to_gib(used_swap_bytes),
                    total_swap_gib: bytes_to_gib(total_swap_bytes),
                    used_swap_human: humanize_binary_bytes(used_swap_bytes),
                    total_swap_human: humanize_binary_bytes(total_swap_bytes),
                    swap_summary: format!(
                        "The server is currently using {} of swap out of {}.",
                        humanize_binary_bytes(used_swap_bytes),
                        humanize_binary_bytes(total_swap_bytes)
                    ),
                },
            ),
        host: diagnostics.host,
        rustyfin: HostRuntimeRustyfinSummary {
            uptime_seconds: diagnostics.runtime.uptime_seconds,
            active_jobs: diagnostics.runtime.jobs.total.active_running,
            active_transcode_sessions: diagnostics.transcoding.active_sessions,
            active_channels_websockets: diagnostics.runtime.websockets.channels.active,
            active_watch_party_websockets: diagnostics.runtime.websockets.watch_party.active,
            ai_chat_requests_in_flight: diagnostics.runtime.assistant.chats.calls_in_flight,
            ai_tool_calls_in_flight: diagnostics.runtime.assistant.tools.calls_in_flight,
        },
    };

    Ok((
        "Rustyfin host runtime summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_current_datetime(
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let requested_location = match &call.input {
        AssistantToolInput::CurrentDateTime { location } => location.clone(),
        AssistantToolInput::None => None,
        _ => None,
    };

    let (label, summary) = if let Some(location) = requested_location {
        let resolved = resolve_public_location_timezone(&location).await?;
        let timezone = resolved.timezone.parse::<Tz>().map_err(|error| {
            format!("failed to parse public timezone for \"{location}\": {error}")
        })?;
        let now = Utc::now().with_timezone(&timezone);
        (
            format!("Current date and time for {}", resolved.resolved_location),
            CurrentDateTimeAssistantSummary {
                local_timestamp: now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
                local_date: now.format("%F").to_string(),
                local_time: now.format("%H:%M:%S").to_string(),
                weekday: now.format("%A").to_string(),
                timezone_offset: now.format("%:z").to_string(),
                timezone_name: Some(resolved.timezone),
                resolved_location: Some(resolved.resolved_location),
                location_query: Some(resolved.location_query),
                unix_timestamp: now.timestamp(),
            },
        )
    } else {
        let now = assistant_local_now();
        (
            format!(
                "Rustyfin host local date and time: {} ({})",
                now.format("%F"),
                now.format("%A")
            ),
            CurrentDateTimeAssistantSummary {
                local_timestamp: now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
                local_date: now.format("%F").to_string(),
                local_time: now.format("%H:%M:%S").to_string(),
                weekday: now.format("%A").to_string(),
                timezone_offset: now.format("%:z").to_string(),
                timezone_name: None,
                resolved_location: None,
                location_query: None,
                unix_timestamp: now.timestamp(),
            },
        )
    };

    Ok((
        label,
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

fn bytes_to_gib(bytes: u64) -> f64 {
    ((bytes as f64 / 1024.0_f64.powi(3)) * 10.0).round() / 10.0
}

fn humanize_binary_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut value = bytes as f64;
    let mut unit_index = 0_usize;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

async fn system_get_backup_summary(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    // Get policy count
    let policies = crate::backups::repo::list_policies(&state.db)
        .await
        .map_err(|e| format!("failed to list backup policies: {e}"))?;

    // Get recent jobs (last 30)
    let jobs = crate::backups::repo::list_jobs(&state.db)
        .await
        .map_err(|e| format!("failed to list backup jobs: {e}"))?;
    let jobs: Vec<_> = jobs.into_iter().take(30).collect();

    let successful_jobs: Vec<_> = jobs.iter().filter(|j| j.status == "completed").collect();
    let failed_jobs: Vec<_> = jobs.iter().filter(|j| j.status == "failed").collect();

    let last_successful_backup_ts = successful_jobs.iter().filter_map(|j| j.end_ts).max();

    let configured = !policies.is_empty();
    let message = if configured {
        if successful_jobs.is_empty() {
            "Backup policies are configured but no successful backups exist yet.".to_string()
        } else {
            format!(
                "{} backup policies configured. {} successful backups, {} failed.",
                policies.len(),
                successful_jobs.len(),
                failed_jobs.len()
            )
        }
    } else {
        "No backup policies are configured on this host.".to_string()
    };

    let summary = BackupAssistantSummary {
        configured,
        restore_supported: true,
        last_successful_backup_ts,
        policy_count: policies.len() as i64,
        total_job_count: jobs.len() as i64,
        successful_job_count: successful_jobs.len() as i64,
        failed_job_count: failed_jobs.len() as i64,
        message,
    };

    Ok((
        "Rustyfin backup capability summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_service_health(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    let mut probes = vec![
        probe_service_health_component(&state.http, "core_api", Some(local_core_health_url())),
        probe_service_health_component(
            &state.http,
            "tmdb_agent",
            Some(health_url_from_base(&state.tmdb_agent_url)),
        ),
        probe_service_health_component(
            &state.http,
            "youtube_agent",
            Some(health_url_from_base(&state.youtube_agent_url)),
        ),
        probe_service_health_component(
            &state.http,
            "transcription_agent",
            Some(health_url_from_base(&state.transcription_agent_url)),
        ),
    ];

    probes.push(probe_service_health_component(
        &state.http,
        "servers_agent",
        state.servers_agent_url.as_deref().map(health_url_from_base),
    ));

    let mut components = futures::future::join_all(probes).await;
    components.push(ServiceHealthComponentSummary {
        name: "rustyvault".to_string(),
        status: if state.rustyvault.available {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        configured: true,
        url: None,
        detail: if state.rustyvault.available {
            "RustyVault runtime is available.".to_string()
        } else {
            state.rustyvault.public_reason().to_string()
        },
    });

    let loaded_model = state.engine.lock().await.loaded_model.clone();
    components.push(ServiceHealthComponentSummary {
        name: "ai_inference".to_string(),
        status: if crate::ai::inference_available() {
            "healthy".to_string()
        } else {
            "disabled".to_string()
        },
        configured: true,
        url: None,
        detail: match (crate::ai::inference_available(), loaded_model) {
            (true, Some(model)) => format!("Inference is available. Loaded model: {model}."),
            (true, None) => "Inference is available. No model is currently loaded.".to_string(),
            (false, _) => "AI inference is unavailable on this host.".to_string(),
        },
    });

    let all_healthy = components.iter().all(|component| {
        !component.configured || matches!(component.status.as_str(), "healthy" | "disabled")
    });
    let summary = ServiceHealthAssistantSummary {
        all_healthy,
        components,
    };

    Ok((
        "Rustyfin service health summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_transcode_summary(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    let active_session_ids = state.transcoder.list_sessions().await;
    let summary = TranscodeAssistantDetailedSummary {
        active_sessions: active_session_ids.len(),
        active_session_ids,
        created_total: state.transcoder.created_total(),
        create_failures_total: state.transcoder.create_failures_total(),
        create_failures_last_minute: state.transcoder.create_failures_last_minute(),
        create_failures_last_five_minutes: state.transcoder.create_failures_last_five_minutes(),
        cleaned_total: state.transcoder.cleaned_total(),
        ffmpeg_path: state.transcoder.ffmpeg_path().display().to_string(),
        ffprobe_path: state.transcoder.ffprobe_path().display().to_string(),
        hw_accel: state
            .transcoder
            .hw_accel()
            .map(|accel| format!("{accel:?}").to_ascii_lowercase()),
        hw_device_path: state
            .transcoder
            .hw_device_path()
            .map(|path| path.display().to_string()),
        hw_accel_required: state.transcoder_hw_accel_required,
    };

    Ok((
        "Rustyfin transcode summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_storage_summary(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    let summary = collect_storage_summary(state).await;
    Ok((
        "Rustyfin storage summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_recent_errors(state: &AppState) -> Result<(String, serde_json::Value), String> {
    let failed_jobs = rustfin_db::repo::jobs::list_jobs_filtered(
        &state.db,
        &["failed", "error"],
        None,
        Some(8),
        None,
    )
    .await
    .map_err(|e| format!("failed to load recent failed jobs: {e}"))?;
    let recent_failed_jobs = failed_jobs
        .into_iter()
        .map(|job| RecentErrorItemSummary {
            source: "job".to_string(),
            kind: job.kind,
            occurred_ts: Some(job.updated_ts),
            message: job
                .error
                .filter(|message| !message.trim().is_empty())
                .unwrap_or_else(|| format!("job {} ended with status {}", job.id, job.status)),
        })
        .collect::<Vec<_>>();

    let runtime = state.runtime_metrics.snapshot();
    let summary = RecentErrorsAssistantSummary {
        recent_failed_jobs,
        transcode_failures: RuntimeFailureWindowSummary {
            failures_last_minute: state.transcoder.create_failures_last_minute(),
            failures_last_five_minutes: state.transcoder.create_failures_last_five_minutes(),
        },
        agent_failures: HashMap::from([
            (
                "servers".to_string(),
                RuntimeFailureWindowSummary {
                    failures_last_minute: runtime.agents.servers.failures_last_minute,
                    failures_last_five_minutes: runtime.agents.servers.failures_last_five_minutes,
                },
            ),
            (
                "tmdb".to_string(),
                RuntimeFailureWindowSummary {
                    failures_last_minute: runtime.agents.tmdb.failures_last_minute,
                    failures_last_five_minutes: runtime.agents.tmdb.failures_last_five_minutes,
                },
            ),
            (
                "transcription".to_string(),
                RuntimeFailureWindowSummary {
                    failures_last_minute: runtime.agents.transcription.failures_last_minute,
                    failures_last_five_minutes: runtime
                        .agents
                        .transcription
                        .failures_last_five_minutes,
                },
            ),
            (
                "youtube".to_string(),
                RuntimeFailureWindowSummary {
                    failures_last_minute: runtime.agents.youtube.failures_last_minute,
                    failures_last_five_minutes: runtime.agents.youtube.failures_last_five_minutes,
                },
            ),
            (
                "assistant".to_string(),
                RuntimeFailureWindowSummary {
                    failures_last_minute: runtime.assistant.tools.failures_last_minute,
                    failures_last_five_minutes: runtime.assistant.tools.failures_last_five_minutes,
                },
            ),
        ]),
    };

    Ok((
        "Recent Rustyfin failures and errors".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn calendar_upcoming_birthdays(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 30);
    let birthday_query = calendar_query_for_call(call);
    let events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from,
        &to,
    )
    .await
    .map_err(|e| format!("failed to load upcoming birthdays: {e}"))?;

    let all_birthdays: Vec<_> = events
        .into_iter()
        .filter(|event| event.event_type == "birthday")
        .collect();
    let total_count = all_birthdays.len();
    let birthdays: Vec<_> = all_birthdays
        .into_iter()
        .filter(|event| {
            birthday_query
                .as_deref()
                .is_none_or(|query| birthday_matches_query(event, query))
        })
        .take(12)
        .map(|event| BirthdaySummary {
            month_day_display: birthday_month_day_display(&event.event_date),
            next_occurs_on: next_birthday_occurrence(&event.event_date),
            birthday_year: event.birthday_year,
            title: event.title,
            event_date: event.event_date,
            scope: event.scope,
            owner_username: event.owner_username,
        })
        .collect();
    let match_count = birthdays.len();

    Ok((
        match birthday_query.as_deref() {
            Some(query) => format!("Birthdays matching \"{query}\" for {label}"),
            None => format!("Upcoming birthdays for {label}"),
        },
        json!({
            "window": {
                "from": from,
                "to": to,
                "label": label,
            },
            "query": birthday_query,
            "match_count": match_count,
            "total_count": total_count,
            "birthdays": birthdays,
        }),
    ))
}

async fn libraries_get_recently_added(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let query = library_recent_query_for_call(call);
    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(
            rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
                .await
                .map_err(|e| format!("failed to load library permissions: {e}"))?,
        )
    };

    let items = rustfin_db::repo::items::list_recent_items(
        &state.db,
        allowed_library_ids.as_deref(),
        query.as_deref(),
        10,
    )
    .await
    .map_err(|e| format!("failed to load recently added library items: {e}"))?;

    let library_names = rustfin_db::repo::libraries::list_libraries(&state.db)
        .await
        .map_err(|e| format!("failed to load libraries: {e}"))?
        .into_iter()
        .map(|library| (library.id, library.name))
        .collect::<HashMap<_, _>>();

    let recent_items: Vec<_> = items
        .into_iter()
        .map(|item| LibraryRecentItemSummary {
            id: item.id,
            title: item.title,
            kind: item.kind,
            year: item.year,
            library_name: library_names.get(&item.library_id).cloned(),
            created_ts: item.created_ts,
        })
        .collect();

    Ok((
        match query.as_deref() {
            Some(query) => format!("Recently added library items matching \"{query}\""),
            None => "Recently added library items".to_string(),
        },
        json!({
            "query": query,
            "total_count": recent_items.len(),
            "items": recent_items,
        }),
    ))
}

async fn rooms_list_joinable(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let room_mode_filter = room_mode_filter_for_call(call);
    let room_query_filter = room_query_filter_for_call(call);
    let public_rooms = rustfin_db::repo::watch_party::list_public_rooms(&state.db)
        .await
        .map_err(|e| format!("failed to load public rooms: {e}"))?;
    let invites = rustfin_db::repo::watch_party::list_invites_for_user(&state.db, &context.user_id)
        .await
        .map_err(|e| format!("failed to load room invites: {e}"))?;

    let mut rooms = Vec::new();
    let mut seen = HashSet::new();

    for room in public_rooms {
        if room_mode_filter
            .as_deref()
            .is_some_and(|room_mode| room.room_mode != room_mode)
        {
            continue;
        }
        if !public_room_matches_query(&room, room_query_filter.as_deref()) {
            continue;
        }
        seen.insert(room.id.clone());
        rooms.push(JoinableRoomSummary {
            room_id: room.id.clone(),
            title: room_title_for_listing(
                &room.room_name,
                &room.room_mode,
                &room.audio_source,
                &room.item_title,
                &room.audio_library_name,
                &room.web_url,
            ),
            room_mode: room.room_mode,
            host_username: room.host_username,
            password_required: room.password_required,
            joinable_via: "public_lobby".to_string(),
            member_count: Some(room.member_count),
        });
    }

    for invite in invites {
        if !seen.insert(invite.room_id.clone()) {
            continue;
        }
        if !invite_matches_room_mode(&invite, room_mode_filter.as_deref()) {
            continue;
        }
        if !invite_matches_query(&invite, room_query_filter.as_deref()) {
            continue;
        }
        rooms.push(JoinableRoomSummary {
            room_id: invite.room_id,
            title: invite.item_title,
            room_mode: "invite".to_string(),
            host_username: invite.host_username,
            password_required: invite.password_required,
            joinable_via: "invite".to_string(),
            member_count: None,
        });
    }

    Ok((
        match room_mode_filter.as_deref() {
            Some(mode) => format!("Joinable {} rooms", room_mode_label(mode)),
            None => "Joinable rooms".to_string(),
        },
        json!({
            "room_mode": room_mode_filter,
            "query": room_query_filter,
            "total_count": rooms.len(),
            "rooms": rooms,
        }),
    ))
}

fn calendar_window_for_call(
    call: &PlannedToolCall,
    fallback_days: i64,
) -> (String, String, String) {
    match &call.input {
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
            ..
        } => (from_date.clone(), to_date.clone(), label.clone()),
        AssistantToolInput::CalendarCreateEvent { event_date, .. }
        | AssistantToolInput::CalendarCreateBirthday { event_date, .. }
        | AssistantToolInput::CalendarDeleteEvent { event_date, .. } => (
            event_date.clone(),
            event_date.clone(),
            "the calendar event".to_string(),
        ),
        AssistantToolInput::None
        | AssistantToolInput::CurrentDateTime { .. }
        | AssistantToolInput::ChannelsFilter { .. }
        | AssistantToolInput::DownloadsFilter { .. }
        | AssistantToolInput::DocumentCreateDownload { .. }
        | AssistantToolInput::LibrarySearch { .. }
        | AssistantToolInput::LibraryRecent { .. }
        | AssistantToolInput::Weather { .. }
        | AssistantToolInput::WeatherHistory { .. }
        | AssistantToolInput::WebSearch { .. }
        | AssistantToolInput::WebFetch { .. }
        | AssistantToolInput::RoomsFilter { .. }
        | AssistantToolInput::ServerFilter { .. } => {
            let from = assistant_local_today();
            let to = from + Duration::days(fallback_days);
            (
                from.format("%F").to_string(),
                to.format("%F").to_string(),
                format!("the next {fallback_days} days"),
            )
        }
    }
}

fn calendar_query_for_call(call: &PlannedToolCall) -> Option<String> {
    match &call.input {
        AssistantToolInput::CalendarWindow { query, .. } => query.clone(),
        _ => None,
    }
}

fn normalize_calendar_event_query(query: &str) -> String {
    let trimmed = query.trim();
    if let Some((title, _date)) = trimmed.rsplit_once(" (")
        && trimmed.ends_with(')')
    {
        return title.trim().to_string();
    }
    trimmed.to_string()
}

fn calendar_event_matches_query(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    query: &str,
) -> bool {
    let normalized_query = normalize_calendar_event_query(query).to_ascii_lowercase();
    if normalized_query.is_empty() {
        return true;
    }

    [
        Some(event.title.as_str()),
        event.description.as_deref(),
        event.owner_username.as_deref(),
        event.created_by_username.as_deref(),
        Some(event.event_date.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .any(|value| value.contains(&normalized_query))
}

fn birthday_matches_query(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    query: &str,
) -> bool {
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return true;
    }

    [
        Some(event.title.as_str()),
        event.owner_username.as_deref(),
        event.description.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_ascii_lowercase())
    .any(|value| value.contains(&normalized_query))
}

fn birthday_month_day_display(event_date: &str) -> String {
    chrono::NaiveDate::parse_from_str(event_date, "%Y-%m-%d")
        .map(|date| date.format("%B %-d").to_string())
        .unwrap_or_else(|_| event_date.to_string())
}

fn next_birthday_occurrence(event_date: &str) -> String {
    let today = assistant_local_today();
    let Ok(date) = chrono::NaiveDate::parse_from_str(event_date, "%Y-%m-%d") else {
        return event_date.to_string();
    };

    for year in [
        today.year(),
        today.year() + 1,
        today.year() + 2,
        today.year() + 3,
    ] {
        if let Some(candidate) = chrono::NaiveDate::from_ymd_opt(year, date.month(), date.day()) {
            if candidate >= today {
                return candidate.format("%F").to_string();
            }
        }
    }

    event_date.to_string()
}

fn channels_query_for_call(call: &PlannedToolCall) -> Option<String> {
    match &call.input {
        AssistantToolInput::ChannelsFilter { query } => query.clone(),
        _ => None,
    }
}

fn channel_matches_query(
    channel: &rustfin_db::repo::channels::ChannelRow,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    channel.name.to_ascii_lowercase().contains(&query)
        || channel.kind.to_ascii_lowercase().contains(&query)
}

fn summarize_transcript_session(
    channel: &rustfin_db::repo::channels::ChannelRow,
    session: &rustfin_db::repo::channel_transcripts::TranscriptSessionRow,
    entries: &[rustfin_db::repo::channel_transcripts::TranscriptEntryRow],
) -> ChannelTranscriptSummary {
    let started_ts_ms = session.started_ts.saturating_mul(1000);

    let mut speaker_counts: HashMap<String, TranscriptSpeakerSummary> = HashMap::new();
    let mut term_counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        let speaker =
            speaker_counts
                .entry(entry.username.clone())
                .or_insert(TranscriptSpeakerSummary {
                    username: entry.username.clone(),
                    segment_count: 0,
                    word_count: 0,
                    approx_spoken_seconds: 0,
                });
        speaker.segment_count += 1;
        speaker.word_count += transcript_word_count(&entry.text);
        speaker.approx_spoken_seconds += transcript_segment_duration_seconds(entry);

        for term in transcript_terms(&entry.text) {
            *term_counts.entry(term).or_insert(0) += 1;
        }
    }

    let mut speakers: Vec<_> = speaker_counts.into_values().collect();
    speakers.sort_by(|left, right| {
        right
            .segment_count
            .cmp(&left.segment_count)
            .then_with(|| right.word_count.cmp(&left.word_count))
            .then_with(|| left.username.cmp(&right.username))
    });

    let top_terms = transcript_top_terms(&term_counts, 8);
    let highlights = transcript_highlights(entries, started_ts_ms, &term_counts, 6);
    let (transcript_excerpt, transcript_excerpt_truncated) =
        transcript_excerpt(entries, started_ts_ms, 9, 7_500);

    ChannelTranscriptSummary {
        channel_id: channel.id.clone(),
        channel_name: channel.name.clone(),
        session_id: session.id.clone(),
        started_ts: session.started_ts,
        ended_ts: session.ended_ts.unwrap_or(session.started_ts),
        duration_seconds: session
            .ended_ts
            .unwrap_or(session.started_ts)
            .saturating_sub(session.started_ts),
        started_by_username: session.started_by_username.clone(),
        entry_count: session.entry_count,
        speaker_count: speakers.len(),
        top_terms,
        speakers,
        highlights,
        transcript_excerpt,
        transcript_excerpt_truncated,
    }
}

fn transcript_word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| !word.is_empty())
        .count()
}

fn transcript_segment_duration_seconds(
    entry: &rustfin_db::repo::channel_transcripts::TranscriptEntryRow,
) -> i64 {
    entry
        .ended_ts_ms
        .max(entry.started_ts_ms)
        .saturating_sub(entry.started_ts_ms)
        / 1000
}

fn transcript_terms(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
        .map(|token| token.trim_matches('\'').to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .filter(|token| !TRANSCRIPT_STOPWORDS.contains(&token.as_str()))
        .collect()
}

fn transcript_top_terms(term_counts: &HashMap<String, usize>, limit: usize) -> Vec<String> {
    let mut terms: Vec<_> = term_counts.iter().collect();
    terms.sort_by(|(left_term, left_count), (right_term, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_term.cmp(right_term))
    });
    terms
        .into_iter()
        .take(limit)
        .map(|(term, _)| term.clone())
        .collect()
}

fn transcript_highlights(
    entries: &[rustfin_db::repo::channel_transcripts::TranscriptEntryRow],
    session_started_ts_ms: i64,
    term_counts: &HashMap<String, usize>,
    limit: usize,
) -> Vec<TranscriptHighlightSummary> {
    let mut ranked: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let normalized = truncate_preview(&entry.text, 220);
            if normalized.is_empty() {
                return None;
            }
            let score = transcript_terms(&entry.text)
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .map(|term| term_counts.get(&term).copied().unwrap_or_default())
                .sum::<usize>();
            Some((index, score.max(1), normalized, entry))
        })
        .collect();
    ranked.sort_by(
        |(left_index, left_score, _, _), (right_index, right_score, _, _)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_index.cmp(right_index))
        },
    );
    ranked.truncate(limit);
    ranked.sort_by_key(|(index, _, _, _)| *index);

    ranked
        .into_iter()
        .map(|(_, _, text, entry)| TranscriptHighlightSummary {
            username: entry.username.clone(),
            started_ts_ms: entry.started_ts_ms,
            ended_ts_ms: entry.ended_ts_ms,
            relative_start: format_transcript_relative_ms(
                entry.started_ts_ms.saturating_sub(session_started_ts_ms),
            ),
            relative_end: format_transcript_relative_ms(
                entry
                    .ended_ts_ms
                    .max(entry.started_ts_ms)
                    .saturating_sub(session_started_ts_ms),
            ),
            text,
        })
        .collect()
}

fn transcript_excerpt(
    entries: &[rustfin_db::repo::channel_transcripts::TranscriptEntryRow],
    session_started_ts_ms: i64,
    sample_limit: usize,
    max_chars: usize,
) -> (String, bool) {
    let sample_indexes = transcript_excerpt_indexes(entries.len(), sample_limit);
    let mut lines = Vec::new();
    let mut total_chars = 0usize;
    let mut truncated = false;

    for index in sample_indexes.iter().copied() {
        let Some(entry) = entries.get(index) else {
            continue;
        };
        let line = format!(
            "[{}-{}] {}: {}",
            format_transcript_relative_ms(
                entry.started_ts_ms.saturating_sub(session_started_ts_ms)
            ),
            format_transcript_relative_ms(
                entry
                    .ended_ts_ms
                    .max(entry.started_ts_ms)
                    .saturating_sub(session_started_ts_ms)
            ),
            entry.username,
            truncate_preview(&entry.text, 240)
        );
        if total_chars + line.len() + usize::from(!lines.is_empty()) > max_chars {
            truncated = true;
            break;
        }
        total_chars += line.len() + usize::from(!lines.is_empty());
        lines.push(line);
    }

    if lines.is_empty() {
        return (String::new(), false);
    }

    if sample_indexes.len() < entries.len() {
        truncated = true;
    }

    (lines.join("\n"), truncated)
}

fn transcript_excerpt_indexes(total: usize, sample_limit: usize) -> Vec<usize> {
    if total == 0 || sample_limit == 0 {
        return Vec::new();
    }
    if total <= sample_limit {
        return (0..total).collect();
    }

    let front = sample_limit.min(3);
    let back = sample_limit.min(3);
    let middle_target = sample_limit.saturating_sub(front + back);
    let mut indexes: Vec<_> = (0..front).collect();
    if middle_target > 0 {
        let middle_start = front;
        let middle_end = total.saturating_sub(back);
        let span = middle_end.saturating_sub(middle_start);
        for offset in 0..middle_target {
            let numerator = (offset + 1) * span;
            let position = middle_start + (numerator / (middle_target + 1));
            indexes.push(position.min(total.saturating_sub(back + 1)));
        }
    }
    indexes.extend((total - back)..total);
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

fn format_transcript_relative_ms(relative_ms: i64) -> String {
    let total_seconds = relative_ms.max(0) / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

const TRANSCRIPT_STOPWORDS: &[&str] = &[
    "the", "and", "that", "this", "with", "from", "have", "just", "about", "into", "like", "yeah",
    "okay", "right", "really", "maybe", "going", "gonna", "call", "voice", "channel", "they",
    "them", "then", "than", "there", "their", "what", "when", "where", "which", "who", "would",
    "could", "should", "your", "youre", "were", "been", "being", "also", "very", "some", "more",
    "much", "many", "want", "need", "dont", "cant", "lets", "well", "im", "ive", "its", "our",
    "out", "for", "are", "was", "were", "has", "had", "did", "not", "but", "all", "any", "can",
    "get",
];

fn truncate_preview(content: &str, max_chars: usize) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    normalized.chars().take(max_chars).collect::<String>() + "..."
}

fn library_recent_query_for_call(call: &PlannedToolCall) -> Option<String> {
    match &call.input {
        AssistantToolInput::LibraryRecent { query } => query.clone(),
        _ => None,
    }
}

fn room_mode_filter_for_call(call: &PlannedToolCall) -> Option<String> {
    match &call.input {
        AssistantToolInput::RoomsFilter { room_mode, .. } => room_mode.clone(),
        _ => None,
    }
}

fn room_query_filter_for_call(call: &PlannedToolCall) -> Option<String> {
    match &call.input {
        AssistantToolInput::RoomsFilter { query, .. } => query.clone(),
        _ => None,
    }
}

fn room_mode_label(room_mode: &str) -> &'static str {
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

fn server_filter_for_call(call: &PlannedToolCall) -> (Option<String>, Option<String>) {
    match &call.input {
        AssistantToolInput::ServerFilter {
            query,
            availability,
        } => (query.clone(), availability.clone()),
        _ => (None, None),
    }
}

fn server_matches_availability(
    server: &rustfin_db::repo::servers::MinecraftServerRow,
    availability_filter: Option<&str>,
) -> bool {
    match availability_filter {
        Some("online") => {
            server.observed_state.eq_ignore_ascii_case("running")
                || server.observed_state.eq_ignore_ascii_case("online")
                || server.health_state.eq_ignore_ascii_case("healthy")
        }
        Some("offline") => {
            server.observed_state.eq_ignore_ascii_case("stopped")
                || server.observed_state.eq_ignore_ascii_case("offline")
                || server.observed_state.eq_ignore_ascii_case("exited")
        }
        Some("healthy") => server.health_state.eq_ignore_ascii_case("healthy"),
        Some("problem") => {
            !server.health_state.eq_ignore_ascii_case("healthy")
                || server.observed_state.eq_ignore_ascii_case("failed")
                || server.observed_state.eq_ignore_ascii_case("error")
        }
        Some(_) | None => true,
    }
}

fn server_matches_query(
    server: &rustfin_db::repo::servers::MinecraftServerRow,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    server.display_name.to_ascii_lowercase().contains(&query)
        || server.slug.to_ascii_lowercase().contains(&query)
}

fn server_status_label(
    count: usize,
    query: Option<&str>,
    availability_filter: Option<&str>,
) -> String {
    match (query, availability_filter) {
        (Some(query), Some(filter)) => {
            format!("{count} accessible Minecraft servers matching \"{query}\" and {filter}")
        }
        (Some(query), None) => format!("{count} accessible Minecraft servers matching \"{query}\""),
        (None, Some(filter)) => format!("{count} accessible Minecraft servers that are {filter}"),
        (None, None) => format!("{count} accessible Minecraft servers"),
    }
}

fn room_matches_query(
    room: &&rustfin_db::repo::watch_party::PublicRoomRow,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    let title = room_title_for_listing(
        &room.room_name,
        &room.room_mode,
        &room.audio_source,
        &room.item_title,
        &room.audio_library_name,
        &room.web_url,
    );
    title.to_ascii_lowercase().contains(&query)
}

fn public_room_matches_query(
    room: &rustfin_db::repo::watch_party::PublicRoomRow,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    let title = room_title_for_listing(
        &room.room_name,
        &room.room_mode,
        &room.audio_source,
        &room.item_title,
        &room.audio_library_name,
        &room.web_url,
    );
    title.to_ascii_lowercase().contains(&query)
}

fn invite_matches_room_mode(
    _invite: &rustfin_db::repo::watch_party::WatchPartyInviteSummary,
    room_mode: Option<&str>,
) -> bool {
    match room_mode {
        Some("invite") | None => true,
        Some(_) => false,
    }
}

fn invite_matches_query(
    invite: &rustfin_db::repo::watch_party::WatchPartyInviteSummary,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    invite.item_title.to_ascii_lowercase().contains(&query)
        || invite.host_username.to_ascii_lowercase().contains(&query)
}

fn web_room_title(web_url: &str) -> String {
    if let Ok(url) = reqwest::Url::parse(web_url.trim()) {
        if let Some(host) = url.host_str() {
            return format!("Web: {host}");
        }
    }
    "Web Room".to_string()
}

fn room_title_for_listing(
    room_name: &str,
    room_mode: &str,
    audio_source: &str,
    item_title: &str,
    audio_library_name: &str,
    web_url: &str,
) -> String {
    if !room_name.trim().is_empty() {
        return room_name.trim().to_string();
    }
    if room_mode == "audio" {
        if audio_library_name.is_empty() {
            return "Listen Together".to_string();
        }
        if audio_source == "online" {
            return format!("Listen Together: {audio_library_name}");
        }
        return format!("Music: {audio_library_name}");
    }
    if room_mode == "youtube" {
        return "YouTube Party".to_string();
    }
    if room_mode == "web" {
        return web_room_title(web_url);
    }
    if room_mode == "screen" {
        return "Screen Share".to_string();
    }
    if room_mode == "create" {
        return "Create Together".to_string();
    }
    if room_mode == "play" {
        return "Play Together".to_string();
    }
    if item_title.trim().is_empty() {
        "Watch Together".to_string()
    } else {
        item_title.to_string()
    }
}

fn local_core_health_url() -> String {
    let bind = std::env::var("RUSTFIN_BIND").unwrap_or_else(|_| "0.0.0.0:8096".to_string());
    if let Ok(addr) = bind.parse::<std::net::SocketAddr>() {
        let host = if addr.ip().is_unspecified() {
            "127.0.0.1".to_string()
        } else {
            addr.ip().to_string()
        };
        return format!(
            "http://{}:{}/health",
            format_host_for_url(&host),
            addr.port()
        );
    }

    let trimmed = bind
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    if let Some((host, port)) = trimmed.rsplit_once(':') {
        let normalized_host = match host {
            "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
            other => other,
        };
        return format!(
            "http://{}:{}/health",
            format_host_for_url(normalized_host),
            port.trim()
        );
    }

    "http://127.0.0.1:8096/health".to_string()
}

fn format_host_for_url(host: &str) -> String {
    let trimmed = host.trim().trim_matches(['[', ']']);
    if trimmed.contains(':') {
        format!("[{trimmed}]")
    } else {
        trimmed.to_string()
    }
}

fn health_url_from_base(base_url: &str) -> String {
    match reqwest::Url::parse(base_url) {
        Ok(mut url) => {
            url.set_path("/health");
            url.set_query(None);
            url.to_string()
        }
        Err(_) => format!("{}/health", base_url.trim_end_matches('/')),
    }
}

async fn probe_service_health_component(
    client: &reqwest::Client,
    name: &str,
    url: Option<String>,
) -> ServiceHealthComponentSummary {
    let Some(url) = url else {
        return ServiceHealthComponentSummary {
            name: name.to_string(),
            status: "disabled".to_string(),
            configured: false,
            url: None,
            detail: "Service is not configured on this host.".to_string(),
        };
    };

    let response =
        tokio::time::timeout(std::time::Duration::from_secs(3), client.get(&url).send()).await;

    match response {
        Ok(Ok(response)) if response.status().is_success() => ServiceHealthComponentSummary {
            name: name.to_string(),
            status: "healthy".to_string(),
            configured: true,
            url: Some(url),
            detail: format!("Health check returned HTTP {}.", response.status().as_u16()),
        },
        Ok(Ok(response)) => ServiceHealthComponentSummary {
            name: name.to_string(),
            status: "error".to_string(),
            configured: true,
            url: Some(url),
            detail: format!("Health check returned HTTP {}.", response.status().as_u16()),
        },
        Ok(Err(error)) => ServiceHealthComponentSummary {
            name: name.to_string(),
            status: "error".to_string(),
            configured: true,
            url: Some(url),
            detail: format!("Health check failed: {error}"),
        },
        Err(_) => ServiceHealthComponentSummary {
            name: name.to_string(),
            status: "error".to_string(),
            configured: true,
            url: Some(url),
            detail: "Health check timed out.".to_string(),
        },
    }
}

async fn collect_storage_summary(state: &AppState) -> StorageAssistantSummary {
    let mut paths = vec![
        ("cache_dir".to_string(), state.cache_dir.clone()),
        (
            "watch_party_audio_dir".to_string(),
            state.watch_party_audio_dir.clone(),
        ),
        (
            "ai_model_dir".to_string(),
            crate::ai_storage::current_model_dir(state).await,
        ),
    ];

    if let Ok(media_root) = std::env::var("RUSTFIN_MEDIA_PATH") {
        let trimmed = media_root.trim();
        if !trimmed.is_empty() {
            paths.push(("media_root".to_string(), std::path::PathBuf::from(trimmed)));
        }
    }

    #[cfg(target_os = "linux")]
    {
        match tokio::task::spawn_blocking(move || collect_linux_storage_summary(paths)).await {
            Ok(summary) => summary,
            Err(error) => StorageAssistantSummary {
                available: false,
                reason: Some(format!("Failed to collect storage summary: {error}")),
                mounts: Vec::new(),
                paths: Vec::new(),
            },
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = paths;
        StorageAssistantSummary {
            available: false,
            reason: Some("Storage summary is only available on Linux hosts.".to_string()),
            mounts: Vec::new(),
            paths: Vec::new(),
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_storage_summary(
    paths: Vec<(String, std::path::PathBuf)>,
) -> StorageAssistantSummary {
    let mounts = linux_mount_entries();
    let path_summaries = paths
        .into_iter()
        .map(|(name, path)| {
            let exists = path.exists();
            let resolved_path = if exists {
                Some(std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()))
            } else {
                None
            };
            let stats_path = nearest_existing_storage_path(&path);
            let mount = stats_path
                .as_deref()
                .and_then(|target| select_linux_mount_entry(&mounts, target));
            let (mount_point, mount_file_system, mount_source) = match mount {
                Some(entry) => (
                    Some(entry.mount_point.display().to_string()),
                    Some(entry.file_system.clone()),
                    entry.source.clone(),
                ),
                None => (None, None, None),
            };
            let (total_bytes, available_bytes) = stats_path
                .as_deref()
                .and_then(read_linux_storage_bytes)
                .map(|(total, available)| (Some(total), Some(available)))
                .unwrap_or((None, None));
            let used_bytes = storage_used_bytes(total_bytes, available_bytes);
            let used_percent = storage_used_percent(total_bytes, available_bytes);

            StoragePathSummary {
                name,
                path: path.display().to_string(),
                exists,
                resolved_path: resolved_path.map(|resolved| resolved.display().to_string()),
                stats_path: stats_path.map(|probe| probe.display().to_string()),
                mount_point,
                mount_file_system,
                mount_source,
                total_bytes,
                total_human: total_bytes.map(humanize_binary_bytes),
                available_bytes,
                available_human: available_bytes.map(humanize_binary_bytes),
                used_bytes,
                used_human: used_bytes.map(humanize_binary_bytes),
                used_percent,
            }
        })
        .collect::<Vec<_>>();
    let mount_summaries = summarize_storage_mounts(&path_summaries);

    StorageAssistantSummary {
        available: true,
        reason: None,
        mounts: mount_summaries,
        paths: path_summaries,
    }
}

#[cfg(target_os = "linux")]
fn summarize_storage_mounts(paths: &[StoragePathSummary]) -> Vec<StorageMountSummary> {
    let mut mounts =
        HashMap::<(String, Option<String>, Option<String>), StorageMountSummary>::new();

    for path in paths {
        let Some(mount_point) = path.mount_point.clone().or_else(|| path.stats_path.clone()) else {
            continue;
        };

        let entry = mounts.entry((
            mount_point.clone(),
            path.mount_file_system.clone(),
            path.mount_source.clone(),
        ));
        let mount_summary = entry.or_insert_with(|| StorageMountSummary {
            mount_point,
            mount_file_system: path.mount_file_system.clone(),
            mount_source: path.mount_source.clone(),
            tracked_paths: Vec::new(),
            total_bytes: path.total_bytes,
            total_human: path.total_human.clone(),
            available_bytes: path.available_bytes,
            available_human: path.available_human.clone(),
            used_bytes: path.used_bytes,
            used_human: path.used_human.clone(),
            used_percent: path.used_percent,
        });
        mount_summary.tracked_paths.push(path.name.clone());
    }

    let mut summaries = mounts.into_values().collect::<Vec<_>>();
    for summary in &mut summaries {
        summary.tracked_paths.sort();
        summary.tracked_paths.dedup();
    }
    summaries.sort_by(|left, right| left.mount_point.cmp(&right.mount_point));
    summaries
}

#[cfg(target_os = "linux")]
fn nearest_existing_storage_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(
                std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf()),
            );
        }
        current = candidate.parent();
    }
    None
}

#[cfg(target_os = "linux")]
fn read_linux_storage_bytes(path: &std::path::Path) -> Option<(u64, u64)> {
    let path_cstr = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: the C string is NUL-terminated and points to a valid existing path.
    let result = unsafe { libc::statvfs(path_cstr.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }

    // SAFETY: statvfs wrote the output struct because the call succeeded.
    let stats = unsafe { stats.assume_init() };
    let fragment_size = if stats.f_frsize > 0 {
        stats.f_frsize as u64
    } else {
        stats.f_bsize as u64
    };
    let total_bytes = (stats.f_blocks as u64).saturating_mul(fragment_size);
    let available_bytes = (stats.f_bavail as u64).saturating_mul(fragment_size);
    Some((total_bytes, available_bytes))
}

#[cfg(target_os = "linux")]
fn linux_mount_entries() -> Vec<LinuxMountEntry> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .ok()
        .map(|contents| {
            contents
                .lines()
                .filter_map(parse_linux_mount_entry)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn parse_linux_mount_entry(line: &str) -> Option<LinuxMountEntry> {
    let (left, right) = line.split_once(" - ")?;
    let left_fields = left.split_whitespace().collect::<Vec<_>>();
    let right_fields = right.split_whitespace().collect::<Vec<_>>();
    if left_fields.len() < 5 || right_fields.is_empty() {
        return None;
    }

    Some(LinuxMountEntry {
        mount_point: std::path::PathBuf::from(decode_linux_mountinfo_field(left_fields[4])),
        file_system: right_fields[0].to_string(),
        source: right_fields
            .get(1)
            .map(|value| decode_linux_mountinfo_field(value)),
    })
}

#[cfg(target_os = "linux")]
fn decode_linux_mountinfo_field(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0_usize;

    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1].is_ascii_digit()
            && bytes[index + 2].is_ascii_digit()
            && bytes[index + 3].is_ascii_digit()
        {
            let octal = &value[index + 1..index + 4];
            if octal.bytes().all(|digit| matches!(digit, b'0'..=b'7'))
                && let Ok(decoded_byte) = u8::from_str_radix(octal, 8)
            {
                decoded.push(decoded_byte);
                index += 4;
                continue;
            }
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(target_os = "linux")]
fn select_linux_mount_entry<'a>(
    mounts: &'a [LinuxMountEntry],
    target: &std::path::Path,
) -> Option<&'a LinuxMountEntry> {
    mounts
        .iter()
        .filter(|entry| target.starts_with(&entry.mount_point))
        .max_by(|left, right| {
            left.mount_point
                .components()
                .count()
                .cmp(&right.mount_point.components().count())
                .then_with(|| {
                    left.file_system
                        .ne("autofs")
                        .cmp(&right.file_system.ne("autofs"))
                })
        })
}

#[cfg(any(target_os = "linux", test))]
fn storage_used_bytes(total_bytes: Option<u64>, available_bytes: Option<u64>) -> Option<u64> {
    total_bytes
        .zip(available_bytes)
        .map(|(total, available)| total.saturating_sub(available))
}

#[cfg(any(target_os = "linux", test))]
fn storage_used_percent(total_bytes: Option<u64>, available_bytes: Option<u64>) -> Option<f64> {
    total_bytes
        .zip(available_bytes)
        .and_then(|(total, available)| {
            if total == 0 {
                None
            } else {
                Some(
                    (((total.saturating_sub(available)) as f64 / total as f64) * 1000.0).round()
                        / 10.0,
                )
            }
        })
}

fn follow_up_input_hint(call: &PlannedToolCall) -> AssistantFollowUpInputHint {
    match &call.input {
        AssistantToolInput::None => AssistantFollowUpInputHint::default(),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
            query,
        } => AssistantFollowUpInputHint {
            calendar_label: Some(label.clone()),
            calendar_from_date: Some(from_date.clone()),
            calendar_to_date: Some(to_date.clone()),
            calendar_query: query.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::CalendarCreateEvent {
            event_date, title, ..
        }
        | AssistantToolInput::CalendarCreateBirthday {
            event_date, title, ..
        } => AssistantFollowUpInputHint {
            calendar_label: Some("the created calendar event".to_string()),
            calendar_from_date: Some(event_date.clone()),
            calendar_to_date: Some(event_date.clone()),
            calendar_query: Some(title.clone()),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::CalendarDeleteEvent { .. }
        | AssistantToolInput::DocumentCreateDownload { .. } => {
            AssistantFollowUpInputHint::default()
        }
        AssistantToolInput::ChannelsFilter { query } => AssistantFollowUpInputHint {
            channels_query: query.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::DownloadsFilter {
            query,
            availability,
        } => AssistantFollowUpInputHint {
            downloads_query: query.clone(),
            downloads_availability: availability.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::LibrarySearch { query } => AssistantFollowUpInputHint {
            library_query: Some(query.clone()),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::LibraryRecent { query } => AssistantFollowUpInputHint {
            library_query: query.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::Weather {
            location,
            forecast_days,
        } => AssistantFollowUpInputHint {
            weather_location: Some(location.clone()),
            weather_days: *forecast_days,
            weather_start_date: None,
            weather_end_date: None,
            weather_label: None,
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::WeatherHistory {
            location,
            start_date,
            end_date,
            label,
        } => AssistantFollowUpInputHint {
            weather_location: Some(location.clone()),
            weather_days: None,
            weather_start_date: Some(start_date.clone()),
            weather_end_date: Some(end_date.clone()),
            weather_label: Some(label.clone()),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::WebSearch { query } => AssistantFollowUpInputHint {
            web_search_query: Some(query.clone()),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::WebFetch { url } => AssistantFollowUpInputHint {
            web_url: Some(url.clone()),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::CurrentDateTime { location } => AssistantFollowUpInputHint {
            current_datetime_location: location.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::RoomsFilter { room_mode, query } => AssistantFollowUpInputHint {
            room_mode: room_mode.clone(),
            room_query: query.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::ServerFilter {
            query,
            availability,
        } => AssistantFollowUpInputHint {
            server_query: query.clone(),
            server_availability: availability.clone(),
            ..AssistantFollowUpInputHint::default()
        },
    }
}

fn follow_up_entities(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> Vec<AssistantFollowUpEntity> {
    match tool {
        AssistantToolName::CalendarListEvents => block
            .data
            .get("events")
            .and_then(serde_json::Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, event)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!(
                                "{} ({})",
                                event.get("title")?.as_str()?,
                                event.get("event_date")?.as_str()?
                            ),
                            identifier: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarGetNextEvent => block
            .data
            .get("next_event")
            .map(|event| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: format!(
                        "{} ({})",
                        event
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&block.label),
                        event
                            .get("next_occurs_on")
                            .or_else(|| event.get("event_date"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                    ),
                    identifier: event
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarUpcomingBirthdays => block
            .data
            .get("birthdays")
            .and_then(serde_json::Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, event)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!(
                                "{} ({})",
                                event.get("title")?.as_str()?,
                                event.get("event_date")?.as_str()?
                            ),
                            identifier: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarGetEventDetails => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: format!(
                "{} ({})",
                block
                    .data
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&block.label),
                block
                    .data
                    .get("event_date")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
            ),
            identifier: block
                .data
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::CalendarCreateEvent | AssistantToolName::CalendarCreateBirthday => block
            .data
            .get("event")
            .map(|event| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: format!(
                        "{} ({})",
                        event
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&block.label),
                        event
                            .get("event_date")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                    ),
                    identifier: event
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarDeleteEvent | AssistantToolName::DocumentCreateDownload => {
            Vec::new()
        }
        AssistantToolName::ChannelsListUnreadActivity => Vec::new(),
        AssistantToolName::ChannelsGetTranscriptSummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("channel_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("channel_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::DownloadsListAvailableArtifacts => block
            .data
            .get("artifacts")
            .and_then(serde_json::Value::as_array)
            .map(|artifacts| {
                artifacts
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, artifact)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: artifact.get("title")?.as_str()?.to_string(),
                            identifier: artifact
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrarySearchTitles => block
            .data
            .get("matches")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, item)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: item.get("title")?.as_str()?.to_string(),
                            identifier: item
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrariesGetRecentlyAdded => block
            .data
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, item)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: item.get("title")?.as_str()?.to_string(),
                            identifier: item
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibraryGetItemSummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::RoomsListActive => block
            .data
            .get("rooms")
            .and_then(serde_json::Value::as_array)
            .map(|rooms| {
                rooms
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, room)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: room.get("title")?.as_str()?.to_string(),
                            identifier: room
                                .get("room_id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::RoomsListJoinable => block
            .data
            .get("rooms")
            .and_then(serde_json::Value::as_array)
            .map(|rooms| {
                rooms
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, room)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: room.get("title")?.as_str()?.to_string(),
                            identifier: room
                                .get("room_id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::RoomsGetRoomSummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("room_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::ServersListMinecraftStatus => block
            .data
            .get("servers")
            .and_then(serde_json::Value::as_array)
            .map(|servers| {
                servers
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, server)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: server.get("display_name")?.as_str()?.to_string(),
                            identifier: server
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::ServersGetMinecraftServerSummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("display_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::WebSearchPublicWeb => block
            .data
            .get("results")
            .and_then(serde_json::Value::as_array)
            .map(|results| {
                results
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, result)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: result.get("title")?.as_str()?.to_string(),
                            identifier: result
                                .get("url")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::WebFetchPublicPageSummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("page_title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("final_url")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        }],
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory
        | AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::NetworkGetTopologySummary
        | AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors => Vec::new(),
    }
}

fn downloads_filter_for_call(call: &PlannedToolCall) -> (Option<String>, Option<String>) {
    match &call.input {
        AssistantToolInput::DownloadsFilter {
            query,
            availability,
        } => (query.clone(), availability.clone()),
        _ => (None, None),
    }
}

fn downloads_matches_query(
    item: &crate::downloads::DownloadArtifactResponse,
    query: Option<&str>,
) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_ascii_lowercase();
    item.title.to_ascii_lowercase().contains(&query)
        || item.summary.to_ascii_lowercase().contains(&query)
        || item.id.to_ascii_lowercase().contains(&query)
}

fn downloads_matches_availability(
    item: &crate::downloads::DownloadArtifactResponse,
    availability: Option<&str>,
) -> bool {
    match availability {
        Some("available") => matches!(
            item.availability,
            crate::downloads::DownloadArtifactAvailability::Available
        ),
        Some("planned") => matches!(
            item.availability,
            crate::downloads::DownloadArtifactAvailability::Planned
        ),
        Some("unavailable") => matches!(
            item.availability,
            crate::downloads::DownloadArtifactAvailability::Unavailable
        ),
        Some(_) | None => true,
    }
}

fn downloads_status_label(count: usize, query: Option<&str>, availability: Option<&str>) -> String {
    match (query, availability) {
        (Some(query), Some(availability)) => {
            format!("{count} {availability} downloads matching \"{query}\"")
        }
        (Some(query), None) => format!("{count} downloads matching \"{query}\""),
        (None, Some(availability)) => format!("{count} {availability} downloads"),
        (None, None) => format!("{count} host-published downloads"),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{
        LinuxMountEntry, StoragePathSummary, nearest_existing_storage_path,
        select_linux_mount_entry, summarize_storage_mounts,
    };
    use super::{
        birthday_matches_query, birthday_month_day_display, enforce_tool_policy,
        next_birthday_occurrence, probe_service_health_component, storage_used_bytes,
        storage_used_percent, transcript_excerpt_indexes, transcript_terms,
    };
    use crate::ai_assistant::context::AssistantContext;
    use crate::ai_assistant::types::{
        AssistantToolSpec, ToolAccessMode, ToolConfirmationPolicy, ToolRiskTier,
        ToolRoleRequirement,
    };
    use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
    use rustfin_db::repo::calendar::CalendarEventRow;

    async fn spawn_health_test_server(status: StatusCode) -> (String, tokio::task::JoinHandle<()>) {
        async fn handler(status: StatusCode) -> impl IntoResponse {
            status
        }

        let app = Router::new().route("/health", get(move || handler(status)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}/health"), handle)
    }

    fn assistant_context(role: &str) -> AssistantContext {
        AssistantContext {
            trace_id: "trace-test".to_string(),
            user_id: "user-test".to_string(),
            username: "tester".to_string(),
            role: role.to_string(),
            is_admin: role == "admin",
            confirmed_write_tool: None,
            conversation_id: None,
        }
    }

    fn tool_spec(
        access_mode: ToolAccessMode,
        required_role: ToolRoleRequirement,
        confirmation: ToolConfirmationPolicy,
    ) -> AssistantToolSpec {
        AssistantToolSpec {
            name: "test_tool",
            summary: "test tool",
            access_mode,
            risk_tier: ToolRiskTier::Low,
            required_role,
            confirmation,
            timeout_ms: 1_000,
            max_result_bytes: 1_024,
        }
    }

    #[test]
    fn enforce_tool_policy_allows_read_only_authenticated_tools() {
        let context = assistant_context("user");
        let spec = tool_spec(
            ToolAccessMode::ReadOnly,
            ToolRoleRequirement::AnyAuthenticatedUser,
            ToolConfirmationPolicy::None,
        );
        assert_eq!(enforce_tool_policy(&context, spec), None);
    }

    #[test]
    fn enforce_tool_policy_denies_admin_only_tools_for_non_admins() {
        let context = assistant_context("user");
        let spec = tool_spec(
            ToolAccessMode::ReadOnly,
            ToolRoleRequirement::AdminOnly,
            ToolConfirmationPolicy::None,
        );
        let message = enforce_tool_policy(&context, spec).expect("policy should reject");
        assert!(message.contains("requires an admin Rustyfin account"));
    }

    #[test]
    fn enforce_tool_policy_requires_explicit_confirmation_for_write_tools() {
        let context = assistant_context("admin");
        let spec = tool_spec(
            ToolAccessMode::Write,
            ToolRoleRequirement::AdminOnly,
            ToolConfirmationPolicy::None,
        );
        let message = enforce_tool_policy(&context, spec).expect("policy should reject");
        assert!(message.contains("requires explicit confirmation"));
    }

    #[test]
    fn enforce_tool_policy_allows_confirmed_write_tools() {
        let context = assistant_context("admin").with_confirmed_write_tool("test_tool");
        let spec = tool_spec(
            ToolAccessMode::Write,
            ToolRoleRequirement::AdminOnly,
            ToolConfirmationPolicy::None,
        );
        assert_eq!(enforce_tool_policy(&context, spec), None);
    }

    #[test]
    fn enforce_tool_policy_denies_confirmation_gated_tools_until_supported() {
        let context = assistant_context("admin");
        let spec = tool_spec(
            ToolAccessMode::ReadOnly,
            ToolRoleRequirement::AdminOnly,
            ToolConfirmationPolicy::ProtectedAction,
        );
        let message = enforce_tool_policy(&context, spec).expect("policy should reject");
        assert!(message.contains("confirmation flow is implemented"));
    }

    fn birthday_event(title: &str, owner_username: Option<&str>) -> CalendarEventRow {
        CalendarEventRow {
            id: "birthday-test".to_string(),
            scope: "global".to_string(),
            owner_user_id: None,
            owner_username: owner_username.map(str::to_string),
            title: title.to_string(),
            description: Some("Friend birthday".to_string()),
            event_date: "2001-02-03".to_string(),
            event_type: "birthday".to_string(),
            recurrence: "yearly".to_string(),
            birthday_year: Some(2001),
            created_by_user_id: "creator".to_string(),
            created_by_username: Some("creator".to_string()),
            created_ts: 0,
            updated_ts: 0,
        }
    }

    #[test]
    fn birthday_match_checks_title_and_owner() {
        let title_match = birthday_event("Rachel", None);
        assert!(birthday_matches_query(&title_match, "rachel"));

        let owner_match = birthday_event("Shared Birthday", Some("rachel"));
        assert!(birthday_matches_query(&owner_match, "rachel"));

        let miss = birthday_event("Sam", Some("sam"));
        assert!(!birthday_matches_query(&miss, "rachel"));
    }

    #[test]
    fn birthday_display_uses_month_day() {
        assert_eq!(birthday_month_day_display("2001-02-03"), "February 3");
    }

    #[test]
    fn next_birthday_occurrence_returns_iso_date() {
        let next = next_birthday_occurrence("2001-02-03");
        assert_eq!(next.len(), 10);
        assert!(next.chars().all(|ch| ch.is_ascii_digit() || ch == '-'));
    }

    #[test]
    fn transcript_excerpt_sampling_keeps_early_middle_and_late_lines() {
        let indexes = transcript_excerpt_indexes(12, 9);
        assert!(indexes.contains(&0));
        assert!(indexes.contains(&1));
        assert!(indexes.contains(&2));
        assert!(indexes.contains(&9));
        assert!(indexes.contains(&10));
        assert!(indexes.contains(&11));
        assert!(indexes.len() <= 9);
    }

    #[test]
    fn transcript_terms_drop_common_fillers() {
        let terms = transcript_terms(
            "Yeah okay the server deploy failed again and Rachel fixed the backup path.",
        );
        assert!(terms.contains(&"server".to_string()));
        assert!(terms.contains(&"deploy".to_string()));
        assert!(terms.contains(&"rachel".to_string()));
        assert!(!terms.contains(&"yeah".to_string()));
        assert!(!terms.contains(&"okay".to_string()));
    }

    #[test]
    fn storage_usage_helpers_compute_used_capacity() {
        assert_eq!(storage_used_bytes(Some(1_000), Some(400)), Some(600));
        assert_eq!(storage_used_percent(Some(1_000), Some(400)), Some(60.0));
        assert_eq!(storage_used_percent(Some(0), Some(0)), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn select_linux_mount_entry_prefers_real_fs_over_autofs_on_same_mount_point() {
        let mounts = vec![
            LinuxMountEntry {
                mount_point: "/mnt/truenas_media".into(),
                file_system: "autofs".to_string(),
                source: Some("systemd-1".to_string()),
            },
            LinuxMountEntry {
                mount_point: "/mnt/truenas_media".into(),
                file_system: "nfs4".to_string(),
                source: Some("192.168.0.4:/mnt/Bluechip/media".to_string()),
            },
        ];

        let selected =
            select_linux_mount_entry(&mounts, std::path::Path::new("/mnt/truenas_media"))
                .expect("mount should be selected");

        assert_eq!(selected.file_system, "nfs4");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn summarize_storage_mounts_deduplicates_paths_on_same_mount() {
        let mounts = summarize_storage_mounts(&[
            StoragePathSummary {
                name: "cache_dir".to_string(),
                path: "/srv/cache".to_string(),
                exists: true,
                resolved_path: Some("/srv/cache".to_string()),
                stats_path: Some("/srv/cache".to_string()),
                mount_point: Some("/".to_string()),
                mount_file_system: Some("ext4".to_string()),
                mount_source: Some("/dev/nvme0n1p3".to_string()),
                total_bytes: Some(100),
                total_human: Some("100 B".to_string()),
                available_bytes: Some(40),
                available_human: Some("40 B".to_string()),
                used_bytes: Some(60),
                used_human: Some("60 B".to_string()),
                used_percent: Some(60.0),
            },
            StoragePathSummary {
                name: "watch_party_audio_dir".to_string(),
                path: "/srv/cache/watch_party_audio".to_string(),
                exists: true,
                resolved_path: Some("/srv/cache/watch_party_audio".to_string()),
                stats_path: Some("/srv/cache/watch_party_audio".to_string()),
                mount_point: Some("/".to_string()),
                mount_file_system: Some("ext4".to_string()),
                mount_source: Some("/dev/nvme0n1p3".to_string()),
                total_bytes: Some(100),
                total_human: Some("100 B".to_string()),
                available_bytes: Some(40),
                available_human: Some("40 B".to_string()),
                used_bytes: Some(60),
                used_human: Some("60 B".to_string()),
                used_percent: Some(60.0),
            },
            StoragePathSummary {
                name: "media_root".to_string(),
                path: "/mnt/truenas_media".to_string(),
                exists: true,
                resolved_path: Some("/mnt/truenas_media".to_string()),
                stats_path: Some("/mnt/truenas_media".to_string()),
                mount_point: Some("/mnt/truenas_media".to_string()),
                mount_file_system: Some("nfs4".to_string()),
                mount_source: Some("192.168.0.4:/mnt/Bluechip/media".to_string()),
                total_bytes: Some(200),
                total_human: Some("200 B".to_string()),
                available_bytes: Some(120),
                available_human: Some("120 B".to_string()),
                used_bytes: Some(80),
                used_human: Some("80 B".to_string()),
                used_percent: Some(40.0),
            },
        ]);

        assert_eq!(mounts.len(), 2);
        assert_eq!(
            mounts[0].tracked_paths,
            vec!["cache_dir".to_string(), "watch_party_audio_dir".to_string()]
        );
        assert_eq!(mounts[1].tracked_paths, vec!["media_root".to_string()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nearest_existing_storage_path_falls_back_to_existing_parent() {
        let tempdir = tempfile::tempdir().unwrap();
        let missing_path = tempdir.path().join("missing").join("models");
        let probe = nearest_existing_storage_path(&missing_path).expect("existing parent expected");
        assert_eq!(probe, tempdir.path());
    }

    #[tokio::test]
    async fn service_health_probe_marks_healthy_services() {
        let (url, handle) = spawn_health_test_server(StatusCode::OK).await;
        let client = reqwest::Client::new();
        let summary = probe_service_health_component(&client, "core_api", Some(url)).await;
        assert_eq!(summary.status, "healthy");
        assert!(summary.detail.contains("HTTP 200"));
        handle.abort();
    }

    #[tokio::test]
    async fn service_health_probe_marks_http_failures_as_error() {
        let (url, handle) = spawn_health_test_server(StatusCode::SERVICE_UNAVAILABLE).await;
        let client = reqwest::Client::new();
        let summary = probe_service_health_component(&client, "core_api", Some(url)).await;
        assert_eq!(summary.status, "error");
        assert!(summary.detail.contains("HTTP 503"));
        handle.abort();
    }

    #[tokio::test]
    async fn service_health_probe_reports_connection_failures() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let client = reqwest::Client::new();
        let summary = probe_service_health_component(
            &client,
            "core_api",
            Some(format!("http://{addr}/health")),
        )
        .await;
        assert_eq!(summary.status, "error");
        assert!(summary.detail.contains("Health check failed"));
    }
}
