use std::collections::{HashMap, HashSet};

use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;

use super::context::AssistantContext;
use super::registry::AssistantToolName;
use super::types::{
    AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
    AssistantGroundingSource, AssistantToolContextBlock, AssistantToolInput, PlannedToolCall,
    ToolAccessMode, ToolConfirmationPolicy, ToolRoleRequirement,
};
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
        AssistantToolName::DownloadsListAvailableArtifacts => {
            downloads_list_available_artifacts(state, context, call).await
        }
        AssistantToolName::LibrariesListAccessible => {
            libraries_list_accessible(state, context).await
        }
        AssistantToolName::LibrarySearchTitles => library_search_titles(state, context, call).await,
        AssistantToolName::LibraryGetItemSummary => {
            library_get_item_summary(state, context, call).await
        }
        AssistantToolName::WebSearchPublicWeb => web_search_public_web(state, context, call).await,
        AssistantToolName::WebFetchPublicPageSummary => {
            web_fetch_public_page_summary(state, context, call).await
        }
        AssistantToolName::RoomsListActive => rooms_list_active(state, context, call).await,
        AssistantToolName::RoomsGetRoomSummary => room_get_room_summary(state, context, call).await,
        AssistantToolName::SystemGetHostRuntimeSummary => {
            system_get_host_runtime_summary(state, context).await
        }
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

async fn calendar_upcoming_birthdays(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 30);
    let events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from,
        &to,
    )
    .await
    .map_err(|e| format!("failed to load upcoming birthdays: {e}"))?;

    let birthdays: Vec<_> = events
        .into_iter()
        .filter(|event| event.event_type == "birthday")
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
        format!("Upcoming birthdays for {label}"),
        json!({
            "window": {
                "from": from,
                "to": to,
                "label": label,
            },
            "birthdays": birthdays,
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
        } => (from_date.clone(), to_date.clone(), label.clone()),
        AssistantToolInput::None
        | AssistantToolInput::DownloadsFilter { .. }
        | AssistantToolInput::LibrarySearch { .. }
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

fn follow_up_input_hint(call: &PlannedToolCall) -> AssistantFollowUpInputHint {
    match &call.input {
        AssistantToolInput::None => AssistantFollowUpInputHint::default(),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
        } => AssistantFollowUpInputHint {
            calendar_label: Some(label.clone()),
            calendar_from_date: Some(from_date.clone()),
            calendar_to_date: Some(to_date.clone()),
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
        AssistantToolName::CalendarListEvents | AssistantToolName::CalendarUpcomingBirthdays => {
            block
                .data
                .get(
                    if matches!(tool, AssistantToolName::CalendarUpcomingBirthdays) {
                        "birthdays"
                    } else {
                        "events"
                    },
                )
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
                .unwrap_or_default()
        }
        AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::LibrariesListAccessible
        | AssistantToolName::SystemGetHostRuntimeSummary => Vec::new(),
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
    use super::enforce_tool_policy;
    use crate::ai_assistant::context::AssistantContext;
    use crate::ai_assistant::types::{
        AssistantToolSpec, ToolAccessMode, ToolConfirmationPolicy, ToolRiskTier,
        ToolRoleRequirement,
    };

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
}
