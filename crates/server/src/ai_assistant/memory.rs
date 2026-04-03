use std::collections::HashMap;

use chrono::Utc;
use rustfin_ai_agent::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::warn;

use super::context::AssistantContext;
use super::memory_selector::{
    AssistantMemoryKind, AssistantMemoryMetadata, AssistantMemorySelectorCandidate,
    score_memory_candidate,
};
use super::operational_index::search_operational_chunks;
use super::replies::{compact_text, rank_and_compress_grounding_chunks};
use super::types::{
    AssistantChatRequest, AssistantFollowUpContext, AssistantFollowUpEntity,
    AssistantGroundingChunk, AssistantGroundingCitation, AssistantGroundingVisibility,
    AssistantHistoryMessage, AssistantToolContextBlock, PlannedToolCall,
};
use crate::state::AppState;

const MAX_MEMORY_SUMMARY_CHARS: usize = 900;
const MAX_MEMORY_ITEM_CHARS: usize = 140;
const MAX_MEMORY_LIST_ITEMS: usize = 6;
const MAX_RECENT_GROUNDED_CONTEXTS: usize = 6;
const MAX_RECENT_GROUNDED_ENTITIES: usize = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationMemoryState {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub durable_facts: Vec<String>,
    #[serde(default)]
    pub user_preferences: Vec<String>,
    #[serde(default)]
    pub open_loops: Vec<String>,
    #[serde(default)]
    pub active_topics: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationPromptDebug {
    pub context_length: u32,
    pub prompt_budget_tokens: u32,
    pub reserved_completion_tokens: u32,
    pub prompt_tokens_estimate: u32,
    pub loaded_history_turns: u32,
    pub retained_raw_turns: u32,
    pub summarized_turns: u32,
    pub recent_grounded_context_count: u32,
    pub used_memory_summary: bool,
    pub memory_turn_index: i64,
    pub memory_summary_chars: usize,
    pub compact_boundary_count: u32,
    pub recovered_from_compact_boundary: bool,
}

#[derive(Debug, Clone)]
pub struct ConversationPromptAssembly {
    pub messages: Vec<ChatMessage>,
    pub debug: ConversationPromptDebug,
    pub pending_summary_turns: Vec<AssistantHistoryMessage>,
    pub pending_summary_last_turn_index: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CompactGroundedContext {
    tool: String,
    label: String,
    entities: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemorySummaryInput<'a> {
    existing_memory: &'a ConversationMemoryState,
    turns: Vec<MemorySummaryTurn>,
}

#[derive(Debug, Serialize)]
struct MemorySummaryTurn {
    role: String,
    content: String,
    grounding_tools: Vec<String>,
    grounded_context: Vec<CompactGroundedContext>,
}

pub fn parse_memory_state_json(raw: &str) -> ConversationMemoryState {
    serde_json::from_str::<ConversationMemoryState>(raw)
        .map(ConversationMemoryState::normalized)
        .unwrap_or_default()
}

pub fn memory_state_json(state: &ConversationMemoryState) -> String {
    serde_json::to_string(&state.clone().normalized()).unwrap_or_else(|_| "{}".to_string())
}

impl ConversationMemoryState {
    pub fn is_empty(&self) -> bool {
        self.summary.trim().is_empty()
            && self.durable_facts.is_empty()
            && self.user_preferences.is_empty()
            && self.open_loops.is_empty()
            && self.active_topics.is_empty()
    }

    pub fn normalized(mut self) -> Self {
        self.summary = truncate_single_line(&self.summary, MAX_MEMORY_SUMMARY_CHARS);
        self.durable_facts = normalize_memory_items(self.durable_facts);
        self.user_preferences = normalize_memory_items(self.user_preferences);
        self.open_loops = normalize_memory_items(self.open_loops);
        self.active_topics = normalize_memory_items(self.active_topics);
        self
    }
}

pub fn build_generation_prompt_messages<F>(
    system_prompt: &str,
    local_now_text: &str,
    grounding_chunks: &[AssistantGroundingChunk],
    history: &[AssistantHistoryMessage],
    current_message: &str,
    memory_state: &ConversationMemoryState,
    memory_turn_index: i64,
    context_length: u32,
    prompt_budget_tokens: u32,
    reserved_completion_tokens: u32,
    count_tokens: F,
) -> ConversationPromptAssembly
where
    F: Fn(&[ChatMessage]) -> u32,
{
    let recent_grounded_contexts = collect_recent_grounded_contexts(history);
    let system_messages = build_system_messages(
        system_prompt,
        local_now_text,
        grounding_chunks,
        memory_state,
        &recent_grounded_contexts,
    );
    let raw_history_start = next_raw_history_start(memory_turn_index, history.len());
    let raw_history = &history[raw_history_start..];

    let mut best_messages = compose_messages(&system_messages, raw_history, current_message);
    let mut best_prompt_tokens = count_tokens(&best_messages);
    let mut keep_from = 0usize;

    if best_prompt_tokens > prompt_budget_tokens {
        for candidate_keep_from in 1..=raw_history.len() {
            let candidate_messages = compose_messages(
                &system_messages,
                &raw_history[candidate_keep_from..],
                current_message,
            );
            let candidate_tokens = count_tokens(&candidate_messages);
            if candidate_tokens <= prompt_budget_tokens {
                keep_from = candidate_keep_from;
                best_messages = candidate_messages;
                best_prompt_tokens = candidate_tokens;
                break;
            }
            if candidate_keep_from == raw_history.len() {
                keep_from = candidate_keep_from;
                best_messages = candidate_messages;
                best_prompt_tokens = candidate_tokens;
            }
        }
    }

    let pending_summary_turns = raw_history[..keep_from].to_vec();
    let pending_summary_last_turn_index = if keep_from == 0 {
        None
    } else {
        Some(raw_history_start as i64 + keep_from as i64 - 1)
    };

    ConversationPromptAssembly {
        messages: best_messages,
        debug: ConversationPromptDebug {
            context_length,
            prompt_budget_tokens,
            reserved_completion_tokens,
            prompt_tokens_estimate: best_prompt_tokens,
            loaded_history_turns: history.len() as u32,
            retained_raw_turns: raw_history.len().saturating_sub(keep_from) as u32,
            summarized_turns: keep_from as u32,
            recent_grounded_context_count: recent_grounded_contexts.len() as u32,
            used_memory_summary: !memory_state.is_empty(),
            memory_turn_index,
            memory_summary_chars: memory_state.summary.len(),
            compact_boundary_count: 0,
            recovered_from_compact_boundary: false,
        },
        pending_summary_turns,
        pending_summary_last_turn_index,
    }
}

pub fn build_memory_update_messages(
    existing_memory: &ConversationMemoryState,
    turns: &[AssistantHistoryMessage],
) -> Vec<ChatMessage> {
    let input = MemorySummaryInput {
        existing_memory,
        turns: turns
            .iter()
            .map(|turn| MemorySummaryTurn {
                role: turn.role.clone(),
                content: truncate_single_line(&turn.content, 500),
                grounding_tools: turn.grounding_tools.clone(),
                grounded_context: turn
                    .follow_up_contexts
                    .iter()
                    .map(compact_grounded_context)
                    .collect(),
            })
            .collect(),
    };

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You maintain compact long-term memory for a Rustyfin assistant conversation. Merge the existing memory with the provided older turns. Keep only durable, useful context for future turns. Preserve user preferences, established facts, unresolved tasks, and the active topic. Do not invent details. Output strict JSON with exactly these keys: summary, durable_facts, user_preferences, open_loops, active_topics.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Update the conversation memory from this JSON input. Keep the summary under 120 words. Keep each list item short.\n{}",
                serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
            ),
        },
    ]
}

pub fn parse_memory_update_response(raw: &str) -> Option<ConversationMemoryState> {
    serde_json::from_str::<ConversationMemoryState>(raw)
        .ok()
        .or_else(|| {
            let start = raw.find('{')?;
            let end = raw.rfind('}')?;
            serde_json::from_str::<ConversationMemoryState>(&raw[start..=end]).ok()
        })
        .map(ConversationMemoryState::normalized)
}

pub fn fallback_memory_update(
    existing_memory: &ConversationMemoryState,
    turns: &[AssistantHistoryMessage],
) -> ConversationMemoryState {
    let mut summary_fragments = Vec::new();
    let recent_user_turns = turns
        .iter()
        .filter(|turn| turn.role == "user")
        .collect::<Vec<_>>();
    for turn in recent_user_turns
        .iter()
        .rev()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        summary_fragments.push(format!(
            "User asked: {}",
            truncate_single_line(&turn.content, 180)
        ));
    }
    for context in collect_recent_grounded_contexts(turns).into_iter().take(3) {
        summary_fragments.push(format!("Grounded context: {}", context.label));
    }

    let mut next = existing_memory.clone();
    let fragment = summary_fragments.join(" ");
    if !fragment.is_empty() {
        next.summary = truncate_single_line(
            &merge_sentences(&existing_memory.summary, &fragment),
            MAX_MEMORY_SUMMARY_CHARS,
        );
    }

    next.active_topics = merge_memory_items(
        &existing_memory.active_topics,
        &collect_recent_grounded_contexts(turns)
            .into_iter()
            .map(|context| context.label)
            .collect::<Vec<_>>(),
    );
    next.open_loops = merge_memory_items(
        &existing_memory.open_loops,
        &turns
            .iter()
            .filter(|turn| turn.role == "user" && turn.content.contains('?'))
            .map(|turn| truncate_single_line(&turn.content, MAX_MEMORY_ITEM_CHARS))
            .collect::<Vec<_>>(),
    );
    next.normalized()
}

fn build_system_messages(
    system_prompt: &str,
    local_now_text: &str,
    grounding_chunks: &[AssistantGroundingChunk],
    memory_state: &ConversationMemoryState,
    recent_grounded_contexts: &[AssistantFollowUpContext],
) -> Vec<ChatMessage> {
    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        ChatMessage {
            role: "system".to_string(),
            content: local_now_text.to_string(),
        },
    ];

    if let Some(message) = build_memory_system_message(memory_state) {
        messages.push(message);
    }
    if let Some(message) = build_recent_grounded_context_message(recent_grounded_contexts) {
        messages.push(message);
    }
    if !grounding_chunks.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding for this turn:\n{}",
                super::replies::grounding_chunks_prompt(grounding_chunks)
            ),
        });
    }

    messages
}

fn build_memory_system_message(memory_state: &ConversationMemoryState) -> Option<ChatMessage> {
    if memory_state.is_empty() {
        return None;
    }

    let mut sections = Vec::new();
    if !memory_state.summary.is_empty() {
        sections.push(format!("Summary: {}", memory_state.summary));
    }
    append_labeled_items(&mut sections, "Durable facts", &memory_state.durable_facts);
    append_labeled_items(
        &mut sections,
        "User preferences",
        &memory_state.user_preferences,
    );
    append_labeled_items(&mut sections, "Open loops", &memory_state.open_loops);
    append_labeled_items(&mut sections, "Active topics", &memory_state.active_topics);

    Some(ChatMessage {
        role: "system".to_string(),
        content: format!(
            "Persisted Rustyfin conversation memory distilled from older turns. Prefer this memory when older raw turns are omitted.\n{}",
            sections.join("\n")
        ),
    })
}

fn build_recent_grounded_context_message(
    contexts: &[AssistantFollowUpContext],
) -> Option<ChatMessage> {
    if contexts.is_empty() {
        return None;
    }

    let payload = contexts
        .iter()
        .map(compact_grounded_context)
        .collect::<Vec<_>>();

    Some(ChatMessage {
        role: "system".to_string(),
        content: format!(
            "Recent grounded context from prior turns for continuity:\n{}",
            serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string())
        ),
    })
}

fn compose_messages(
    system_messages: &[ChatMessage],
    history: &[AssistantHistoryMessage],
    current_message: &str,
) -> Vec<ChatMessage> {
    let mut messages = system_messages.to_vec();
    for turn in history {
        messages.push(ChatMessage {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: current_message.to_string(),
    });
    messages
}

fn collect_recent_grounded_contexts(
    history: &[AssistantHistoryMessage],
) -> Vec<AssistantFollowUpContext> {
    let mut collected = Vec::new();
    for turn in history.iter().rev() {
        for context in turn.follow_up_contexts.iter().rev() {
            collected.push(trim_follow_up_context(context));
            if collected.len() >= MAX_RECENT_GROUNDED_CONTEXTS {
                collected.reverse();
                return collected;
            }
        }
    }
    collected.reverse();
    collected
}

fn trim_follow_up_context(context: &AssistantFollowUpContext) -> AssistantFollowUpContext {
    let mut trimmed = context.clone();
    trimmed.entities = trimmed
        .entities
        .into_iter()
        .take(MAX_RECENT_GROUNDED_ENTITIES)
        .collect();
    trimmed
}

fn compact_grounded_context(context: &AssistantFollowUpContext) -> CompactGroundedContext {
    CompactGroundedContext {
        tool: context.tool.clone(),
        label: truncate_single_line(&context.label, 120),
        entities: context
            .entities
            .iter()
            .take(MAX_RECENT_GROUNDED_ENTITIES)
            .map(|entity| truncate_single_line(&entity.label, 80))
            .collect(),
    }
}

fn append_labeled_items(sections: &mut Vec<String>, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    sections.push(format!("{label}: {}", items.join(" | ")));
}

fn normalize_memory_items(items: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for item in items {
        let trimmed = truncate_single_line(&item, MAX_MEMORY_ITEM_CHARS);
        if trimmed.is_empty() {
            continue;
        }
        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&trimmed))
        {
            continue;
        }
        normalized.push(trimmed);
        if normalized.len() >= MAX_MEMORY_LIST_ITEMS {
            break;
        }
    }
    normalized
}

fn merge_memory_items(existing: &[String], extra: &[String]) -> Vec<String> {
    let mut merged = existing.to_vec();
    merged.extend(extra.iter().cloned());
    normalize_memory_items(merged)
}

fn merge_sentences(existing: &str, fragment: &str) -> String {
    match (existing.trim(), fragment.trim()) {
        ("", next) => next.to_string(),
        (current, "") => current.to_string(),
        (current, next) => format!("{current} {next}"),
    }
}

fn truncate_single_line(value: &str, max_chars: usize) -> String {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut truncated = String::new();
    for (index, ch) in normalized.chars().enumerate() {
        if index >= max_chars {
            truncated.push_str("...");
            break;
        }
        truncated.push(ch);
    }
    truncated
}

fn next_raw_history_start(memory_turn_index: i64, history_len: usize) -> usize {
    if memory_turn_index < 0 {
        0
    } else {
        usize::try_from(memory_turn_index.saturating_add(1))
            .unwrap_or(usize::MAX)
            .min(history_len)
    }
}
const MAX_RETRIEVAL_HITS: i64 = 6;
const MAX_ENTITY_GRAPH_CONTEXTS: usize = 2;

pub(crate) fn stable_id(prefix: &str, parts: &[&str]) -> String {
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

fn visibility_for_tool(tool: &str) -> AssistantGroundingVisibility {
    match tool {
        "system_get_host_runtime_summary"
        | "system_get_backup_summary"
        | "system_get_service_health"
        | "system_get_transcode_summary"
        | "system_get_storage_summary"
        | "system_get_recent_errors" => AssistantGroundingVisibility::Admin,
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details"
        | "calendar_create_event"
        | "calendar_create_birthday"
        | "channels_get_transcript_summary"
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
        "libraries_list_accessible" => Some("libraries:accessible".to_string()),
        "library_search_titles" | "library_get_item_summary" | "libraries_get_recently_added" => {
            block
                .data
                .get("library_id")
                .and_then(Value::as_str)
                .map(|library_id| format!("library:{library_id}"))
                .or_else(|| {
                    block
                        .data
                        .get("query")
                        .and_then(Value::as_str)
                        .map(|query| format!("library_query:{query}"))
                })
        }
        "rooms_list_active" | "rooms_list_joinable" | "rooms_get_room_summary" => {
            Some("rooms:catalog".to_string())
        }
        "servers_list_minecraft_status" | "servers_get_minecraft_server_summary" => {
            Some("servers:catalog".to_string())
        }
        "network_get_topology_summary" => Some("network:topology".to_string()),
        "system_get_host_runtime_summary" => Some("admin:runtime".to_string()),
        "system_get_backup_summary" => Some("admin:backups".to_string()),
        "system_get_service_health" => Some("admin:service_health".to_string()),
        "system_get_transcode_summary" => Some("admin:transcode".to_string()),
        "system_get_storage_summary" => Some("admin:storage".to_string()),
        "system_get_recent_errors" => Some("admin:recent_errors".to_string()),
        "weather_get_current" | "weather_get_forecast" | "weather_get_history" => block
            .data
            .get("resolved_location")
            .and_then(Value::as_str)
            .or_else(|| block.data.get("location").and_then(Value::as_str))
            .map(|location| format!("weather:{location}")),
        "web_search_public_web" | "web_fetch_public_page_summary" => Some("web:public".to_string()),
        _ => None,
    }
}

fn first_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_string))
}

pub(crate) fn chunk_citation(
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

pub(crate) fn chunk_from_parts(
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
    let hash_parts = if source_id.is_some() || source_sub_id.is_some() {
        vec![
            source_kind.to_string(),
            source_id.clone().unwrap_or_default(),
            source_sub_id.clone().unwrap_or_default(),
            topic_key.clone().unwrap_or_default(),
        ]
    } else {
        vec![
            source_kind.to_string(),
            topic_key.clone().unwrap_or_default(),
            title.clone(),
            excerpt.clone(),
        ]
    };
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

struct MemoryItemRecord {
    memory_key: String,
    memory_type: AssistantMemoryKind,
    title: String,
    content: String,
    search_text: String,
    weight: f64,
    metadata: AssistantMemoryMetadata,
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

fn memory_kind_for_chunk(chunk: &AssistantGroundingChunk) -> AssistantMemoryKind {
    match chunk.source_kind.as_str() {
        "account_get_profile_summary" => AssistantMemoryKind::UserPreference,
        "calendar_list_events"
        | "calendar_get_next_event"
        | "calendar_upcoming_birthdays"
        | "calendar_get_event_details"
        | "calendar_create_event"
        | "calendar_create_birthday"
        | "library_get_item_summary"
        | "transcript_excerpt"
        | "channels_get_transcript_summary" => AssistantMemoryKind::DurableFact,
        "system_get_recent_errors" | "recent_error" => AssistantMemoryKind::ToolGotcha,
        "rooms_list_active"
        | "rooms_list_joinable"
        | "rooms_get_room_summary"
        | "servers_list_minecraft_status"
        | "servers_get_minecraft_server_summary"
        | "network_get_topology_summary"
        | "system_get_host_runtime_summary"
        | "system_get_backup_summary"
        | "system_get_service_health"
        | "system_get_transcode_summary"
        | "system_get_storage_summary"
        | "downloads_list_available_artifacts"
        | "download_artifact"
        | "libraries_list_accessible"
        | "library_item"
        | "library_search_titles"
        | "libraries_get_recently_added"
        | "weather_get_current"
        | "weather_get_forecast"
        | "weather_get_history" => AssistantMemoryKind::EnvironmentFact,
        _ => AssistantMemoryKind::Runbook,
    }
}

fn memory_tags_for_chunk(chunk: &AssistantGroundingChunk) -> Vec<String> {
    let mut tags = Vec::new();
    tags.push(chunk.source_kind.to_ascii_lowercase());
    if let Some(topic_key) = chunk.topic_key.as_ref() {
        tags.push(topic_key.to_ascii_lowercase());
        tags.extend(
            topic_key
                .split(':')
                .filter(|segment| !segment.trim().is_empty())
                .map(|segment| segment.to_ascii_lowercase()),
        );
    }
    if let Some(source_id) = chunk.source_id.as_ref() {
        tags.push(source_id.to_ascii_lowercase());
    }
    if let Some(source_sub_id) = chunk.source_sub_id.as_ref() {
        tags.push(source_sub_id.to_ascii_lowercase());
    }
    if let Some(citation) = chunk.citation.as_ref() {
        if let Some(label) = citation.label.as_ref() {
            tags.extend(
                label
                    .split(|ch: char| !ch.is_ascii_alphanumeric())
                    .map(|value| value.trim().to_ascii_lowercase())
                    .filter(|value| value.len() >= 3),
            );
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

fn memory_expiry_ts(kind: AssistantMemoryKind, chunk: &AssistantGroundingChunk) -> Option<i64> {
    let now = Utc::now().timestamp();
    match kind {
        AssistantMemoryKind::DurableFact => Some(now + 60 * 60 * 24 * 180),
        AssistantMemoryKind::UserPreference => Some(now + 60 * 60 * 24 * 90),
        AssistantMemoryKind::EnvironmentFact => match chunk.source_kind.as_str() {
            "weather_get_current" | "weather_get_forecast" | "weather_get_history" => {
                Some(now + 60 * 60 * 12)
            }
            "system_get_host_runtime_summary"
            | "system_get_service_health"
            | "system_get_transcode_summary"
            | "system_get_storage_summary"
            | "system_get_recent_errors"
            | "recent_error" => Some(now + 60 * 60 * 24),
            _ => Some(now + 60 * 60 * 24 * 14),
        },
        AssistantMemoryKind::ToolGotcha | AssistantMemoryKind::OpenLoop => {
            Some(now + 60 * 60 * 24 * 30)
        }
        AssistantMemoryKind::Runbook => Some(now + 60 * 60 * 24 * 60),
    }
}

fn memory_record_for_chunk(chunk: &AssistantGroundingChunk) -> MemoryItemRecord {
    let memory_type = memory_kind_for_chunk(chunk);
    let tags = memory_tags_for_chunk(chunk);
    let metadata = AssistantMemoryMetadata {
        tags: tags.clone(),
        confidence: (0.55 + (chunk.score / 2.0)).clamp(0.55, 0.99),
        expires_ts: memory_expiry_ts(memory_type, chunk),
        source_kind: Some(chunk.source_kind.clone()),
        source_id: chunk.source_id.clone(),
        source_sub_id: chunk.source_sub_id.clone(),
        source_chunk_id: Some(chunk.id.clone()),
    };
    let memory_key = stable_id(
        "memory",
        &[
            memory_type.as_str(),
            &chunk.source_kind,
            chunk.source_id.as_deref().unwrap_or(&chunk.id),
            chunk.source_sub_id.as_deref().unwrap_or(""),
            chunk.topic_key.as_deref().unwrap_or(""),
        ],
    );
    let mut search_parts = vec![
        chunk.title.clone(),
        chunk.excerpt.clone(),
        chunk.source_kind.clone(),
    ];
    if let Some(topic_key) = chunk.topic_key.as_ref() {
        search_parts.push(topic_key.clone());
    }
    search_parts.extend(tags.iter().cloned());

    MemoryItemRecord {
        memory_key,
        memory_type,
        title: chunk.title.clone(),
        content: chunk.excerpt.clone(),
        search_text: search_parts.join("\n"),
        weight: chunk.score,
        metadata,
    }
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

fn build_chunks_from_block(
    context: &AssistantContext,
    call: &PlannedToolCall,
    block: &AssistantToolContextBlock,
    source: &super::types::AssistantGroundingSource,
) -> Vec<AssistantGroundingChunk> {
    if call.tool == super::registry::AssistantToolName::ChannelsGetTranscriptSummary {
        return transcript_chunks_for_block(context, call, block);
    }
    if call.tool == super::registry::AssistantToolName::SystemGetRecentErrors {
        return recent_error_chunks_for_block(context, call, block);
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
    if lower.contains("transcript") || lower.contains("call") {
        return Some("transcript:conversation".to_string());
    }
    if lower.contains("download") || lower.contains("extension") {
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
                        .or_else(|| {
                            let topic = maybe_topic_from_message(&context.label);
                            topic
                        })
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
                let metadata: AssistantMemoryMetadata =
                    serde_json::from_str(&row.metadata_json).unwrap_or_default();
                let Some(memory_kind) = AssistantMemoryKind::from_str(&row.memory_type) else {
                    continue;
                };
                let Some(selection) = score_memory_candidate(
                    &AssistantMemorySelectorCandidate {
                        title: &row.title,
                        content: &row.content,
                        topic_key: row.topic_key.as_deref(),
                        weight: row.weight,
                        lexical_rank: hit.rank,
                        memory_kind,
                        updated_ts: row.updated_ts,
                        metadata: metadata.clone(),
                    },
                    topic_key,
                    query,
                ) else {
                    continue;
                };
                chunks.push(AssistantGroundingChunk {
                    id: format!("memory:{}", row.memory_key),
                    source_kind: format!("memory_{}", row.memory_type),
                    title: row.title.clone(),
                    excerpt: row.content.clone(),
                    score: selection.score,
                    visibility: if row
                        .topic_key
                        .as_deref()
                        .is_some_and(|value| value.starts_with("admin:"))
                    {
                        AssistantGroundingVisibility::Admin
                    } else {
                        AssistantGroundingVisibility::User
                    },
                    topic_key: row.topic_key.clone(),
                    owner_user_id: Some(row.user_id.clone()),
                    source_id: metadata
                        .source_id
                        .clone()
                        .or_else(|| Some(row.memory_key.clone())),
                    source_sub_id: metadata.source_sub_id.clone(),
                    citation: Some(chunk_citation(
                        &format!("memory_{}", row.memory_type),
                        metadata.source_id.as_deref().unwrap_or(&row.memory_key),
                        metadata.source_sub_id.as_deref(),
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

    if topic_key.is_some() || query.is_some() {
        chunks.extend(search_operational_chunks(state, context, topic_key.as_deref(), query).await);
    }

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
            let memory_record = memory_record_for_chunk(chunk);
            let memory_metadata_json =
                serde_json::to_string(&memory_record.metadata).unwrap_or_else(|_| "{}".to_string());
            let memory_params = rustfin_db::repo::ai_grounding::UpsertAiMemoryItemParams {
                user_id: &context.user_id,
                memory_key: &memory_record.memory_key,
                memory_type: memory_record.memory_type.as_str(),
                topic_key: chunk.topic_key.as_deref(),
                title: &memory_record.title,
                content: &memory_record.content,
                search_text: &memory_record.search_text,
                weight: memory_record.weight,
                metadata_json: &memory_metadata_json,
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
        let root_access_scope = match context_block.tool.as_str() {
            "system_get_host_runtime_summary"
            | "system_get_backup_summary"
            | "system_get_service_health"
            | "system_get_transcode_summary"
            | "system_get_storage_summary"
            | "system_get_recent_errors" => "admin",
            _ => "user",
        };
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
            access_scope: root_access_scope,
            access_key: None,
            ordinal: 0,
            metadata_json: &root_metadata,
        };

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

    fn fake_counter(messages: &[rustfin_ai_agent::ChatMessage]) -> u32 {
        messages
            .iter()
            .map(|message| (message.content.len() / 16) as u32 + 1)
            .sum()
    }

    #[test]
    fn prompt_builder_requests_summary_when_budget_is_exceeded() {
        let history = vec![
            AssistantHistoryMessage {
                role: "user".to_string(),
                content:
                    "Please remember this very long first turn about the house network and backups."
                        .repeat(8),
                grounding_tools: vec![],
                follow_up_contexts: vec![],
            },
            AssistantHistoryMessage {
                role: "assistant".to_string(),
                content: "I checked the host runtime and backup state.".repeat(8),
                grounding_tools: vec!["system_get_host_runtime_summary".to_string()],
                follow_up_contexts: vec![AssistantFollowUpContext {
                    tool: "system_get_host_runtime_summary".to_string(),
                    label: "Rustyfin host runtime summary".to_string(),
                    input_hint: Default::default(),
                    entities: vec![AssistantFollowUpEntity {
                        ordinal: 1,
                        label: "runtime".to_string(),
                        identifier: None,
                    }],
                }],
            },
        ];

        let assembly = build_generation_prompt_messages(
            "system prompt",
            "Current host time",
            &[],
            &history,
            "Follow up question",
            &ConversationMemoryState::default(),
            -1,
            128,
            24,
            104,
            fake_counter,
        );

        assert_eq!(assembly.pending_summary_turns.len(), 2);
        assert_eq!(assembly.debug.summarized_turns, 2);
        assert_eq!(assembly.debug.retained_raw_turns, 0);
    }

    #[test]
    fn prompt_builder_retains_recent_raw_turns_when_memory_covers_older_history() {
        let history = vec![
            AssistantHistoryMessage {
                role: "user".to_string(),
                content: "Older summarized turn".to_string(),
                grounding_tools: vec![],
                follow_up_contexts: vec![],
            },
            AssistantHistoryMessage {
                role: "assistant".to_string(),
                content: "Recent unsummarized answer".to_string(),
                grounding_tools: vec![],
                follow_up_contexts: vec![],
            },
        ];

        let assembly = build_generation_prompt_messages(
            "system prompt",
            "Current host time",
            &[],
            &history,
            "Next question",
            &ConversationMemoryState {
                summary: "Older summarized turn".to_string(),
                ..Default::default()
            },
            0,
            512,
            384,
            128,
            fake_counter,
        );

        assert!(assembly.pending_summary_turns.is_empty());
        assert_eq!(assembly.debug.retained_raw_turns, 1);
        assert!(assembly.debug.used_memory_summary);
    }

    #[test]
    fn parse_memory_update_response_extracts_wrapped_json() {
        let parsed = parse_memory_update_response(
            "Here is the memory:\n{\"summary\":\"User is planning a trip.\",\"durable_facts\":[\"Trip is in July\"],\"user_preferences\":[],\"open_loops\":[\"Pick a hotel\"],\"active_topics\":[\"travel\"]}",
        )
        .expect("expected parsed memory");

        assert_eq!(parsed.summary, "User is planning a trip.");
        assert_eq!(parsed.open_loops, vec!["Pick a hotel".to_string()]);
    }

    #[test]
    fn fallback_memory_update_preserves_recent_user_questions() {
        let next = fallback_memory_update(
            &ConversationMemoryState::default(),
            &[AssistantHistoryMessage {
                role: "user".to_string(),
                content: "What should I pack for the trip?".to_string(),
                grounding_tools: vec![],
                follow_up_contexts: vec![],
            }],
        );

        assert!(!next.summary.is_empty());
        assert_eq!(
            next.open_loops,
            vec!["What should I pack for the trip?".to_string()]
        );
    }

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
    }
}
