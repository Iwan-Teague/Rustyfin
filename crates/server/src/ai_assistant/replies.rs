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
    public_host: Option<String>,
    remote_access_enabled: bool,
    access: GroundedNetworkAccess,
    nodes: Vec<GroundedNetworkNode>,
}

#[derive(Debug, Deserialize)]
struct GroundedNetworkAccess {
    ui_port: u16,
    backend_port: u16,
    calendar_port: u16,
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
    if grounding_blocks.len() != 1 {
        return None;
    }
    let block = grounding_blocks.first()?;
    if block.tool != "network_get_topology_summary" {
        return None;
    }

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
        || (lower.contains("connect") && lower.contains("rustyfin"))
        || lower.contains("what ip would i use")
        || lower.contains("which ip would i use");

    let preferred_local_url = envelope
        .access
        .preferred_local_url
        .clone()
        .or_else(|| envelope.access.public_url.clone());
    let preferred_local_ip = envelope
        .access
        .preferred_local_ipv4
        .clone()
        .or_else(|| envelope.public_host.clone());

    if local_connect_question {
        let mut parts = Vec::new();
        if let Some(url) = preferred_local_url {
            parts.push(format!("On your local network, open Rustyfin at {url}."));
        } else {
            parts.push(format!(
                "Rustyfin is listening on HTTPS port {} on this host.",
                envelope.access.ui_port
            ));
        }
        if let Some(ip) = preferred_local_ip {
            parts.push(format!("The preferred local IP is {ip}."));
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

    let interface_summary = envelope
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

    let host_label = envelope
        .host_label
        .as_deref()
        .unwrap_or("this Rustyfin host");
    let mut parts = vec![format!("Rustyfin network summary for {host_label}.")];
    if let Some(url) = envelope.access.preferred_local_url {
        parts.push(format!("Preferred local URL: {url}."));
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

#[cfg(test)]
mod tests {
    use super::{deterministic_calendar_reply, deterministic_network_reply};
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
        assert!(reply.contains("port is 3008"));
        assert!(reply.contains("8097"));
    }
}
