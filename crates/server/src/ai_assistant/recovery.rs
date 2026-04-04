use super::orchestrator::{
    birthday_calendar_window_input, extract_birthday_query, is_next_birthday_request,
};
use super::outcomes::normalized_args_hash;
use super::provider::ToolExecutionProfile;
use super::registry::AssistantToolName;
use super::types::{
    AssistantClarificationRequest, AssistantExecutionBudget, AssistantExecutionStopReason,
    AssistantExecutionTrace, AssistantRecoveryDecision, AssistantResponseMode, AssistantToolInput,
    AssistantToolOutcome, AssistantToolOutcomeKind, PlannedToolCall,
};

pub fn choose_recovery_step(
    message: &str,
    response_mode: AssistantResponseMode,
    budget: &AssistantExecutionBudget,
    trace: &AssistantExecutionTrace,
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
    profile: &ToolExecutionProfile,
) -> AssistantRecoveryDecision {
    if trace.tool_step_count >= u32::from(budget.max_tool_steps) {
        return AssistantRecoveryDecision::Stop {
            reason: AssistantExecutionStopReason::BudgetExhausted,
        };
    }

    match latest_outcome.kind {
        AssistantToolOutcomeKind::Answer => {
            return AssistantRecoveryDecision::Stop {
                reason: AssistantExecutionStopReason::SufficientAnswer,
            };
        }
        AssistantToolOutcomeKind::ClarificationNeeded | AssistantToolOutcomeKind::Ambiguous => {
            return AssistantRecoveryDecision::AskClarification {
                request: AssistantClarificationRequest {
                    message: latest_outcome.message.clone().unwrap_or_else(|| {
                        "I need one more detail to answer that safely.".to_string()
                    }),
                    missing_field: None,
                },
            };
        }
        AssistantToolOutcomeKind::Denied => {
            return AssistantRecoveryDecision::Stop {
                reason: AssistantExecutionStopReason::AclDenied,
            };
        }
        AssistantToolOutcomeKind::FatalError => {
            return AssistantRecoveryDecision::Stop {
                reason: AssistantExecutionStopReason::FatalError,
            };
        }
        AssistantToolOutcomeKind::Conflicting => {
            return if budget.allow_verifier
                && matches!(response_mode, AssistantResponseMode::Extended)
            {
                AssistantRecoveryDecision::VerifierPass
            } else {
                AssistantRecoveryDecision::Stop {
                    reason: AssistantExecutionStopReason::ConflictUnresolved,
                }
            };
        }
        _ => {}
    }

    let attempts_for_call = trace
        .attempts
        .iter()
        .filter(|attempt| attempt.tool == latest_call.tool.as_str())
        .count();
    if attempts_for_call >= usize::from(budget.max_same_signature_repeats.saturating_add(1)) {
        return AssistantRecoveryDecision::Stop {
            reason: AssistantExecutionStopReason::DuplicateSignature,
        };
    }

    for candidate in candidate_edges(message, latest_call, latest_outcome) {
        if !candidate.tool.recovery_eligible() {
            continue;
        }
        if profile
            .denial_reason(candidate.tool, candidate.tool.spec())
            .is_some()
        {
            continue;
        }
        if trace.alternate_tool_count >= u32::from(budget.max_alternate_steps)
            && candidate.is_alternate
        {
            continue;
        }
        if candidate.recovery_depth > budget.max_recovery_depth {
            continue;
        }
        let candidate_hash = normalized_args_hash(&candidate.call.input);
        if trace.attempts.iter().any(|attempt| {
            attempt.tool == candidate.tool.as_str() && attempt.args_hash == candidate_hash
        }) {
            continue;
        }
        return AssistantRecoveryDecision::RunNext {
            call: candidate.call,
            edge_label: candidate.edge_label,
            recovery_depth: candidate.recovery_depth,
            is_alternate: candidate.is_alternate,
        };
    }

    match latest_outcome.kind {
        AssistantToolOutcomeKind::Partial | AssistantToolOutcomeKind::WeakMatch => {
            AssistantRecoveryDecision::SynthesizeNow
        }
        AssistantToolOutcomeKind::NotFound | AssistantToolOutcomeKind::Empty => {
            AssistantRecoveryDecision::Stop {
                reason: AssistantExecutionStopReason::NoPermittedFallback,
            }
        }
        AssistantToolOutcomeKind::TransientError | AssistantToolOutcomeKind::ValidationFailed => {
            AssistantRecoveryDecision::Stop {
                reason: AssistantExecutionStopReason::NoPermittedFallback,
            }
        }
        _ => AssistantRecoveryDecision::Stop {
            reason: AssistantExecutionStopReason::WeakEvidenceOnly,
        },
    }
}

struct RecoveryCandidate {
    tool: AssistantToolName,
    call: PlannedToolCall,
    edge_label: String,
    recovery_depth: u8,
    is_alternate: bool,
}

fn candidate_edges(
    message: &str,
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
) -> Vec<RecoveryCandidate> {
    let mut edges = Vec::new();
    edges.extend(calendar_edges(message, latest_call, latest_outcome));
    edges.extend(ai_runtime_edges(message, latest_call, latest_outcome));
    edges.extend(weather_edges(latest_call, latest_outcome));
    edges.extend(library_edges(message, latest_call, latest_outcome));
    edges
}

fn calendar_edges(
    message: &str,
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
) -> Vec<RecoveryCandidate> {
    let mut edges = Vec::new();
    match latest_call.tool {
        AssistantToolName::CalendarGetNextEvent => {
            if matches!(latest_outcome.kind, AssistantToolOutcomeKind::Empty)
                && (extract_birthday_query(message).is_some() || is_next_birthday_request(message))
            {
                let input =
                    birthday_calendar_window_input(message, extract_birthday_query(message));
                edges.push(RecoveryCandidate {
                    tool: AssistantToolName::CalendarUpcomingBirthdays,
                    call: PlannedToolCall {
                        tool: AssistantToolName::CalendarUpcomingBirthdays,
                        input,
                    },
                    edge_label: "calendar.next_event_to_birthdays".to_string(),
                    recovery_depth: 1,
                    is_alternate: true,
                });
            }
        }
        AssistantToolName::CalendarUpcomingBirthdays => {
            if matches!(
                latest_outcome.kind,
                AssistantToolOutcomeKind::Empty
                    | AssistantToolOutcomeKind::NotFound
                    | AssistantToolOutcomeKind::ValidationFailed
            ) {
                let recovered_input =
                    birthday_calendar_window_input(message, extract_birthday_query(message));
                if recovered_input != latest_call.input {
                    edges.push(RecoveryCandidate {
                        tool: AssistantToolName::CalendarUpcomingBirthdays,
                        call: PlannedToolCall {
                            tool: AssistantToolName::CalendarUpcomingBirthdays,
                            input: recovered_input,
                        },
                        edge_label: "calendar.birthday_window_repair".to_string(),
                        recovery_depth: 1,
                        is_alternate: false,
                    });
                } else if is_next_birthday_request(message) {
                    edges.push(RecoveryCandidate {
                        tool: AssistantToolName::CalendarGetNextEvent,
                        call: PlannedToolCall {
                            tool: AssistantToolName::CalendarGetNextEvent,
                            input: AssistantToolInput::None,
                        },
                        edge_label: "calendar.birthdays_to_next_event".to_string(),
                        recovery_depth: 1,
                        is_alternate: true,
                    });
                }
            }
        }
        _ => {}
    }
    edges
}

fn ai_runtime_edges(
    message: &str,
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
) -> Vec<RecoveryCandidate> {
    if latest_call.tool == AssistantToolName::SystemGetAiRuntimeSummary {
        return Vec::new();
    }
    if !matches!(
        latest_outcome.kind,
        AssistantToolOutcomeKind::Answer
            | AssistantToolOutcomeKind::Partial
            | AssistantToolOutcomeKind::NotFound
            | AssistantToolOutcomeKind::Empty
    ) {
        return Vec::new();
    }
    let lower = message.to_ascii_lowercase();
    let ai_runtime_intent = [
        "ai model",
        "what model are you",
        "loaded model",
        "backend",
        "scheduler",
        "warm pool",
        "queue depth",
        "runtime",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !ai_runtime_intent {
        return Vec::new();
    }
    vec![RecoveryCandidate {
        tool: AssistantToolName::SystemGetAiRuntimeSummary,
        call: PlannedToolCall {
            tool: AssistantToolName::SystemGetAiRuntimeSummary,
            input: AssistantToolInput::None,
        },
        edge_label: "system.intent_to_ai_runtime".to_string(),
        recovery_depth: 1,
        is_alternate: true,
    }]
}

fn weather_edges(
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
) -> Vec<RecoveryCandidate> {
    let mut edges = Vec::new();
    let current_location = match &latest_call.input {
        AssistantToolInput::Weather { location, .. } => Some(location.as_str()),
        AssistantToolInput::WeatherHistory { location, .. } => Some(location.as_str()),
        _ => None,
    };

    if matches!(
        latest_outcome.kind,
        AssistantToolOutcomeKind::ValidationFailed | AssistantToolOutcomeKind::WeakMatch
    ) {
        if let Some(location) = current_location {
            for variant in weather_location_variants(location)
                .into_iter()
                .skip(1)
                .take(2)
            {
                if variant == location {
                    continue;
                }
                let next_input = match &latest_call.input {
                    AssistantToolInput::Weather { forecast_days, .. } => {
                        AssistantToolInput::Weather {
                            location: variant.clone(),
                            forecast_days: *forecast_days,
                        }
                    }
                    AssistantToolInput::WeatherHistory {
                        start_date,
                        end_date,
                        label,
                        ..
                    } => AssistantToolInput::WeatherHistory {
                        location: variant.clone(),
                        start_date: start_date.clone(),
                        end_date: end_date.clone(),
                        label: label.clone(),
                    },
                    _ => continue,
                };
                edges.push(RecoveryCandidate {
                    tool: latest_call.tool,
                    call: PlannedToolCall {
                        tool: latest_call.tool,
                        input: next_input,
                    },
                    edge_label: "weather.location_variant_retry".to_string(),
                    recovery_depth: 1,
                    is_alternate: false,
                });
            }
        }
    }

    edges
}

fn library_edges(
    message: &str,
    latest_call: &PlannedToolCall,
    latest_outcome: &AssistantToolOutcome,
) -> Vec<RecoveryCandidate> {
    if latest_call.tool != AssistantToolName::LibrarySearchTitles
        || !matches!(latest_outcome.kind, AssistantToolOutcomeKind::Partial)
    {
        return Vec::new();
    }

    let lower = message.to_ascii_lowercase();
    if !["summary", "about", "details", "tell me more", "what is"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return Vec::new();
    }

    let title = latest_outcome
        .block
        .data
        .get("matches")
        .and_then(serde_json::Value::as_array)
        .and_then(|matches| matches.first())
        .and_then(|item| item.get("title"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    title
        .map(|query| RecoveryCandidate {
            tool: AssistantToolName::LibraryGetItemSummary,
            call: PlannedToolCall {
                tool: AssistantToolName::LibraryGetItemSummary,
                input: AssistantToolInput::LibrarySearch { query },
            },
            edge_label: "library.search_to_detail".to_string(),
            recovery_depth: 1,
            is_alternate: true,
        })
        .into_iter()
        .collect()
}

fn weather_location_variants(location: &str) -> Vec<String> {
    let mut variants = Vec::new();
    push_variant(&mut variants, location);
    let normalized_location = strip_weather_location_prefix(location);
    if normalized_location != location {
        push_variant(&mut variants, normalized_location);
    }
    if normalized_location.to_ascii_lowercase().contains(" in ") {
        push_variant(
            &mut variants,
            &replace_case_insensitive(normalized_location, " in ", ", "),
        );
    }
    let tokens = normalized_location
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if !normalized_location.contains(',') && tokens.len() >= 2 {
        push_variant(
            &mut variants,
            &format!("{}, {}", tokens[0], tokens[1..].join(" ")),
        );
        if tokens.len() >= 3 {
            push_variant(
                &mut variants,
                &format!(
                    "{}, {}, {}",
                    tokens[0],
                    tokens[1..tokens.len() - 1].join(" "),
                    tokens[tokens.len() - 1]
                ),
            );
        }
    }
    if let Some(core) = tokens.first().copied().filter(|token| token.len() >= 4) {
        push_variant(&mut variants, core);
    }
    variants
}

fn strip_weather_location_prefix(location: &str) -> &str {
    location
        .strip_prefix("for ")
        .or_else(|| location.strip_prefix("For "))
        .or_else(|| location.strip_prefix("in "))
        .or_else(|| location.strip_prefix("In "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(location)
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if let Some(index) = lower.find(&needle_lower) {
        let end = index + needle.len();
        format!("{}{}{}", &haystack[..index], replacement, &haystack[end..])
    } else {
        haystack.to_string()
    }
}

fn push_variant(variants: &mut Vec<String>, candidate: &str) {
    let normalized = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || variants.iter().any(|value| value == &normalized) {
        return;
    }
    variants.push(normalized);
}

#[cfg(test)]
mod tests {
    use super::choose_recovery_step;
    use crate::ai_assistant::provider::ToolExecutionProfile;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{
        AssistantExecutionBudget, AssistantExecutionTrace, AssistantResponseMode,
        AssistantToolInput, AssistantToolOutcome, AssistantToolOutcomeKind, PlannedToolCall,
    };
    use serde_json::json;

    fn birthday_call(query: Option<&str>) -> PlannedToolCall {
        PlannedToolCall {
            tool: AssistantToolName::CalendarUpcomingBirthdays,
            input: AssistantToolInput::CalendarWindow {
                from_date: "2026-04-04".to_string(),
                to_date: "2026-05-04".to_string(),
                label: "the next 30 days".to_string(),
                query: query.map(str::to_string),
            },
        }
    }

    fn birthday_outcome(kind: AssistantToolOutcomeKind) -> AssistantToolOutcome {
        AssistantToolOutcome {
            tool: "calendar_upcoming_birthdays".to_string(),
            label: "Birthdays".to_string(),
            domain_family: crate::ai_assistant::types::AssistantDomainFamily::Calendar,
            kind,
            confidence: 0.9,
            block: crate::ai_assistant::types::AssistantToolContextBlock {
                tool: "calendar_upcoming_birthdays",
                label: "Birthdays".to_string(),
                status: "ok",
                data: json!({"birthdays":[]}),
            },
            evidence_items: Vec::new(),
            ambiguity_keys: Vec::new(),
            recovery_hints: Vec::new(),
            args_hash: "hash".to_string(),
            result_signature: "sig".to_string(),
            message: None,
            stale: false,
        }
    }

    #[test]
    fn birthday_recovery_repairs_named_window_queries() {
        let decision = choose_recovery_step(
            "When is Rachel's next birthday?",
            AssistantResponseMode::Thinking,
            &AssistantExecutionBudget::for_mode(AssistantResponseMode::Thinking),
            &AssistantExecutionTrace {
                response_mode: AssistantResponseMode::Thinking,
                budget: AssistantExecutionBudget::for_mode(AssistantResponseMode::Thinking),
                planner_mode: Some("deterministic_fallback".to_string()),
                attempts: Vec::new(),
                retained_evidence: Vec::new(),
                stop_reason:
                    crate::ai_assistant::types::AssistantExecutionStopReason::WeakEvidenceOnly,
                final_outcome_kind: None,
                final_answer_path: crate::ai_assistant::types::AssistantSynthesisMode::None,
                planner_pass_count: 1,
                tool_step_count: 1,
                alternate_tool_count: 0,
                recovery_step_count: 0,
                clarification_count: 0,
                conflict_count: 0,
                deterministic_answer_used: false,
                synthesis_used: false,
                used_role_backends: Vec::new(),
                outcome_counts: Default::default(),
            },
            &birthday_call(Some("next")),
            &birthday_outcome(AssistantToolOutcomeKind::NotFound),
            &ToolExecutionProfile::full_access(),
        );
        match decision {
            crate::ai_assistant::types::AssistantRecoveryDecision::RunNext { call, .. } => {
                assert_eq!(call.tool, AssistantToolName::CalendarUpcomingBirthdays);
                assert_ne!(call, birthday_call(Some("next")));
            }
            other => panic!("expected recovery step, got {other:?}"),
        }
    }

    #[test]
    fn write_tools_are_never_selected_as_recovery_targets() {
        let decision = choose_recovery_step(
            "delete the event",
            AssistantResponseMode::Thinking,
            &AssistantExecutionBudget::for_mode(AssistantResponseMode::Thinking),
            &AssistantExecutionTrace {
                response_mode: AssistantResponseMode::Thinking,
                budget: AssistantExecutionBudget::for_mode(AssistantResponseMode::Thinking),
                planner_mode: Some("deterministic_fallback".to_string()),
                attempts: Vec::new(),
                retained_evidence: Vec::new(),
                stop_reason:
                    crate::ai_assistant::types::AssistantExecutionStopReason::WeakEvidenceOnly,
                final_outcome_kind: None,
                final_answer_path: crate::ai_assistant::types::AssistantSynthesisMode::None,
                planner_pass_count: 1,
                tool_step_count: 1,
                alternate_tool_count: 0,
                recovery_step_count: 0,
                clarification_count: 0,
                conflict_count: 0,
                deterministic_answer_used: false,
                synthesis_used: false,
                used_role_backends: Vec::new(),
                outcome_counts: Default::default(),
            },
            &PlannedToolCall {
                tool: AssistantToolName::CalendarDeleteEvent,
                input: AssistantToolInput::None,
            },
            &AssistantToolOutcome {
                tool: "calendar_delete_event".to_string(),
                label: "Delete".to_string(),
                domain_family: crate::ai_assistant::types::AssistantDomainFamily::Calendar,
                kind: AssistantToolOutcomeKind::Denied,
                confidence: 1.0,
                block: crate::ai_assistant::types::AssistantToolContextBlock {
                    tool: "calendar_delete_event",
                    label: "Delete".to_string(),
                    status: "error",
                    data: json!({"message":"denied"}),
                },
                evidence_items: Vec::new(),
                ambiguity_keys: Vec::new(),
                recovery_hints: Vec::new(),
                args_hash: "hash".to_string(),
                result_signature: "sig".to_string(),
                message: Some("denied".to_string()),
                stale: false,
            },
            &ToolExecutionProfile::full_access(),
        );
        assert!(matches!(
            decision,
            crate::ai_assistant::types::AssistantRecoveryDecision::Stop { .. }
        ));
    }

    #[test]
    fn weather_weak_match_retries_with_sanitized_location_variant() {
        let decision = choose_recovery_step(
            "for Campile, Ireland?",
            AssistantResponseMode::Extended,
            &AssistantExecutionBudget::for_mode(AssistantResponseMode::Extended),
            &AssistantExecutionTrace {
                response_mode: AssistantResponseMode::Extended,
                budget: AssistantExecutionBudget::for_mode(AssistantResponseMode::Extended),
                planner_mode: Some("deterministic_fallback".to_string()),
                attempts: Vec::new(),
                retained_evidence: Vec::new(),
                stop_reason:
                    crate::ai_assistant::types::AssistantExecutionStopReason::WeakEvidenceOnly,
                final_outcome_kind: None,
                final_answer_path: crate::ai_assistant::types::AssistantSynthesisMode::None,
                planner_pass_count: 1,
                tool_step_count: 1,
                alternate_tool_count: 0,
                recovery_step_count: 0,
                clarification_count: 0,
                conflict_count: 0,
                deterministic_answer_used: false,
                synthesis_used: false,
                used_role_backends: Vec::new(),
                outcome_counts: Default::default(),
            },
            &PlannedToolCall {
                tool: AssistantToolName::WeatherGetCurrent,
                input: AssistantToolInput::Weather {
                    location: "for Campile, Ireland".to_string(),
                    forecast_days: None,
                },
            },
            &AssistantToolOutcome {
                tool: "weather_get_current".to_string(),
                label: "Weather".to_string(),
                domain_family: crate::ai_assistant::types::AssistantDomainFamily::Weather,
                kind: AssistantToolOutcomeKind::WeakMatch,
                confidence: 0.9,
                block: crate::ai_assistant::types::AssistantToolContextBlock {
                    tool: "weather_get_current",
                    label: "Weather".to_string(),
                    status: "ok",
                    data: json!({"resolved_location":"For, Blue Nile State, Sudan"}),
                },
                evidence_items: Vec::new(),
                ambiguity_keys: Vec::new(),
                recovery_hints: vec!["normalize_location".to_string()],
                args_hash: "hash".to_string(),
                result_signature: "sig".to_string(),
                message: Some("weak match".to_string()),
                stale: false,
            },
            &ToolExecutionProfile::full_access(),
        );
        match decision {
            crate::ai_assistant::types::AssistantRecoveryDecision::RunNext { call, .. } => {
                match call.input {
                    AssistantToolInput::Weather { location, .. } => {
                        assert_eq!(location, "Campile, Ireland");
                    }
                    other => panic!("expected weather input, got {other:?}"),
                }
            }
            other => panic!("expected recovery step, got {other:?}"),
        }
    }
}
