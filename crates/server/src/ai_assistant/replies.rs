use std::cmp::Ordering;
use std::collections::HashSet;

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::Value;

use super::types::{
    AssistantExecutionStopReason, AssistantExecutionTrace, AssistantGroundingChunk,
    AssistantGroundingCitation, AssistantToolContextBlock, AssistantToolOutcomeKind,
};
use super::web_sources::curated_web_category_label;

pub const MAX_GROUNDING_CHUNKS: usize = 10;
pub const MAX_GROUNDING_PROMPT_CHARS: usize = 5_500;

pub fn rank_and_compress_grounding_chunks(
    chunks: &[AssistantGroundingChunk],
    max_chunks: usize,
    max_chars: usize,
) -> Vec<AssistantGroundingChunk> {
    let mut ranked = chunks.to_vec();
    ranked.sort_by(compare_grounding_chunks);

    let mut seen = HashSet::<String>::new();
    let mut compacted = Vec::new();
    let mut chars_used = 0usize;

    for chunk in ranked {
        if !seen.insert(chunk.id.clone()) {
            continue;
        }

        let chunk_chars = grounding_chunk_prompt_line(&chunk).len();
        if !compacted.is_empty() && chars_used + chunk_chars > max_chars {
            break;
        }

        chars_used += chunk_chars;
        compacted.push(chunk);
        if compacted.len() >= max_chunks {
            break;
        }
    }

    compacted
}

pub fn grounding_chunks_prompt(chunks: &[AssistantGroundingChunk]) -> String {
    if chunks.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "Grounding chunks for this turn. Use these ranked stable IDs and short excerpts as evidence.\n",
    );
    for (index, chunk) in chunks.iter().enumerate() {
        let line = grounding_chunk_prompt_line(chunk);
        out.push_str(&format!("{:02}. {line}\n", index + 1));
    }
    out.trim_end().to_string()
}

pub fn grounding_chunk_prompt_line(chunk: &AssistantGroundingChunk) -> String {
    let visibility = match chunk.visibility {
        super::types::AssistantGroundingVisibility::User => "user",
        super::types::AssistantGroundingVisibility::Shared => "shared",
        super::types::AssistantGroundingVisibility::Admin => "admin",
    };
    let mut parts = vec![
        format!("[{}]", chunk.id),
        chunk.title.trim().to_string(),
        format!("kind={}", chunk.source_kind),
        format!("vis={visibility}"),
        format!("score={:.3}", chunk.score),
        format!("excerpt={}", compact_text(&chunk.excerpt, 260)),
    ];

    if let Some(topic_key) = chunk
        .topic_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("topic={topic_key}"));
    }
    if let Some(source_id) = chunk
        .source_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("source={source_id}"));
    }
    if let Some(source_sub_id) = chunk
        .source_sub_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("sub={source_sub_id}"));
    }
    if let Some(citation) = chunk.citation.as_ref() {
        parts.push(format!("cite={}", citation_brief(citation)));
    }

    parts.join(" | ")
}

pub fn citation_brief(citation: &AssistantGroundingCitation) -> String {
    let mut parts = vec![
        citation.citation_id.clone(),
        format!("{}:{}", citation.source_kind, citation.source_id),
    ];
    if let Some(sub_id) = citation
        .source_sub_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(sub_id.to_string());
    }
    if let (Some(started), Some(ended)) = (citation.started_ts_ms, citation.ended_ts_ms) {
        parts.push(format!("{started}-{ended}"));
    }
    if let Some(label) = citation
        .label
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(compact_text(label, 80));
    }
    parts.join("@")
}

fn compare_grounding_chunks(
    left: &AssistantGroundingChunk,
    right: &AssistantGroundingChunk,
) -> Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.visibility.cmp(&right.visibility))
        .then_with(|| left.source_kind.cmp(&right.source_kind))
        .then_with(|| left.topic_key.cmp(&right.topic_key))
        .then_with(|| left.title.cmp(&right.title))
        .then_with(|| left.id.cmp(&right.id))
}

pub fn compact_text(text: &str, max_chars: usize) -> String {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut out = String::new();
    for (index, ch) in normalized.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn format_binary_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{bytes} {}", UNITS[unit_index])
    } else {
        format!("{value:.1} {}", UNITS[unit_index])
    }
}

fn format_decimal(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract().abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

fn format_elapsed_seconds(value: u64) -> String {
    let days = value / 86_400;
    let hours = (value % 86_400) / 3_600;
    let minutes = (value % 3_600) / 60;
    let seconds = value % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 || !parts.is_empty() {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 || !parts.is_empty() {
        parts.push(format!("{minutes}m"));
    }
    parts.push(format!("{seconds}s"));
    parts.join(" ")
}

#[derive(Debug, Deserialize)]
struct GroundedNextEventEnvelope {
    next_event: Option<GroundedNextEventSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedNextEventSummary {
    title: String,
    event_type: String,
    scope: String,
    next_occurs_on: String,
}

#[derive(Debug, Deserialize)]
struct GroundedCalendarConflictEnvelope {
    window: GroundedCalendarWindow,
    total_event_count: usize,
    conflict_day_count: usize,
    conflict_days: Vec<GroundedCalendarConflictDay>,
}

#[derive(Debug, Deserialize)]
struct GroundedCalendarConflictDay {
    date: String,
    event_count: usize,
    events: Vec<GroundedCalendarEventOccurrence>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarEventOccurrence {
    title: String,
    event_date: String,
    occurs_on: String,
    scope: String,
    event_type: String,
    #[serde(default)]
    owner_username: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarFreeDaysEnvelope {
    window: GroundedCalendarWindow,
    occupied_day_count: usize,
    free_day_count: usize,
    free_days: Vec<GroundedCalendarFreeDay>,
}

#[derive(Debug, Deserialize)]
struct GroundedCalendarFreeDay {
    date: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarCountEnvelope {
    window: GroundedCalendarWindow,
    total_event_count: usize,
    busy_day_count: usize,
    #[serde(default)]
    busiest_day_count: Option<usize>,
    day_counts: Vec<GroundedCalendarDayCount>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarDayCount {
    date: String,
    event_count: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarBusyDaysEnvelope {
    window: GroundedCalendarWindow,
    total_event_count: usize,
    busy_day_count: usize,
    busy_days: Vec<GroundedCalendarBusyDay>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedCalendarBusyDay {
    date: String,
    event_count: usize,
    events: Vec<GroundedCalendarEventOccurrence>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedNextEventTimingEnvelope {
    today: String,
    #[serde(default)]
    days_until: Option<i64>,
    next_event: Option<GroundedNextEventSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedBirthdayEnvelope {
    window: GroundedCalendarWindow,
    query: Option<String>,
    birthdays: Vec<GroundedBirthdaySummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedCalendarWindow {
    label: String,
}

#[derive(Debug, Deserialize)]
struct GroundedBirthdaySummary {
    title: String,
    next_occurs_on: String,
    birthday_year: Option<i32>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryFactsEnvelope {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    topic_key: Option<String>,
    total_count: usize,
    facts: Vec<GroundedMemoryFactSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryFactSummary {
    id: String,
    memory_key: String,
    memory_type: String,
    #[serde(default)]
    topic_key: Option<String>,
    title: String,
    content: String,
    weight: f64,
    created_ts: i64,
    updated_ts: i64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntitiesEnvelope {
    #[serde(default)]
    query: Option<String>,
    total_count: usize,
    entities: Vec<GroundedMemoryEntitySummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntityRelationSummary {
    direction: String,
    relation: String,
    weight: f64,
    created_ts: i64,
    entity: GroundedMemoryEntitySummary,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntityRelationsEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    #[serde(default)]
    root: Option<GroundedMemoryEntitySummary>,
    relations: Vec<GroundedMemoryEntityRelationSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryPersonSummaryEnvelope {
    query: String,
    matched_by: String,
    person: GroundedMemoryEntitySummary,
    relation_count: usize,
    relations: Vec<GroundedMemoryEntityRelationSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntitySummary {
    id: String,
    node_key: String,
    entity_kind: String,
    label: String,
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default)]
    topic_key: Option<String>,
    #[serde(default)]
    source_chunk_id: Option<String>,
    access_scope: String,
    ordinal: i64,
    created_ts: i64,
    updated_ts: i64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryRecentChangesEnvelope {
    #[serde(default)]
    query: Option<String>,
    fact_count: usize,
    entity_count: usize,
    facts: Vec<GroundedMemoryFactSummary>,
    entities: Vec<GroundedMemoryEntitySummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryFactConflictSummary {
    #[serde(default)]
    topic_key: Option<String>,
    title: String,
    fact_count: usize,
    distinct_content_count: usize,
    facts: Vec<GroundedMemoryFactSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryConflictingFactsEnvelope {
    #[serde(default)]
    query: Option<String>,
    total_count: usize,
    conflict_group_count: usize,
    conflicts: Vec<GroundedMemoryFactConflictSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntityProvenanceSummary {
    id: String,
    node_key: String,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    entity_kind: String,
    label: String,
    #[serde(default)]
    identifier: Option<String>,
    #[serde(default)]
    topic_key: Option<String>,
    #[serde(default)]
    source_chunk_id: Option<String>,
    access_scope: String,
    ordinal: i64,
    created_ts: i64,
    updated_ts: i64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryProvenanceChunkSummary {
    chunk_key: String,
    source_kind: String,
    source_id: String,
    #[serde(default)]
    source_sub_id: Option<String>,
    #[serde(default)]
    owner_user_id: Option<String>,
    access_scope: String,
    #[serde(default)]
    access_key: Option<String>,
    #[serde(default)]
    topic_key: Option<String>,
    title: String,
    excerpt: String,
    source_ts: i64,
    updated_ts: i64,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedMemoryEntityProvenanceEnvelope {
    query: String,
    matched_by: String,
    #[serde(default)]
    entity: Option<GroundedMemoryEntityProvenanceSummary>,
    #[serde(default)]
    source_chunk: Option<GroundedMemoryProvenanceChunkSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkEnvelope {
    host_label: Option<String>,
    remote_access_enabled: bool,
    access: GroundedNetworkAccess,
    nodes: Vec<GroundedNetworkNode>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkInterfaceEnvelope {
    query: String,
    matched_by: String,
    host_label: Option<String>,
    remote_access_enabled: bool,
    access: GroundedNetworkAccess,
    interface: GroundedNetworkNode,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkDefaultRouteEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    routes: Vec<GroundedNetworkDefaultRouteSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkDefaultRouteSummary {
    route: String,
    #[serde(default)]
    gateway: Option<String>,
    #[serde(default)]
    interface: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    metric: Option<u32>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkHostnameAliasesEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    host_label: Option<String>,
    #[serde(default)]
    canonical_hostname: Option<String>,
    #[serde(default)]
    fqdn: Option<String>,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    aliases: Vec<GroundedNetworkHostnameAliasSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkHostnameAliasSummary {
    name: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkDnsServersEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    dns_servers: Vec<GroundedNetworkDnsServerSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkDnsServerSummary {
    scope: String,
    #[serde(default)]
    interface: Option<String>,
    server: String,
    source: String,
    raw_line: String,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkAccess {
    ui_port: u16,
    backend_port: u16,
    calendar_port: u16,
    preferred_local_interface: Option<String>,
    preferred_local_ipv4: Option<String>,
    preferred_local_url: Option<String>,
    login_url: Option<String>,
    ai_url: Option<String>,
    public_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkNode {
    name: String,
    status: String,
    addresses: Vec<GroundedNetworkAddress>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkAddress {
    family: String,
    address: String,
}

#[derive(Debug, Deserialize)]
struct GroundedDownloadArtifactSummary {
    id: String,
    artifact_id: String,
    title: String,
    summary: String,
    availability: String,
    detail: String,
    platform: String,
    architecture: String,
    version: Option<String>,
    channel: String,
    package_filename: Option<String>,
    file_size: Option<i64>,
    checksum: Option<String>,
    signature_status: String,
    distribution_mode: String,
    external_url: Option<String>,
    download_path: Option<String>,
    install_mode: Option<String>,
    setup_path: Option<String>,
    #[serde(default)]
    requires_sign_in: bool,
    #[serde(default)]
    install_steps: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDownloadListEnvelope {
    #[serde(default)]
    total_count: usize,
    query: Option<String>,
    availability_filter: Option<String>,
    artifacts: Vec<GroundedDownloadArtifactSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedDownloadDetailEnvelope {
    query: Option<String>,
    matched_by: Option<String>,
    #[serde(flatten)]
    artifact: GroundedDownloadArtifactSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryAccountIdentityEnvelope {
    linked: bool,
    #[serde(default)]
    person_id: Option<String>,
    #[serde(default)]
    person_name: Option<String>,
    #[serde(default)]
    family_workspace_id: Option<String>,
    #[serde(default)]
    friends_workspace_id: Option<String>,
    #[serde(default)]
    work_workspace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryWorkspaceSummary {
    workspace_id: String,
    title: String,
    workspace_kind: String,
    #[serde(default)]
    owner_user_id: Option<String>,
    is_system_seeded: bool,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryVisibleWorkspacesEnvelope {
    #[serde(default)]
    workspaces: Vec<GroundedDictionaryWorkspaceSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryPersonSummary {
    id: String,
    display_name: String,
    canonical_name: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryFactSummary {
    fact_key: String,
    value_type: String,
    #[serde(default)]
    value_text: Option<String>,
    #[serde(default)]
    value_int: Option<i64>,
    #[serde(default)]
    value_bool: Option<bool>,
    #[serde(default)]
    value_date: Option<String>,
    #[serde(default)]
    value_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryRelationSummary {
    relation_id: String,
    relation_group_key: String,
    relation_type: String,
    direction: String,
    other_person_id: String,
    other_person_name: String,
    #[serde(default)]
    other_person_summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryPersonBundleEnvelope {
    workspace_id: String,
    person: GroundedDictionaryPersonSummary,
    #[serde(default)]
    facts: Vec<GroundedDictionaryFactSummary>,
    #[serde(default)]
    relations: Vec<GroundedDictionaryRelationSummary>,
    #[serde(default)]
    document_title: Option<String>,
    #[serde(default)]
    document_excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryWorkspacePeopleEnvelope {
    workspace_id: String,
    workspace_title: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    people: Vec<GroundedDictionaryPersonSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryResolvedCandidate {
    person_id: String,
    display_name: String,
    #[serde(default)]
    summary: Option<String>,
    relation_type: String,
    #[serde(default)]
    birthday: Option<String>,
    #[serde(default)]
    hobbies: Vec<String>,
    #[serde(default)]
    document_excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedDictionaryRelationshipResolutionEnvelope {
    reference: String,
    relation_kind: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    workspace_title: Option<String>,
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    linked_person_id: Option<String>,
    #[serde(default)]
    linked_person_name: Option<String>,
    #[serde(default)]
    candidates: Vec<GroundedDictionaryResolvedCandidate>,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryDuplicateTitlesEnvelope {
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    duplicate_group_count: usize,
    #[serde(default)]
    duplicates: Vec<GroundedLibraryDuplicateTitleSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryDuplicateTitleSummary {
    title: String,
    item_count: usize,
    library_count: usize,
    libraries: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedLibraryMissingMetadataEnvelope {
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    missing_item_count: usize,
    #[serde(default)]
    items: Vec<GroundedLibraryMissingMetadataItemSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedLibraryMissingMetadataItemSummary {
    library_id: String,
    #[serde(default)]
    library_name: Option<String>,
    id: String,
    title: String,
    kind: String,
    #[serde(default)]
    year: Option<i64>,
    missing_fields: Vec<String>,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Deserialize)]
struct GroundedServiceHealthEnvelope {
    all_healthy: bool,
    components: Vec<GroundedServiceComponent>,
}

#[derive(Debug, Deserialize)]
struct GroundedServiceComponent {
    name: String,
    status: String,
    configured: bool,
    url: Option<String>,
    detail: String,
}

#[derive(Debug, Deserialize)]
struct GroundedServiceDetailEnvelope {
    query: String,
    matched_by: String,
    component: GroundedServiceComponent,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemPortConflictsEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    conflicts: Vec<GroundedSystemPortConflictSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemPortConflictSummary {
    protocol: String,
    state: String,
    local_address: String,
    #[serde(default)]
    local_port: Option<u16>,
    #[serde(default)]
    peer_address: Option<String>,
    raw_entry: String,
    #[serde(default)]
    processes: Vec<GroundedSystemPortConflictProcessSummary>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedSystemPortConflictDetailEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    total_count: usize,
    #[serde(flatten)]
    conflict: GroundedSystemPortConflictSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemPortConflictProcessSummary {
    name: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    fd: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemFailedUnitsEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    units: Vec<GroundedSystemFailedUnitSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemFailedUnitDetailEnvelope {
    #[serde(default)]
    query: Option<String>,
    matched_by: String,
    detail: GroundedSystemFailedUnitDetailSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemFailedUnitDetailSummary {
    unit: GroundedSystemFailedUnitSummary,
    status: GroundedSystemFailedUnitDetailStatusSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemFailedUnitDetailStatusSummary {
    #[serde(default)]
    fragment_path: Option<String>,
    #[serde(default)]
    unit_file_state: Option<String>,
    #[serde(default)]
    main_pid: Option<u32>,
    #[serde(default)]
    exec_main_code: Option<String>,
    #[serde(default)]
    exec_main_status: Option<String>,
    #[serde(default)]
    status_excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemFailedUnitSummary {
    name: String,
    load: String,
    active: String,
    sub: String,
    description: String,
    #[serde(default)]
    recent_log_excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemProcessDetailEnvelope {
    #[serde(default)]
    available: bool,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    matched_by: Option<String>,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    processes: Vec<GroundedSystemProcessSummary>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemProcessSummary {
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    ppid: Option<u32>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    cpu_percent: Option<f64>,
    #[serde(default)]
    mem_percent: Option<f64>,
    #[serde(default)]
    elapsed_secs: Option<u64>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<String>,
    #[serde(default)]
    raw_line: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemListenerDetailEnvelope {
    #[serde(default)]
    available: bool,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    matched_by: Option<String>,
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    listeners: Vec<GroundedSystemListenerSummary>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemListenerSummary {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    recv_q: Option<String>,
    #[serde(default)]
    send_q: Option<String>,
    #[serde(default)]
    local_address: Option<String>,
    #[serde(default)]
    local_port: Option<u16>,
    #[serde(default)]
    peer_address: Option<String>,
    #[serde(default)]
    process: Option<String>,
    #[serde(default)]
    raw_line: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedSystemDiskUsageDetailEnvelope {
    #[serde(default)]
    available: bool,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    matched_by: Option<String>,
    #[serde(default)]
    mount_point: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    fs_type: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    mount_id: Option<u64>,
    #[serde(default)]
    parent_id: Option<u64>,
    #[serde(default)]
    major_minor: Option<String>,
    #[serde(default)]
    options: Option<String>,
    #[serde(default)]
    super_options: Option<String>,
    #[serde(default)]
    total_bytes: Option<u64>,
    #[serde(default)]
    free_bytes: Option<u64>,
    #[serde(default)]
    available_bytes: Option<u64>,
    #[serde(default)]
    used_bytes: Option<u64>,
    #[serde(default)]
    used_percent: Option<f64>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedStoragePathDetailEnvelope {
    query: String,
    matched_by: String,
    #[serde(flatten)]
    path: GroundedStoragePathSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedStoragePathSummary {
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedStorageMountSummary {
    mount_point: String,
    #[serde(default)]
    mount_file_system: Option<String>,
    #[serde(default)]
    mount_source: Option<String>,
    #[serde(default)]
    tracked_paths: Vec<String>,
    #[serde(default)]
    total_bytes: Option<u64>,
    #[serde(default)]
    total_human: Option<String>,
    #[serde(default)]
    available_bytes: Option<u64>,
    #[serde(default)]
    available_human: Option<String>,
    #[serde(default)]
    used_bytes: Option<u64>,
    #[serde(default)]
    used_human: Option<String>,
    #[serde(default)]
    used_percent: Option<f64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GroundedStorageMountDetailEnvelope {
    query: String,
    matched_by: String,
    total_count: usize,
    #[serde(flatten)]
    mount: GroundedStorageMountSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedLibrarySearchEnvelope {
    #[serde(default)]
    match_count: usize,
    #[serde(default)]
    matches: Vec<GroundedLibrarySearchMatch>,
}

#[derive(Debug, Deserialize)]
struct GroundedLibrarySearchMatch {
    title: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    library_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryPathSummary {
    id: String,
    path: String,
    is_read_only: bool,
}

#[derive(Debug, Deserialize)]
struct GroundedLibrarySettingsSummary {
    show_images: bool,
    prefer_local_artwork: bool,
    fetch_online_artwork: bool,
    tmdb_store_in_media_dir: bool,
    tmdb_sync_on_new_media: bool,
    tmdb_sync_schedule: String,
    #[serde(default)]
    tmdb_last_sync_ts: Option<i64>,
    tmdb_fetch_posters: bool,
    tmdb_fetch_backdrops: bool,
    tmdb_fetch_metadata: bool,
    tmdb_fetch_reviews: bool,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryItemDetailEnvelope {
    library_id: String,
    id: String,
    title: String,
    kind: String,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    library_name: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryDetailEnvelope {
    query: Option<String>,
    matched_by: String,
    id: String,
    name: String,
    kind: String,
    item_count: i64,
    paths: Vec<GroundedLibraryPathSummary>,
    settings: GroundedLibrarySettingsSummary,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Deserialize)]
struct GroundedLibraryItemMediaEnvelope {
    query: String,
    matched_by: String,
    library_id: String,
    id: String,
    title: String,
    kind: String,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    library_name: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    duration_ms: Option<i64>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    media_path: Option<String>,
    #[serde(default)]
    resolved_media_path: Option<String>,
    #[serde(default)]
    first_descendant_media_path: Option<String>,
    #[serde(default)]
    source_paths: Vec<String>,
    #[serde(default)]
    poster_url: Option<String>,
    #[serde(default)]
    backdrop_url: Option<String>,
    #[serde(default)]
    logo_url: Option<String>,
    #[serde(default)]
    thumb_url: Option<String>,
    created_ts: i64,
    updated_ts: i64,
}

#[derive(Debug, Deserialize)]
struct GroundedAiRuntimeEnvelope {
    model: GroundedAiRuntimeModel,
    scheduler: GroundedAiRuntimeScheduler,
    #[serde(default)]
    gpus: Vec<GroundedAiRuntimeGpu>,
    #[serde(default)]
    role_routing: Vec<GroundedAiRuntimeRoleRoute>,
}

#[derive(Debug, Deserialize)]
struct GroundedAiRuntimeModel {
    name: Option<String>,
    backend: String,
    loaded: bool,
    context_length: u32,
    n_threads: u32,
    split_mode: String,
    #[serde(default)]
    device_indices: Vec<usize>,
}

#[derive(Debug, Deserialize)]
struct GroundedAiRuntimeScheduler {
    overload_state: String,
    active_turns: u64,
    queued_turns: u64,
    warm_pool_bytes: u64,
    warm_pool_budget_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct GroundedAiRuntimeGpu {
    #[serde(default)]
    index: Option<u32>,
    name: String,
    #[serde(default)]
    utilization_percent: Option<f64>,
    #[serde(default)]
    vram_used_bytes: Option<u64>,
    #[serde(default)]
    vram_total_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GroundedAiRuntimeRoleRoute {
    role: String,
    model_name: String,
    backend_kind: String,
}

pub fn deterministic_ai_runtime_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    if grounding_blocks.len() != 1 {
        return None;
    }
    let block = grounding_blocks.first()?;
    if block.tool != "system_get_ai_runtime_summary" {
        return None;
    }
    if block.status != "ok" {
        return Some("I couldn't load the current Rustyfin AI runtime details.".to_string());
    }

    let runtime = serde_json::from_value::<GroundedAiRuntimeEnvelope>(block.data.clone()).ok()?;
    let lower = message.to_ascii_lowercase();
    let mentions_gpu_memory = [
        "vram",
        "video memory",
        "gpu memory",
        "graphics memory",
        "gpu ram",
        "cuda memory",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    if mentions_gpu_memory {
        if runtime.gpus.is_empty() {
            return Some("I couldn't find any live GPU VRAM metrics right now.".to_string());
        }

        let mut lines = vec!["Current GPU VRAM usage:".to_string()];
        for gpu in &runtime.gpus {
            let label = match gpu.index {
                Some(index) => format!("GPU {index} ({})", gpu.name),
                None => gpu.name.clone(),
            };
            match (gpu.vram_used_bytes, gpu.vram_total_bytes) {
                (Some(used), Some(total)) if total > 0 => {
                    let percent = (used as f64 / total as f64) * 100.0;
                    lines.push(format!(
                        "- {label} is using {} of {} ({percent:.1}%).",
                        format_binary_bytes(used),
                        format_binary_bytes(total)
                    ));
                }
                (Some(used), _) => {
                    lines.push(format!(
                        "- {label} is using {}, but total VRAM was not reported.",
                        format_binary_bytes(used)
                    ));
                }
                _ => {
                    let utilization_suffix = gpu
                        .utilization_percent
                        .map(|util| format!(" GPU utilization is {util:.1}%."))
                        .unwrap_or_default();
                    lines.push(format!(
                        "- {label} did not report live VRAM usage.{utilization_suffix}"
                    ));
                }
            }
        }
        return Some(lines.join("\n"));
    }

    let answer_route = runtime
        .role_routing
        .iter()
        .find(|route| route.role.eq_ignore_ascii_case("answer"));
    let planner_route = runtime
        .role_routing
        .iter()
        .find(|route| route.role.eq_ignore_ascii_case("planner"));
    let effective_model = runtime
        .model
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| answer_route.map(|route| route.model_name.as_str()));

    let mut lines = Vec::new();
    if runtime.model.loaded {
        if let Some(model_name) = effective_model {
            lines.push(format!(
                "The currently loaded AI model is `{model_name}` on the {} backend.",
                runtime.model.backend
            ));
        } else {
            lines.push(format!(
                "Rustyfin AI is currently loaded on the {} backend, but the model name was not available.",
                runtime.model.backend
            ));
        }
    } else if let Some(model_name) = effective_model {
        lines.push(format!(
            "Rustyfin AI is configured for `{model_name}` on the {} backend, but no model is currently loaded.",
            runtime.model.backend
        ));
    } else {
        lines.push("No AI model is currently loaded.".to_string());
    }

    let lower = message.to_ascii_lowercase();
    let mentions_gpu_memory = [
        "vram",
        "video memory",
        "gpu memory",
        "graphics memory",
        "gpu ram",
        "cuda memory",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if mentions_gpu_memory {
        if runtime.gpus.is_empty() {
            lines.push(
                "I couldn't find any live GPU VRAM metrics in the current Rustyfin AI runtime summary."
                    .to_string(),
            );
        } else {
            lines.push("Current GPU VRAM usage:".to_string());
            for gpu in &runtime.gpus {
                let gpu_label = gpu
                    .index
                    .map(|index| format!("GPU {index}"))
                    .unwrap_or_else(|| gpu.name.clone());
                match (gpu.vram_used_bytes, gpu.vram_total_bytes) {
                    (Some(used), Some(total)) if total > 0 => {
                        let percent = ((used as f64 / total as f64) * 100.0 * 10.0).round() / 10.0;
                        lines.push(format!(
                            "- {gpu_label} ({}) is using {} of {} ({percent:.1}%).",
                            gpu.name,
                            format_binary_bytes(used),
                            format_binary_bytes(total),
                        ));
                    }
                    (Some(used), _) => {
                        lines.push(format!(
                            "- {gpu_label} ({}) is using {} of VRAM. The total VRAM was not available.",
                            gpu.name,
                            format_binary_bytes(used),
                        ));
                    }
                    _ => {
                        lines.push(format!(
                            "- {gpu_label} ({}) did not report live VRAM usage.",
                            gpu.name
                        ));
                    }
                }
            }
        }
    }
    let mentions_backend = [
        "backend", "local", "remote", "gpu", "gpus", "device", "devices", "threads", "context",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if mentions_backend {
        let device_summary = if runtime.model.device_indices.is_empty() {
            "CPU only".to_string()
        } else {
            format!(
                "devices {}",
                runtime
                    .model
                    .device_indices
                    .iter()
                    .map(|index| index.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        lines.push(format!(
            "Runtime settings: context {} tokens, {} threads, {} split, {device_summary}.",
            runtime.model.context_length, runtime.model.n_threads, runtime.model.split_mode
        ));
    }

    let mentions_roles = [
        "planner",
        "summarizer",
        "answer",
        "verifier",
        "worker",
        "role",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if mentions_roles
        || planner_route.is_some_and(|route| {
            answer_route.is_some_and(|answer| {
                route.model_name != answer.model_name || route.backend_kind != answer.backend_kind
            })
        })
    {
        let role_summary = runtime
            .role_routing
            .iter()
            .map(|route| {
                format!(
                    "{} -> `{}` ({})",
                    route.role, route.model_name, route.backend_kind
                )
            })
            .collect::<Vec<_>>();
        if !role_summary.is_empty() {
            lines.push(format!("Role routing: {}.", role_summary.join(", ")));
        }
    }

    let mentions_scheduler = [
        "queue",
        "queued",
        "scheduler",
        "warm pool",
        "overload",
        "hot model",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if mentions_scheduler {
        lines.push(format!(
            "Scheduler is {} with {} active and {} queued. Warm pool is {} of {}.",
            runtime.scheduler.overload_state.to_ascii_lowercase(),
            runtime.scheduler.active_turns,
            runtime.scheduler.queued_turns,
            format_binary_bytes(runtime.scheduler.warm_pool_bytes),
            format_binary_bytes(runtime.scheduler.warm_pool_budget_bytes),
        ));
    }

    Some(lines.join("\n"))
}

pub fn deterministic_calendar_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    if grounding_blocks.len() != 1 {
        return None;
    }
    let block = grounding_blocks.first()?;

    match block.tool {
        "calendar_get_next_event" => Some(if block.status == "ok" {
            format_next_event_reply(
                serde_json::from_value::<GroundedNextEventEnvelope>(block.data.clone()).ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load your next visible calendar event.")
        }),
        "calendar_get_next_event_timing" => Some(if block.status == "ok" {
            format_next_event_timing_reply(
                serde_json::from_value::<GroundedNextEventTimingEnvelope>(block.data.clone())
                    .ok()?,
            )
        } else {
            format_calendar_error(
                block,
                "I couldn't load the timing for your next visible calendar event.",
            )
        }),
        "calendar_list_date_conflicts" => Some(if block.status == "ok" {
            format_calendar_conflicts_reply(
                serde_json::from_value::<GroundedCalendarConflictEnvelope>(block.data.clone())
                    .ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load the visible calendar conflicts.")
        }),
        "calendar_list_free_days" => Some(if block.status == "ok" {
            format_calendar_free_days_reply(
                serde_json::from_value::<GroundedCalendarFreeDaysEnvelope>(block.data.clone())
                    .ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load the visible calendar free days.")
        }),
        "calendar_count_events" => Some(if block.status == "ok" {
            format_calendar_count_reply(
                serde_json::from_value::<GroundedCalendarCountEnvelope>(block.data.clone()).ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't count the visible calendar events.")
        }),
        "calendar_list_busy_days" => Some(if block.status == "ok" {
            format_calendar_busy_days_reply(
                serde_json::from_value::<GroundedCalendarBusyDaysEnvelope>(block.data.clone())
                    .ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load the visible calendar busy days.")
        }),
        "calendar_upcoming_birthdays" => Some(if block.status == "ok" {
            format_birthdays_reply(
                message,
                serde_json::from_value::<GroundedBirthdayEnvelope>(block.data.clone()).ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load the upcoming birthdays.")
        }),
        _ => None,
    }
}

pub fn deterministic_network_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "network_get_default_route")
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "network_get_hostname_aliases")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "network_get_dns_servers")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "network_get_interface_by_ip")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "network_get_interface_details")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "network_get_topology_summary")
        })?;

    Some(if block.status == "ok" {
        match block.tool {
            "network_get_default_route" => format_network_default_route_reply(
                message,
                serde_json::from_value::<GroundedNetworkDefaultRouteEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "network_get_hostname_aliases" => format_network_hostname_aliases_reply(
                serde_json::from_value::<GroundedNetworkHostnameAliasesEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "network_get_dns_servers" => format_network_dns_servers_reply(
                serde_json::from_value::<GroundedNetworkDnsServersEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "network_get_interface_by_ip" => format_network_interface_reply(
                serde_json::from_value::<GroundedNetworkInterfaceEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "network_get_topology_summary" => format_network_reply(
                message,
                serde_json::from_value::<GroundedNetworkEnvelope>(block.data.clone()).ok()?,
            ),
            "network_get_interface_details" => format_network_interface_reply(
                serde_json::from_value::<GroundedNetworkInterfaceEnvelope>(block.data.clone())
                    .ok()?,
            ),
            _ => return None,
        }
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the Rustyfin network details. {message}"))
            .unwrap_or_else(|| "I couldn't load the Rustyfin network details.".to_string())
    })
}

pub fn deterministic_system_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "system_get_process_detail")
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_listener_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_disk_usage_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_port_conflict_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_port_conflicts")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_failed_unit_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_failed_units")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_service_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_mount_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_storage_path_detail")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "system_get_service_health")
        })?;

    Some(if block.status == "ok" {
        match block.tool {
            "system_get_process_detail" => format_system_process_detail_reply(
                message,
                serde_json::from_value::<GroundedSystemProcessDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_listener_detail" => format_system_listener_detail_reply(
                message,
                serde_json::from_value::<GroundedSystemListenerDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_disk_usage_detail" => format_system_disk_usage_detail_reply(
                message,
                serde_json::from_value::<GroundedSystemDiskUsageDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_port_conflict_detail" => format_system_port_conflict_detail_reply(
                message,
                serde_json::from_value::<GroundedSystemPortConflictDetailEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "system_get_port_conflicts" => format_system_port_conflicts_reply(
                serde_json::from_value::<GroundedSystemPortConflictsEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_failed_unit_detail" => format_system_failed_unit_detail_reply(
                serde_json::from_value::<GroundedSystemFailedUnitDetailEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "system_get_failed_units" => format_system_failed_units_reply(
                serde_json::from_value::<GroundedSystemFailedUnitsEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_storage_path_detail" => format_storage_path_detail_reply(
                message,
                serde_json::from_value::<GroundedStoragePathDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "system_get_service_health" => format_service_health_reply(
                serde_json::from_value::<GroundedServiceHealthEnvelope>(block.data.clone()).ok()?,
            ),
            "system_get_service_detail" => format_service_detail_reply(
                serde_json::from_value::<GroundedServiceDetailEnvelope>(block.data.clone()).ok()?,
            ),
            "system_get_mount_detail" => format_storage_mount_detail_reply(
                message,
                serde_json::from_value::<GroundedStorageMountDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the system details.")
    })
}

pub fn deterministic_memory_reply(
    _message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks.iter().find(|block| {
        matches!(
            block.tool,
            "memory_list_recent_facts"
                | "memory_list_recent_entities"
                | "memory_search_facts"
                | "memory_search_entities"
                | "memory_get_entity_relations"
                | "memory_get_person_summary"
                | "memory_list_recent_changes"
                | "memory_list_conflicting_facts"
                | "memory_get_entity_provenance"
        )
    })?;

    Some(if block.status == "ok" {
        match block.tool {
            "memory_list_recent_facts" | "memory_search_facts" => format_memory_facts_reply(
                serde_json::from_value::<GroundedMemoryFactsEnvelope>(block.data.clone()).ok()?,
            ),
            "memory_list_recent_entities" | "memory_search_entities" => {
                format_memory_entities_reply(
                    serde_json::from_value::<GroundedMemoryEntitiesEnvelope>(block.data.clone())
                        .ok()?,
                )
            }
            "memory_get_entity_relations" => format_memory_entity_relations_reply(
                serde_json::from_value::<GroundedMemoryEntityRelationsEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "memory_get_person_summary" => format_memory_person_summary_reply(
                serde_json::from_value::<GroundedMemoryPersonSummaryEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "memory_list_recent_changes" => format_memory_recent_changes_reply(
                serde_json::from_value::<GroundedMemoryRecentChangesEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "memory_list_conflicting_facts" => format_memory_conflicting_facts_reply(
                serde_json::from_value::<GroundedMemoryConflictingFactsEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "memory_get_entity_provenance" => format_memory_entity_provenance_reply(
                serde_json::from_value::<GroundedMemoryEntityProvenanceEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the stored memory details.")
    })
}

pub fn deterministic_dictionary_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks.iter().find(|block| {
        matches!(
            block.tool,
            "dictionary_get_account_identity"
                | "dictionary_list_visible_workspaces"
                | "dictionary_browse_workspace_people"
                | "dictionary_search_people"
                | "dictionary_get_person_bundle"
                | "dictionary_resolve_relationship_reference"
        )
    })?;

    Some(if block.status == "ok" {
        match block.tool {
            "dictionary_get_account_identity" => format_dictionary_account_identity_reply(
                serde_json::from_value::<GroundedDictionaryAccountIdentityEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "dictionary_list_visible_workspaces" => format_dictionary_workspaces_reply(
                serde_json::from_value::<GroundedDictionaryVisibleWorkspacesEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "dictionary_browse_workspace_people" => format_dictionary_workspace_people_reply(
                serde_json::from_value::<GroundedDictionaryWorkspacePeopleEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "dictionary_search_people" => format_dictionary_search_reply(
                serde_json::from_value::<Vec<GroundedDictionaryPersonSummary>>(
                    block
                        .data
                        .get("people")
                        .cloned()
                        .unwrap_or_else(|| Value::Array(Vec::new())),
                )
                .ok()?,
                block.data.get("query").and_then(Value::as_str),
            ),
            "dictionary_get_person_bundle" => format_dictionary_person_bundle_reply(
                message,
                serde_json::from_value::<GroundedDictionaryPersonBundleEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "dictionary_resolve_relationship_reference" => format_dictionary_relationship_reply(
                message,
                serde_json::from_value::<GroundedDictionaryRelationshipResolutionEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the Human Dictionary details.")
    })
}

fn format_dictionary_account_identity_reply(
    envelope: GroundedDictionaryAccountIdentityEnvelope,
) -> String {
    if !envelope.linked {
        return "Your Rustyfin account is not linked to a Human Dictionary person yet.".to_string();
    }

    let name = envelope
        .person_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("an unnamed person");
    let mut scopes = Vec::new();
    if envelope.family_workspace_id.is_some() {
        scopes.push("family");
    }
    if envelope.friends_workspace_id.is_some() {
        scopes.push("friends");
    }
    if envelope.work_workspace_id.is_some() {
        scopes.push("work");
    }

    if scopes.is_empty() {
        format!("Your Rustyfin account is linked to {name} in the Human Dictionary.")
    } else {
        format!(
            "Your Rustyfin account is linked to {name} in the Human Dictionary, with {} workspace link{} available.",
            scopes.join(", "),
            if scopes.len() == 1 { "" } else { "s" }
        )
    }
}

fn format_dictionary_workspaces_reply(
    envelope: GroundedDictionaryVisibleWorkspacesEnvelope,
) -> String {
    if envelope.workspaces.is_empty() {
        return "I couldn't find any visible Human Dictionary workspaces.".to_string();
    }

    let mut lines = vec!["Visible Human Dictionary workspaces:".to_string()];
    for workspace in envelope.workspaces.iter().take(8) {
        let mut line = format!(
            "- {} ({})",
            workspace.title,
            dictionary_workspace_kind_label(&workspace.workspace_kind)
        );
        if workspace.is_system_seeded {
            line.push_str(" [seeded]");
        }
        lines.push(line);
    }
    if envelope.workspaces.len() > 8 {
        lines.push(format!("... and {} more.", envelope.workspaces.len() - 8));
    }
    lines.join("\n")
}

fn format_dictionary_workspace_people_reply(
    envelope: GroundedDictionaryWorkspacePeopleEnvelope,
) -> String {
    if envelope.people.is_empty() {
        return envelope
            .query
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                format!(
                    "I couldn't find any visible Human Dictionary people in {} matching \"{}\".",
                    envelope.workspace_title, value
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "I couldn't find any visible Human Dictionary people in {}.",
                    envelope.workspace_title
                )
            });
    }

    let mut lines = vec![
        envelope
            .query
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                format!(
                    "Visible Human Dictionary people in {} matching \"{}\":",
                    envelope.workspace_title, value
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "Visible Human Dictionary people in {}:",
                    envelope.workspace_title
                )
            }),
    ];
    for person in envelope.people.iter().take(8) {
        let mut line = format!("- {}", person.display_name);
        if person.canonical_name != person.display_name {
            line.push_str(&format!(" ({})", person.canonical_name));
        }
        if let Some(summary) = person
            .summary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(": {}", compact_text(summary, 120)));
        }
        lines.push(line);
    }
    if envelope.people.len() > 8 {
        lines.push(format!("... and {} more.", envelope.people.len() - 8));
    }
    lines.join("\n")
}

fn format_dictionary_search_reply(
    people: Vec<GroundedDictionaryPersonSummary>,
    query: Option<&str>,
) -> String {
    if people.is_empty() {
        return query
            .map(|value| {
                format!("I couldn't find any visible Human Dictionary people matching \"{value}\".")
            })
            .unwrap_or_else(|| {
                "I couldn't find any visible Human Dictionary people there.".to_string()
            });
    }

    let mut lines = vec![
        query
            .map(|value| format!("Visible Human Dictionary people matching \"{value}\":"))
            .unwrap_or_else(|| "Visible Human Dictionary people:".to_string()),
    ];
    for person in people.iter().take(8) {
        let mut line = format!("- {}", person.display_name);
        if person.canonical_name != person.display_name {
            line.push_str(&format!(" ({})", person.canonical_name));
        }
        if let Some(summary) = person
            .summary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(": {}", compact_text(summary, 120)));
        }
        lines.push(line);
    }
    if people.len() > 8 {
        lines.push(format!("... and {} more.", people.len() - 8));
    }
    lines.join("\n")
}

fn format_dictionary_person_bundle_reply(
    message: &str,
    envelope: GroundedDictionaryPersonBundleEnvelope,
) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("birthday") || lower.contains("born") {
        if let Some(birthday) = envelope
            .facts
            .iter()
            .find(|fact| fact.fact_key == "birthday")
            .and_then(dictionary_fact_birthday)
        {
            return format!(
                "{} has a birthday recorded for {}.",
                envelope.person.display_name,
                format_dictionary_date(&birthday)
            );
        }
        return format!(
            "I don't have a birthday recorded for {}.",
            envelope.person.display_name
        );
    }

    if lower.contains("hobbies") || lower.contains("hobby") {
        let hobbies = envelope
            .facts
            .iter()
            .find(|fact| fact.fact_key == "hobbies")
            .map(dictionary_fact_hobbies)
            .unwrap_or_default();
        if hobbies.is_empty() {
            return format!(
                "I don't have any hobbies recorded for {}.",
                envelope.person.display_name
            );
        }
        return format!(
            "{} has these hobbies recorded: {}.",
            envelope.person.display_name,
            hobbies.join(", ")
        );
    }

    let mut lines = vec![format!(
        "Human Dictionary summary for {}.",
        envelope.person.display_name
    )];
    if let Some(summary) = envelope
        .person
        .summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(compact_text(summary, 180));
    }
    if let Some(birthday) = envelope
        .facts
        .iter()
        .find(|fact| fact.fact_key == "birthday")
        .and_then(dictionary_fact_birthday)
    {
        lines.push(format!("Birthday: {}.", format_dictionary_date(&birthday)));
    }
    let hobbies = envelope
        .facts
        .iter()
        .find(|fact| fact.fact_key == "hobbies")
        .map(dictionary_fact_hobbies)
        .unwrap_or_default();
    if !hobbies.is_empty() {
        lines.push(format!("Hobbies: {}.", hobbies.join(", ")));
    }
    if !envelope.relations.is_empty() {
        let relations = envelope
            .relations
            .iter()
            .take(4)
            .map(|relation| {
                format!(
                    "{} ({})",
                    relation.other_person_name, relation.relation_type
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!("Related people: {}.", relations.join(", ")));
    }
    if let Some(excerpt) = envelope
        .document_excerpt
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Notes: {}", compact_text(excerpt, 180)));
    }
    lines.join("\n")
}

fn format_dictionary_relationship_reply(
    message: &str,
    envelope: GroundedDictionaryRelationshipResolutionEnvelope,
) -> String {
    if let Some(message) = envelope
        .message
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return message.to_string();
    }
    if envelope.candidates.is_empty() {
        return format!(
            "I couldn't find a visible Human Dictionary match for {}.",
            envelope.reference
        );
    }

    let lower = message.to_ascii_lowercase();
    if (lower.contains("birthday") || lower.contains("born")) && envelope.candidates.len() == 1 {
        let candidate = &envelope.candidates[0];
        let reference_label = dictionary_reference_label(&envelope.reference);
        return match candidate.birthday.as_deref() {
            Some(birthday) if !birthday.trim().is_empty() => format!(
                "{} {} has a birthday recorded for {}.",
                capitalize_dictionary_reference(&reference_label),
                candidate.display_name,
                format_dictionary_date(birthday)
            ),
            _ => format!(
                "I don't have a birthday recorded for {} {}.",
                reference_label, candidate.display_name
            ),
        };
    }

    if (lower.contains("hobbies") || lower.contains("hobby")) && envelope.candidates.len() == 1 {
        let candidate = &envelope.candidates[0];
        let reference_label = dictionary_reference_label(&envelope.reference);
        if candidate.hobbies.is_empty() {
            return format!(
                "I don't have any hobbies recorded for {} {}.",
                reference_label, candidate.display_name
            );
        }
        return format!(
            "{} {} has these hobbies recorded: {}.",
            capitalize_dictionary_reference(&reference_label),
            candidate.display_name,
            candidate.hobbies.join(", ")
        );
    }

    let workspace_suffix = envelope
        .workspace_title
        .as_deref()
        .map(|title| format!(" in {}", title))
        .unwrap_or_default();
    let reference_label = dictionary_reference_label(&envelope.reference);

    if envelope.candidates.len() == 1 {
        let candidate = &envelope.candidates[0];
        let mut lines = vec![format!(
            "{}{} is {}.",
            capitalize_dictionary_reference(&reference_label),
            workspace_suffix,
            candidate.display_name
        )];
        if let Some(summary) = candidate
            .summary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(compact_text(summary, 180));
        }
        if let Some(birthday) = candidate
            .birthday
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Birthday: {}.", format_dictionary_date(birthday)));
        }
        if !candidate.hobbies.is_empty() {
            lines.push(format!("Hobbies: {}.", candidate.hobbies.join(", ")));
        }
        if let Some(excerpt) = candidate
            .document_excerpt
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("Notes: {}", compact_text(excerpt, 180)));
        }
        return lines.join("\n");
    }

    let mut lines = vec![format!(
        "I found these matches for {}{}:",
        reference_label, workspace_suffix
    )];
    for candidate in envelope.candidates.iter().take(8) {
        let mut line = format!("- {}", candidate.display_name);
        if let Some(summary) = candidate
            .summary
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(": {}", compact_text(summary, 100)));
        }
        lines.push(line);
    }
    if envelope.candidates.len() > 8 {
        lines.push(format!("... and {} more.", envelope.candidates.len() - 8));
    }
    lines.join("\n")
}

fn dictionary_fact_birthday(fact: &GroundedDictionaryFactSummary) -> Option<String> {
    if fact.fact_key != "birthday" {
        return None;
    }
    if fact.value_type == "date" {
        return fact.value_date.clone();
    }
    fact.value_text.clone()
}

fn dictionary_fact_hobbies(fact: &GroundedDictionaryFactSummary) -> Vec<String> {
    if fact.fact_key != "hobbies" {
        return Vec::new();
    }
    if let Some(Value::Array(values)) = fact.value_json.as_ref() {
        return values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
    }
    fact.value_text
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn dictionary_workspace_kind_label(workspace_kind: &str) -> &str {
    match workspace_kind {
        "family_shared" => "family",
        "friends_private" => "friends",
        "work_private" => "work",
        other => other,
    }
}

fn format_dictionary_date(raw: &str) -> String {
    parse_ymd(raw)
        .map(format_with_weekday)
        .unwrap_or_else(|| raw.to_string())
}

fn dictionary_reference_label(reference: &str) -> String {
    let trimmed = reference.trim();
    if let Some(rest) = trimmed.strip_prefix("my ") {
        format!("your {}", rest.trim())
    } else {
        trimmed.to_string()
    }
}

fn capitalize_dictionary_reference(reference: &str) -> String {
    let mut chars = reference.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn format_memory_facts_reply(envelope: GroundedMemoryFactsEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();
    let mut lines = vec![if envelope.facts.is_empty() {
        format!("I don't have any stored memory facts{query_suffix}.")
    } else if envelope.query.is_some() {
        format!(
            "Stored memory facts{query_suffix}: {} found.",
            envelope.total_count
        )
    } else {
        format!(
            "Recent stored memory facts: {} found.",
            envelope.total_count
        )
    }];

    for fact in envelope.facts.iter().take(6) {
        let mut line = format!("- {}: {}", fact.title, compact_text(&fact.content, 180));
        if let Some(topic_key) = fact
            .topic_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" [{topic_key}]"));
        } else if !fact.memory_type.trim().is_empty() {
            line.push_str(&format!(" [{}]", fact.memory_type));
        }
        lines.push(line);
    }

    if envelope.facts.len() > 6 {
        lines.push(format!("... and {} more.", envelope.facts.len() - 6));
    }

    lines.join("\n")
}

fn format_memory_entities_reply(envelope: GroundedMemoryEntitiesEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();
    let mut lines = vec![if envelope.entities.is_empty() {
        format!("I don't have any stored people or group records{query_suffix}.")
    } else if envelope.query.is_some() {
        format!(
            "Stored entities{query_suffix}: {} found.",
            envelope.total_count
        )
    } else {
        format!("Recent stored entities: {} found.", envelope.total_count)
    }];

    for entity in envelope.entities.iter().take(6) {
        let mut line = format!("- {} ({})", entity.label, entity.entity_kind);
        if let Some(identifier) = entity
            .identifier
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" id={identifier}"));
        }
        if let Some(topic_key) = entity
            .topic_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" [{topic_key}]"));
        }
        lines.push(line);
    }

    if envelope.entities.len() > 6 {
        lines.push(format!("... and {} more.", envelope.entities.len() - 6));
    }

    lines.join("\n")
}

fn format_memory_entity_relations_reply(envelope: GroundedMemoryEntityRelationsEnvelope) -> String {
    let query_suffix = envelope
        .root
        .as_ref()
        .map(|root| format!(" for \"{}\"", root.label))
        .unwrap_or_else(|| format!(" matching \"{}\"", envelope.query));
    let mut lines = vec![if envelope.root.is_none() {
        format!("I don't have any stored entity relations{query_suffix}.")
    } else if envelope.relations.is_empty() {
        format!(
            "Stored entity relations{query_suffix}: none found via {}.",
            envelope.matched_by
        )
    } else {
        format!(
            "Stored entity relations{query_suffix}: {} found via {}.",
            envelope.total_count, envelope.matched_by
        )
    }];

    if let Some(root) = envelope.root.as_ref() {
        let mut root_line = format!("- Root: {} ({})", root.label, root.entity_kind);
        if let Some(topic_key) = root
            .topic_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            root_line.push_str(&format!(" [{topic_key}]"));
        }
        lines.push(root_line);
    }

    for relation in envelope.relations.iter().take(6) {
        let arrow = if relation.direction == "incoming" {
            "<-"
        } else {
            "->"
        };
        let mut line = format!(
            "- {} {} {}",
            relation.relation, arrow, relation.entity.label
        );
        if !relation.entity.entity_kind.trim().is_empty() {
            line.push_str(&format!(" ({})", relation.entity.entity_kind));
        }
        lines.push(line);
    }

    if envelope.relations.len() > 6 {
        lines.push(format!("... and {} more.", envelope.relations.len() - 6));
    }

    lines.join("\n")
}

fn format_memory_person_summary_reply(envelope: GroundedMemoryPersonSummaryEnvelope) -> String {
    let mut lines = vec![format!(
        "Stored person summary for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];

    let mut person_line = format!(
        "- Person: {} ({})",
        envelope.person.label, envelope.person.entity_kind
    );
    if let Some(identifier) = envelope
        .person
        .identifier
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        person_line.push_str(&format!(" id={identifier}"));
    }
    if let Some(topic_key) = envelope
        .person
        .topic_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        person_line.push_str(&format!(" [{topic_key}]"));
    }
    lines.push(person_line);
    lines.push(format!(
        "- Scope: {}. Ordinal: {}. Created: {}. Updated: {}. Related entities: {}.",
        envelope.person.access_scope,
        envelope.person.ordinal,
        format_unix_timestamp(envelope.person.created_ts),
        format_unix_timestamp(envelope.person.updated_ts),
        envelope.relation_count
    ));

    for relation in envelope.relations.iter().take(6) {
        let arrow = if relation.direction == "incoming" {
            "<-"
        } else {
            "->"
        };
        let mut line = format!(
            "- {} {} {}",
            relation.relation, arrow, relation.entity.label
        );
        if !relation.entity.entity_kind.trim().is_empty() {
            line.push_str(&format!(" ({})", relation.entity.entity_kind));
        }
        lines.push(line);
    }

    if envelope.relations.len() > 6 {
        lines.push(format!("... and {} more.", envelope.relations.len() - 6));
    }

    lines.join("\n")
}

fn format_memory_recent_changes_reply(envelope: GroundedMemoryRecentChangesEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    let mut lines = vec![
        if envelope.facts.is_empty() && envelope.entities.is_empty() {
            format!("I didn't find any stored memory changes{query_suffix}.")
        } else if envelope.query.is_some() {
            format!(
                "Recent stored memory changes{query_suffix}: {} facts and {} entities found.",
                envelope.fact_count, envelope.entity_count
            )
        } else {
            format!(
                "Recent stored memory changes: {} facts and {} entities found.",
                envelope.fact_count, envelope.entity_count
            )
        },
    ];

    if !envelope.facts.is_empty() {
        lines.push("Facts:".to_string());
        for fact in envelope.facts.iter().take(4) {
            let mut line = format!("- {}: {}", fact.title, compact_text(&fact.content, 160));
            if let Some(topic_key) = fact
                .topic_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str(&format!(" [{topic_key}]"));
            } else if !fact.memory_type.trim().is_empty() {
                line.push_str(&format!(" [{}]", fact.memory_type));
            }
            lines.push(line);
        }
        if envelope.facts.len() > 4 {
            lines.push(format!("... and {} more facts.", envelope.facts.len() - 4));
        }
    }

    if !envelope.entities.is_empty() {
        lines.push("Entities:".to_string());
        for entity in envelope.entities.iter().take(4) {
            let mut line = format!("- {} ({})", entity.label, entity.entity_kind);
            if let Some(identifier) = entity
                .identifier
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str(&format!(" id={identifier}"));
            }
            if let Some(topic_key) = entity
                .topic_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str(&format!(" [{topic_key}]"));
            }
            if let Some(source_chunk_id) = entity
                .source_chunk_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                line.push_str(&format!(" source={source_chunk_id}"));
            }
            lines.push(line);
        }
        if envelope.entities.len() > 4 {
            lines.push(format!(
                "... and {} more entities.",
                envelope.entities.len() - 4
            ));
        }
    }

    lines.join("\n")
}

fn format_memory_conflicting_facts_reply(
    envelope: GroundedMemoryConflictingFactsEnvelope,
) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    let mut lines = vec![if envelope.conflicts.is_empty() {
        format!("I didn't find any conflicting stored memory facts{query_suffix}.")
    } else if envelope.query.is_some() {
        format!(
            "Conflicting stored memory facts{query_suffix}: {} conflict groups across {} facts.",
            envelope.conflict_group_count, envelope.total_count
        )
    } else {
        format!(
            "Conflicting stored memory facts: {} conflict groups across {} facts.",
            envelope.conflict_group_count, envelope.total_count
        )
    }];

    for conflict in envelope.conflicts.iter().take(4) {
        let mut line = format!(
            "- {}: {} facts, {} distinct contents",
            conflict.title, conflict.fact_count, conflict.distinct_content_count
        );
        if let Some(topic_key) = conflict
            .topic_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" [{topic_key}]"));
        }
        lines.push(line);

        for fact in conflict.facts.iter().take(3) {
            lines.push(format!(
                "  - {}: {}",
                fact.title,
                compact_text(&fact.content, 140)
            ));
        }
    }

    if envelope.conflicts.len() > 4 {
        lines.push(format!(
            "... and {} more conflict groups.",
            envelope.conflicts.len() - 4
        ));
    }

    lines.join("\n")
}

fn format_memory_entity_provenance_reply(
    envelope: GroundedMemoryEntityProvenanceEnvelope,
) -> String {
    let Some(entity) = envelope.entity.as_ref() else {
        return format!(
            "I couldn't find a stored entity matching \"{}\".",
            envelope.query
        );
    };

    let mut lines = vec![format!(
        "Stored entity provenance for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];
    let mut entity_line = format!("- Entity: {} ({})", entity.label, entity.entity_kind);
    if let Some(identifier) = entity
        .identifier
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        entity_line.push_str(&format!(" id={identifier}"));
    }
    if let Some(topic_key) = entity
        .topic_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        entity_line.push_str(&format!(" [{topic_key}]"));
    }
    if let Some(source_chunk_id) = entity
        .source_chunk_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        entity_line.push_str(&format!(" source={source_chunk_id}"));
    }
    lines.push(entity_line);
    lines.push(format!(
        "- Scope: {}. Ordinal: {}. Created: {}. Updated: {}.",
        entity.access_scope,
        entity.ordinal,
        format_unix_timestamp(entity.created_ts),
        format_unix_timestamp(entity.updated_ts)
    ));
    if let Some(conversation_id) = entity
        .conversation_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("- Conversation: {conversation_id}."));
    }
    if let Some(turn_id) = entity
        .turn_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("- Turn: {turn_id}."));
    }

    if let Some(source_chunk) = envelope.source_chunk.as_ref() {
        let mut chunk_line = format!(
            "- Source chunk: {} ({}, {}).",
            source_chunk.title, source_chunk.source_kind, source_chunk.source_id
        );
        if let Some(source_sub_id) = source_chunk
            .source_sub_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            chunk_line.push_str(&format!(" Sub-id: {source_sub_id}."));
        }
        if let Some(topic_key) = source_chunk
            .topic_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            chunk_line.push_str(&format!(" [{topic_key}]"));
        }
        if let Some(access_key) = source_chunk
            .access_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            chunk_line.push_str(&format!(" access={access_key}"));
        }
        lines.push(chunk_line);
        if let Some(owner_user_id) = source_chunk
            .owner_user_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("- Source owner: {owner_user_id}."));
        }
        lines.push(format!(
            "- Source scope: {}. Source updated: {}. Observed: {}.",
            source_chunk.access_scope,
            format_unix_timestamp(source_chunk.updated_ts),
            format_unix_timestamp(source_chunk.source_ts)
        ));
        lines.push(format!(
            "- Excerpt: {}",
            compact_text(&source_chunk.excerpt, 180)
        ));
    }

    lines.join("\n")
}

fn format_storage_path_detail_reply(
    message: &str,
    envelope: GroundedStoragePathDetailEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Storage path detail for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];
    lines.push(format!("Name: {}.", envelope.path.name));
    lines.push(format!("Path: {}.", envelope.path.path));
    lines.push(format!(
        "Exists: {}.",
        if envelope.path.exists { "yes" } else { "no" }
    ));
    if let Some(resolved_path) = envelope
        .path
        .resolved_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Resolved path: {resolved_path}."));
    }
    if let Some(stats_path) = envelope
        .path
        .stats_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Stats path: {stats_path}."));
    }
    if let Some(mount_point) = envelope
        .path
        .mount_point
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Mount point: {mount_point}."));
    }
    if let Some(file_system) = envelope
        .path
        .mount_file_system
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("File system: {file_system}."));
    }
    if let Some(mount_source) = envelope
        .path
        .mount_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Mount source: {mount_source}."));
    }
    if let Some(total_human) = envelope
        .path
        .total_human
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mut usage = format!(
            "Usage: {} used of {}",
            envelope.path.used_human.as_deref().unwrap_or("unknown"),
            total_human
        );
        if let Some(used_percent) = envelope.path.used_percent {
            usage.push_str(&format!(" ({used_percent:.1}% used)"));
        }
        usage.push('.');
        lines.push(usage);
    } else if let Some(available_human) = envelope
        .path
        .available_human
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Available space: {available_human}."));
    }
    if let Some(total_bytes) = envelope.path.total_bytes {
        lines.push(format!("Total bytes: {total_bytes}."));
    }
    if let Some(available_bytes) = envelope.path.available_bytes {
        lines.push(format!("Available bytes: {available_bytes}."));
    }
    if let Some(used_bytes) = envelope.path.used_bytes {
        lines.push(format!("Used bytes: {used_bytes}."));
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw storage payload.".to_string());
    }

    lines.join("\n")
}

fn format_system_port_conflict_detail_reply(
    message: &str,
    envelope: GroundedSystemPortConflictDetailEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Port conflict detail for \"{}\" matched by {}.",
        envelope
            .query
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&envelope.conflict.raw_entry),
        envelope.matched_by
    )];

    lines.push(format!(
        "Matched {} listening socket(s).",
        envelope.total_count.max(1)
    ));
    lines.push(format!(
        "Socket: {} {}",
        envelope.conflict.protocol.to_ascii_uppercase(),
        envelope.conflict.local_address
    ));
    if let Some(port) = envelope.conflict.local_port {
        if let Some(last) = lines.last_mut() {
            last.push_str(&format!(":{port}."));
        }
    } else if let Some(last) = lines.last_mut() {
        last.push('.');
    }

    lines.push(format!("State: {}.", envelope.conflict.state));
    if let Some(peer) = envelope
        .conflict
        .peer_address
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Peer address: {peer}."));
    }

    if envelope.conflict.processes.is_empty() {
        lines.push("Processes: none reported.".to_string());
    } else {
        let processes = envelope
            .conflict
            .processes
            .iter()
            .take(6)
            .map(|process| {
                let mut label = process.name.clone();
                if let Some(pid) = process.pid {
                    label.push_str(&format!(" pid={pid}"));
                }
                if let Some(fd) = process.fd {
                    label.push_str(&format!(" fd={fd}"));
                }
                label
            })
            .collect::<Vec<_>>();
        lines.push(format!("Processes: {}.", processes.join(", ")));
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines.push(format!("Raw entry: {}.", envelope.conflict.raw_entry));
    }

    lines.join("\n")
}

fn format_storage_mount_detail_reply(
    message: &str,
    envelope: GroundedStorageMountDetailEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Storage mount detail for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];
    lines.push(format!("Matched {} mount(s).", envelope.total_count.max(1)));
    lines.push(format!("Mount point: {}.", envelope.mount.mount_point));
    if let Some(file_system) = envelope
        .mount
        .mount_file_system
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("File system: {file_system}."));
    }
    if let Some(mount_source) = envelope
        .mount
        .mount_source
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Mount source: {mount_source}."));
    }
    if envelope.mount.tracked_paths.is_empty() {
        lines.push("Tracked paths: none.".to_string());
    } else {
        lines.push(format!(
            "Tracked paths: {}.",
            envelope.mount.tracked_paths.join(", ")
        ));
    }
    if let Some(total_human) = envelope
        .mount
        .total_human
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let mut usage = format!(
            "Usage: {} used of {}",
            envelope.mount.used_human.as_deref().unwrap_or("unknown"),
            total_human
        );
        if let Some(used_percent) = envelope.mount.used_percent {
            usage.push_str(&format!(" ({used_percent:.1}% used)"));
        }
        usage.push('.');
        lines.push(usage);
    } else if let Some(available_human) = envelope
        .mount
        .available_human
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Available space: {available_human}."));
    }
    if let Some(total_bytes) = envelope.mount.total_bytes {
        lines.push(format!("Total bytes: {total_bytes}."));
    }
    if let Some(available_bytes) = envelope.mount.available_bytes {
        lines.push(format!("Available bytes: {available_bytes}."));
    }
    if let Some(used_bytes) = envelope.mount.used_bytes {
        lines.push(format!("Used bytes: {used_bytes}."));
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw storage payload.".to_string());
    }

    lines.join("\n")
}

pub fn deterministic_downloads_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks.iter().find(|block| {
        matches!(
            block.tool,
            "downloads_get_artifact_checksum"
                | "downloads_get_artifact_install_steps"
                | "downloads_get_artifact_compatibility"
                | "downloads_get_artifact_source"
                | "downloads_get_release_notes"
                | "downloads_get_artifact_details"
                | "downloads_list_available_artifacts"
        )
    })?;

    Some(if block.status == "ok" {
        match block.tool {
            "downloads_list_available_artifacts" => format_downloads_reply(
                serde_json::from_value::<GroundedDownloadListEnvelope>(block.data.clone()).ok()?,
            ),
            "downloads_get_artifact_details" => format_download_artifact_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "downloads_get_artifact_checksum" => format_download_artifact_checksum_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "downloads_get_artifact_install_steps" => format_download_artifact_install_steps_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "downloads_get_artifact_compatibility" => format_download_artifact_compatibility_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "downloads_get_artifact_source" => format_download_artifact_source_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "downloads_get_release_notes" => format_download_release_notes_reply(
                message,
                serde_json::from_value::<GroundedDownloadDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the download details.")
    })
}

pub fn deterministic_library_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "library_get_item_media_details")
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "library_get_item_source_paths")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "library_get_item_summary")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "libraries_get_library_summary")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "libraries_find_duplicate_titles")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "libraries_list_missing_metadata")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "library_search_titles")
        })?;

    Some(if block.status == "ok" {
        match block.tool {
            "library_get_item_media_details" => format_library_item_media_reply(
                message,
                serde_json::from_value::<GroundedLibraryItemMediaEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "library_get_item_source_paths" => format_library_item_source_paths_reply(
                message,
                serde_json::from_value::<GroundedLibraryItemMediaEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "library_search_titles" => {
                let envelope =
                    serde_json::from_value::<GroundedLibrarySearchEnvelope>(block.data.clone())
                        .ok()?;
                format_library_search_reply(
                    extract_library_search_query(message)
                        .or_else(|| extract_quoted_phrase(&block.label))
                        .as_deref(),
                    envelope,
                )
            }
            "library_get_item_summary" => format_library_item_detail_reply(
                message,
                serde_json::from_value::<GroundedLibraryItemDetailEnvelope>(block.data.clone())
                    .ok()?,
            ),
            "libraries_get_library_summary" => format_library_detail_reply(
                message,
                serde_json::from_value::<GroundedLibraryDetailEnvelope>(block.data.clone()).ok()?,
            ),
            "libraries_find_duplicate_titles" => format_library_duplicate_titles_reply(
                message,
                serde_json::from_value::<GroundedLibraryDuplicateTitlesEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            "libraries_list_missing_metadata" => format_library_missing_metadata_reply(
                message,
                serde_json::from_value::<GroundedLibraryMissingMetadataEnvelope>(
                    block.data.clone(),
                )
                .ok()?,
            ),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the library details.")
    })
}

pub fn deterministic_web_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "web_list_curated_sources")
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "web_search_public_web")
        })
        .or_else(|| {
            grounding_blocks
                .iter()
                .find(|block| block.tool == "web_fetch_public_page_summary")
        })?;

    Some(if block.status == "ok" {
        match block.tool {
            "web_list_curated_sources" => format_web_curated_sources_reply(&block.data)
                .unwrap_or_else(|| {
                    "I loaded the curated public web source catalog, but I couldn't format it."
                        .to_string()
                }),
            "web_search_public_web" => format_web_search_reply(message, &block.data)
                .unwrap_or_else(|| {
                    "I loaded public web search results, but I couldn't format them.".to_string()
                }),
            "web_fetch_public_page_summary" => format_web_page_summary_reply(message, &block.data)
                .unwrap_or_else(|| {
                    "I loaded the public web page summary, but I couldn't format it.".to_string()
                }),
            _ => return None,
        }
    } else {
        format_tool_error(block, "I couldn't load the public web details.")
    })
}

fn format_web_curated_sources_reply(data: &Value) -> Option<String> {
    let categories = data.get("categories")?.as_array()?;
    let mut lines = vec!["Curated public web source catalog:".to_string()];

    for category in categories {
        let label = category
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("Curated");
        let description = category
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let source_count = category
            .get("source_count")
            .and_then(Value::as_u64)
            .map(|count| count as usize)
            .or_else(|| {
                category
                    .get("sources")
                    .and_then(Value::as_array)
                    .map(Vec::len)
            })
            .unwrap_or(0);

        let mut line = format!("- {}: {} source(s)", label, source_count);
        if let Some(description) = description {
            line.push_str(&format!(" - {description}"));
        }
        lines.push(line);

        if let Some(source_names) = category
            .get("sources")
            .and_then(Value::as_array)
            .map(|sources| {
                sources
                    .iter()
                    .filter_map(|source| source.get("name").and_then(Value::as_str))
                    .take(8)
                    .collect::<Vec<_>>()
            })
            .filter(|names| !names.is_empty())
        {
            lines.push(format!("  Sources: {}.", source_names.join(", ")));
        }
    }

    Some(lines.join("\n"))
}

fn format_web_search_reply(message: &str, data: &Value) -> Option<String> {
    let query = data
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("the public web");
    let category_label = data
        .get("category")
        .and_then(Value::as_str)
        .and_then(curated_web_category_label);
    let results = data.get("results").and_then(Value::as_array)?;

    let mut lines = vec![match category_label {
        Some(label) => format!("Curated {label} web results for \"{query}\":"),
        None => format!("Public web results for \"{query}\":"),
    }];
    if results.is_empty() {
        lines.push("No results were returned.".to_string());
        return Some(lines.join("\n"));
    }

    for (index, result) in results.iter().take(5).enumerate() {
        let title = result
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Untitled");
        let source_host = result
            .get("source_host")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown host");
        let url = result
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let mut line = format!("{}. {} ({source_host})", index + 1, title);
        if let Some(url) = url {
            line.push_str(&format!(": {url}"));
        }
        lines.push(line);

        if let Some(snippet) = result
            .get("snippet")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            lines.push(format!("   {}", compact_text(snippet, 220)));
        }
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw payload.".to_string());
    }

    Some(lines.join("\n"))
}

fn format_web_page_summary_reply(_message: &str, data: &Value) -> Option<String> {
    let category_label = data
        .get("category")
        .and_then(Value::as_str)
        .and_then(curated_web_category_label);
    let page_title = data
        .get("page_title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source_host = data
        .get("source_host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown host");
    let requested_url = data
        .get("requested_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let final_url = data
        .get("final_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let summary = data
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let content_type = data
        .get("content_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut lines = vec![match (category_label, page_title) {
        (Some(label), Some(title)) => format!("{label} page summary for \"{title}\":"),
        (Some(label), None) => format!("{label} page summary from {source_host}:"),
        (None, Some(title)) => format!("Public page summary for \"{title}\":"),
        (None, None) => format!("Public page summary from {source_host}:"),
    }];

    if let Some(requested_url) = requested_url {
        if final_url.map(|url| url != requested_url).unwrap_or(true) {
            lines.push(format!("Requested URL: {requested_url}."));
        }
    }
    if let Some(final_url) = final_url {
        lines.push(format!("Final URL: {final_url}."));
    }
    lines.push(format!("Source host: {source_host}."));
    if let Some(summary) = summary {
        lines.push(format!("Summary: {summary}."));
    } else {
        lines.push("No page summary text was returned.".to_string());
    }
    if let Some(content_type) = content_type {
        lines.push(format!("Content type: {content_type}."));
    }

    Some(lines.join("\n"))
}

pub fn deterministic_multi_step_reply(
    message: &str,
    execution_trace: Option<&AssistantExecutionTrace>,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let trace = execution_trace?;
    if matches!(
        trace.stop_reason,
        AssistantExecutionStopReason::DeterministicReply
            | AssistantExecutionStopReason::SufficientAnswer
            | AssistantExecutionStopReason::ModelAnswerCompleted
            | AssistantExecutionStopReason::ClarificationRequired
    ) {
        return None;
    }

    if trace.stop_reason == AssistantExecutionStopReason::ConflictUnresolved {
        return Some(
            "I found conflicting grounded results and couldn't safely reconcile them in this bounded pass."
                .to_string(),
        );
    }

    if let Some(kind) = trace.final_outcome_kind {
        if matches!(
            kind,
            AssistantToolOutcomeKind::NotFound
                | AssistantToolOutcomeKind::Empty
                | AssistantToolOutcomeKind::ValidationFailed
                | AssistantToolOutcomeKind::WeakMatch
        ) {
            if let Some(reply) = deterministic_calendar_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_dictionary_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_downloads_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_library_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_web_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_memory_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) =
                super::weather::deterministic_weather_reply(message, grounding_blocks)
            {
                return Some(reply);
            }
            if let Some(reply) = deterministic_ai_runtime_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_system_reply(message, grounding_blocks) {
                return Some(reply);
            }
            if let Some(reply) = deterministic_network_reply(message, grounding_blocks) {
                return Some(reply);
            }
        }
    }

    let last_message = grounding_blocks
        .last()
        .and_then(|block| block.data.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match trace.stop_reason {
        AssistantExecutionStopReason::BudgetExhausted => Some(
            "I checked the most likely grounded sources, but I still don't have a confident answer."
                .to_string(),
        ),
        AssistantExecutionStopReason::NoPermittedFallback
        | AssistantExecutionStopReason::WeakEvidenceOnly => last_message
            .map(str::to_string)
            .or_else(|| {
                Some(
                    "I checked the most likely grounded sources, but I couldn't confirm a stronger answer."
                        .to_string(),
                )
            }),
        AssistantExecutionStopReason::DuplicateSignature => Some(
            "I hit the same grounded result again, so I stopped rather than looping."
                .to_string(),
        ),
        AssistantExecutionStopReason::AclDenied => Some(
            "I couldn't continue because the next grounded step was outside your allowed access scope."
                .to_string(),
        ),
        AssistantExecutionStopReason::FatalError => Some(
            "I couldn't complete that grounded lookup because the underlying tool failed."
                .to_string(),
        ),
        _ => None,
    }
}

fn format_next_event_reply(envelope: GroundedNextEventEnvelope) -> String {
    let Some(next_event) = envelope.next_event else {
        return "You do not have any visible upcoming calendar events.".to_string();
    };

    let event_date = parse_ymd(&next_event.next_occurs_on);
    let scope = scope_label(&next_event.scope);
    let timing = event_date.map(describe_relative_timing).unwrap_or_default();

    if next_event.event_type == "birthday" {
        let human_date = event_date
            .map(format_with_weekday)
            .unwrap_or_else(|| next_event.next_occurs_on.clone());
        return format!(
            "Your next visible calendar item is the recurring birthday \"{}\" on {} in {}.{}",
            next_event.title, human_date, scope, timing
        );
    }

    let human_date = event_date
        .map(format_with_weekday)
        .unwrap_or_else(|| next_event.next_occurs_on.clone());
    format!(
        "Your next visible calendar event is \"{}\" on {} in {}.{}",
        next_event.title, human_date, scope, timing
    )
}

fn format_next_event_timing_reply(envelope: GroundedNextEventTimingEnvelope) -> String {
    let Some(next_event) = envelope.next_event else {
        return "You do not have any visible upcoming calendar events.".to_string();
    };

    let event_date = parse_ymd(&next_event.next_occurs_on);
    let scope = scope_label(&next_event.scope);
    let timing = envelope
        .days_until
        .map(|days| match days {
            0 => " That is today.".to_string(),
            1 => " That is tomorrow.".to_string(),
            value => format!(" That is in {value} days."),
        })
        .unwrap_or_else(|| event_date.map(describe_relative_timing).unwrap_or_default());

    if next_event.event_type == "birthday" {
        let human_date = event_date
            .map(format_with_weekday)
            .unwrap_or_else(|| next_event.next_occurs_on.clone());
        return format!(
            "Your next visible calendar item is the recurring birthday \"{}\" on {} in {}.{}",
            next_event.title, human_date, scope, timing
        );
    }

    let human_date = event_date
        .map(format_with_weekday)
        .unwrap_or_else(|| next_event.next_occurs_on.clone());
    format!(
        "Your next visible calendar event is \"{}\" on {} in {}.{}",
        next_event.title, human_date, scope, timing
    )
}

fn format_calendar_conflicts_reply(envelope: GroundedCalendarConflictEnvelope) -> String {
    if envelope.conflict_days.is_empty() {
        if envelope.total_event_count == 0 {
            return format!(
                "I didn't find any visible calendar events in {}, so there were no conflicts to report.",
                envelope.window.label
            );
        }
        return format!(
            "I didn't find any visible calendar date conflicts in {}. I found {} visible calendar events across separate dates.",
            envelope.window.label, envelope.total_event_count
        );
    }

    let mut lines = vec![format!(
        "Visible calendar conflicts in {}:",
        envelope.window.label
    )];
    for day in envelope.conflict_days.iter().take(6) {
        let human_date = parse_ymd(&day.date)
            .map(format_with_weekday)
            .unwrap_or_else(|| day.date.clone());
        let titles = day
            .events
            .iter()
            .take(4)
            .map(|event| event.title.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if titles.is_empty() {
            lines.push(format!(
                "- {}: {} visible events.",
                human_date, day.event_count
            ));
        } else {
            lines.push(format!(
                "- {}: {} visible events. {}",
                human_date, day.event_count, titles
            ));
        }
    }
    if envelope.conflict_day_count > lines.len().saturating_sub(1) {
        lines.push(format!(
            "I found {} conflicting dates in total.",
            envelope.conflict_day_count
        ));
    }
    lines.join("\n")
}

fn format_calendar_free_days_reply(envelope: GroundedCalendarFreeDaysEnvelope) -> String {
    if envelope.free_days.is_empty() {
        return format!(
            "I didn't find any free visible calendar days in {}.",
            envelope.window.label
        );
    }

    let dates = envelope
        .free_days
        .iter()
        .take(8)
        .map(|day| {
            parse_ymd(&day.date)
                .map(format_with_weekday)
                .unwrap_or_else(|| day.date.clone())
        })
        .collect::<Vec<_>>();
    if dates.len() == 1 {
        return format!(
            "You have one free visible calendar day in {}: {}.",
            envelope.window.label, dates[0]
        );
    }

    let mut lines = vec![format!(
        "Free visible calendar days in {}: {}.",
        envelope.window.label,
        dates.join(", ")
    )];
    if envelope.free_day_count > dates.len() {
        lines.push(format!(
            "I found at least {} free dates in that window.",
            envelope.free_day_count
        ));
    }
    lines.join("\n")
}

fn format_calendar_count_reply(envelope: GroundedCalendarCountEnvelope) -> String {
    if envelope.total_event_count == 0 {
        return format!(
            "I didn't find any visible calendar events in {}.",
            envelope.window.label
        );
    }

    let busiest_day = envelope.day_counts.iter().max_by_key(|day| day.event_count);
    let mut lines = vec![format!(
        "You have {} visible calendar events in {} across {} busy days.",
        envelope.total_event_count, envelope.window.label, envelope.busy_day_count
    )];
    if let Some(day) = busiest_day {
        let human_date = parse_ymd(&day.date)
            .map(format_with_weekday)
            .unwrap_or_else(|| day.date.clone());
        lines.push(format!(
            "The busiest day is {} with {} events.",
            human_date, day.event_count
        ));
    }

    let top_days = envelope
        .day_counts
        .iter()
        .take(5)
        .map(|day| {
            let human_date = parse_ymd(&day.date)
                .map(format_with_weekday)
                .unwrap_or_else(|| day.date.clone());
            format!("{human_date} ({} events)", day.event_count)
        })
        .collect::<Vec<_>>();
    if !top_days.is_empty() {
        lines.push(format!("Busy days: {}.", top_days.join(", ")));
    }

    lines.join("\n")
}

fn format_calendar_busy_days_reply(envelope: GroundedCalendarBusyDaysEnvelope) -> String {
    if envelope.busy_days.is_empty() {
        return format!(
            "I didn't find any visible calendar events in {}, so there were no busy days to report.",
            envelope.window.label
        );
    }

    let mut lines = vec![format!(
        "Busy visible calendar days in {}:",
        envelope.window.label
    )];
    for day in envelope.busy_days.iter().take(6) {
        let human_date = parse_ymd(&day.date)
            .map(format_with_weekday)
            .unwrap_or_else(|| day.date.clone());
        let titles = day
            .events
            .iter()
            .take(4)
            .map(|event| event.title.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if titles.is_empty() {
            lines.push(format!(
                "- {}: {} visible events.",
                human_date, day.event_count
            ));
        } else {
            lines.push(format!(
                "- {}: {} visible events. {}",
                human_date, day.event_count, titles
            ));
        }
    }
    if envelope.busy_day_count > lines.len().saturating_sub(1) {
        lines.push(format!(
            "I found {} busy dates in total across {} visible events.",
            envelope.busy_day_count, envelope.total_event_count
        ));
    }
    lines.join("\n")
}

fn format_birthdays_reply(message: &str, envelope: GroundedBirthdayEnvelope) -> String {
    if envelope.birthdays.is_empty() {
        return match envelope.query.as_deref() {
            Some(query) => format!(
                "I couldn't find a visible birthday matching \"{query}\" in {}.",
                envelope.window.label
            ),
            None => format!(
                "There are no visible upcoming birthdays in {}.",
                envelope.window.label
            ),
        };
    }

    if envelope.birthdays.len() == 1
        || crate::ai_assistant::orchestrator::is_next_birthday_request(message)
    {
        return format_single_birthday_reply(&envelope.birthdays[0]);
    }

    let mut lines = vec![format!("Upcoming birthdays in {}:", envelope.window.label)];
    for birthday in envelope.birthdays.iter().take(6) {
        let name = birthday_display_name(&birthday.title);
        let date = parse_ymd(&birthday.next_occurs_on)
            .map(format_with_weekday)
            .unwrap_or_else(|| birthday.next_occurs_on.clone());
        let age = birthday_turning_age(birthday)
            .map(|value| format!(" (turns {value})"))
            .unwrap_or_default();
        lines.push(format!("- {name}: {date}{age}"));
    }
    lines.join("\n")
}

fn format_single_birthday_reply(birthday: &GroundedBirthdaySummary) -> String {
    let name = birthday_display_name(&birthday.title);
    let date = parse_ymd(&birthday.next_occurs_on)
        .map(format_with_weekday)
        .unwrap_or_else(|| birthday.next_occurs_on.clone());
    let age = birthday_turning_age(birthday);
    match age {
        Some(age) => format!("{name}'s next birthday is on {date}. They turn {age}."),
        None => format!("{name}'s next birthday is on {date}."),
    }
}

fn format_network_reply(message: &str, envelope: GroundedNetworkEnvelope) -> String {
    let lower = message.to_ascii_lowercase();
    let local_connect_question = lower.contains("local network")
        || lower.contains("lan")
        || lower.contains("another device")
        || lower.contains("same network")
        || (lower.contains("connect") && lower.contains("rustyfin"))
        || (lower.contains("what ip") && lower.contains("rustyfin"))
        || lower.contains("what ip would i use")
        || lower.contains("which ip would i use");

    if local_connect_question {
        let mut parts = Vec::new();
        if let Some(url) = envelope.access.preferred_local_url.clone() {
            parts.push(format!("On your local network, open Rustyfin at {url}."));
        } else {
            parts.push(format!(
                "Rustyfin is listening on HTTPS port {} on this host.",
                envelope.access.ui_port
            ));
        }
        if let Some(ip) = envelope.access.preferred_local_ipv4.clone() {
            parts.push(format!("The preferred local IP is {ip}."));
        }
        if let Some(interface) = envelope.access.preferred_local_interface.clone() {
            parts.push(format!("The primary LAN interface is {interface}."));
        }
        if let Some(login_url) = envelope.access.login_url {
            parts.push(format!("Login page: {login_url}."));
        }
        if let Some(ai_url) = envelope.access.ai_url {
            parts.push(format!("AI page: {ai_url}."));
        }
        parts.push(format!(
            "The Rustyfin edge/UI port is {}. Internal backend services use {} for the main API and {} for calendar on the host.",
            envelope.access.ui_port, envelope.access.backend_port, envelope.access.calendar_port
        ));
        parts.push(if envelope.remote_access_enabled {
            "Remote access is enabled.".to_string()
        } else {
            "Remote access is currently disabled.".to_string()
        });
        return parts.join(" ");
    }

    let mut interface_summary = envelope
        .nodes
        .iter()
        .filter(|node| node.status == "online")
        .filter(|node| is_notable_user_facing_interface(&node.name))
        .take(3)
        .map(|node| {
            let addresses = node
                .addresses
                .iter()
                .filter(|address| address.family == "inet")
                .map(|address| address.address.clone())
                .collect::<Vec<_>>()
                .join(", ");
            if addresses.is_empty() {
                node.name.clone()
            } else {
                format!("{} ({})", node.name, addresses)
            }
        })
        .collect::<Vec<_>>();
    if interface_summary.is_empty() {
        interface_summary = envelope
            .nodes
            .iter()
            .filter(|node| node.status == "online")
            .take(3)
            .map(|node| {
                let addresses = node
                    .addresses
                    .iter()
                    .filter(|address| address.family == "inet")
                    .map(|address| address.address.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                if addresses.is_empty() {
                    node.name.clone()
                } else {
                    format!("{} ({})", node.name, addresses)
                }
            })
            .collect::<Vec<_>>();
    }

    let host_label = envelope
        .host_label
        .as_deref()
        .unwrap_or("this Rustyfin host");
    let mut parts = vec![format!("Rustyfin network summary for {host_label}.")];
    if let Some(url) = envelope.access.preferred_local_url {
        parts.push(format!("Preferred local URL: {url}."));
    }
    if let Some(interface) = envelope.access.preferred_local_interface {
        parts.push(format!("Primary LAN interface: {interface}."));
    }
    if let Some(public_url) = envelope.access.public_url {
        parts.push(format!("Configured public host URL: {public_url}."));
    }
    parts.push(format!(
        "Edge/UI port {}. Internal API port {}. Internal calendar port {}.",
        envelope.access.ui_port, envelope.access.backend_port, envelope.access.calendar_port
    ));
    parts.push(if envelope.remote_access_enabled {
        "Remote access is enabled.".to_string()
    } else {
        "Remote access is disabled.".to_string()
    });
    if !interface_summary.is_empty() {
        parts.push(format!(
            "Online interfaces: {}.",
            interface_summary.join(", ")
        ));
    }
    parts.join(" ")
}

fn format_network_interface_reply(envelope: GroundedNetworkInterfaceEnvelope) -> String {
    let mut parts = vec![format!(
        "Network interface details for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];
    parts.push(format!(
        "Interface {} is {}.",
        envelope.interface.name, envelope.interface.status
    ));

    if let Some(host_label) = envelope.host_label.as_deref() {
        parts.push(format!("Host label: {host_label}."));
    }
    if let Some(preferred) = envelope.access.preferred_local_interface.as_deref() {
        if preferred.eq_ignore_ascii_case(&envelope.interface.name) {
            parts.push("This is the preferred LAN interface.".to_string());
        }
    }
    if let Some(preferred_ipv4) = envelope.access.preferred_local_ipv4.as_deref() {
        if envelope
            .interface
            .addresses
            .iter()
            .any(|address| address.address == preferred_ipv4)
        {
            parts.push(format!("Preferred local IPv4: {preferred_ipv4}."));
        }
    }
    let addresses = envelope
        .interface
        .addresses
        .iter()
        .map(|address| format!("{} {}", address.family, address.address))
        .collect::<Vec<_>>();
    if !addresses.is_empty() {
        parts.push(format!("Addresses: {}.", addresses.join(", ")));
    }
    parts.push(if envelope.remote_access_enabled {
        "Remote access is enabled.".to_string()
    } else {
        "Remote access is disabled.".to_string()
    });
    parts.push(format!(
        "The Rustyfin edge/UI port is {}.",
        envelope.access.ui_port
    ));
    parts.join(" ")
}

fn format_network_default_route_reply(
    message: &str,
    envelope: GroundedNetworkDefaultRouteEnvelope,
) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    if envelope.routes.is_empty() {
        return format!("I couldn't find a default route{query_suffix} on this host.");
    }

    let mut lines = vec![if envelope.routes.len() == 1 {
        format!("Default route{query_suffix}:")
    } else {
        format!(
            "Default routes{query_suffix}: {} routes found.",
            envelope.total_count.max(envelope.routes.len())
        )
    }];

    for route in envelope.routes.iter().take(4) {
        let mut line = format!("- {}", route.route);
        if let Some(gateway) = route
            .gateway
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" via {gateway}"));
        }
        if let Some(interface) = route
            .interface
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" on {interface}"));
        }
        if let Some(source) = route
            .source
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" src {source}"));
        }
        if let Some(metric) = route.metric {
            line.push_str(&format!(" metric {metric}"));
        }
        if let Some(protocol) = route
            .protocol
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" proto {protocol}"));
        }
        if let Some(scope) = route
            .scope
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" scope {scope}"));
        }
        lines.push(line);
    }

    if envelope.routes.len() > 4 {
        lines.push(format!(
            "... and {} more route(s).",
            envelope.routes.len() - 4
        ));
    }

    if message.to_ascii_lowercase().contains("gateway") {
        lines.push(
            "I kept this answer grounded to the host's active default route and did not guess any wider network path."
                .to_string(),
        );
    }

    lines.join("\n")
}

fn format_network_hostname_aliases_reply(
    envelope: GroundedNetworkHostnameAliasesEnvelope,
) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    let mut lines = vec![format!("Hostname aliases{query_suffix}.")];
    if let Some(host_label) = envelope
        .host_label
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Host label: {host_label}."));
    }
    if let Some(canonical) = envelope
        .canonical_hostname
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Canonical hostname: {canonical}."));
    }
    if let Some(fqdn) = envelope
        .fqdn
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("FQDN: {fqdn}."));
    }

    if envelope.aliases.is_empty() {
        lines.push("No additional hostname aliases were detected.".to_string());
        return lines.join("\n");
    }

    lines.push(format!(
        "Aliases: {}.",
        envelope
            .aliases
            .iter()
            .take(6)
            .map(|alias| format!("{} ({})", alias.name, alias.source))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    if envelope.aliases.len() > 6 {
        lines.push(format!(
            "... and {} more alias(es).",
            envelope.aliases.len() - 6
        ));
    }

    lines.join("\n")
}

fn format_network_dns_servers_reply(envelope: GroundedNetworkDnsServersEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    let mut lines = vec![if envelope.total_count == 0 {
        format!("No DNS servers were returned{query_suffix}.")
    } else {
        format!(
            "DNS servers{query_suffix}: {} found via {}.",
            envelope.total_count, envelope.matched_by
        )
    }];

    for server in envelope.dns_servers.iter().take(8) {
        let mut line = format!("- {} -> {}", server.scope, server.server);
        if let Some(interface) = server
            .interface
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" on {interface}"));
        }
        line.push_str(&format!(" ({})", server.source));
        lines.push(line);
    }

    if envelope.dns_servers.len() > 8 {
        lines.push(format!(
            "... and {} more DNS server entry(ies).",
            envelope.dns_servers.len() - 8
        ));
    }

    lines.join("\n")
}

fn format_service_health_reply(envelope: GroundedServiceHealthEnvelope) -> String {
    let healthy_count = envelope
        .components
        .iter()
        .filter(|component| component.status == "healthy")
        .count();
    let mut parts = vec![if envelope.all_healthy {
        format!(
            "All {} tracked service components are healthy.",
            envelope.components.len()
        )
    } else {
        format!(
            "{healthy_count} of {} tracked service components are healthy.",
            envelope.components.len()
        )
    }];

    for component in envelope.components.iter().take(6) {
        let configured = if component.configured {
            "configured"
        } else {
            "not configured"
        };
        parts.push(format!(
            "- {}: {} ({configured})",
            component.name, component.status
        ));
    }

    parts.join("\n")
}

fn format_service_detail_reply(envelope: GroundedServiceDetailEnvelope) -> String {
    let component = envelope.component;
    let mut parts = vec![format!(
        "Service detail for \"{}\" matched by {}.",
        envelope.query, envelope.matched_by
    )];
    parts.push(format!(
        "Service {} is {}.",
        component.name, component.status
    ));
    parts.push(format!("Configured: {}.", on_off(component.configured)));
    if let Some(url) = component.url {
        parts.push(format!("Health URL: {url}."));
    }
    parts.push(component.detail);
    parts.join(" ")
}

fn format_system_port_conflicts_reply(envelope: GroundedSystemPortConflictsEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    if envelope.conflicts.is_empty() {
        return format!("No listening sockets matched{query_suffix}.");
    }

    let mut lines = vec![format!(
        "Port conflicts{query_suffix}: {} listening socket(s).",
        envelope.total_count.max(envelope.conflicts.len())
    )];

    for conflict in envelope.conflicts.iter().take(6) {
        let mut line = format!(
            "- {} {}",
            conflict.protocol.to_ascii_uppercase(),
            conflict.local_address
        );
        if let Some(port) = conflict.local_port {
            line.push_str(&format!(":{port}"));
        }
        if let Some(peer) = conflict
            .peer_address
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" -> {peer}"));
        }
        line.push_str(&format!(" [{}]", conflict.state));

        if !conflict.processes.is_empty() {
            let processes = conflict
                .processes
                .iter()
                .map(|process| {
                    let mut label = process.name.clone();
                    if let Some(pid) = process.pid {
                        label.push_str(&format!(" pid={pid}"));
                    }
                    if let Some(fd) = process.fd {
                        label.push_str(&format!(" fd={fd}"));
                    }
                    label
                })
                .collect::<Vec<_>>()
                .join(", ");
            line.push_str(&format!(" {processes}"));
        }

        lines.push(line);
    }

    if envelope.conflicts.len() > 6 {
        lines.push(format!(
            "... and {} more socket(s).",
            envelope.conflicts.len() - 6
        ));
    }

    lines.join("\n")
}

fn format_system_failed_units_reply(envelope: GroundedSystemFailedUnitsEnvelope) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    if envelope.units.is_empty() {
        return format!("No failed systemd units were found{query_suffix}.");
    }

    let mut lines = vec![format!(
        "Failed systemd units{query_suffix}: {} unit(s).",
        envelope.total_count.max(envelope.units.len())
    )];

    for unit in envelope.units.iter().take(6) {
        lines.push(format!(
            "- {}: {}/{}/{}",
            unit.name, unit.load, unit.active, unit.sub
        ));
        lines.push(format!("  - {}", compact_text(&unit.description, 160)));
        if let Some(log_excerpt) = unit
            .recent_log_excerpt
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("  - Logs: {}", compact_text(log_excerpt, 220)));
        }
    }

    if envelope.units.len() > 6 {
        lines.push(format!(
            "... and {} more failed unit(s).",
            envelope.units.len() - 6
        ));
    }

    lines.join("\n")
}

fn format_system_failed_unit_detail_reply(
    envelope: GroundedSystemFailedUnitDetailEnvelope,
) -> String {
    let query_suffix = envelope
        .query
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();

    let unit = envelope.detail.unit;
    let status = envelope.detail.status;

    let mut lines = vec![format!(
        "Failed systemd unit detail{query_suffix}: {}.",
        unit.name
    )];
    lines.push(format!(
        "  - State: {}/{}/{}",
        unit.load, unit.active, unit.sub
    ));
    lines.push(format!(
        "  - Description: {}",
        compact_text(&unit.description, 160)
    ));

    if let Some(value) = status
        .unit_file_state
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  - Unit file state: {value}"));
    }
    if let Some(value) = status
        .fragment_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  - Fragment: {value}"));
    }
    if let Some(pid) = status.main_pid {
        lines.push(format!("  - Main PID: {pid}"));
    }
    if let Some(value) = status
        .exec_main_code
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let suffix = status
            .exec_main_status
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|status| format!(" ({status})"))
            .unwrap_or_default();
        lines.push(format!("  - Exec result: {value}{suffix}"));
    } else if let Some(value) = status
        .exec_main_status
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  - Exec status: {value}"));
    }
    if let Some(log_excerpt) = unit
        .recent_log_excerpt
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  - Logs: {}", compact_text(log_excerpt, 220)));
    }
    if let Some(status_excerpt) = status
        .status_excerpt
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("  - Status: {}", compact_text(status_excerpt, 220)));
    }

    lines.join("\n")
}

fn format_system_process_detail_reply(
    message: &str,
    envelope: GroundedSystemProcessDetailEnvelope,
) -> String {
    if !envelope.available {
        return envelope
            .reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|reason| format!("Process detail is not available on this host. {reason}"))
            .unwrap_or_else(|| "Process detail is not available on this host.".to_string());
    }

    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();
    let matched_by = envelope
        .matched_by
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("query_contains");
    let mut lines = vec![format!(
        "Process detail{query_suffix}: {} process(es) matched by {}.",
        envelope.total_count, matched_by
    )];

    if let Some(observed_at) = envelope
        .observed_at
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Observed at: {observed_at}."));
    }

    for process in envelope.processes.iter().take(5) {
        let mut line = String::from("- ");
        if let Some(pid) = process.pid {
            line.push_str(&format!("pid={pid} "));
        }
        if let Some(user) = process
            .user
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("user={user} "));
        }
        if let Some(state) = process
            .state
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("state={state} "));
        }
        if let Some(cpu) = process.cpu_percent {
            line.push_str(&format!("cpu={} ", format_decimal(cpu)));
        }
        if let Some(mem) = process.mem_percent {
            line.push_str(&format!("mem={} ", format_decimal(mem)));
        }
        if let Some(elapsed_secs) = process.elapsed_secs {
            line.push_str(&format!(
                "elapsed={} ",
                format_elapsed_seconds(elapsed_secs)
            ));
        }
        if let Some(command) = process
            .command
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("command={command}"));
        }
        if let Some(args) = process
            .args
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            if !line.ends_with(' ') {
                line.push(' ');
            }
            line.push_str(&format!("args={args}"));
        }
        lines.push(line.trim_end().to_string());
        if message.to_ascii_lowercase().contains("raw") {
            if let Some(raw_line) = process
                .raw_line
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("  raw: {raw_line}"));
            }
        }
    }

    if envelope.processes.len() > 5 {
        lines.push(format!("... and {} more.", envelope.processes.len() - 5));
    }

    lines.join("\n")
}

fn format_system_listener_detail_reply(
    message: &str,
    envelope: GroundedSystemListenerDetailEnvelope,
) -> String {
    if !envelope.available {
        return envelope
            .reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|reason| format!("Listener detail is not available on this host. {reason}"))
            .unwrap_or_else(|| "Listener detail is not available on this host.".to_string());
    }

    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();
    let matched_by = envelope
        .matched_by
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("query_contains");
    let mut lines = vec![format!(
        "Listener detail{query_suffix}: {} listener(s) matched by {}.",
        envelope.total_count, matched_by
    )];

    if let Some(observed_at) = envelope
        .observed_at
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Observed at: {observed_at}."));
    }

    for listener in envelope.listeners.iter().take(5) {
        let mut line = String::from("- ");
        if let Some(protocol) = listener
            .protocol
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("{protocol} "));
        }
        if let Some(state) = listener
            .state
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("state={state} "));
        }
        if let Some(local_address) = listener
            .local_address
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!("local={local_address}"));
        }
        if let Some(local_port) = listener.local_port {
            line.push_str(&format!(":{local_port}"));
        }
        if let Some(peer_address) = listener
            .peer_address
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" peer={peer_address}"));
        }
        if let Some(process) = listener
            .process
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" process={process}"));
        }
        if let Some(recv_q) = listener
            .recv_q
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" recv_q={recv_q}"));
        }
        if let Some(send_q) = listener
            .send_q
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            line.push_str(&format!(" send_q={send_q}"));
        }
        lines.push(line.trim_end().to_string());
        if message.to_ascii_lowercase().contains("raw") {
            if let Some(raw_line) = listener
                .raw_line
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("  raw: {raw_line}"));
            }
        }
    }

    if envelope.listeners.len() > 5 {
        lines.push(format!("... and {} more.", envelope.listeners.len() - 5));
    }

    lines.join("\n")
}

fn format_system_disk_usage_detail_reply(
    message: &str,
    envelope: GroundedSystemDiskUsageDetailEnvelope,
) -> String {
    if !envelope.available {
        return envelope
            .reason
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|reason| format!("Disk usage detail is not available on this host. {reason}"))
            .unwrap_or_else(|| "Disk usage detail is not available on this host.".to_string());
    }

    let query_suffix = envelope
        .query
        .as_deref()
        .map(|query| format!(" matching \"{query}\""))
        .unwrap_or_default();
    let matched_by = envelope
        .matched_by
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("contains_match");
    let mut lines = vec![format!(
        "Disk usage detail{query_suffix} on {} matched by {}.",
        envelope
            .mount_point
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("the mounted filesystem"),
        matched_by
    )];

    if let Some(observed_at) = envelope
        .observed_at
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Observed at: {observed_at}."));
    }
    if let Some(source) = envelope
        .source
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Source: {source}."));
    }
    if let Some(fs_type) = envelope
        .fs_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("File system: {fs_type}."));
    }
    if let Some(root) = envelope
        .root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Mount root: {root}."));
    }
    if let Some(mount_id) = envelope.mount_id {
        lines.push(format!("Mount id: {mount_id}."));
    }
    if let Some(parent_id) = envelope.parent_id {
        lines.push(format!("Parent id: {parent_id}."));
    }
    if let Some(major_minor) = envelope
        .major_minor
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Device: {major_minor}."));
    }
    if let Some(options) = envelope
        .options
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Options: {options}."));
    }
    if let Some(super_options) = envelope
        .super_options
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("Super options: {super_options}."));
    }
    if let Some(total_bytes) = envelope.total_bytes {
        lines.push(format!("Total bytes: {total_bytes}."));
    }
    if let Some(free_bytes) = envelope.free_bytes {
        lines.push(format!("Free bytes: {free_bytes}."));
    }
    if let Some(available_bytes) = envelope.available_bytes {
        lines.push(format!("Available bytes: {available_bytes}."));
    }
    if let Some(used_bytes) = envelope.used_bytes {
        lines.push(format!("Used bytes: {used_bytes}."));
    }
    if let Some(used_percent) = envelope.used_percent {
        lines.push(format!("Used percent: {}.", format_decimal(used_percent)));
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw disk payload.".to_string());
    }

    lines.join("\n")
}

fn format_downloads_reply(envelope: GroundedDownloadListEnvelope) -> String {
    let query = envelope
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let availability = envelope
        .availability_filter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let artifacts = envelope.artifacts;
    let reported_count = envelope.total_count.max(artifacts.len());

    if artifacts.is_empty() {
        return match (query, availability) {
            (Some(query), Some(availability)) => {
                format!("No {availability} downloads matched \"{query}\".")
            }
            (Some(query), None) => format!("No downloads matched \"{query}\"."),
            (None, Some(availability)) => format!("No {availability} downloads were found."),
            (None, None) => "No downloads were found.".to_string(),
        };
    }

    let header = match (query, availability, reported_count) {
        (Some(query), Some(availability), 1) => {
            format!("I found 1 {availability} download matching \"{query}\":")
        }
        (Some(query), Some(availability), count) => {
            format!("I found {count} {availability} downloads matching \"{query}\":")
        }
        (Some(query), None, 1) => format!("I found 1 download matching \"{query}\":"),
        (Some(query), None, count) => format!("I found {count} downloads matching \"{query}\":"),
        (None, Some(availability), 1) => format!("I found 1 {availability} download:"),
        (None, Some(availability), count) => format!("I found {count} {availability} downloads:"),
        (None, None, 1) => "I found 1 download:".to_string(),
        (None, None, count) => format!("I found {count} downloads:"),
    };

    let mut lines = vec![header];
    for artifact in artifacts.iter().take(5) {
        lines.push(format!("- {}", format_download_artifact_brief(artifact)));
    }
    if reported_count > 5 {
        lines.push(format!("And {} more.", reported_count - 5));
    }

    lines.join("\n")
}

fn format_download_artifact_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download artifact details for \"{}\":",
        artifact.title
    )];
    if let Some(query) = envelope
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let matched_by = envelope
            .matched_by
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("query");
        lines.push(format!("Matched by {matched_by} for \"{query}\"."));
    }
    lines.push(format!("Artifact id: {}.", artifact.id));
    lines.push(format!("Artifact key: {}.", artifact.artifact_id));
    lines.push(format!(
        "Availability: {}. Platform: {}. Architecture: {}. Channel: {}.",
        artifact.availability, artifact.platform, artifact.architecture, artifact.channel
    ));
    lines.push(format!(
        "Signature status: {}. Distribution mode: {}.",
        artifact.signature_status, artifact.distribution_mode
    ));
    if let Some(version) = artifact
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Version: {version}."));
    }
    if let Some(install_mode) = artifact
        .install_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Install mode: {install_mode}."));
    }
    if let Some(file_size) = artifact
        .file_size
        .and_then(|bytes| u64::try_from(bytes).ok())
    {
        lines.push(format!("Package size: {}.", format_binary_bytes(file_size)));
    }
    if let Some(package_filename) = artifact
        .package_filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Package filename: {package_filename}."));
    }
    if let Some(download_path) = artifact
        .download_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Download path: {download_path}."));
    }
    if let Some(setup_path) = artifact
        .setup_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Setup path: {setup_path}."));
    }
    if let Some(external_url) = artifact
        .external_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("External URL: {external_url}."));
    }
    if let Some(checksum) = artifact
        .checksum
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Checksum: {checksum}."));
    }
    lines.push(if artifact.requires_sign_in {
        "A signed-in host session is required to use this artifact.".to_string()
    } else {
        "This artifact does not require a signed-in host session.".to_string()
    });
    lines.push(format!("Summary: {}.", artifact.summary));
    if artifact.detail.trim() != artifact.summary.trim() {
        lines.push(format!("Details: {}.", artifact.detail));
    }
    if !artifact.install_steps.is_empty() {
        lines.push(format!(
            "Install steps: {}.",
            artifact.install_steps.join(" -> ")
        ));
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_artifact_checksum_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download artifact checksum for \"{}\":",
        artifact.title
    )];
    if let Some(checksum) = artifact
        .checksum
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Checksum: {checksum}."));
    } else {
        lines.push("No checksum was published for this artifact.".to_string());
    }
    if let Some(package_filename) = artifact
        .package_filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Package filename: {package_filename}."));
    }
    lines.push(format!("Signature status: {}.", artifact.signature_status));
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_artifact_install_steps_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download artifact install steps for \"{}\":",
        artifact.title
    )];
    if artifact.install_steps.is_empty() {
        lines.push("No install steps were published for this artifact.".to_string());
    } else {
        for (index, step) in artifact.install_steps.iter().enumerate() {
            lines.push(format!("{}. {}", index + 1, step.trim()));
        }
    }
    if let Some(install_mode) = artifact
        .install_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Install mode: {install_mode}."));
    }
    if let Some(setup_path) = artifact
        .setup_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Setup path: {setup_path}."));
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_artifact_compatibility_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download artifact compatibility for \"{}\":",
        artifact.title
    )];
    lines.push(format!(
        "Platform: {}. Architecture: {}.",
        artifact.platform, artifact.architecture
    ));
    lines.push(format!(
        "Distribution mode: {}.",
        artifact.distribution_mode
    ));
    if let Some(install_mode) = artifact
        .install_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Install mode: {install_mode}."));
    }
    lines.push(if artifact.requires_sign_in {
        "A signed-in host session is required for this artifact.".to_string()
    } else {
        "This artifact does not require a signed-in host session.".to_string()
    });
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_artifact_source_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download artifact source for \"{}\":",
        artifact.title
    )];
    if let Some(query) = envelope
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let matched_by = envelope
            .matched_by
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("query");
        lines.push(format!("Matched by {matched_by} for \"{query}\"."));
    }

    let mut source_bits = Vec::new();
    if let Some(external_url) = artifact
        .external_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source_bits.push(format!("external URL {external_url}"));
    }
    if let Some(download_path) = artifact
        .download_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source_bits.push(format!("download path {download_path}"));
    }
    if let Some(setup_path) = artifact
        .setup_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source_bits.push(format!("setup path {setup_path}"));
    }
    if let Some(package_filename) = artifact
        .package_filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        source_bits.push(format!("package filename {package_filename}"));
    }

    if source_bits.is_empty() {
        lines.push("No dedicated source path or URL was published for this artifact.".to_string());
    } else {
        lines.push(format!("Source: {}.", source_bits.join("; ")));
    }
    lines.push(format!(
        "Distribution mode: {}.",
        artifact.distribution_mode
    ));
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_release_notes_reply(
    message: &str,
    envelope: GroundedDownloadDetailEnvelope,
) -> String {
    let artifact = envelope.artifact;
    let mut lines = vec![format!(
        "Download release notes for \"{}\":",
        artifact.title
    )];
    if let Some(query) = envelope
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let matched_by = envelope
            .matched_by
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("query");
        lines.push(format!("Matched by {matched_by} for \"{query}\"."));
    }
    if artifact.detail.trim().is_empty() {
        lines.push("No separate release-note text was published for this artifact.".to_string());
    } else if artifact.detail.trim() == artifact.summary.trim() {
        lines.push(format!("Release notes: {}.", artifact.detail.trim()));
    } else {
        lines.push(format!("Release notes: {}.", artifact.detail.trim()));
    }
    if let Some(version) = artifact
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Version: {version}."));
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw catalog payload.".to_string());
    }

    lines.join("\n")
}

fn format_download_artifact_brief(artifact: &GroundedDownloadArtifactSummary) -> String {
    let mut parts = vec![artifact.title.trim().to_string()];
    parts.push(format!("[{}]", artifact.availability));
    if let Some(version) = artifact
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("v{version}"));
    }
    parts.push(format!("{}/{}", artifact.platform, artifact.architecture));
    if let Some(install_mode) = artifact
        .install_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(install_mode.to_string());
    }
    if let Some(file_size) = artifact
        .file_size
        .and_then(|bytes| u64::try_from(bytes).ok())
    {
        parts.push(format_binary_bytes(file_size));
    }
    let mut line = parts.join(" ");
    if !artifact.summary.trim().is_empty() {
        line.push_str(": ");
        line.push_str(artifact.summary.trim());
    }
    line
}

fn format_library_detail_reply(message: &str, envelope: GroundedLibraryDetailEnvelope) -> String {
    let mut lines = vec![format!(
        "Library summary for \"{}\" ({}) on {}:",
        envelope.name, envelope.kind, envelope.id
    )];
    if let Some(query) = envelope
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "Matched by {} for \"{query}\".",
            envelope.matched_by
        ));
    }
    lines.push(format!("Item count: {}.", envelope.item_count));

    if envelope.paths.is_empty() {
        lines.push("No library paths were returned.".to_string());
    } else {
        let paths = envelope
            .paths
            .iter()
            .map(|path| {
                let access = if path.is_read_only {
                    "read-only"
                } else {
                    "read-write"
                };
                format!("{} (id={}, {access})", path.path, path.id)
            })
            .collect::<Vec<_>>();
        lines.push(format!("Paths: {}.", paths.join(", ")));
    }

    let settings = envelope.settings;
    lines.push(format!(
        "Settings: images={}, local artwork={}, online artwork={}, TMDb store in media dir={}, TMDb sync on new media={}, schedule={}, posters={}, backdrops={}, metadata={}, reviews={}.",
        on_off(settings.show_images),
        on_off(settings.prefer_local_artwork),
        on_off(settings.fetch_online_artwork),
        on_off(settings.tmdb_store_in_media_dir),
        on_off(settings.tmdb_sync_on_new_media),
        settings.tmdb_sync_schedule,
        on_off(settings.tmdb_fetch_posters),
        on_off(settings.tmdb_fetch_backdrops),
        on_off(settings.tmdb_fetch_metadata),
        on_off(settings.tmdb_fetch_reviews),
    ));
    if let Some(ts) = settings.tmdb_last_sync_ts {
        lines.push(format!("Last TMDb sync: {}.", format_unix_timestamp(ts)));
    } else {
        lines.push("Last TMDb sync: not recorded.".to_string());
    }
    lines.push(format!(
        "Created {} and updated {}.",
        format_unix_timestamp(envelope.created_ts),
        format_unix_timestamp(envelope.updated_ts),
    ));
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw library payload.".to_string());
    }

    lines.join("\n")
}

fn format_library_duplicate_titles_reply(
    message: &str,
    envelope: GroundedLibraryDuplicateTitlesEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Duplicate library titles across accessible libraries: {} groups across {} items.",
        envelope.duplicate_group_count, envelope.total_count
    )];

    if envelope.duplicates.is_empty() {
        lines.push("No duplicate titles were found in the accessible libraries.".to_string());
    } else {
        for duplicate in envelope.duplicates.iter().take(6) {
            lines.push(format!(
                "- {}: {} items in {} libraries ({})",
                duplicate.title,
                duplicate.item_count,
                duplicate.library_count,
                duplicate.libraries.join(", ")
            ));
        }
        if envelope.duplicate_group_count > 6 {
            lines.push(format!(
                "And {} more duplicate title groups.",
                envelope.duplicate_group_count - 6
            ));
        }
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines
            .push("I kept this answer grounded and omitted the raw duplicate payload.".to_string());
    }

    lines.join("\n")
}

fn format_library_missing_metadata_reply(
    message: &str,
    envelope: GroundedLibraryMissingMetadataEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Library items with missing metadata: {} items.",
        envelope.missing_item_count
    )];

    if envelope.items.is_empty() {
        lines.push(
            "No accessible library items were missing the tracked metadata fields.".to_string(),
        );
    } else {
        for item in envelope.items.iter().take(6) {
            let library_name = item.library_name.as_deref().unwrap_or(&item.library_id);
            lines.push(format!(
                "- {} ({}) missing {}",
                item.title,
                library_name,
                item.missing_fields.join(", ")
            ));
        }
        if envelope.missing_item_count > 6 {
            lines.push(format!(
                "And {} more items with missing metadata.",
                envelope.missing_item_count - 6
            ));
        }
    }

    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw metadata payload.".to_string());
    }

    lines.join("\n")
}

fn format_library_search_reply(
    query: Option<&str>,
    envelope: GroundedLibrarySearchEnvelope,
) -> String {
    let query = query.map(str::trim).filter(|value| !value.is_empty());
    let matches = envelope.matches;
    let reported_count = envelope.match_count.max(matches.len());

    if matches.is_empty() {
        return match query {
            Some(query) => format!(
                "No {query} titles were found in your libraries. Would you like to try a different title or search a specific collection?"
            ),
            None => {
                "No matching titles were found in your libraries. Would you like to try a different title or search a specific collection?"
                    .to_string()
            }
        };
    }

    let header = match query {
        Some(query) if reported_count == 1 => {
            format!("I found 1 title matching {query}:")
        }
        Some(query) => format!("I found {} titles matching {query}:", reported_count),
        None if reported_count == 1 => "I found 1 matching title:".to_string(),
        None => format!("I found {} matching titles:", reported_count),
    };

    let mut lines = vec![header];
    for item in matches.iter().take(5) {
        lines.push(format!("- {}", format_library_match(item)));
    }
    if reported_count > 5 {
        lines.push(format!("And {} more.", reported_count - 5));
    }

    lines.join("\n")
}

fn format_library_item_detail_reply(
    message: &str,
    envelope: GroundedLibraryItemDetailEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Library item summary for \"{}\" from {}:",
        envelope.title, envelope.library_id
    )];
    if let Some(library_name) = envelope
        .library_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Library: {library_name}."));
    }
    let mut item_bits = vec![format!("Kind: {}", envelope.kind)];
    if let Some(year) = envelope.year {
        item_bits.push(format!("Year: {year}"));
    }
    if let Some(duration_ms) = envelope.duration_ms {
        item_bits.push(format!("Duration: {} minutes", duration_ms / 60_000));
    }
    lines.push(item_bits.join(". "));
    lines.push(format!("Item id: {}.", envelope.id));
    if let Some(overview) = envelope
        .overview
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Overview: {overview}."));
    } else {
        lines.push("No overview was returned for this item.".to_string());
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw item payload.".to_string());
    }

    lines.join("\n")
}

fn format_library_item_media_reply(
    message: &str,
    envelope: GroundedLibraryItemMediaEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Library media details for \"{}\" from {}:",
        envelope.title, envelope.library_id
    )];
    if let Some(library_name) = envelope
        .library_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Library: {library_name}."));
    }
    lines.push(format!("Item id: {}.", envelope.id));
    lines.push(format!(
        "Matched by {} for \"{}\".",
        envelope.matched_by, envelope.query
    ));

    let mut item_bits = vec![format!("Kind: {}", envelope.kind)];
    if let Some(year) = envelope.year {
        item_bits.push(format!("Year: {year}"));
    }
    if let Some(duration_ms) = envelope.duration_ms {
        item_bits.push(format!("Duration: {} minutes", duration_ms / 60_000));
    }
    if let Some(parent_id) = envelope
        .parent_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        item_bits.push(format!("Parent id: {parent_id}"));
    }
    lines.push(item_bits.join(". "));

    if let Some(resolved_media_path) = envelope
        .resolved_media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Resolved media path: {resolved_media_path}."));
    } else {
        lines.push("Resolved media path: not available.".to_string());
    }

    if let Some(media_path) = envelope
        .media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Direct media path: {media_path}."));
    }
    if let Some(first_descendant_media_path) = envelope
        .first_descendant_media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "First descendant media path: {first_descendant_media_path}."
        ));
    }
    if !envelope.source_paths.is_empty() {
        lines.push(format!(
            "Source paths: {}.",
            envelope.source_paths.join(", ")
        ));
    }

    let mut artwork_bits = Vec::new();
    if let Some(poster_url) = envelope
        .poster_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        artwork_bits.push(format!("poster={poster_url}"));
    }
    if let Some(backdrop_url) = envelope
        .backdrop_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        artwork_bits.push(format!("backdrop={backdrop_url}"));
    }
    if let Some(logo_url) = envelope
        .logo_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        artwork_bits.push(format!("logo={logo_url}"));
    }
    if let Some(thumb_url) = envelope
        .thumb_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        artwork_bits.push(format!("thumb={thumb_url}"));
    }
    if artwork_bits.is_empty() {
        lines.push("Artwork: no artwork URLs were returned.".to_string());
    } else {
        lines.push(format!("Artwork: {}.", artwork_bits.join(", ")));
    }

    if let Some(overview) = envelope
        .overview
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Overview: {overview}."));
    }
    lines.push(format!(
        "Created {} and updated {}.",
        format_unix_timestamp(envelope.created_ts),
        format_unix_timestamp(envelope.updated_ts),
    ));
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw item payload.".to_string());
    }

    lines.join("\n")
}

fn format_library_item_source_paths_reply(
    message: &str,
    envelope: GroundedLibraryItemMediaEnvelope,
) -> String {
    let mut lines = vec![format!(
        "Library item source paths for \"{}\" from {}:",
        envelope.title, envelope.library_id
    )];
    if let Some(library_name) = envelope
        .library_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Library: {library_name}."));
    }
    lines.push(format!("Item id: {}.", envelope.id));
    lines.push(format!(
        "Matched by {} for \"{}\".",
        envelope.matched_by, envelope.query
    ));

    if !envelope.source_paths.is_empty() {
        lines.push(format!(
            "Source paths: {}.",
            envelope.source_paths.join(", ")
        ));
    }

    if envelope.source_paths.is_empty() {
        lines.push("No source paths were returned for this item.".to_string());
    } else {
        for path in envelope.source_paths.iter().take(6) {
            lines.push(format!("- {path}"));
        }
        if envelope.source_paths.len() > 6 {
            lines.push(format!("... and {} more.", envelope.source_paths.len() - 6));
        }
    }

    if let Some(resolved_media_path) = envelope
        .resolved_media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Resolved media path: {resolved_media_path}."));
    }
    if let Some(media_path) = envelope
        .media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Direct media path: {media_path}."));
    }
    if let Some(first_descendant_media_path) = envelope
        .first_descendant_media_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "First descendant media path: {first_descendant_media_path}."
        ));
    }
    if message.to_ascii_lowercase().contains("raw") {
        lines.push("I kept this answer grounded and omitted the raw item payload.".to_string());
    }

    lines.join("\n")
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn format_unix_timestamp(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|value| value.format("%A, %B %-d, %Y %H:%M UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn format_library_match(item: &GroundedLibrarySearchMatch) -> String {
    let mut parts = vec![item.title.trim().to_string()];
    if let Some(year) = item.year {
        parts.push(format!("({year})"));
    }
    if let Some(kind) = item
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("[{kind}]"));
    }
    if let Some(library_name) = item
        .library_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("in {library_name}"));
    }
    parts.join(" ")
}

fn is_notable_user_facing_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !(lower == "docker0"
        || lower.starts_with("br-")
        || lower.starts_with("veth")
        || lower.starts_with("virbr")
        || lower.starts_with("cni")
        || lower.starts_with("flannel")
        || lower.starts_with("podman"))
}

fn birthday_display_name(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    if lower.ends_with(" birthday") {
        return title[..title.len().saturating_sub(" birthday".len())]
            .trim()
            .to_string();
    }
    title.to_string()
}

fn birthday_turning_age(birthday: &GroundedBirthdaySummary) -> Option<i32> {
    let next_date = parse_ymd(&birthday.next_occurs_on)?;
    let birth_year = birthday.birthday_year?;
    Some(next_date.year() - birth_year)
}

fn scope_label(scope: &str) -> &'static str {
    if scope == "global" {
        "the shared calendar"
    } else {
        "your personal calendar"
    }
}

fn describe_relative_timing(date: NaiveDate) -> String {
    let today = super::dates::assistant_local_today();
    let days = (date - today).num_days();
    match days {
        i64::MIN..=-1 => String::new(),
        0 => " That is today.".to_string(),
        1 => " That is tomorrow.".to_string(),
        value => format!(" That is in {value} days."),
    }
}

fn parse_ymd(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn format_with_weekday(date: NaiveDate) -> String {
    date.format("%A, %B %-d, %Y").to_string()
}

fn format_calendar_error(block: &AssistantToolContextBlock, fallback: &str) -> String {
    block
        .data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(|message| format!("{fallback} {message}"))
        .unwrap_or_else(|| fallback.to_string())
}

fn format_tool_error(block: &AssistantToolContextBlock, fallback: &str) -> String {
    block
        .data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(|message| format!("{fallback} {message}"))
        .unwrap_or_else(|| fallback.to_string())
}

fn extract_library_search_query(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("access to") {
        return None;
    }

    if let Some(quoted) = extract_quoted_phrase(message) {
        if lower.contains("search")
            || lower.contains("find")
            || lower.contains("look for")
            || lower.contains("is there")
            || lower.contains("library")
            || lower.contains("libraries")
        {
            return Some(quoted);
        }
    }

    None
}

fn extract_quoted_phrase(message: &str) -> Option<String> {
    let start = message.find('"')?;
    let rest = &message[start + 1..];
    let end = rest.find('"')?;
    let candidate = rest[..end].trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        deterministic_ai_runtime_reply, deterministic_calendar_reply,
        deterministic_dictionary_reply, deterministic_downloads_reply, deterministic_library_reply,
        deterministic_memory_reply, deterministic_network_reply, deterministic_system_reply,
        deterministic_web_reply, grounding_chunks_prompt, rank_and_compress_grounding_chunks,
    };
    use crate::ai_assistant::types::{
        AssistantGroundingChunk, AssistantGroundingCitation, AssistantGroundingVisibility,
        AssistantToolContextBlock,
    };
    use serde_json::json;

    fn chunk(id: &str, score: f64, title: &str, excerpt: &str) -> AssistantGroundingChunk {
        AssistantGroundingChunk {
            id: id.to_string(),
            source_kind: "transcript".to_string(),
            title: title.to_string(),
            excerpt: excerpt.to_string(),
            score,
            visibility: AssistantGroundingVisibility::User,
            topic_key: Some("topic".to_string()),
            owner_user_id: Some("user-1".to_string()),
            source_id: Some("source".to_string()),
            source_sub_id: Some("sub".to_string()),
            citation: Some(AssistantGroundingCitation {
                citation_id: format!("cite-{id}"),
                source_kind: "transcript".to_string(),
                source_id: "source".to_string(),
                source_sub_id: Some("sub".to_string()),
                label: Some(title.to_string()),
                excerpt: Some(excerpt.to_string()),
                started_ts_ms: Some(1000),
                ended_ts_ms: Some(2000),
                url: None,
            }),
        }
    }

    #[test]
    fn compresses_and_sorts_by_score() {
        let ranked = rank_and_compress_grounding_chunks(
            &[
                chunk("b", 0.2, "B", "b"),
                chunk("a", 0.9, "A", "a"),
                chunk("a", 0.1, "A dup", "dup"),
            ],
            10,
            10_000,
        );
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].id, "a");
        assert_eq!(ranked[1].id, "b");
    }

    #[test]
    fn prompt_renders_stable_ids() {
        let prompt = grounding_chunks_prompt(&[chunk("a", 0.9, "Alpha", "This is an excerpt.")]);
        assert!(prompt.contains("[a]"));
        assert!(prompt.contains("kind=transcript"));
        assert!(prompt.contains("excerpt=This is an excerpt."));
    }

    #[test]
    fn deterministic_calendar_reply_formats_next_event_details() {
        let reply = deterministic_calendar_reply(
            "What is the next thing coming up in my calendar?",
            &[AssistantToolContextBlock {
                tool: "calendar_get_next_event",
                label: "Next visible calendar event".to_string(),
                status: "ok",
                data: json!({
                    "next_event": {
                        "title": "Dentist appointment",
                        "event_type": "event",
                        "scope": "personal",
                        "next_occurs_on": "2026-08-14"
                    }
                }),
            }],
        )
        .expect("expected deterministic next-event reply");

        assert!(reply.contains("Dentist appointment"));
        assert!(reply.contains("Friday, August 14, 2026"));
    }

    #[test]
    fn deterministic_calendar_reply_formats_next_event_timing() {
        let reply = deterministic_calendar_reply(
            "How long until my next event?",
            &[AssistantToolContextBlock {
                tool: "calendar_get_next_event_timing",
                label: "Next visible calendar event timing".to_string(),
                status: "ok",
                data: json!({
                    "today": "2026-04-02",
                    "days_until": 3,
                    "next_event": {
                        "title": "Dentist appointment",
                        "event_type": "event",
                        "scope": "personal",
                        "next_occurs_on": "2026-04-05"
                    }
                }),
            }],
        )
        .expect("expected deterministic next-event timing reply");

        assert!(reply.contains("Dentist appointment"));
        assert!(reply.contains("Sunday, April 5, 2026"));
        assert!(reply.contains("in 3 days"));
    }

    #[test]
    fn deterministic_calendar_reply_formats_event_count() {
        let reply = deterministic_calendar_reply(
            "How many events do I have next week?",
            &[AssistantToolContextBlock {
                tool: "calendar_count_events",
                label: "Visible calendar event counts for next week".to_string(),
                status: "ok",
                data: json!({
                    "window": { "from": "2026-04-06", "to": "2026-04-12", "label": "next week" },
                    "total_event_count": 3,
                    "busy_day_count": 2,
                    "day_counts": [
                        { "date": "2026-04-07", "event_count": 2 },
                        { "date": "2026-04-09", "event_count": 1 }
                    ]
                }),
            }],
        )
        .expect("expected deterministic event-count reply");

        assert!(reply.contains("3 visible calendar events in next week across 2 busy days"));
        assert!(reply.contains("The busiest day is Tuesday, April 7, 2026 with 2 events."));
    }

    #[test]
    fn deterministic_calendar_reply_formats_busy_days() {
        let reply = deterministic_calendar_reply(
            "Which days are busiest next week?",
            &[AssistantToolContextBlock {
                tool: "calendar_list_busy_days",
                label: "Visible calendar busy days for next week".to_string(),
                status: "ok",
                data: json!({
                    "window": { "from": "2026-04-06", "to": "2026-04-12", "label": "next week" },
                    "total_event_count": 3,
                    "busy_day_count": 2,
                    "busy_days": [
                        {
                            "date": "2026-04-07",
                            "event_count": 2,
                            "events": [
                                {
                                    "title": "Dentist appointment",
                                    "event_date": "2026-04-07",
                                    "occurs_on": "2026-04-07",
                                    "scope": "personal",
                                    "event_type": "event",
                                    "owner_username": "tester"
                                },
                                {
                                    "title": "Team standup",
                                    "event_date": "2026-04-07",
                                    "occurs_on": "2026-04-07",
                                    "scope": "global",
                                    "event_type": "event",
                                    "owner_username": null
                                }
                            ]
                        },
                        {
                            "date": "2026-04-09",
                            "event_count": 1,
                            "events": [
                                {
                                    "title": "Lunch with Sam",
                                    "event_date": "2026-04-09",
                                    "occurs_on": "2026-04-09",
                                    "scope": "personal",
                                    "event_type": "event",
                                    "owner_username": "tester"
                                }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic busy-days reply");

        assert!(reply.contains("Busy visible calendar days in next week"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
        assert!(reply.contains("Dentist appointment"));
        assert!(reply.contains("Team standup"));
        assert!(reply.contains("Lunch with Sam"));
    }

    #[test]
    fn deterministic_calendar_reply_formats_conflicts() {
        let reply = deterministic_calendar_reply(
            "Do I have any calendar conflicts next Tuesday?",
            &[AssistantToolContextBlock {
                tool: "calendar_list_date_conflicts",
                label: "Visible calendar conflicts for next Tuesday".to_string(),
                status: "ok",
                data: json!({
                    "window": { "from": "2026-04-07", "to": "2026-04-07", "label": "next Tuesday" },
                    "total_event_count": 2,
                    "conflict_day_count": 1,
                    "conflict_days": [
                        {
                            "date": "2026-04-07",
                            "event_count": 2,
                            "events": [
                                {
                                    "title": "Dentist appointment",
                                    "event_date": "2026-04-07",
                                    "occurs_on": "2026-04-07",
                                    "scope": "personal",
                                    "event_type": "event",
                                    "owner_username": "tester"
                                },
                                {
                                    "title": "Team standup",
                                    "event_date": "2026-04-07",
                                    "occurs_on": "2026-04-07",
                                    "scope": "global",
                                    "event_type": "event",
                                    "owner_username": null
                                }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic conflict reply");

        assert!(reply.contains("Visible calendar conflicts in next Tuesday"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
        assert!(reply.contains("Dentist appointment"));
        assert!(reply.contains("Team standup"));
    }

    #[test]
    fn deterministic_calendar_reply_formats_free_days() {
        let reply = deterministic_calendar_reply(
            "What days are free next week?",
            &[AssistantToolContextBlock {
                tool: "calendar_list_free_days",
                label: "Visible calendar free days for next week".to_string(),
                status: "ok",
                data: json!({
                    "window": { "from": "2026-04-06", "to": "2026-04-12", "label": "next week" },
                    "occupied_day_count": 2,
                    "free_day_count": 3,
                    "free_days": [
                        { "date": "2026-04-07" },
                        { "date": "2026-04-09" },
                        { "date": "2026-04-11" }
                    ]
                }),
            }],
        )
        .expect("expected deterministic free-days reply");

        assert!(reply.contains("Free visible calendar days in next week"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
        assert!(reply.contains("Thursday, April 9, 2026"));
        assert!(reply.contains("Saturday, April 11, 2026"));
    }

    #[test]
    fn deterministic_calendar_reply_lists_birthday_details() {
        let reply = deterministic_calendar_reply(
            "Whose birthdays are coming up?",
            &[AssistantToolContextBlock {
                tool: "calendar_upcoming_birthdays",
                label: "Upcoming birthdays".to_string(),
                status: "ok",
                data: json!({
                    "window": { "label": "the next 30 days" },
                    "query": null,
                    "birthdays": [
                        {
                            "title": "Rachel birthday",
                            "next_occurs_on": "2026-04-07",
                            "birthday_year": 2003
                        },
                        {
                            "title": "Sam birthday",
                            "next_occurs_on": "2026-04-12",
                            "birthday_year": 2000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic birthday reply");

        assert!(reply.contains("Rachel"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
        assert!(reply.contains("turns 23"));
        assert!(reply.contains("Sam"));
    }

    #[test]
    fn deterministic_calendar_reply_uses_first_birthday_for_next_birthday_questions() {
        let reply = deterministic_calendar_reply(
            "What's the next birthday in my calendar?",
            &[AssistantToolContextBlock {
                tool: "calendar_upcoming_birthdays",
                label: "Upcoming birthdays for the next 366 days".to_string(),
                status: "ok",
                data: json!({
                    "window": { "label": "the next 366 days" },
                    "query": null,
                    "birthdays": [
                        {
                            "title": "Rachel birthday",
                            "next_occurs_on": "2026-04-07",
                            "birthday_year": 2003
                        },
                        {
                            "title": "Sam birthday",
                            "next_occurs_on": "2026-04-12",
                            "birthday_year": 2000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic next birthday reply");

        assert!(reply.contains("Rachel"));
        assert!(reply.contains("Tuesday, April 7, 2026"));
        assert!(!reply.contains("Sam"));
    }

    #[test]
    fn deterministic_dictionary_reply_formats_relationship_birthday() {
        let reply = deterministic_dictionary_reply(
            "When is my mother's birthday?",
            &[AssistantToolContextBlock {
                tool: "dictionary_resolve_relationship_reference",
                label: "Human Dictionary relationship reference".to_string(),
                status: "ok",
                data: json!({
                    "reference": "my mother",
                    "relation_kind": "mother",
                    "workspace_id": "family-workspace",
                    "workspace_title": "Family",
                    "status": "resolved",
                    "message": null,
                    "linked_person_id": "person-self",
                    "linked_person_name": "Iwan",
                    "candidates": [
                        {
                            "person_id": "person-mary",
                            "display_name": "Mary",
                            "summary": "Loves gardening and baking.",
                            "relation_type": "child_of",
                            "birthday": "1974-06-09",
                            "hobbies": ["gardening", "baking"],
                            "document_excerpt": "Prefers lilies and dahlias."
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic dictionary reply");

        assert!(reply.contains("Your mother Mary"));
        assert!(reply.contains("Sunday, June 9, 1974"));
    }

    #[test]
    fn deterministic_dictionary_reply_formats_relationship_hobbies() {
        let reply = deterministic_dictionary_reply(
            "What are my brother's hobbies?",
            &[AssistantToolContextBlock {
                tool: "dictionary_resolve_relationship_reference",
                label: "Human Dictionary relationship reference".to_string(),
                status: "ok",
                data: json!({
                    "reference": "my brother",
                    "relation_kind": "brother",
                    "workspace_id": "family-workspace",
                    "workspace_title": "Family",
                    "status": "resolved",
                    "message": null,
                    "linked_person_id": "person-self",
                    "linked_person_name": "Iwan",
                    "candidates": [
                        {
                            "person_id": "person-john",
                            "display_name": "John",
                            "summary": "Always tinkering with bikes.",
                            "relation_type": "sibling_of",
                            "birthday": null,
                            "hobbies": ["cycling", "chess"],
                            "document_excerpt": null
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic dictionary reply");

        assert!(reply.contains("Your brother John"));
        assert!(reply.contains("cycling, chess"));
    }

    #[test]
    fn deterministic_dictionary_reply_formats_relationship_lists() {
        let reply = deterministic_dictionary_reply(
            "Who are my co-workers?",
            &[AssistantToolContextBlock {
                tool: "dictionary_resolve_relationship_reference",
                label: "Human Dictionary relationship reference".to_string(),
                status: "ok",
                data: json!({
                    "reference": "my co-workers",
                    "relation_kind": "coworker",
                    "workspace_id": "work-workspace",
                    "workspace_title": "Work",
                    "status": "list",
                    "message": null,
                    "linked_person_id": "person-self",
                    "linked_person_name": "Iwan",
                    "candidates": [
                        {
                            "person_id": "person-a",
                            "display_name": "Alice",
                            "summary": "Frontend engineer on the payments team.",
                            "relation_type": "coworker_of",
                            "birthday": null,
                            "hobbies": [],
                            "document_excerpt": null
                        },
                        {
                            "person_id": "person-b",
                            "display_name": "Brian",
                            "summary": "Backend engineer focused on infra.",
                            "relation_type": "coworker_of",
                            "birthday": null,
                            "hobbies": [],
                            "document_excerpt": null
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic dictionary reply");

        assert!(reply.contains("your co-workers in Work"));
        assert!(reply.contains("Alice"));
        assert!(reply.contains("Brian"));
    }

    #[test]
    fn deterministic_dictionary_reply_formats_visible_workspaces() {
        let reply = deterministic_dictionary_reply(
            "Show me my dictionary workspaces",
            &[AssistantToolContextBlock {
                tool: "dictionary_list_visible_workspaces",
                label: "Visible Human Dictionary workspaces".to_string(),
                status: "ok",
                data: json!({
                    "workspaces": [
                        {
                            "workspace_id": "ws-family",
                            "title": "Family",
                            "workspace_kind": "family_shared",
                            "owner_user_id": "user-1",
                            "is_system_seeded": true
                        },
                        {
                            "workspace_id": "ws-work",
                            "title": "Work",
                            "workspace_kind": "work_private",
                            "owner_user_id": "user-1",
                            "is_system_seeded": true
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic dictionary reply");

        assert!(reply.contains("Visible Human Dictionary workspaces"));
        assert!(reply.contains("Family (family)"));
        assert!(reply.contains("Work (work)"));
    }

    #[test]
    fn deterministic_dictionary_reply_formats_workspace_people() {
        let reply = deterministic_dictionary_reply(
            "Find Rachel in my work dictionary",
            &[AssistantToolContextBlock {
                tool: "dictionary_browse_workspace_people",
                label: "Visible Human Dictionary people in workspace".to_string(),
                status: "ok",
                data: json!({
                    "workspace_id": "ws-work",
                    "workspace_title": "Work",
                    "query": "Rachel",
                    "people": [
                        {
                            "id": "person-rachel",
                            "display_name": "Rachel",
                            "canonical_name": "Rachel Murphy",
                            "summary": "Backend engineer on the infra team."
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic dictionary reply");

        assert!(reply.contains("Visible Human Dictionary people in Work matching \"Rachel\""));
        assert!(reply.contains("Rachel (Rachel Murphy)"));
    }

    #[test]
    fn deterministic_network_reply_prefers_local_connect_url() {
        let reply = deterministic_network_reply(
            "If I was on the local network, what IP would I use to connect to Rustyfin?",
            &[AssistantToolContextBlock {
                tool: "network_get_topology_summary",
                label: "Rustyfin network topology summary".to_string(),
                status: "ok",
                data: json!({
                    "host_label": "server",
                    "public_host": "192.168.0.36",
                    "remote_access_enabled": true,
                    "access": {
                        "ui_port": 3008,
                        "backend_port": 8097,
                        "calendar_port": 8099,
                        "preferred_local_interface": "enp3s0",
                        "preferred_local_ipv4": "192.168.0.36",
                        "preferred_local_url": "https://192.168.0.36:3008",
                        "login_url": "https://192.168.0.36:3008/login",
                        "ai_url": "https://192.168.0.36:3008/ai",
                        "public_url": "https://192.168.0.36:3008"
                    },
                    "nodes": [
                        {
                            "name": "enp3s0",
                            "status": "online",
                            "addresses": [
                                { "family": "inet", "address": "192.168.0.36" }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic network reply");

        assert!(reply.contains("https://192.168.0.36:3008"));
        assert!(reply.contains("preferred local IP is 192.168.0.36"));
        assert!(reply.contains("primary LAN interface is enp3s0"));
        assert!(reply.contains("port is 3008"));
        assert!(reply.contains("8097"));
    }

    #[test]
    fn deterministic_network_reply_formats_interface_by_ip_details() {
        let reply = deterministic_network_reply(
            "Which interface owns 192.168.0.36?",
            &[AssistantToolContextBlock {
                tool: "network_get_interface_by_ip",
                label: "Network interface for IP \"192.168.0.36\"".to_string(),
                status: "ok",
                data: json!({
                    "query": "192.168.0.36",
                    "matched_by": "exact_address",
                    "host_label": "server",
                    "remote_access_enabled": true,
                    "access": {
                        "ui_port": 3008,
                        "backend_port": 8097,
                        "calendar_port": 8099,
                        "preferred_local_interface": "enp3s0",
                        "preferred_local_ipv4": "192.168.0.36",
                        "preferred_local_url": "https://192.168.0.36:3008",
                        "login_url": "https://192.168.0.36:3008/login",
                        "ai_url": "https://192.168.0.36:3008/ai",
                        "public_url": "https://192.168.0.36:3008"
                    },
                    "interface": {
                        "name": "enp3s0",
                        "status": "online",
                        "is_loopback": false,
                        "addresses": [
                            { "family": "inet", "address": "192.168.0.36" }
                        ]
                    }
                }),
            }],
        )
        .expect("expected deterministic interface-by-ip reply");

        assert!(reply.contains("Network interface details for \"192.168.0.36\""));
        assert!(reply.contains("enp3s0"));
        assert!(reply.contains("192.168.0.36"));
        assert!(!reply.contains('{'));
    }

    #[test]
    fn deterministic_network_reply_formats_default_route_details() {
        let reply = deterministic_network_reply(
            "What is the default route on this machine?",
            &[AssistantToolContextBlock {
                tool: "network_get_default_route",
                label: "Default route".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "matched_by": "default_route",
                    "total_count": 1,
                    "routes": [
                        {
                            "route": "default via 192.168.0.1 dev enp3s0 src 192.168.0.36 metric 100",
                            "gateway": "192.168.0.1",
                            "interface": "enp3s0",
                            "source": "192.168.0.36",
                            "metric": 100,
                            "protocol": "dhcp",
                            "scope": "global"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic default route reply");

        assert!(reply.contains("Default route"));
        assert!(reply.contains("192.168.0.1"));
        assert!(reply.contains("enp3s0"));
        assert!(reply.contains("192.168.0.36"));
    }

    #[test]
    fn deterministic_network_reply_formats_hostname_aliases() {
        let reply = deterministic_network_reply(
            "What hostname aliases does this host have?",
            &[AssistantToolContextBlock {
                tool: "network_get_hostname_aliases",
                label: "Hostname aliases".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "matched_by": "all_aliases",
                    "host_label": "server",
                    "canonical_hostname": "server",
                    "fqdn": "server.local",
                    "total_count": 2,
                    "aliases": [
                        { "name": "rustyfin", "source": "hostname -a" },
                        { "name": "server-lan", "source": "/etc/hosts" }
                    ]
                }),
            }],
        )
        .expect("expected deterministic hostname alias reply");

        assert!(reply.contains("Hostname aliases"));
        assert!(reply.contains("Canonical hostname: server"));
        assert!(reply.contains("server.local"));
        assert!(reply.contains("rustyfin"));
        assert!(reply.contains("server-lan"));
    }

    #[test]
    fn deterministic_network_reply_formats_dns_servers() {
        let reply = deterministic_network_reply(
            "What DNS servers does this host use?",
            &[AssistantToolContextBlock {
                tool: "network_get_dns_servers",
                label: "DNS servers".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "matched_by": "dns_resolvers",
                    "total_count": 2,
                    "dns_servers": [
                        {
                            "scope": "global",
                            "interface": null,
                            "server": "1.1.1.1",
                            "source": "resolvectl",
                            "raw_line": "DNS Servers: 1.1.1.1 8.8.8.8"
                        },
                        {
                            "scope": "link 2 (enp3s0)",
                            "interface": "enp3s0",
                            "server": "8.8.8.8",
                            "source": "resolvectl",
                            "raw_line": "DNS Servers: 1.1.1.1 8.8.8.8"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic DNS server reply");

        assert!(reply.contains("DNS servers"));
        assert!(reply.contains("1.1.1.1"));
        assert!(reply.contains("8.8.8.8"));
        assert!(reply.contains("enp3s0"));
    }

    #[test]
    fn deterministic_system_reply_formats_storage_path_details_without_raw_json() {
        let reply = deterministic_system_reply(
            "How much space is on the AI model dir?",
            &[AssistantToolContextBlock {
                tool: "system_get_storage_path_detail",
                label: "Storage path detail for ai_model_dir".to_string(),
                status: "ok",
                data: json!({
                    "query": "ai_model_dir",
                    "matched_by": "name exact match",
                    "name": "ai_model_dir",
                    "path": "/var/lib/rustyfin/ai/models",
                    "exists": true,
                    "resolved_path": "/var/lib/rustyfin/ai/models",
                    "stats_path": "/var/lib/rustyfin/ai/models",
                    "mount_point": "/var/lib",
                    "mount_file_system": "ext4",
                    "mount_source": "/dev/sda1",
                    "total_bytes": 1000000,
                    "total_human": "976.6 KiB",
                    "available_bytes": 250000,
                    "available_human": "244.1 KiB",
                    "used_bytes": 750000,
                    "used_human": "732.4 KiB",
                    "used_percent": 75.0
                }),
            }],
        )
        .expect("expected deterministic storage path reply");

        assert!(reply.contains("Storage path detail for \"ai_model_dir\""));
        assert!(reply.contains("/var/lib/rustyfin/ai/models"));
        assert!(reply.contains("Mount point"));
        assert!(reply.contains("Usage"));
        assert!(!reply.contains("\"mount_point\""));
    }

    #[test]
    fn deterministic_system_reply_formats_mount_details() {
        let reply = deterministic_system_reply(
            "What filesystem is mounted on /var/lib/rustyfin/ai/models?",
            &[AssistantToolContextBlock {
                tool: "system_get_mount_detail",
                label: "Storage mount detail for /var/lib".to_string(),
                status: "ok",
                data: json!({
                    "query": "/var/lib/rustyfin/ai/models",
                    "matched_by": "tracked_path exact match",
                    "total_count": 1,
                    "mount_point": "/var/lib",
                    "mount_file_system": "ext4",
                    "mount_source": "/dev/sda1",
                    "tracked_paths": ["/var/lib/rustyfin/ai/models", "/var/lib/rustyfin/cache"],
                    "total_bytes": 1000000,
                    "total_human": "976.6 KiB",
                    "available_bytes": 250000,
                    "available_human": "244.1 KiB",
                    "used_bytes": 750000,
                    "used_human": "732.4 KiB",
                    "used_percent": 75.0
                }),
            }],
        )
        .expect("expected deterministic mount detail reply");

        assert!(reply.contains("Storage mount detail"));
        assert!(reply.contains("/var/lib"));
        assert!(reply.contains("Tracked paths"));
        assert!(reply.contains("Usage"));
        assert!(!reply.contains("\"mount_point\""));
    }

    #[test]
    fn deterministic_system_reply_formats_process_details_without_raw_json() {
        let reply = deterministic_system_reply(
            "Which process is using node on the host?",
            &[AssistantToolContextBlock {
                tool: "system_get_process_detail",
                label: "Process detail for node".to_string(),
                status: "ok",
                data: json!({
                    "available": true,
                    "observed_at": "2026-04-06T10:00:00Z",
                    "query": "node",
                    "matched_by": "query_contains",
                    "total_count": 1,
                    "processes": [
                        {
                            "pid": 123,
                            "ppid": 1,
                            "user": "root",
                            "state": "S",
                            "cpu_percent": 1.5,
                            "mem_percent": 2.2,
                            "elapsed_secs": 3600,
                            "command": "node",
                            "args": "server.js",
                            "raw_line": "123 1 root S 1.5 2.2 3600 node server.js"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic process detail reply");

        assert!(reply.contains("Process detail"));
        assert!(reply.contains("pid=123"));
        assert!(reply.contains("user=root"));
        assert!(reply.contains("command=node"));
        assert!(!reply.contains("\"processes\""));
    }

    #[test]
    fn deterministic_system_reply_formats_listener_details_without_raw_json() {
        let reply = deterministic_system_reply(
            "What is listening on port 3008?",
            &[AssistantToolContextBlock {
                tool: "system_get_listener_detail",
                label: "Listener detail for 3008".to_string(),
                status: "ok",
                data: json!({
                    "available": true,
                    "observed_at": "2026-04-06T10:00:00Z",
                    "query": "3008",
                    "matched_by": "port_exact",
                    "total_count": 1,
                    "listeners": [
                        {
                            "protocol": "tcp",
                            "state": "LISTEN",
                            "recv_q": "0",
                            "send_q": "0",
                            "local_address": "127.0.0.1",
                            "local_port": 3008,
                            "peer_address": null,
                            "process": "users:(\"node\",pid=123,fd=10)",
                            "raw_line": "tcp LISTEN 0 0 127.0.0.1:3008 *:* users:(\"node\",pid=123,fd=10)"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic listener detail reply");

        assert!(reply.contains("Listener detail"));
        assert!(reply.contains("local=127.0.0.1:3008"));
        assert!(reply.contains("process=users"));
        assert!(!reply.contains("\"listeners\""));
    }

    #[test]
    fn deterministic_system_reply_formats_disk_usage_details_without_raw_json() {
        let reply = deterministic_system_reply(
            "How full is /var/lib?",
            &[AssistantToolContextBlock {
                tool: "system_get_disk_usage_detail",
                label: "Disk usage detail for /var/lib".to_string(),
                status: "ok",
                data: json!({
                    "available": true,
                    "observed_at": "2026-04-06T10:00:00Z",
                    "query": "/var/lib",
                    "matched_by": "mount_point_exact",
                    "mount_point": "/var/lib",
                    "source": "/dev/sda1",
                    "fs_type": "ext4",
                    "root": "/",
                    "mount_id": 42,
                    "parent_id": 1,
                    "major_minor": "8:1",
                    "options": "rw,relatime",
                    "super_options": "errors=remount-ro",
                    "total_bytes": 1000000,
                    "free_bytes": 100000,
                    "available_bytes": 75000,
                    "used_bytes": 900000,
                    "used_percent": 90.0
                }),
            }],
        )
        .expect("expected deterministic disk usage detail reply");

        assert!(reply.contains("Disk usage detail"));
        assert!(reply.contains("/var/lib"));
        assert!(reply.contains("File system: ext4"));
        assert!(reply.contains("Used percent: 90"));
        assert!(!reply.contains("\"mount_id\""));
    }

    #[test]
    fn deterministic_system_reply_formats_port_conflict_detail() {
        let reply = deterministic_system_reply(
            "What process is using port 3008?",
            &[AssistantToolContextBlock {
                tool: "system_get_port_conflict_detail",
                label: "Port conflict detail for 3008".to_string(),
                status: "ok",
                data: json!({
                    "query": "3008",
                    "matched_by": "port exact",
                    "total_count": 1,
                    "protocol": "tcp",
                    "state": "LISTEN",
                    "local_address": "127.0.0.1",
                    "local_port": 3008,
                    "peer_address": "0.0.0.0:*",
                    "raw_entry": "LISTEN 0 4096 127.0.0.1:3008 0.0.0.0:* users:(\"node\",pid=123,fd=10)",
                    "processes": [
                        { "name": "node", "pid": 123, "fd": 10 }
                    ]
                }),
            }],
        )
        .expect("expected deterministic port conflict detail reply");

        assert!(reply.contains("Port conflict detail"));
        assert!(reply.contains("TCP 127.0.0.1:3008"));
        assert!(reply.contains("LISTEN"));
        assert!(reply.contains("node"));
        assert!(!reply.contains("\"local_port\""));
    }

    #[test]
    fn deterministic_system_reply_formats_port_conflicts() {
        let reply = deterministic_system_reply(
            "Which ports are in use?",
            &[AssistantToolContextBlock {
                tool: "system_get_port_conflicts",
                label: "Port conflicts".to_string(),
                status: "ok",
                data: json!({
                    "query": "3008",
                    "matched_by": "port_exact",
                    "total_count": 1,
                    "conflicts": [
                        {
                            "protocol": "tcp",
                            "state": "LISTEN",
                            "local_address": "127.0.0.1",
                            "local_port": 3008,
                            "peer_address": "0.0.0.0:*",
                            "raw_entry": "LISTEN 0 4096 127.0.0.1:3008 0.0.0.0:* users:(\"node\",pid=123,fd=10)",
                            "processes": [
                                { "name": "node", "pid": 123, "fd": 10 }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic port conflicts reply");

        assert!(reply.contains("Port conflicts"));
        assert!(reply.contains("TCP 127.0.0.1:3008"));
        assert!(reply.contains("node"));
        assert!(reply.contains("pid=123"));
    }

    #[test]
    fn deterministic_system_reply_formats_failed_units() {
        let reply = deterministic_system_reply(
            "Which systemd units are failed?",
            &[AssistantToolContextBlock {
                tool: "system_get_failed_units",
                label: "Failed systemd units".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "matched_by": "failed_units",
                    "total_count": 1,
                    "units": [
                        {
                            "name": "rustfin.service",
                            "load": "loaded",
                            "active": "failed",
                            "sub": "failed",
                            "description": "Rustyfin native service",
                            "recent_log_excerpt": "Something broke"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic failed units reply");

        assert!(reply.contains("Failed systemd units"));
        assert!(reply.contains("rustfin.service"));
        assert!(reply.contains("loaded/failed/failed"));
        assert!(reply.contains("Logs"));
    }

    #[test]
    fn deterministic_system_reply_formats_failed_unit_detail() {
        let reply = deterministic_system_reply(
            "Show me details for the failed unit rustfin.service.",
            &[AssistantToolContextBlock {
                tool: "system_get_failed_unit_detail",
                label: "Failed systemd unit detail for rustfin.service".to_string(),
                status: "ok",
                data: json!({
                    "query": "rustfin.service",
                    "matched_by": "exact_name",
                    "detail": {
                        "unit": {
                            "name": "rustfin.service",
                            "load": "loaded",
                            "active": "failed",
                            "sub": "failed",
                            "description": "Rustyfin native service",
                            "recent_log_excerpt": "Something broke"
                        },
                        "status": {
                            "fragment_path": "/etc/systemd/system/rustfin.service",
                            "unit_file_state": "enabled",
                            "main_pid": 123,
                            "exec_main_code": "exited",
                            "exec_main_status": "1",
                            "status_excerpt": "Loaded: loaded",
                        }
                    }
                }),
            }],
        )
        .expect("expected deterministic failed unit detail reply");

        assert!(reply.contains("Failed systemd unit detail"));
        assert!(reply.contains("rustfin.service"));
        assert!(reply.contains("Unit file state"));
        assert!(reply.contains("Main PID"));
        assert!(reply.contains("Logs"));
    }

    #[test]
    fn deterministic_memory_reply_formats_facts_without_raw_json() {
        let reply = deterministic_memory_reply(
            "What do you know about Rachel?",
            &[AssistantToolContextBlock {
                tool: "memory_search_facts",
                label: "Memory facts matching \"Rachel\"".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "topic_key": null,
                    "total_count": 2,
                    "facts": [
                        {
                            "id": "fact-1",
                            "memory_key": "fact-1",
                            "memory_type": "user_memory",
                            "topic_key": "memory:personal",
                            "title": "favorite color",
                            "content": "Dark green",
                            "weight": 1.0,
                            "created_ts": 1000,
                            "updated_ts": 1000
                        },
                        {
                            "id": "fact-2",
                            "memory_key": "fact-2",
                            "memory_type": "user_memory",
                            "topic_key": "memory:people",
                            "title": "family relation",
                            "content": "Rachel is my sister",
                            "weight": 0.8,
                            "created_ts": 2000,
                            "updated_ts": 2000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic memory reply");

        assert!(reply.contains("Stored memory facts matching \"Rachel\""));
        assert!(reply.contains("favorite color"));
        assert!(reply.contains("Rachel is my sister"));
        assert!(!reply.contains("\"facts\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_memory_reply_formats_entity_search_without_raw_json() {
        let reply = deterministic_memory_reply(
            "Who is Rachel in my family?",
            &[AssistantToolContextBlock {
                tool: "memory_search_entities",
                label: "Stored entities matching \"Rachel\"".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "total_count": 1,
                    "entities": [
                        {
                            "id": "entity-1",
                            "node_key": "person:rachel",
                            "entity_kind": "person",
                            "label": "Rachel",
                            "identifier": "rachel",
                            "topic_key": "memory:people",
                            "source_chunk_id": null,
                            "access_scope": "user",
                            "ordinal": 1,
                            "created_ts": 3000,
                            "updated_ts": 3000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic entity reply");

        assert!(reply.contains("Stored entities matching \"Rachel\""));
        assert!(reply.contains("Rachel (person)"));
        assert!(!reply.contains("\"entities\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_memory_reply_formats_recent_entities_without_raw_json() {
        let reply = deterministic_memory_reply(
            "Who do you remember?",
            &[AssistantToolContextBlock {
                tool: "memory_list_recent_entities",
                label: "Recent stored entities".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "total_count": 1,
                    "entities": [
                        {
                            "id": "entity-1",
                            "node_key": "person:rachel",
                            "entity_kind": "person",
                            "label": "Rachel",
                            "identifier": "rachel",
                            "topic_key": "memory:people",
                            "source_chunk_id": null,
                            "access_scope": "user",
                            "ordinal": 1,
                            "created_ts": 3000,
                            "updated_ts": 3000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic recent entities reply");

        assert!(reply.contains("Recent stored entities"));
        assert!(reply.contains("Rachel (person)"));
        assert!(!reply.contains("\"entities\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_memory_reply_formats_recent_changes_without_raw_json() {
        let reply = deterministic_memory_reply(
            "What's new in my memory?",
            &[AssistantToolContextBlock {
                tool: "memory_list_recent_changes",
                label: "Recent stored memory changes".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "fact_count": 1,
                    "entity_count": 1,
                    "facts": [
                        {
                            "id": "fact-1",
                            "memory_key": "fact-1",
                            "memory_type": "user_memory",
                            "topic_key": "memory:people",
                            "title": "favorite color",
                            "content": "Dark green",
                            "weight": 1.0,
                            "created_ts": 1000,
                            "updated_ts": 1000
                        }
                    ],
                    "entities": [
                        {
                            "id": "entity-1",
                            "node_key": "person:rachel",
                            "entity_kind": "person",
                            "label": "Rachel",
                            "identifier": "rachel",
                            "topic_key": "memory:people",
                            "source_chunk_id": null,
                            "access_scope": "user",
                            "ordinal": 1,
                            "created_ts": 3000,
                            "updated_ts": 3000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic recent changes reply");

        assert!(reply.contains("Recent stored memory changes"));
        assert!(reply.contains("favorite color"));
        assert!(reply.contains("Rachel (person)"));
        assert!(!reply.contains("\"fact_count\""));
        assert!(!reply.contains("\"entity_count\""));
    }

    #[test]
    fn deterministic_memory_reply_formats_conflicting_facts_without_raw_json() {
        let reply = deterministic_memory_reply(
            "What conflicting facts do you have about Rachel?",
            &[AssistantToolContextBlock {
                tool: "memory_list_conflicting_facts",
                label: "Conflicting stored memory facts".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "total_count": 2,
                    "conflict_group_count": 1,
                    "conflicts": [
                        {
                            "topic_key": "memory:people",
                            "title": "favorite color",
                            "fact_count": 2,
                            "distinct_content_count": 2,
                            "facts": [
                                {
                                    "id": "fact-1",
                                    "memory_key": "fact-1",
                                    "memory_type": "user_memory",
                                    "topic_key": "memory:people",
                                    "title": "favorite color",
                                    "content": "Dark green",
                                    "weight": 1.0,
                                    "created_ts": 1000,
                                    "updated_ts": 1000
                                },
                                {
                                    "id": "fact-2",
                                    "memory_key": "fact-2",
                                    "memory_type": "user_memory",
                                    "topic_key": "memory:people",
                                    "title": "favorite color",
                                    "content": "Blue",
                                    "weight": 0.9,
                                    "created_ts": 2000,
                                    "updated_ts": 2000
                                }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic conflicting facts reply");

        assert!(reply.contains("Conflicting stored memory facts"));
        assert!(reply.contains("favorite color"));
        assert!(reply.contains("Dark green"));
        assert!(reply.contains("Blue"));
        assert!(!reply.contains("\"conflicts\""));
    }

    #[test]
    fn deterministic_memory_reply_formats_entity_provenance_without_raw_json() {
        let reply = deterministic_memory_reply(
            "Where did you learn about Rachel?",
            &[AssistantToolContextBlock {
                tool: "memory_get_entity_provenance",
                label: "Stored entity provenance for Rachel".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "matched_by": "exact entity search",
                    "entity": {
                        "id": "entity-1",
                        "node_key": "person:rachel",
                        "conversation_id": "conv-1",
                        "turn_id": "turn-1",
                        "entity_kind": "person",
                        "label": "Rachel",
                        "identifier": "rachel",
                        "topic_key": "memory:people",
                        "source_chunk_id": "chunk-1",
                        "access_scope": "user",
                        "ordinal": 1,
                        "created_ts": 3000,
                        "updated_ts": 4000
                    },
                    "source_chunk": {
                        "chunk_key": "chunk-1",
                        "source_kind": "conversation",
                        "source_id": "conv-1",
                        "source_sub_id": "turn-1",
                        "owner_user_id": "user-1",
                        "access_scope": "user",
                        "access_key": null,
                        "topic_key": "memory:people",
                        "title": "Rachel family note",
                        "excerpt": "Rachel is my sister.",
                        "source_ts": 3000,
                        "updated_ts": 4000
                    }
                }),
            }],
        )
        .expect("expected deterministic provenance reply");

        assert!(reply.contains("Stored entity provenance for \"Rachel\""));
        assert!(reply.contains("Rachel (person)"));
        assert!(reply.contains("Source chunk"));
        assert!(reply.contains("Rachel family note"));
        assert!(!reply.contains("\"source_chunk\""));
    }

    #[test]
    fn deterministic_memory_reply_formats_entity_relations_without_raw_json() {
        let reply = deterministic_memory_reply(
            "Who is Rachel related to?",
            &[AssistantToolContextBlock {
                tool: "memory_get_entity_relations",
                label: "Stored entity relations for Rachel".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "matched_by": "entity search",
                    "total_count": 1,
                    "root": {
                        "id": "entity-1",
                        "node_key": "person:rachel",
                        "entity_kind": "person",
                        "label": "Rachel",
                        "identifier": "rachel",
                        "topic_key": "memory:people",
                        "source_chunk_id": null,
                        "access_scope": "user",
                        "ordinal": 1,
                        "created_ts": 3000,
                        "updated_ts": 3000
                    },
                    "relations": [
                        {
                            "direction": "outgoing",
                            "relation": "sibling_of",
                            "weight": 1.0,
                            "created_ts": 4000,
                            "entity": {
                                "id": "entity-2",
                                "node_key": "person:sam",
                                "entity_kind": "person",
                                "label": "Sam",
                                "identifier": "sam",
                                "topic_key": "memory:people",
                                "source_chunk_id": null,
                                "access_scope": "user",
                                "ordinal": 2,
                                "created_ts": 3001,
                                "updated_ts": 3001
                            }
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic entity relations reply");

        assert!(reply.contains("Stored entity relations for \"Rachel\""));
        assert!(reply.contains("sibling_of -> Sam"));
        assert!(!reply.contains("\"relations\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_memory_reply_formats_person_summary_without_raw_json() {
        let reply = deterministic_memory_reply(
            "Give me a summary of Rachel.",
            &[AssistantToolContextBlock {
                tool: "memory_get_person_summary",
                label: "Stored person summary for Rachel".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "matched_by": "exact entity search",
                    "person": {
                        "id": "entity-1",
                        "node_key": "person:rachel",
                        "entity_kind": "person",
                        "label": "Rachel",
                        "identifier": "rachel",
                        "topic_key": "memory:people",
                        "source_chunk_id": null,
                        "access_scope": "user",
                        "ordinal": 1,
                        "created_ts": 3000,
                        "updated_ts": 4000
                    },
                    "relation_count": 1,
                    "relations": [
                        {
                            "direction": "outgoing",
                            "relation": "sibling_of",
                            "weight": 1.0,
                            "created_ts": 4000,
                            "entity": {
                                "id": "entity-2",
                                "node_key": "person:sam",
                                "entity_kind": "person",
                                "label": "Sam",
                                "identifier": "sam",
                                "topic_key": "memory:people",
                                "source_chunk_id": null,
                                "access_scope": "user",
                                "ordinal": 2,
                                "created_ts": 3001,
                                "updated_ts": 3001
                            }
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic person summary reply");

        assert!(reply.contains("Stored person summary for \"Rachel\""));
        assert!(reply.contains("Rachel (person)"));
        assert!(reply.contains("sibling_of -> Sam"));
        assert!(!reply.contains("\"person\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_library_reply_formats_search_results_without_raw_json() {
        let reply = deterministic_library_reply(
            "Search my libraries for Star Trek",
            &[AssistantToolContextBlock {
                tool: "library_search_titles",
                label: "Library matches for \"Star Trek\"".to_string(),
                status: "ok",
                data: json!({
                    "match_count": 0,
                    "matches": [],
                    "query": "Star Trek"
                }),
            }],
        )
        .expect("expected deterministic library reply");

        assert!(reply.contains("No Star Trek titles were found in your libraries."));
        assert!(!reply.contains("\"query\""));
        assert!(!reply.contains("\"matches\""));
        assert!(!reply.contains("[{"));
    }

    #[test]
    fn deterministic_library_reply_formats_item_summary_without_raw_json() {
        let reply = deterministic_library_reply(
            "Tell me about Interstellar.",
            &[AssistantToolContextBlock {
                tool: "library_get_item_summary",
                label: "Library item summary for Interstellar".to_string(),
                status: "ok",
                data: json!({
                    "library_id": "library-1",
                    "id": "item-1",
                    "title": "Interstellar",
                    "kind": "movie",
                    "year": 2014,
                    "library_name": "Movies",
                    "overview": "A team travels through a wormhole.",
                    "duration_ms": 101 * 60 * 1000
                }),
            }],
        )
        .expect("expected deterministic library item reply");

        assert!(reply.contains("Interstellar"));
        assert!(reply.contains("Movies"));
        assert!(reply.contains("Kind: movie"));
        assert!(reply.contains("Overview"));
        assert!(!reply.contains("\"library_id\""));
    }

    #[test]
    fn deterministic_library_reply_formats_item_media_details_without_raw_json() {
        let reply = deterministic_library_reply(
            "Where is Interstellar stored and what artwork does it have?",
            &[AssistantToolContextBlock {
                tool: "library_get_item_media_details",
                label: "Library media details for Interstellar".to_string(),
                status: "ok",
                data: json!({
                    "query": "Interstellar",
                    "matched_by": "exact_title",
                    "library_id": "library-1",
                    "id": "item-1",
                    "title": "Interstellar",
                    "kind": "movie",
                    "year": 2014,
                    "library_name": "Movies",
                    "overview": "A team travels through a wormhole.",
                    "duration_ms": 101 * 60 * 1000,
                    "parent_id": null,
                    "media_path": "/media/movies/Interstellar.mkv",
                    "resolved_media_path": "/media/movies/Interstellar.mkv",
                    "first_descendant_media_path": null,
                    "poster_url": "/art/poster.jpg",
                    "backdrop_url": "/art/backdrop.jpg",
                    "logo_url": null,
                    "thumb_url": "/art/thumb.jpg",
                    "created_ts": 1712500000,
                    "updated_ts": 1712600000
                }),
            }],
        )
        .expect("expected deterministic library media reply");

        assert!(reply.contains("Interstellar"));
        assert!(reply.contains("Resolved media path"));
        assert!(reply.contains("/media/movies/Interstellar.mkv"));
        assert!(reply.contains("Artwork"));
        assert!(reply.contains("/art/poster.jpg"));
        assert!(!reply.contains("\"resolved_media_path\""));
    }

    #[test]
    fn deterministic_library_reply_formats_item_source_paths_without_raw_json() {
        let reply = deterministic_library_reply(
            "Where are the source paths for Interstellar?",
            &[AssistantToolContextBlock {
                tool: "library_get_item_source_paths",
                label: "Library item source paths for Interstellar".to_string(),
                status: "ok",
                data: json!({
                    "query": "Interstellar",
                    "matched_by": "exact_title",
                    "library_id": "library-1",
                    "id": "item-1",
                    "title": "Interstellar",
                    "kind": "movie",
                    "year": 2014,
                    "library_name": "Movies",
                    "overview": "A team travels through a wormhole.",
                    "duration_ms": 101 * 60 * 1000,
                    "parent_id": null,
                    "media_path": "/media/movies/Interstellar.mkv",
                    "resolved_media_path": "/media/movies/Interstellar.mkv",
                    "first_descendant_media_path": null,
                    "source_paths": [
                        "/media/movies/Interstellar.mkv"
                    ],
                    "poster_url": "/art/poster.jpg",
                    "backdrop_url": "/art/backdrop.jpg",
                    "logo_url": null,
                    "thumb_url": "/art/thumb.jpg",
                    "created_ts": 1712500000,
                    "updated_ts": 1712600000
                }),
            }],
        )
        .expect("expected deterministic library source paths reply");

        assert!(reply.contains("Library item source paths for \"Interstellar\""));
        assert!(reply.contains("/media/movies/Interstellar.mkv"));
        assert!(reply.contains("Source paths"));
        assert!(!reply.contains("\"source_paths\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_artifact_details_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "Tell me more about the RustyVault browser extension package.",
            &[AssistantToolContextBlock {
                tool: "downloads_get_artifact_details",
                label: "Download artifact details for RustyVault Browser Extension".to_string(),
                status: "ok",
                data: json!({
                    "query": "RustyVault browser extension",
                    "matched_by": "title",
                    "id": "download-1",
                    "artifact_id": "rustyvault-webext",
                    "title": "RustyVault Browser Extension",
                    "summary": "Browser extension package",
                    "availability": "available",
                    "detail": "Extension for autofill and pairing.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.0.0",
                    "channel": "stable",
                    "package_filename": "rustyvault-webext.tar.gz",
                    "file_size": 1234567,
                    "checksum": "abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": "https://example.com",
                    "download_path": "/api/v1/downloads/artifacts/rustyvault-webext/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Install extension"
                    ]
                }),
            }],
        )
        .expect("expected deterministic downloads reply");

        assert!(reply.contains("RustyVault Browser Extension"));
        assert!(reply.contains("available"));
        assert!(reply.contains("Package size"));
        assert!(reply.contains("Install steps"));
        assert!(reply.contains("Download artifact details"));
        assert!(!reply.contains("\"artifact_id\""));
        assert!(!reply.contains("\"install_steps\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_source_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "Where is the Rustyfin App package source?",
            &[AssistantToolContextBlock {
                tool: "downloads_get_artifact_source",
                label: "Download artifact source for Rustyfin App".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rustyfin App",
                    "matched_by": "title",
                    "id": "download-2",
                    "artifact_id": "rustyfin-app",
                    "title": "Rustyfin App",
                    "summary": "Desktop companion app",
                    "availability": "available",
                    "detail": "Desktop client for Rustyfin.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.2.3",
                    "channel": "stable",
                    "package_filename": "rustyfin-app.tar.gz",
                    "file_size": 123456,
                    "checksum": "sha256:abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": "https://example.com",
                    "download_path": "/api/v1/downloads/artifacts/rustyfin-app/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Run setup"
                    ]
                }),
            }],
        )
        .expect("expected deterministic artifact source reply");

        assert!(reply.contains("Download artifact source for \"Rustyfin App\""));
        assert!(reply.contains("external URL https://example.com"));
        assert!(reply.contains("download path /api/v1/downloads/artifacts/rustyfin-app/package"));
        assert!(!reply.contains("\"download_path\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_release_notes_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "What are the release notes for Rustyfin App?",
            &[AssistantToolContextBlock {
                tool: "downloads_get_release_notes",
                label: "Download release notes for Rustyfin App".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rustyfin App",
                    "matched_by": "title",
                    "id": "download-2",
                    "artifact_id": "rustyfin-app",
                    "title": "Rustyfin App",
                    "summary": "Desktop companion app",
                    "availability": "available",
                    "detail": "Desktop client for Rustyfin with bug fixes and polish.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.2.3",
                    "channel": "stable",
                    "package_filename": "rustyfin-app.tar.gz",
                    "file_size": 123456,
                    "checksum": "sha256:abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": null,
                    "download_path": "/api/v1/downloads/artifacts/rustyfin-app/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Run setup"
                    ]
                }),
            }],
        )
        .expect("expected deterministic release notes reply");

        assert!(reply.contains("Download release notes for \"Rustyfin App\""));
        assert!(reply.contains("Desktop client for Rustyfin with bug fixes and polish."));
        assert!(reply.contains("Version: 1.2.3"));
        assert!(!reply.contains("\"detail\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_checksum_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "What is the checksum for the Rustyfin App package?",
            &[AssistantToolContextBlock {
                tool: "downloads_get_artifact_checksum",
                label: "Download artifact checksum for Rustyfin App".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rustyfin App",
                    "matched_by": "title",
                    "id": "download-2",
                    "artifact_id": "rustyfin-app",
                    "title": "Rustyfin App",
                    "summary": "Desktop companion app",
                    "availability": "available",
                    "detail": "Desktop client for Rustyfin.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.2.3",
                    "channel": "stable",
                    "package_filename": "rustyfin-app.tar.gz",
                    "file_size": 123456,
                    "checksum": "sha256:abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": null,
                    "download_path": "/api/v1/downloads/artifacts/rustyfin-app/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Run setup"
                    ]
                }),
            }],
        )
        .expect("expected deterministic checksum reply");

        assert!(reply.contains("Download artifact checksum for \"Rustyfin App\""));
        assert!(reply.contains("Checksum: sha256:abc123"));
        assert!(reply.contains("Signature status: verified"));
        assert!(!reply.contains("\"artifact_id\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_install_steps_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "How do I install the Rustyfin App?",
            &[AssistantToolContextBlock {
                tool: "downloads_get_artifact_install_steps",
                label: "Download artifact install steps for Rustyfin App".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rustyfin App",
                    "matched_by": "title",
                    "id": "download-2",
                    "artifact_id": "rustyfin-app",
                    "title": "Rustyfin App",
                    "summary": "Desktop companion app",
                    "availability": "available",
                    "detail": "Desktop client for Rustyfin.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.2.3",
                    "channel": "stable",
                    "package_filename": "rustyfin-app.tar.gz",
                    "file_size": 123456,
                    "checksum": "sha256:abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": null,
                    "download_path": "/api/v1/downloads/artifacts/rustyfin-app/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Run setup"
                    ]
                }),
            }],
        )
        .expect("expected deterministic install steps reply");

        assert!(reply.contains("Download artifact install steps for \"Rustyfin App\""));
        assert!(reply.contains("1. Download package"));
        assert!(reply.contains("2. Run setup"));
        assert!(reply.contains("Install mode: package"));
        assert!(reply.contains("Setup path: /opt/rustyfin/setup.sh"));
        assert!(!reply.contains("\"install_steps\""));
    }

    #[test]
    fn deterministic_downloads_reply_formats_compatibility_without_raw_json() {
        let reply = deterministic_downloads_reply(
            "Is the Rustyfin App compatible with Linux?",
            &[AssistantToolContextBlock {
                tool: "downloads_get_artifact_compatibility",
                label: "Download artifact compatibility for Rustyfin App".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rustyfin App",
                    "matched_by": "title",
                    "id": "download-2",
                    "artifact_id": "rustyfin-app",
                    "title": "Rustyfin App",
                    "summary": "Desktop companion app",
                    "availability": "available",
                    "detail": "Desktop client for Rustyfin.",
                    "platform": "linux",
                    "architecture": "x86_64",
                    "version": "1.2.3",
                    "channel": "stable",
                    "package_filename": "rustyfin-app.tar.gz",
                    "file_size": 123456,
                    "checksum": "sha256:abc123",
                    "signature_status": "verified",
                    "distribution_mode": "download",
                    "external_url": null,
                    "download_path": "/api/v1/downloads/artifacts/rustyfin-app/package",
                    "install_mode": "package",
                    "setup_path": "/opt/rustyfin/setup.sh",
                    "requires_sign_in": true,
                    "install_steps": [
                        "Download package",
                        "Run setup"
                    ]
                }),
            }],
        )
        .expect("expected deterministic compatibility reply");

        assert!(reply.contains("Download artifact compatibility for \"Rustyfin App\""));
        assert!(reply.contains("Platform: linux. Architecture: x86_64."));
        assert!(reply.contains("Distribution mode: download."));
        assert!(reply.contains("A signed-in host session is required for this artifact."));
        assert!(!reply.contains("\"distribution_mode\""));
    }

    #[test]
    fn deterministic_library_reply_formats_duplicate_titles_without_raw_json() {
        let reply = deterministic_library_reply(
            "Do I have duplicate titles in my libraries?",
            &[AssistantToolContextBlock {
                tool: "libraries_find_duplicate_titles",
                label: "Library duplicate titles".to_string(),
                status: "ok",
                data: json!({
                    "total_count": 3,
                    "duplicate_group_count": 1,
                    "duplicates": [
                        {
                            "title": "Star Trek",
                            "item_count": 2,
                            "library_count": 2,
                            "libraries": ["Movies", "TV"]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic duplicate titles reply");

        assert!(reply.contains(
            "Duplicate library titles across accessible libraries: 1 groups across 3 items."
        ));
        assert!(reply.contains("- Star Trek: 2 items in 2 libraries (Movies, TV)"));
        assert!(!reply.contains("\"duplicate_group_count\""));
    }

    #[test]
    fn deterministic_library_reply_formats_missing_metadata_without_raw_json() {
        let reply = deterministic_library_reply(
            "What library items are missing metadata?",
            &[AssistantToolContextBlock {
                tool: "libraries_list_missing_metadata",
                label: "Library items with missing metadata".to_string(),
                status: "ok",
                data: json!({
                    "total_count": 2,
                    "missing_item_count": 1,
                    "items": [
                        {
                            "library_id": "library-1",
                            "library_name": "Movies",
                            "id": "item-1",
                            "title": "Interstellar",
                            "kind": "movie",
                            "year": 2014,
                            "missing_fields": ["overview", "poster"],
                            "created_ts": 1712500000,
                            "updated_ts": 1712600000
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic missing metadata reply");

        assert!(reply.contains("Library items with missing metadata: 1 items."));
        assert!(reply.contains("- Interstellar (Movies) missing overview, poster"));
        assert!(!reply.contains("\"missing_item_count\""));
    }

    #[test]
    fn deterministic_library_reply_formats_library_summary_without_raw_json() {
        let reply = deterministic_library_reply(
            "Tell me about my Movies library.",
            &[AssistantToolContextBlock {
                tool: "libraries_get_library_summary",
                label: "Library summary for Movies".to_string(),
                status: "ok",
                data: json!({
                    "query": "Movies",
                    "matched_by": "name",
                    "id": "library-1",
                    "name": "Movies",
                    "kind": "movie",
                    "item_count": 42,
                    "paths": [
                        {
                            "id": "path-1",
                            "path": "/media/movies",
                            "is_read_only": false
                        }
                    ],
                    "settings": {
                        "show_images": true,
                        "prefer_local_artwork": true,
                        "fetch_online_artwork": false,
                        "tmdb_store_in_media_dir": false,
                        "tmdb_sync_on_new_media": true,
                        "tmdb_sync_schedule": "manual",
                        "tmdb_last_sync_ts": 1712500000,
                        "tmdb_fetch_posters": true,
                        "tmdb_fetch_backdrops": true,
                        "tmdb_fetch_metadata": true,
                        "tmdb_fetch_reviews": false
                    },
                    "created_ts": 1712000000,
                    "updated_ts": 1712500000
                }),
            }],
        )
        .expect("expected deterministic library summary reply");

        assert!(reply.contains("Movies"));
        assert!(reply.contains("Item count: 42"));
        assert!(reply.contains("/media/movies"));
        assert!(reply.contains("TMDb sync"));
        assert!(reply.contains("Created"));
        assert!(!reply.contains("\"settings\""));
        assert!(!reply.contains("\"paths\""));
    }

    #[test]
    fn deterministic_network_reply_uses_network_block_even_with_extra_grounding() {
        let reply = deterministic_network_reply(
            "How do I connect to Rustyfin on this network from another device?",
            &[
                AssistantToolContextBlock {
                    tool: "downloads_list_available_artifacts",
                    label: "Available downloads".to_string(),
                    status: "ok",
                    data: json!({ "artifacts": [] }),
                },
                AssistantToolContextBlock {
                    tool: "network_get_topology_summary",
                    label: "Rustyfin network topology summary".to_string(),
                    status: "ok",
                    data: json!({
                        "host_label": "server",
                        "public_host": "example.rustyfin.local",
                        "remote_access_enabled": false,
                        "access": {
                            "ui_port": 3008,
                            "backend_port": 8097,
                            "calendar_port": 8099,
                            "preferred_local_interface": "enp5s0",
                            "preferred_local_ipv4": "192.168.0.36",
                            "preferred_local_url": "https://192.168.0.36:3008",
                            "login_url": "https://192.168.0.36:3008/login",
                            "ai_url": "https://192.168.0.36:3008/ai",
                            "public_url": "https://example.rustyfin.local:3008"
                        },
                        "nodes": [
                            {
                                "name": "br-76e2dd24505e",
                                "status": "online",
                                "addresses": [
                                    { "family": "inet", "address": "192.168.112.1" }
                                ]
                            },
                            {
                                "name": "enp5s0",
                                "status": "online",
                                "addresses": [
                                    { "family": "inet", "address": "192.168.0.36" }
                                ]
                            }
                        ]
                    }),
                },
            ],
        )
        .expect("expected deterministic network reply");

        assert!(reply.contains("https://192.168.0.36:3008"));
        assert!(reply.contains("192.168.0.36"));
        assert!(!reply.contains("192.168.112.1"));
    }

    #[test]
    fn deterministic_web_reply_formats_curated_source_catalog() {
        let reply = deterministic_web_reply(
            "What sites do you use for technology?",
            &[AssistantToolContextBlock {
                tool: "web_list_curated_sources",
                label: "Curated public web source catalog".to_string(),
                status: "ok",
                data: json!({
                    "categories": [
                        {
                            "category": "technology",
                            "label": "Technology",
                            "description": "Technology news, engineering, product, and developer sources.",
                            "source_count": 2,
                            "sources": [
                                { "name": "Ars Technica" },
                                { "name": "TechCrunch" }
                            ]
                        },
                        {
                            "category": "business",
                            "label": "Business",
                            "description": "Business and company news sources.",
                            "source_count": 1,
                            "sources": [
                                { "name": "Reuters Business" }
                            ]
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic web catalog reply");

        assert!(reply.contains("Curated public web source catalog"));
        assert!(reply.contains("Technology"));
        assert!(reply.contains("Reuters Business"));
        assert!(!reply.contains("\"categories\""));
    }

    #[test]
    fn deterministic_web_reply_formats_curated_search_results() {
        let reply = deterministic_web_reply(
            "search the web for Rust compiler release notes",
            &[AssistantToolContextBlock {
                tool: "web_search_public_web",
                label: "Curated Technology web results".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rust compiler release notes",
                    "category": "technology",
                    "results": [
                        {
                            "title": "Rust 1.78.0 Release Notes",
                            "url": "https://blog.rust-lang.org/2024/05/02/Rust-1.78.0.html",
                            "source_host": "blog.rust-lang.org",
                            "snippet": "The Rust team is happy to announce a new version of Rust."
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic web search reply");

        assert!(reply.contains("Curated Technology web results"));
        assert!(reply.contains("Rust 1.78.0 Release Notes"));
        assert!(reply.contains("blog.rust-lang.org"));
        assert!(reply.contains("https://blog.rust-lang.org/2024/05/02/Rust-1.78.0.html"));
        assert!(!reply.contains("\"results\""));
    }

    #[test]
    fn deterministic_web_reply_formats_curated_page_summary() {
        let reply = deterministic_web_reply(
            "Fetch https://www.reuters.com/markets/",
            &[AssistantToolContextBlock {
                tool: "web_fetch_public_page_summary",
                label: "Curated Business page summary".to_string(),
                status: "ok",
                data: json!({
                    "category": "business",
                    "requested_url": "https://www.reuters.com/markets/",
                    "final_url": "https://www.reuters.com/markets/",
                    "source_host": "www.reuters.com",
                    "page_title": "Markets",
                    "summary": "Reuters markets coverage and reporting.",
                    "content_type": "text/html; charset=utf-8"
                }),
            }],
        )
        .expect("expected deterministic web page reply");

        assert!(reply.contains("Business page summary"));
        assert!(reply.contains("Reuters markets coverage"));
        assert!(reply.contains("www.reuters.com"));
        assert!(!reply.contains("\"summary\""));
    }

    #[test]
    fn deterministic_ai_runtime_reply_reports_loaded_model() {
        let reply = deterministic_ai_runtime_reply(
            "What AI model is loaded right now?",
            &[AssistantToolContextBlock {
                tool: "system_get_ai_runtime_summary",
                label: "Rustyfin AI runtime summary".to_string(),
                status: "ok",
                data: json!({
                    "model": {
                        "name": "Llama-3.2-3B-Instruct-Q4_K_M",
                        "backend": "local",
                        "loaded": true,
                        "context_length": 4096,
                        "n_threads": 8,
                        "split_mode": "layer",
                        "device_indices": [0, 1]
                    },
                    "scheduler": {
                        "overload_state": "Normal",
                        "active_turns": 0,
                        "queued_turns": 0,
                        "warm_pool_bytes": 3006477107_u64,
                        "warm_pool_budget_bytes": 8589934592_u64
                    },
                    "role_routing": [
                        {
                            "role": "answer",
                            "model_name": "Llama-3.2-3B-Instruct-Q4_K_M",
                            "backend_kind": "local"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic ai runtime reply");

        assert!(reply.contains("Llama-3.2-3B-Instruct-Q4_K_M"));
        assert!(reply.contains("local backend"));
    }

    #[test]
    fn deterministic_ai_runtime_reply_reports_scheduler_when_requested() {
        let reply = deterministic_ai_runtime_reply(
            "What is the AI warm pool and queue depth right now?",
            &[AssistantToolContextBlock {
                tool: "system_get_ai_runtime_summary",
                label: "Rustyfin AI runtime summary".to_string(),
                status: "ok",
                data: json!({
                    "model": {
                        "name": null,
                        "backend": "local",
                        "loaded": false,
                        "context_length": 4096,
                        "n_threads": 8,
                        "split_mode": "layer",
                        "device_indices": []
                    },
                    "scheduler": {
                        "overload_state": "Normal",
                        "active_turns": 1,
                        "queued_turns": 2,
                        "warm_pool_bytes": 2147483648_u64,
                        "warm_pool_budget_bytes": 8589934592_u64
                    },
                    "role_routing": []
                }),
            }],
        )
        .expect("expected deterministic ai runtime reply");

        assert!(reply.contains("No AI model is currently loaded."));
        assert!(reply.contains("Scheduler is normal with 1 active and 2 queued."));
        assert!(reply.contains("2.0 GiB"));
        assert!(reply.contains("8.0 GiB"));
    }

    #[test]
    fn deterministic_ai_runtime_reply_reports_gpu_vram_when_requested() {
        let reply = deterministic_ai_runtime_reply(
            "How much VRAM are the GPUs using right now?",
            &[AssistantToolContextBlock {
                tool: "system_get_ai_runtime_summary",
                label: "Rustyfin AI runtime summary".to_string(),
                status: "ok",
                data: json!({
                    "model": {
                        "name": "Llama-3.2-3B-Instruct-Q4_K_M",
                        "backend": "local",
                        "loaded": true,
                        "context_length": 4096,
                        "n_threads": 8,
                        "split_mode": "layer",
                        "device_indices": [0, 1]
                    },
                    "scheduler": {
                        "overload_state": "Normal",
                        "active_turns": 0,
                        "queued_turns": 0,
                        "warm_pool_bytes": 0_u64,
                        "warm_pool_budget_bytes": 8589934592_u64
                    },
                    "gpus": [
                        {
                            "index": 0,
                            "name": "RTX 3090",
                            "vram_used_bytes": 2147483648_u64,
                            "vram_total_bytes": 25769803776_u64
                        },
                        {
                            "index": 1,
                            "name": "RTX 3080",
                            "vram_used_bytes": 1073741824_u64,
                            "vram_total_bytes": 10737418240_u64
                        }
                    ],
                    "role_routing": []
                }),
            }],
        )
        .expect("expected deterministic ai runtime reply");

        assert!(reply.contains("Current GPU VRAM usage:"));
        assert!(reply.contains("GPU 0 (RTX 3090) is using 2.0 GiB of 24.0 GiB (8.3%)."));
        assert!(reply.contains("GPU 1 (RTX 3080) is using 1.0 GiB of 10.0 GiB (10.0%)."));
    }
}
