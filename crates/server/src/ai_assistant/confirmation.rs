use std::sync::OnceLock;

use chrono::{Datelike, NaiveDate, Utc};
use regex::Regex;

use super::registry::AssistantToolName;
use super::types::{
    AssistantConfirmationPayload, AssistantPendingActionKind, AssistantToolInput, PlannedToolCall,
};
use crate::auth::AuthUser;

pub const CONFIRMATION_TOKEN_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct ParsedPendingActionRequest {
    pub payload: AssistantConfirmationPayload,
}

pub fn pending_action_request_for_message(
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Option<Result<ParsedPendingActionRequest, String>> {
    let lower = message.to_ascii_lowercase();
    if !is_supported_calendar_create_intent(&lower) {
        return None;
    }

    Some(if lower.contains("birthday") {
        parse_birthday_request(user, message, conversation_id)
    } else {
        parse_event_request(user, message, conversation_id)
    })
}

pub fn is_supported_calendar_create_intent(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &["add ", "create ", "make ", "save ", "schedule "],
    ) && has_any(
        message_lower,
        &["calendar", "event", "events", "birthday", "birthdays"],
    ) && !has_any(
        message_lower,
        &[
            "how do i ",
            "how can i ",
            "can i ",
            "is it possible to ",
            "do you support ",
            "does rustyfin ai support ",
        ],
    )
}

fn parse_birthday_request(
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Result<ParsedPendingActionRequest, String> {
    let scope = resolve_scope(user, message)?;
    let (person, date_candidate) = extract_birthday_subject_and_date(message).ok_or_else(|| {
        "I can add a birthday, but I need the person and the date. Try \"Add Rachel's birthday on 2003-06-09\".".to_string()
    })?;
    let name = normalize_person_name(&person).ok_or_else(|| {
        "I can add a birthday, but I couldn't determine whose birthday this is. Try \"Add Rachel's birthday on 2003-06-09\".".to_string()
    })?;

    let birthday_year = date_candidate
        .year
        .or_else(|| extract_year_hint(message))
        .ok_or_else(|| {
            "I can add that birthday, but I need the birth year. Try \"Add Rachel's birthday on 2003-06-09\".".to_string()
        })?;
    validate_birthday_year(birthday_year)?;

    let event_date =
        NaiveDate::from_ymd_opt(birthday_year, date_candidate.month, date_candidate.day)
            .ok_or_else(|| {
                "I couldn't parse that birthday date. Use YYYY-MM-DD or a date like June 9, 2003."
                    .to_string()
            })?;

    let name_display = name.clone();
    let title = format!("{name} birthday");
    let summary = format!(
        "Create recurring birthday for {} on {} in {}",
        name_display,
        human_date(event_date),
        scope_summary(&scope),
    );

    Ok(ParsedPendingActionRequest {
        payload: AssistantConfirmationPayload {
            action_kind: AssistantPendingActionKind::CalendarCreateBirthday,
            call: PlannedToolCall {
                tool: AssistantToolName::CalendarCreateBirthday,
                input: AssistantToolInput::CalendarCreateBirthday {
                    scope,
                    title,
                    description: Some("Birthday".to_string()),
                    event_date: event_date.format("%F").to_string(),
                    birthday_year,
                },
            },
            summary,
            conversation_id: conversation_id.map(str::to_string),
        },
    })
}

fn parse_event_request(
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Result<ParsedPendingActionRequest, String> {
    let scope = resolve_scope(user, message)?;
    let date_candidate = extract_first_date_candidate(message).ok_or_else(|| {
        "I can create a calendar event, but I need a date. Try \"Add dentist appointment on 2026-06-09\".".to_string()
    })?;
    let event_date = resolve_event_date(date_candidate.clone())?;
    let title = extract_event_title(message, date_candidate.matched_text.as_deref()).ok_or_else(|| {
        "I can create a calendar event, but I need a title and a date. Try \"Add dentist appointment on 2026-06-09\".".to_string()
    })?;
    let summary = format!(
        "Create calendar event \"{}\" on {} in {}",
        title,
        human_date(event_date),
        scope_summary(&scope),
    );

    Ok(ParsedPendingActionRequest {
        payload: AssistantConfirmationPayload {
            action_kind: AssistantPendingActionKind::CalendarCreateEvent,
            call: PlannedToolCall {
                tool: AssistantToolName::CalendarCreateEvent,
                input: AssistantToolInput::CalendarCreateEvent {
                    scope,
                    title,
                    description: None,
                    event_date: event_date.format("%F").to_string(),
                },
            },
            summary,
            conversation_id: conversation_id.map(str::to_string),
        },
    })
}

fn resolve_scope(user: &AuthUser, message: &str) -> Result<String, String> {
    let lower = message.to_ascii_lowercase();
    let wants_shared = has_any(
        &lower,
        &[
            "shared calendar",
            "global calendar",
            "for everyone",
            "everyone can see",
            "shared event",
            "shared birthday",
        ],
    );
    if wants_shared {
        if user.role != "admin" {
            return Err(
                "Only admins can create shared calendar entries through Rustyfin AI.".to_string(),
            );
        }
        return Ok("global".to_string());
    }

    Ok("personal".to_string())
}

#[derive(Debug, Clone)]
struct DateCandidate {
    month: u32,
    day: u32,
    year: Option<i32>,
    matched_text: Option<String>,
}

fn extract_birthday_subject_and_date(message: &str) -> Option<(String, DateCandidate)> {
    let birthday_regex = birthday_subject_regex();
    let captures = birthday_regex.captures(message)?;
    let subject = captures.get(1)?.as_str().trim().to_string();
    let date_candidate = extract_first_date_candidate(message)?;
    Some((subject, date_candidate))
}

fn extract_first_date_candidate(message: &str) -> Option<DateCandidate> {
    if let Some(iso) = extract_iso_date(message) {
        return Some(iso);
    }
    extract_month_name_date(message)
}

fn extract_iso_date(message: &str) -> Option<DateCandidate> {
    let regex = iso_date_regex();
    let matched = regex.find(message)?;
    let raw = matched.as_str();
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
    Some(DateCandidate {
        month: date.month(),
        day: date.day(),
        year: Some(date.year()),
        matched_text: Some(raw.to_string()),
    })
}

fn extract_month_name_date(message: &str) -> Option<DateCandidate> {
    let regex = month_name_date_regex();
    let captures = regex.captures(message)?;
    let raw = captures.get(0)?.as_str().trim().to_string();
    let month = parse_month_name(captures.get(1)?.as_str())?;
    let day = captures
        .get(2)?
        .as_str()
        .parse::<u32>()
        .ok()
        .filter(|day| (1..=31).contains(day))?;
    let year = captures
        .get(3)
        .and_then(|value| value.as_str().parse::<i32>().ok());
    Some(DateCandidate {
        month,
        day,
        year,
        matched_text: Some(raw),
    })
}

fn resolve_event_date(candidate: DateCandidate) -> Result<NaiveDate, String> {
    if let Some(year) = candidate.year {
        return NaiveDate::from_ymd_opt(year, candidate.month, candidate.day).ok_or_else(|| {
            "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                .to_string()
        });
    }

    let today = Utc::now().date_naive();
    let current_year = today.year();
    let mut date = NaiveDate::from_ymd_opt(current_year, candidate.month, candidate.day)
        .ok_or_else(|| {
            "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                .to_string()
        })?;
    if date < today {
        date = NaiveDate::from_ymd_opt(current_year + 1, candidate.month, candidate.day)
            .ok_or_else(|| {
                "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                    .to_string()
            })?;
    }
    Ok(date)
}

fn extract_event_title(message: &str, matched_date: Option<&str>) -> Option<String> {
    let mut title = message.trim().to_string();
    let lower = title.to_ascii_lowercase();
    for prefix in ["add ", "create ", "make ", "save ", "schedule "] {
        if lower.starts_with(prefix) {
            title = title[prefix.len()..].to_string();
            break;
        }
    }
    if let Some(date_text) = matched_date
        && let Some(index) = title
            .to_ascii_lowercase()
            .find(&date_text.to_ascii_lowercase())
    {
        title = title[..index].trim_end().to_string();
    }

    title = title
        .trim_end_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
        .to_string();

    for suffix in [
        " on",
        " for",
        " to my calendar",
        " in my calendar",
        " on my calendar",
        " calendar",
        " event",
        " events",
    ] {
        if title.to_ascii_lowercase().ends_with(suffix) {
            let end = title.len().saturating_sub(suffix.len());
            title = title[..end].trim_end().to_string();
        }
    }

    title = title
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch.is_whitespace())
        .to_string();

    (!title.is_empty()).then_some(title)
}

fn normalize_person_name(raw: &str) -> Option<String> {
    let mut value = raw
        .trim()
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch.is_whitespace())
        .to_string();
    let lower = value.to_ascii_lowercase();
    for prefix in ["a ", "an ", "the "] {
        if lower.starts_with(prefix) {
            value = value[prefix.len()..].trim_start().to_string();
            break;
        }
    }
    if let Some(stripped) = value.strip_suffix("'s") {
        value = stripped.trim().to_string();
    } else if let Some(stripped) = value.strip_suffix("s'") {
        value = stripped.trim().to_string();
    }
    (!value.is_empty()).then_some(value)
}

fn extract_year_hint(message: &str) -> Option<i32> {
    let regex = year_hint_regex();
    regex
        .captures(message)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<i32>().ok())
}

fn validate_birthday_year(year: i32) -> Result<(), String> {
    let current_year = Utc::now().year();
    if !(1900..=current_year).contains(&year) {
        return Err(format!(
            "I can add that birthday, but the birth year must be between 1900 and {current_year}."
        ));
    }
    Ok(())
}

fn scope_summary(scope: &str) -> &'static str {
    if scope == "global" {
        "the shared calendar"
    } else {
        "your personal calendar"
    }
}

fn human_date(date: NaiveDate) -> String {
    date.format("%B %-d, %Y").to_string()
}

fn parse_month_name(raw: &str) -> Option<u32> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn birthday_subject_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:add|create|make|save|schedule)\s+(.+?)\s+birthday\b")
            .expect("birthday subject regex should compile")
    })
}

fn iso_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").expect("iso date regex should compile")
    })
}

fn month_name_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\s+(\d{1,2})(?:st|nd|rd|th)?(?:,\s*|\s+)?(\d{4})?\b",
        )
        .expect("month name date regex should compile")
    })
}

fn year_hint_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(?:born\s+in|birth\s+year\s+is|birth\s+year|year)\s+(\d{4})\b")
            .expect("year hint regex should compile")
    })
}

#[cfg(test)]
mod tests {
    use super::{AssistantPendingActionKind, pending_action_request_for_message};
    use crate::auth::AuthUser;

    fn test_user(role: &str) -> AuthUser {
        AuthUser {
            user_id: "user-1".to_string(),
            username: "alpha".to_string(),
            role: role.to_string(),
        }
    }

    #[test]
    fn parses_birthday_confirmation_request() {
        let parsed = pending_action_request_for_message(
            &test_user("user"),
            "Add Rachel's birthday on June 9, 2003 to my calendar",
            Some("conversation-1"),
        )
        .expect("birthday intent should be supported")
        .expect("birthday intent should parse");

        assert_eq!(
            parsed.payload.action_kind,
            AssistantPendingActionKind::CalendarCreateBirthday
        );
        assert_eq!(
            parsed.payload.summary,
            "Create recurring birthday for Rachel on June 9, 2003 in your personal calendar"
        );
    }

    #[test]
    fn parses_calendar_event_confirmation_request() {
        let parsed = pending_action_request_for_message(
            &test_user("user"),
            "Add dentist appointment on 2026-06-09 to my calendar",
            None,
        )
        .expect("event intent should be supported")
        .expect("event intent should parse");

        assert_eq!(
            parsed.payload.action_kind,
            AssistantPendingActionKind::CalendarCreateEvent
        );
        assert!(parsed.payload.summary.contains("dentist appointment"));
    }

    #[test]
    fn birthday_request_requires_birth_year() {
        let parsed = pending_action_request_for_message(
            &test_user("user"),
            "Add Rachel's birthday on June 9 to my calendar",
            None,
        )
        .expect("birthday intent should be supported")
        .expect_err("birthday intent should require a birth year");

        assert!(parsed.contains("need the birth year"));
    }
}
