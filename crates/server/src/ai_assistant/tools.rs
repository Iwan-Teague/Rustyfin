use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, Utc};
use serde::Serialize;
use serde_json::json;

use super::context::AssistantContext;
use super::registry::AssistantToolName;
use super::types::{
    AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
    AssistantGroundingSource, AssistantToolContextBlock, AssistantToolInput, PlannedToolCall,
    ToolAccessMode, ToolConfirmationPolicy, ToolRoleRequirement,
};
use super::weather::{fetch_public_weather_current, fetch_public_weather_forecast};
use super::web::{fetch_public_page_summary, public_web_tools_enabled, search_public_web};
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
struct PublicWebSearchSummary {
    query: String,
    results: Vec<super::web::PublicWebSearchResult>,
}

#[derive(Debug, Serialize)]
struct BackupAssistantSummary {
    configured: bool,
    restore_supported: bool,
    last_successful_backup_ts: Option<i64>,
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

#[derive(Debug, Serialize)]
struct StoragePathSummary {
    name: String,
    path: String,
    exists: bool,
    mount_point: Option<String>,
    total_bytes: Option<u64>,
    available_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct StorageAssistantSummary {
    available: bool,
    reason: Option<String>,
    paths: Vec<StoragePathSummary>,
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
        AssistantToolName::CalendarUpcomingBirthdays => {
            calendar_upcoming_birthdays(state, context, call).await
        }
        AssistantToolName::CalendarGetEventDetails => {
            calendar_get_event_details(state, context, call).await
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
        AssistantToolName::WebSearchPublicWeb => web_search_public_web(state, context, call).await,
        AssistantToolName::WebFetchPublicPageSummary => {
            web_fetch_public_page_summary(state, context, call).await
        }
        AssistantToolName::RoomsListActive => rooms_list_active(state, context, call).await,
        AssistantToolName::RoomsListJoinable => rooms_list_joinable(state, context, call).await,
        AssistantToolName::RoomsGetRoomSummary => room_get_room_summary(state, context, call).await,
        AssistantToolName::SystemGetHostRuntimeSummary => {
            system_get_host_runtime_summary(state, context).await
        }
        AssistantToolName::SystemGetBackupSummary => system_get_backup_summary().await,
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
        ToolAccessMode::Write | ToolAccessMode::DestructiveWrite => {
            return Some(format!(
                "{} is not available because assistant writes are disabled.",
                spec.name
            ));
        }
    }

    match spec.confirmation {
        ToolConfirmationPolicy::None => None,
        ToolConfirmationPolicy::ExplicitUserConfirm | ToolConfirmationPolicy::ProtectedAction => {
            Some(format!(
                "{} is blocked until the confirmation flow is implemented.",
                spec.name
            ))
        }
    }
}

pub fn source_from_block(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> AssistantGroundingSource {
    let spec = tool.spec();
    AssistantGroundingSource {
        tool: spec.name,
        label: block.label.clone(),
        access_mode: spec.access_mode,
        risk_tier: spec.risk_tier,
        status: block.status,
    }
}

pub fn build_follow_up_context(
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
) -> AssistantFollowUpContext {
    AssistantFollowUpContext {
        tool: call.tool.as_str().to_string(),
        label: block.label.clone(),
        input_hint: follow_up_input_hint(call),
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
    let catalog = crate::downloads::build_download_catalog(state);

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

async fn system_get_backup_summary() -> Result<(String, serde_json::Value), String> {
    let summary = BackupAssistantSummary {
        configured: false,
        restore_supported: false,
        last_successful_backup_ts: None,
        message: "Rustyfin backup and restore workflows are not implemented on this host yet."
            .to_string(),
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
        AssistantToolInput::None
        | AssistantToolInput::ChannelsFilter { .. }
        | AssistantToolInput::DownloadsFilter { .. }
        | AssistantToolInput::LibrarySearch { .. }
        | AssistantToolInput::LibraryRecent { .. }
        | AssistantToolInput::Weather { .. }
        | AssistantToolInput::WebSearch { .. }
        | AssistantToolInput::WebFetch { .. }
        | AssistantToolInput::RoomsFilter { .. }
        | AssistantToolInput::ServerFilter { .. } => {
            let from = Utc::now().date_naive();
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
    let today = Utc::now().date_naive();
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
            paths: Vec::new(),
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_storage_summary(
    paths: Vec<(String, std::path::PathBuf)>,
) -> StorageAssistantSummary {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let summaries = paths
        .into_iter()
        .map(|(name, path)| {
            let exists = path.exists();
            let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let disk = disks
                .list()
                .iter()
                .filter(|disk| target.starts_with(disk.mount_point()))
                .max_by_key(|disk| disk.mount_point().components().count());

            StoragePathSummary {
                name,
                path: path.display().to_string(),
                exists,
                mount_point: disk.map(|disk| disk.mount_point().display().to_string()),
                total_bytes: disk.map(|disk| disk.total_space()),
                available_bytes: disk.map(|disk| disk.available_space()),
            }
        })
        .collect();

    StorageAssistantSummary {
        available: true,
        reason: None,
        paths: summaries,
    }
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
        | AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::NetworkGetTopologySummary
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
    use super::{
        birthday_matches_query, birthday_month_day_display, enforce_tool_policy,
        next_birthday_occurrence, probe_service_health_component, transcript_excerpt_indexes,
        transcript_terms,
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
    fn enforce_tool_policy_denies_write_tools() {
        let context = assistant_context("admin");
        let spec = tool_spec(
            ToolAccessMode::Write,
            ToolRoleRequirement::AdminOnly,
            ToolConfirmationPolicy::None,
        );
        let message = enforce_tool_policy(&context, spec).expect("policy should reject");
        assert!(message.contains("assistant writes are disabled"));
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
