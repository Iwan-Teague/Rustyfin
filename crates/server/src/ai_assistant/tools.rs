use std::collections::{BTreeMap, HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use rustfin_db::repo::dictionary::{self as dictionary_repo, SubjectKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;

use super::context::AssistantContext;
use super::dates::{assistant_local_now, assistant_local_today, assistant_local_year};
use super::diagnostics;
use super::outcomes::normalize_tool_result;
use super::provider::{ToolExecutionProfile, default_tool_registry};
use super::registry::AssistantToolName;
use super::replies::compact_text;
use super::types::{
    AssistantFollowUpContext, AssistantFollowUpEntity, AssistantFollowUpInputHint,
    AssistantGroundingSource, AssistantToolContextBlock, AssistantToolInput, AssistantToolOutcome,
    PlannedToolCall, ToolAccessMode, ToolConfirmationPolicy, ToolRoleRequirement,
};
use super::weather::{
    fetch_public_weather_current, fetch_public_weather_forecast, fetch_public_weather_history,
};
use super::web::{fetch_public_page_summary, public_web_tools_enabled, search_public_web};
use super::web_sources::{
    CuratedWebCategory, curated_web_catalog_summary, curated_web_category_for_url,
    curated_web_category_label, fetch_curated_web_page_summary, search_curated_web,
};
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
struct DictionaryAccountIdentityEnvelope {
    linked: bool,
    person_id: Option<String>,
    person_name: Option<String>,
    family_workspace_id: Option<String>,
    friends_workspace_id: Option<String>,
    work_workspace_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct DictionaryWorkspaceSummary {
    workspace_id: String,
    title: String,
    workspace_kind: String,
    owner_user_id: Option<String>,
    is_system_seeded: bool,
}

#[derive(Debug, Serialize)]
struct DictionaryVisibleWorkspacesEnvelope {
    workspaces: Vec<DictionaryWorkspaceSummary>,
}

#[derive(Debug, Serialize)]
struct DictionaryPersonSummary {
    id: String,
    display_name: String,
    canonical_name: String,
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct DictionaryFactSummary {
    fact_key: String,
    value_type: String,
    value_text: Option<String>,
    value_int: Option<i64>,
    value_bool: Option<bool>,
    value_date: Option<String>,
    value_json: Option<Value>,
}

#[derive(Debug, Serialize)]
struct DictionaryRelationSummary {
    relation_id: String,
    relation_group_key: String,
    relation_type: String,
    direction: String,
    other_person_id: String,
    other_person_name: String,
    other_person_summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct DictionaryPersonBundleEnvelope {
    workspace_id: String,
    person: DictionaryPersonSummary,
    facts: Vec<DictionaryFactSummary>,
    relations: Vec<DictionaryRelationSummary>,
    document_title: Option<String>,
    document_excerpt: Option<String>,
}

#[derive(Debug, Serialize)]
struct DictionaryWorkspacePeopleEnvelope {
    workspace_id: String,
    workspace_title: String,
    query: Option<String>,
    people: Vec<DictionaryPersonSummary>,
}

#[derive(Debug, Serialize)]
struct DictionaryResolvedCandidate {
    person_id: String,
    display_name: String,
    summary: Option<String>,
    relation_type: String,
    birthday: Option<String>,
    hobbies: Vec<String>,
    document_excerpt: Option<String>,
}

#[derive(Debug, Serialize)]
struct DictionaryRelationshipResolutionEnvelope {
    reference: String,
    relation_kind: String,
    workspace_id: Option<String>,
    workspace_title: Option<String>,
    status: String,
    message: Option<String>,
    linked_person_id: Option<String>,
    linked_person_name: Option<String>,
    candidates: Vec<DictionaryResolvedCandidate>,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryFactSummary {
    id: String,
    memory_key: String,
    memory_type: String,
    topic_key: Option<String>,
    title: String,
    content: String,
    weight: f64,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Serialize)]
struct MemoryFactsEnvelope {
    query: Option<String>,
    topic_key: Option<String>,
    total_count: usize,
    facts: Vec<MemoryFactSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryEntitySummary {
    id: String,
    node_key: String,
    entity_kind: String,
    label: String,
    identifier: Option<String>,
    topic_key: Option<String>,
    source_chunk_id: Option<String>,
    access_scope: String,
    ordinal: i64,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Serialize)]
struct MemoryEntitiesEnvelope {
    query: Option<String>,
    total_count: usize,
    entities: Vec<MemoryEntitySummary>,
}

#[derive(Debug, Serialize)]
struct MemoryExactEntitiesEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    entities: Vec<MemoryEntitySummary>,
}

#[derive(Debug, Serialize)]
struct MemoryRecentChangesEnvelope {
    query: Option<String>,
    fact_count: usize,
    entity_count: usize,
    facts: Vec<MemoryFactSummary>,
    entities: Vec<MemoryEntitySummary>,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryFactConflictSummary {
    topic_key: Option<String>,
    title: String,
    fact_count: usize,
    distinct_content_count: usize,
    facts: Vec<MemoryFactSummary>,
}

#[derive(Debug, Serialize)]
struct MemoryConflictingFactsEnvelope {
    query: Option<String>,
    total_count: usize,
    conflict_group_count: usize,
    conflicts: Vec<MemoryFactConflictSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MemoryEntityProvenanceSummary {
    id: String,
    node_key: String,
    conversation_id: Option<String>,
    turn_id: Option<String>,
    entity_kind: String,
    label: String,
    identifier: Option<String>,
    topic_key: Option<String>,
    source_chunk_id: Option<String>,
    access_scope: String,
    ordinal: i64,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MemoryProvenanceChunkSummary {
    chunk_key: String,
    source_kind: String,
    source_id: String,
    source_sub_id: Option<String>,
    owner_user_id: Option<String>,
    access_scope: String,
    access_key: Option<String>,
    topic_key: Option<String>,
    title: String,
    excerpt: String,
    source_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Serialize)]
struct MemoryEntityProvenanceEnvelope {
    query: String,
    matched_by: String,
    entity: Option<MemoryEntityProvenanceSummary>,
    source_chunk: Option<MemoryProvenanceChunkSummary>,
}

#[derive(Debug, Serialize)]
struct MemoryPersonSummaryEnvelope {
    query: String,
    matched_by: String,
    person: MemoryEntitySummary,
    relation_count: usize,
    relations: Vec<MemoryEntityRelationSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryEntityRelationSummary {
    direction: String,
    relation: String,
    weight: f64,
    created_ts: i64,
    entity: MemoryEntitySummary,
}

#[derive(Debug, Serialize)]
struct MemoryEntityRelationsEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    root: Option<MemoryEntitySummary>,
    relations: Vec<MemoryEntityRelationSummary>,
}

#[derive(Debug, Serialize)]
struct MemoryEntityRelationPathEnvelope {
    query: String,
    source_query: String,
    target_query: String,
    matched_by: String,
    total_hops: usize,
    path_found: bool,
    root: Option<MemoryEntitySummary>,
    target: Option<MemoryEntitySummary>,
    path: Vec<MemoryEntityRelationSummary>,
}

fn memory_fact_summary_from_row(
    row: rustfin_db::repo::ai_grounding::AiMemoryItemRow,
) -> MemoryFactSummary {
    MemoryFactSummary {
        id: row.id,
        memory_key: row.memory_key,
        memory_type: row.memory_type,
        topic_key: row.topic_key,
        title: row.title,
        content: row.content,
        weight: row.weight,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
    }
}

fn memory_entity_summary_from_row(
    row: rustfin_db::repo::ai_grounding::AiEntityNodeRow,
) -> MemoryEntitySummary {
    MemoryEntitySummary {
        id: row.id,
        node_key: row.node_key,
        entity_kind: row.entity_kind,
        label: row.label,
        identifier: row.identifier,
        topic_key: row.topic_key,
        source_chunk_id: row.source_chunk_id,
        access_scope: row.access_scope,
        ordinal: row.ordinal,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
    }
}

fn memory_entity_summary_from_pg_row(
    row: &sqlx::postgres::PgRow,
) -> Result<MemoryEntitySummary, sqlx::Error> {
    Ok(MemoryEntitySummary {
        id: row.try_get("id")?,
        node_key: row.try_get("node_key")?,
        entity_kind: row.try_get("entity_kind")?,
        label: row.try_get("label")?,
        identifier: row.try_get("identifier")?,
        topic_key: row.try_get("topic_key")?,
        source_chunk_id: row.try_get("source_chunk_id")?,
        access_scope: row.try_get("access_scope")?,
        ordinal: row.try_get("ordinal")?,
        created_ts: row.try_get("created_ts")?,
        updated_ts: row.try_get("updated_ts")?,
    })
}

fn memory_entity_provenance_summary_from_row(
    row: rustfin_db::repo::ai_grounding::AiEntityNodeRow,
) -> MemoryEntityProvenanceSummary {
    MemoryEntityProvenanceSummary {
        id: row.id,
        node_key: row.node_key,
        conversation_id: row.conversation_id,
        turn_id: row.turn_id,
        entity_kind: row.entity_kind,
        label: row.label,
        identifier: row.identifier,
        topic_key: row.topic_key,
        source_chunk_id: row.source_chunk_id,
        access_scope: row.access_scope,
        ordinal: row.ordinal,
        created_ts: row.created_ts,
        updated_ts: row.updated_ts,
    }
}

fn memory_provenance_chunk_summary_from_row(
    row: rustfin_db::repo::ai_grounding::AiRetrievalChunkRow,
) -> MemoryProvenanceChunkSummary {
    MemoryProvenanceChunkSummary {
        chunk_key: row.chunk_key,
        source_kind: row.source_kind,
        source_id: row.source_id,
        source_sub_id: row.source_sub_id,
        owner_user_id: row.owner_user_id,
        access_scope: row.access_scope,
        access_key: row.access_key,
        topic_key: row.topic_key,
        title: row.title,
        excerpt: row.excerpt,
        source_ts: row.source_ts,
        updated_ts: row.updated_ts,
    }
}

async fn memory_relation_rows_for_node(
    pool: &sqlx::PgPool,
    user_id: &str,
    is_admin: bool,
    node_key: &str,
    direction: &str,
    limit: i64,
) -> Result<Vec<MemoryEntityRelationSummary>, String> {
    let neighbor_column = if direction == "outgoing" {
        "to_node_key"
    } else {
        "from_node_key"
    };
    let match_column = if direction == "outgoing" {
        "from_node_key"
    } else {
        "to_node_key"
    };
    let sql = format!(
        "SELECT e.relation, e.weight, e.created_ts AS relation_created_ts,
                n.id, n.node_key, n.owner_user_id, n.conversation_id, n.turn_id,
                n.entity_kind, n.label, n.identifier, n.topic_key, n.source_chunk_id,
                n.access_scope, n.access_key, n.ordinal, n.metadata_json,
                n.created_ts, n.updated_ts
         FROM ai_entity_edge e
         JOIN ai_entity_node n ON n.node_key = e.{neighbor_column}
         WHERE e.{match_column} = $1
           AND (n.access_scope = 'shared'
                OR (n.access_scope = 'admin' AND $2)
                OR (n.access_scope = 'user' AND n.owner_user_id = $3))
         ORDER BY e.weight DESC, e.created_ts DESC, n.ordinal ASC
         LIMIT $4"
    );

    let rows = sqlx::query(&sql)
        .bind(node_key)
        .bind(is_admin)
        .bind(user_id)
        .bind(limit.clamp(1, 16))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("failed to load memory relations: {e}"))?;

    rows.into_iter()
        .map(|row| {
            Ok(MemoryEntityRelationSummary {
                direction: direction.to_string(),
                relation: row.try_get("relation")?,
                weight: row.try_get("weight")?,
                created_ts: row.try_get("relation_created_ts")?,
                entity: memory_entity_summary_from_pg_row(&row)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(|e| format!("failed to parse memory relations: {e}"))
}

#[derive(Debug, Serialize)]
struct LibrarySummary {
    library_id: String,
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
struct DownloadArtifactDetailSummary {
    query: Option<String>,
    matched_by: String,
    #[serde(flatten)]
    artifact: crate::downloads::DownloadArtifactResponse,
}

#[derive(Debug, Serialize)]
struct DownloadArtifactSourceSummary {
    query: Option<String>,
    matched_by: String,
    source_url: Option<String>,
    download_path: Option<String>,
    external_url: Option<String>,
    #[serde(flatten)]
    artifact: crate::downloads::DownloadArtifactResponse,
}

#[derive(Debug, Serialize)]
struct DownloadArtifactReleaseNotesSummary {
    query: Option<String>,
    matched_by: String,
    release_notes: String,
    #[serde(flatten)]
    artifact: crate::downloads::DownloadArtifactResponse,
}

#[derive(Debug, Serialize)]
struct LibraryDuplicateTitleSummary {
    title: String,
    item_count: usize,
    library_count: usize,
    libraries: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LibraryMissingMetadataItemSummary {
    library_id: String,
    library_name: Option<String>,
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    missing_fields: Vec<String>,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Serialize)]
struct NetworkInterfaceDetailSummary {
    query: String,
    matched_by: String,
    host_label: Option<String>,
    remote_access_enabled: bool,
    access: crate::network_diagnostics::RustyfinNetworkAccess,
    interface: crate::network_diagnostics::NetworkNodeSummary,
}

#[derive(Debug, Serialize)]
struct NetworkDefaultRouteSummary {
    route: String,
    gateway: Option<String>,
    interface: Option<String>,
    source: Option<String>,
    metric: Option<u32>,
    protocol: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct NetworkDefaultRouteEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    routes: Vec<NetworkDefaultRouteSummary>,
}

#[derive(Debug, Serialize)]
struct NetworkHostnameAliasSummary {
    name: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct NetworkHostnameAliasesEnvelope {
    query: Option<String>,
    matched_by: String,
    host_label: Option<String>,
    canonical_hostname: Option<String>,
    fqdn: Option<String>,
    total_count: usize,
    aliases: Vec<NetworkHostnameAliasSummary>,
}

#[derive(Debug, Serialize)]
struct NetworkDnsServerSummary {
    scope: String,
    interface: Option<String>,
    server: String,
    source: String,
    raw_line: String,
}

#[derive(Debug, Serialize)]
struct NetworkDnsServersEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    dns_servers: Vec<NetworkDnsServerSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct SystemPortConflictProcessSummary {
    name: String,
    pid: Option<u32>,
    fd: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct SystemPortConflictSummary {
    protocol: String,
    state: String,
    local_address: String,
    local_port: Option<u16>,
    peer_address: Option<String>,
    raw_entry: String,
    processes: Vec<SystemPortConflictProcessSummary>,
}

#[derive(Debug, Serialize)]
struct SystemPortConflictsEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    conflicts: Vec<SystemPortConflictSummary>,
}

#[derive(Debug, Serialize)]
struct SystemPortConflictDetailEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    #[serde(flatten)]
    conflict: SystemPortConflictSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemFailedUnitSummary {
    name: String,
    load: String,
    active: String,
    sub: String,
    description: String,
    recent_log_excerpt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SystemFailedUnitsEnvelope {
    query: Option<String>,
    matched_by: String,
    total_count: usize,
    units: Vec<SystemFailedUnitSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SystemFailedUnitDetailStatusSummary {
    fragment_path: Option<String>,
    unit_file_state: Option<String>,
    main_pid: Option<u32>,
    exec_main_code: Option<String>,
    exec_main_status: Option<String>,
    status_excerpt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SystemFailedUnitDetailSummary {
    unit: SystemFailedUnitSummary,
    status: SystemFailedUnitDetailStatusSummary,
}

#[derive(Debug, Serialize)]
struct SystemFailedUnitDetailEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    detail: SystemFailedUnitDetailSummary,
}

#[derive(Debug, Serialize)]
struct SystemProcessDetailEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    processes: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct SystemListenerDetailEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    listeners: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct SystemDiskUsageDetailEnvelope {
    query: String,
    matched_by: String,
    mount_point: String,
    source: String,
    fs_type: String,
    root: String,
    mount_id: u64,
    parent_id: u64,
    major_minor: String,
    options: String,
    super_options: String,
    total_bytes: u64,
    free_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
    used_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
struct LibraryItemMatch {
    library_id: String,
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct LibraryItemDetailSummary {
    library_id: String,
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
    overview: Option<String>,
    duration_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
struct LibraryItemMediaDetailSummary {
    query: String,
    matched_by: String,
    library_id: String,
    id: String,
    title: String,
    kind: String,
    year: Option<i64>,
    library_name: Option<String>,
    overview: Option<String>,
    duration_ms: Option<i64>,
    parent_id: Option<String>,
    media_path: Option<String>,
    resolved_media_path: Option<String>,
    first_descendant_media_path: Option<String>,
    source_paths: Vec<String>,
    poster_url: Option<String>,
    backdrop_url: Option<String>,
    logo_url: Option<String>,
    thumb_url: Option<String>,
    created_ts: i64,
    updated_ts: i64,
}

fn push_unique_nonempty_string(target: &mut Vec<String>, candidate: Option<String>) {
    let Some(candidate) = candidate
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    if target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&candidate))
    {
        return;
    }
    target.push(candidate);
}

fn collect_library_item_source_paths(
    media_path: Option<String>,
    resolved_media_path: Option<String>,
    first_descendant_media_path: Option<String>,
) -> Vec<String> {
    let mut source_paths = Vec::new();
    push_unique_nonempty_string(&mut source_paths, media_path);
    push_unique_nonempty_string(&mut source_paths, resolved_media_path);
    push_unique_nonempty_string(&mut source_paths, first_descendant_media_path);
    source_paths
}

#[derive(Debug, Serialize)]
struct LibraryPathSummary {
    id: String,
    path: String,
    is_read_only: bool,
}

#[derive(Debug, Serialize)]
struct LibrarySettingsSummary {
    show_images: bool,
    prefer_local_artwork: bool,
    fetch_online_artwork: bool,
    tmdb_store_in_media_dir: bool,
    tmdb_sync_on_new_media: bool,
    tmdb_sync_schedule: String,
    tmdb_last_sync_ts: Option<i64>,
    tmdb_fetch_posters: bool,
    tmdb_fetch_backdrops: bool,
    tmdb_fetch_metadata: bool,
    tmdb_fetch_reviews: bool,
}

#[derive(Debug, Serialize)]
struct LibraryDetailSummary {
    query: Option<String>,
    matched_by: String,
    id: String,
    name: String,
    kind: String,
    item_count: i64,
    paths: Vec<LibraryPathSummary>,
    settings: LibrarySettingsSummary,
    created_ts: i64,
    updated_ts: i64,
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
struct CalendarEventSeriesSummary {
    query: String,
    matched_by: String,
    title: String,
    event_type: String,
    recurrence: String,
    scope: String,
    owner_username: Option<String>,
    total_count: usize,
    first_event_date: Option<String>,
    last_event_date: Option<String>,
    occurrences: Vec<CalendarEventSummary>,
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
struct CalendarEventOccurrenceSummary {
    title: String,
    event_date: String,
    occurs_on: String,
    scope: String,
    event_type: String,
    owner_username: Option<String>,
}

#[derive(Debug, Serialize)]
struct CalendarConflictDaySummary {
    date: String,
    event_count: usize,
    events: Vec<CalendarEventOccurrenceSummary>,
}

#[derive(Debug, Serialize)]
struct CalendarFreeDaySummary {
    date: String,
}

#[derive(Debug, Serialize)]
struct CalendarDayCountSummary {
    date: String,
    event_count: usize,
}

#[derive(Debug, Serialize)]
struct CalendarBusyDaySummary {
    date: String,
    event_count: usize,
    events: Vec<CalendarEventOccurrenceSummary>,
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
    entry_id: String,
    citation_id: String,
    channel_id: String,
    session_id: String,
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
    library_id: String,
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
    unix_timestamp: i64,
}

#[derive(Debug, Serialize)]
struct PublicWebSearchSummary {
    query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
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
struct ServiceDetailSummary {
    query: String,
    matched_by: String,
    component: ServiceHealthComponentSummary,
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

#[derive(Debug, Serialize)]
struct StoragePathDetailSummary {
    query: String,
    matched_by: String,
    #[serde(flatten)]
    path: StoragePathSummary,
}

#[derive(Debug, Serialize)]
struct StorageMountDetailEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    #[serde(flatten)]
    mount: StorageMountSummary,
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
    let profile = ToolExecutionProfile::full_access();
    execute_tool_raw(state, context, call, &profile).await
}

pub async fn execute_tool_raw(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
    profile: &ToolExecutionProfile,
) -> AssistantToolContextBlock {
    execute_tool_with_profile(state, context, call, profile).await
}

pub async fn execute_tool_with_profile(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
    profile: &ToolExecutionProfile,
) -> AssistantToolContextBlock {
    let registry = default_tool_registry();
    let spec = registry
        .entry(call.tool)
        .map(|entry| {
            debug_assert_eq!(entry.tool, call.tool);
            entry.spec
        })
        .unwrap_or_else(|| call.tool.spec());

    if let Some(message) = profile.denial_reason(call.tool, spec) {
        return tool_error_block(spec, message);
    }
    if let Some(message) = enforce_tool_policy(context, spec) {
        return tool_error_block(spec, message);
    }

    let Some(provider) = registry.provider_for_tool(call.tool) else {
        return tool_error_block(
            spec,
            format!("{} is not registered with an internal provider.", spec.name),
        );
    };

    provider.execute(state, context, call).await
}

pub fn tool_result_to_outcome(
    message: &str,
    call: &PlannedToolCall,
    block: AssistantToolContextBlock,
) -> AssistantToolOutcome {
    normalize_tool_result(message, call, block)
}

fn tool_context_block_for_result(
    tool: AssistantToolName,
    result: Result<(String, serde_json::Value), String>,
) -> AssistantToolContextBlock {
    let spec = tool.spec();
    match result {
        Ok((label, data)) => AssistantToolContextBlock {
            tool: spec.name,
            label,
            status: "ok",
            data,
        },
        Err(message) => tool_error_block(spec, message),
    }
}

fn tool_error_block(
    spec: super::types::AssistantToolSpec,
    message: impl Into<String>,
) -> AssistantToolContextBlock {
    AssistantToolContextBlock {
        tool: spec.name,
        label: spec.summary.to_string(),
        status: "error",
        data: json!({ "message": message.into() }),
    }
}

pub(crate) async fn execute_account_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::AccountGetProfileSummary => {
            account_get_profile_summary(state, context).await
        }
        _ => Err(format!(
            "{} is not handled by the account provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_dictionary_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::DictionaryGetAccountIdentity => {
            dictionary_get_account_identity(state, context).await
        }
        AssistantToolName::DictionaryListVisibleWorkspaces => {
            dictionary_list_visible_workspaces(state, context).await
        }
        AssistantToolName::DictionaryBrowseWorkspacePeople => {
            dictionary_browse_workspace_people(state, context, call).await
        }
        AssistantToolName::DictionarySearchPeople => {
            dictionary_search_people(state, context, call).await
        }
        AssistantToolName::DictionaryGetPersonBundle => {
            dictionary_get_person_bundle(state, context, call).await
        }
        AssistantToolName::DictionaryResolveRelationshipReference => {
            dictionary_resolve_relationship_reference(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the dictionary provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_ai_runtime_provider_tool(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::AiListBackgroundJobs => ai_list_background_jobs(state).await,
        AssistantToolName::AiGetJobStatus => ai_get_job_status(state).await,
        AssistantToolName::AiGetToolRegistry => ai_get_tool_registry().await,
        AssistantToolName::AiGetGroundingSummary => ai_get_grounding_summary(state).await,
        AssistantToolName::AiGetLastToolFailureReason => {
            ai_get_last_tool_failure_reason(state).await
        }
        _ => Err(format!(
            "{} is not handled by the AI runtime provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_memory_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::MemoryListRecentFacts => memory_list_recent_facts(state, context).await,
        AssistantToolName::MemoryListRecentEntities => {
            memory_list_recent_entities(state, context).await
        }
        AssistantToolName::MemorySearchFacts => memory_search_facts(state, context, call).await,
        AssistantToolName::MemorySearchEntities => {
            memory_search_entities(state, context, call).await
        }
        AssistantToolName::MemoryFindExactEntity => {
            memory_find_exact_entity(state, context, call).await
        }
        AssistantToolName::MemoryGetEntityRelations => {
            memory_get_entity_relations(state, context, call).await
        }
        AssistantToolName::MemoryGetEntityRelationPath => {
            memory_get_entity_relation_path(state, context, call).await
        }
        AssistantToolName::MemoryListRecentChanges => {
            memory_list_recent_changes(state, context, call).await
        }
        AssistantToolName::MemoryListConflictingFacts => {
            memory_list_conflicting_facts(state, context, call).await
        }
        AssistantToolName::MemoryGetEntityProvenance => {
            memory_get_entity_provenance(state, context, call).await
        }
        AssistantToolName::MemoryGetPersonSummary => {
            memory_get_person_summary(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the memory provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_calendar_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::CalendarListEvents => calendar_list_events(state, context, call).await,
        AssistantToolName::CalendarGetNextEvent => calendar_get_next_event(state, context).await,
        AssistantToolName::CalendarListDateConflicts => {
            calendar_list_date_conflicts(state, context, call).await
        }
        AssistantToolName::CalendarListFreeDays => {
            calendar_list_free_days(state, context, call).await
        }
        AssistantToolName::CalendarGetNextFreeDay => {
            calendar_get_next_free_day(state, context, call).await
        }
        AssistantToolName::CalendarGetEventByExactDateAndTitle => {
            calendar_get_event_by_exact_date_and_title(state, context, call).await
        }
        AssistantToolName::CalendarGetEventSeriesSummary => {
            calendar_get_event_series_summary(state, context, call).await
        }
        AssistantToolName::CalendarGetNextFreeSlot => {
            calendar_get_next_free_slot(state, context, call).await
        }
        AssistantToolName::CalendarListBusySlots => {
            calendar_list_busy_slots(state, context, call).await
        }
        AssistantToolName::CalendarGetNextEventTiming => {
            calendar_get_next_event_timing(state, context).await
        }
        AssistantToolName::CalendarCountEvents => calendar_count_events(state, context, call).await,
        AssistantToolName::CalendarListBusyDays => {
            calendar_list_busy_days(state, context, call).await
        }
        AssistantToolName::CalendarListOverlappingEvents => {
            calendar_list_overlapping_events(state, context, call).await
        }
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
        _ => Err(format!(
            "{} is not handled by the calendar provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_channels_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::ChannelsListUnreadActivity => {
            channels_list_unread_activity(state, context, call).await
        }
        AssistantToolName::ChannelsGetTranscriptSummary => {
            channels_get_transcript_summary(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the channels provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_documents_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::DocumentCreateDownload => {
            document_create_download(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the documents provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_conversations_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::ConversationsArchiveSelection => {
            conversations_archive_selection(state, context, call).await
        }
        AssistantToolName::ConversationsDeleteSelection => {
            conversations_delete_selection(state, context, call).await
        }
        AssistantToolName::ConversationsMoveToGroupSelection => {
            conversations_move_to_group_selection(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the conversations provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_downloads_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::DownloadsListAvailableArtifacts => {
            downloads_list_available_artifacts(state, context, call).await
        }
        AssistantToolName::DownloadsGetArtifactDetails => {
            downloads_get_artifact_details(state, context, call).await
        }
        AssistantToolName::DownloadsGetArtifactSource => {
            downloads_get_artifact_source(state, context, call).await
        }
        AssistantToolName::DownloadsGetReleaseNotes => {
            downloads_get_release_notes(state, context, call).await
        }
        AssistantToolName::DownloadsGetArtifactChecksum => {
            downloads_get_artifact_checksum(state, context, call).await
        }
        AssistantToolName::DownloadsGetArtifactInstallSteps => {
            downloads_get_artifact_install_steps(state, context, call).await
        }
        AssistantToolName::DownloadsGetArtifactCompatibility => {
            downloads_get_artifact_compatibility(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the downloads provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_libraries_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::LibrariesListAccessible => {
            libraries_list_accessible(state, context).await
        }
        AssistantToolName::LibrariesGetLibrarySummary => {
            libraries_get_library_summary(state, context, call).await
        }
        AssistantToolName::LibrarySearchTitles => library_search_titles(state, context, call).await,
        AssistantToolName::LibraryGetItemSummary => {
            library_get_item_summary(state, context, call).await
        }
        AssistantToolName::LibraryGetItemMediaDetails => {
            library_get_item_media_details(state, context, call).await
        }
        AssistantToolName::LibraryGetItemSourcePaths => {
            library_get_item_source_paths(state, context, call).await
        }
        AssistantToolName::LibrariesGetRecentlyAdded => {
            libraries_get_recently_added(state, context, call).await
        }
        AssistantToolName::LibrariesFindDuplicateTitles => {
            libraries_find_duplicate_titles(state, context).await
        }
        AssistantToolName::LibrariesListMissingMetadata => {
            libraries_list_missing_metadata(state, context).await
        }
        _ => Err(format!(
            "{} is not handled by the libraries provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_network_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::NetworkGetTopologySummary => {
            network_get_topology_summary(state, context).await
        }
        AssistantToolName::NetworkGetInterfaceDetails => {
            network_get_interface_details(state, context, call).await
        }
        AssistantToolName::NetworkGetInterfaceByIp => {
            network_get_interface_by_ip(state, context, call).await
        }
        AssistantToolName::NetworkGetDefaultRoute => {
            network_get_default_route(state, context, call).await
        }
        AssistantToolName::NetworkGetHostnameAliases => {
            network_get_hostname_aliases(state, context, call).await
        }
        AssistantToolName::NetworkGetDnsServers => {
            network_get_dns_servers(state, context, call).await
        }
        AssistantToolName::NetworkGetRouteTable => diagnostics::network_get_route_table().await,
        AssistantToolName::NetworkGetActiveConnections => {
            diagnostics::network_get_active_connections().await
        }
        AssistantToolName::NetworkGetInterfaceCounters => {
            diagnostics::network_get_interface_counters().await
        }
        AssistantToolName::NetworkGetWifiStatus => diagnostics::network_get_wifi_status().await,
        AssistantToolName::NetworkGetVpnStatus => diagnostics::network_get_vpn_status().await,
        _ => Err(format!(
            "{} is not handled by the network provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_rooms_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::RoomsListActive => rooms_list_active(state, context, call).await,
        AssistantToolName::RoomsListJoinable => rooms_list_joinable(state, context, call).await,
        AssistantToolName::RoomsGetRoomSummary => room_get_room_summary(state, context, call).await,
        _ => Err(format!(
            "{} is not handled by the rooms provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_servers_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::ServersListMinecraftStatus => {
            servers_list_minecraft_status(state, context, call).await
        }
        AssistantToolName::ServersGetMinecraftServerSummary => {
            server_get_minecraft_server_summary(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the servers provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_system_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::SystemGetCurrentDateTime => system_get_current_datetime().await,
        AssistantToolName::SystemGetAiRuntimeSummary => system_get_ai_runtime_summary(state).await,
        AssistantToolName::SystemGetHostRuntimeSummary => {
            system_get_host_runtime_summary(state, context).await
        }
        AssistantToolName::SystemGetBackupSummary => system_get_backup_summary(state).await,
        AssistantToolName::SystemGetServiceHealth => system_get_service_health(state).await,
        AssistantToolName::SystemGetServiceDetail => {
            system_get_service_detail(state, context, call).await
        }
        AssistantToolName::SystemGetTranscodeSummary => system_get_transcode_summary(state).await,
        AssistantToolName::SystemGetStorageSummary => system_get_storage_summary(state).await,
        AssistantToolName::SystemGetStoragePathDetail => {
            system_get_storage_path_detail(state, context, call).await
        }
        AssistantToolName::SystemGetMountDetail => {
            system_get_mount_detail(state, context, call).await
        }
        AssistantToolName::SystemGetPortConflicts => {
            system_get_port_conflicts(state, context, call).await
        }
        AssistantToolName::SystemGetPortConflictDetail => {
            system_get_port_conflict_detail(state, context, call).await
        }
        AssistantToolName::SystemGetFailedUnits => {
            system_get_failed_units(state, context, call).await
        }
        AssistantToolName::SystemGetFailedUnitDetail => {
            system_get_failed_unit_detail(state, context, call).await
        }
        AssistantToolName::SystemGetRecentErrors => system_get_recent_errors(state).await,
        AssistantToolName::SystemGetKernelInfo => diagnostics::system_get_kernel_info().await,
        AssistantToolName::SystemGetCpuTopology => diagnostics::system_get_cpu_topology().await,
        AssistantToolName::SystemGetTemperatureSensors => {
            diagnostics::system_get_temperature_sensors().await
        }
        AssistantToolName::SystemGetBlockDeviceInventory => {
            diagnostics::system_get_block_device_inventory().await
        }
        AssistantToolName::SystemGetFilesystemTable => {
            diagnostics::system_get_filesystem_table().await
        }
        AssistantToolName::SystemGetGpuInventory => diagnostics::system_get_gpu_inventory().await,
        AssistantToolName::SystemGetPciDevices => diagnostics::system_get_pci_devices().await,
        AssistantToolName::SystemGetUsbDevices => diagnostics::system_get_usb_devices().await,
        AssistantToolName::SystemGetBootLogSummary => {
            diagnostics::system_get_boot_log_summary().await
        }
        AssistantToolName::SystemGetJournalSummary => {
            diagnostics::system_get_journal_summary().await
        }
        AssistantToolName::SystemGetProcessDetail => {
            system_get_process_detail(state, context, call).await
        }
        AssistantToolName::SystemGetListenerDetail => {
            system_get_listener_detail(state, context, call).await
        }
        AssistantToolName::SystemGetDiskUsageDetail => {
            system_get_disk_usage_detail(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the system provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_weather_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::WeatherGetCurrent => weather_get_current(state, context, call).await,
        AssistantToolName::WeatherGetForecast => weather_get_forecast(state, context, call).await,
        AssistantToolName::WeatherGetHistory => weather_get_history(state, context, call).await,
        AssistantToolName::WeatherGetHourlyWindow => {
            weather_get_hourly_window(state, context, call).await
        }
        AssistantToolName::WeatherResolveLocationAlias => {
            weather_resolve_location_alias(state, context, call).await
        }
        AssistantToolName::WeatherGetForecastForDate => {
            weather_get_forecast_for_date(state, context, call).await
        }
        AssistantToolName::WeatherGetRecentHistoryForDate => {
            weather_get_recent_history_for_date(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the weather provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
}

pub(crate) async fn execute_web_provider_tool(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> AssistantToolContextBlock {
    let result = match call.tool {
        AssistantToolName::WebListCuratedSources => {
            web_list_curated_sources(state, context, call).await
        }
        AssistantToolName::WebSearchPublicWeb => web_search_public_web(state, context, call).await,
        AssistantToolName::WebFetchPublicPageSummary => {
            web_fetch_public_page_summary(state, context, call).await
        }
        _ => Err(format!(
            "{} is not handled by the web provider.",
            call.tool.as_str()
        )),
    };
    tool_context_block_for_result(call.tool, result)
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
            .get("download_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_file_name: block
            .data
            .get("file_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_media_type: block
            .data
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        download_size_bytes: block
            .data
            .get("size_bytes")
            .and_then(serde_json::Value::as_i64),
    }
}

pub fn build_follow_up_context(
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
) -> AssistantFollowUpContext {
    let mut input_hint = follow_up_input_hint(call);
    if matches!(
        call.tool,
        AssistantToolName::WeatherGetCurrent
            | AssistantToolName::WeatherGetForecast
            | AssistantToolName::WeatherGetHistory
            | AssistantToolName::WeatherResolveLocationAlias
            | AssistantToolName::WeatherGetForecastForDate
            | AssistantToolName::WeatherGetRecentHistoryForDate
    ) {
        input_hint.weather_location = block
            .data
            .get("resolved_location")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(input_hint.weather_location);
    }
    if call.tool == AssistantToolName::CalendarGetNextEvent
        || call.tool == AssistantToolName::CalendarGetNextEventTiming
    {
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
        AssistantToolName::CalendarListDateConflicts | AssistantToolName::CalendarListFreeDays
    ) {
        input_hint.calendar_label = Some(block.label.clone());
        let first_date = if call.tool == AssistantToolName::CalendarListDateConflicts {
            block
                .data
                .get("conflict_days")
                .and_then(serde_json::Value::as_array)
                .and_then(|days| days.first())
                .and_then(|day| day.get("date"))
        } else {
            block
                .data
                .get("free_days")
                .and_then(serde_json::Value::as_array)
                .and_then(|days| days.first())
                .and_then(|day| day.get("date"))
        };
        input_hint.calendar_from_date = first_date
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        input_hint.calendar_to_date = input_hint.calendar_from_date.clone();
    } else if matches!(
        call.tool,
        AssistantToolName::CalendarCountEvents | AssistantToolName::CalendarListBusyDays
    ) {
        input_hint.calendar_label = Some(block.label.clone());
        let first_date = if call.tool == AssistantToolName::CalendarCountEvents {
            block
                .data
                .get("day_counts")
                .and_then(serde_json::Value::as_array)
                .and_then(|days| days.first())
                .and_then(|day| day.get("date"))
        } else {
            block
                .data
                .get("busy_days")
                .and_then(serde_json::Value::as_array)
                .and_then(|days| days.first())
                .and_then(|day| day.get("date"))
        };
        input_hint.calendar_from_date = first_date
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        input_hint.calendar_to_date = input_hint.calendar_from_date.clone();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DictionaryReferenceKind {
    Mother,
    Father,
    Parent,
    Brother,
    Sister,
    Sibling,
    Spouse,
    Partner,
    Friend,
    Coworker,
    Child,
    Grandparent,
}

impl DictionaryReferenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mother => "mother",
            Self::Father => "father",
            Self::Parent => "parent",
            Self::Brother => "brother",
            Self::Sister => "sister",
            Self::Sibling => "sibling",
            Self::Spouse => "spouse",
            Self::Partner => "partner",
            Self::Friend => "friend",
            Self::Coworker => "coworker",
            Self::Child => "child",
            Self::Grandparent => "grandparent",
        }
    }

    fn prefers_plural(self, reference: &str) -> bool {
        let lower = reference.to_ascii_lowercase();
        matches!(self, Self::Coworker)
            || lower.contains("co-workers")
            || lower.contains("coworkers")
            || lower.contains("colleagues")
            || lower.contains("friends")
            || lower.contains("siblings")
            || lower.contains("parents")
            || lower.contains("grandparents")
            || lower.contains("children")
    }
}

fn classify_dictionary_reference(reference: &str) -> Option<DictionaryReferenceKind> {
    let lower = reference.to_ascii_lowercase();
    if lower.contains("my mother") || lower.contains("my mum") || lower.contains("my mom") {
        return Some(DictionaryReferenceKind::Mother);
    }
    if lower.contains("my father") || lower.contains("my dad") {
        return Some(DictionaryReferenceKind::Father);
    }
    if lower.contains("my parent") || lower.contains("my parents") {
        return Some(DictionaryReferenceKind::Parent);
    }
    if lower.contains("my brother") {
        return Some(DictionaryReferenceKind::Brother);
    }
    if lower.contains("my sister") {
        return Some(DictionaryReferenceKind::Sister);
    }
    if lower.contains("my sibling") || lower.contains("my siblings") {
        return Some(DictionaryReferenceKind::Sibling);
    }
    if lower.contains("my spouse") || lower.contains("my wife") || lower.contains("my husband") {
        return Some(DictionaryReferenceKind::Spouse);
    }
    if lower.contains("my partner") {
        return Some(DictionaryReferenceKind::Partner);
    }
    if lower.contains("my co-worker")
        || lower.contains("my co-workers")
        || lower.contains("my coworker")
        || lower.contains("my coworkers")
        || lower.contains("my colleague")
        || lower.contains("my colleagues")
    {
        return Some(DictionaryReferenceKind::Coworker);
    }
    if lower.contains("my friend") || lower.contains("my friends") {
        return Some(DictionaryReferenceKind::Friend);
    }
    if lower.contains("my child")
        || lower.contains("my children")
        || lower.contains("my son")
        || lower.contains("my daughter")
    {
        return Some(DictionaryReferenceKind::Child);
    }
    if lower.contains("my grandparent") || lower.contains("my grandparents") {
        return Some(DictionaryReferenceKind::Grandparent);
    }
    None
}

fn dictionary_person_summary_from_row(
    row: &dictionary_repo::DictionaryPersonRow,
) -> DictionaryPersonSummary {
    DictionaryPersonSummary {
        id: row.id.clone(),
        display_name: row.display_name.clone(),
        canonical_name: row.canonical_name.clone(),
        summary: row.summary.clone(),
    }
}

fn dictionary_fact_summary_from_row(
    row: &dictionary_repo::DictionaryFactRow,
) -> DictionaryFactSummary {
    DictionaryFactSummary {
        fact_key: row.fact_key.clone(),
        value_type: row.value_type.clone(),
        value_text: row.value_text.clone(),
        value_int: row.value_int,
        value_bool: row.value_bool,
        value_date: row.value_date.clone(),
        value_json: row.value_json.clone(),
    }
}

fn dictionary_relation_summary_from_row(
    row: &dictionary_repo::DictionaryResolvedRelationRow,
) -> DictionaryRelationSummary {
    DictionaryRelationSummary {
        relation_id: row.relation_id.clone(),
        relation_group_key: row.relation_group_key.clone(),
        relation_type: row.relation_type.clone(),
        direction: row.direction.clone(),
        other_person_id: row.other_person.id.clone(),
        other_person_name: row.other_person.display_name.clone(),
        other_person_summary: row.other_person.summary.clone(),
    }
}

fn dictionary_gender_from_facts(facts: &[dictionary_repo::DictionaryFactRow]) -> Option<&str> {
    facts.iter().find_map(|fact| {
        if !matches!(fact.fact_key.as_str(), "gender" | "sex") {
            return None;
        }
        fact.value_text
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .and_then(|value| match value.as_str() {
                "female" | "woman" | "girl" => Some("female"),
                "male" | "man" | "boy" => Some("male"),
                _ => None,
            })
    })
}

fn dictionary_birthday_from_facts(facts: &[dictionary_repo::DictionaryFactRow]) -> Option<String> {
    facts
        .iter()
        .find(|fact| fact.fact_key == "birthday")
        .and_then(|fact| fact.value_date.clone())
}

fn dictionary_hobbies_from_facts(facts: &[dictionary_repo::DictionaryFactRow]) -> Vec<String> {
    facts
        .iter()
        .find(|fact| fact.fact_key == "hobbies")
        .and_then(|fact| fact.value_json.clone())
        .and_then(|value| value.as_array().cloned())
        .map(|items| {
            items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::trim).map(str::to_string))
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn dictionary_document_excerpt(
    document: &Option<dictionary_repo::DictionaryDocumentRow>,
) -> Option<String> {
    document
        .as_ref()
        .map(|row| compact_text(&row.markdown_body, 220))
        .filter(|value| !value.trim().is_empty())
}

fn dictionary_workspace_id_for_kind(
    account_link: &dictionary_repo::DictionaryAccountLinkRow,
    kind: DictionaryReferenceKind,
) -> Option<String> {
    match kind {
        DictionaryReferenceKind::Coworker => account_link.work_workspace_id.clone(),
        DictionaryReferenceKind::Friend => account_link.friends_workspace_id.clone(),
        _ => account_link.family_workspace_id.clone(),
    }
}

fn dictionary_workspace_selector_matches(
    workspace: &dictionary_repo::DictionaryWorkspaceRow,
    selector: &str,
) -> bool {
    if workspace.id == selector {
        return true;
    }

    let normalized_selector = selector.trim().to_ascii_lowercase();
    if normalized_selector.is_empty() {
        return false;
    }

    if workspace.slug.eq_ignore_ascii_case(&normalized_selector)
        || workspace.title.to_ascii_lowercase() == normalized_selector
    {
        return true;
    }

    matches!(
        (
            workspace.workspace_kind.as_str(),
            normalized_selector.as_str()
        ),
        (
            "family_shared",
            "family" | "family_shared" | "family-shared" | "family shared"
        ) | (
            "friends_private",
            "friends" | "friend" | "friends_private" | "friends-private" | "friends private"
        ) | (
            "work_private",
            "work" | "work_private" | "work-private" | "work private"
        )
    )
}

async fn resolve_visible_dictionary_workspace(
    state: &AppState,
    user_id: &str,
    workspace_selector: &str,
) -> Result<dictionary_repo::DictionaryWorkspaceRow, String> {
    if let Some(workspace) = dictionary_repo::find_workspace_by_id(&state.db, workspace_selector)
        .await
        .map_err(|e| format!("failed to load dictionary workspace: {e}"))?
    {
        let visible = dictionary_repo::user_can_access_workspace(&state.db, &workspace.id, user_id)
            .await
            .map_err(|e| format!("failed to validate dictionary workspace access: {e}"))?;
        if visible {
            return Ok(workspace);
        }
    }

    dictionary_repo::list_visible_workspaces(&state.db, user_id)
        .await
        .map_err(|e| format!("failed to list visible dictionary workspaces: {e}"))?
        .into_iter()
        .find(|workspace| dictionary_workspace_selector_matches(workspace, workspace_selector))
        .ok_or_else(|| "dictionary workspace access denied".to_string())
}

fn dictionary_relation_matches_kind(
    kind: DictionaryReferenceKind,
    relation: &dictionary_repo::DictionaryResolvedRelationRow,
    facts: &[dictionary_repo::DictionaryFactRow],
    sibling_pool_len: usize,
    parent_pool_len: usize,
) -> bool {
    match kind {
        DictionaryReferenceKind::Mother => {
            if relation.relation_type != "child_of" {
                return false;
            }
            matches!(dictionary_gender_from_facts(facts), Some("female")) || parent_pool_len == 1
        }
        DictionaryReferenceKind::Father => {
            if relation.relation_type != "child_of" {
                return false;
            }
            matches!(dictionary_gender_from_facts(facts), Some("male")) || parent_pool_len == 1
        }
        DictionaryReferenceKind::Parent => relation.relation_type == "child_of",
        DictionaryReferenceKind::Brother => {
            if relation.relation_type != "sibling_of" {
                return false;
            }
            matches!(dictionary_gender_from_facts(facts), Some("male")) || sibling_pool_len == 1
        }
        DictionaryReferenceKind::Sister => {
            if relation.relation_type != "sibling_of" {
                return false;
            }
            matches!(dictionary_gender_from_facts(facts), Some("female")) || sibling_pool_len == 1
        }
        DictionaryReferenceKind::Sibling => relation.relation_type == "sibling_of",
        DictionaryReferenceKind::Spouse | DictionaryReferenceKind::Partner => {
            relation.relation_type == "spouse_of"
        }
        DictionaryReferenceKind::Friend => relation.relation_type == "friend_of",
        DictionaryReferenceKind::Coworker => matches!(
            relation.relation_type.as_str(),
            "coworker_of" | "manager_of" | "reports_to"
        ),
        DictionaryReferenceKind::Child => matches!(
            relation.relation_type.as_str(),
            "mother_of" | "father_of" | "parent_of"
        ),
        DictionaryReferenceKind::Grandparent => matches!(
            relation.relation_type.as_str(),
            "grandparent_of" | "grandchild_of"
        ),
    }
}

async fn dictionary_get_account_identity(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, Value), String> {
    let link = dictionary_repo::get_account_link(&state.db, &context.user_id)
        .await
        .map_err(|e| format!("failed to load dictionary account link: {e}"))?;

    let (person_id, person_name, family_workspace_id, friends_workspace_id, work_workspace_id) =
        if let Some(link) = link {
            let person_name = dictionary_repo::find_person_by_id(&state.db, &link.person_id)
                .await
                .map_err(|e| format!("failed to load linked dictionary person: {e}"))?
                .map(|row| row.display_name);
            (
                Some(link.person_id),
                person_name,
                link.family_workspace_id,
                link.friends_workspace_id,
                link.work_workspace_id,
            )
        } else {
            (None, None, None, None, None)
        };

    Ok((
        "Linked Human Dictionary identity".to_string(),
        serde_json::to_value(DictionaryAccountIdentityEnvelope {
            linked: person_id.is_some(),
            person_id,
            person_name,
            family_workspace_id,
            friends_workspace_id,
            work_workspace_id,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn dictionary_list_visible_workspaces(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, Value), String> {
    let rows = dictionary_repo::list_visible_workspaces(&state.db, &context.user_id)
        .await
        .map_err(|e| format!("failed to list visible dictionary workspaces: {e}"))?;

    let envelope = DictionaryVisibleWorkspacesEnvelope {
        workspaces: rows
            .into_iter()
            .map(|row| DictionaryWorkspaceSummary {
                workspace_id: row.id,
                title: row.title,
                workspace_kind: row.workspace_kind,
                owner_user_id: row.owner_user_id,
                is_system_seeded: row.is_system_seeded,
            })
            .collect(),
    };

    Ok((
        "Visible Human Dictionary workspaces".to_string(),
        serde_json::to_value(envelope).unwrap_or_else(|_| json!({})),
    ))
}

async fn dictionary_browse_workspace_people(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, Value), String> {
    let (workspace_id, query, limit) = match &call.input {
        AssistantToolInput::DictionaryBrowseWorkspacePeople {
            workspace_id,
            query,
            limit,
        } => (workspace_id, query.clone(), limit.unwrap_or(20)),
        _ => {
            return Err(
                "dictionary_browse_workspace_people requires a workspace_id and optional query"
                    .to_string(),
            );
        }
    };

    let workspace =
        resolve_visible_dictionary_workspace(state, &context.user_id, workspace_id).await?;

    let rows = match query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(normalized_query) => dictionary_repo::search_visible_people(
            &state.db,
            &dictionary_repo::SearchVisiblePeopleParams {
                workspace_id: workspace.id.clone(),
                query: normalized_query.to_string(),
                limit,
            },
        )
        .await
        .map_err(|e| format!("failed to search dictionary people: {e}"))?,
        None => dictionary_repo::list_visible_people(&state.db, &workspace.id, limit)
            .await
            .map_err(|e| format!("failed to browse dictionary people: {e}"))?,
    };

    let envelope = DictionaryWorkspacePeopleEnvelope {
        workspace_id: workspace.id,
        workspace_title: workspace.title,
        query,
        people: rows
            .iter()
            .map(dictionary_person_summary_from_row)
            .collect(),
    };

    Ok((
        "Visible Human Dictionary people in workspace".to_string(),
        serde_json::to_value(envelope).unwrap_or_else(|_| json!({})),
    ))
}

async fn dictionary_search_people(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, Value), String> {
    let (workspace_id, query, limit) = match &call.input {
        AssistantToolInput::DictionarySearchPeople {
            workspace_id,
            query,
            limit,
        } => (workspace_id, query, limit.unwrap_or(12)),
        _ => {
            return Err("dictionary_search_people requires a workspace_id and query".to_string());
        }
    };

    let workspace =
        resolve_visible_dictionary_workspace(state, &context.user_id, workspace_id).await?;

    let rows = dictionary_repo::search_visible_people(
        &state.db,
        &dictionary_repo::SearchVisiblePeopleParams {
            workspace_id: workspace.id.clone(),
            query: query.clone(),
            limit,
        },
    )
    .await
    .map_err(|e| format!("failed to search dictionary people: {e}"))?;

    let people = rows
        .iter()
        .map(dictionary_person_summary_from_row)
        .collect::<Vec<_>>();

    Ok((
        "Visible Human Dictionary people".to_string(),
        json!({
            "workspace_id": workspace.id,
            "query": query,
            "people": people,
        }),
    ))
}

async fn dictionary_get_person_bundle(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, Value), String> {
    let (workspace_id, person_id) = match &call.input {
        AssistantToolInput::DictionaryGetPersonBundle {
            workspace_id,
            person_id,
        } => (workspace_id, person_id),
        _ => {
            return Err(
                "dictionary_get_person_bundle requires a workspace_id and person_id".to_string(),
            );
        }
    };

    let workspace =
        resolve_visible_dictionary_workspace(state, &context.user_id, workspace_id).await?;

    let nodes = dictionary_repo::list_tree_nodes_for_person(&state.db, &workspace.id, person_id)
        .await
        .map_err(|e| format!("failed to validate dictionary person visibility: {e}"))?;
    if nodes.is_empty() {
        return Err("dictionary person is not visible in that workspace".to_string());
    }

    let person = dictionary_repo::find_person_by_id(&state.db, person_id)
        .await
        .map_err(|e| format!("failed to load dictionary person: {e}"))?
        .ok_or_else(|| "dictionary person not found".to_string())?;
    let facts = dictionary_repo::list_facts_for_subject(
        &state.db,
        &workspace.id,
        SubjectKind::Person,
        person_id,
    )
    .await
    .map_err(|e| format!("failed to load dictionary facts: {e}"))?;
    let relations =
        dictionary_repo::list_resolved_relations_for_person(&state.db, &workspace.id, person_id)
            .await
            .map_err(|e| format!("failed to load dictionary relations: {e}"))?;
    let document = dictionary_repo::get_document_for_subject(
        &state.db,
        &workspace.id,
        SubjectKind::Person,
        person_id,
    )
    .await
    .map_err(|e| format!("failed to load dictionary document: {e}"))?;

    let envelope = DictionaryPersonBundleEnvelope {
        workspace_id: workspace.id,
        person: dictionary_person_summary_from_row(&person),
        facts: facts.iter().map(dictionary_fact_summary_from_row).collect(),
        relations: relations
            .iter()
            .map(dictionary_relation_summary_from_row)
            .collect(),
        document_title: document.as_ref().map(|row| row.title.clone()),
        document_excerpt: dictionary_document_excerpt(&document),
    };

    Ok((
        "Visible Human Dictionary person bundle".to_string(),
        serde_json::to_value(envelope).unwrap_or_else(|_| json!({})),
    ))
}

async fn dictionary_resolve_relationship_reference(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, Value), String> {
    let (reference, workspace_override) = match &call.input {
        AssistantToolInput::DictionaryResolveRelationshipReference {
            reference,
            workspace_id,
        } => (reference, workspace_id.clone()),
        _ => {
            return Err(
                "dictionary_resolve_relationship_reference requires a relationship reference"
                    .to_string(),
            );
        }
    };

    let relation_kind = classify_dictionary_reference(reference)
        .ok_or_else(|| "unsupported dictionary relationship reference".to_string())?;

    let account_link = dictionary_repo::get_account_link(&state.db, &context.user_id)
        .await
        .map_err(|e| format!("failed to load dictionary account link: {e}"))?
        .ok_or_else(|| {
            "Link your Rustyfin account to a Human Dictionary person before using relationship-relative questions."
                .to_string()
        })?;

    let workspace_id = workspace_override
        .or_else(|| dictionary_workspace_id_for_kind(&account_link, relation_kind))
        .ok_or_else(|| {
            format!(
                "I couldn't find a default Human Dictionary workspace for {}.",
                relation_kind.as_str()
            )
        })?;

    let visible =
        dictionary_repo::user_can_access_workspace(&state.db, &workspace_id, &context.user_id)
            .await
            .map_err(|e| format!("failed to validate dictionary workspace access: {e}"))?;
    if !visible {
        return Err("dictionary workspace access denied".to_string());
    }

    let workspace = dictionary_repo::find_workspace_by_id(&state.db, &workspace_id)
        .await
        .map_err(|e| format!("failed to load dictionary workspace: {e}"))?
        .ok_or_else(|| "dictionary workspace not found".to_string())?;

    let linked_person = dictionary_repo::find_person_by_id(&state.db, &account_link.person_id)
        .await
        .map_err(|e| format!("failed to load linked dictionary person: {e}"))?
        .ok_or_else(|| "linked dictionary person not found".to_string())?;

    let visible_nodes = dictionary_repo::list_tree_nodes_for_person(
        &state.db,
        &workspace_id,
        &account_link.person_id,
    )
    .await
    .map_err(|e| format!("failed to validate linked dictionary person visibility: {e}"))?;
    if visible_nodes.is_empty() {
        return Err(format!(
            "I couldn't find your linked Human Dictionary person inside the {} workspace.",
            workspace.title
        ));
    }

    let relations = dictionary_repo::list_resolved_relations_for_person(
        &state.db,
        &workspace_id,
        &account_link.person_id,
    )
    .await
    .map_err(|e| format!("failed to load dictionary relations: {e}"))?;

    let sibling_pool_len = relations
        .iter()
        .filter(|relation| relation.relation_type == "sibling_of")
        .count();
    let parent_pool_len = relations
        .iter()
        .filter(|relation| relation.relation_type == "child_of")
        .count();

    let candidate_ids = relations
        .iter()
        .map(|relation| relation.other_person.id.clone())
        .collect::<Vec<_>>();
    let facts_by_person =
        dictionary_repo::list_facts_for_people(&state.db, &workspace_id, &candidate_ids)
            .await
            .map_err(|e| format!("failed to load candidate dictionary facts: {e}"))?;
    let documents_by_person =
        dictionary_repo::list_documents_for_people(&state.db, &workspace_id, &candidate_ids)
            .await
            .map_err(|e| format!("failed to load candidate dictionary document: {e}"))?;

    let mut candidates = Vec::new();
    for relation in relations {
        let facts = facts_by_person
            .get(&relation.other_person.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !dictionary_relation_matches_kind(
            relation_kind,
            &relation,
            facts,
            sibling_pool_len,
            parent_pool_len,
        ) {
            continue;
        }

        let document_excerpt = documents_by_person
            .get(&relation.other_person.id)
            .map(|row| compact_text(&row.markdown_body, 220))
            .filter(|value| !value.trim().is_empty());

        candidates.push(DictionaryResolvedCandidate {
            person_id: relation.other_person.id.clone(),
            display_name: relation.other_person.display_name.clone(),
            summary: relation.other_person.summary.clone(),
            relation_type: relation.relation_type.clone(),
            birthday: dictionary_birthday_from_facts(facts),
            hobbies: dictionary_hobbies_from_facts(facts),
            document_excerpt,
        });
    }

    let plural_expected = relation_kind.prefers_plural(reference);
    if candidates.is_empty() {
        let message = format!(
            "I couldn't find a visible Human Dictionary match for {} in {}.",
            reference.trim(),
            workspace.title
        );
        return Ok((
            "Human Dictionary relationship reference".to_string(),
            json!(DictionaryRelationshipResolutionEnvelope {
                reference: reference.clone(),
                relation_kind: relation_kind.as_str().to_string(),
                workspace_id: Some(workspace.id),
                workspace_title: Some(workspace.title),
                status: "not_found".to_string(),
                message: Some(message),
                linked_person_id: Some(linked_person.id),
                linked_person_name: Some(linked_person.display_name),
                candidates,
            }),
        ));
    }

    if !plural_expected && candidates.len() > 1 {
        let names = candidates
            .iter()
            .map(|candidate| candidate.display_name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(super::types::encode_assistant_clarification_message(
            &format!(
                "I found multiple visible Human Dictionary matches for {} in {}: {}. Which one did you mean?",
                reference.trim(),
                workspace.title,
                names
            ),
        ));
    }

    Ok((
        "Human Dictionary relationship reference".to_string(),
        json!(DictionaryRelationshipResolutionEnvelope {
            reference: reference.clone(),
            relation_kind: relation_kind.as_str().to_string(),
            workspace_id: Some(workspace.id),
            workspace_title: Some(workspace.title),
            status: if plural_expected {
                "list".to_string()
            } else {
                "resolved".to_string()
            },
            message: None,
            linked_person_id: Some(linked_person.id),
            linked_person_name: Some(linked_person.display_name),
            candidates,
        }),
    ))
}

async fn memory_list_recent_facts(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let rows =
        rustfin_db::repo::ai_grounding::list_memory_items_for_user(&state.db, &context.user_id, 12)
            .await
            .map_err(|e| format!("failed to load recent memory facts: {e}"))?;

    let facts = rows
        .into_iter()
        .map(memory_fact_summary_from_row)
        .collect::<Vec<_>>();

    Ok((
        "Recent stored memory facts".to_string(),
        serde_json::to_value(MemoryFactsEnvelope {
            query: None,
            topic_key: None,
            total_count: facts.len(),
            facts,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_search_facts(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing memory query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing memory query".to_string());
    }

    let hits = rustfin_db::repo::ai_grounding::search_memory_items_for_user(
        &state.db,
        &context.user_id,
        None,
        Some(query),
        12,
    )
    .await
    .map_err(|e| format!("failed to search memory facts: {e}"))?;

    let facts = hits
        .into_iter()
        .map(|hit| memory_fact_summary_from_row(hit.row))
        .collect::<Vec<_>>();

    Ok((
        format!("Memory facts matching \"{query}\""),
        serde_json::to_value(MemoryFactsEnvelope {
            query: Some(query.to_string()),
            topic_key: None,
            total_count: facts.len(),
            facts,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_search_entities(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing entity query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing entity query".to_string());
    }

    let hits = rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        Some(query),
        12,
    )
    .await
    .map_err(|e| format!("failed to search stored entities: {e}"))?;

    let entities = hits
        .into_iter()
        .map(|hit| memory_entity_summary_from_row(hit.row))
        .collect::<Vec<_>>();

    Ok((
        format!("Stored entities matching \"{query}\""),
        serde_json::to_value(MemoryEntitiesEnvelope {
            query: Some(query.to_string()),
            total_count: entities.len(),
            entities,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_list_recent_entities(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let hits = rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        None,
        12,
    )
    .await
    .map_err(|e| format!("failed to load recent entities: {e}"))?;

    let entities = hits
        .into_iter()
        .map(|hit| memory_entity_summary_from_row(hit.row))
        .collect::<Vec<_>>();

    Ok((
        "Recent stored entities".to_string(),
        serde_json::to_value(MemoryEntitiesEnvelope {
            query: None,
            total_count: entities.len(),
            entities,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_list_recent_changes(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let query = match &call.input {
        AssistantToolInput::SystemService { query } => {
            let query = query.trim();
            if query.is_empty() {
                None
            } else {
                Some(query.to_string())
            }
        }
        _ => None,
    };

    let fact_rows = if let Some(query) = query.as_deref() {
        rustfin_db::repo::ai_grounding::search_memory_items_for_user(
            &state.db,
            &context.user_id,
            None,
            Some(query),
            8,
        )
        .await
        .map_err(|e| format!("failed to load recent memory changes: {e}"))?
        .into_iter()
        .map(|hit| hit.row)
        .collect::<Vec<_>>()
    } else {
        rustfin_db::repo::ai_grounding::list_memory_items_for_user(&state.db, &context.user_id, 8)
            .await
            .map_err(|e| format!("failed to load recent memory changes: {e}"))?
    };

    let entity_hits = if let Some(query) = query.as_deref() {
        rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
            &state.db,
            &context.user_id,
            context.is_admin,
            None,
            Some(query),
            8,
        )
        .await
        .map_err(|e| format!("failed to load recent memory changes: {e}"))?
    } else {
        rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
            &state.db,
            &context.user_id,
            context.is_admin,
            None,
            None,
            8,
        )
        .await
        .map_err(|e| format!("failed to load recent memory changes: {e}"))?
    };

    let mut facts = fact_rows
        .into_iter()
        .map(memory_fact_summary_from_row)
        .collect::<Vec<_>>();
    facts.sort_by(|left, right| {
        right
            .updated_ts
            .cmp(&left.updated_ts)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.memory_key.cmp(&right.memory_key))
    });

    let mut entities = entity_hits
        .into_iter()
        .map(|hit| memory_entity_summary_from_row(hit.row))
        .collect::<Vec<_>>();
    entities.sort_by(|left, right| {
        right
            .updated_ts
            .cmp(&left.updated_ts)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.node_key.cmp(&right.node_key))
    });

    let label = match query.as_deref() {
        Some(query) => format!("Recent stored memory changes matching \"{query}\""),
        None => "Recent stored memory changes".to_string(),
    };

    Ok((
        label,
        serde_json::to_value(MemoryRecentChangesEnvelope {
            query,
            fact_count: facts.len(),
            entity_count: entities.len(),
            facts,
            entities,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_list_conflicting_facts(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let query = match &call.input {
        AssistantToolInput::SystemService { query } => {
            let query = query.trim();
            if query.is_empty() {
                None
            } else {
                Some(query.to_string())
            }
        }
        _ => None,
    };

    let fact_rows = if let Some(query) = query.as_deref() {
        rustfin_db::repo::ai_grounding::search_memory_items_for_user(
            &state.db,
            &context.user_id,
            None,
            Some(query),
            40,
        )
        .await
        .map_err(|e| format!("failed to load conflicting memory facts: {e}"))?
        .into_iter()
        .map(|hit| hit.row)
        .collect::<Vec<_>>()
    } else {
        rustfin_db::repo::ai_grounding::list_memory_items_for_user(&state.db, &context.user_id, 40)
            .await
            .map_err(|e| format!("failed to load conflicting memory facts: {e}"))?
    };

    let mut grouped = BTreeMap::<String, MemoryFactConflictBuilder>::new();
    for fact in fact_rows.iter() {
        let normalized_title = normalize_memory_fact_key(&fact.title);
        if normalized_title.is_empty() {
            continue;
        }
        let group_key = format!(
            "{}|{}",
            fact.topic_key
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
            normalized_title
        );
        let builder = grouped
            .entry(group_key)
            .or_insert_with(|| MemoryFactConflictBuilder::new(fact));
        builder.push_fact(fact.clone());
    }

    let mut conflicts = grouped
        .into_values()
        .filter_map(MemoryFactConflictBuilder::into_summary)
        .collect::<Vec<_>>();
    conflicts.sort_by(|left, right| {
        right
            .fact_count
            .cmp(&left.fact_count)
            .then_with(|| {
                right
                    .distinct_content_count
                    .cmp(&left.distinct_content_count)
            })
            .then_with(|| left.title.cmp(&right.title))
    });

    let label = match query.as_deref() {
        Some(query) => format!("Conflicting stored memory facts matching \"{query}\""),
        None => "Conflicting stored memory facts".to_string(),
    };

    Ok((
        label,
        serde_json::to_value(MemoryConflictingFactsEnvelope {
            query,
            total_count: fact_rows.len(),
            conflict_group_count: conflicts.len(),
            conflicts,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_get_entity_provenance(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing entity query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing entity query".to_string());
    }

    let (entity, matched_by) =
        resolve_unique_memory_entity_for_query(state, context, query).await?;
    let source_chunk = if let Some(source_chunk_id) = entity.source_chunk_id.as_deref() {
        let allowed_library_ids = if context.is_admin {
            None
        } else {
            Some(
                rustfin_db::repo::users::get_library_access(&state.db, &context.user_id)
                    .await
                    .map_err(|e| format!("failed to load library permissions: {e}"))?
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
        };

        rustfin_db::repo::ai_grounding::get_retrieval_chunk_for_user_by_key(
            &state.db,
            &context.user_id,
            context.is_admin,
            allowed_library_ids.as_deref(),
            source_chunk_id,
        )
        .await
        .map_err(|e| format!("failed to load entity provenance: {e}"))?
        .map(memory_provenance_chunk_summary_from_row)
    } else {
        None
    };

    let entity_summary = memory_entity_provenance_summary_from_row(entity);
    let label = format!("Stored entity provenance for \"{query}\"");

    Ok((
        label,
        serde_json::to_value(MemoryEntityProvenanceEnvelope {
            query: query.to_string(),
            matched_by,
            entity: Some(entity_summary),
            source_chunk,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

struct MemoryFactConflictBuilder {
    topic_key: Option<String>,
    title: String,
    facts: Vec<MemoryFactSummary>,
    distinct_contents: HashSet<String>,
}

impl MemoryFactConflictBuilder {
    fn new(fact: &rustfin_db::repo::ai_grounding::AiMemoryItemRow) -> Self {
        let summary = memory_fact_summary_from_row(fact.clone());
        let mut distinct_contents = HashSet::new();
        distinct_contents.insert(normalize_memory_fact_content_key(&summary.content));
        Self {
            topic_key: summary.topic_key.clone(),
            title: summary.title.clone(),
            facts: vec![summary],
            distinct_contents,
        }
    }

    fn push_fact(&mut self, fact: rustfin_db::repo::ai_grounding::AiMemoryItemRow) {
        let summary = memory_fact_summary_from_row(fact);
        self.distinct_contents
            .insert(normalize_memory_fact_content_key(&summary.content));
        self.facts.push(summary);
    }

    fn into_summary(mut self) -> Option<MemoryFactConflictSummary> {
        if self.facts.len() < 2 || self.distinct_contents.len() < 2 {
            return None;
        }
        self.facts.sort_by(|left, right| {
            right
                .updated_ts
                .cmp(&left.updated_ts)
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.memory_key.cmp(&right.memory_key))
        });
        Some(MemoryFactConflictSummary {
            topic_key: self.topic_key,
            title: self.title,
            fact_count: self.facts.len(),
            distinct_content_count: self.distinct_contents.len(),
            facts: self.facts,
        })
    }
}

fn normalize_memory_fact_key(value: &str) -> String {
    normalize_memory_text_key(value)
}

fn normalize_memory_fact_content_key(value: &str) -> String {
    normalize_memory_text_key(value)
}

fn normalize_memory_text_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

async fn resolve_unique_memory_entity_for_query(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<(rustfin_db::repo::ai_grounding::AiEntityNodeRow, String), String> {
    let exact_hits = rustfin_db::repo::ai_grounding::find_exact_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        query,
        8,
    )
    .await
    .map_err(|e| format!("failed to load exact entity matches: {e}"))?;

    if exact_hits.len() == 1 {
        let hit = exact_hits
            .into_iter()
            .next()
            .ok_or_else(|| format!("no stored entity matched \"{query}\""))?;
        return Ok((hit.row, "exact entity search".to_string()));
    }
    if exact_hits.len() > 1 {
        return Err(format!(
            "multiple stored entities matched \"{query}\"; which one do you mean?"
        ));
    }

    let fuzzy_hits = rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        Some(query),
        8,
    )
    .await
    .map_err(|e| format!("failed to search stored entities: {e}"))?;

    if fuzzy_hits.len() == 1 {
        let hit = fuzzy_hits
            .into_iter()
            .next()
            .ok_or_else(|| format!("no stored entity matched \"{query}\""))?;
        return Ok((hit.row, "entity search".to_string()));
    }
    if fuzzy_hits.is_empty() {
        return Err(format!("no stored entity matched \"{query}\""));
    }

    Err(format!(
        "multiple stored entities matched \"{query}\"; which one do you mean?"
    ))
}

async fn memory_get_entity_relations(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing entity query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing entity query".to_string());
    }

    let root_hits = rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        Some(query),
        1,
    )
    .await
    .map_err(|e| format!("failed to load stored entity relations: {e}"))?;

    let Some(root_hit) = root_hits.into_iter().next() else {
        return Ok((
            format!("Memory relations matching \"{query}\""),
            serde_json::to_value(MemoryEntityRelationsEnvelope {
                query: query.to_string(),
                matched_by: "entity search".to_string(),
                total_count: 0,
                root: None,
                relations: Vec::new(),
            })
            .unwrap_or_else(|_| json!({})),
        ));
    };

    let root = root_hit.row;
    let root_summary = memory_entity_summary_from_row(root.clone());
    let mut relations = Vec::new();
    relations.extend(
        memory_relation_rows_for_node(
            &state.db,
            &context.user_id,
            context.is_admin,
            &root.node_key,
            "outgoing",
            8,
        )
        .await?,
    );
    relations.extend(
        memory_relation_rows_for_node(
            &state.db,
            &context.user_id,
            context.is_admin,
            &root.node_key,
            "incoming",
            8,
        )
        .await?,
    );
    relations.sort_by(|left, right| {
        right
            .weight
            .partial_cmp(&left.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.created_ts.cmp(&left.created_ts))
    });

    Ok((
        format!("Stored entity relations for \"{query}\""),
        serde_json::to_value(MemoryEntityRelationsEnvelope {
            query: query.to_string(),
            matched_by: "entity search".to_string(),
            total_count: relations.len(),
            root: Some(root_summary),
            relations,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_find_exact_entity(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing entity query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing entity query".to_string());
    }

    let hits = rustfin_db::repo::ai_grounding::find_exact_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        None,
        query,
        12,
    )
    .await
    .map_err(|e| format!("failed to load exact stored entity matches: {e}"))?;

    let entities = hits
        .into_iter()
        .map(|hit| memory_entity_summary_from_row(hit.row))
        .collect::<Vec<_>>();

    Ok((
        format!("Exact stored entities matching \"{query}\""),
        serde_json::to_value(MemoryExactEntitiesEnvelope {
            query: Some(query.to_string()),
            matched_by: "exact entity search".to_string(),
            total_count: entities.len(),
            entities,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_get_entity_relation_path(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing entity path query".to_string());
    };
    let Some((source_query, target_query)) = split_memory_relation_path_query(query) else {
        return Err(
            "memory relation path queries require two entity names joined with ||".to_string(),
        );
    };

    let (root, root_match_by) =
        resolve_unique_memory_entity_for_path(state, context, &source_query).await?;
    let (target, target_match_by) =
        resolve_unique_memory_entity_for_path(state, context, &target_query).await?;

    let root_summary = memory_entity_summary_from_row(root.clone());
    let target_summary = memory_entity_summary_from_row(target.clone());
    let path =
        memory_relation_path_between_entities(state, context, &root_summary, &target_summary, 3)
            .await?;
    let path_found = root_summary.node_key == target_summary.node_key || !path.is_empty();

    Ok((
        format!("Stored entity relation path between \"{source_query}\" and \"{target_query}\""),
        serde_json::to_value(MemoryEntityRelationPathEnvelope {
            query: query.trim().to_string(),
            source_query,
            target_query,
            matched_by: format!("{root_match_by}; {target_match_by}"),
            total_hops: path.len(),
            path_found,
            root: Some(root_summary),
            target: Some(target_summary),
            path,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn memory_get_person_summary(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing person query".to_string());
    };
    let query = query.trim();
    if query.is_empty() {
        return Err("missing person query".to_string());
    }

    let (person, matched_by) =
        resolve_unique_memory_entity_for_query(state, context, query).await?;
    let summary = memory_entity_summary_from_row(person.clone());
    let mut relations = Vec::new();
    relations.extend(
        memory_relation_rows_for_node(
            &state.db,
            &context.user_id,
            context.is_admin,
            &person.node_key,
            "outgoing",
            8,
        )
        .await?,
    );
    relations.extend(
        memory_relation_rows_for_node(
            &state.db,
            &context.user_id,
            context.is_admin,
            &person.node_key,
            "incoming",
            8,
        )
        .await?,
    );
    relations.sort_by(|left, right| {
        right
            .weight
            .partial_cmp(&left.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.created_ts.cmp(&left.created_ts))
    });

    Ok((
        format!("Stored person summary for \"{}\"", summary.label),
        serde_json::to_value(MemoryPersonSummaryEnvelope {
            query: query.to_string(),
            matched_by,
            person: summary,
            relation_count: relations.len(),
            relations,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

fn split_memory_relation_path_query(query: &str) -> Option<(String, String)> {
    let trimmed = query.trim();
    let (left, right) = trimmed.split_once("||")?;
    let source = left.trim();
    let target = right.trim();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    Some((source.to_string(), target.to_string()))
}

async fn resolve_unique_memory_entity_for_path(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<(rustfin_db::repo::ai_grounding::AiEntityNodeRow, String), String> {
    resolve_unique_memory_entity_for_query(state, context, query).await
}

async fn memory_relation_path_between_entities(
    state: &AppState,
    context: &AssistantContext,
    root: &MemoryEntitySummary,
    target: &MemoryEntitySummary,
    max_hops: usize,
) -> Result<Vec<MemoryEntityRelationSummary>, String> {
    if root.node_key == target.node_key {
        return Ok(Vec::new());
    }

    let mut frontier = vec![root.node_key.clone()];
    let mut visited = HashSet::from([root.node_key.clone()]);
    let mut parents: HashMap<String, (String, MemoryEntityRelationSummary)> = HashMap::new();

    for _depth in 0..max_hops {
        let mut next_frontier = Vec::new();
        for current_node_key in frontier {
            let mut neighbors = memory_relation_rows_for_node(
                &state.db,
                &context.user_id,
                context.is_admin,
                &current_node_key,
                "outgoing",
                8,
            )
            .await?;
            neighbors.extend(
                memory_relation_rows_for_node(
                    &state.db,
                    &context.user_id,
                    context.is_admin,
                    &current_node_key,
                    "incoming",
                    8,
                )
                .await?,
            );
            neighbors.sort_by(|left, right| {
                right
                    .weight
                    .partial_cmp(&left.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| right.created_ts.cmp(&left.created_ts))
                    .then_with(|| left.entity.label.cmp(&right.entity.label))
            });

            for hop in neighbors {
                let neighbor_key = hop.entity.node_key.clone();
                if visited.contains(&neighbor_key) {
                    continue;
                }
                parents.insert(
                    neighbor_key.clone(),
                    (current_node_key.clone(), hop.clone()),
                );
                if neighbor_key == target.node_key {
                    return Ok(reconstruct_memory_relation_path(
                        &parents,
                        &root.node_key,
                        &target.node_key,
                    ));
                }
                visited.insert(neighbor_key.clone());
                next_frontier.push(neighbor_key);
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    Ok(Vec::new())
}

fn reconstruct_memory_relation_path(
    parents: &HashMap<String, (String, MemoryEntityRelationSummary)>,
    root_key: &str,
    target_key: &str,
) -> Vec<MemoryEntityRelationSummary> {
    let mut current_key = target_key.to_string();
    let mut path = Vec::new();

    while current_key != root_key {
        let Some((previous_key, hop)) = parents.get(&current_key) else {
            break;
        };
        path.push(hop.clone());
        current_key = previous_key.clone();
    }

    path.reverse();
    path
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
            library_id: lib.id.clone(),
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

async fn libraries_get_library_summary(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::LibrarySearch { query } = &call.input else {
        return Err("missing library summary query".to_string());
    };

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

    let Some((library, matched_by)) = libraries_find_library_detail(&libraries, query) else {
        return Err(format!("no accessible library matched \"{query}\""));
    };

    let item_count = rustfin_db::repo::libraries::count_library_items(&state.db, &library.id)
        .await
        .map_err(|e| format!("failed to count library items: {e}"))?;
    let paths = rustfin_db::repo::libraries::get_library_paths(&state.db, &library.id)
        .await
        .map_err(|e| format!("failed to load library paths: {e}"))?;
    let settings = rustfin_db::repo::libraries::get_library_settings(&state.db, &library.id)
        .await
        .map_err(|e| format!("failed to load library settings: {e}"))?
        .unwrap_or_else(|| default_library_settings_row(&library.id));

    Ok((
        format!("Library summary for \"{}\"", library.name),
        serde_json::to_value(LibraryDetailSummary {
            query: Some(query.clone()),
            matched_by,
            id: library.id.clone(),
            name: library.name.clone(),
            kind: library.kind.clone(),
            item_count,
            paths: paths
                .into_iter()
                .map(|path| LibraryPathSummary {
                    id: path.id,
                    path: path.path,
                    is_read_only: path.is_read_only,
                })
                .collect(),
            settings: library_settings_summary(&settings),
            created_ts: library.created_ts,
            updated_ts: library.updated_ts,
        })
        .unwrap_or_else(|_| json!({})),
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

async fn downloads_get_artifact_details(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    Ok((
        format!("Download artifact details for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactDetailSummary {
            query: Some(query),
            matched_by,
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn downloads_get_artifact_source(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    let source_url = artifact
        .external_url
        .clone()
        .or_else(|| artifact.download_path.clone())
        .or_else(|| artifact.setup_path.clone());

    Ok((
        format!("Download artifact source for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactSourceSummary {
            query: Some(query),
            matched_by,
            source_url: source_url.clone(),
            download_path: artifact.download_path.clone(),
            external_url: artifact.external_url.clone(),
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn downloads_get_release_notes(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    Ok((
        format!("Download artifact release notes for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactReleaseNotesSummary {
            query: Some(query),
            matched_by,
            release_notes: artifact.detail.clone(),
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn downloads_get_artifact_checksum(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    Ok((
        format!("Download artifact checksum for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactDetailSummary {
            query: Some(query),
            matched_by,
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn downloads_get_artifact_install_steps(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    Ok((
        format!("Download artifact install steps for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactDetailSummary {
            query: Some(query),
            matched_by,
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn downloads_get_artifact_compatibility(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (query, availability_filter) = downloads_filter_for_call(call);
    let query = query.ok_or_else(|| "missing download artifact query".to_string())?;
    let catalog = crate::downloads::build_download_catalog(state)
        .await
        .map_err(|error| format!("failed to build download catalog: {}", error.0))?;

    let Some((artifact, matched_by)) =
        downloads_find_artifact_detail(&catalog.items, &query, availability_filter.as_deref())
    else {
        return Err(format!("no download artifact matched \"{query}\""));
    };

    Ok((
        format!("Download artifact compatibility for \"{}\"", artifact.title),
        serde_json::to_value(DownloadArtifactDetailSummary {
            query: Some(query),
            matched_by,
            artifact: artifact.clone(),
        })
        .unwrap_or_else(|_| json!({})),
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

async fn weather_resolve_location_alias(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::Weather { location, .. } = &call.input else {
        return Err("missing public weather location".to_string());
    };
    let alias = super::weather::resolve_public_location_timezone(location).await?;
    Ok((
        format!(
            "Resolved public location alias for {}",
            alias.resolved_location
        ),
        serde_json::to_value(alias).unwrap_or_else(|_| json!({})),
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

async fn weather_get_forecast_for_date(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    weather_get_forecast(state, context, call).await
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

async fn weather_get_hourly_window(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::WeatherHistory {
        location,
        start_date,
        end_date,
        label,
    } = &call.input
    else {
        return Err("missing public weather hourly input".to_string());
    };
    let start_date = NaiveDate::parse_from_str(start_date, "%F")
        .map_err(|error| format!("invalid public weather hourly start date: {error}"))?;
    let end_date = NaiveDate::parse_from_str(end_date, "%F")
        .map_err(|error| format!("invalid public weather hourly end date: {error}"))?;
    if start_date != end_date {
        return Err("public weather hourly windows must cover exactly one day".to_string());
    }
    let today = assistant_local_today();
    if start_date < today {
        return Err(
            "public weather hourly windows are only available for today and future dates"
                .to_string(),
        );
    }
    let hourly = super::weather::fetch_public_weather_hourly_window(location, start_date).await?;
    Ok((
        format!(
            "Hourly weather window for {} on {}",
            hourly.resolved_location, label
        ),
        serde_json::to_value(hourly).unwrap_or_else(|_| json!({})),
    ))
}

async fn weather_get_recent_history_for_date(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    weather_get_history(state, context, call).await
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
    let AssistantToolInput::WebSearch { query, category } = &call.input else {
        return Err("missing public web search query".to_string());
    };
    let results = if let Some(category_slug) = category.as_deref() {
        let category = CuratedWebCategory::from_slug(category_slug)
            .ok_or_else(|| format!("unknown curated web category: {category_slug}"))?;
        search_curated_web(category, query).await?
    } else {
        search_public_web(query, Some(5)).await?
    };
    let label = match category.as_deref() {
        Some(category_slug) => {
            let category_label = curated_web_category_label(category_slug).unwrap_or("Curated");
            format!(
                "{} {} results for \"{}\"",
                results.len(),
                category_label,
                query
            )
        }
        None => format!("{} public web results for \"{}\"", results.len(), query),
    };
    Ok((
        label,
        serde_json::to_value(PublicWebSearchSummary {
            query: query.clone(),
            category: category.clone(),
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
    let AssistantToolInput::WebFetch { url, category } = &call.input else {
        return Err("missing public web URL".to_string());
    };
    let summary = if let Some(category_slug) = category.as_deref() {
        let category = CuratedWebCategory::from_slug(category_slug)
            .ok_or_else(|| format!("unknown curated web category: {category_slug}"))?;
        let mut summary = fetch_curated_web_page_summary(category, url).await?;
        summary.category = Some(category.slug().to_string());
        summary
    } else if let Some(category_slug) = curated_web_category_for_url(url) {
        let category = CuratedWebCategory::from_slug(category_slug)
            .ok_or_else(|| format!("unknown curated web category: {category_slug}"))?;
        let mut summary = fetch_curated_web_page_summary(category, url).await?;
        summary.category = Some(category.slug().to_string());
        summary
    } else {
        fetch_public_page_summary(url).await?
    };
    let label = match summary
        .category
        .as_deref()
        .and_then(curated_web_category_label)
    {
        Some(category_label) => match summary.page_title.as_deref() {
            Some(title) => format!("Fetched {category_label} page \"{title}\""),
            None => format!("Fetched {category_label} page from {}", summary.source_host),
        },
        None => match summary.page_title.as_deref() {
            Some(title) => format!("Fetched public page \"{title}\""),
            None => format!("Fetched public page from {}", summary.source_host),
        },
    };
    Ok((
        label,
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn web_list_curated_sources(
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
    if !matches!(call.input, AssistantToolInput::None) {
        return Err("web list curated sources does not accept arguments".to_string());
    }
    Ok((
        "Curated public web source catalog".to_string(),
        serde_json::to_value(curated_web_catalog_summary()).unwrap_or_else(|_| json!({})),
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
            library_id: item.library_id.clone(),
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

struct ResolvedLibraryItem {
    query: String,
    matched_by: String,
    item: rustfin_db::repo::items::ItemRow,
    library_name: Option<String>,
}

async fn resolve_library_item_by_query(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<ResolvedLibraryItem, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("missing library item query".to_string());
    }

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

    let matched_by = if best_match.title.eq_ignore_ascii_case(query)
        || best_match
            .sort_title
            .as_deref()
            .is_some_and(|sort_title| sort_title.eq_ignore_ascii_case(query))
    {
        "exact_title".to_string()
    } else {
        "title_search".to_string()
    };

    Ok(ResolvedLibraryItem {
        query: query.to_string(),
        matched_by,
        item,
        library_name,
    })
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
        library_id: item.library_id.clone(),
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

async fn library_get_item_media_details(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::LibrarySearch { query } = &call.input else {
        return Err("missing library item query".to_string());
    };

    let resolved = resolve_library_item_by_query(state, context, query).await?;
    let ResolvedLibraryItem {
        query,
        matched_by,
        item,
        library_name,
    } = resolved;

    let artwork = rustfin_db::repo::items::get_item_artwork(&state.db, &item.id)
        .await
        .map_err(|e| format!("failed to load library item artwork: {e}"))?;
    let media_path = rustfin_db::repo::items::get_item_media_path(&state.db, &item.id)
        .await
        .map_err(|e| format!("failed to load library item media path: {e}"))?;
    let first_descendant_media_path =
        rustfin_db::repo::items::get_first_descendant_media_path(&state.db, &item.id)
            .await
            .map_err(|e| format!("failed to load library item descendant media path: {e}"))?;
    let (poster_url, backdrop_url, logo_url, thumb_url) =
        artwork.unwrap_or_else(|| (None, None, None, None));
    let resolved_media_path = media_path
        .clone()
        .or_else(|| first_descendant_media_path.clone());
    let source_paths = collect_library_item_source_paths(
        media_path.clone(),
        resolved_media_path.clone(),
        first_descendant_media_path.clone(),
    );

    let summary = LibraryItemMediaDetailSummary {
        query,
        matched_by,
        library_id: item.library_id.clone(),
        id: item.id,
        title: item.title,
        kind: item.kind,
        year: item.year,
        library_name,
        overview: item.overview,
        duration_ms: item.duration_ms,
        parent_id: item.parent_id,
        media_path,
        resolved_media_path,
        first_descendant_media_path,
        source_paths,
        poster_url,
        backdrop_url,
        logo_url,
        thumb_url,
        created_ts: item.created_ts,
        updated_ts: item.updated_ts,
    };

    Ok((
        format!("Library media details for \"{}\"", summary.title),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn library_get_item_source_paths(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::LibrarySearch { query } = &call.input else {
        return Err("missing library item query".to_string());
    };

    let resolved = resolve_library_item_by_query(state, context, query).await?;
    let ResolvedLibraryItem {
        query,
        matched_by,
        item,
        library_name,
    } = resolved;

    let artwork = rustfin_db::repo::items::get_item_artwork(&state.db, &item.id)
        .await
        .map_err(|e| format!("failed to load library item artwork: {e}"))?;
    let media_path = rustfin_db::repo::items::get_item_media_path(&state.db, &item.id)
        .await
        .map_err(|e| format!("failed to load library item media path: {e}"))?;
    let first_descendant_media_path =
        rustfin_db::repo::items::get_first_descendant_media_path(&state.db, &item.id)
            .await
            .map_err(|e| format!("failed to load library item descendant media path: {e}"))?;
    let (poster_url, backdrop_url, logo_url, thumb_url) =
        artwork.unwrap_or_else(|| (None, None, None, None));
    let resolved_media_path = media_path
        .clone()
        .or_else(|| first_descendant_media_path.clone());
    let source_paths = collect_library_item_source_paths(
        media_path.clone(),
        resolved_media_path.clone(),
        first_descendant_media_path.clone(),
    );

    let summary = LibraryItemMediaDetailSummary {
        query,
        matched_by,
        library_id: item.library_id.clone(),
        id: item.id,
        title: item.title,
        kind: item.kind,
        year: item.year,
        library_name,
        overview: item.overview,
        duration_ms: item.duration_ms,
        parent_id: item.parent_id,
        media_path,
        resolved_media_path,
        first_descendant_media_path,
        source_paths,
        poster_url,
        backdrop_url,
        logo_url,
        thumb_url,
        created_ts: item.created_ts,
        updated_ts: item.updated_ts,
    };

    Ok((
        format!("Library item source paths for \"{}\"", summary.title),
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

fn network_find_interface_detail(
    snapshot: &crate::network_diagnostics::NetworkTopologySnapshot,
    query: &str,
) -> Option<(crate::network_diagnostics::NetworkNodeSummary, String)> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lowered = trimmed.to_ascii_lowercase();
    let preferred_interface = snapshot.access.preferred_local_interface.as_deref();
    let preferred_node = preferred_interface.and_then(|name| {
        snapshot
            .nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    });

    if let Some(node) = snapshot
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case(trimmed))
    {
        return Some((node.clone(), "exact_name".to_string()));
    }
    if let Some(node) = snapshot.nodes.iter().find(|node| {
        node.addresses
            .iter()
            .any(|address| address.address.eq_ignore_ascii_case(trimmed))
    }) {
        return Some((node.clone(), "exact_address".to_string()));
    }
    if let Some(node) = snapshot.nodes.iter().find(|node| {
        node.name.to_ascii_lowercase().contains(&lowered)
            || node
                .addresses
                .iter()
                .any(|address| address.address.to_ascii_lowercase().contains(&lowered))
    }) {
        return Some((node.clone(), "contains".to_string()));
    }

    if let Some(host_label) = snapshot.host_label.as_deref() {
        if host_label.eq_ignore_ascii_case(trimmed) {
            if let Some(node) = preferred_node {
                return Some((node.clone(), "host_label".to_string()));
            }
        }
    }

    None
}

fn network_find_interface_by_ip(
    snapshot: &crate::network_diagnostics::NetworkTopologySnapshot,
    query: &str,
) -> Option<(crate::network_diagnostics::NetworkNodeSummary, String)> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.parse::<std::net::IpAddr>().is_err() {
        return None;
    }

    snapshot
        .nodes
        .iter()
        .find(|node| {
            node.addresses
                .iter()
                .any(|address| address.address.eq_ignore_ascii_case(trimmed))
        })
        .cloned()
        .map(|node| (node, "exact_address".to_string()))
}

async fn network_get_interface_details(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::NetworkInterface { query } = &call.input else {
        return Err("missing network interface query".to_string());
    };

    let snapshot =
        crate::network_diagnostics::collect_network_topology_snapshot(state, context.is_admin)
            .await;
    if !snapshot.available {
        return Err(snapshot
            .reason
            .unwrap_or_else(|| "network diagnostics are unavailable".to_string()));
    }

    let Some((interface, matched_by)) = network_find_interface_detail(&snapshot, query) else {
        return Err(format!("no network interface matched \"{query}\""));
    };

    let title = format!("Network interface details for \"{}\"", interface.name);
    Ok((
        title,
        serde_json::to_value(NetworkInterfaceDetailSummary {
            query: query.clone(),
            matched_by,
            host_label: snapshot.host_label,
            remote_access_enabled: snapshot.remote_access_enabled,
            access: snapshot.access,
            interface,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn network_get_interface_by_ip(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::NetworkInterface { query } = &call.input else {
        return Err("missing network interface IP query".to_string());
    };

    let trimmed = query.trim();
    if trimmed.parse::<std::net::IpAddr>().is_err() {
        return Err(format!(
            "network interface by IP requires a valid IP address, got \"{query}\""
        ));
    }

    let snapshot =
        crate::network_diagnostics::collect_network_topology_snapshot(state, context.is_admin)
            .await;
    if !snapshot.available {
        return Err(snapshot
            .reason
            .unwrap_or_else(|| "network diagnostics are unavailable".to_string()));
    }

    let Some((interface, matched_by)) = network_find_interface_by_ip(&snapshot, trimmed) else {
        return Err(format!("no network interface matched IP \"{trimmed}\""));
    };

    let title = format!("Network interface for IP \"{}\"", trimmed);
    Ok((
        title,
        serde_json::to_value(NetworkInterfaceDetailSummary {
            query: query.clone(),
            matched_by,
            host_label: snapshot.host_label,
            remote_access_enabled: snapshot.remote_access_enabled,
            access: snapshot.access,
            interface,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn network_get_default_route(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::NetworkDefaultRoute { query } = &call.input else {
        return Err("missing default route query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    #[cfg(target_os = "linux")]
    {
        let routes = collect_linux_default_routes().await?;
        let mut routes = if let Some(query) = _query.as_deref() {
            let filtered = routes
                .into_iter()
                .filter(|route| default_route_matches_query(route, query))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(format!("no default route matched \"{query}\""));
            }
            filtered
        } else {
            routes
        };

        routes.sort_by(|left, right| {
            left.metric
                .unwrap_or(u32::MAX)
                .cmp(&right.metric.unwrap_or(u32::MAX))
                .then_with(|| left.interface.cmp(&right.interface))
                .then_with(|| left.gateway.cmp(&right.gateway))
                .then_with(|| left.route.cmp(&right.route))
        });

        let matched_by = match _query.as_deref() {
            Some(query) if routes.len() == 1 && routes[0].route.eq_ignore_ascii_case(query) => {
                "exact_route".to_string()
            }
            Some(_) => "query_contains".to_string(),
            None => "default_route".to_string(),
        };

        return Ok((
            match _query.as_deref() {
                Some(query) => format!("Default route matching \"{query}\""),
                None => "Default route".to_string(),
            },
            serde_json::to_value(NetworkDefaultRouteEnvelope {
                query: _query,
                matched_by,
                total_count: routes.len(),
                routes,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Default route details are only available on Linux hosts.".to_string())
    }
}

async fn network_get_hostname_aliases(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::NetworkHostnameAliases { query } = &call.input else {
        return Err("missing hostname alias query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    #[cfg(target_os = "linux")]
    {
        let mut aliases = collect_linux_hostname_aliases().await?;
        let host_label = detect_linux_host_label();
        let canonical_hostname = linux_hostname_short_name().await.ok().flatten();
        let fqdn = linux_hostname_fqdn().await.ok().flatten();

        if let Some(query) = _query.as_deref() {
            let query_matches_primary = canonical_hostname
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(query))
                || host_label
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(query))
                || fqdn
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(query));
            if !query_matches_primary {
                aliases.retain(|alias| hostname_alias_matches_query(alias, query));
            }
            if aliases.is_empty() && !query_matches_primary {
                return Err(format!("no hostname aliases matched \"{query}\""));
            }
        }

        aliases.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.source.cmp(&right.source))
        });
        aliases.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));

        let matched_by = match _query.as_deref() {
            Some(query)
                if canonical_hostname
                    .as_deref()
                    .is_some_and(|canonical| canonical.eq_ignore_ascii_case(query))
                    || host_label
                        .as_deref()
                        .is_some_and(|label| label.eq_ignore_ascii_case(query))
                    || fqdn
                        .as_deref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(query)) =>
            {
                "canonical_hostname".to_string()
            }
            Some(query)
                if aliases
                    .iter()
                    .any(|alias| alias.name.eq_ignore_ascii_case(query)) =>
            {
                "alias_exact".to_string()
            }
            Some(_) => "alias_contains".to_string(),
            None => "all_aliases".to_string(),
        };

        return Ok((
            match _query.as_deref() {
                Some(query) => format!("Hostname aliases matching \"{query}\""),
                None => "Hostname aliases".to_string(),
            },
            serde_json::to_value(NetworkHostnameAliasesEnvelope {
                query: _query,
                matched_by,
                host_label,
                canonical_hostname,
                fqdn,
                total_count: aliases.len(),
                aliases,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Hostname aliases are only available on Linux hosts.".to_string())
    }
}

async fn network_get_dns_servers(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::NetworkDnsServers { query } = &call.input else {
        return Err("missing DNS server query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    #[cfg(target_os = "linux")]
    {
        let mut dns_servers = collect_linux_dns_servers().await?;
        if let Some(query) = _query.as_deref() {
            let filtered = dns_servers
                .into_iter()
                .filter(|server| dns_server_matches_query(server, query))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(format!("no DNS servers matched \"{query}\""));
            }
            dns_servers = filtered;
        }

        dns_servers.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then_with(|| left.interface.cmp(&right.interface))
                .then_with(|| left.server.cmp(&right.server))
                .then_with(|| left.source.cmp(&right.source))
        });
        dns_servers.dedup_by(|left, right| {
            left.scope.eq_ignore_ascii_case(&right.scope)
                && left
                    .interface
                    .as_deref()
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(right.interface.as_deref().unwrap_or_default())
                && left.server.eq_ignore_ascii_case(&right.server)
        });

        let matched_by = match _query.as_deref() {
            Some(query)
                if dns_servers
                    .iter()
                    .any(|server| server.server.eq_ignore_ascii_case(query)) =>
            {
                "server_exact".to_string()
            }
            Some(_) => "query_contains".to_string(),
            None => "dns_resolvers".to_string(),
        };

        return Ok((
            match _query.as_deref() {
                Some(query) => format!("DNS servers matching \"{query}\""),
                None => "DNS servers".to_string(),
            },
            serde_json::to_value(NetworkDnsServersEnvelope {
                query: _query,
                matched_by,
                total_count: dns_servers.len(),
                dns_servers,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("DNS server details are only available on Linux hosts.".to_string())
    }
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

async fn calendar_get_next_event_timing(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let today = assistant_local_today();
    let next_event = rustfin_db::repo::calendar::find_next_visible_event(
        &state.db,
        &context.user_id,
        context.is_admin,
        today,
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
    let days_until = next_event
        .as_ref()
        .and_then(|event| chrono::NaiveDate::parse_from_str(&event.next_occurs_on, "%F").ok())
        .map(|date| (date - today).num_days());

    Ok((
        "Next visible calendar event timing".to_string(),
        json!({
            "today": today.format("%F").to_string(),
            "days_until": days_until,
            "next_event": next_event,
        }),
    ))
}

async fn calendar_list_date_conflicts(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 7);
    let (from_date, to_date) = validate_calendar_analysis_window(&from, &to)?;
    let occurrences = rustfin_db::repo::calendar::list_visible_event_occurrences(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from_date.format("%F").to_string(),
        &to_date.format("%F").to_string(),
    )
    .await
    .map_err(|e| format!("failed to load visible calendar conflicts: {e}"))?;

    let total_event_count = occurrences.len();
    let mut grouped: BTreeMap<chrono::NaiveDate, Vec<CalendarEventOccurrenceSummary>> =
        BTreeMap::new();
    for occurrence in occurrences {
        grouped
            .entry(occurrence.occurs_on)
            .or_default()
            .push(calendar_event_occurrence_summary(&occurrence));
    }

    let conflict_days: Vec<_> = grouped
        .into_iter()
        .filter(|(_, events)| events.len() > 1)
        .take(12)
        .map(|(date, events)| CalendarConflictDaySummary {
            date: date.format("%F").to_string(),
            event_count: events.len(),
            events,
        })
        .collect();

    Ok((
        format!("Visible calendar conflicts for {label}"),
        json!({
            "window": {
                "from": from_date.format("%F").to_string(),
                "to": to_date.format("%F").to_string(),
                "label": label,
            },
            "total_event_count": total_event_count,
            "conflict_day_count": conflict_days.len(),
            "conflict_days": conflict_days,
        }),
    ))
}

async fn calendar_list_free_days(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 7);
    let (from_date, to_date) = validate_calendar_analysis_window(&from, &to)?;
    let occurrences = rustfin_db::repo::calendar::list_visible_event_occurrences(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from_date.format("%F").to_string(),
        &to_date.format("%F").to_string(),
    )
    .await
    .map_err(|e| format!("failed to load visible calendar free days: {e}"))?;

    let occupied_days: BTreeMap<chrono::NaiveDate, usize> =
        occurrences
            .into_iter()
            .fold(BTreeMap::new(), |mut acc, occurrence| {
                *acc.entry(occurrence.occurs_on).or_insert(0) += 1;
                acc
            });

    let mut free_days = Vec::new();
    let mut date = from_date;
    while date <= to_date {
        if !occupied_days.contains_key(&date) {
            free_days.push(CalendarFreeDaySummary {
                date: date.format("%F").to_string(),
            });
            if free_days.len() >= 14 {
                break;
            }
        }
        date += Duration::days(1);
    }

    Ok((
        format!("Visible calendar free days for {label}"),
        json!({
            "window": {
                "from": from_date.format("%F").to_string(),
                "to": to_date.format("%F").to_string(),
                "label": label,
            },
            "occupied_day_count": occupied_days.len(),
            "free_day_count": free_days.len(),
            "free_days": free_days,
        }),
    ))
}

async fn calendar_get_next_free_day(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 30);
    let (from_date, to_date) = validate_calendar_analysis_window(&from, &to)?;
    let occurrences = rustfin_db::repo::calendar::list_visible_event_occurrences(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from_date.format("%F").to_string(),
        &to_date.format("%F").to_string(),
    )
    .await
    .map_err(|e| format!("failed to load visible calendar free day search: {e}"))?;

    let occupied_days: BTreeMap<chrono::NaiveDate, usize> =
        occurrences
            .into_iter()
            .fold(BTreeMap::new(), |mut acc, occurrence| {
                *acc.entry(occurrence.occurs_on).or_insert(0) += 1;
                acc
            });

    let mut next_free_day = None;
    let mut searched_day_count = 0usize;
    let mut date = from_date;
    while date <= to_date {
        searched_day_count += 1;
        if !occupied_days.contains_key(&date) {
            next_free_day = Some(CalendarFreeDaySummary {
                date: date.format("%F").to_string(),
            });
            break;
        }
        date += Duration::days(1);
    }

    Ok((
        format!("Next visible free calendar day for {label}"),
        json!({
            "window": {
                "from": from_date.format("%F").to_string(),
                "to": to_date.format("%F").to_string(),
                "label": label,
            },
            "occupied_day_count": occupied_days.len(),
            "searched_day_count": searched_day_count,
            "next_free_day": next_free_day,
        }),
    ))
}

async fn calendar_count_events(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 7);
    let (from_date, to_date) = validate_calendar_analysis_window(&from, &to)?;
    let occurrences = rustfin_db::repo::calendar::list_visible_event_occurrences(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from_date.format("%F").to_string(),
        &to_date.format("%F").to_string(),
    )
    .await
    .map_err(|e| format!("failed to load visible calendar counts: {e}"))?;

    let mut grouped: BTreeMap<chrono::NaiveDate, usize> = BTreeMap::new();
    for occurrence in occurrences {
        *grouped.entry(occurrence.occurs_on).or_insert(0) += 1;
    }

    let total_event_count: usize = grouped.values().sum();
    let day_counts: Vec<_> = grouped
        .iter()
        .map(|(date, event_count)| CalendarDayCountSummary {
            date: date.format("%F").to_string(),
            event_count: *event_count,
        })
        .collect();

    let busiest_day_count = grouped.values().copied().max();
    let busy_day_count = grouped.len();

    Ok((
        format!("Visible calendar event counts for {label}"),
        json!({
            "window": {
                "from": from_date.format("%F").to_string(),
                "to": to_date.format("%F").to_string(),
                "label": label,
            },
            "total_event_count": total_event_count,
            "busy_day_count": busy_day_count,
            "busiest_day_count": busiest_day_count,
            "day_counts": day_counts,
        }),
    ))
}

async fn calendar_list_busy_days(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 7);
    let (from_date, to_date) = validate_calendar_analysis_window(&from, &to)?;
    let occurrences = rustfin_db::repo::calendar::list_visible_event_occurrences(
        &state.db,
        &context.user_id,
        context.is_admin,
        &from_date.format("%F").to_string(),
        &to_date.format("%F").to_string(),
    )
    .await
    .map_err(|e| format!("failed to load visible calendar busy days: {e}"))?;

    let total_event_count = occurrences.len();
    let mut grouped: BTreeMap<chrono::NaiveDate, Vec<CalendarEventOccurrenceSummary>> =
        BTreeMap::new();
    for occurrence in occurrences {
        grouped
            .entry(occurrence.occurs_on)
            .or_default()
            .push(calendar_event_occurrence_summary(&occurrence));
    }

    let busy_days: Vec<_> = grouped
        .into_iter()
        .filter(|(_, events)| !events.is_empty())
        .take(12)
        .map(|(date, events)| CalendarBusyDaySummary {
            date: date.format("%F").to_string(),
            event_count: events.len(),
            events,
        })
        .collect();

    Ok((
        format!("Visible calendar busy days for {label}"),
        json!({
            "window": {
                "from": from_date.format("%F").to_string(),
                "to": to_date.format("%F").to_string(),
                "label": label,
            },
            "total_event_count": total_event_count,
            "busy_day_count": busy_days.len(),
            "busy_days": busy_days,
        }),
    ))
}

async fn calendar_list_overlapping_events(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    calendar_list_date_conflicts(state, context, call).await
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

async fn calendar_get_event_by_exact_date_and_title(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    calendar_get_event_details(state, context, call).await
}

async fn calendar_get_event_series_summary(
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

    let mut matches = visible_events
        .into_iter()
        .filter(|event| calendar_event_matches_query(event, &query))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(format!(
            "no visible calendar event series matched \"{query}\" in {label}"
        ));
    }

    matches.sort_by(|left, right| {
        left.event_date
            .cmp(&right.event_date)
            .then_with(|| left.created_ts.cmp(&right.created_ts))
    });

    let first = matches.first().cloned().unwrap();
    let total_count = matches.len();
    let first_event_date = matches.first().map(|event| event.event_date.clone());
    let last_event_date = matches.last().map(|event| event.event_date.clone());
    let occurrences = matches
        .into_iter()
        .take(12)
        .map(|event| CalendarEventSummary {
            title: event.title,
            event_date: event.event_date,
            scope: event.scope,
            event_type: event.event_type,
            owner_username: event.owner_username,
        })
        .collect::<Vec<_>>();

    Ok((
        format!("Calendar event series summary for \"{}\"", first.title),
        serde_json::to_value(CalendarEventSeriesSummary {
            query,
            matched_by: "title_contains".to_string(),
            title: first.title,
            event_type: first.event_type,
            recurrence: first.recurrence,
            scope: first.scope,
            owner_username: first.owner_username,
            total_count,
            first_event_date,
            last_event_date,
            occurrences,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn calendar_get_next_free_slot(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    calendar_get_next_free_day(state, context, call).await
}

async fn calendar_list_busy_slots(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    calendar_list_busy_days(state, context, call).await
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
    _context: &AssistantContext,
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

    let deleted = rustfin_db::repo::calendar::delete_event(&state.db, event_id)
        .await
        .map_err(|e| format!("failed to delete the calendar event: {e}"))?;
    if !deleted {
        return Err("that calendar event is no longer available to delete".to_string());
    }

    Ok((
        format!("Deleted calendar event \"{}\"", title),
        json!({
            "verified": true,
            "event": {
                "id": event_id,
                "title": title,
                "event_date": event_date,
                "scope": scope,
                "event_type": event_type,
                "recurrence": recurrence,
                "deleted": true,
            },
        }),
    ))
}

async fn document_create_download(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::DocumentCreateDownload {
        file_name, format, ..
    } = &call.input
    else {
        return Err("missing document generation payload".to_string());
    };

    let _ = format;
    Err(format!(
        "Rustyfin AI could not create `{file_name}` because document artifact generation is not yet wired into this tool path for confirmed actions."
    ))
}

async fn conversations_archive_selection(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::ConversationArchive {
        conversation_ids,
        titles,
        selection_label,
        archived,
    } = &call.input
    else {
        return Err("missing conversation archive payload".to_string());
    };

    if conversation_ids.is_empty() {
        return Err("no AI conversations were selected for archiving".to_string());
    }

    let mut verified = Vec::with_capacity(conversation_ids.len());
    for conversation_id in conversation_ids {
        let Some(row) = rustfin_db::repo::ai_conversations::get_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
        )
        .await
        .map_err(|e| format!("failed to load AI conversation {conversation_id}: {e}"))?
        else {
            return Err(
                "one of those AI conversations is no longer available. Ask me to prepare the action again."
                    .to_string(),
            );
        };

        let updated = rustfin_db::repo::ai_conversations::update_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
            None,
            Some(*archived),
            None,
            None,
        )
        .await
        .map_err(|e| format!("failed to update AI conversation \"{}\": {e}", row.title))?
        .ok_or_else(|| format!("AI conversation \"{}\" is no longer available.", row.title))?;

        if updated.archived != *archived {
            return Err(format!(
                "Rustyfin AI changed \"{}\", but could not verify the archive state.",
                updated.title
            ));
        }
        verified.push(json!({
            "id": updated.id,
            "title": updated.title,
            "archived": updated.archived,
            "group_name": updated.group_name,
        }));
    }

    let action_label = if *archived { "Archived" } else { "Restored" };
    let fallback_titles = titles
        .iter()
        .map(|title| json!({ "title": title }))
        .collect::<Vec<_>>();

    Ok((
        format!("{action_label} {} AI conversations", conversation_ids.len()),
        json!({
            "verified": true,
            "operation": if *archived { "archive" } else { "restore" },
            "selection_label": selection_label,
            "conversation_count": conversation_ids.len(),
            "conversations": if verified.is_empty() { fallback_titles } else { verified },
        }),
    ))
}

async fn conversations_delete_selection(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::ConversationDelete {
        conversation_ids,
        titles,
        selection_label,
    } = &call.input
    else {
        return Err("missing conversation delete payload".to_string());
    };

    if conversation_ids.is_empty() {
        return Err("no AI conversations were selected for deletion".to_string());
    }

    let mut deleted_conversations = Vec::with_capacity(conversation_ids.len());
    for (index, conversation_id) in conversation_ids.iter().enumerate() {
        let Some(row) = rustfin_db::repo::ai_conversations::get_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
        )
        .await
        .map_err(|e| format!("failed to load AI conversation {conversation_id}: {e}"))?
        else {
            return Err(
                "one of those AI conversations is no longer available. Ask me to prepare the action again."
                    .to_string(),
            );
        };

        let deleted = rustfin_db::repo::ai_conversations::delete_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
        )
        .await
        .map_err(|e| format!("failed to delete AI conversation \"{}\": {e}", row.title))?;
        if !deleted {
            return Err(format!(
                "Rustyfin AI could not delete \"{}\" because it was no longer available.",
                row.title
            ));
        }

        let exists_after_delete = rustfin_db::repo::ai_conversations::get_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
        )
        .await
        .map_err(|e| {
            format!(
                "failed to verify deleted AI conversation \"{}\": {e}",
                row.title
            )
        })?
        .is_some();
        if exists_after_delete {
            return Err(format!(
                "Rustyfin AI deleted \"{}\", but could not verify the removal.",
                row.title
            ));
        }

        deleted_conversations.push(json!({
            "id": conversation_id,
            "title": titles.get(index).cloned().unwrap_or(row.title),
            "deleted": true,
        }));
    }

    Ok((
        format!("Deleted {} AI conversations", conversation_ids.len()),
        json!({
            "verified": true,
            "operation": "delete",
            "selection_label": selection_label,
            "conversation_count": conversation_ids.len(),
            "conversations": deleted_conversations,
        }),
    ))
}

async fn conversations_move_to_group_selection(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::ConversationMoveToGroup {
        conversation_ids,
        titles,
        selection_label,
        group_name,
    } = &call.input
    else {
        return Err("missing conversation move-to-group payload".to_string());
    };

    if conversation_ids.is_empty() {
        return Err("no AI conversations were selected to move into a group".to_string());
    }

    let normalized_group = group_name.trim();
    if normalized_group.is_empty() {
        return Err("missing destination AI conversation group name".to_string());
    }

    let mut verified = Vec::with_capacity(conversation_ids.len());
    for (index, conversation_id) in conversation_ids.iter().enumerate() {
        let Some(row) = rustfin_db::repo::ai_conversations::get_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
        )
        .await
        .map_err(|e| format!("failed to load AI conversation {conversation_id}: {e}"))?
        else {
            return Err(
                "one of those AI conversations is no longer available. Ask me to prepare the action again."
                    .to_string(),
            );
        };

        let updated = rustfin_db::repo::ai_conversations::update_conversation_for_user(
            &state.db,
            conversation_id,
            &context.user_id,
            None,
            None,
            Some(Some(normalized_group)),
            None,
        )
        .await
        .map_err(|e| {
            format!(
                "failed to move AI conversation \"{}\" into group \"{normalized_group}\": {e}",
                row.title
            )
        })?
        .ok_or_else(|| format!("AI conversation \"{}\" is no longer available.", row.title))?;

        if updated.group_name.as_deref() != Some(normalized_group) {
            return Err(format!(
                "Rustyfin AI changed \"{}\", but could not verify the group update.",
                updated.title
            ));
        }

        verified.push(json!({
            "id": updated.id,
            "title": titles.get(index).cloned().unwrap_or(updated.title),
            "group_name": updated.group_name,
            "archived": updated.archived,
        }));
    }

    Ok((
        format!(
            "Moved {} AI conversations into group \"{normalized_group}\"",
            conversation_ids.len()
        ),
        json!({
            "verified": true,
            "operation": "move_to_group",
            "selection_label": selection_label,
            "group_name": normalized_group,
            "conversation_count": conversation_ids.len(),
            "conversations": verified,
        }),
    ))
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

fn validate_calendar_analysis_window(
    from: &str,
    to: &str,
) -> Result<(chrono::NaiveDate, chrono::NaiveDate), String> {
    let from_date = validate_calendar_date(from)?;
    let to_date = validate_calendar_date(to)?;
    if to_date < from_date {
        return Err(
            "calendar analysis window end date must not be before the start date".to_string(),
        );
    }
    if (to_date - from_date).num_days() > 31 {
        return Err("calendar analysis windows are limited to 31 days".to_string());
    }
    Ok((from_date, to_date))
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

fn calendar_event_occurrence_summary(
    occurrence: &rustfin_db::repo::calendar::VisibleCalendarEventOccurrenceRow,
) -> CalendarEventOccurrenceSummary {
    CalendarEventOccurrenceSummary {
        title: occurrence.event.title.clone(),
        event_date: occurrence.event.event_date.clone(),
        occurs_on: occurrence.occurs_on.format("%F").to_string(),
        scope: occurrence.event.scope.clone(),
        event_type: occurrence.event.event_type.clone(),
        owner_username: occurrence.event.owner_username.clone(),
    }
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

async fn system_get_ai_runtime_summary(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    let summary = crate::ai_runtime::collect_ai_runtime_response(state).await;
    Ok((
        "Rustyfin AI runtime summary".to_string(),
        serde_json::to_value(summary).unwrap_or_else(|_| json!({})),
    ))
}

async fn ai_list_background_jobs(state: &AppState) -> Result<(String, serde_json::Value), String> {
    let jobs = rustfin_db::repo::jobs::list_jobs_filtered(&state.db, &[], None, Some(50), None)
        .await
        .map_err(|e| format!("failed to list jobs: {e}"))?;
    let observed_at = chrono::Utc::now().to_rfc3339();
    let total_count = jobs.len();
    let queued_count = jobs.iter().filter(|job| job.status == "queued").count();
    let running_count = jobs.iter().filter(|job| job.status == "running").count();
    let failed_count = jobs
        .iter()
        .filter(|job| matches!(job.status.as_str(), "failed" | "cancelled" | "error"))
        .count();
    let jobs = jobs
        .into_iter()
        .map(|job| {
            json!({
                "id": job.id,
                "kind": job.kind,
                "status": job.status,
                "progress": job.progress,
                "payload_json": job.payload_json,
                "error": job.error,
                "created_ts": job.created_ts,
                "updated_ts": job.updated_ts,
            })
        })
        .collect::<Vec<_>>();

    Ok((
        "Rustyfin background jobs".to_string(),
        json!({
            "observed_at": observed_at,
            "total_count": total_count,
            "queued_count": queued_count,
            "running_count": running_count,
            "failed_count": failed_count,
            "jobs": jobs,
        }),
    ))
}

async fn ai_get_job_status(state: &AppState) -> Result<(String, serde_json::Value), String> {
    let snapshot = state.runtime_metrics.snapshot();
    Ok((
        "Rustyfin AI job status".to_string(),
        json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "uptime_seconds": snapshot.uptime_seconds,
            "jobs": snapshot.jobs,
            "assistant": snapshot.assistant,
            "websockets": snapshot.websockets,
            "agents": snapshot.agents,
        }),
    ))
}

async fn ai_get_tool_registry() -> Result<(String, serde_json::Value), String> {
    let registry = default_tool_registry();
    let tools = AssistantToolName::all()
        .iter()
        .copied()
        .filter_map(|tool| registry.entry(tool).map(|entry| (tool, entry)))
        .map(|(tool, entry)| {
            let spec = entry.spec;
            json!({
                "tool": tool.as_str(),
                "summary": spec.summary,
                "provider_id": entry.provider_id,
                "domain_family": entry.domain_family.as_str(),
                "access_mode": tool_access_mode_label(spec.access_mode),
                "risk_tier": spec.risk_tier,
                "required_role": spec.required_role,
                "confirmation": spec.confirmation,
                "timeout_ms": spec.timeout_ms,
                "max_result_bytes": spec.max_result_bytes,
                "recovery_eligible": entry.recovery_eligible,
                "can_parallelize": entry.can_parallelize,
                "ambiguity_prone": entry.ambiguity_prone,
                "freshness_sensitive": entry.freshness_sensitive,
            })
        })
        .collect::<Vec<_>>();

    Ok((
        "Rustyfin AI tool registry".to_string(),
        json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "tool_count": tools.len(),
            "tools": tools,
        }),
    ))
}

async fn ai_get_grounding_summary(state: &AppState) -> Result<(String, serde_json::Value), String> {
    let runtime = crate::ai_runtime::collect_ai_runtime_response(state).await;
    Ok((
        "Rustyfin AI grounding summary".to_string(),
        json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "model": runtime.model,
            "turn": runtime.turn,
            "scheduler": runtime.scheduler,
            "resources": runtime.resources,
            "gpus": runtime.gpus,
            "role_routing": runtime.role_routing,
        }),
    ))
}

async fn ai_get_last_tool_failure_reason(
    state: &AppState,
) -> Result<(String, serde_json::Value), String> {
    let last_execution = {
        let engine = state.engine.lock().await;
        engine.last_execution_trace.clone()
    };

    let Some(trace) = last_execution else {
        return Ok((
            "Rustyfin AI last tool failure".to_string(),
            json!({
                "observed_at": chrono::Utc::now().to_rfc3339(),
                "available": false,
                "reason": "no prior AI execution trace was recorded",
            }),
        ));
    };

    let failure_attempt = trace
        .attempts
        .iter()
        .rev()
        .find(|attempt| {
            attempt.status != "ok"
                || !matches!(
                    attempt.outcome_kind,
                    super::types::AssistantToolOutcomeKind::Answer
                )
        })
        .cloned();

    Ok((
        "Rustyfin AI last tool failure".to_string(),
        json!({
            "observed_at": chrono::Utc::now().to_rfc3339(),
            "available": true,
            "stop_reason": trace.stop_reason.as_str(),
            "final_answer_path": trace.final_answer_path.as_str(),
            "deterministic_answer_used": trace.deterministic_answer_used,
            "synthesis_used": trace.synthesis_used,
            "attempt_count": trace.attempts.len(),
            "failure_attempt": failure_attempt,
        }),
    ))
}

fn tool_access_mode_label(mode: ToolAccessMode) -> &'static str {
    match mode {
        ToolAccessMode::ReadOnly => "read_only",
        ToolAccessMode::Write => "write",
        ToolAccessMode::DestructiveWrite => "destructive_write",
    }
}

async fn system_get_current_datetime() -> Result<(String, serde_json::Value), String> {
    let now = assistant_local_now();
    let summary = CurrentDateTimeAssistantSummary {
        local_timestamp: now.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
        local_date: now.format("%F").to_string(),
        local_time: now.format("%H:%M:%S").to_string(),
        weekday: now.format("%A").to_string(),
        timezone_offset: now.format("%:z").to_string(),
        unix_timestamp: now.timestamp(),
    };

    Ok((
        format!(
            "Rustyfin host local date and time: {} ({})",
            summary.local_date, summary.weekday
        ),
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
    let components = collect_service_health_components(state).await;
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

async fn system_get_service_detail(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing service query".to_string());
    };

    let components = collect_service_health_components(state).await;
    let Some((component, matched_by)) = system_find_service_detail(&components, query) else {
        return Err(format!("no service component matched \"{query}\""));
    };

    Ok((
        format!("Service detail for \"{}\"", component.name),
        serde_json::to_value(ServiceDetailSummary {
            query: query.clone(),
            matched_by,
            component,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn collect_service_health_components(state: &AppState) -> Vec<ServiceHealthComponentSummary> {
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
    components
}

fn system_find_service_detail(
    components: &[ServiceHealthComponentSummary],
    query: &str,
) -> Option<(ServiceHealthComponentSummary, String)> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized_query = normalize_service_component_query(trimmed);
    if normalized_query.is_empty() {
        return None;
    }

    let mut ranked = components
        .iter()
        .filter_map(|component| {
            service_component_match(component, &normalized_query).map(|matched_by| {
                let score =
                    service_component_match_score(component, &normalized_query, &matched_by);
                (score, component.clone(), matched_by)
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    ranked
        .into_iter()
        .next()
        .map(|(_, component, matched_by)| (component.clone(), matched_by))
}

fn normalize_service_component_query(query: &str) -> String {
    query
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn service_component_match(
    component: &ServiceHealthComponentSummary,
    query: &str,
) -> Option<String> {
    let normalized_name = normalize_service_component_query(&component.name);
    if normalized_name == query {
        return Some("exact_name".to_string());
    }
    if normalized_name.contains(query) || query.contains(&normalized_name) {
        return Some("name_contains".to_string());
    }

    for alias in service_component_aliases(&component.name) {
        let alias = normalize_service_component_query(alias);
        if alias == query {
            return Some(format!("alias:{alias}"));
        }
        if alias.contains(query) || query.contains(&alias) {
            return Some(format!("alias:{alias}"));
        }
    }

    None
}

fn service_component_match_score(
    component: &ServiceHealthComponentSummary,
    query: &str,
    matched_by: &str,
) -> (u8, u8, usize, usize) {
    let normalized_name = normalize_service_component_query(&component.name);
    let exact_match = u8::from(matched_by == "exact_name");
    let alias_match = u8::from(matched_by.starts_with("alias:"));
    let containment = u8::from(matched_by == "name_contains" || matched_by.starts_with("alias:"));
    let distance = normalized_name.len().abs_diff(query.len());
    (
        1 - exact_match,
        1 - alias_match,
        distance,
        containment as usize,
    )
}

fn service_component_aliases(name: &str) -> &'static [&'static str] {
    match name {
        "core_api" => &["core api", "core", "api", "backend"],
        "tmdb_agent" => &["tmdb", "metadata"],
        "youtube_agent" => &["youtube", "listen together", "video"],
        "transcription_agent" => &["transcription", "speech to text", "stt"],
        "servers_agent" => &["servers", "server agent", "minecraft"],
        "rustyvault" => &["vault", "rustyvault"],
        "ai_inference" => &["ai", "inference", "model", "llm"],
        _ => &[],
    }
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

async fn system_get_storage_path_detail(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing storage path query".to_string());
    };

    let summary = collect_storage_summary(state).await;
    if !summary.available {
        return Err(summary
            .reason
            .unwrap_or_else(|| "storage details are unavailable on this host.".to_string()));
    }

    let Some((path, matched_by)) = system_find_storage_path_detail(&summary.paths, query) else {
        return Err(format!("no storage path matched \"{query}\""));
    };

    Ok((
        format!("Storage path detail for \"{}\"", path.name),
        serde_json::to_value(StoragePathDetailSummary {
            query: query.clone(),
            matched_by,
            path,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_mount_detail(
    state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing storage mount query".to_string());
    };

    let summary = collect_storage_summary(state).await;
    if !summary.available {
        return Err(summary
            .reason
            .unwrap_or_else(|| "storage mount details are unavailable on this host.".to_string()));
    }

    let Some((mount, matched_by, total_count)) =
        system_find_storage_mount_detail(&summary.mounts, query)
    else {
        return Err(format!("no storage mount matched \"{query}\""));
    };

    Ok((
        format!("Storage mount detail for \"{}\"", mount.mount_point),
        serde_json::to_value(StorageMountDetailEnvelope {
            query: query.clone(),
            matched_by,
            total_count,
            mount,
        })
        .unwrap_or_else(|_| json!({})),
    ))
}

async fn system_get_port_conflicts(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemPortConflicts { query } = &call.input else {
        return Err("missing port conflict query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    #[cfg(target_os = "linux")]
    {
        let conflicts = collect_linux_port_conflicts().await?;
        let mut conflicts = if let Some(query) = _query.as_deref() {
            let filtered = conflicts
                .into_iter()
                .filter(|conflict| port_conflict_matches_query(conflict, query))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(format!("no listening sockets matched \"{query}\""));
            }
            filtered
        } else {
            conflicts
        };

        conflicts.sort_by(|left, right| {
            left.local_port
                .unwrap_or(u16::MAX)
                .cmp(&right.local_port.unwrap_or(u16::MAX))
                .then_with(|| left.protocol.cmp(&right.protocol))
                .then_with(|| left.local_address.cmp(&right.local_address))
        });

        let matched_by = match _query.as_deref() {
            Some(query) if query.chars().all(|ch| ch.is_ascii_digit()) => "port_exact".to_string(),
            Some(_) => "query_contains".to_string(),
            None => "listening_sockets".to_string(),
        };

        return Ok((
            match _query.as_deref() {
                Some(query) => format!("Port conflicts matching \"{query}\""),
                None => "Port conflicts".to_string(),
            },
            serde_json::to_value(SystemPortConflictsEnvelope {
                query: _query,
                matched_by,
                total_count: conflicts.len(),
                conflicts,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Port conflict details are only available on Linux hosts.".to_string())
    }
}

async fn system_get_port_conflict_detail(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemPortConflicts { query } = &call.input else {
        return Err("missing port conflict query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "missing port conflict query".to_string())?;

    #[cfg(target_os = "linux")]
    {
        let conflicts = collect_linux_port_conflicts().await?;
        let Some((conflict, matched_by, total_count)) =
            system_find_port_conflict_detail(&conflicts, &_query)
        else {
            return Err(format!("no port conflict matched \"{_query}\""));
        };

        return Ok((
            format!(
                "Port conflict detail for \"{}\"",
                conflict
                    .local_port
                    .map(|port| format!("{}:{port}", conflict.protocol.to_ascii_uppercase()))
                    .unwrap_or_else(|| conflict.raw_entry.clone())
            ),
            serde_json::to_value(SystemPortConflictDetailEnvelope {
                query: Some(_query),
                matched_by,
                total_count,
                conflict,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Port conflict details are only available on Linux hosts.".to_string())
    }
}

async fn system_get_process_detail(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing process detail query".to_string());
    };
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("missing process detail query".to_string());
    }

    let (label, data) = diagnostics::system_get_process_detail(&query).await?;
    Ok((label, data))
}

async fn system_get_listener_detail(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemPortConflicts { query } = &call.input else {
        return Err("missing listener detail query".to_string());
    };
    let query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "missing listener detail query".to_string())?;

    let (label, data) = diagnostics::system_get_listener_detail(&query).await?;
    Ok((label, data))
}

async fn system_get_disk_usage_detail(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemService { query } = &call.input else {
        return Err("missing disk usage detail query".to_string());
    };
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("missing disk usage detail query".to_string());
    }

    let (label, data) = diagnostics::system_get_disk_usage_detail(&query).await?;
    Ok((label, data))
}

async fn system_get_failed_units(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemFailedUnits { query } = &call.input else {
        return Err("missing failed unit query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    #[cfg(target_os = "linux")]
    {
        let units = collect_linux_failed_units().await?;
        let mut units = if let Some(query) = _query.as_deref() {
            let filtered = units
                .into_iter()
                .filter(|unit| failed_unit_matches_query(unit, query))
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return Err(format!("no failed systemd units matched \"{query}\""));
            }
            filtered
        } else {
            units
        };

        units.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.description.cmp(&right.description))
        });

        let matched_by = match _query.as_deref() {
            Some(query) if query.ends_with(".service") || query.ends_with(".socket") => {
                "unit_exact".to_string()
            }
            Some(_) => "query_contains".to_string(),
            None => "failed_units".to_string(),
        };

        return Ok((
            match _query.as_deref() {
                Some(query) => format!("Failed systemd units matching \"{query}\""),
                None => "Failed systemd units".to_string(),
            },
            serde_json::to_value(SystemFailedUnitsEnvelope {
                query: _query,
                matched_by,
                total_count: units.len(),
                units,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Failed systemd unit details are only available on Linux hosts.".to_string())
    }
}

async fn system_get_failed_unit_detail(
    _state: &AppState,
    _context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let AssistantToolInput::SystemFailedUnits { query } = &call.input else {
        return Err("missing failed unit query".to_string());
    };
    let _query = query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "missing failed unit query".to_string())?;

    #[cfg(target_os = "linux")]
    {
        let units = collect_linux_failed_unit_candidates().await?;
        let Some((mut unit, matched_by)) = find_linux_failed_unit_detail(&units, &_query) else {
            return Err(format!("no failed systemd unit matched \"{_query}\""));
        };

        let output = run_linux_command(
            "systemctl",
            &[
                "show",
                &unit.name,
                "--property=LoadState,ActiveState,SubState,FragmentPath,UnitFileState,MainPID,ExecMainStatus,ExecMainCode,Description",
            ],
            4,
        )
        .await
        .map_err(|e| format!("failed to load failed unit detail: {e}"))?;
        let properties = parse_linux_systemctl_properties(&output);

        if let Some(value) = properties.get("LoadState").cloned() {
            unit.load = value;
        }
        if let Some(value) = properties.get("ActiveState").cloned() {
            unit.active = value;
        }
        if let Some(value) = properties.get("SubState").cloned() {
            unit.sub = value;
        }
        if let Some(value) = properties.get("Description").cloned() {
            unit.description = value;
        }
        unit.recent_log_excerpt = collect_linux_failed_unit_excerpt(&unit.name).await;

        let detail = SystemFailedUnitDetailSummary {
            unit: unit.clone(),
            status: SystemFailedUnitDetailStatusSummary {
                fragment_path: properties
                    .get("FragmentPath")
                    .cloned()
                    .filter(|value| !value.is_empty()),
                unit_file_state: properties
                    .get("UnitFileState")
                    .cloned()
                    .filter(|value| !value.is_empty()),
                main_pid: properties
                    .get("MainPID")
                    .and_then(|value| value.parse::<u32>().ok()),
                exec_main_code: properties
                    .get("ExecMainCode")
                    .cloned()
                    .filter(|value| !value.is_empty()),
                exec_main_status: properties
                    .get("ExecMainStatus")
                    .cloned()
                    .filter(|value| !value.is_empty()),
                status_excerpt: collect_linux_failed_unit_status_excerpt(&unit.name).await,
            },
        };

        return Ok((
            format!("Failed systemd unit detail for \"{}\"", unit.name),
            serde_json::to_value(SystemFailedUnitDetailEnvelope {
                query: Some(_query),
                matched_by,
                detail,
            })
            .unwrap_or_else(|_| json!({})),
        ));
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Failed systemd unit details are only available on Linux hosts.".to_string())
    }
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

fn system_find_storage_path_detail(
    paths: &[StoragePathSummary],
    query: &str,
) -> Option<(StoragePathSummary, String)> {
    let normalized_query = normalize_storage_path_query(query);
    let raw_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() && raw_query.is_empty() {
        return None;
    }

    let mut ranked = paths
        .iter()
        .filter_map(|path| {
            storage_path_match_score(path, &normalized_query, &raw_query)
                .map(|(score, matched_by)| (score, path.clone(), matched_by))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    ranked
        .into_iter()
        .next()
        .map(|(_, path, matched_by)| (path, matched_by))
}

fn storage_path_match_score(
    path: &StoragePathSummary,
    normalized_query: &str,
    raw_query: &str,
) -> Option<(u8, String)> {
    let mut best: Option<(u8, String)> = None;
    let mut consider = |field: &str, value: Option<&str>, exact_score: u8, contains_score: u8| {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let value_raw = value.to_ascii_lowercase();
        let value_normalized = normalize_storage_path_query(value);

        let exact_match = (!raw_query.is_empty() && value_raw == raw_query)
            || (!normalized_query.is_empty() && value_normalized == normalized_query);
        let contains_match = (!raw_query.is_empty()
            && (value_raw.contains(raw_query) || raw_query.contains(&value_raw)))
            || (!normalized_query.is_empty()
                && (value_normalized.contains(normalized_query)
                    || normalized_query.contains(&value_normalized)));

        if exact_match {
            let score = exact_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} exact match")));
            }
        } else if contains_match {
            let score = contains_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} contains match")));
            }
        }
    };

    consider("name", Some(path.name.as_str()), 100, 92);
    consider("path", Some(path.path.as_str()), 98, 90);
    consider("resolved_path", path.resolved_path.as_deref(), 96, 88);
    consider("stats_path", path.stats_path.as_deref(), 94, 86);
    consider("mount_point", path.mount_point.as_deref(), 88, 80);
    consider("mount_source", path.mount_source.as_deref(), 84, 76);

    best
}

fn system_find_storage_mount_detail(
    mounts: &[StorageMountSummary],
    query: &str,
) -> Option<(StorageMountSummary, String, usize)> {
    let normalized_query = normalize_storage_path_query(query);
    let raw_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() && raw_query.is_empty() {
        return None;
    }

    let mut ranked = mounts
        .iter()
        .filter_map(|mount| {
            storage_mount_match_score(mount, &normalized_query, &raw_query)
                .map(|(score, matched_by)| (score, mount.clone(), matched_by))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.mount_point.cmp(&right.1.mount_point))
    });
    let total_count = ranked.len();
    ranked
        .into_iter()
        .next()
        .map(|(_, mount, matched_by)| (mount, matched_by, total_count))
}

fn storage_mount_match_score(
    mount: &StorageMountSummary,
    normalized_query: &str,
    raw_query: &str,
) -> Option<(u8, String)> {
    let mut best: Option<(u8, String)> = None;
    let mut consider = |field: &str, value: Option<&str>, exact_score: u8, contains_score: u8| {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let value_raw = value.to_ascii_lowercase();
        let value_normalized = normalize_storage_path_query(value);

        let exact_match = (!raw_query.is_empty() && value_raw == raw_query)
            || (!normalized_query.is_empty() && value_normalized == normalized_query);
        let contains_match = (!raw_query.is_empty()
            && (value_raw.contains(raw_query) || raw_query.contains(&value_raw)))
            || (!normalized_query.is_empty()
                && (value_normalized.contains(normalized_query)
                    || normalized_query.contains(&value_normalized)));

        if exact_match {
            let score = exact_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} exact match")));
            }
        } else if contains_match {
            let score = contains_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} contains match")));
            }
        }
    };

    consider("mount_point", Some(mount.mount_point.as_str()), 100, 92);
    consider("mount_source", mount.mount_source.as_deref(), 96, 88);
    consider(
        "mount_file_system",
        mount.mount_file_system.as_deref(),
        94,
        86,
    );
    for tracked_path in &mount.tracked_paths {
        consider("tracked_path", Some(tracked_path.as_str()), 98, 90);
    }

    best
}

fn normalize_storage_path_query(query: &str) -> String {
    query
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!', ':', ';'].contains(&ch))
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

async fn calendar_upcoming_birthdays(
    state: &AppState,
    context: &AssistantContext,
    call: &PlannedToolCall,
) -> Result<(String, serde_json::Value), String> {
    let (from, to, label) = calendar_window_for_call(call, 30);
    let birthday_query = resolve_birthday_query_for_context(context, calendar_query_for_call(call));
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
            library_id: item.library_id.clone(),
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

async fn libraries_find_duplicate_titles(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let libraries = accessible_libraries_for_context(state, context).await?;
    let library_names: HashMap<_, _> = libraries
        .iter()
        .map(|library| (library.id.clone(), library.name.clone()))
        .collect();

    let mut groups = BTreeMap::<String, Vec<rustfin_db::repo::items::ItemRow>>::new();
    for library in &libraries {
        for item in rustfin_db::repo::items::get_library_items(&state.db, &library.id)
            .await
            .map_err(|e| format!("failed to load library items: {e}"))?
        {
            groups
                .entry(normalize_library_duplicate_key(&item))
                .or_default()
                .push(item);
        }
    }

    let mut duplicates: Vec<_> = groups
        .into_iter()
        .filter_map(|(_, items)| {
            if items.len() <= 1 {
                return None;
            }
            let mut library_ids = HashSet::<String>::new();
            for item in &items {
                library_ids.insert(item.library_id.clone());
            }
            let title = items
                .first()
                .map(|item| item.title.trim().to_string())
                .filter(|value| !value.is_empty())?;
            let mut libraries = library_ids
                .into_iter()
                .map(|library_id| {
                    library_names
                        .get(&library_id)
                        .cloned()
                        .unwrap_or(library_id)
                })
                .collect::<Vec<_>>();
            libraries.sort();
            libraries.dedup();
            Some(LibraryDuplicateTitleSummary {
                title,
                item_count: items.len(),
                library_count: libraries.len(),
                libraries,
            })
        })
        .collect();

    duplicates.sort_by(|left, right| {
        right
            .item_count
            .cmp(&left.item_count)
            .then_with(|| right.library_count.cmp(&left.library_count))
            .then_with(|| left.title.cmp(&right.title))
    });
    let duplicate_group_count = duplicates.len();
    let total_count = duplicates
        .iter()
        .map(|group| group.item_count)
        .sum::<usize>();
    duplicates.truncate(12);

    Ok((
        "Duplicate library titles across accessible libraries".to_string(),
        json!({
            "total_count": total_count,
            "duplicate_group_count": duplicate_group_count,
            "duplicates": duplicates,
        }),
    ))
}

async fn libraries_list_missing_metadata(
    state: &AppState,
    context: &AssistantContext,
) -> Result<(String, serde_json::Value), String> {
    let libraries = accessible_libraries_for_context(state, context).await?;
    let library_names: HashMap<_, _> = libraries
        .iter()
        .map(|library| (library.id.clone(), library.name.clone()))
        .collect();

    let mut items = Vec::new();
    for library in &libraries {
        let library_items = rustfin_db::repo::items::get_library_items(&state.db, &library.id)
            .await
            .map_err(|e| format!("failed to load library items: {e}"))?;
        for item in library_items {
            let missing_fields = library_item_missing_metadata_fields(&item);
            if missing_fields.is_empty() {
                continue;
            }
            items.push(LibraryMissingMetadataItemSummary {
                library_id: item.library_id.clone(),
                library_name: library_names.get(&item.library_id).cloned(),
                id: item.id,
                title: item.title,
                kind: item.kind,
                year: item.year,
                missing_fields,
                created_ts: item.created_ts,
                updated_ts: item.updated_ts,
            });
        }
    }

    items.sort_by(|left, right| {
        right
            .missing_fields
            .len()
            .cmp(&left.missing_fields.len())
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.library_id.cmp(&right.library_id))
    });
    let missing_item_count = items.len();
    items.truncate(12);

    Ok((
        "Library items with missing metadata across accessible libraries".to_string(),
        json!({
            "total_count": missing_item_count,
            "missing_item_count": missing_item_count,
            "items": items,
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
        | AssistantToolInput::CalendarCreateBirthday { event_date, .. } => (
            event_date.clone(),
            event_date.clone(),
            "the created calendar event".to_string(),
        ),
        AssistantToolInput::CalendarDeleteEvent { event_date, .. } => (
            event_date.clone(),
            event_date.clone(),
            "the deleted calendar event".to_string(),
        ),
        AssistantToolInput::None
        | AssistantToolInput::ChannelsFilter { .. }
        | AssistantToolInput::DownloadsFilter { .. }
        | AssistantToolInput::NetworkInterface { .. }
        | AssistantToolInput::NetworkDefaultRoute { .. }
        | AssistantToolInput::NetworkHostnameAliases { .. }
        | AssistantToolInput::NetworkDnsServers { .. }
        | AssistantToolInput::NetworkRouteDestination { .. }
        | AssistantToolInput::NetworkActiveConnection { .. }
        | AssistantToolInput::LibrarySearch { .. }
        | AssistantToolInput::LibraryRecent { .. }
        | AssistantToolInput::Weather { .. }
        | AssistantToolInput::WeatherHistory { .. }
        | AssistantToolInput::WebSearch { .. }
        | AssistantToolInput::WebFetch { .. }
        | AssistantToolInput::DocumentCreateDownload { .. }
        | AssistantToolInput::ConversationArchive { .. }
        | AssistantToolInput::ConversationDelete { .. }
        | AssistantToolInput::ConversationMoveToGroup { .. }
        | AssistantToolInput::CurrentDateTime { .. }
        | AssistantToolInput::RoomsFilter { .. }
        | AssistantToolInput::SystemService { .. }
        | AssistantToolInput::SystemPortConflicts { .. }
        | AssistantToolInput::SystemFailedUnits { .. }
        | AssistantToolInput::ServerFilter { .. }
        | AssistantToolInput::DictionaryGetAccountIdentity
        | AssistantToolInput::DictionaryListVisibleWorkspaces
        | AssistantToolInput::DictionaryBrowseWorkspacePeople { .. }
        | AssistantToolInput::DictionarySearchPeople { .. }
        | AssistantToolInput::DictionaryGetPersonBundle { .. }
        | AssistantToolInput::DictionaryResolveRelationshipReference { .. } => {
            let from = assistant_local_today();
            let to = from + Duration::days(fallback_days);
            (
                from.format("%F").to_string(),
                to.format("%F").to_string(),
                format!("the next {fallback_days} days"),
            )
        }
        _ => {
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

fn resolve_birthday_query_for_context(
    context: &AssistantContext,
    query: Option<String>,
) -> Option<String> {
    let query = query
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let lower = query.to_ascii_lowercase();
    if matches!(lower.as_str(), "my" | "me" | "mine") {
        Some(context.username.clone())
    } else {
        Some(query)
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
            entry_id: entry.id.clone(),
            citation_id: format!("transcript:{}:{}", entry.session_id, entry.id),
            channel_id: entry.channel_id.clone(),
            session_id: entry.session_id.clone(),
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

#[cfg(target_os = "linux")]
async fn run_linux_command(
    program: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<String, String> {
    use tokio::process::Command;

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        Command::new(program).args(args).output(),
    )
    .await
    .map_err(|_| format!("timed out while running `{program}`"))?
    .map_err(|error| format!("failed to run `{program}`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("`{program}` exited with {}", output.status)
        } else {
            format!("`{program}` failed: {stderr}")
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "linux")]
fn detect_linux_host_label() -> Option<String> {
    if let Ok(value) = std::env::var("HOSTNAME") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
async fn linux_hostname_short_name() -> Result<Option<String>, String> {
    match run_linux_command("hostname", &["-s"], 2).await {
        Ok(output) => Ok(output
            .split_whitespace()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)),
        Err(_) => Ok(None),
    }
}

#[cfg(target_os = "linux")]
async fn linux_hostname_fqdn() -> Result<Option<String>, String> {
    match run_linux_command("hostname", &["-f"], 2).await {
        Ok(output) => Ok(output
            .split_whitespace()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)),
        Err(_) => Ok(None),
    }
}

#[cfg(target_os = "linux")]
async fn collect_linux_default_routes() -> Result<Vec<NetworkDefaultRouteSummary>, String> {
    let output = run_linux_command("ip", &["route", "show", "default"], 3).await?;
    let mut routes = output
        .lines()
        .filter_map(parse_linux_default_route_line)
        .collect::<Vec<_>>();
    routes.sort_by(|left, right| {
        left.metric
            .unwrap_or(u32::MAX)
            .cmp(&right.metric.unwrap_or(u32::MAX))
            .then_with(|| left.interface.cmp(&right.interface))
            .then_with(|| left.gateway.cmp(&right.gateway))
            .then_with(|| left.route.cmp(&right.route))
    });
    Ok(routes)
}

#[cfg(target_os = "linux")]
fn parse_linux_default_route_line(line: &str) -> Option<NetworkDefaultRouteSummary> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    if tokens.first().copied() != Some("default") {
        return None;
    }

    let mut gateway = None;
    let mut interface = None;
    let mut source = None;
    let mut metric = None;
    let mut protocol = None;
    let mut scope = None;

    let mut index = 1usize;
    while index < tokens.len() {
        match tokens[index] {
            "via" => {
                index += 1;
                gateway = tokens.get(index).map(|value| (*value).to_string());
            }
            "dev" => {
                index += 1;
                interface = tokens.get(index).map(|value| (*value).to_string());
            }
            "src" => {
                index += 1;
                source = tokens.get(index).map(|value| (*value).to_string());
            }
            "metric" => {
                index += 1;
                metric = tokens
                    .get(index)
                    .and_then(|value| value.parse::<u32>().ok());
            }
            "proto" => {
                index += 1;
                protocol = tokens.get(index).map(|value| (*value).to_string());
            }
            "scope" => {
                index += 1;
                scope = tokens.get(index).map(|value| (*value).to_string());
            }
            _ => {}
        }
        index += 1;
    }

    let mut route_parts = vec!["default".to_string()];
    if let Some(value) = gateway.as_deref() {
        route_parts.push("via".to_string());
        route_parts.push(value.to_string());
    }
    if let Some(value) = interface.as_deref() {
        route_parts.push("dev".to_string());
        route_parts.push(value.to_string());
    }
    if let Some(value) = source.as_deref() {
        route_parts.push("src".to_string());
        route_parts.push(value.to_string());
    }
    if let Some(value) = metric {
        route_parts.push("metric".to_string());
        route_parts.push(value.to_string());
    }
    if let Some(value) = protocol.as_deref() {
        route_parts.push("proto".to_string());
        route_parts.push(value.to_string());
    }
    if let Some(value) = scope.as_deref() {
        route_parts.push("scope".to_string());
        route_parts.push(value.to_string());
    }

    Some(NetworkDefaultRouteSummary {
        route: route_parts.join(" "),
        gateway,
        interface,
        source,
        metric,
        protocol,
        scope,
    })
}

#[cfg(target_os = "linux")]
fn default_route_matches_query(route: &NetworkDefaultRouteSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    let digits_only = query
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if let Ok(metric) = digits_only.parse::<u32>()
        && route.metric.is_some_and(|candidate| candidate == metric)
    {
        return true;
    }

    [
        route.route.as_str(),
        route.gateway.as_deref().unwrap_or_default(),
        route.interface.as_deref().unwrap_or_default(),
        route.source.as_deref().unwrap_or_default(),
        route.protocol.as_deref().unwrap_or_default(),
        route.scope.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .map(|value| value.to_ascii_lowercase())
    .any(|value| value.contains(&query))
}

#[cfg(target_os = "linux")]
async fn collect_linux_hostname_aliases() -> Result<Vec<NetworkHostnameAliasSummary>, String> {
    let canonical_hostname = linux_hostname_short_name().await.ok().flatten();
    let host_label = detect_linux_host_label();
    let fqdn = linux_hostname_fqdn().await.ok().flatten();

    let mut aliases = BTreeMap::<String, NetworkHostnameAliasSummary>::new();
    let mut insert_alias = |name: &str, source: &str| {
        let trimmed = name.trim();
        if trimmed.is_empty()
            || hostname_name_is_primary(
                trimmed,
                canonical_hostname.as_deref(),
                host_label.as_deref(),
                fqdn.as_deref(),
            )
        {
            return;
        }
        aliases
            .entry(trimmed.to_ascii_lowercase())
            .or_insert_with(|| NetworkHostnameAliasSummary {
                name: trimmed.to_string(),
                source: source.to_string(),
            });
    };

    if let Ok(output) = run_linux_command("hostname", &["-a"], 2).await {
        for alias in output.split_whitespace() {
            insert_alias(alias, "hostname -a");
        }
    }

    if let Ok(output) = run_linux_command("hostname", &["-A"], 2).await {
        for alias in output.split_whitespace() {
            insert_alias(alias, "hostname -A");
        }
    }

    if let Ok(contents) = std::fs::read_to_string("/etc/hosts") {
        for line in contents.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 2 {
                continue;
            }
            let names = &fields[1..];
            if !names.iter().any(|name| {
                hostname_name_is_primary(
                    name,
                    canonical_hostname.as_deref(),
                    host_label.as_deref(),
                    fqdn.as_deref(),
                )
            }) {
                continue;
            }
            for name in names {
                insert_alias(name, "/etc/hosts");
            }
        }
    }

    Ok(aliases.into_values().collect())
}

#[cfg(target_os = "linux")]
fn hostname_name_is_primary(
    name: &str,
    canonical_hostname: Option<&str>,
    host_label: Option<&str>,
    fqdn: Option<&str>,
) -> bool {
    [canonical_hostname, host_label, fqdn]
        .into_iter()
        .flatten()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

#[cfg(target_os = "linux")]
fn hostname_alias_matches_query(alias: &NetworkHostnameAliasSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    alias.name.to_ascii_lowercase().contains(&query)
        || query.contains(&alias.name.to_ascii_lowercase())
        || alias.source.to_ascii_lowercase().contains(&query)
}

#[cfg(target_os = "linux")]
async fn collect_linux_dns_servers() -> Result<Vec<NetworkDnsServerSummary>, String> {
    let mut servers = Vec::new();

    if let Ok(output) = run_linux_command("resolvectl", &["status"], 3).await {
        servers.extend(parse_linux_resolvectl_dns_servers(&output));
    }

    if servers.is_empty()
        && let Ok(contents) = std::fs::read_to_string("/etc/resolv.conf")
    {
        servers.extend(parse_linux_resolv_conf_dns_servers(&contents));
    }

    servers.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.interface.cmp(&right.interface))
            .then_with(|| left.server.cmp(&right.server))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.raw_line.cmp(&right.raw_line))
    });
    servers.dedup_by(|left, right| {
        left.scope.eq_ignore_ascii_case(&right.scope)
            && left
                .interface
                .as_deref()
                .unwrap_or_default()
                .eq_ignore_ascii_case(right.interface.as_deref().unwrap_or_default())
            && left.server.eq_ignore_ascii_case(&right.server)
            && left.source.eq_ignore_ascii_case(&right.source)
    });

    Ok(servers)
}

#[cfg(target_os = "linux")]
fn parse_linux_resolvectl_dns_servers(stdout: &str) -> Vec<NetworkDnsServerSummary> {
    let mut current_scope = "global".to_string();
    let mut current_interface = None::<String>;
    let mut servers = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("global") {
            current_scope = "global".to_string();
            current_interface = None;
            continue;
        }
        if let Some((scope, interface)) = parse_linux_resolvectl_link_scope(trimmed) {
            current_scope = scope;
            current_interface = interface;
            continue;
        }

        let Some(rest) = trimmed
            .strip_prefix("DNS Servers:")
            .or_else(|| trimmed.strip_prefix("Current DNS Server:"))
        else {
            continue;
        };

        for server in rest.split_whitespace() {
            let server = server.trim_matches(|ch: char| [';', ','].contains(&ch));
            if server.is_empty() {
                continue;
            }
            servers.push(NetworkDnsServerSummary {
                scope: current_scope.clone(),
                interface: current_interface.clone(),
                server: server.to_string(),
                source: "resolvectl".to_string(),
                raw_line: trimmed.to_string(),
            });
        }
    }

    servers
}

#[cfg(target_os = "linux")]
fn parse_linux_resolvectl_link_scope(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("link ") {
        return None;
    }

    let interface = trimmed
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(value, _)| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Some((trimmed.to_string(), interface))
}

#[cfg(target_os = "linux")]
fn parse_linux_resolv_conf_dns_servers(contents: &str) -> Vec<NetworkDnsServerSummary> {
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.split('#').next().unwrap_or("").trim();
            if !trimmed.starts_with("nameserver") {
                return None;
            }
            let server = trimmed.split_whitespace().nth(1)?.trim();
            if server.is_empty() {
                return None;
            }
            Some(NetworkDnsServerSummary {
                scope: "resolv.conf".to_string(),
                interface: None,
                server: server.to_string(),
                source: "/etc/resolv.conf".to_string(),
                raw_line: trimmed.to_string(),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn dns_server_matches_query(server: &NetworkDnsServerSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    server.scope.to_ascii_lowercase().contains(&query)
        || server
            .interface
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(&query))
            .unwrap_or(false)
        || server.server.to_ascii_lowercase().contains(&query)
        || server.source.to_ascii_lowercase().contains(&query)
        || server.raw_line.to_ascii_lowercase().contains(&query)
}

#[cfg(target_os = "linux")]
async fn collect_linux_port_conflicts() -> Result<Vec<SystemPortConflictSummary>, String> {
    let tcp = run_linux_command("ss", &["-H", "-ltnp"], 3).await?;
    let udp = run_linux_command("ss", &["-H", "-lunp"], 3).await?;

    let mut conflicts = parse_linux_port_conflicts("tcp", &tcp);
    conflicts.extend(parse_linux_port_conflicts("udp", &udp));
    Ok(conflicts)
}

#[cfg(target_os = "linux")]
fn parse_linux_port_conflicts(protocol: &str, stdout: &str) -> Vec<SystemPortConflictSummary> {
    stdout
        .lines()
        .filter_map(|line| parse_linux_port_conflict_line(protocol, line))
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_linux_port_conflict_line(protocol: &str, line: &str) -> Option<SystemPortConflictSummary> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let fields = trimmed.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 5 {
        return None;
    }

    let state = fields[0].to_string();
    let local_field = fields[3];
    let peer_field = fields[4];
    let users_field = if fields.len() > 5 {
        fields[5..].join(" ")
    } else {
        String::new()
    };
    let (local_address, local_port) = split_linux_socket_endpoint(local_field);
    let peer_address = normalize_linux_peer_field(peer_field);
    let processes = parse_linux_socket_processes(&users_field);

    Some(SystemPortConflictSummary {
        protocol: protocol.to_string(),
        state,
        local_address,
        local_port,
        peer_address,
        raw_entry: trimmed.to_string(),
        processes,
    })
}

#[cfg(target_os = "linux")]
fn split_linux_socket_endpoint(value: &str) -> (String, Option<u16>) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }

    if let Some((host, port)) = trimmed.rsplit_once(':') {
        let port = port.parse::<u16>().ok();
        (host.trim_matches(['[', ']']).to_string(), port)
    } else {
        (trimmed.to_string(), None)
    }
}

#[cfg(target_os = "linux")]
fn normalize_linux_peer_field(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "*:*" {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(target_os = "linux")]
fn parse_linux_socket_processes(users_field: &str) -> Vec<SystemPortConflictProcessSummary> {
    let Some(inner) = users_field.strip_prefix("users:(") else {
        return Vec::new();
    };
    let inner = inner.trim_end_matches(')');
    let inner = inner.trim().trim_start_matches('(').trim_end_matches(')');

    inner
        .split("),(")
        .filter_map(|entry| {
            let entry = entry.trim_matches(|ch| ch == '(' || ch == ')').trim();
            if entry.is_empty() {
                return None;
            }
            let name = entry.split('"').nth(1)?.to_string();
            let pid = entry
                .split("pid=")
                .nth(1)
                .and_then(|value| value.split(',').next())
                .and_then(|value| value.parse::<u32>().ok());
            let fd = entry
                .split("fd=")
                .nth(1)
                .and_then(|value| value.split(',').next())
                .and_then(|value| value.parse::<u32>().ok());
            Some(SystemPortConflictProcessSummary { name, pid, fd })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn port_conflict_matches_query(conflict: &SystemPortConflictSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    let digits_only = query
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if let Ok(port) = digits_only.parse::<u16>()
        && conflict
            .local_port
            .is_some_and(|candidate| candidate == port)
    {
        return true;
    }

    conflict.raw_entry.to_ascii_lowercase().contains(&query)
        || conflict.local_address.to_ascii_lowercase().contains(&query)
        || conflict
            .peer_address
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(&query))
            .unwrap_or(false)
        || conflict.protocol.to_ascii_lowercase().contains(&query)
        || conflict.processes.iter().any(|process| {
            process.name.to_ascii_lowercase().contains(&query)
                || process
                    .pid
                    .map(|pid| pid.to_string().contains(&query))
                    .unwrap_or(false)
        })
}

fn system_find_port_conflict_detail(
    conflicts: &[SystemPortConflictSummary],
    query: &str,
) -> Option<(SystemPortConflictSummary, String, usize)> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }

    let mut ranked = conflicts
        .iter()
        .filter_map(|conflict| {
            port_conflict_detail_match_score(conflict, &query)
                .map(|(score, matched_by)| (score, conflict.clone(), matched_by))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.local_port.cmp(&right.1.local_port))
            .then_with(|| left.1.protocol.cmp(&right.1.protocol))
            .then_with(|| left.1.local_address.cmp(&right.1.local_address))
            .then_with(|| left.1.raw_entry.cmp(&right.1.raw_entry))
    });
    let total_count = ranked.len();
    ranked
        .into_iter()
        .next()
        .map(|(_, conflict, matched_by)| (conflict, matched_by, total_count))
}

fn port_conflict_detail_match_score(
    conflict: &SystemPortConflictSummary,
    query: &str,
) -> Option<(u8, String)> {
    let mut best: Option<(u8, String)> = None;
    let mut consider = |field: &str, value: Option<&str>, exact_score: u8, contains_score: u8| {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let value_raw = value.to_ascii_lowercase();

        let exact_match = value_raw == query;
        let contains_match = value_raw.contains(query) || query.contains(&value_raw);

        if exact_match {
            let score = exact_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} exact match")));
            }
        } else if contains_match {
            let score = contains_score;
            if best
                .as_ref()
                .map(|(current, _)| *current < score)
                .unwrap_or(true)
            {
                best = Some((score, format!("{field} contains match")));
            }
        }
    };

    let digits_only = query
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if let Ok(port) = digits_only.parse::<u16>()
        && conflict
            .local_port
            .is_some_and(|candidate| candidate == port)
    {
        return Some((100, "port exact".to_string()));
    }

    consider(
        "process",
        conflict
            .processes
            .iter()
            .find(|process| process.name.to_ascii_lowercase().contains(query))
            .map(|process| process.name.as_str()),
        98,
        90,
    );
    consider(
        "local_address",
        Some(conflict.local_address.as_str()),
        96,
        88,
    );
    consider("protocol", Some(conflict.protocol.as_str()), 94, 84);
    consider("raw_entry", Some(conflict.raw_entry.as_str()), 92, 82);
    consider("peer_address", conflict.peer_address.as_deref(), 90, 80);
    if let Some(port) = conflict.local_port {
        let port_string = port.to_string();
        consider("local_port", Some(port_string.as_str()), 100, 92);
    }
    for process in &conflict.processes {
        consider("process", Some(process.name.as_str()), 98, 90);
        if let Some(pid) = process.pid {
            let pid_string = pid.to_string();
            consider("pid", Some(pid_string.as_str()), 96, 88);
        }
    }

    best
}

#[cfg(target_os = "linux")]
async fn collect_linux_failed_units() -> Result<Vec<SystemFailedUnitSummary>, String> {
    let output = run_linux_command(
        "systemctl",
        &["--failed", "--no-legend", "--plain", "--all"],
        3,
    )
    .await?;
    let mut units = parse_linux_failed_units(&output);
    units.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.description.cmp(&right.description))
    });
    units.truncate(8);

    for unit in units.iter_mut().take(4) {
        unit.recent_log_excerpt = collect_linux_failed_unit_excerpt(&unit.name).await;
    }

    Ok(units)
}

#[cfg(target_os = "linux")]
async fn collect_linux_failed_unit_candidates() -> Result<Vec<SystemFailedUnitSummary>, String> {
    let output = run_linux_command(
        "systemctl",
        &["--failed", "--no-legend", "--plain", "--all"],
        3,
    )
    .await?;
    Ok(parse_linux_failed_units(&output))
}

#[cfg(target_os = "linux")]
fn parse_linux_failed_units(stdout: &str) -> Vec<SystemFailedUnitSummary> {
    stdout
        .lines()
        .filter_map(parse_linux_failed_unit_line)
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_linux_failed_unit_line(line: &str) -> Option<SystemFailedUnitSummary> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let fields = trimmed.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 5 {
        return None;
    }

    let start_index = if fields.first().copied() == Some("●") {
        1
    } else {
        0
    };
    if fields.len() < start_index + 5 {
        return None;
    }

    Some(SystemFailedUnitSummary {
        name: fields[start_index].to_string(),
        load: fields[start_index + 1].to_string(),
        active: fields[start_index + 2].to_string(),
        sub: fields[start_index + 3].to_string(),
        description: fields[start_index + 4..].join(" "),
        recent_log_excerpt: None,
    })
}

#[cfg(target_os = "linux")]
fn parse_linux_systemctl_properties(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

#[cfg(target_os = "linux")]
async fn collect_linux_failed_unit_status_excerpt(unit: &str) -> Option<String> {
    let output = run_linux_command(
        "systemctl",
        &["status", unit, "--no-pager", "--full", "--lines", "6"],
        4,
    )
    .await
    .ok()?;

    let excerpt = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join(" | ");
    if excerpt.is_empty() {
        None
    } else {
        Some(excerpt)
    }
}

#[cfg(target_os = "linux")]
async fn collect_linux_failed_unit_excerpt(unit: &str) -> Option<String> {
    let output = run_linux_command(
        "journalctl",
        &["-u", unit, "-n", "3", "--no-pager", "-o", "cat"],
        3,
    )
    .await
    .ok()?;

    let excerpt = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    if excerpt.is_empty() {
        None
    } else {
        Some(excerpt)
    }
}

#[cfg(target_os = "linux")]
fn find_linux_failed_unit_detail(
    units: &[SystemFailedUnitSummary],
    query: &str,
) -> Option<(SystemFailedUnitSummary, String)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let exact_matches: Vec<_> = units
        .iter()
        .filter_map(|unit| {
            if unit.name.eq_ignore_ascii_case(query) {
                Some((unit.clone(), "exact_name".to_string()))
            } else {
                None
            }
        })
        .collect();
    if exact_matches.len() == 1 {
        return Some((exact_matches[0].0.clone(), exact_matches[0].1.clone()));
    }
    if exact_matches.len() > 1 {
        return None;
    }

    let partial_matches: Vec<_> = units
        .iter()
        .filter_map(|unit| {
            if failed_unit_matches_query(unit, query) {
                Some((unit.clone(), "query_contains".to_string()))
            } else {
                None
            }
        })
        .collect();

    if partial_matches.len() == 1 {
        Some((partial_matches[0].0.clone(), partial_matches[0].1.clone()))
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn failed_unit_matches_query(unit: &SystemFailedUnitSummary, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }

    unit.name.to_ascii_lowercase().contains(&query)
        || unit.description.to_ascii_lowercase().contains(&query)
        || unit.load.to_ascii_lowercase().contains(&query)
        || unit.active.to_ascii_lowercase().contains(&query)
        || unit.sub.to_ascii_lowercase().contains(&query)
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
        AssistantToolInput::CalendarDeleteEvent {
            event_date, title, ..
        } => AssistantFollowUpInputHint {
            calendar_label: Some("the deleted calendar event".to_string()),
            calendar_from_date: Some(event_date.clone()),
            calendar_to_date: Some(event_date.clone()),
            calendar_query: Some(title.clone()),
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
        AssistantToolInput::NetworkInterface { .. }
        | AssistantToolInput::NetworkDefaultRoute { .. }
        | AssistantToolInput::NetworkHostnameAliases { .. }
        | AssistantToolInput::NetworkDnsServers { .. }
        | AssistantToolInput::NetworkRouteDestination { .. }
        | AssistantToolInput::NetworkActiveConnection { .. }
        | AssistantToolInput::SystemService { .. }
        | AssistantToolInput::SystemPortConflicts { .. }
        | AssistantToolInput::SystemFailedUnits { .. } => AssistantFollowUpInputHint::default(),
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
        AssistantToolInput::WebSearch { query, category } => AssistantFollowUpInputHint {
            web_search_query: Some(query.clone()),
            web_category: category.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::WebFetch { url, category } => AssistantFollowUpInputHint {
            web_url: Some(url.clone()),
            web_category: category.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::DocumentCreateDownload { request_prompt, .. } => {
            AssistantFollowUpInputHint {
                web_search_query: Some(request_prompt.clone()),
                ..AssistantFollowUpInputHint::default()
            }
        }
        AssistantToolInput::ConversationArchive { .. }
        | AssistantToolInput::ConversationDelete { .. }
        | AssistantToolInput::ConversationMoveToGroup { .. } => {
            AssistantFollowUpInputHint::default()
        }
        AssistantToolInput::CurrentDateTime { location } => AssistantFollowUpInputHint {
            current_datetime_location: location.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::RoomsFilter { room_mode, query } => AssistantFollowUpInputHint {
            room_mode: room_mode.clone(),
            room_query: query.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        AssistantToolInput::DictionaryGetAccountIdentity
        | AssistantToolInput::DictionaryListVisibleWorkspaces
        | AssistantToolInput::DictionaryBrowseWorkspacePeople { .. }
        | AssistantToolInput::DictionarySearchPeople { .. }
        | AssistantToolInput::DictionaryGetPersonBundle { .. }
        | AssistantToolInput::DictionaryResolveRelationshipReference { .. } => {
            AssistantFollowUpInputHint::default()
        }
        AssistantToolInput::ServerFilter {
            query,
            availability,
        } => AssistantFollowUpInputHint {
            server_query: query.clone(),
            server_availability: availability.clone(),
            ..AssistantFollowUpInputHint::default()
        },
        _ => AssistantFollowUpInputHint::default(),
    }
}

fn follow_up_entities(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> Vec<AssistantFollowUpEntity> {
    match tool {
        AssistantToolName::ConversationsArchiveSelection
        | AssistantToolName::ConversationsDeleteSelection
        | AssistantToolName::ConversationsMoveToGroupSelection => block
            .data
            .get("conversations")
            .and_then(serde_json::Value::as_array)
            .map(|conversations| {
                conversations
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, conversation)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: conversation.get("title")?.as_str()?.to_string(),
                            identifier: conversation
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            kind: Some("ai_conversation".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarDeleteEvent => block
            .data
            .get("event")
            .map(|event| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: event
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Deleted calendar event")
                        .to_string(),
                    identifier: event
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    kind: Some("calendar_event".to_string()),
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::DocumentCreateDownload => block
            .data
            .get("artifact")
            .map(|artifact| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: artifact
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Generated document")
                        .to_string(),
                    identifier: artifact
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    kind: Some("generated_document".to_string()),
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
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
                            ..Default::default()
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
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarGetNextEventTiming => block
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
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                    ),
                    identifier: event
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    kind: Some("calendar_event".to_string()),
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarListDateConflicts => block
            .data
            .get("conflict_days")
            .and_then(serde_json::Value::as_array)
            .map(|days| {
                days.iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, day)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!(
                                "{} ({} events)",
                                day.get("date")?.as_str()?,
                                day.get("event_count")?.as_u64()?
                            ),
                            identifier: None,
                            kind: Some("calendar_date".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarListOverlappingEvents => block
            .data
            .get("conflict_days")
            .and_then(serde_json::Value::as_array)
            .map(|days| {
                days.iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, day)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!(
                                "{} ({} events)",
                                day.get("date")?.as_str()?,
                                day.get("event_count")?.as_u64()?
                            ),
                            identifier: None,
                            kind: Some("calendar_date".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarListFreeDays => block
            .data
            .get("free_days")
            .and_then(serde_json::Value::as_array)
            .map(|days| {
                days.iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, day)| {
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: day.get("date")?.as_str()?.to_string(),
                            identifier: None,
                            kind: Some("calendar_date".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarGetNextFreeDay => block
            .data
            .get("next_free_day")
            .and_then(|value| value.get("date"))
            .and_then(serde_json::Value::as_str)
            .map(|date| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: date.to_string(),
                    identifier: Some(date.to_string()),
                    kind: Some("calendar_date".to_string()),
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarCountEvents => block
            .data
            .get("day_counts")
            .and_then(serde_json::Value::as_array)
            .map(|days| {
                days.iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, day)| {
                        let date = day.get("date")?.as_str()?.to_string();
                        let event_count = day.get("event_count")?.as_u64()?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{date} ({event_count} events)"),
                            identifier: Some(date),
                            kind: Some("calendar_date".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::CalendarListBusyDays => block
            .data
            .get("busy_days")
            .and_then(serde_json::Value::as_array)
            .map(|days| {
                days.iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, day)| {
                        let date = day.get("date")?.as_str()?.to_string();
                        let event_count = day.get("event_count")?.as_u64()?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{date} ({event_count} events)"),
                            identifier: Some(date),
                            kind: Some("calendar_date".to_string()),
                            ..Default::default()
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
                            ..Default::default()
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
            ..Default::default()
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
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::MemoryListRecentFacts | AssistantToolName::MemorySearchFacts => block
            .data
            .get("facts")
            .and_then(serde_json::Value::as_array)
            .map(|facts| {
                facts
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, fact)| {
                        let title = fact.get("title").and_then(serde_json::Value::as_str)?;
                        let memory_key = fact
                            .get("memory_key")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string);
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: title.to_string(),
                            identifier: memory_key,
                            kind: Some("memory_fact".to_string()),
                            topic_key: fact
                                .get("topic_key")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string)
                                .or_else(|| {
                                    fact.get("memory_type")
                                        .and_then(serde_json::Value::as_str)
                                        .map(|kind| format!("memory:{kind}"))
                                }),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::MemoryListRecentChanges => {
            let mut entities = block
                .data
                .get("facts")
                .and_then(serde_json::Value::as_array)
                .map(|facts| {
                    facts
                        .iter()
                        .take(4)
                        .enumerate()
                        .filter_map(|(index, fact)| {
                            let title = fact.get("title").and_then(serde_json::Value::as_str)?;
                            Some(AssistantFollowUpEntity {
                                ordinal: index + 1,
                                label: title.to_string(),
                                identifier: fact
                                    .get("memory_key")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string),
                                kind: Some("memory_fact".to_string()),
                                topic_key: fact
                                    .get("topic_key")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string)
                                    .or_else(|| {
                                        fact.get("memory_type")
                                            .and_then(serde_json::Value::as_str)
                                            .map(|kind| format!("memory:{kind}"))
                                    }),
                                ..Default::default()
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if let Some(memory_entities) = block
                .data
                .get("entities")
                .and_then(serde_json::Value::as_array)
            {
                let offset = entities.len();
                entities.extend(memory_entities.iter().take(4).enumerate().filter_map(
                    move |(index, entity)| {
                        let label = entity.get("label").and_then(serde_json::Value::as_str)?;
                        let entity_kind = entity
                            .get("entity_kind")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("entity");
                        Some(AssistantFollowUpEntity {
                            ordinal: offset + index + 1,
                            label: format!("{label} ({entity_kind})"),
                            identifier: entity
                                .get("node_key")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            kind: Some(entity_kind.to_string()),
                            topic_key: entity
                                .get("topic_key")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            ..Default::default()
                        })
                    },
                ));
            }

            entities
        }
        AssistantToolName::MemoryListConflictingFacts => block
            .data
            .get("conflicts")
            .and_then(serde_json::Value::as_array)
            .map(|conflicts| {
                conflicts
                    .iter()
                    .take(4)
                    .enumerate()
                    .flat_map(|(group_index, conflict)| {
                        conflict
                            .get("facts")
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                            .take(2)
                            .enumerate()
                            .filter_map(move |(index, fact)| {
                                let title =
                                    fact.get("title").and_then(serde_json::Value::as_str)?;
                                Some(AssistantFollowUpEntity {
                                    ordinal: group_index * 2 + index + 1,
                                    label: title.to_string(),
                                    identifier: fact
                                        .get("memory_key")
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_string),
                                    kind: Some("memory_fact".to_string()),
                                    topic_key: fact
                                        .get("topic_key")
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_string)
                                        .or_else(|| {
                                            fact.get("memory_type")
                                                .and_then(serde_json::Value::as_str)
                                                .map(|kind| format!("memory:{kind}"))
                                        }),
                                    ..Default::default()
                                })
                            })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::MemoryGetEntityProvenance => {
            let mut entities = block
                .data
                .get("entity")
                .and_then(|value| {
                    serde_json::from_value::<MemoryEntityProvenanceSummary>(value.clone()).ok()
                })
                .map(|entity| {
                    vec![AssistantFollowUpEntity {
                        ordinal: 1,
                        label: format!("{} ({})", entity.label, entity.entity_kind),
                        identifier: Some(entity.node_key),
                        kind: Some(entity.entity_kind),
                        topic_key: entity.topic_key,
                        ..Default::default()
                    }]
                })
                .unwrap_or_default();

            if let Some(source_chunk) = block.data.get("source_chunk").and_then(|value| {
                serde_json::from_value::<MemoryProvenanceChunkSummary>(value.clone()).ok()
            }) {
                entities.push(AssistantFollowUpEntity {
                    ordinal: entities.len() + 1,
                    label: source_chunk.title,
                    identifier: Some(source_chunk.chunk_key),
                    kind: Some("memory_source".to_string()),
                    topic_key: source_chunk.topic_key,
                    ..Default::default()
                });
            }

            entities
        }
        AssistantToolName::MemoryListRecentEntities | AssistantToolName::MemorySearchEntities => {
            block
                .data
                .get("entities")
                .and_then(serde_json::Value::as_array)
                .map(|entities| {
                    entities
                        .iter()
                        .take(8)
                        .enumerate()
                        .filter_map(|(index, entity)| {
                            let label = entity.get("label").and_then(serde_json::Value::as_str)?;
                            let entity_kind = entity
                                .get("entity_kind")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("entity");
                            Some(AssistantFollowUpEntity {
                                ordinal: index + 1,
                                label: format!("{label} ({entity_kind})"),
                                identifier: entity
                                    .get("node_key")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string),
                                kind: Some(entity_kind.to_string()),
                                topic_key: entity
                                    .get("topic_key")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string),
                                ..Default::default()
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        AssistantToolName::MemoryFindExactEntity => block
            .data
            .get("entities")
            .and_then(serde_json::Value::as_array)
            .map(|entities| {
                entities
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, entity)| {
                        let label = entity.get("label").and_then(serde_json::Value::as_str)?;
                        let entity_kind = entity
                            .get("entity_kind")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("entity");
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{label} ({entity_kind})"),
                            identifier: entity
                                .get("node_key")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            kind: Some(entity_kind.to_string()),
                            topic_key: entity
                                .get("topic_key")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::MemoryGetEntityRelations => {
            let mut entities = block
                .data
                .get("root")
                .and_then(|value| serde_json::from_value::<MemoryEntitySummary>(value.clone()).ok())
                .map(|entity| {
                    vec![AssistantFollowUpEntity {
                        ordinal: 1,
                        label: format!("{} ({})", entity.label, entity.entity_kind),
                        identifier: Some(entity.node_key),
                        kind: Some(entity.entity_kind),
                        topic_key: entity.topic_key,
                        ..Default::default()
                    }]
                })
                .unwrap_or_default();

            if let Some(relations) = block
                .data
                .get("relations")
                .and_then(serde_json::Value::as_array)
            {
                entities.extend(relations.iter().take(8).enumerate().filter_map(
                    |(index, relation)| {
                        let entity = relation.get("entity")?;
                        let entity =
                            serde_json::from_value::<MemoryEntitySummary>(entity.clone()).ok()?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 2,
                            label: format!("{} ({})", entity.label, entity.entity_kind),
                            identifier: Some(entity.node_key),
                            kind: Some(entity.entity_kind),
                            topic_key: entity.topic_key,
                            ..Default::default()
                        })
                    },
                ));
            }

            entities
        }
        AssistantToolName::MemoryGetPersonSummary => {
            let mut entities = block
                .data
                .get("person")
                .and_then(|value| serde_json::from_value::<MemoryEntitySummary>(value.clone()).ok())
                .map(|entity| {
                    vec![AssistantFollowUpEntity {
                        ordinal: 1,
                        label: format!("{} ({})", entity.label, entity.entity_kind),
                        identifier: Some(entity.node_key),
                        kind: Some(entity.entity_kind),
                        topic_key: entity.topic_key,
                        ..Default::default()
                    }]
                })
                .unwrap_or_default();

            if let Some(relations) = block
                .data
                .get("relations")
                .and_then(serde_json::Value::as_array)
            {
                entities.extend(relations.iter().take(8).enumerate().filter_map(
                    |(index, relation)| {
                        let entity = relation.get("entity")?;
                        let entity =
                            serde_json::from_value::<MemoryEntitySummary>(entity.clone()).ok()?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 2,
                            label: format!("{} ({})", entity.label, entity.entity_kind),
                            identifier: Some(entity.node_key),
                            kind: Some(entity.entity_kind),
                            topic_key: entity.topic_key,
                            ..Default::default()
                        })
                    },
                ));
            }

            entities
        }
        AssistantToolName::MemoryGetEntityRelationPath => {
            let mut entities = block
                .data
                .get("root")
                .and_then(|value| serde_json::from_value::<MemoryEntitySummary>(value.clone()).ok())
                .map(|entity| {
                    vec![AssistantFollowUpEntity {
                        ordinal: 1,
                        label: format!("{} ({})", entity.label, entity.entity_kind),
                        identifier: Some(entity.node_key),
                        kind: Some(entity.entity_kind),
                        topic_key: entity.topic_key,
                        ..Default::default()
                    }]
                })
                .unwrap_or_default();

            if let Some(target) = block
                .data
                .get("target")
                .and_then(|value| serde_json::from_value::<MemoryEntitySummary>(value.clone()).ok())
            {
                entities.push(AssistantFollowUpEntity {
                    ordinal: entities.len() + 1,
                    label: format!("{} ({})", target.label, target.entity_kind),
                    identifier: Some(target.node_key),
                    kind: Some(target.entity_kind),
                    topic_key: target.topic_key,
                    ..Default::default()
                });
            }

            if let Some(path) = block.data.get("path").and_then(serde_json::Value::as_array) {
                entities.extend(path.iter().take(8).enumerate().filter_map(|(index, hop)| {
                    let entity = hop.get("entity")?;
                    let entity =
                        serde_json::from_value::<MemoryEntitySummary>(entity.clone()).ok()?;
                    Some(AssistantFollowUpEntity {
                        ordinal: index + 3,
                        label: format!("{} ({})", entity.label, entity.entity_kind),
                        identifier: Some(entity.node_key),
                        kind: Some(entity.entity_kind),
                        topic_key: entity.topic_key,
                        ..Default::default()
                    })
                }));
            }

            entities
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
            ..Default::default()
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
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::DownloadsGetArtifactDetails
        | AssistantToolName::DownloadsGetArtifactSource
        | AssistantToolName::DownloadsGetReleaseNotes
        | AssistantToolName::DownloadsGetArtifactChecksum
        | AssistantToolName::DownloadsGetArtifactInstallSteps
        | AssistantToolName::DownloadsGetArtifactCompatibility => vec![AssistantFollowUpEntity {
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
            kind: Some("download_artifact".to_string()),
            topic_key: block
                .data
                .get("artifact_id")
                .and_then(serde_json::Value::as_str)
                .map(|artifact_id| format!("downloads:{artifact_id}")),
            ..Default::default()
        }],
        AssistantToolName::NetworkGetTopologySummary => block
            .data
            .get("nodes")
            .and_then(serde_json::Value::as_array)
            .map(|nodes| {
                nodes
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, node)| {
                        let name = node.get("name").and_then(serde_json::Value::as_str)?;
                        let status = node
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{name} ({status})"),
                            identifier: Some(name.to_string()),
                            kind: Some("network_interface".to_string()),
                            topic_key: Some(format!("network:{name}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::NetworkGetInterfaceDetails => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("interface")
                .and_then(|interface| interface.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("interface")
                .and_then(|interface| interface.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            kind: Some("network_interface".to_string()),
            topic_key: block
                .data
                .get("interface")
                .and_then(|interface| interface.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(|name| format!("network:{name}")),
            ..Default::default()
        }],
        AssistantToolName::NetworkGetDefaultRoute => block
            .data
            .get("routes")
            .and_then(serde_json::Value::as_array)
            .map(|routes| {
                routes
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, route)| {
                        let label = route.get("route").and_then(serde_json::Value::as_str)?;
                        let gateway = route.get("gateway").and_then(serde_json::Value::as_str);
                        let interface = route.get("interface").and_then(serde_json::Value::as_str);
                        let topic_key = Some("network:default_route".to_string());
                        let identifier = interface
                            .map(str::to_string)
                            .or_else(|| gateway.map(str::to_string))
                            .or_else(|| Some(label.to_string()));
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: match (gateway, interface) {
                                (Some(gateway), Some(interface)) => {
                                    format!("{label} via {gateway} on {interface}")
                                }
                                (Some(gateway), None) => format!("{label} via {gateway}"),
                                (None, Some(interface)) => format!("{label} on {interface}"),
                                (None, None) => label.to_string(),
                            },
                            identifier,
                            kind: Some("network_route".to_string()),
                            topic_key,
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::NetworkGetHostnameAliases => block
            .data
            .get("aliases")
            .and_then(serde_json::Value::as_array)
            .map(|aliases| {
                aliases
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, alias)| {
                        let name = alias.get("name").and_then(serde_json::Value::as_str)?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: name.to_string(),
                            identifier: Some(name.to_string()),
                            kind: Some("network_hostname_alias".to_string()),
                            topic_key: Some("network:hostname_aliases".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::NetworkGetDnsServers => Vec::new(),
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
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrariesListAccessible => block
            .data
            .get("libraries")
            .and_then(serde_json::Value::as_array)
            .map(|libraries| {
                libraries
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, library)| {
                        let id = library.get("id").and_then(serde_json::Value::as_str)?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: library
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(id)
                                .to_string(),
                            identifier: Some(id.to_string()),
                            kind: Some("library".to_string()),
                            topic_key: Some(format!("library:{id}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrariesGetLibrarySummary => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            kind: Some("library".to_string()),
            topic_key: block
                .data
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|library_id| format!("library:{library_id}")),
            ..Default::default()
        }],
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
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrariesFindDuplicateTitles => block
            .data
            .get("duplicates")
            .and_then(serde_json::Value::as_array)
            .map(|duplicates| {
                duplicates
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, duplicate)| {
                        let title = duplicate.get("title").and_then(serde_json::Value::as_str)?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: title.to_string(),
                            identifier: Some(title.to_string()),
                            kind: Some("library_title_group".to_string()),
                            topic_key: Some(format!("library_title:{title}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::LibrariesListMissingMetadata => block
            .data
            .get("items")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let title = item.get("title").and_then(serde_json::Value::as_str)?;
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: title.to_string(),
                            identifier: item
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            kind: Some("library_item".to_string()),
                            topic_key: item
                                .get("library_id")
                                .and_then(serde_json::Value::as_str)
                                .map(|library_id| format!("library:{library_id}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetServiceHealth => block
            .data
            .get("components")
            .and_then(serde_json::Value::as_array)
            .map(|components| {
                components
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, component)| {
                        let name = component.get("name").and_then(serde_json::Value::as_str)?;
                        let status = component
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{name} ({status})"),
                            identifier: Some(name.to_string()),
                            kind: Some("system_service".to_string()),
                            topic_key: Some(format!("service:{name}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetServiceDetail => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("component")
                .and_then(|component| component.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("component")
                .and_then(|component| component.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            kind: Some("system_service".to_string()),
            topic_key: block
                .data
                .get("component")
                .and_then(|component| component.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(|name| format!("service:{name}")),
            ..Default::default()
        }],
        AssistantToolName::SystemGetPortConflicts => block
            .data
            .get("conflicts")
            .and_then(serde_json::Value::as_array)
            .map(|conflicts| {
                conflicts
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, conflict)| {
                        let protocol = conflict
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)?;
                        let local_address = conflict
                            .get("local_address")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("*");
                        let local_port = conflict
                            .get("local_port")
                            .and_then(serde_json::Value::as_u64);
                        let label = match local_port {
                            Some(port) => format!("{protocol} {local_address}:{port}"),
                            None => format!("{protocol} {local_address}"),
                        };
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: label.clone(),
                            identifier: Some(label),
                            kind: Some("system_port_conflict".to_string()),
                            topic_key: Some("system:port_conflicts".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetPortConflictDetail => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("local_port")
                .and_then(serde_json::Value::as_u64)
                .map(|port| {
                    format!(
                        "{}:{}",
                        block
                            .data
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("tcp")
                            .to_ascii_uppercase(),
                        port
                    )
                })
                .or_else(|| {
                    block
                        .data
                        .get("local_address")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| block.label.clone()),
            identifier: block
                .data
                .get("local_port")
                .and_then(serde_json::Value::as_u64)
                .map(|port| {
                    format!(
                        "{}:{}",
                        block
                            .data
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("tcp")
                            .to_ascii_uppercase(),
                        port
                    )
                })
                .or_else(|| {
                    block
                        .data
                        .get("local_address")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                }),
            kind: Some("system_port_conflict".to_string()),
            topic_key: Some("system:port_conflicts".to_string()),
            ..Default::default()
        }],
        AssistantToolName::SystemGetFailedUnits => block
            .data
            .get("units")
            .and_then(serde_json::Value::as_array)
            .map(|units| {
                units
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, unit)| {
                        let name = unit.get("name").and_then(serde_json::Value::as_str)?;
                        let active = unit
                            .get("active")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        let sub = unit
                            .get("sub")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{name} ({active}/{sub})"),
                            identifier: Some(name.to_string()),
                            kind: Some("system_failed_unit".to_string()),
                            topic_key: Some("system:failed_units".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetFailedUnitDetail => block
            .data
            .get("detail")
            .and_then(|value| {
                serde_json::from_value::<SystemFailedUnitDetailSummary>(value.clone()).ok()
            })
            .map(|detail| {
                vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: format!(
                        "{} ({}/{})",
                        detail.unit.name, detail.unit.load, detail.unit.active
                    ),
                    identifier: Some(detail.unit.name),
                    kind: Some("system_failed_unit".to_string()),
                    topic_key: Some("system:failed_units".to_string()),
                    ..Default::default()
                }]
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetStorageSummary => block
            .data
            .get("paths")
            .and_then(serde_json::Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, path)| {
                        let name = path.get("name").and_then(serde_json::Value::as_str)?;
                        let path_value = path
                            .get("path")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(name);
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: format!("{name} ({path_value})"),
                            identifier: Some(path_value.to_string()),
                            kind: Some("storage_path".to_string()),
                            topic_key: Some(format!("storage_path:{name}")),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetStoragePathDetail => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("path").and_then(serde_json::Value::as_str))
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("path")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_string),
            kind: Some("storage_path".to_string()),
            topic_key: block
                .data
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("path").and_then(serde_json::Value::as_str))
                .map(|name| format!("storage_path:{name}")),
            ..Default::default()
        }],
        AssistantToolName::SystemGetMountDetail => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("path").and_then(serde_json::Value::as_str))
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    block
                        .data
                        .get("mount_source")
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::to_string),
            kind: Some("storage_mount".to_string()),
            topic_key: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .map(|mount_point| format!("storage_mount:{mount_point}")),
            ..Default::default()
        }],
        AssistantToolName::SystemGetProcessDetail => block
            .data
            .get("processes")
            .and_then(serde_json::Value::as_array)
            .map(|processes| {
                processes
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, process)| {
                        let pid = process.get("pid").and_then(serde_json::Value::as_u64);
                        let command = process
                            .get("command")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("process");
                        let args = process
                            .get("args")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        let label = if args.is_empty() {
                            pid.map(|pid| format!("{command} pid={pid}"))
                                .unwrap_or_else(|| command.to_string())
                        } else {
                            format!("{command} {args}")
                        };
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: label.clone(),
                            identifier: pid
                                .map(|pid| pid.to_string())
                                .or_else(|| Some(command.to_string())),
                            kind: Some("system_process".to_string()),
                            topic_key: Some("system:processes".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetListenerDetail => block
            .data
            .get("listeners")
            .and_then(serde_json::Value::as_array)
            .map(|listeners| {
                listeners
                    .iter()
                    .take(8)
                    .enumerate()
                    .filter_map(|(index, listener)| {
                        let protocol = listener
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("tcp");
                        let local_address = listener
                            .get("local_address")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("*");
                        let local_port = listener
                            .get("local_port")
                            .and_then(serde_json::Value::as_u64);
                        let process = listener
                            .get("process")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        let label = match local_port {
                            Some(port) if !process.is_empty() => {
                                format!("{protocol} {local_address}:{port} {process}")
                            }
                            Some(port) => format!("{protocol} {local_address}:{port}"),
                            None if !process.is_empty() => {
                                format!("{protocol} {local_address} {process}")
                            }
                            None => format!("{protocol} {local_address}"),
                        };
                        Some(AssistantFollowUpEntity {
                            ordinal: index + 1,
                            label: label.clone(),
                            identifier: local_port
                                .map(|port| format!("{protocol}:{local_address}:{port}"))
                                .or_else(|| Some(format!("{protocol}:{local_address}"))),
                            kind: Some("system_listener".to_string()),
                            topic_key: Some("system:listeners".to_string()),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AssistantToolName::SystemGetDiskUsageDetail => vec![AssistantFollowUpEntity {
            ordinal: 1,
            label: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("query").and_then(serde_json::Value::as_str))
                .unwrap_or(&block.label)
                .to_string(),
            identifier: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .or_else(|| block.data.get("source").and_then(serde_json::Value::as_str))
                .map(str::to_string),
            kind: Some("storage_mount".to_string()),
            topic_key: block
                .data
                .get("mount_point")
                .and_then(serde_json::Value::as_str)
                .map(|mount_point| format!("storage_mount:{mount_point}")),
            ..Default::default()
        }],
        AssistantToolName::LibraryGetItemSummary
        | AssistantToolName::LibraryGetItemMediaDetails
        | AssistantToolName::LibraryGetItemSourcePaths => {
            vec![AssistantFollowUpEntity {
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
                kind: Some("library_item".to_string()),
                topic_key: block
                    .data
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(|item_id| format!("library_item:{item_id}"))
                    .or_else(|| {
                        block
                            .data
                            .get("library_id")
                            .and_then(serde_json::Value::as_str)
                            .map(|library_id| format!("library:{library_id}"))
                    }),
                ..Default::default()
            }]
        }
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
                            ..Default::default()
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
                            ..Default::default()
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
            ..Default::default()
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
                            ..Default::default()
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
            ..Default::default()
        }],
        AssistantToolName::WebSearchPublicWeb => block
            .data
            .get("results")
            .and_then(serde_json::Value::as_array)
            .map(|results| {
                let topic_key = block
                    .data
                    .get("category")
                    .and_then(serde_json::Value::as_str)
                    .map(|category| format!("web:{category}"))
                    .or_else(|| Some("web:public".to_string()));
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
                            kind: Some("web_result".to_string()),
                            topic_key: topic_key.clone(),
                            ..Default::default()
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
            kind: Some("web_page".to_string()),
            topic_key: block
                .data
                .get("category")
                .and_then(serde_json::Value::as_str)
                .map(|category| format!("web:{category}"))
                .or_else(|| Some("web:public".to_string())),
            ..Default::default()
        }],
        AssistantToolName::WeatherGetCurrent
        | AssistantToolName::WeatherGetForecast
        | AssistantToolName::WeatherGetHistory
        | AssistantToolName::WeatherResolveLocationAlias
        | AssistantToolName::WeatherGetForecastForDate
        | AssistantToolName::WeatherGetRecentHistoryForDate
        | AssistantToolName::AccountGetProfileSummary
        | AssistantToolName::SystemGetCurrentDateTime
        | AssistantToolName::SystemGetAiRuntimeSummary
        | AssistantToolName::SystemGetHostRuntimeSummary
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetRecentErrors => Vec::new(),
        _ => Vec::new(),
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
        || item.artifact_id.to_ascii_lowercase().contains(&query)
        || item
            .package_filename
            .as_deref()
            .map(|value| value.to_ascii_lowercase().contains(&query))
            .unwrap_or(false)
        || item.detail.to_ascii_lowercase().contains(&query)
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

fn downloads_find_artifact_detail<'a>(
    items: &'a [crate::downloads::DownloadArtifactResponse],
    query: &str,
    availability_filter: Option<&str>,
) -> Option<(&'a crate::downloads::DownloadArtifactResponse, String)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_ascii_lowercase();

    let filtered: Vec<_> = items
        .iter()
        .filter(|item| downloads_matches_availability(item, availability_filter))
        .collect();

    let exact_matches: Vec<_> = filtered
        .iter()
        .filter_map(|item| {
            if item.id.eq_ignore_ascii_case(query) {
                Some((*item, "id".to_string()))
            } else if item.artifact_id.eq_ignore_ascii_case(query) {
                Some((*item, "artifact_id".to_string()))
            } else if item.title.eq_ignore_ascii_case(query) {
                Some((*item, "title".to_string()))
            } else if item
                .package_filename
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(query))
            {
                Some((*item, "package_filename".to_string()))
            } else {
                None
            }
        })
        .collect();

    if exact_matches.len() == 1 {
        return Some((exact_matches[0].0, exact_matches[0].1.clone()));
    }
    if exact_matches.len() > 1 {
        return None;
    }

    let partial_matches: Vec<_> = filtered
        .iter()
        .filter_map(|item| {
            if item.id.to_ascii_lowercase().contains(&query_lower) {
                Some((*item, "id".to_string()))
            } else if item.artifact_id.to_ascii_lowercase().contains(&query_lower) {
                Some((*item, "artifact_id".to_string()))
            } else if item.title.to_ascii_lowercase().contains(&query_lower) {
                Some((*item, "title".to_string()))
            } else if item
                .package_filename
                .as_deref()
                .map(|value| value.to_ascii_lowercase().contains(&query_lower))
                .unwrap_or(false)
            {
                Some((*item, "package_filename".to_string()))
            } else if item.summary.to_ascii_lowercase().contains(&query_lower) {
                Some((*item, "summary".to_string()))
            } else if item.detail.to_ascii_lowercase().contains(&query_lower) {
                Some((*item, "detail".to_string()))
            } else {
                None
            }
        })
        .collect();

    if partial_matches.len() == 1 {
        Some((partial_matches[0].0, partial_matches[0].1.clone()))
    } else {
        None
    }
}

fn default_library_settings_row(
    library_id: &str,
) -> rustfin_db::repo::libraries::LibrarySettingsRow {
    rustfin_db::repo::libraries::LibrarySettingsRow {
        library_id: library_id.to_string(),
        show_images: true,
        prefer_local_artwork: true,
        fetch_online_artwork: true,
        tmdb_store_in_media_dir: false,
        tmdb_sync_on_new_media: true,
        tmdb_sync_schedule: "manual".to_string(),
        tmdb_last_sync_ts: None,
        tmdb_fetch_posters: true,
        tmdb_fetch_backdrops: true,
        tmdb_fetch_metadata: true,
        tmdb_fetch_reviews: false,
        updated_ts: chrono::Utc::now().timestamp(),
    }
}

fn library_settings_summary(
    settings: &rustfin_db::repo::libraries::LibrarySettingsRow,
) -> LibrarySettingsSummary {
    LibrarySettingsSummary {
        show_images: settings.show_images,
        prefer_local_artwork: settings.prefer_local_artwork,
        fetch_online_artwork: settings.fetch_online_artwork,
        tmdb_store_in_media_dir: settings.tmdb_store_in_media_dir,
        tmdb_sync_on_new_media: settings.tmdb_sync_on_new_media,
        tmdb_sync_schedule: settings.tmdb_sync_schedule.clone(),
        tmdb_last_sync_ts: settings.tmdb_last_sync_ts,
        tmdb_fetch_posters: settings.tmdb_fetch_posters,
        tmdb_fetch_backdrops: settings.tmdb_fetch_backdrops,
        tmdb_fetch_metadata: settings.tmdb_fetch_metadata,
        tmdb_fetch_reviews: settings.tmdb_fetch_reviews,
    }
}

fn library_matches_query(
    library: &rustfin_db::repo::libraries::LibraryRow,
    query: &str,
) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let query_lower = query.to_ascii_lowercase();
    if library.id.eq_ignore_ascii_case(query) {
        Some("id".to_string())
    } else if library.name.eq_ignore_ascii_case(query) {
        Some("name".to_string())
    } else if library.kind.eq_ignore_ascii_case(query) {
        Some("kind".to_string())
    } else if library.id.to_ascii_lowercase().contains(&query_lower) {
        Some("id".to_string())
    } else if library.name.to_ascii_lowercase().contains(&query_lower) {
        Some("name".to_string())
    } else if library.kind.to_ascii_lowercase().contains(&query_lower) {
        Some("kind".to_string())
    } else {
        None
    }
}

fn libraries_find_library_detail<'a>(
    libraries: &'a [rustfin_db::repo::libraries::LibraryRow],
    query: &str,
) -> Option<(&'a rustfin_db::repo::libraries::LibraryRow, String)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let exact_matches: Vec<_> = libraries
        .iter()
        .filter_map(|library| {
            if library.id.eq_ignore_ascii_case(query) {
                Some((library, "id".to_string()))
            } else if library.name.eq_ignore_ascii_case(query) {
                Some((library, "name".to_string()))
            } else if library.kind.eq_ignore_ascii_case(query) {
                Some((library, "kind".to_string()))
            } else {
                None
            }
        })
        .collect();

    if exact_matches.len() == 1 {
        return Some((exact_matches[0].0, exact_matches[0].1.clone()));
    }
    if exact_matches.len() > 1 {
        return None;
    }

    let partial_matches: Vec<_> = libraries
        .iter()
        .filter_map(|library| {
            library_matches_query(library, query).map(|matched_by| (library, matched_by))
        })
        .collect();

    if partial_matches.len() == 1 {
        Some((partial_matches[0].0, partial_matches[0].1.clone()))
    } else {
        None
    }
}

async fn accessible_libraries_for_context(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<rustfin_db::repo::libraries::LibraryRow>, String> {
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

    Ok(libraries
        .into_iter()
        .filter(|library| {
            allowed_library_ids
                .as_ref()
                .map(|allowed| allowed.contains(&library.id))
                .unwrap_or(true)
        })
        .collect())
}

fn normalize_library_duplicate_key(item: &rustfin_db::repo::items::ItemRow) -> String {
    let raw = item
        .sort_title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&item.title);
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn library_item_missing_metadata_fields(item: &rustfin_db::repo::items::ItemRow) -> Vec<String> {
    let mut missing = Vec::new();
    if item.year.is_none() {
        missing.push("year".to_string());
    }
    if item
        .overview
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("overview".to_string());
    }
    if !item_has_any_artwork(item) {
        missing.push("artwork".to_string());
    }
    missing
}

fn item_has_any_artwork(item: &rustfin_db::repo::items::ItemRow) -> bool {
    [
        item.poster_url.as_deref(),
        item.backdrop_url.as_deref(),
        item.logo_url.as_deref(),
        item.thumb_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .any(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{
        LinuxMountEntry, StoragePathSummary, nearest_existing_storage_path,
        select_linux_mount_entry, summarize_storage_mounts,
    };
    use super::{
        birthday_matches_query, birthday_month_day_display, build_follow_up_context,
        enforce_tool_policy, execute_tool_with_profile, next_birthday_occurrence,
        probe_service_health_component, resolve_birthday_query_for_context, storage_used_bytes,
        storage_used_percent, transcript_excerpt_indexes, transcript_highlights, transcript_terms,
    };
    use crate::ai_assistant::context::AssistantContext;
    use crate::ai_assistant::provider::ToolExecutionProfile;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{
        AssistantToolContextBlock, AssistantToolInput, AssistantToolSpec, PlannedToolCall,
        ToolAccessMode, ToolConfirmationPolicy, ToolRiskTier, ToolRoleRequirement,
    };
    use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
    use rustfin_db::repo::calendar::CalendarEventRow;
    use rustfin_db::repo::channel_transcripts::TranscriptEntryRow;
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::collections::HashMap;
    use std::sync::Arc;

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

    fn test_state() -> crate::state::AppState {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
            .expect("lazy postgres pool");
        let tc_config = rustfin_transcoder::TranscoderConfig {
            transcode_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-tools-test-{}", uuid::Uuid::new_v4())),
            max_concurrent: 1,
            ..Default::default()
        };
        let ffmpeg_path = tc_config.ffmpeg_path.clone();
        let ffprobe_path = tc_config.ffprobe_path.clone();
        let transcoder = Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
        let (events_tx, _) = tokio::sync::broadcast::channel(8);

        crate::state::AppState {
            db: pool,
            rustyvault: crate::state::RustyVaultRuntimeState::available(),
            jwt_secret: "test-secret".to_string(),
            http: reqwest::Client::builder().build().unwrap(),
            runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
            tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
            tmdb_agent_token: None,
            youtube_agent_url: "http://127.0.0.1:8101".to_string(),
            youtube_agent_token: None,
            transcription_agent_url: "http://127.0.0.1:8102".to_string(),
            transcription_agent_token: None,
            servers_agent_url: None,
            servers_agent_token: None,
            model_dir: Arc::new(tokio::sync::RwLock::new(
                std::env::temp_dir().join("rustyfin-ai-tools-models-test"),
            )),
            engine: Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
            transcoder,
            ffmpeg_path,
            ffprobe_path,
            transcoder_hw_accel: None,
            transcoder_hw_accel_required: false,
            cache_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-tools-cache-{}", uuid::Uuid::new_v4())),
            watch_party_audio_dir: std::env::temp_dir()
                .join(format!("rustyfin-ai-tools-audio-{}", uuid::Uuid::new_v4())),
            events: events_tx,
            watch_party: Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
            channel_manager: Arc::new(crate::channels::manager::ChannelManager::new()),
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

    #[tokio::test]
    async fn execute_tool_with_profile_denies_disallowed_tools() {
        let state = test_state();
        let context = assistant_context("user");
        let call = PlannedToolCall {
            tool: AssistantToolName::LibrarySearchTitles,
            input: AssistantToolInput::LibrarySearch {
                query: "Star Trek".to_string(),
            },
        };
        let profile =
            ToolExecutionProfile::restricted([AssistantToolName::LibrariesListAccessible], true, 1);

        let block = execute_tool_with_profile(&state, &context, &call, &profile).await;
        assert_eq!(block.status, "error");
        assert_eq!(block.tool, "library_search_titles");
        assert!(
            block.data["message"]
                .as_str()
                .unwrap_or_default()
                .contains("not available")
        );
    }

    #[tokio::test]
    async fn execute_tool_with_profile_preserves_confirmation_gates() {
        let state = test_state();
        let context = assistant_context("user");
        let call = PlannedToolCall {
            tool: AssistantToolName::CalendarCreateEvent,
            input: AssistantToolInput::CalendarCreateEvent {
                scope: "global".to_string(),
                title: "Launch".to_string(),
                description: Some("Ship it".to_string()),
                event_date: "2026-04-10".to_string(),
            },
        };
        let profile =
            ToolExecutionProfile::restricted([AssistantToolName::CalendarCreateEvent], false, 1);

        let block = execute_tool_with_profile(&state, &context, &call, &profile).await;
        assert_eq!(block.status, "error");
        assert!(
            block.data["message"]
                .as_str()
                .unwrap_or_default()
                .contains("requires explicit confirmation")
        );
    }

    #[test]
    fn build_follow_up_context_keeps_library_search_entities_stable() {
        let call = PlannedToolCall {
            tool: AssistantToolName::LibrarySearchTitles,
            input: AssistantToolInput::LibrarySearch {
                query: "Star Trek".to_string(),
            },
        };
        let block = AssistantToolContextBlock {
            tool: call.tool.as_str(),
            label: "Library matches for \"Star Trek\"".to_string(),
            status: "ok",
            data: json!({
                "match_count": 2,
                "matches": [
                    { "id": "item-1", "title": "Star Trek", "kind": "show" },
                    { "id": "item-2", "title": "Star Trek II", "kind": "movie" }
                ]
            }),
        };

        let context = build_follow_up_context(&call, &block);
        assert_eq!(context.tool, "library_search_titles");
        assert_eq!(context.entities.len(), 2);
        assert_eq!(context.entities[0].label, "Star Trek");
        assert_eq!(context.entities[1].identifier.as_deref(), Some("item-2"));
    }

    #[test]
    fn build_follow_up_context_prefers_resolved_weather_location() {
        let call = PlannedToolCall {
            tool: AssistantToolName::WeatherGetForecast,
            input: AssistantToolInput::Weather {
                location: "Campile Ireland".to_string(),
                forecast_days: Some(7),
            },
        };
        let block = AssistantToolContextBlock {
            tool: call.tool.as_str(),
            label: "7-day weather forecast for Campile, County Wexford, Leinster, Ireland"
                .to_string(),
            status: "ok",
            data: json!({
                "location_query": "Campile Ireland",
                "resolved_location": "Campile, County Wexford, Leinster, Ireland",
                "forecast_days": []
            }),
        };

        let context = build_follow_up_context(&call, &block);
        assert_eq!(
            context.input_hint.weather_location.as_deref(),
            Some("Campile, County Wexford, Leinster, Ireland")
        );
        assert_eq!(context.input_hint.weather_days, Some(7));
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
    fn birthday_query_maps_my_to_signed_in_username() {
        let context = assistant_context("user");
        assert_eq!(
            resolve_birthday_query_for_context(&context, Some("my".to_string())).as_deref(),
            Some("tester")
        );
        assert_eq!(
            resolve_birthday_query_for_context(&context, Some("Rachel".to_string())).as_deref(),
            Some("Rachel")
        );
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
    fn transcript_highlights_attach_citation_ids_and_windows() {
        let entries = vec![TranscriptEntryRow {
            id: "entry-1".to_string(),
            session_id: "session-1".to_string(),
            channel_id: "channel-1".to_string(),
            user_id: "user-1".to_string(),
            username: "Rachel".to_string(),
            started_ts_ms: 10_000,
            ended_ts_ms: 15_000,
            text: "The server is back online and the download is ready.".to_string(),
            created_ts: 20_000,
        }];
        let term_counts = HashMap::from([
            ("server".to_string(), 1_usize),
            ("download".to_string(), 1_usize),
        ]);

        let highlights = transcript_highlights(&entries, 5_000, &term_counts, 3);
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].entry_id, "entry-1");
        assert_eq!(highlights[0].citation_id, "transcript:session-1:entry-1");
        assert_eq!(highlights[0].relative_start, "00:05");
        assert_eq!(highlights[0].relative_end, "00:10");
        assert!(highlights[0].text.contains("server"));
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
