use chrono::{Datelike, NaiveDate};
use serde::Deserialize;

use super::types::AssistantToolContextBlock;

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
struct GroundedRoomsEnvelope {
    total_count: usize,
    room_mode_filter: Option<String>,
    room_mode: Option<String>,
    query: Option<String>,
    rooms: Vec<GroundedRoomSummary>,
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

#[cfg(test)]
mod tests {
    use super::{
        deterministic_calendar_reply, deterministic_network_reply, deterministic_rooms_reply,
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
}
