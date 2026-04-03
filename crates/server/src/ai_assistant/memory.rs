use rustfin_ai_agent::ChatMessage;
use serde::{Deserialize, Serialize};

use super::types::{AssistantFollowUpContext, AssistantHistoryMessage, AssistantToolContextBlock};

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

#[derive(Debug, Clone, Default, Serialize)]
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
    grounding_blocks: &[AssistantToolContextBlock],
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
        grounding_blocks,
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
    grounding_blocks: &[AssistantToolContextBlock],
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
    if !grounding_blocks.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: format!(
                "Authoritative Rustyfin grounding for this turn:\n{}",
                serde_json::to_string(grounding_blocks).unwrap_or_else(|_| "[]".to_string())
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

#[cfg(test)]
mod tests {
    use super::{
        ConversationMemoryState, build_generation_prompt_messages, fallback_memory_update,
        parse_memory_update_response,
    };
    use crate::ai_assistant::types::{
        AssistantFollowUpContext, AssistantFollowUpEntity, AssistantHistoryMessage,
    };

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
}
