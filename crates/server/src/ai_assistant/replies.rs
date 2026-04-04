use std::cmp::Ordering;
use std::collections::HashSet;

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

use super::types::{
    AssistantGroundingChunk, AssistantGroundingCitation, AssistantToolContextBlock,
};

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

#[derive(Debug, Deserialize)]
struct GroundedNetworkEnvelope {
    host_label: Option<String>,
    remote_access_enabled: bool,
    access: GroundedNetworkAccess,
    nodes: Vec<GroundedNetworkNode>,
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
struct GroundedAiRuntimeEnvelope {
    model: GroundedAiRuntimeModel,
    scheduler: GroundedAiRuntimeScheduler,
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
    _message: &str,
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
        "calendar_upcoming_birthdays" => Some(if block.status == "ok" {
            format_birthdays_reply(
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
        .find(|block| block.tool == "network_get_topology_summary")?;

    Some(if block.status == "ok" {
        format_network_reply(
            message,
            serde_json::from_value::<GroundedNetworkEnvelope>(block.data.clone()).ok()?,
        )
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the Rustyfin network details. {message}"))
            .unwrap_or_else(|| "I couldn't load the Rustyfin network details.".to_string())
    })
}

pub fn deterministic_library_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "library_search_titles")?;

    if block.status != "ok" {
        return Some(format_calendar_error(
            block,
            "I couldn't search your libraries.",
        ));
    }

    let envelope =
        serde_json::from_value::<GroundedLibrarySearchEnvelope>(block.data.clone()).ok()?;
    Some(format_library_search_reply(
        extract_library_search_query(message)
            .or_else(|| extract_quoted_phrase(&block.label))
            .as_deref(),
        envelope,
    ))
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

fn format_birthdays_reply(envelope: GroundedBirthdayEnvelope) -> String {
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

    if envelope.birthdays.len() == 1 {
        let birthday = &envelope.birthdays[0];
        let name = birthday_display_name(&birthday.title);
        let date = parse_ymd(&birthday.next_occurs_on)
            .map(format_with_weekday)
            .unwrap_or_else(|| birthday.next_occurs_on.clone());
        let age = birthday_turning_age(birthday);
        return match age {
            Some(age) => format!("{name}'s next birthday is on {date}. They turn {age}."),
            None => format!("{name}'s next birthday is on {date}."),
        };
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
        deterministic_ai_runtime_reply, deterministic_calendar_reply, deterministic_library_reply,
        deterministic_network_reply, grounding_chunks_prompt, rank_and_compress_grounding_chunks,
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
}
