use std::collections::HashMap;

use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::warn;

use super::context::AssistantContext;
use super::replies::{compact_text, rank_and_compress_grounding_chunks};
use super::types::{
    AssistantChatRequest, AssistantFollowUpContext, AssistantFollowUpEntity,
    AssistantGroundingChunk, AssistantGroundingCitation, AssistantGroundingVisibility,
    AssistantHistoryMessage, AssistantToolContextBlock, PlannedToolCall,
};
use crate::state::AppState;

const MAX_RETRIEVAL_HITS: i64 = 6;
const MAX_ENTITY_GRAPH_CONTEXTS: usize = 2;

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(b"|");
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"|");
    }
    let digest = hasher.finalize();
    format!("{prefix}:{}", hex::encode(&digest[..16]))
}

fn humanize_binary_bytes(bytes: u64) -> String {
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

fn visibility_for_tool(tool: &str) -> AssistantGroundingVisibility {
    match tool {
        "system_get_host_runtime_summary"
        | "system_get_backup_summary"
        | "system_get_service_health"
        | "system_get_service_detail"
        | "system_get_transcode_summary"
        | "system_get_storage_summary"
        | "system_get_recent_errors"
        | "system_get_kernel_info"
        | "system_get_cpu_topology"
        | "system_get_temperature_sensors"
        | "system_get_block_device_inventory"
        | "system_get_filesystem_table"
        | "system_get_gpu_inventory"
        | "system_get_pci_devices"
        | "system_get_usb_devices"
        | "system_get_boot_log_summary"
        | "system_get_journal_summary"
        | "system_get_port_conflicts"
        | "system_get_failed_units"
        | "ai_list_background_jobs"
        | "ai_get_job_status"
        | "ai_get_tool_registry"
        | "ai_get_grounding_summary"
        | "ai_get_last_tool_failure_reason" => AssistantGroundingVisibility::Admin,
        "downloads_list_available_artifacts" | "downloads_get_artifact_details" => {
            AssistantGroundingVisibility::Shared
        }
        "network_get_topology_summary"
        | "network_get_interface_details"
        | "network_get_interface_by_ip"
        | "network_get_default_route"
        | "network_get_hostname_aliases" => AssistantGroundingVisibility::Shared,
        "network_get_route_table"
        | "network_get_active_connections"
        | "network_get_interface_counters"
        | "network_get_wifi_status"
        | "network_get_vpn_status" => AssistantGroundingVisibility::Admin,
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details"
        | "calendar_get_event_by_exact_date_and_title"
        | "calendar_get_event_series_summary"
        | "calendar_get_next_free_slot"
        | "calendar_list_busy_slots"
        | "calendar_count_events"
        | "calendar_list_busy_days"
        | "calendar_list_overlapping_events"
        | "calendar_create_event"
        | "calendar_create_birthday"
        | "channels_get_transcript_summary"
        | "memory_get_person_summary"
        | "memory_list_recent_facts"
        | "memory_list_recent_entities"
        | "memory_search_facts"
        | "memory_search_entities"
        | "memory_find_exact_entity"
        | "memory_get_entity_relations"
        | "memory_get_entity_relation_path"
        | "memory_list_recent_changes"
        | "memory_list_conflicting_facts"
        | "memory_get_entity_provenance"
        | "account_get_profile_summary" => AssistantGroundingVisibility::User,
        _ => AssistantGroundingVisibility::Shared,
    }
}

fn topic_key_for_tool(call: &PlannedToolCall, block: &AssistantToolContextBlock) -> Option<String> {
    match call.tool.as_str() {
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details"
        | "calendar_get_event_by_exact_date_and_title"
        | "calendar_get_event_series_summary"
        | "calendar_get_next_free_slot"
        | "calendar_list_busy_slots"
        | "calendar_count_events"
        | "calendar_list_busy_days"
        | "calendar_list_overlapping_events"
        | "calendar_create_event"
        | "calendar_create_birthday" => block
            .data
            .get("window")
            .and_then(|window| window.get("label"))
            .and_then(Value::as_str)
            .map(|label| format!("calendar:{label}"))
            .or_else(|| {
                block
                    .data
                    .get("event")
                    .and_then(|event| event.get("id"))
                    .and_then(Value::as_str)
                    .map(|event_id| format!("calendar:{event_id}"))
            }),
        "channels_get_transcript_summary" => block
            .data
            .get("channel_id")
            .and_then(Value::as_str)
            .map(|channel_id| format!("transcript:{channel_id}")),
        "downloads_list_available_artifacts" => Some("downloads:catalog".to_string()),
        "downloads_get_artifact_details" => block
            .data
            .get("artifact_id")
            .and_then(Value::as_str)
            .or_else(|| block.data.get("id").and_then(Value::as_str))
            .map(|artifact_id| format!("downloads:{artifact_id}"))
            .or_else(|| Some("downloads:catalog".to_string())),
        "network_get_topology_summary" => Some("network:topology".to_string()),
        "network_get_interface_details" => block
            .data
            .get("interface")
            .and_then(|interface| interface.get("name"))
            .and_then(Value::as_str)
            .map(|name| format!("network:{name}"))
            .or_else(|| Some("network:topology".to_string())),
        "network_get_interface_by_ip" => block
            .data
            .get("interface")
            .and_then(|interface| interface.get("name"))
            .and_then(Value::as_str)
            .map(|name| format!("network:{name}"))
            .or_else(|| Some("network:topology".to_string())),
        "network_get_default_route" => Some("network:default_route".to_string()),
        "network_get_hostname_aliases" => Some("network:hostname_aliases".to_string()),
        "network_get_route_table" => Some("admin:route_table".to_string()),
        "network_get_active_connections" => Some("admin:connections".to_string()),
        "network_get_interface_counters" => Some("admin:interface_counters".to_string()),
        "network_get_wifi_status" => Some("admin:wifi".to_string()),
        "network_get_vpn_status" => Some("admin:vpn".to_string()),
        "libraries_list_accessible" => Some("libraries:accessible".to_string()),
        "libraries_get_library_summary" => block
            .data
            .get("id")
            .and_then(Value::as_str)
            .map(|library_id| format!("library:{library_id}"))
            .or_else(|| Some("libraries:accessible".to_string())),
        "memory_list_recent_changes" => block
            .data
            .get("entities")
            .and_then(Value::as_array)
            .and_then(|entities| {
                entities
                    .iter()
                    .find_map(|entity| entity.get("topic_key").and_then(Value::as_str))
            })
            .or_else(|| {
                block
                    .data
                    .get("facts")
                    .and_then(Value::as_array)
                    .and_then(|facts| {
                        facts
                            .iter()
                            .find_map(|fact| fact.get("topic_key").and_then(Value::as_str))
                    })
            })
            .map(str::to_string)
            .or_else(|| Some("memory:recent_changes".to_string())),
        "memory_list_conflicting_facts" => block
            .data
            .get("conflicts")
            .and_then(Value::as_array)
            .and_then(|conflicts| {
                conflicts
                    .iter()
                    .find_map(|conflict| conflict.get("topic_key").and_then(Value::as_str))
            })
            .map(str::to_string)
            .or_else(|| Some("memory:conflicts".to_string())),
        "memory_get_entity_provenance" => block
            .data
            .get("entity")
            .and_then(|entity| entity.get("topic_key"))
            .and_then(Value::as_str)
            .or_else(|| {
                block
                    .data
                    .get("source_chunk")
                    .and_then(|chunk| chunk.get("topic_key"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
            .or_else(|| Some("memory:provenance".to_string())),
        "memory_get_person_summary" => block
            .data
            .get("person")
            .and_then(|person| person.get("topic_key"))
            .and_then(Value::as_str)
            .or_else(|| {
                block
                    .data
                    .get("relations")
                    .and_then(Value::as_array)
                    .and_then(|relations| {
                        relations.iter().find_map(|relation| {
                            relation
                                .get("entity")
                                .and_then(|entity| entity.get("topic_key"))
                        })
                    })
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
            .or_else(|| Some("memory:people".to_string())),
        "system_get_port_conflicts" => Some("admin:port_conflicts".to_string()),
        "system_get_failed_units" => Some("admin:failed_units".to_string()),
        "system_get_kernel_info" => Some("admin:kernel".to_string()),
        "system_get_cpu_topology" => Some("admin:cpu_topology".to_string()),
        "system_get_temperature_sensors" => Some("admin:temperature_sensors".to_string()),
        "system_get_block_device_inventory" => Some("admin:block_devices".to_string()),
        "system_get_filesystem_table" => Some("admin:filesystem_table".to_string()),
        "system_get_gpu_inventory" => Some("admin:gpu_inventory".to_string()),
        "system_get_pci_devices" => Some("admin:pci_devices".to_string()),
        "system_get_usb_devices" => Some("admin:usb_devices".to_string()),
        "system_get_boot_log_summary" => Some("admin:boot_logs".to_string()),
        "system_get_journal_summary" => Some("admin:journal".to_string()),
        "library_search_titles"
        | "library_get_item_summary"
        | "library_get_item_media_details"
        | "libraries_get_recently_added" => block
            .data
            .get("id")
            .and_then(Value::as_str)
            .map(|item_id| format!("library_item:{item_id}"))
            .or_else(|| {
                block
                    .data
                    .get("library_id")
                    .and_then(Value::as_str)
                    .map(|library_id| format!("library:{library_id}"))
            })
            .or_else(|| Some("libraries:search".to_string())),
        "rooms_list_active" | "rooms_list_joinable" | "rooms_get_room_summary" => {
            Some("rooms:catalog".to_string())
        }
        "servers_list_minecraft_status" | "servers_get_minecraft_server_summary" => {
            Some("servers:catalog".to_string())
        }
        "system_get_ai_runtime_summary" => Some("ai:runtime".to_string()),
        "system_get_host_runtime_summary" => Some("admin:runtime".to_string()),
        "system_get_backup_summary" => Some("admin:backups".to_string()),
        "system_get_service_health" => Some("admin:service_health".to_string()),
        "system_get_service_detail" => block
            .data
            .get("component")
            .and_then(|component| component.get("name"))
            .and_then(Value::as_str)
            .map(|name| format!("admin:service:{name}"))
            .or_else(|| Some("admin:service_health".to_string())),
        "system_get_transcode_summary" => Some("admin:transcode".to_string()),
        "system_get_storage_summary" => Some("admin:storage".to_string()),
        "system_get_recent_errors" => Some("admin:recent_errors".to_string()),
        "weather_get_current" | "weather_get_forecast" | "weather_get_history" => block
            .data
            .get("resolved_location")
            .and_then(Value::as_str)
            .or_else(|| block.data.get("location").and_then(Value::as_str))
            .map(|location| format!("weather:{location}")),
        "web_list_curated_sources" => Some("web:catalog".to_string()),
        "web_search_public_web" | "web_fetch_public_page_summary" => block
            .data
            .get("category")
            .and_then(Value::as_str)
            .map(|category| format!("web:{category}"))
            .or_else(|| Some("web:public".to_string())),
        _ => None,
    }
}

fn first_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_string))
}

#[allow(clippy::too_many_arguments)]
fn chunk_citation(
    source_kind: &str,
    source_id: &str,
    source_sub_id: Option<&str>,
    label: Option<&str>,
    excerpt: Option<&str>,
    started_ts_ms: Option<i64>,
    ended_ts_ms: Option<i64>,
    url: Option<&str>,
) -> AssistantGroundingCitation {
    AssistantGroundingCitation {
        citation_id: stable_id(
            "cite",
            &[
                source_kind,
                source_id,
                source_sub_id.unwrap_or(""),
                label.unwrap_or(""),
                excerpt.unwrap_or(""),
            ],
        ),
        source_kind: source_kind.to_string(),
        source_id: source_id.to_string(),
        source_sub_id: source_sub_id.map(str::to_string),
        label: label.map(str::to_string),
        excerpt: excerpt.map(str::to_string),
        started_ts_ms,
        ended_ts_ms,
        url: url.map(str::to_string),
    }
}

#[allow(clippy::too_many_arguments)]
fn chunk_from_parts(
    source_kind: &str,
    title: String,
    excerpt: String,
    score: f64,
    visibility: AssistantGroundingVisibility,
    topic_key: Option<String>,
    owner_user_id: Option<String>,
    source_id: Option<String>,
    source_sub_id: Option<String>,
    citation: Option<AssistantGroundingCitation>,
) -> AssistantGroundingChunk {
    let mut hash_parts = vec![
        source_kind.to_string(),
        topic_key.clone().unwrap_or_default(),
        title.clone(),
        excerpt.clone(),
    ];
    if let Some(source_id_value) = source_id.as_deref() {
        hash_parts.push(source_id_value.to_string());
    }
    if let Some(source_sub_id_value) = source_sub_id.as_deref() {
        hash_parts.push(source_sub_id_value.to_string());
    }
    let id = stable_id(
        "grounding",
        &hash_parts.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    AssistantGroundingChunk {
        id,
        source_kind: source_kind.to_string(),
        title,
        excerpt,
        score,
        visibility,
        topic_key,
        owner_user_id,
        source_id,
        source_sub_id,
        citation,
    }
}

fn chunk_search_text(chunk: &AssistantGroundingChunk) -> String {
    let mut parts = vec![
        chunk.source_kind.clone(),
        chunk.title.clone(),
        chunk.excerpt.clone(),
    ];
    if let Some(topic_key) = chunk.topic_key.as_ref() {
        parts.push(topic_key.clone());
    }
    if let Some(source_id) = chunk.source_id.as_ref() {
        parts.push(source_id.clone());
    }
    if let Some(source_sub_id) = chunk.source_sub_id.as_ref() {
        parts.push(source_sub_id.clone());
    }
    if let Some(citation) = chunk.citation.as_ref() {
        if let Some(label) = citation.label.as_ref() {
            parts.push(label.clone());
        }
        if let Some(excerpt) = citation.excerpt.as_ref() {
            parts.push(excerpt.clone());
        }
    }
    parts.join("\n")
}

fn chunk_access_scope(
    chunk: &AssistantGroundingChunk,
    user_id: &str,
) -> (String, Option<String>, Option<String>) {
    match chunk.visibility {
        AssistantGroundingVisibility::Admin => {
            ("admin".to_string(), Some(user_id.to_string()), None)
        }
        AssistantGroundingVisibility::User => ("user".to_string(), Some(user_id.to_string()), None),
        AssistantGroundingVisibility::Shared => ("shared".to_string(), None, None),
    }
}

fn metadata_json_for_chunk(chunk: &AssistantGroundingChunk) -> String {
    serde_json::to_string(&serde_json::json!({
        "source_kind": chunk.source_kind,
        "visibility": chunk.visibility,
        "topic_key": chunk.topic_key,
        "source_id": chunk.source_id,
        "source_sub_id": chunk.source_sub_id,
        "citation": chunk.citation,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn chunk_source_ts(chunk: &AssistantGroundingChunk) -> i64 {
    chunk
        .citation
        .as_ref()
        .and_then(|citation| citation.started_ts_ms)
        .map(|ts| ts / 1000)
        .unwrap_or_else(|| Utc::now().timestamp())
}

fn generic_chunk_for_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
    source_id: Option<String>,
    source_sub_id: Option<String>,
) -> AssistantGroundingChunk {
    let source_kind = call.tool.as_str();
    let topic_key = topic_key_for_tool(call, block);
    let visibility = visibility_for_tool(source_kind);
    let search_text = serde_json::to_string(&block.data).unwrap_or_else(|_| block.label.clone());
    let excerpt = compact_text(&search_text, 320);
    let citation = source_id.as_deref().map(|source_id| {
        chunk_citation(
            source_kind,
            source_id,
            source_sub_id.as_deref(),
            Some(&block.label),
            Some(&excerpt),
            None,
            None,
            None,
        )
    });

    chunk_from_parts(
        source_kind,
        block.label.clone(),
        excerpt,
        if block.status == "ok" { 1.0 } else { 0.2 },
        visibility,
        topic_key,
        Some(context.user_id.clone()),
        source_id,
        source_sub_id,
        citation,
    )
}

fn library_search_chunks_for_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
    source_id: Option<String>,
    source_sub_id: Option<String>,
) -> Vec<AssistantGroundingChunk> {
    let matches = block
        .data
        .get("matches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let match_count = block
        .data
        .get("match_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(matches.len());

    let excerpt = if match_count == 0 {
        "No matching library titles were found.".to_string()
    } else {
        let mut titles = matches
            .iter()
            .take(5)
            .filter_map(|item| item.get("title").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if titles.is_empty() {
            format!("Found {match_count} matching library titles.")
        } else {
            let extra = match_count.saturating_sub(titles.len());
            if extra > 0 {
                titles.push(format!("and {extra} more"));
            }
            format!(
                "Found {match_count} matching library titles: {}.",
                titles.join(", ")
            )
        }
    };

    let visibility = visibility_for_tool(call.tool.as_str());
    let citation = source_id.as_deref().map(|source_id| {
        chunk_citation(
            call.tool.as_str(),
            source_id,
            source_sub_id.as_deref(),
            Some(&block.label),
            Some(&excerpt),
            None,
            None,
            None,
        )
    });

    vec![chunk_from_parts(
        call.tool.as_str(),
        block.label.clone(),
        excerpt,
        if block.status == "ok" { 1.0 } else { 0.2 },
        visibility,
        Some("libraries:search".to_string()),
        Some(context.user_id.clone()),
        source_id,
        source_sub_id,
        citation,
    )]
}

fn transcript_chunks_for_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
) -> Vec<AssistantGroundingChunk> {
    let channel_id = block
        .data
        .get("channel_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let session_id = block
        .data
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let channel_name = block
        .data
        .get("channel_name")
        .and_then(Value::as_str)
        .unwrap_or(&block.label);
    let topic_key = Some(format!("transcript:{channel_id}"));
    let visibility = AssistantGroundingVisibility::User;

    let mut out = Vec::new();
    let summary_excerpt = block
        .data
        .get("transcript_excerpt")
        .and_then(Value::as_str)
        .map(|text| compact_text(text, 420))
        .unwrap_or_else(|| {
            compact_text(&serde_json::to_string(&block.data).unwrap_or_default(), 420)
        });
    let summary_citation = chunk_citation(
        call.tool.as_str(),
        session_id,
        None,
        Some(channel_name),
        Some(&summary_excerpt),
        block
            .data
            .get("started_ts")
            .and_then(Value::as_i64)
            .map(|ts| ts * 1000),
        block
            .data
            .get("ended_ts")
            .and_then(Value::as_i64)
            .map(|ts| ts * 1000),
        None,
    );
    out.push(chunk_from_parts(
        call.tool.as_str(),
        format!("Transcript summary for {channel_name}"),
        summary_excerpt,
        1.2,
        visibility,
        topic_key.clone(),
        Some(context.user_id.clone()),
        Some(session_id.to_string()),
        None,
        Some(summary_citation),
    ));

    if let Some(highlights) = block.data.get("highlights").and_then(Value::as_array) {
        for highlight in highlights.iter().take(6) {
            let entry_id = highlight
                .get("entry_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let citation_id = highlight
                .get("citation_id")
                .and_then(Value::as_str)
                .unwrap_or(entry_id);
            let text = highlight
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if text.is_empty() {
                continue;
            }
            let started_ts_ms = highlight.get("started_ts_ms").and_then(Value::as_i64);
            let ended_ts_ms = highlight.get("ended_ts_ms").and_then(Value::as_i64);
            let citation = AssistantGroundingCitation {
                citation_id: citation_id.to_string(),
                source_kind: call.tool.as_str().to_string(),
                source_id: session_id.to_string(),
                source_sub_id: Some(entry_id.to_string()),
                label: highlight
                    .get("username")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                excerpt: Some(text.to_string()),
                started_ts_ms,
                ended_ts_ms,
                url: None,
            };
            out.push(chunk_from_parts(
                call.tool.as_str(),
                format!(
                    "{} [{}-{}]",
                    highlight
                        .get("username")
                        .and_then(Value::as_str)
                        .unwrap_or("speaker"),
                    highlight
                        .get("relative_start")
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                    highlight
                        .get("relative_end")
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                ),
                compact_text(text, 260),
                1.35,
                visibility,
                topic_key.clone(),
                Some(context.user_id.clone()),
                Some(session_id.to_string()),
                Some(entry_id.to_string()),
                Some(citation),
            ));
        }
    }

    out
}

fn recent_error_chunks_for_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
) -> Vec<AssistantGroundingChunk> {
    let mut out = Vec::new();
    let summary_excerpt =
        compact_text(&serde_json::to_string(&block.data).unwrap_or_default(), 420);
    let summary_topic = Some("admin:recent_errors".to_string());
    let summary_source_id =
        first_string_field(&block.data, &["source", "kind", "status", "message"])
            .or_else(|| Some("recent_errors".to_string()));
    let citation = summary_source_id.as_deref().map(|source_id| {
        chunk_citation(
            call.tool.as_str(),
            source_id,
            None,
            Some(&block.label),
            Some(&summary_excerpt),
            None,
            None,
            None,
        )
    });
    out.push(chunk_from_parts(
        call.tool.as_str(),
        block.label.clone(),
        summary_excerpt,
        1.2,
        AssistantGroundingVisibility::Admin,
        summary_topic.clone(),
        Some(context.user_id.clone()),
        summary_source_id.clone(),
        None,
        citation,
    ));

    if let Some(failed_jobs) = block
        .data
        .get("recent_failed_jobs")
        .and_then(Value::as_array)
    {
        for job in failed_jobs.iter().take(5) {
            let kind = job.get("kind").and_then(Value::as_str).unwrap_or("job");
            let message = job
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if message.is_empty() {
                continue;
            }
            let source_id = job
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or(kind)
                .to_string();
            let title = format!("{kind} failure");
            let excerpt = compact_text(message, 260);
            let citation = chunk_citation(
                call.tool.as_str(),
                &source_id,
                None,
                Some(&title),
                Some(&excerpt),
                job.get("occurred_ts")
                    .and_then(Value::as_i64)
                    .map(|ts| ts * 1000),
                job.get("occurred_ts")
                    .and_then(Value::as_i64)
                    .map(|ts| ts * 1000),
                None,
            );
            out.push(chunk_from_parts(
                call.tool.as_str(),
                title,
                excerpt,
                1.0,
                AssistantGroundingVisibility::Admin,
                summary_topic.clone(),
                Some(context.user_id.clone()),
                Some(source_id),
                None,
                Some(citation),
            ));
        }
    }

    out
}

fn ai_runtime_chunks_for_block(
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
    source_id: Option<String>,
    source_sub_id: Option<String>,
) -> Vec<AssistantGroundingChunk> {
    let loaded = block
        .data
        .get("model")
        .and_then(|model| model.get("loaded"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let model_name = block
        .data
        .get("model")
        .and_then(|model| model.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let backend = block
        .data
        .get("model")
        .and_then(|model| model.get("backend"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let active_turns = block
        .data
        .get("scheduler")
        .and_then(|scheduler| scheduler.get("active_turns"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let queued_turns = block
        .data
        .get("scheduler")
        .and_then(|scheduler| scheduler.get("queued_turns"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let warm_pool_bytes = block
        .data
        .get("scheduler")
        .and_then(|scheduler| scheduler.get("warm_pool_bytes"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let warm_pool_budget_bytes = block
        .data
        .get("scheduler")
        .and_then(|scheduler| scheduler.get("warm_pool_budget_bytes"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let excerpt = match model_name {
        Some(model_name) if loaded => format!(
            "Loaded AI model `{model_name}` on the {backend} backend. Scheduler has {active_turns} active and {queued_turns} queued turns. Warm pool is {} of {}.",
            humanize_binary_bytes(warm_pool_bytes),
            humanize_binary_bytes(warm_pool_budget_bytes),
        ),
        Some(model_name) => format!(
            "Rustyfin AI is configured for `{model_name}` on the {backend} backend, but no model is currently loaded. Scheduler has {active_turns} active and {queued_turns} queued turns. Warm pool is {} of {}.",
            humanize_binary_bytes(warm_pool_bytes),
            humanize_binary_bytes(warm_pool_budget_bytes),
        ),
        None => format!(
            "No AI model is currently loaded. Scheduler has {active_turns} active and {queued_turns} queued turns. Warm pool is {} of {}.",
            humanize_binary_bytes(warm_pool_bytes),
            humanize_binary_bytes(warm_pool_budget_bytes),
        ),
    };
    let citation = source_id.as_deref().map(|source_id| {
        chunk_citation(
            call.tool.as_str(),
            source_id,
            source_sub_id.as_deref(),
            Some(&block.label),
            Some(&excerpt),
            None,
            None,
            None,
        )
    });

    vec![chunk_from_parts(
        call.tool.as_str(),
        block.label.clone(),
        excerpt,
        if block.status == "ok" { 1.25 } else { 0.2 },
        AssistantGroundingVisibility::Shared,
        Some("ai:runtime".to_string()),
        None,
        source_id,
        source_sub_id,
        citation,
    )]
}

fn build_chunks_from_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
    source: &super::types::AssistantGroundingSource,
) -> Vec<AssistantGroundingChunk> {
    if call.tool == super::registry::AssistantToolName::LibrarySearchTitles {
        let source_id = first_string_field(&block.data, &["library_id"])
            .or_else(|| Some(stable_id("source", &[call.tool.as_str(), &block.label])));
        let source_sub_id = first_string_field(&block.data, &["item_id"]);
        let mut chunks =
            library_search_chunks_for_block(context, call, block, source_id, source_sub_id);
        if let Some(first_chunk) = chunks.first_mut() {
            first_chunk.score = (first_chunk.score
                + match source.status.as_str() {
                    "ok" => 0.6,
                    "error" => 0.1,
                    _ => 0.2,
                })
            .min(2.0);
        }
        return chunks;
    }
    if call.tool == super::registry::AssistantToolName::ChannelsGetTranscriptSummary {
        return transcript_chunks_for_block(context, call, block);
    }
    if call.tool == super::registry::AssistantToolName::SystemGetRecentErrors {
        return recent_error_chunks_for_block(context, call, block);
    }
    if call.tool == super::registry::AssistantToolName::SystemGetAiRuntimeSummary {
        let source_id = first_string_field(&block.data, &["name"])
            .or_else(|| {
                block
                    .data
                    .get("model")
                    .and_then(|model| model.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| Some(stable_id("source", &[call.tool.as_str(), &block.label])));
        return ai_runtime_chunks_for_block(call, block, source_id, None);
    }

    let source_id = first_string_field(
        &block.data,
        &[
            "id",
            "session_id",
            "channel_id",
            "artifact_id",
            "room_id",
            "server_id",
            "library_id",
            "url",
        ],
    )
    .or_else(|| Some(stable_id("source", &[call.tool.as_str(), &block.label])));
    let source_sub_id = first_string_field(&block.data, &["entry_id", "item_id", "segment_id"]);
    let mut chunk = generic_chunk_for_block(context, call, block, source_id, source_sub_id);
    chunk.score = (chunk.score
        + match source.status.as_str() {
            "ok" => 0.6,
            "error" => 0.1,
            _ => 0.2,
        })
    .min(2.0);
    chunk.citation.get_or_insert_with(|| {
        chunk_citation(
            call.tool.as_str(),
            chunk.source_id.as_deref().unwrap_or("source"),
            chunk.source_sub_id.as_deref(),
            Some(&chunk.title),
            Some(&chunk.excerpt),
            None,
            None,
            None,
        )
    });
    vec![chunk]
}

fn maybe_topic_from_message(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("ai model")
        || lower.contains("loaded model")
        || lower.contains("inference")
        || lower.contains("warm pool")
        || lower.contains("queue depth")
        || lower.contains("scheduler")
        || (lower.contains("backend") && lower.contains("ai"))
    {
        return Some("ai:runtime".to_string());
    }
    if lower.contains("transcript") || lower.contains("call") {
        return Some("transcript:conversation".to_string());
    }
    if lower.contains("download")
        || lower.contains("extension")
        || lower.contains("artifact")
        || lower.contains("package")
    {
        return Some("downloads:catalog".to_string());
    }
    if lower.contains("library") || lower.contains("movie") || lower.contains("show") {
        return Some("libraries:accessible".to_string());
    }
    if lower.contains("error") || lower.contains("failure") {
        return Some("admin:recent_errors".to_string());
    }
    if lower.contains("server") {
        return Some("servers:catalog".to_string());
    }
    if lower.contains("room") {
        return Some("rooms:catalog".to_string());
    }
    if lower.contains("calendar") || lower.contains("birthday") {
        return Some("calendar:recent".to_string());
    }
    if lower.contains("person summary")
        || lower.contains("profile summary")
        || lower.contains("person profile")
        || lower.contains("profile details")
        || lower.contains("profile info")
        || lower.contains("profile information")
        || lower.contains("person details")
        || lower.contains("person info")
        || lower.contains("person information")
    {
        return Some("memory:people".to_string());
    }
    None
}

pub fn derive_topic_key_from_history(history: &[AssistantHistoryMessage]) -> Option<String> {
    history.iter().rev().find_map(|message| {
        if !message.role.eq_ignore_ascii_case("assistant") {
            return None;
        }
        message
            .grounding_chunks
            .iter()
            .find_map(|chunk| chunk.topic_key.clone())
            .or_else(|| {
                message.follow_up_contexts.iter().find_map(|context| {
                    context
                        .entities
                        .iter()
                        .find_map(|entity| entity.topic_key.clone())
                        .or_else(|| maybe_topic_from_message(&context.label))
                })
            })
    })
}

async fn search_memory_chunks(
    state: &AppState,
    context: &AssistantContext,
    topic_key: Option<&str>,
    query: Option<&str>,
) -> Vec<AssistantGroundingChunk> {
    let allowed_library_ids = if context.is_admin {
        None
    } else {
        match rustfin_db::repo::users::get_library_access(&state.db, &context.user_id).await {
            Ok(library_ids) if !library_ids.is_empty() => Some(library_ids),
            Ok(_) => Some(Vec::new()),
            Err(error) => {
                warn!(error = %error, "failed to load library access for grounding search");
                None
            }
        }
    };

    let mut chunks = Vec::new();
    match rustfin_db::repo::ai_grounding::search_memory_items_for_user(
        &state.db,
        &context.user_id,
        topic_key,
        query,
        MAX_RETRIEVAL_HITS,
    )
    .await
    {
        Ok(hits) => {
            for hit in hits {
                let row = hit.row;
                chunks.push(AssistantGroundingChunk {
                    id: format!("memory:{}", row.memory_key),
                    source_kind: row.memory_type.clone(),
                    title: row.title.clone(),
                    excerpt: row.content.clone(),
                    score: hit.rank + row.weight,
                    visibility: match row.memory_type.as_str() {
                        "system_get_host_runtime_summary"
                        | "system_get_backup_summary"
                        | "system_get_service_health"
                        | "system_get_transcode_summary"
                        | "system_get_storage_summary"
                        | "system_get_recent_errors" => AssistantGroundingVisibility::Admin,
                        _ => AssistantGroundingVisibility::User,
                    },
                    topic_key: row.topic_key.clone(),
                    owner_user_id: Some(row.user_id.clone()),
                    source_id: Some(row.memory_key.clone()),
                    source_sub_id: None,
                    citation: Some(chunk_citation(
                        &row.memory_type,
                        &row.memory_key,
                        None,
                        Some(&row.title),
                        Some(&row.content),
                        Some(row.created_ts * 1000),
                        Some(row.updated_ts * 1000),
                        None,
                    )),
                });
            }
        }
        Err(error) => {
            warn!(error = %error, "failed to search AI memory items");
        }
    }

    match rustfin_db::repo::ai_grounding::search_retrieval_chunks(
        &state.db,
        &context.user_id,
        context.is_admin,
        allowed_library_ids.as_deref(),
        topic_key,
        query,
        MAX_RETRIEVAL_HITS,
    )
    .await
    {
        Ok(hits) => {
            for hit in hits {
                let row = hit.row;
                chunks.push(AssistantGroundingChunk {
                    id: row.chunk_key.clone(),
                    source_kind: row.source_kind.clone(),
                    title: row.title.clone(),
                    excerpt: row.excerpt.clone(),
                    score: hit.rank + row.score_boost,
                    visibility: match row.access_scope.as_str() {
                        "admin" => AssistantGroundingVisibility::Admin,
                        "user" | "library" => AssistantGroundingVisibility::User,
                        _ => AssistantGroundingVisibility::Shared,
                    },
                    topic_key: row.topic_key.clone(),
                    owner_user_id: row.owner_user_id.clone(),
                    source_id: Some(row.source_id.clone()),
                    source_sub_id: row.source_sub_id.clone(),
                    citation: Some(chunk_citation(
                        &row.source_kind,
                        &row.source_id,
                        row.source_sub_id.as_deref(),
                        Some(&row.title),
                        Some(&row.excerpt),
                        Some(row.source_ts * 1000),
                        Some(row.source_ts * 1000),
                        None,
                    )),
                });
            }
        }
        Err(error) => {
            warn!(error = %error, "failed to search AI retrieval chunks");
        }
    }

    chunks
}

pub async fn build_grounding_chunks_for_turn(
    state: &AppState,
    context: &AssistantContext,
    request: &AssistantChatRequest,
    planned_tools: &[PlannedToolCall],
    grounding_blocks: &[AssistantToolContextBlock],
    grounding_sources: &[super::types::AssistantGroundingSource],
    history: &[AssistantHistoryMessage],
) -> Vec<AssistantGroundingChunk> {
    let topic_key = derive_topic_key_from_history(history)
        .or_else(|| maybe_topic_from_message(&request.message));
    let query = request.message.trim();
    let query = if query.is_empty() { None } else { Some(query) };

    let mut chunks = if topic_key.is_some() || query.is_some() {
        search_memory_chunks(state, context, topic_key.as_deref(), query).await
    } else {
        Vec::new()
    };

    for ((call, block), source) in planned_tools
        .iter()
        .zip(grounding_blocks.iter())
        .zip(grounding_sources.iter())
    {
        chunks.extend(build_chunks_from_block(context, call, block, source));
    }

    rank_and_compress_grounding_chunks(
        &chunks,
        super::replies::MAX_GROUNDING_CHUNKS,
        super::replies::MAX_GROUNDING_PROMPT_CHARS,
    )
}

pub async fn persist_grounding_artifacts(
    state: &AppState,
    context: &AssistantContext,
    conversation_id: &str,
    turn_id: &str,
    grounding_chunks: &[AssistantGroundingChunk],
    follow_up_contexts: &[AssistantFollowUpContext],
) {
    for chunk in grounding_chunks {
        let (access_scope, access_key, owner_user_id) = chunk_access_scope(chunk, &context.user_id);
        let search_text = chunk_search_text(chunk);
        let metadata_json = metadata_json_for_chunk(chunk);
        let source_id = chunk.source_id.as_deref().unwrap_or(&chunk.id).to_string();
        let retrieval_params = rustfin_db::repo::ai_grounding::UpsertAiRetrievalChunkParams {
            chunk_key: &chunk.id,
            source_kind: &chunk.source_kind,
            source_id: &source_id,
            source_sub_id: chunk.source_sub_id.as_deref(),
            owner_user_id: owner_user_id.as_deref(),
            access_scope: &access_scope,
            access_key: access_key.as_deref(),
            topic_key: chunk.topic_key.as_deref(),
            title: &chunk.title,
            excerpt: &chunk.excerpt,
            search_text: &search_text,
            score_boost: chunk.score,
            metadata_json: &metadata_json,
            source_ts: chunk_source_ts(chunk),
        };
        if let Err(error) =
            rustfin_db::repo::ai_grounding::upsert_retrieval_chunk(&state.db, retrieval_params)
                .await
        {
            warn!(error = %error, chunk_id = %chunk.id, "failed to persist grounding chunk");
        }

        if chunk.visibility != AssistantGroundingVisibility::Shared {
            let memory_search_text = chunk_search_text(chunk);
            let memory_params = rustfin_db::repo::ai_grounding::UpsertAiMemoryItemParams {
                user_id: &context.user_id,
                memory_key: &chunk.id,
                memory_type: &chunk.source_kind,
                topic_key: chunk.topic_key.as_deref(),
                title: &chunk.title,
                content: &chunk.excerpt,
                search_text: &memory_search_text,
                weight: chunk.score,
            };
            if let Err(error) =
                rustfin_db::repo::ai_grounding::upsert_memory_item(&state.db, memory_params).await
            {
                warn!(error = %error, chunk_id = %chunk.id, "failed to persist AI memory item");
            }
        }
    }

    if let Err(error) =
        rustfin_db::repo::ai_grounding::delete_entity_nodes_for_turn(&state.db, turn_id).await
    {
        warn!(error = %error, turn_id = %turn_id, "failed to reset entity graph rows");
    }

    let chunk_lookup: HashMap<_, _> = grounding_chunks
        .iter()
        .map(|chunk| (chunk.id.clone(), chunk.clone()))
        .collect();

    for context_block in follow_up_contexts {
        let source_chunk_id = context_block
            .entities
            .iter()
            .find_map(|entity| entity.source_chunk_id.clone())
            .or_else(|| {
                grounding_chunks
                    .iter()
                    .find(|chunk| {
                        chunk.source_kind == context_block.tool
                            || chunk.title == context_block.label
                            || chunk.topic_key.as_deref()
                                == context_block.input_hint.calendar_label.as_deref()
                    })
                    .map(|chunk| chunk.id.clone())
            });
        let topic_key = context_block
            .entities
            .iter()
            .find_map(|entity| entity.topic_key.clone())
            .or_else(|| {
                source_chunk_id
                    .as_ref()
                    .and_then(|chunk_id| chunk_lookup.get(chunk_id))
                    .and_then(|chunk| chunk.topic_key.clone())
            });
        let root_node_key = stable_id(
            "entity-context",
            &[
                conversation_id,
                turn_id,
                &context_block.tool,
                &context_block.label,
            ],
        );
        let root_metadata = serde_json::to_string(&serde_json::json!({
            "tool": context_block.tool,
            "label": context_block.label,
            "input_hint": context_block.input_hint,
        }))
        .unwrap_or_else(|_| "{}".to_string());
        let root_params = rustfin_db::repo::ai_grounding::UpsertAiEntityNodeParams {
            node_key: &root_node_key,
            owner_user_id: Some(&context.user_id),
            conversation_id: Some(conversation_id),
            turn_id: Some(turn_id),
            entity_kind: &context_block.tool,
            label: &context_block.label,
            identifier: None,
            topic_key: topic_key.as_deref(),
            source_chunk_id: source_chunk_id.as_deref(),
            access_scope: match context_block.tool.as_str() {
                "system_get_host_runtime_summary"
                | "system_get_backup_summary"
                | "system_get_service_health"
                | "system_get_transcode_summary"
                | "system_get_storage_summary"
                | "system_get_recent_errors" => "admin",
                _ => "user",
            },
            access_key: None,
            ordinal: 0,
            metadata_json: &root_metadata,
        };
        let root_access_scope = root_params.access_scope;

        if let Err(error) =
            rustfin_db::repo::ai_grounding::upsert_entity_node(&state.db, root_params).await
        {
            warn!(error = %error, turn_id = %turn_id, "failed to persist entity context node");
        }

        for entity in &context_block.entities {
            let node_key = stable_id(
                "entity-node",
                &[
                    conversation_id,
                    turn_id,
                    &context_block.tool,
                    &context_block.label,
                    &entity.label,
                    &entity.ordinal.to_string(),
                ],
            );
            let metadata_json = serde_json::to_string(entity).unwrap_or_else(|_| "{}".to_string());
            let entity_params = rustfin_db::repo::ai_grounding::UpsertAiEntityNodeParams {
                node_key: &node_key,
                owner_user_id: Some(&context.user_id),
                conversation_id: Some(conversation_id),
                turn_id: Some(turn_id),
                entity_kind: entity.kind.as_deref().unwrap_or("entity"),
                label: &entity.label,
                identifier: entity.identifier.as_deref(),
                topic_key: entity.topic_key.as_deref().or(topic_key.as_deref()),
                source_chunk_id: entity
                    .source_chunk_id
                    .as_deref()
                    .or(source_chunk_id.as_deref()),
                access_scope: root_access_scope,
                access_key: None,
                ordinal: entity.ordinal as i64,
                metadata_json: &metadata_json,
            };
            if let Err(error) =
                rustfin_db::repo::ai_grounding::upsert_entity_node(&state.db, entity_params).await
            {
                warn!(error = %error, turn_id = %turn_id, "failed to persist entity node");
            }

            let edge_key = stable_id(
                "entity-edge",
                &[
                    conversation_id,
                    turn_id,
                    &context_block.tool,
                    &context_block.label,
                    &entity.label,
                    &entity.ordinal.to_string(),
                ],
            );
            let edge_params = rustfin_db::repo::ai_grounding::UpsertAiEntityEdgeParams {
                edge_key: &edge_key,
                from_node_key: &root_node_key,
                to_node_key: &node_key,
                relation: "contains",
                weight: 1.0,
            };
            if let Err(error) =
                rustfin_db::repo::ai_grounding::upsert_entity_edge(&state.db, edge_params).await
            {
                warn!(error = %error, turn_id = %turn_id, "failed to persist entity edge");
            }
        }
    }
}

fn group_entity_rows_into_contexts(
    rows: &[rustfin_db::repo::ai_grounding::AiEntityNodeHit],
) -> Vec<AssistantFollowUpContext> {
    let mut contexts = Vec::new();
    let mut index = 0usize;

    while index < rows.len() && contexts.len() < MAX_ENTITY_GRAPH_CONTEXTS {
        let root = &rows[index].row;
        if root.ordinal != 0 {
            index += 1;
            continue;
        }

        let turn_id = root.turn_id.clone().unwrap_or_default();
        let source_chunk_id = root.source_chunk_id.clone();
        let mut entities = Vec::new();
        let mut cursor = index + 1;
        while cursor < rows.len() {
            let row = &rows[cursor].row;
            if row.turn_id != root.turn_id || row.source_chunk_id != source_chunk_id {
                break;
            }
            if row.ordinal > 0 {
                entities.push(AssistantFollowUpEntity {
                    ordinal: row.ordinal as usize,
                    label: row.label.clone(),
                    identifier: row.identifier.clone(),
                    kind: Some(row.entity_kind.clone()),
                    topic_key: row.topic_key.clone(),
                    source_chunk_id: row.source_chunk_id.clone(),
                });
            }
            cursor += 1;
        }

        if !entities.is_empty() {
            contexts.push(AssistantFollowUpContext {
                tool: root.entity_kind.clone(),
                label: root.label.clone(),
                input_hint: Default::default(),
                entities,
            });
        } else if !turn_id.is_empty() {
            contexts.push(AssistantFollowUpContext {
                tool: root.entity_kind.clone(),
                label: root.label.clone(),
                input_hint: Default::default(),
                entities: vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: root.label.clone(),
                    identifier: root.identifier.clone(),
                    kind: Some(root.entity_kind.clone()),
                    topic_key: root.topic_key.clone(),
                    source_chunk_id: root.source_chunk_id.clone(),
                }],
            });
        }

        index = cursor.max(index + 1);
    }

    contexts
}

pub async fn augment_history_with_entity_graph(
    state: &AppState,
    context: &AssistantContext,
    history: &[AssistantHistoryMessage],
    message: &str,
) -> Vec<AssistantHistoryMessage> {
    let topic_key =
        derive_topic_key_from_history(history).or_else(|| maybe_topic_from_message(message));
    let query = message.trim();
    let query = if query.is_empty() { None } else { Some(query) };
    let rows = match rustfin_db::repo::ai_grounding::search_entity_nodes_for_user(
        &state.db,
        &context.user_id,
        context.is_admin,
        topic_key.as_deref(),
        query,
        24,
    )
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            warn!(error = %error, "failed to search entity graph");
            return history.to_vec();
        }
    };

    let contexts = group_entity_rows_into_contexts(&rows);
    if contexts.is_empty() {
        return history.to_vec();
    }

    let mut augmented = history.to_vec();
    for context_block in contexts {
        augmented.push(AssistantHistoryMessage {
            role: "assistant".to_string(),
            content: String::new(),
            grounding_tools: Vec::new(),
            follow_up_contexts: vec![context_block],
            grounding_chunks: Vec::new(),
        });
    }

    augmented
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{AssistantToolContextBlock, AssistantToolInput};
    use serde_json::json;

    #[test]
    fn topic_key_from_history_prefers_latest_assistant_context() {
        let history = vec![AssistantHistoryMessage {
            role: "assistant".to_string(),
            content: String::new(),
            grounding_tools: Vec::new(),
            follow_up_contexts: vec![AssistantFollowUpContext {
                tool: "library_search_titles".to_string(),
                label: "Library matches".to_string(),
                input_hint: Default::default(),
                entities: vec![AssistantFollowUpEntity {
                    ordinal: 1,
                    label: "Star Trek".to_string(),
                    identifier: None,
                    kind: Some("item".to_string()),
                    topic_key: Some("library:abc".to_string()),
                    source_chunk_id: None,
                }],
            }],
            grounding_chunks: Vec::new(),
        }];

        assert_eq!(
            derive_topic_key_from_history(&history).as_deref(),
            Some("library:abc")
        );
    }

    #[test]
    fn maybe_topic_from_message_maps_common_topics() {
        assert_eq!(
            maybe_topic_from_message("what downloads are available"),
            Some("downloads:catalog".to_string())
        );
        assert_eq!(
            maybe_topic_from_message("give me a person summary for Rachel"),
            Some("memory:people".to_string())
        );
    }

    #[test]
    fn topic_key_for_detail_tools_uses_specific_entities() {
        let download_call = PlannedToolCall {
            tool: AssistantToolName::DownloadsGetArtifactDetails,
            input: AssistantToolInput::DownloadsFilter {
                query: Some("RustyVault".to_string()),
                availability: None,
            },
        };
        let download_block = AssistantToolContextBlock {
            tool: "downloads_get_artifact_details",
            label: "Download artifact details".to_string(),
            status: "ok",
            data: json!({
                "id": "download-1",
                "artifact_id": "rustyvault-webext"
            }),
        };
        assert_eq!(
            topic_key_for_tool(&download_call, &download_block).as_deref(),
            Some("downloads:rustyvault-webext")
        );

        let library_call = PlannedToolCall {
            tool: AssistantToolName::LibrariesGetLibrarySummary,
            input: AssistantToolInput::LibrarySearch {
                query: "Movies".to_string(),
            },
        };
        let library_block = AssistantToolContextBlock {
            tool: "libraries_get_library_summary",
            label: "Library summary".to_string(),
            status: "ok",
            data: json!({
                "id": "library-1",
                "name": "Movies",
                "kind": "movie",
                "item_count": 42,
                "paths": [],
                "settings": {
                    "show_images": true,
                    "prefer_local_artwork": true,
                    "fetch_online_artwork": true,
                    "tmdb_store_in_media_dir": false,
                    "tmdb_sync_on_new_media": true,
                    "tmdb_sync_schedule": "manual",
                    "tmdb_last_sync_ts": null,
                    "tmdb_fetch_posters": true,
                    "tmdb_fetch_backdrops": true,
                    "tmdb_fetch_metadata": true,
                    "tmdb_fetch_reviews": false
                },
                "created_ts": 0,
                "updated_ts": 0
            }),
        };
        assert_eq!(
            topic_key_for_tool(&library_call, &library_block).as_deref(),
            Some("library:library-1")
        );

        let memory_person_call = PlannedToolCall {
            tool: AssistantToolName::MemoryGetPersonSummary,
            input: AssistantToolInput::SystemService {
                query: "Rachel".to_string(),
            },
        };
        let memory_person_block = AssistantToolContextBlock {
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
                    "created_ts": 0,
                    "updated_ts": 0
                },
                "relation_count": 0,
                "relations": []
            }),
        };
        assert_eq!(
            topic_key_for_tool(&memory_person_call, &memory_person_block).as_deref(),
            Some("memory:people")
        );
    }

    #[test]
    fn topic_key_for_network_and_system_detail_tools_is_specific() {
        let network_default_route_call = PlannedToolCall {
            tool: AssistantToolName::NetworkGetDefaultRoute,
            input: AssistantToolInput::NetworkDefaultRoute { query: None },
        };
        let network_default_route_block = AssistantToolContextBlock {
            tool: "network_get_default_route",
            label: "Default route".to_string(),
            status: "ok",
            data: json!({
                "routes": [
                    {
                        "route": "default via 192.168.0.1 dev enp3s0"
                    }
                ]
            }),
        };
        assert_eq!(
            topic_key_for_tool(&network_default_route_call, &network_default_route_block)
                .as_deref(),
            Some("network:default_route")
        );

        let network_hostname_aliases_call = PlannedToolCall {
            tool: AssistantToolName::NetworkGetHostnameAliases,
            input: AssistantToolInput::NetworkHostnameAliases { query: None },
        };
        let network_hostname_aliases_block = AssistantToolContextBlock {
            tool: "network_get_hostname_aliases",
            label: "Hostname aliases".to_string(),
            status: "ok",
            data: json!({
                "aliases": [
                    {
                        "name": "server",
                        "source": "hostname -a"
                    }
                ]
            }),
        };
        assert_eq!(
            topic_key_for_tool(
                &network_hostname_aliases_call,
                &network_hostname_aliases_block
            )
            .as_deref(),
            Some("network:hostname_aliases")
        );

        let system_port_conflicts_call = PlannedToolCall {
            tool: AssistantToolName::SystemGetPortConflicts,
            input: AssistantToolInput::SystemPortConflicts { query: None },
        };
        let system_port_conflicts_block = AssistantToolContextBlock {
            tool: "system_get_port_conflicts",
            label: "Port conflicts".to_string(),
            status: "ok",
            data: json!({
                "conflicts": [
                    {
                        "protocol": "tcp",
                        "state": "LISTEN",
                        "local_address": "127.0.0.1",
                        "raw_entry": "LISTEN ..."
                    }
                ]
            }),
        };
        assert_eq!(
            topic_key_for_tool(&system_port_conflicts_call, &system_port_conflicts_block)
                .as_deref(),
            Some("system:port_conflicts")
        );

        let system_failed_units_call = PlannedToolCall {
            tool: AssistantToolName::SystemGetFailedUnits,
            input: AssistantToolInput::SystemFailedUnits { query: None },
        };
        let system_failed_units_block = AssistantToolContextBlock {
            tool: "system_get_failed_units",
            label: "Failed units".to_string(),
            status: "ok",
            data: json!({
                "units": [
                    {
                        "name": "rustfin.service",
                        "load": "loaded",
                        "active": "failed",
                        "sub": "failed",
                        "description": "Rustyfin native service"
                    }
                ]
            }),
        };
        assert_eq!(
            topic_key_for_tool(&system_failed_units_call, &system_failed_units_block).as_deref(),
            Some("system:failed_units")
        );
    }

    #[test]
    fn visibility_for_tool_marks_admin_only_sources() {
        assert_eq!(
            visibility_for_tool("system_get_recent_errors"),
            AssistantGroundingVisibility::Admin
        );
        assert_eq!(
            visibility_for_tool("system_get_port_conflicts"),
            AssistantGroundingVisibility::Admin
        );
        assert_eq!(
            visibility_for_tool("system_get_failed_units"),
            AssistantGroundingVisibility::Admin
        );
        assert_eq!(
            visibility_for_tool("downloads_list_available_artifacts"),
            AssistantGroundingVisibility::Shared
        );
        assert_eq!(
            visibility_for_tool("downloads_get_artifact_details"),
            AssistantGroundingVisibility::Shared
        );
        assert_eq!(
            visibility_for_tool("network_get_default_route"),
            AssistantGroundingVisibility::Shared
        );
        assert_eq!(
            visibility_for_tool("network_get_hostname_aliases"),
            AssistantGroundingVisibility::Shared
        );
    }

    #[test]
    fn chunk_access_scope_matches_visibility() {
        let shared_chunk = AssistantGroundingChunk {
            id: "chunk-1".to_string(),
            source_kind: "downloads".to_string(),
            title: "Downloads".to_string(),
            excerpt: "Available artifacts".to_string(),
            score: 1.0,
            visibility: AssistantGroundingVisibility::Shared,
            topic_key: None,
            owner_user_id: None,
            source_id: None,
            source_sub_id: None,
            citation: None,
        };
        let admin_chunk = AssistantGroundingChunk {
            visibility: AssistantGroundingVisibility::Admin,
            ..shared_chunk.clone()
        };

        assert_eq!(
            chunk_access_scope(&shared_chunk, "user-1"),
            ("shared".to_string(), None, None)
        );
        assert_eq!(
            chunk_access_scope(&admin_chunk, "user-1"),
            ("admin".to_string(), Some("user-1".to_string()), None,)
        );
    }

    #[test]
    fn library_search_chunks_use_compact_summary() {
        let context = AssistantContext {
            trace_id: "trace".to_string(),
            user_id: "user-1".to_string(),
            username: "user".to_string(),
            role: "user".to_string(),
            is_admin: false,
            confirmed_write_tool: None,
            conversation_id: None,
        };
        let call = PlannedToolCall {
            tool: AssistantToolName::LibrarySearchTitles,
            input: AssistantToolInput::LibrarySearch {
                query: "Star Trek".to_string(),
            },
        };
        let block = AssistantToolContextBlock {
            tool: "library_search_titles",
            label: "Library matches for \"Star Trek\"".to_string(),
            status: "ok",
            data: serde_json::json!({
                "match_count": 0,
                "matches": [],
                "query": "Star Trek"
            }),
        };
        let chunks = library_search_chunks_for_block(&context, &call, &block, None, None);
        let prompt = crate::ai_assistant::replies::grounding_chunks_prompt(&chunks);

        assert!(prompt.contains("Library matches for \"Star Trek\""));
        assert!(prompt.contains("No matching library titles were found."));
        assert!(!prompt.contains("\"query\""));
        assert!(!prompt.contains("\"matches\""));
        assert!(!prompt.contains("[{"));
        assert!(!prompt.contains("library_query:"));
    }
}
