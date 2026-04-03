use chrono::{Datelike, NaiveDate};
use std::cmp::Ordering;
use std::collections::HashSet;
use serde::Deserialize;

use super::types::{AssistantGroundingChunk, AssistantGroundingCitation, AssistantToolContextBlock};

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
struct GroundedCalendarEventsEnvelope {
    window: GroundedCalendarWindow,
    events: Vec<GroundedCalendarEventSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedCalendarEventSummary {
    title: String,
    event_date: String,
    #[serde(default)]
    next_occurs_on: Option<String>,
    scope: String,
    event_type: String,
    owner_username: Option<String>,
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
struct GroundedRoomsEnvelope {
    total_count: usize,
    room_mode_filter: Option<String>,
    room_mode: Option<String>,
    query: Option<String>,
    rooms: Vec<GroundedRoomSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedHostRuntimeEnvelope {
    ai: GroundedHostRuntimeAiSummary,
}

#[derive(Debug, Deserialize)]
struct GroundedHostRuntimeAiSummary {
    phase: String,
    active_request_count: u64,
    queue_depth: u64,
    tool_calls_in_flight: u64,
    loaded_model: Option<String>,
    model_loaded: bool,
    context_length: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GroundedAccountProfileSummary {
    username: String,
    display_name: String,
    role: String,
    time_zone: Option<String>,
    accessible_library_count: usize,
}

#[derive(Debug, Deserialize)]
struct GroundedDownloadsEnvelope {
    total_count: usize,
    query: Option<String>,
    availability_filter: Option<String>,
    artifacts: Vec<GroundedDownloadArtifactSummary>,
}

#[derive(Debug, Deserialize)]
struct GroundedDownloadArtifactSummary {
    title: String,
    availability: String,
    version: Option<String>,
    install_mode: Option<String>,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct GroundedBackupSummary {
    configured: bool,
    restore_supported: bool,
    last_successful_backup_ts: Option<i64>,
    policy_count: i64,
    total_job_count: i64,
    successful_job_count: i64,
    failed_job_count: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GroundedServiceHealthSummary {
    all_healthy: bool,
    components: Vec<GroundedServiceHealthComponent>,
}

#[derive(Debug, Deserialize)]
struct GroundedServiceHealthComponent {
    name: String,
    status: String,
    configured: bool,
    detail: String,
}

#[derive(Debug, Deserialize)]
struct GroundedTranscodeSummary {
    active_sessions: usize,
    created_total: u64,
    create_failures_total: u64,
    create_failures_last_minute: u64,
    create_failures_last_five_minutes: u64,
    cleaned_total: u64,
    hw_accel: Option<String>,
    hw_accel_required: bool,
}

#[derive(Debug, Deserialize)]
struct GroundedStorageSummary {
    available: bool,
    reason: Option<String>,
    mounts: Vec<GroundedStorageMount>,
}

#[derive(Debug, Deserialize)]
struct GroundedStorageMount {
    mount_point: String,
    tracked_paths: Vec<String>,
    total_human: Option<String>,
    available_human: Option<String>,
    used_percent: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct GroundedRecentErrorsSummary {
    recent_failed_jobs: Vec<GroundedRecentErrorItem>,
}

#[derive(Debug, Deserialize)]
struct GroundedRecentErrorItem {
    kind: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GroundedRoomSummary {
    title: String,
    room_mode: String,
    host_username: String,
    password_required: bool,
    joinable_via: Option<String>,
    member_count: Option<i64>,
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
        "calendar_list_events" => Some(if block.status == "ok" {
            format_calendar_events_reply(
                serde_json::from_value::<GroundedCalendarEventsEnvelope>(block.data.clone())
                    .ok()?,
            )
        } else {
            format_calendar_error(block, "I couldn't load the visible calendar events.")
        }),
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

pub fn deterministic_rooms_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| matches!(block.tool, "rooms_list_joinable" | "rooms_list_active"))?;

    Some(if block.status == "ok" {
        format_rooms_reply(
            message,
            block.tool,
            serde_json::from_value::<GroundedRoomsEnvelope>(block.data.clone()).ok()?,
        )
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the room details. {message}"))
            .unwrap_or_else(|| "I couldn't load the room details.".to_string())
    })
}

pub fn deterministic_runtime_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "system_get_host_runtime_summary")?;

    Some(if block.status == "ok" {
        format_host_runtime_reply(
            message,
            serde_json::from_value::<GroundedHostRuntimeEnvelope>(block.data.clone()).ok()?,
        )
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the Rustyfin AI runtime status. {message}"))
            .unwrap_or_else(|| "I couldn't load the Rustyfin AI runtime status.".to_string())
    })
}

pub fn deterministic_profile_reply(
    _message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "account_get_profile_summary")?;

    Some(if block.status == "ok" {
        let profile =
            serde_json::from_value::<GroundedAccountProfileSummary>(block.data.clone()).ok()?;
        let time_zone = profile
            .time_zone
            .as_deref()
            .map(|value| format!(" Time zone: {value}."))
            .unwrap_or_default();
        format!(
            "You are signed into Rustyfin as {} (@{}). Role: {}. Accessible libraries: {}.{}",
            profile.display_name,
            profile.username,
            profile.role,
            profile.accessible_library_count,
            time_zone
        )
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load your Rustyfin account details. {message}"))
            .unwrap_or_else(|| "I couldn't load your Rustyfin account details.".to_string())
    })
}

pub fn deterministic_downloads_reply(
    _message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks
        .iter()
        .find(|block| block.tool == "downloads_list_available_artifacts")?;

    let reply = if block.status == "ok" {
        let envelope =
            serde_json::from_value::<GroundedDownloadsEnvelope>(block.data.clone()).ok()?;
        if envelope.artifacts.is_empty() {
            match envelope.query.as_deref() {
                Some(query) => {
                    format!("I couldn't find any downloads matching \"{query}\" right now.")
                }
                None => "There are no Rustyfin downloads available right now.".to_string(),
            }
        } else {
            let heading = match (
                envelope.query.as_deref(),
                envelope.availability_filter.as_deref(),
            ) {
                (Some(query), Some(availability)) => {
                    format!("Rustyfin downloads matching \"{query}\" with status {availability}:")
                }
                (Some(query), None) => format!("Rustyfin downloads matching \"{query}\":"),
                (None, Some(availability)) => {
                    format!("Rustyfin downloads with status {availability}:")
                }
                (None, None) => "Rustyfin downloads available right now:".to_string(),
            };

            let mut lines = vec![heading];
            for artifact in envelope.artifacts.iter().take(8) {
                let version = artifact
                    .version
                    .as_deref()
                    .map(|value| format!(" {value}"))
                    .unwrap_or_default();
                let install_mode = artifact
                    .install_mode
                    .as_deref()
                    .map(|value| format!(" via {value}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "- {} ({}){}{}: {}",
                    artifact.title, artifact.availability, version, install_mode, artifact.summary
                ));
            }
            lines.join("\n")
        }
    } else {
        block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the Rustyfin downloads catalog. {message}"))
            .unwrap_or_else(|| "I couldn't load the Rustyfin downloads catalog.".to_string())
    };

    Some(reply)
}

pub fn deterministic_service_reply(
    _message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    let block = grounding_blocks.iter().find(|block| {
        matches!(
            block.tool,
            "system_get_backup_summary"
                | "system_get_service_health"
                | "system_get_transcode_summary"
                | "system_get_storage_summary"
                | "system_get_recent_errors"
        )
    })?;

    let reply = match block.tool {
        "system_get_backup_summary" if block.status == "ok" => {
            let summary =
                serde_json::from_value::<GroundedBackupSummary>(block.data.clone()).ok()?;
            let last_success = summary
                .last_successful_backup_ts
                .map(|value| format!(" Last successful backup timestamp: {value}."))
                .unwrap_or_default();
            format!(
                "{}{} Restore supported: {}.",
                summary.message, last_success, summary.restore_supported
            )
        }
        "system_get_service_health" if block.status == "ok" => {
            let summary =
                serde_json::from_value::<GroundedServiceHealthSummary>(block.data.clone()).ok()?;
            if summary.all_healthy {
                format!(
                    "Rustyfin services are healthy. Checked {} component{}.",
                    summary.components.len(),
                    plural_suffix(summary.components.len())
                )
            } else {
                let degraded = summary
                    .components
                    .iter()
                    .filter(|component| component.configured && component.status != "healthy")
                    .map(|component| format!("{} ({})", component.name, component.detail))
                    .collect::<Vec<_>>();
                format!("Rustyfin has degraded services: {}.", degraded.join("; "))
            }
        }
        "system_get_transcode_summary" if block.status == "ok" => {
            let summary =
                serde_json::from_value::<GroundedTranscodeSummary>(block.data.clone()).ok()?;
            let accel = summary
                .hw_accel
                .as_deref()
                .map(|value| format!(" Hardware acceleration: {value}."))
                .unwrap_or_default();
            format!(
                "Rustyfin transcode runtime has {} active session{}. Total created: {}. Failures: {} total, {} in the last minute, {} in the last five minutes. Cleaned total: {}.{} Required: {}.",
                summary.active_sessions,
                plural_suffix(summary.active_sessions),
                summary.created_total,
                summary.create_failures_total,
                summary.create_failures_last_minute,
                summary.create_failures_last_five_minutes,
                summary.cleaned_total,
                accel,
                summary.hw_accel_required
            )
        }
        "system_get_storage_summary" if block.status == "ok" => {
            let summary =
                serde_json::from_value::<GroundedStorageSummary>(block.data.clone()).ok()?;
            if !summary.available {
                summary
                    .reason
                    .unwrap_or_else(|| "I couldn't load the Rustyfin storage summary.".to_string())
            } else if summary.mounts.is_empty() {
                "Rustyfin does not have any tracked storage mounts right now.".to_string()
            } else {
                let mut lines = vec!["Rustyfin storage summary:".to_string()];
                for mount in summary.mounts.iter().take(6) {
                    let capacity = match (&mount.available_human, &mount.total_human) {
                        (Some(available), Some(total)) => format!("{available} free of {total}"),
                        _ => "capacity unavailable".to_string(),
                    };
                    let used = mount
                        .used_percent
                        .map(|value| format!(" ({value:.0}% used)"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "- {}: {}{} [{}]",
                        mount.mount_point,
                        capacity,
                        used,
                        mount.tracked_paths.join(", ")
                    ));
                }
                lines.join("\n")
            }
        }
        "system_get_recent_errors" if block.status == "ok" => {
            let summary =
                serde_json::from_value::<GroundedRecentErrorsSummary>(block.data.clone()).ok()?;
            if summary.recent_failed_jobs.is_empty() {
                "Rustyfin has no recent failed jobs or recorded assistant/runtime errors in the current summary.".to_string()
            } else {
                let mut lines = vec!["Recent Rustyfin failures:".to_string()];
                for item in summary.recent_failed_jobs.iter().take(6) {
                    lines.push(format!("- {}: {}", item.kind, item.message));
                }
                lines.join("\n")
            }
        }
        _ => block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(|message| format!("I couldn't load the Rustyfin service summary. {message}"))
            .unwrap_or_else(|| "I couldn't load the Rustyfin service summary.".to_string()),
    };

    Some(reply)
}

fn format_next_event_reply(envelope: GroundedNextEventEnvelope) -> String {
    let Some(next_event) = envelope.next_event else {
        return "You do not have any visible upcoming calendar events.".to_string();
    };

    let event_date = parse_ymd(&next_event.next_occurs_on);
    let scope = scope_label(&next_event.scope);
    let timing = event_date
        .map(|date| describe_relative_timing(date))
        .unwrap_or_default();

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

fn format_calendar_events_reply(envelope: GroundedCalendarEventsEnvelope) -> String {
    if envelope.events.is_empty() {
        return format!(
            "You do not have any visible calendar events for {}.",
            envelope.window.label
        );
    }

    let mut lines = vec![format!(
        "You have {} visible calendar event{} for {}:",
        envelope.events.len(),
        plural_suffix(envelope.events.len()),
        envelope.window.label
    )];

    for event in envelope.events.iter().take(8) {
        let occurs_on = event.next_occurs_on.as_deref().unwrap_or(&event.event_date);
        let human_date = parse_ymd(occurs_on)
            .map(format_with_weekday)
            .unwrap_or_else(|| occurs_on.to_string());
        let title = if event.event_type == "birthday" {
            birthday_display_name(&event.title)
        } else {
            event.title.clone()
        };
        let kind = if event.event_type == "birthday" {
            "birthday"
        } else {
            "event"
        };
        let owner = event
            .owner_username
            .as_deref()
            .filter(|owner| event.scope == "global" && !owner.is_empty())
            .map(|owner| format!(" for {owner}"))
            .unwrap_or_default();
        lines.push(format!(
            "- {title} ({kind}) on {human_date} in {}{owner}",
            scope_label(&event.scope)
        ));
    }

    lines.join("\n")
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

fn format_host_runtime_reply(message: &str, envelope: GroundedHostRuntimeEnvelope) -> String {
    let lower = message.to_ascii_lowercase();
    let model_summary = if envelope.ai.model_loaded {
        match envelope.ai.loaded_model.as_deref() {
            Some(model) => format!("Loaded model: {model}."),
            None => "A model is loaded.".to_string(),
        }
    } else {
        "No model is currently loaded.".to_string()
    };
    let context_summary = envelope
        .ai
        .context_length
        .map(|value| format!(" Context window: {value} tokens."))
        .unwrap_or_default();
    let request_summary = format!(
        "Active AI requests: {}. Queue depth: {}. Grounded AI tool calls in flight: {}.",
        envelope.ai.active_request_count, envelope.ai.queue_depth, envelope.ai.tool_calls_in_flight
    );
    let meaning_summary =
        " This request count reflects active assistant turns, not separate AI instances.";

    if lower.contains("ai runtime")
        || lower.contains("assistant runtime")
        || lower.contains("runtime status")
        || lower.contains("how many ai")
        || lower.contains("how many requests")
    {
        return format!(
            "Rustyfin AI runtime is currently {}. {} {}{}{}",
            humanize_runtime_phase(&envelope.ai.phase),
            model_summary,
            request_summary,
            context_summary,
            meaning_summary
        );
    }

    format!(
        "Rustyfin runtime summary: AI phase is {}. {} {}{}",
        humanize_runtime_phase(&envelope.ai.phase),
        model_summary,
        request_summary,
        context_summary
    )
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

fn format_rooms_reply(message: &str, tool: &str, envelope: GroundedRoomsEnvelope) -> String {
    let lower = message.to_ascii_lowercase();
    let active_only = tool == "rooms_list_active";
    let room_mode_filter = envelope
        .room_mode_filter
        .clone()
        .or(envelope.room_mode.clone());
    let query = envelope.query.clone();

    if envelope.rooms.is_empty() {
        return if active_only {
            match (room_mode_filter.as_deref(), query.as_deref()) {
                (Some(mode), Some(query)) => {
                    format!("There are no active {mode} rooms matching \"{query}\" right now.")
                }
                (Some(mode), None) => format!("There are no active {mode} rooms right now."),
                (None, Some(query)) => {
                    format!("There are no active rooms matching \"{query}\" right now.")
                }
                (None, None) => "There are no active rooms right now.".to_string(),
            }
        } else if lower.contains("invite") {
            match query.as_deref() {
                Some(query) => {
                    format!(
                        "You do not have any usable room invites matching \"{query}\" right now."
                    )
                }
                None => "You do not have any usable room invites right now.".to_string(),
            }
        } else {
            match (room_mode_filter.as_deref(), query.as_deref()) {
                (Some(mode), Some(query)) => {
                    format!("There are no joinable {mode} rooms matching \"{query}\" right now.")
                }
                (Some(mode), None) => format!("There are no joinable {mode} rooms right now."),
                (None, Some(query)) => {
                    format!("There are no joinable rooms matching \"{query}\" right now.")
                }
                (None, None) => "There are no rooms you can join right now.".to_string(),
            }
        };
    }

    let mut lines = if active_only {
        vec![format!(
            "{} active room{} right now:",
            envelope.total_count,
            plural_suffix(envelope.total_count)
        )]
    } else if lower.contains("invite") {
        vec![format!(
            "{} invite{} you can use right now:",
            envelope.total_count,
            plural_suffix(envelope.total_count)
        )]
    } else {
        vec![format!(
            "{} room{} you can join right now:",
            envelope.total_count,
            plural_suffix(envelope.total_count)
        )]
    };

    for room in envelope.rooms.iter().take(8) {
        let joinable_via = match room.joinable_via.as_deref() {
            Some("invite") => "invite",
            Some("public_lobby") => "public lobby",
            Some(other) => other,
            None => "room",
        };
        let room_mode = if room.room_mode.eq_ignore_ascii_case("invite") {
            "invite".to_string()
        } else {
            room.room_mode.replace('_', " ")
        };
        let mut details = vec![format!("hosted by {}", room.host_username)];
        if let Some(count) = room.member_count {
            details.push(format!("{} member{}", count, plural_suffix(count as usize)));
        }
        if room.password_required {
            details.push("password required".to_string());
        }
        if !active_only {
            details.push(format!("join via {joinable_via}"));
        }
        lines.push(format!(
            "- {} ({room_mode}; {})",
            room.title,
            details.join(", ")
        ));
    }

    lines.join("\n")
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

fn humanize_runtime_phase(phase: &str) -> &str {
    match phase {
        "idle" => "idle",
        "loading_model" => "loading a model",
        "planning" => "planning a turn",
        "grounding" => "running grounded tools",
        "generating" => "generating a reply",
        _ => "running",
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

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
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

#[cfg(test)]
mod tests {
    use super::{
        deterministic_calendar_reply, deterministic_downloads_reply, deterministic_network_reply,
        deterministic_profile_reply, deterministic_rooms_reply, deterministic_runtime_reply,
        deterministic_service_reply,
    };
    use crate::ai_assistant::types::AssistantToolContextBlock;
    use serde_json::json;

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
    fn deterministic_calendar_reply_formats_visible_events_without_raw_birth_year() {
        let reply = deterministic_calendar_reply(
            "Show my visible calendar events for the next seven days.",
            &[AssistantToolContextBlock {
                tool: "calendar_list_events",
                label: "Visible calendar events for the next 7 days".to_string(),
                status: "ok",
                data: json!({
                    "window": { "label": "the next 7 days" },
                    "events": [
                        {
                            "title": "Iwans birthday",
                            "event_date": "2003-06-09",
                            "next_occurs_on": "2026-04-05",
                            "scope": "personal",
                            "event_type": "birthday",
                            "owner_username": null
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic calendar events reply");

        assert!(reply.contains("the next 7 days"));
        assert!(reply.contains("Sunday, April 5, 2026"));
        assert!(!reply.contains("2003-06-09"));
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
    fn deterministic_rooms_reply_formats_joinable_invites() {
        let reply = deterministic_rooms_reply(
            "Which invites can I use right now?",
            &[AssistantToolContextBlock {
                tool: "rooms_list_joinable",
                label: "Joinable rooms".to_string(),
                status: "ok",
                data: json!({
                    "room_mode": null,
                    "query": null,
                    "total_count": 2,
                    "rooms": [
                        {
                            "title": "Living Room",
                            "room_mode": "watch",
                            "host_username": "User1",
                            "password_required": false,
                            "joinable_via": "public_lobby",
                            "member_count": 0
                        },
                        {
                            "title": "Kitchen",
                            "room_mode": "invite",
                            "host_username": "User1",
                            "password_required": true,
                            "joinable_via": "invite",
                            "member_count": null
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic room reply");

        assert!(reply.contains("2 invites"));
        assert!(reply.contains("Living Room"));
        assert!(reply.contains("public lobby"));
        assert!(reply.contains("Kitchen"));
        assert!(reply.contains("password required"));
    }

    #[test]
    fn deterministic_runtime_reply_explains_ai_request_counts() {
        let reply = deterministic_runtime_reply(
            "What is the AI runtime status right now?",
            &[AssistantToolContextBlock {
                tool: "system_get_host_runtime_summary",
                label: "Rustyfin host runtime summary".to_string(),
                status: "ok",
                data: json!({
                    "ai": {
                        "phase": "planning",
                        "active_request_count": 1,
                        "queue_depth": 0,
                        "tool_calls_in_flight": 1,
                        "loaded_model": "tiny.gguf",
                        "model_loaded": true,
                        "context_length": 8192
                    }
                }),
            }],
        )
        .expect("expected deterministic runtime reply");

        assert!(reply.contains("planning a turn"));
        assert!(reply.contains("Active AI requests: 1"));
        assert!(reply.contains("not separate AI instances"));
        assert!(reply.contains("tiny.gguf"));
    }

    #[test]
    fn deterministic_downloads_reply_lists_available_artifacts() {
        let reply = deterministic_downloads_reply(
            "Which downloads are available right now?",
            &[AssistantToolContextBlock {
                tool: "downloads_list_available_artifacts",
                label: "Available downloads".to_string(),
                status: "ok",
                data: json!({
                    "total_count": 1,
                    "query": null,
                    "availability_filter": null,
                    "artifacts": [
                        {
                            "title": "Rustyfin Desktop",
                            "availability": "ready",
                            "version": "1.2.3",
                            "install_mode": "manual",
                            "summary": "macOS build"
                        }
                    ]
                }),
            }],
        )
        .expect("expected deterministic downloads reply");

        assert!(reply.contains("Rustyfin downloads available right now"));
        assert!(reply.contains("Rustyfin Desktop"));
        assert!(reply.contains("1.2.3"));
    }

    #[test]
    fn deterministic_profile_reply_formats_account_summary() {
        let reply = deterministic_profile_reply(
            "Who am I signed in as?",
            &[AssistantToolContextBlock {
                tool: "account_get_profile_summary",
                label: "Signed-in Rustyfin account summary".to_string(),
                status: "ok",
                data: json!({
                    "username": "iwan",
                    "display_name": "Iwan",
                    "role": "admin",
                    "time_zone": "Europe/Dublin",
                    "accessible_library_count": 5
                }),
            }],
        )
        .expect("expected deterministic profile reply");

        assert!(reply.contains("Iwan"));
        assert!(reply.contains("@iwan"));
        assert!(reply.contains("Accessible libraries: 5"));
        assert!(reply.contains("Europe/Dublin"));
    }

    #[test]
    fn deterministic_service_reply_formats_transcode_summary() {
        let reply = deterministic_service_reply(
            "What is the transcode runtime status?",
            &[AssistantToolContextBlock {
                tool: "system_get_transcode_summary",
                label: "Rustyfin transcode summary".to_string(),
                status: "ok",
                data: json!({
                    "active_sessions": 1,
                    "created_total": 12,
                    "create_failures_total": 2,
                    "create_failures_last_minute": 1,
                    "create_failures_last_five_minutes": 2,
                    "cleaned_total": 9,
                    "hw_accel": "nvenc",
                    "hw_accel_required": false
                }),
            }],
        )
        .expect("expected deterministic service reply");

        assert!(reply.contains("Total created: 12"));
        assert!(reply.contains("Failures: 2 total"));
        assert!(reply.contains("Hardware acceleration: nvenc"));
    }

    use super::*;
    use crate::ai_assistant::types::{AssistantGroundingCitation, AssistantGroundingVisibility};

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
}
