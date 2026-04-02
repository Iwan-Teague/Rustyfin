use std::sync::OnceLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;

use super::dates::{
    DateCandidate, assistant_local_today, assistant_local_year, extract_first_date_candidate,
    resolve_event_date,
};
use super::registry::AssistantToolName;
use super::types::{
    AssistantConfirmationPayload, AssistantPendingActionKind, AssistantToolInput, PlannedToolCall,
};
use crate::auth::AuthUser;
use crate::state::AppState;

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

pub async fn pending_action_request_for_message_with_state(
    state: &AppState,
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Option<Result<ParsedPendingActionRequest, String>> {
    if let Some(result) = pending_action_request_for_message(user, message, conversation_id) {
        return Some(result);
    }

    let lower = message.to_ascii_lowercase();
    if !is_supported_calendar_delete_intent(&lower) {
        return None;
    }

    Some(parse_delete_request(state, user, message, conversation_id).await)
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

pub fn is_supported_calendar_delete_intent(message_lower: &str) -> bool {
    has_any(message_lower, &["delete ", "remove ", "cancel "])
        && has_any(
            message_lower,
            &["calendar", "event", "events", "birthday", "birthdays"],
        )
        && !has_any(
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
    parse_birthday_request_for(user, message, conversation_id, assistant_local_today())
}

fn parse_birthday_request_for(
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
    today: NaiveDate,
) -> Result<ParsedPendingActionRequest, String> {
    let scope = resolve_scope(user, message)?;
    let (person, date_candidate) = extract_birthday_subject_and_date(message, today).ok_or_else(|| {
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
    parse_event_request_for(user, message, conversation_id, assistant_local_today())
}

fn parse_event_request_for(
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
    today: NaiveDate,
) -> Result<ParsedPendingActionRequest, String> {
    let scope = resolve_scope(user, message)?;
    let date_candidate = extract_first_date_candidate(message, today).ok_or_else(|| {
        "I can create a calendar event, but I need a date. Try \"Add dentist appointment on 2026-06-09\".".to_string()
    })?;
    let event_date = resolve_event_date(&date_candidate, today)?;
    let title = extract_event_title(message, Some(date_candidate.matched_text.as_str()))
        .ok_or_else(|| {
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

async fn parse_delete_request(
    state: &AppState,
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Result<ParsedPendingActionRequest, String> {
    let today = assistant_local_today();
    let visible_events = rustfin_db::repo::calendar::list_visible_events(
        &state.db,
        &user.user_id,
        user.role == "admin",
        "1900-01-01",
        "9999-12-31",
    )
    .await
    .map_err(|e| format!("failed to load visible calendar events for deletion: {e}"))?;

    let target = select_delete_target(user, message, today, &visible_events)?;
    let event_date = parse_calendar_row_date(&target.event_date)
        .ok_or_else(|| format!("calendar event {} has an invalid date", target.id))?;

    let summary = if target.event_type == "birthday" {
        format!(
            "Delete recurring birthday \"{}\" on {} from {}",
            target.title,
            human_date(event_date),
            scope_summary(&target.scope),
        )
    } else {
        format!(
            "Delete calendar event \"{}\" on {} from {}",
            target.title,
            human_date(event_date),
            scope_summary(&target.scope),
        )
    };

    Ok(ParsedPendingActionRequest {
        payload: AssistantConfirmationPayload {
            action_kind: AssistantPendingActionKind::CalendarDeleteEvent,
            call: PlannedToolCall {
                tool: AssistantToolName::CalendarDeleteEvent,
                input: AssistantToolInput::CalendarDeleteEvent {
                    event_id: target.id.clone(),
                    title: target.title.clone(),
                    event_date: target.event_date.clone(),
                    scope: target.scope.clone(),
                    event_type: target.event_type.clone(),
                    recurrence: target.recurrence.clone(),
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

fn extract_birthday_subject_and_date(
    message: &str,
    today: NaiveDate,
) -> Option<(String, DateCandidate)> {
    let birthday_regex = birthday_subject_regex();
    let captures = birthday_regex.captures(message)?;
    let subject = captures.get(1)?.as_str().trim().to_string();
    let date_candidate = extract_first_date_candidate(message, today)?;
    Some((subject, date_candidate))
}

fn select_delete_target<'a>(
    user: &AuthUser,
    message: &str,
    today: NaiveDate,
    visible_events: &'a [rustfin_db::repo::calendar::CalendarEventRow],
) -> Result<&'a rustfin_db::repo::calendar::CalendarEventRow, String> {
    let lower = message.to_ascii_lowercase();
    let date_candidate = extract_first_date_candidate(message, today);
    let resolved_date = date_candidate
        .as_ref()
        .map(|candidate| resolve_event_date(candidate, today))
        .transpose()?;
    let matched_date = date_candidate
        .as_ref()
        .map(|candidate| candidate.matched_text.as_str());
    let next_event_only = delete_targets_next_calendar_event(&lower) && matched_date.is_none();
    let query = if next_event_only {
        None
    } else {
        extract_delete_event_query(message, matched_date)
    };
    let wants_birthday = lower.contains("birthday");

    if !next_event_only && query.is_none() && resolved_date.is_none() {
        return Err(
            "I can delete a calendar entry, but I need the event name, a date, or a phrase like \"my next event\"."
                .to_string(),
        );
    }

    if next_event_only {
        let mut candidates = visible_events
            .iter()
            .filter(|event| calendar_user_can_manage_event(user, event))
            .filter_map(|event| {
                delete_next_occurrence_on_or_after(event, today)
                    .map(|next_occurs_on| (event, next_occurs_on))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.0.title.cmp(&right.0.title))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        return candidates
            .into_iter()
            .map(|(event, _)| event)
            .next()
            .ok_or_else(|| {
                "You do not have a deletable upcoming calendar event right now.".to_string()
            });
    }

    let matching_visible = visible_events
        .iter()
        .filter(|event| {
            delete_candidate_matches(event, query.as_deref(), resolved_date, wants_birthday)
        })
        .collect::<Vec<_>>();

    if matching_visible.is_empty() {
        return Err(delete_no_match_message(
            query.as_deref(),
            resolved_date,
            wants_birthday,
        ));
    }

    let manageable = matching_visible
        .into_iter()
        .filter(|event| calendar_user_can_manage_event(user, event))
        .collect::<Vec<_>>();

    if manageable.is_empty() {
        return Err(
            "I found a matching calendar entry, but your Rustyfin account is not allowed to delete it."
                .to_string(),
        );
    }

    if manageable.len() > 1 {
        let matches = manageable
            .iter()
            .take(3)
            .map(|event| describe_delete_candidate(event))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "I found more than one matching calendar entry: {matches}. Include the date or a more specific title so I delete the right one."
        ));
    }

    Ok(manageable[0])
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

fn extract_delete_event_query(message: &str, matched_date: Option<&str>) -> Option<String> {
    let mut query = message.trim().to_string();
    let lower = query.to_ascii_lowercase();
    for prefix in ["delete ", "remove ", "cancel "] {
        if lower.starts_with(prefix) {
            query = query[prefix.len()..].to_string();
            break;
        }
    }
    if let Some(date_text) = matched_date
        && let Some(index) = query
            .to_ascii_lowercase()
            .find(&date_text.to_ascii_lowercase())
    {
        query = query[..index].trim_end().to_string();
    }

    query = query
        .trim_end_matches(|ch: char| ch.is_ascii_punctuation() || ch.is_whitespace())
        .to_string();

    for suffix in [
        " on",
        " for",
        " from my calendar",
        " in my calendar",
        " on my calendar",
        " from the calendar",
        " from calendar",
        " in the calendar",
        " in calendar",
        " calendar",
    ] {
        if query.to_ascii_lowercase().ends_with(suffix) {
            let end = query.len().saturating_sub(suffix.len());
            query = query[..end].trim_end().to_string();
        }
    }

    query = query
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch.is_whitespace())
        .to_string();

    if delete_targets_next_calendar_event(&query.to_ascii_lowercase()) {
        return None;
    }

    (!query.is_empty()).then_some(query)
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
    let current_year = assistant_local_year();
    if !(1900..=current_year).contains(&year) {
        return Err(format!(
            "I can add that birthday, but the birth year must be between 1900 and {current_year}."
        ));
    }
    Ok(())
}

fn parse_calendar_row_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn delete_next_occurrence_on_or_after(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    on_or_after: NaiveDate,
) -> Option<NaiveDate> {
    let source_date = parse_calendar_row_date(&event.event_date)?;
    if event.recurrence != "yearly" {
        return (source_date >= on_or_after).then_some(source_date);
    }

    let current_year = on_or_after.year();
    for year in [current_year, current_year + 1] {
        let candidate = with_year_safe(source_date, year)?;
        if candidate >= on_or_after {
            return Some(candidate);
        }
    }

    None
}

fn with_year_safe(date: NaiveDate, year: i32) -> Option<NaiveDate> {
    if let Some(updated) = date.with_year(year) {
        return Some(updated);
    }
    if date.month() == 2 && date.day() == 29 {
        return NaiveDate::from_ymd_opt(year, 2, 28);
    }
    None
}

fn delete_candidate_matches(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    query: Option<&str>,
    resolved_date: Option<NaiveDate>,
    wants_birthday: bool,
) -> bool {
    if wants_birthday && event.event_type != "birthday" {
        return false;
    }
    if let Some(date) = resolved_date
        && !calendar_event_matches_date(event, date)
    {
        return false;
    }
    if let Some(query) = query {
        return calendar_event_matches_query_for_delete(event, query);
    }
    true
}

fn calendar_event_matches_date(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    date: NaiveDate,
) -> bool {
    let Some(source_date) = parse_calendar_row_date(&event.event_date) else {
        return false;
    };
    if event.recurrence == "yearly" {
        return source_date.month() == date.month() && source_date.day() == date.day();
    }
    source_date == date
}

fn calendar_event_matches_query_for_delete(
    event: &rustfin_db::repo::calendar::CalendarEventRow,
    query: &str,
) -> bool {
    let normalized_query = normalize_calendar_text(query);
    if normalized_query.is_empty() {
        return true;
    }

    [
        Some(event.title.as_str()),
        event.description.as_deref(),
        event.owner_username.as_deref(),
        event.created_by_username.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(normalize_calendar_text)
    .any(|value| value.contains(&normalized_query) || normalized_query.contains(value.as_str()))
}

fn normalize_calendar_text(value: &str) -> String {
    value
        .replace("'s", "")
        .replace("’s", "")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn delete_targets_next_calendar_event(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "next event",
            "next calendar event",
            "next thing coming up",
            "coming up next on my calendar",
            "my next event",
            "the next event",
        ],
    )
}

fn calendar_user_can_manage_event(
    user: &AuthUser,
    event: &rustfin_db::repo::calendar::CalendarEventRow,
) -> bool {
    user.role == "admin"
        || (event.scope == "personal"
            && event.owner_user_id.as_deref() == Some(user.user_id.as_str()))
}

fn delete_no_match_message(
    query: Option<&str>,
    resolved_date: Option<NaiveDate>,
    wants_birthday: bool,
) -> String {
    match (query, resolved_date, wants_birthday) {
        (Some(query), Some(date), true) => format!(
            "I couldn't find a visible birthday matching \"{query}\" on {}.",
            human_date(date)
        ),
        (Some(query), Some(date), false) => format!(
            "I couldn't find a visible calendar event matching \"{query}\" on {}.",
            human_date(date)
        ),
        (Some(query), None, true) => {
            format!("I couldn't find a visible birthday matching \"{query}\".")
        }
        (Some(query), None, false) => {
            format!("I couldn't find a visible calendar event matching \"{query}\".")
        }
        (None, Some(date), true) => {
            format!(
                "I couldn't find a visible birthday on {}.",
                human_date(date)
            )
        }
        (None, Some(date), false) => format!(
            "I couldn't find a visible calendar event on {}.",
            human_date(date)
        ),
        (None, None, true) => "I couldn't find a visible birthday to delete.".to_string(),
        (None, None, false) => "I couldn't find a visible calendar event to delete.".to_string(),
    }
}

fn describe_delete_candidate(event: &rustfin_db::repo::calendar::CalendarEventRow) -> String {
    let date = parse_calendar_row_date(&event.event_date)
        .map(human_date)
        .unwrap_or_else(|| event.event_date.clone());
    format!("\"{}\" on {}", event.title, date)
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

fn year_hint_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(?:born\s+in|birth\s+year\s+is|birth\s+year|year)\s+(\d{4})\b")
            .expect("year hint regex should compile")
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{
        AssistantPendingActionKind, calendar_event_matches_query_for_delete,
        extract_delete_event_query, parse_birthday_request_for, parse_event_request_for,
        pending_action_request_for_message, select_delete_target,
    };
    use crate::auth::AuthUser;
    use rustfin_db::repo::calendar::CalendarEventRow;

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

    #[test]
    fn parses_relative_weekday_event_against_local_today() {
        let parsed = parse_event_request_for(
            &test_user("user"),
            "Make test event for next Tuesday",
            None,
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
        )
        .expect("event intent should parse");

        assert_eq!(
            parsed.payload.summary,
            "Create calendar event \"test event\" on April 7, 2026 in your personal calendar"
        );
    }

    #[test]
    fn parses_day_first_month_name_event_without_second_prompt() {
        let parsed = parse_event_request_for(
            &test_user("user"),
            "Make the test event for 7th of April",
            None,
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
        )
        .expect("event intent should parse");

        assert_eq!(
            parsed.payload.summary,
            "Create calendar event \"the test event\" on April 7, 2026 in your personal calendar"
        );
    }

    #[test]
    fn parses_day_first_birthday_date() {
        let parsed = parse_birthday_request_for(
            &test_user("user"),
            "Add Rachel's birthday on the 7th of April 2003 to my calendar",
            None,
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
        )
        .expect("birthday intent should parse");

        assert_eq!(
            parsed.payload.summary,
            "Create recurring birthday for Rachel on April 7, 2003 in your personal calendar"
        );
    }

    fn sample_event(
        id: &str,
        title: &str,
        event_date: &str,
        event_type: &str,
        recurrence: &str,
        owner_user_id: Option<&str>,
    ) -> CalendarEventRow {
        CalendarEventRow {
            id: id.to_string(),
            scope: if owner_user_id.is_some() {
                "personal".to_string()
            } else {
                "global".to_string()
            },
            owner_user_id: owner_user_id.map(str::to_string),
            owner_username: owner_user_id.map(|_| "alpha".to_string()),
            title: title.to_string(),
            description: Some("Calendar item".to_string()),
            event_date: event_date.to_string(),
            event_type: event_type.to_string(),
            recurrence: recurrence.to_string(),
            birthday_year: (event_type == "birthday").then(|| {
                event_date
                    .split('-')
                    .next()
                    .unwrap()
                    .parse::<i32>()
                    .unwrap()
            }),
            created_by_user_id: "user-1".to_string(),
            created_by_username: Some("alpha".to_string()),
            created_ts: 0,
            updated_ts: 0,
        }
    }

    #[test]
    fn extract_delete_query_strips_delete_scaffolding() {
        let query = extract_delete_event_query(
            "Delete dentist appointment on 2026-06-09 from my calendar",
            Some("2026-06-09"),
        )
        .expect("expected delete query");

        assert_eq!(query, "dentist appointment");
    }

    #[test]
    fn delete_query_matches_event_title() {
        let event = sample_event(
            "event-1",
            "Dentist appointment",
            "2026-06-09",
            "event",
            "none",
            Some("user-1"),
        );
        assert!(calendar_event_matches_query_for_delete(
            &event,
            "dentist appointment"
        ));
    }

    #[test]
    fn select_delete_target_prefers_exact_title_and_date() {
        let events = vec![
            sample_event(
                "event-1",
                "Dentist appointment",
                "2026-06-09",
                "event",
                "none",
                Some("user-1"),
            ),
            sample_event(
                "event-2",
                "Dentist appointment",
                "2026-06-10",
                "event",
                "none",
                Some("user-1"),
            ),
        ];

        let selected = select_delete_target(
            &test_user("user"),
            "Delete dentist appointment on 2026-06-10 from my calendar",
            NaiveDate::from_ymd_opt(2026, 4, 2).unwrap(),
            &events,
        )
        .expect("expected delete target");

        assert_eq!(selected.id, "event-2");
    }

    #[test]
    fn select_delete_target_resolves_next_event_phrase() {
        let events = vec![
            sample_event(
                "event-1",
                "First upcoming event",
                "2026-04-04",
                "event",
                "none",
                Some("user-1"),
            ),
            sample_event(
                "event-2",
                "Later event",
                "2026-05-04",
                "event",
                "none",
                Some("user-1"),
            ),
        ];

        let selected = select_delete_target(
            &test_user("user"),
            "Delete my next event",
            NaiveDate::from_ymd_opt(2026, 4, 2).unwrap(),
            &events,
        )
        .expect("expected delete target");

        assert_eq!(selected.id, "event-1");
    }
}
