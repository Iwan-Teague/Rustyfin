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
const MAX_DOCUMENT_TITLE_CHARS: usize = 80;
const MAX_DOCUMENT_FILE_NAME_CHARS: usize = 96;

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
    model_name: &str,
) -> Option<Result<ParsedPendingActionRequest, String>> {
    if let Some(result) = pending_action_request_for_message(user, message, conversation_id) {
        return Some(result);
    }

    let lower = message.to_ascii_lowercase();
    if is_supported_document_create_intent(&lower) {
        return Some(parse_document_request(message, conversation_id, model_name));
    }
    if is_supported_conversation_manage_intent(&lower) {
        return Some(
            parse_conversation_manage_request(state, user, message, conversation_id).await,
        );
    }
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

pub fn is_supported_document_create_intent(message_lower: &str) -> bool {
    has_any(
        message_lower,
        &[
            "create ",
            "make ",
            "write ",
            "generate ",
            "prepare ",
            "save ",
            "export ",
        ],
    ) && has_any(
        message_lower,
        &[
            "document",
            "markdown",
            "plain text",
            "text file",
            "txt file",
            "report",
            "notes",
            "note file",
        ],
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

pub fn is_supported_conversation_manage_intent(message_lower: &str) -> bool {
    let archive_intent = message_lower.contains("archive")
        || (message_lower.contains("move") && message_lower.contains("archive"));
    let delete_intent = has_any(message_lower, &["delete ", "remove ", "clear "]);

    (archive_intent || delete_intent)
        && has_any(
            message_lower,
            &[
                "conversation",
                "conversations",
                "chat",
                "chats",
                "chat history",
                "ai history",
                "ai conversation",
                "ai conversations",
            ],
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

fn parse_document_request(
    message: &str,
    conversation_id: Option<&str>,
    model_name: &str,
) -> Result<ParsedPendingActionRequest, String> {
    let lower = message.to_ascii_lowercase();
    let format = detect_document_format(&lower)?;
    let meaningful_tokens = meaningful_document_request_tokens(message);
    let only_pronoun_reference = meaningful_tokens
        .iter()
        .all(|token| matches!(token.as_str(), "that" | "this" | "it"));
    if meaningful_tokens.is_empty() || (only_pronoun_reference && conversation_id.is_none()) {
        return Err(
            "I can create a downloadable markdown or plain-text document, but I need to know what it should contain. Try \"Create a markdown document summarizing my next event\"."
                .to_string(),
        );
    }

    let title = extract_document_title(message)
        .unwrap_or_else(|| default_document_title(&meaningful_tokens))
        .chars()
        .take(MAX_DOCUMENT_TITLE_CHARS)
        .collect::<String>();
    let file_name = extract_document_file_name(message, format.extension())
        .unwrap_or_else(|| default_document_file_name(&title, format.extension()));

    let summary = format!(
        "Create downloadable {} document \"{}\"",
        format.label(),
        file_name,
    );

    Ok(ParsedPendingActionRequest {
        payload: AssistantConfirmationPayload {
            action_kind: AssistantPendingActionKind::DocumentCreateDownload,
            call: PlannedToolCall {
                tool: AssistantToolName::DocumentCreateDownload,
                input: AssistantToolInput::DocumentCreateDownload {
                    title,
                    file_name,
                    format: format.as_str().to_string(),
                    request_prompt: message.trim().to_string(),
                    model_name: model_name.trim().to_string(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversationActionOperation {
    Archive,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversationSelectionOrder {
    DisplayFirst,
    MostRecent,
    AllMatching,
}

#[derive(Debug, Clone)]
struct ConversationSelectionCandidate {
    id: String,
    title: String,
    group_name: Option<String>,
    archived: bool,
    sort_order: i64,
    updated_ts: i64,
}

#[derive(Debug, Clone)]
struct ResolvedConversationSelection {
    selection_label: String,
    selected: Vec<ConversationSelectionCandidate>,
    current_excluded: bool,
}

async fn parse_conversation_manage_request(
    state: &AppState,
    user: &AuthUser,
    message: &str,
    conversation_id: Option<&str>,
) -> Result<ParsedPendingActionRequest, String> {
    let operation = detect_conversation_action_operation(message).ok_or_else(|| {
        "I can archive or delete AI conversations, but I couldn't determine which action you wanted."
            .to_string()
    })?;
    let rows = rustfin_db::repo::ai_conversations::list_conversations_for_user(
        &state.db,
        &user.user_id,
        true,
        200,
    )
    .await
    .map_err(|e| format!("failed to load AI conversations: {e}"))?;
    let candidates = rows
        .into_iter()
        .map(|row| ConversationSelectionCandidate {
            id: row.id,
            title: row.title,
            group_name: row.group_name,
            archived: row.archived,
            sort_order: row.sort_order,
            updated_ts: row.updated_ts,
        })
        .collect::<Vec<_>>();

    let selection =
        resolve_conversation_selection(message, conversation_id, operation, &candidates)?;
    let titles = selection
        .selected
        .iter()
        .map(|candidate| candidate.title.clone())
        .collect::<Vec<_>>();
    let summary = conversation_action_summary(operation, &selection);

    Ok(ParsedPendingActionRequest {
        payload: AssistantConfirmationPayload {
            action_kind: match operation {
                ConversationActionOperation::Archive => {
                    AssistantPendingActionKind::ConversationArchive
                }
                ConversationActionOperation::Delete => {
                    AssistantPendingActionKind::ConversationDelete
                }
            },
            call: PlannedToolCall {
                tool: match operation {
                    ConversationActionOperation::Archive => {
                        AssistantToolName::ConversationsArchiveSelection
                    }
                    ConversationActionOperation::Delete => {
                        AssistantToolName::ConversationsDeleteSelection
                    }
                },
                input: match operation {
                    ConversationActionOperation::Archive => {
                        AssistantToolInput::ConversationArchive {
                            conversation_ids: selection
                                .selected
                                .iter()
                                .map(|candidate| candidate.id.clone())
                                .collect(),
                            titles,
                            selection_label: selection.selection_label.clone(),
                            archived: true,
                        }
                    }
                    ConversationActionOperation::Delete => AssistantToolInput::ConversationDelete {
                        conversation_ids: selection
                            .selected
                            .iter()
                            .map(|candidate| candidate.id.clone())
                            .collect(),
                        titles,
                        selection_label: selection.selection_label.clone(),
                    },
                },
            },
            summary,
            conversation_id: conversation_id.map(str::to_string),
        },
    })
}

fn detect_conversation_action_operation(message: &str) -> Option<ConversationActionOperation> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("archive") || (lower.contains("move") && lower.contains("archive")) {
        return Some(ConversationActionOperation::Archive);
    }
    if has_any(&lower, &["delete ", "remove ", "clear "]) {
        return Some(ConversationActionOperation::Delete);
    }
    None
}

fn resolve_conversation_selection(
    message: &str,
    conversation_id: Option<&str>,
    operation: ConversationActionOperation,
    candidates: &[ConversationSelectionCandidate],
) -> Result<ResolvedConversationSelection, String> {
    let lower = message.to_ascii_lowercase();
    let requested_group = detect_conversation_group_name(message, candidates)?;
    let requested_count = extract_conversation_count_hint(message);
    let selection_order =
        determine_conversation_selection_order(&lower, requested_group.is_some(), requested_count);

    let mut eligible = candidates
        .iter()
        .filter(|candidate| {
            requested_group
                .as_deref()
                .map(|group_name| {
                    candidate
                        .group_name
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(group_name))
                })
                .unwrap_or(true)
        })
        .filter(|candidate| match operation {
            ConversationActionOperation::Archive => !candidate.archived,
            ConversationActionOperation::Delete => {
                if conversation_delete_targets_archived_only(&lower) {
                    candidate.archived
                } else {
                    true
                }
            }
        })
        .cloned()
        .collect::<Vec<_>>();

    if eligible.is_empty() {
        return Err(conversation_selection_empty_message(
            operation,
            requested_group.as_deref(),
        ));
    }

    let selected = match selection_order {
        ConversationSelectionOrder::AllMatching => eligible.clone(),
        ConversationSelectionOrder::DisplayFirst => {
            if let Some(count) = requested_count {
                eligible.into_iter().take(count).collect::<Vec<_>>()
            } else {
                eligible.clone()
            }
        }
        ConversationSelectionOrder::MostRecent => {
            eligible.sort_by(|left, right| {
                right
                    .updated_ts
                    .cmp(&left.updated_ts)
                    .then_with(|| right.sort_order.cmp(&left.sort_order))
                    .then_with(|| left.title.cmp(&right.title))
                    .then_with(|| left.id.cmp(&right.id))
            });
            if let Some(count) = requested_count {
                eligible.into_iter().take(count).collect::<Vec<_>>()
            } else {
                eligible
            }
        }
    };

    let mut selected = selected;
    let mut current_excluded = false;
    if operation == ConversationActionOperation::Delete
        && let Some(current_id) = conversation_id
        && selected.iter().any(|candidate| candidate.id == current_id)
    {
        current_excluded = true;
        selected.retain(|candidate| candidate.id != current_id);

        if let Some(count) = requested_count {
            let mut ordered_candidates = candidates
                .iter()
                .filter(|candidate| {
                    requested_group
                        .as_deref()
                        .map(|group_name| {
                            candidate
                                .group_name
                                .as_deref()
                                .is_some_and(|value| value.eq_ignore_ascii_case(group_name))
                        })
                        .unwrap_or(true)
                })
                .filter(|candidate| {
                    !candidate.id.eq(current_id)
                        && (!conversation_delete_targets_archived_only(&lower)
                            || candidate.archived)
                })
                .cloned()
                .collect::<Vec<_>>();

            if matches!(selection_order, ConversationSelectionOrder::MostRecent) {
                ordered_candidates.sort_by(|left, right| {
                    right
                        .updated_ts
                        .cmp(&left.updated_ts)
                        .then_with(|| right.sort_order.cmp(&left.sort_order))
                        .then_with(|| left.title.cmp(&right.title))
                        .then_with(|| left.id.cmp(&right.id))
                });
            }

            for candidate in ordered_candidates {
                if selected.len() >= count {
                    break;
                }
                if selected.iter().any(|existing| existing.id == candidate.id) {
                    continue;
                }
                selected.push(candidate);
            }
        }
    }

    if selected.is_empty() {
        return Err(match operation {
            ConversationActionOperation::Archive => {
                "I couldn't find any AI conversations to archive with that description."
                    .to_string()
            }
            ConversationActionOperation::Delete => {
                "I can't delete the conversation we're currently using from inside itself. Open another conversation or choose a different target."
                    .to_string()
            }
        });
    }

    let selection_label = conversation_selection_label(
        operation,
        selection_order,
        requested_count,
        selected.len(),
        requested_group.as_deref(),
    );

    Ok(ResolvedConversationSelection {
        selection_label,
        selected,
        current_excluded,
    })
}

fn determine_conversation_selection_order(
    lower: &str,
    has_group: bool,
    requested_count: Option<usize>,
) -> ConversationSelectionOrder {
    if has_any(lower, &["all ", "all of", "every ", "entire ", "whole "])
        || lower.contains("history")
    {
        return ConversationSelectionOrder::AllMatching;
    }
    if has_any(lower, &["first ", "top "]) {
        return ConversationSelectionOrder::DisplayFirst;
    }
    if has_any(lower, &["latest ", "most recent", "recent ", "last "]) {
        return ConversationSelectionOrder::MostRecent;
    }
    if has_group && requested_count.is_none() {
        return ConversationSelectionOrder::AllMatching;
    }
    if requested_count.is_some() {
        return ConversationSelectionOrder::MostRecent;
    }
    ConversationSelectionOrder::AllMatching
}

fn conversation_delete_targets_archived_only(lower: &str) -> bool {
    lower.contains("archived conversation")
        || lower.contains("archived conversations")
        || lower.contains("archive conversation")
        || lower.contains("archive conversations")
}

fn conversation_selection_label(
    operation: ConversationActionOperation,
    order: ConversationSelectionOrder,
    requested_count: Option<usize>,
    selected_count: usize,
    group_name: Option<&str>,
) -> String {
    let count = selected_count;
    let noun = if count == 1 {
        "AI conversation"
    } else {
        "AI conversations"
    };
    let verb = match operation {
        ConversationActionOperation::Archive => "Archive",
        ConversationActionOperation::Delete => "Delete",
    };
    match (order, requested_count, group_name) {
        (ConversationSelectionOrder::AllMatching, _, Some(group)) => {
            format!("{verb} all {noun} in group \"{group}\"")
        }
        (ConversationSelectionOrder::AllMatching, _, None) => {
            format!("{verb} all {noun}")
        }
        (ConversationSelectionOrder::DisplayFirst, Some(requested), Some(group)) => format!(
            "{verb} the first {} {noun} in group \"{group}\"",
            requested.min(count)
        ),
        (ConversationSelectionOrder::DisplayFirst, Some(requested), None) => {
            format!("{verb} the first {} {noun}", requested.min(count))
        }
        (ConversationSelectionOrder::MostRecent, Some(requested), Some(group)) => format!(
            "{verb} the {} most recent {noun} in group \"{group}\"",
            requested.min(count)
        ),
        (ConversationSelectionOrder::MostRecent, Some(requested), None) => {
            format!("{verb} the {} most recent {noun}", requested.min(count))
        }
        (_, _, Some(group)) => format!("{verb} {count} {noun} in group \"{group}\""),
        _ => format!("{verb} {count} {noun}"),
    }
}

fn conversation_action_summary(
    operation: ConversationActionOperation,
    selection: &ResolvedConversationSelection,
) -> String {
    let mut lines = vec![format!("{}:", selection.selection_label)];
    for title in selection
        .selected
        .iter()
        .take(8)
        .map(|candidate| &candidate.title)
    {
        lines.push(format!("- {title}"));
    }
    if selection.selected.len() > 8 {
        lines.push(format!(
            "- and {} more",
            selection.selected.len().saturating_sub(8)
        ));
    }
    if matches!(operation, ConversationActionOperation::Delete) {
        lines.push("This will permanently remove them from your AI history.".to_string());
    }
    if selection.current_excluded {
        lines.push(
            "The current conversation is excluded because Rustyfin AI cannot delete the active conversation from inside itself."
                .to_string(),
        );
    }
    lines.join("\n")
}

fn conversation_selection_empty_message(
    operation: ConversationActionOperation,
    group_name: Option<&str>,
) -> String {
    match (operation, group_name) {
        (ConversationActionOperation::Archive, Some(group_name)) => {
            format!("I couldn't find any non-archived AI conversations in group \"{group_name}\".")
        }
        (ConversationActionOperation::Archive, None) => {
            "I couldn't find any AI conversations to archive with that description.".to_string()
        }
        (ConversationActionOperation::Delete, Some(group_name)) => {
            format!("I couldn't find any AI conversations to delete in group \"{group_name}\".")
        }
        (ConversationActionOperation::Delete, None) => {
            "I couldn't find any AI conversations to delete with that description.".to_string()
        }
    }
}

fn detect_conversation_group_name(
    message: &str,
    candidates: &[ConversationSelectionCandidate],
) -> Result<Option<String>, String> {
    let lower = message.to_ascii_lowercase();
    let mut groups = candidates
        .iter()
        .filter_map(|candidate| candidate.group_name.clone())
        .filter(|group_name| !group_name.trim().is_empty())
        .collect::<Vec<_>>();
    groups.sort_by_key(|group_name| std::cmp::Reverse(group_name.len()));
    groups.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    if let Some(quoted) = extract_quoted_phrase(message) {
        if let Some(group_name) = groups
            .iter()
            .find(|group_name| group_name.eq_ignore_ascii_case(quoted.trim()))
        {
            return Ok(Some(group_name.clone()));
        }
    }

    for group_name in &groups {
        let normalized = group_name.to_ascii_lowercase();
        if lower.contains(&format!("group {normalized}"))
            || lower.contains(&format!("in {normalized}"))
            || lower.contains(&format!("from {normalized}"))
            || lower.contains(&format!("named {normalized}"))
            || lower.contains(&format!("called {normalized}"))
        {
            return Ok(Some(group_name.clone()));
        }
    }

    if lower.contains("group ") {
        return Err("I couldn't match that AI conversation group. Use the exact group name shown in the left conversation rail.".to_string());
    }

    Ok(None)
}

fn extract_conversation_count_hint(message: &str) -> Option<usize> {
    let lower = message.to_ascii_lowercase();
    let regex = conversation_count_regex();
    let captures = regex.captures(&lower)?;
    let value = captures.get(1)?.as_str();
    if let Ok(parsed) = value.parse::<usize>() {
        return Some(parsed.clamp(1, 200));
    }
    small_count_word(value)
}

fn small_count_word(value: &str) -> Option<usize> {
    match value {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        _ => None,
    }
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
    value = extract_named_person_fragment(&value).unwrap_or(value);
    if relationship_only_subject(&value) {
        return None;
    }
    (!value.is_empty()).then_some(value)
}

fn extract_named_person_fragment(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();

    for prefix in [
        "my brother named ",
        "my brother called ",
        "my brother ",
        "my sister named ",
        "my sister called ",
        "my sister ",
        "my son named ",
        "my son called ",
        "my son ",
        "my daughter named ",
        "my daughter called ",
        "my daughter ",
        "my dad named ",
        "my dad called ",
        "my dad ",
        "my father named ",
        "my father called ",
        "my father ",
        "my mum named ",
        "my mum called ",
        "my mum ",
        "my mom named ",
        "my mom called ",
        "my mom ",
        "my mother named ",
        "my mother called ",
        "my mother ",
        "my wife named ",
        "my wife called ",
        "my wife ",
        "my husband named ",
        "my husband called ",
        "my husband ",
        "my partner named ",
        "my partner called ",
        "my partner ",
        "my friend named ",
        "my friend called ",
        "my friend ",
        "my cousin named ",
        "my cousin called ",
        "my cousin ",
    ] {
        if lower.starts_with(prefix) {
            let candidate = trimmed[prefix.len()..]
                .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == ',' || ch.is_whitespace())
                .trim();
            if !candidate.is_empty() && !relationship_only_subject(candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

fn relationship_only_subject(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "my brother"
            | "brother"
            | "my sister"
            | "sister"
            | "my son"
            | "son"
            | "my daughter"
            | "daughter"
            | "my dad"
            | "dad"
            | "my father"
            | "father"
            | "my mum"
            | "mum"
            | "my mom"
            | "mom"
            | "my mother"
            | "mother"
            | "my wife"
            | "wife"
            | "my husband"
            | "husband"
            | "my partner"
            | "partner"
            | "my friend"
            | "friend"
            | "my cousin"
            | "cousin"
            | "my uncle"
            | "uncle"
            | "my aunt"
            | "aunt"
            | "my nephew"
            | "nephew"
            | "my niece"
            | "niece"
            | "my grandfather"
            | "grandfather"
            | "my grandmother"
            | "grandmother"
            | "my grandad"
            | "grandad"
            | "my granny"
            | "granny"
            | "my grandpa"
            | "grandpa"
            | "my grandma"
            | "grandma"
    )
}

fn extract_year_hint(message: &str) -> Option<i32> {
    let regex = year_hint_regex();
    regex
        .captures(message)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<i32>().ok())
}

#[derive(Clone, Copy)]
enum GeneratedDocumentFormat {
    Markdown,
    Text,
}

impl GeneratedDocumentFormat {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Text => "plain-text",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Text => "txt",
        }
    }
}

fn detect_document_format(message_lower: &str) -> Result<GeneratedDocumentFormat, String> {
    if has_any(
        message_lower,
        &[
            "pdf",
            "docx",
            "word document",
            "spreadsheet",
            "csv",
            "json",
            "html",
        ],
    ) {
        return Err(
            "Rustyfin AI can currently create downloadable markdown or plain-text documents only."
                .to_string(),
        );
    }
    if has_any(
        message_lower,
        &["plain text", "text file", "txt file", ".txt", "plain-text"],
    ) {
        Ok(GeneratedDocumentFormat::Text)
    } else {
        Ok(GeneratedDocumentFormat::Markdown)
    }
}

fn meaningful_document_request_tokens(message: &str) -> Vec<String> {
    message
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '\'' {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|token| {
            !matches!(
                *token,
                "a" | "an"
                    | "the"
                    | "please"
                    | "create"
                    | "make"
                    | "write"
                    | "generate"
                    | "prepare"
                    | "save"
                    | "export"
                    | "me"
                    | "my"
                    | "downloadable"
                    | "markdown"
                    | "plain"
                    | "text"
                    | "file"
                    | "document"
                    | "report"
                    | "note"
                    | "notes"
                    | "called"
                    | "named"
                    | "as"
                    | "into"
                    | "to"
            )
        })
        .map(str::to_string)
        .collect()
}

fn extract_document_title(message: &str) -> Option<String> {
    let quoted = extract_quoted_phrase(message)?;
    let trimmed = quoted.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_extension = trimmed
        .strip_suffix(".md")
        .or_else(|| trimmed.strip_suffix(".txt"))
        .unwrap_or(trimmed)
        .trim();
    (!without_extension.is_empty()).then_some(without_extension.to_string())
}

fn extract_document_file_name(message: &str, default_extension: &str) -> Option<String> {
    let quoted = extract_quoted_phrase(message)?;
    normalize_document_file_name(quoted.trim(), default_extension)
}

fn extract_quoted_phrase(message: &str) -> Option<&str> {
    let start = message.find('"').or_else(|| message.find('\''))?;
    let quote = message[start..].chars().next()?;
    let rest = &message[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn default_document_title(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return "Rustyfin note".to_string();
    }
    tokens
        .iter()
        .take(6)
        .map(|token| {
            let mut chars = token.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_document_file_name(title: &str, extension: &str) -> String {
    let stem = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let stem = if stem.is_empty() {
        "rustyfin-note".to_string()
    } else {
        stem
    };
    format!(
        "{}.{}",
        stem.chars()
            .take(MAX_DOCUMENT_FILE_NAME_CHARS.saturating_sub(extension.len() + 1))
            .collect::<String>(),
        extension
    )
}

fn normalize_document_file_name(raw: &str, default_extension: &str) -> Option<String> {
    let mut normalized = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if normalized.is_empty() {
        return None;
    }

    if !normalized.ends_with(".md") && !normalized.ends_with(".txt") {
        normalized.push('.');
        normalized.push_str(default_extension);
    }

    let file_name = normalized
        .chars()
        .take(MAX_DOCUMENT_FILE_NAME_CHARS)
        .collect::<String>();
    (!file_name.is_empty()).then_some(file_name)
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

fn conversation_count_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:first|top|last|latest|recent|most\s+recent|delete|remove|clear|archive)\s+(one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|\d{1,3})\b",
        )
        .expect("conversation count regex should compile")
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{
        AssistantPendingActionKind, AssistantToolInput, ConversationActionOperation,
        ConversationSelectionCandidate, calendar_event_matches_query_for_delete,
        extract_delete_event_query, is_supported_conversation_manage_intent,
        is_supported_document_create_intent, parse_birthday_request_for, parse_document_request,
        parse_event_request_for, pending_action_request_for_message,
        resolve_conversation_selection, select_delete_target,
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

    #[test]
    fn birthday_request_prefers_named_person_over_relationship_label() {
        let parsed = parse_birthday_request_for(
            &test_user("user"),
            "Add my brother Deri's birthday on June 9, 2003 to my calendar",
            None,
            NaiveDate::from_ymd_opt(2026, 4, 4).unwrap(),
        )
        .expect("birthday intent should parse");

        assert_eq!(
            parsed.payload.summary,
            "Create recurring birthday for Deri on June 9, 2003 in your personal calendar"
        );
        match parsed.payload.call.input {
            AssistantToolInput::CalendarCreateBirthday { title, .. } => {
                assert_eq!(title, "Deri birthday");
            }
            other => panic!("expected birthday input, got {other:?}"),
        }
    }

    #[test]
    fn birthday_request_rejects_relationship_only_subjects() {
        let error = parse_birthday_request_for(
            &test_user("user"),
            "Add my brother's birthday on June 9, 2003 to my calendar",
            None,
            NaiveDate::from_ymd_opt(2026, 4, 4).unwrap(),
        )
        .expect_err("relationship-only birthday subject should be rejected");

        assert!(error.contains("couldn't determine whose birthday this is"));
    }

    #[test]
    fn detects_supported_document_create_intent() {
        assert!(is_supported_document_create_intent(
            "create a markdown document about my next event"
        ));
    }

    #[test]
    fn parses_document_request_with_title_and_filename() {
        let parsed = parse_document_request(
            "Create a markdown document called \"LAN setup note\" explaining the local Rustyfin IP and login URL",
            Some("conversation-1"),
            "assistant-model.gguf",
        )
        .expect("document intent should parse");

        assert_eq!(
            parsed.payload.action_kind,
            AssistantPendingActionKind::DocumentCreateDownload
        );
        assert_eq!(
            parsed.payload.summary,
            "Create downloadable markdown document \"LAN-setup-note.md\""
        );
        match parsed.payload.call.input {
            AssistantToolInput::DocumentCreateDownload {
                title,
                file_name,
                format,
                model_name,
                ..
            } => {
                assert_eq!(title, "LAN setup note");
                assert_eq!(file_name, "LAN-setup-note.md");
                assert_eq!(format, "markdown");
                assert_eq!(model_name, "assistant-model.gguf");
            }
            other => panic!("unexpected tool input: {other:?}"),
        }
    }

    #[test]
    fn document_request_rejects_unsupported_format() {
        let error = parse_document_request(
            "Create a PDF report about my next Rustyfin event",
            None,
            "assistant-model.gguf",
        )
        .expect_err("pdf output should be rejected");

        assert!(error.contains("markdown or plain-text"));
    }

    #[test]
    fn document_request_requires_meaningful_content_request() {
        let error =
            parse_document_request("Create a markdown document", None, "assistant-model.gguf")
                .expect_err("empty document request should be rejected");

        assert!(error.contains("need to know what it should contain"));
    }

    #[test]
    fn detects_supported_conversation_manage_intent() {
        assert!(is_supported_conversation_manage_intent(
            "delete the last 5 ai conversations"
        ));
        assert!(is_supported_conversation_manage_intent(
            "archive the first 3 conversations"
        ));
    }

    #[test]
    fn resolves_delete_last_five_conversations_by_recency() {
        let candidates = vec![
            conversation_candidate("a", "Alpha", None, false, 1024, 10),
            conversation_candidate("b", "Bravo", None, false, 2048, 30),
            conversation_candidate("c", "Charlie", None, false, 3072, 20),
            conversation_candidate("d", "Delta", None, false, 4096, 40),
            conversation_candidate("e", "Echo", None, false, 5120, 50),
            conversation_candidate("f", "Foxtrot", None, false, 6144, 60),
        ];

        let selection = resolve_conversation_selection(
            "Delete the last 5 AI conversations",
            None,
            ConversationActionOperation::Delete,
            &candidates,
        )
        .expect("selection should resolve");

        let selected_titles = selection
            .selected
            .iter()
            .map(|candidate| candidate.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            selected_titles,
            vec!["Foxtrot", "Echo", "Delta", "Bravo", "Charlie"]
        );
    }

    #[test]
    fn resolves_archive_first_three_conversations_by_display_order() {
        let candidates = vec![
            conversation_candidate("a", "Alpha", None, false, 4096, 10),
            conversation_candidate("b", "Bravo", None, false, 3072, 20),
            conversation_candidate("c", "Charlie", None, false, 2048, 30),
            conversation_candidate("d", "Delta", None, false, 1024, 40),
        ];

        let selection = resolve_conversation_selection(
            "Archive the first 3 conversations",
            None,
            ConversationActionOperation::Archive,
            &candidates,
        )
        .expect("selection should resolve");

        let selected_titles = selection
            .selected
            .iter()
            .map(|candidate| candidate.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(selected_titles, vec!["Alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn resolves_delete_all_conversations_in_group() {
        let candidates = vec![
            conversation_candidate("a", "Alpha", Some("Trips"), false, 4096, 10),
            conversation_candidate("b", "Bravo", Some("Trips"), true, 3072, 20),
            conversation_candidate("c", "Charlie", Some("Work"), false, 2048, 30),
        ];

        let selection = resolve_conversation_selection(
            "Delete all conversations in group Trips",
            None,
            ConversationActionOperation::Delete,
            &candidates,
        )
        .expect("selection should resolve");

        let selected_titles = selection
            .selected
            .iter()
            .map(|candidate| candidate.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(selected_titles, vec!["Alpha", "Bravo"]);
    }

    #[test]
    fn delete_selection_excludes_active_conversation_and_refills() {
        let candidates = vec![
            conversation_candidate("a", "Alpha", None, false, 1024, 10),
            conversation_candidate("b", "Bravo", None, false, 2048, 20),
            conversation_candidate("c", "Charlie", None, false, 3072, 30),
            conversation_candidate("d", "Delta", None, false, 4096, 40),
        ];

        let selection = resolve_conversation_selection(
            "Delete the last 3 AI conversations",
            Some("d"),
            ConversationActionOperation::Delete,
            &candidates,
        )
        .expect("selection should resolve");

        let selected_titles = selection
            .selected
            .iter()
            .map(|candidate| candidate.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(selected_titles, vec!["Charlie", "Bravo", "Alpha"]);
        assert!(selection.current_excluded);
    }

    fn conversation_candidate(
        id: &str,
        title: &str,
        group_name: Option<&str>,
        archived: bool,
        sort_order: i64,
        updated_ts: i64,
    ) -> ConversationSelectionCandidate {
        ConversationSelectionCandidate {
            id: id.to_string(),
            title: title.to_string(),
            group_name: group_name.map(str::to_string),
            archived,
            sort_order,
            updated_ts,
        }
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
