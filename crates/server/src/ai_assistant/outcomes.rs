use sha2::{Digest, Sha256};

use super::registry::AssistantToolName;
use super::replies::compact_text;
use super::types::{
    AssistantDomainFamily, AssistantEvidenceItem, AssistantToolContextBlock, AssistantToolInput,
    AssistantToolOutcome, AssistantToolOutcomeKind, decode_assistant_clarification_message,
};

pub fn normalized_args_hash(input: &AssistantToolInput) -> String {
    let json = serde_json::to_vec(input).unwrap_or_default();
    short_hash(&json)
}

pub fn salient_result_signature(block: &AssistantToolContextBlock) -> String {
    let json = serde_json::to_vec(&block.data).unwrap_or_default();
    short_hash(&json)
}

pub fn normalize_tool_result(
    message: &str,
    call: &super::types::PlannedToolCall,
    block: AssistantToolContextBlock,
) -> AssistantToolOutcome {
    let args_hash = normalized_args_hash(&call.input);
    let result_signature = salient_result_signature(&block);
    let domain_family = call.tool.domain_family();
    let inspector = if block.status == "error" {
        classify_error_outcome(call.tool, &block)
    } else {
        classify_ok_outcome(message, call.tool, &block)
    };
    let evidence_items = extract_evidence_items(call.tool, &block, inspector.kind);

    AssistantToolOutcome {
        tool: call.tool.as_str().to_string(),
        label: block.label.clone(),
        domain_family,
        kind: inspector.kind,
        confidence: inspector.confidence,
        block,
        evidence_items,
        ambiguity_keys: inspector.ambiguity_keys,
        recovery_hints: inspector.recovery_hints,
        args_hash,
        result_signature,
        message: inspector.message,
        stale: inspector.stale,
    }
}

#[derive(Default)]
struct OutcomeInspection {
    kind: AssistantToolOutcomeKind,
    confidence: f32,
    ambiguity_keys: Vec<String>,
    recovery_hints: Vec<String>,
    message: Option<String>,
    stale: bool,
}

fn classify_error_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    let mut inspection = OutcomeInspection {
        kind: AssistantToolOutcomeKind::FatalError,
        confidence: 0.95,
        message: tool_message(block),
        ..OutcomeInspection::default()
    };
    let lower = inspection
        .message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if let Some(clarification) = inspection
        .message
        .as_deref()
        .and_then(decode_assistant_clarification_message)
    {
        inspection.kind = if clarification.to_ascii_lowercase().contains("which one") {
            AssistantToolOutcomeKind::Ambiguous
        } else {
            AssistantToolOutcomeKind::ClarificationNeeded
        };
        inspection.message = Some(clarification.to_string());
        inspection.confidence = 0.99;
        return inspection;
    }

    if lower.contains("multiple") && lower.contains("which one") {
        inspection.kind = AssistantToolOutcomeKind::Ambiguous;
        inspection.confidence = 0.98;
        return inspection;
    }

    if lower.contains("not available in this assistant execution profile")
        || lower.contains("read-only")
        || lower.contains("requires admin")
        || lower.contains("admin-only")
        || lower.contains("blocked")
    {
        inspection.kind = AssistantToolOutcomeKind::Denied;
        inspection.confidence = 0.99;
        return inspection;
    }

    if lower.contains("confirmation token") || lower.contains("confirmation") {
        inspection.kind = AssistantToolOutcomeKind::Denied;
        inspection.confidence = 0.99;
        return inspection;
    }

    if lower.contains("invalid")
        || lower.contains("required")
        || lower.contains("missing")
        || lower.contains("must be")
        || lower.contains("validation failed")
    {
        inspection.kind = AssistantToolOutcomeKind::ValidationFailed;
        inspection.confidence = 0.96;
        return inspection;
    }

    if lower.contains("timed out")
        || lower.contains("temporarily")
        || lower.contains("try again")
        || lower.contains("unavailable")
    {
        inspection.kind = AssistantToolOutcomeKind::TransientError;
        inspection.confidence = 0.90;
        return inspection;
    }

    if lower.contains("no accessible")
        || lower.contains("no public weather location matched")
        || lower.contains("couldn't find")
        || lower.contains("not found")
        || lower.contains("no visible")
    {
        inspection.kind = outcome_kind_for_not_found(tool);
        inspection.confidence = 0.96;
    }

    inspection
}

fn classify_ok_outcome(
    message: &str,
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    if let Some(clarification) = tool_message(block)
        .as_deref()
        .and_then(decode_assistant_clarification_message)
    {
        return OutcomeInspection {
            kind: if clarification.to_ascii_lowercase().contains("which one") {
                AssistantToolOutcomeKind::Ambiguous
            } else {
                AssistantToolOutcomeKind::ClarificationNeeded
            },
            confidence: 0.99,
            message: Some(clarification.to_string()),
            ..OutcomeInspection::default()
        };
    }

    if tool == AssistantToolName::SystemGetHostRuntimeSummary && ai_runtime_intent(message) {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::Partial,
            confidence: 0.88,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            recovery_hints: vec!["ai_runtime_summary".to_string()],
            message: tool_message(block),
            ..OutcomeInspection::default()
        };
    }

    match tool.domain_family() {
        AssistantDomainFamily::Calendar => classify_calendar_outcome(message, tool, block),
        AssistantDomainFamily::Weather => classify_weather_outcome(block),
        AssistantDomainFamily::Downloads => classify_downloads_outcome(tool, block),
        AssistantDomainFamily::Library => classify_library_outcome(message, tool, block),
        AssistantDomainFamily::Transcript => classify_transcript_outcome(block),
        AssistantDomainFamily::Rooms => classify_rooms_outcome(tool, block),
        AssistantDomainFamily::Servers => classify_servers_outcome(tool, block),
        AssistantDomainFamily::AiRuntime => OutcomeInspection {
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 0.99,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            message: tool_message(block),
            ..OutcomeInspection::default()
        },
        AssistantDomainFamily::Network => classify_network_outcome(tool, block),
        AssistantDomainFamily::Memory => classify_memory_outcome(tool, block),
        AssistantDomainFamily::System => classify_system_outcome(tool, block),
        _ => classify_generic_outcome(block),
    }
}

fn ai_runtime_intent(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "ai model",
        "what model are you",
        "loaded model",
        "ai runtime",
        "backend",
        "scheduler",
        "warm pool",
        "queue depth",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn classify_calendar_outcome(
    _message: &str,
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::CalendarGetNextEvent => {
            if block.data.get("next_event").is_none()
                || block
                    .data
                    .get("next_event")
                    .is_some_and(|value| value.is_null())
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Empty,
                    confidence: 0.99,
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.99,
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::CalendarUpcomingBirthdays => {
            let birthdays = block
                .data
                .get("birthdays")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if birthdays == 0 {
                OutcomeInspection {
                    kind: if block
                        .data
                        .get("query")
                        .and_then(serde_json::Value::as_str)
                        .is_some()
                    {
                        AssistantToolOutcomeKind::NotFound
                    } else {
                        AssistantToolOutcomeKind::Empty
                    },
                    confidence: 0.99,
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.99,
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::CalendarGetEventDetails => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no visible")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::CalendarListEvents => {
            let events = block
                .data
                .get("events")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if events == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::CalendarCountEvents => {
            let total = block
                .data
                .get("total_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if total == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::CalendarListBusyDays => {
            let busy_days = block
                .data
                .get("busy_days")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if busy_days == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                ..OutcomeInspection::default()
            }
        }
        _ => classify_generic_outcome(block),
    }
}

fn classify_weather_outcome(block: &AssistantToolContextBlock) -> OutcomeInspection {
    let message = tool_message(block);
    let lower = message.as_deref().unwrap_or_default().to_ascii_lowercase();
    if lower.contains("multiple locations matching") {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::Ambiguous,
            confidence: 0.99,
            message,
            ..OutcomeInspection::default()
        };
    }
    if lower.contains("no public weather location matched") {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::ValidationFailed,
            confidence: 0.98,
            message,
            recovery_hints: vec!["normalize_location".to_string()],
            ..OutcomeInspection::default()
        };
    }
    if weather_resolution_looks_weak(&block.data) {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::WeakMatch,
            confidence: 0.90,
            message,
            recovery_hints: vec!["normalize_location".to_string()],
            ..OutcomeInspection::default()
        };
    }
    if block.tool == "weather_get_forecast"
        && block
            .data
            .get("forecast_days")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::Empty,
            confidence: 0.99,
            ..OutcomeInspection::default()
        };
    }
    if block.tool == "weather_get_hourly_window"
        && block
            .data
            .get("hourly_points")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::Empty,
            confidence: 0.99,
            ..OutcomeInspection::default()
        };
    }
    OutcomeInspection {
        kind: AssistantToolOutcomeKind::Answer,
        confidence: 0.98,
        stale: true,
        message,
        ..OutcomeInspection::default()
    }
}

fn weather_resolution_looks_weak(data: &serde_json::Value) -> bool {
    let query = data
        .get("location_query")
        .and_then(serde_json::Value::as_str)
        .map(strip_weather_prefixes)
        .filter(|value| !value.is_empty());
    let resolved = data
        .get("resolved_location")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(query) = query else {
        return false;
    };
    let Some(resolved) = resolved else {
        return false;
    };

    let primary_query = primary_weather_segment(query);
    let primary_resolved = primary_weather_segment(resolved);
    if primary_query.is_empty() || primary_resolved.is_empty() {
        return false;
    }

    let primary_query_text = normalize_weather_text(primary_query);
    let primary_resolved_text = normalize_weather_text(primary_resolved);
    if primary_query_text.is_empty() || primary_resolved_text.is_empty() {
        return false;
    }
    if primary_resolved_text.contains(&primary_query_text)
        || primary_query_text.contains(&primary_resolved_text)
    {
        return false;
    }

    let query_tokens = significant_weather_tokens(query);
    if query_tokens.len() < 2 {
        return false;
    }
    let resolved_tokens = significant_weather_tokens(resolved);
    let overlap = query_tokens
        .iter()
        .filter(|token| resolved_tokens.contains(*token))
        .count();

    overlap == 0
}

fn strip_weather_prefixes(value: &str) -> &str {
    value
        .trim()
        .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        .strip_prefix("for ")
        .or_else(|| value.trim().strip_prefix("For "))
        .or_else(|| value.trim().strip_prefix("in "))
        .or_else(|| value.trim().strip_prefix("In "))
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
        .unwrap_or_else(|| {
            value
                .trim()
                .trim_matches(|ch: char| ['"', '\'', '(', ')', ',', '.', '?', '!'].contains(&ch))
        })
}

fn primary_weather_segment(value: &str) -> &str {
    value
        .split(',')
        .next()
        .map(str::trim)
        .unwrap_or(value)
        .split(" in ")
        .next()
        .map(str::trim)
        .unwrap_or(value)
}

fn normalize_weather_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn significant_weather_tokens(value: &str) -> Vec<String> {
    normalize_weather_text(value)
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .filter(|token| {
            !matches!(
                *token,
                "for" | "the" | "and" | "county" | "state" | "province"
            )
        })
        .map(str::to_string)
        .collect()
}

fn classify_library_outcome(
    message: &str,
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::LibrarySearchTitles => {
            let matches = block
                .data
                .get("matches")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            if matches == 0 {
                return OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.99,
                    ..OutcomeInspection::default()
                };
            }
            let lower = message.to_ascii_lowercase();
            let wants_detail = ["summary", "about", "details", "tell me more", "what is"]
                .iter()
                .any(|needle| lower.contains(needle));
            OutcomeInspection {
                kind: if matches == 1 && wants_detail {
                    AssistantToolOutcomeKind::Partial
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                recovery_hints: if matches == 1 && wants_detail {
                    vec!["library_detail".to_string()]
                } else {
                    Vec::new()
                },
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::LibrariesGetLibrarySummary => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no accessible library matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::LibraryGetItemSummary
        | AssistantToolName::LibraryGetItemMediaDetails => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no accessible library item matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::LibrariesGetRecentlyAdded
        | AssistantToolName::LibrariesListAccessible => {
            classify_generic_list_outcome(block, "items")
        }
        AssistantToolName::LibrariesFindDuplicateTitles => {
            let duplicates = block
                .data
                .get("duplicates")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if duplicates == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::LibrariesListMissingMetadata => {
            let items = block
                .data
                .get("items")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if items == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.97,
                ..OutcomeInspection::default()
            }
        }
        _ => classify_generic_outcome(block),
    }
}

fn classify_downloads_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::DownloadsListAvailableArtifacts => {
            classify_generic_list_outcome(block, "artifacts")
        }
        AssistantToolName::DownloadsGetArtifactDetails
        | AssistantToolName::DownloadsGetArtifactChecksum
        | AssistantToolName::DownloadsGetArtifactInstallSteps
        | AssistantToolName::DownloadsGetArtifactCompatibility => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no download artifact matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    ..OutcomeInspection::default()
                }
            }
        }
        _ => classify_generic_outcome(block),
    }
}

fn classify_network_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::NetworkGetDefaultRoute => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no default route matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::NetworkGetHostnameAliases => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no hostname aliases matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::NetworkGetDnsServers => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no dns servers matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::NetworkGetInterfaceDetails
        | AssistantToolName::NetworkGetInterfaceByIp => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no network interface matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: true,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::NetworkGetTopologySummary => OutcomeInspection {
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 0.97,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            message: tool_message(block),
            ..OutcomeInspection::default()
        },
        AssistantToolName::NetworkGetRouteTable
        | AssistantToolName::NetworkGetActiveConnections
        | AssistantToolName::NetworkGetInterfaceCounters
        | AssistantToolName::NetworkGetWifiStatus
        | AssistantToolName::NetworkGetVpnStatus => OutcomeInspection {
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 0.97,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            message: tool_message(block),
            ..OutcomeInspection::default()
        },
        _ => classify_generic_outcome(block),
    }
}

fn classify_system_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::SystemGetStoragePathDetail => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no storage path matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: true,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetMountDetail => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no storage mount matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: true,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetPortConflicts => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no listening sockets matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetPortConflictDetail => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no port conflict matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: true,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetFailedUnits => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no failed systemd units were found") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Empty,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else if lower.contains("no failed systemd units matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetFailedUnitDetail => {
            let lower = tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.contains("no failed systemd unit matched") {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: tool.freshness_sensitive() && !block.data.is_null(),
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetServiceDetail => {
            if tool_message(block)
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("no service component matched")
            {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::NotFound,
                    confidence: 0.98,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            } else {
                OutcomeInspection {
                    kind: AssistantToolOutcomeKind::Answer,
                    confidence: 0.98,
                    stale: true,
                    message: tool_message(block),
                    ..OutcomeInspection::default()
                }
            }
        }
        AssistantToolName::SystemGetServiceHealth
        | AssistantToolName::SystemGetBackupSummary
        | AssistantToolName::SystemGetTranscodeSummary
        | AssistantToolName::SystemGetStorageSummary
        | AssistantToolName::SystemGetRecentErrors
        | AssistantToolName::SystemGetHostRuntimeSummary => OutcomeInspection {
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 0.97,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            message: tool_message(block),
            ..OutcomeInspection::default()
        },
        AssistantToolName::SystemGetKernelInfo
        | AssistantToolName::SystemGetCpuTopology
        | AssistantToolName::SystemGetTemperatureSensors
        | AssistantToolName::SystemGetBlockDeviceInventory
        | AssistantToolName::SystemGetFilesystemTable
        | AssistantToolName::SystemGetGpuInventory
        | AssistantToolName::SystemGetPciDevices
        | AssistantToolName::SystemGetUsbDevices
        | AssistantToolName::SystemGetBootLogSummary
        | AssistantToolName::SystemGetJournalSummary => OutcomeInspection {
            kind: AssistantToolOutcomeKind::Answer,
            confidence: 0.97,
            stale: tool.freshness_sensitive() && !block.data.is_null(),
            message: tool_message(block),
            ..OutcomeInspection::default()
        },
        _ => classify_generic_outcome(block),
    }
}

fn classify_transcript_outcome(block: &AssistantToolContextBlock) -> OutcomeInspection {
    let message = tool_message(block);
    if message
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("no accessible")
    {
        return OutcomeInspection {
            kind: AssistantToolOutcomeKind::NotFound,
            confidence: 0.97,
            message,
            ..OutcomeInspection::default()
        };
    }
    OutcomeInspection {
        kind: AssistantToolOutcomeKind::Answer,
        confidence: 0.96,
        ..OutcomeInspection::default()
    }
}

fn classify_rooms_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::RoomsListActive | AssistantToolName::RoomsListJoinable => {
            classify_generic_list_outcome(block, "rooms")
        }
        AssistantToolName::RoomsGetRoomSummary => classify_generic_outcome(block),
        _ => classify_generic_outcome(block),
    }
}

fn classify_servers_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::ServersListMinecraftStatus => {
            classify_generic_list_outcome(block, "servers")
        }
        AssistantToolName::ServersGetMinecraftServerSummary => classify_generic_outcome(block),
        _ => classify_generic_outcome(block),
    }
}

fn classify_memory_outcome(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
) -> OutcomeInspection {
    match tool {
        AssistantToolName::MemoryListRecentFacts | AssistantToolName::MemorySearchFacts => {
            classify_generic_list_outcome(block, "facts")
        }
        AssistantToolName::MemoryListRecentEntities | AssistantToolName::MemorySearchEntities => {
            classify_generic_list_outcome(block, "entities")
        }
        AssistantToolName::MemoryListRecentChanges => {
            let facts = block
                .data
                .get("facts")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let entities = block
                .data
                .get("entities")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if facts == 0 && entities == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.95,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::MemoryListConflictingFacts => {
            let conflicts = block
                .data
                .get("conflicts")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            OutcomeInspection {
                kind: if conflicts == 0 {
                    AssistantToolOutcomeKind::Empty
                } else {
                    AssistantToolOutcomeKind::Answer
                },
                confidence: 0.95,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::MemoryGetEntityProvenance => {
            let entity_present = block
                .data
                .get("entity")
                .is_some_and(|value| !value.is_null());
            let source_present = block
                .data
                .get("source_chunk")
                .is_some_and(|value| !value.is_null());
            OutcomeInspection {
                kind: if entity_present || source_present {
                    AssistantToolOutcomeKind::Answer
                } else {
                    AssistantToolOutcomeKind::Empty
                },
                confidence: 0.95,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::MemoryGetPersonSummary => {
            let person_present = block
                .data
                .get("person")
                .is_some_and(|value| !value.is_null());
            OutcomeInspection {
                kind: if person_present {
                    AssistantToolOutcomeKind::Answer
                } else {
                    AssistantToolOutcomeKind::Empty
                },
                confidence: 0.95,
                ..OutcomeInspection::default()
            }
        }
        AssistantToolName::MemoryGetEntityRelations => {
            classify_generic_list_outcome(block, "relations")
        }
        _ => classify_generic_outcome(block),
    }
}

fn classify_generic_list_outcome(
    block: &AssistantToolContextBlock,
    array_key: &str,
) -> OutcomeInspection {
    let count = block
        .data
        .get(array_key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    OutcomeInspection {
        kind: if count == 0 {
            AssistantToolOutcomeKind::Empty
        } else {
            AssistantToolOutcomeKind::Answer
        },
        confidence: 0.95,
        ..OutcomeInspection::default()
    }
}

fn classify_generic_outcome(block: &AssistantToolContextBlock) -> OutcomeInspection {
    let message = tool_message(block);
    if let Some(message) = message.as_deref() {
        let lower = message.to_ascii_lowercase();
        if lower.contains("not found") || lower.contains("couldn't find") {
            return OutcomeInspection {
                kind: AssistantToolOutcomeKind::NotFound,
                confidence: 0.95,
                message: Some(message.to_string()),
                ..OutcomeInspection::default()
            };
        }
        if lower.contains("multiple") && lower.contains("which one") {
            return OutcomeInspection {
                kind: AssistantToolOutcomeKind::Ambiguous,
                confidence: 0.95,
                message: Some(message.to_string()),
                ..OutcomeInspection::default()
            };
        }
    }
    OutcomeInspection {
        kind: AssistantToolOutcomeKind::Answer,
        confidence: 0.90,
        message,
        ..OutcomeInspection::default()
    }
}

fn extract_evidence_items(
    tool: AssistantToolName,
    block: &AssistantToolContextBlock,
    kind: AssistantToolOutcomeKind,
) -> Vec<AssistantEvidenceItem> {
    let mut items = Vec::new();
    let base_excerpt = summarize_value(&block.data);
    if !base_excerpt.is_empty() {
        items.push(AssistantEvidenceItem {
            id: format!(
                "ev:{}:{}",
                tool.as_str(),
                short_hash(base_excerpt.as_bytes())
            ),
            tool: tool.as_str().to_string(),
            domain_family: tool.domain_family(),
            title: block.label.clone(),
            excerpt: base_excerpt,
            score: evidence_score(kind),
            tags: vec![kind.as_str().to_string()],
            source_chunk_id: None,
            freshness_hint: if tool.freshness_sensitive() {
                Some("live".to_string())
            } else {
                None
            },
            conflict_key: None,
        });
    }

    items.truncate(2);
    items
}

fn evidence_score(kind: AssistantToolOutcomeKind) -> f64 {
    match kind {
        AssistantToolOutcomeKind::Answer => 1.0,
        AssistantToolOutcomeKind::Partial => 0.8,
        AssistantToolOutcomeKind::Ambiguous
        | AssistantToolOutcomeKind::ClarificationNeeded
        | AssistantToolOutcomeKind::WeakMatch => 0.6,
        AssistantToolOutcomeKind::NotFound | AssistantToolOutcomeKind::Empty => 0.5,
        AssistantToolOutcomeKind::Stale => 0.4,
        AssistantToolOutcomeKind::Conflicting => 0.3,
        AssistantToolOutcomeKind::Denied
        | AssistantToolOutcomeKind::ValidationFailed
        | AssistantToolOutcomeKind::TransientError
        | AssistantToolOutcomeKind::FatalError => 0.2,
    }
}

fn summarize_value(value: &serde_json::Value) -> String {
    if let Some(message) = value.get("message").and_then(serde_json::Value::as_str) {
        return compact_text(message, 220);
    }
    compact_text(&serde_json::to_string(value).unwrap_or_default(), 220)
}

fn tool_message(block: &AssistantToolContextBlock) -> Option<String> {
    block
        .data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn outcome_kind_for_not_found(tool: AssistantToolName) -> AssistantToolOutcomeKind {
    match tool.domain_family() {
        AssistantDomainFamily::Weather => AssistantToolOutcomeKind::ValidationFailed,
        _ => AssistantToolOutcomeKind::NotFound,
    }
}

fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::normalize_tool_result;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{
        AssistantToolContextBlock, AssistantToolInput, PlannedToolCall,
    };
    use serde_json::json;

    #[test]
    fn normalizes_empty_birthday_payload_as_not_found() {
        let outcome = normalize_tool_result(
            "When is Rachel's birthday?",
            &PlannedToolCall {
                tool: AssistantToolName::CalendarUpcomingBirthdays,
                input: AssistantToolInput::CalendarWindow {
                    from_date: "2026-04-04".to_string(),
                    to_date: "2027-04-04".to_string(),
                    label: "the next year".to_string(),
                    query: Some("Rachel".to_string()),
                },
            },
            AssistantToolContextBlock {
                tool: "calendar_upcoming_birthdays",
                label: "Birthdays".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "birthdays": []
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::NotFound
        );
    }

    #[test]
    fn normalizes_weather_clarification_as_ambiguous() {
        let outcome = normalize_tool_result(
            "What's the weather tomorrow in Cork?",
            &PlannedToolCall {
                tool: AssistantToolName::WeatherGetForecast,
                input: AssistantToolInput::Weather {
                    location: "Cork".to_string(),
                    forecast_days: Some(2),
                },
            },
            AssistantToolContextBlock {
                tool: "weather_get_forecast",
                label: "Forecast".to_string(),
                status: "ok",
                data: json!({
                    "message": "clarification:I found multiple locations matching \"Cork\". Which one did you mean?"
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Ambiguous
        );
        assert!(
            outcome
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("Which one did you mean")
        );
    }

    #[test]
    fn normalizes_denied_tool_errors() {
        let outcome = normalize_tool_result(
            "Delete my event",
            &PlannedToolCall {
                tool: AssistantToolName::CalendarDeleteEvent,
                input: AssistantToolInput::None,
            },
            AssistantToolContextBlock {
                tool: "calendar_delete_event",
                label: "Delete".to_string(),
                status: "error",
                data: json!({
                    "message": "calendar_delete_event is blocked because this assistant execution profile is read-only."
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Denied
        );
    }

    #[test]
    fn normalizes_host_runtime_for_ai_model_question_as_partial() {
        let outcome = normalize_tool_result(
            "What AI model is loaded?",
            &PlannedToolCall {
                tool: AssistantToolName::SystemGetHostRuntimeSummary,
                input: AssistantToolInput::None,
            },
            AssistantToolContextBlock {
                tool: "system_get_host_runtime_summary",
                label: "Current Rustyfin host runtime summary".to_string(),
                status: "ok",
                data: json!({
                    "cpu_usage_percent": 11.4,
                    "memory_used_bytes": 11000000000_u64,
                    "memory_total_bytes": 33000000000_u64
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Partial
        );
    }

    #[test]
    fn normalizes_wrong_weather_geocode_as_weak_match() {
        let outcome = normalize_tool_result(
            "for Campile, Ireland?",
            &PlannedToolCall {
                tool: AssistantToolName::WeatherGetCurrent,
                input: AssistantToolInput::Weather {
                    location: "for Campile, Ireland".to_string(),
                    forecast_days: None,
                },
            },
            AssistantToolContextBlock {
                tool: "weather_get_current",
                label: "Current weather for For, Blue Nile State, Sudan".to_string(),
                status: "ok",
                data: json!({
                    "location_query": "for Campile, Ireland",
                    "resolved_location": "For, Blue Nile State, Sudan",
                    "condition": "Partly cloudy",
                    "temperature_c": 36.4
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::WeakMatch
        );
        assert!(
            outcome
                .recovery_hints
                .iter()
                .any(|hint| hint == "normalize_location")
        );
    }

    #[test]
    fn normalizes_recent_changes_as_answer() {
        let outcome = normalize_tool_result(
            "What's new in my memory?",
            &PlannedToolCall {
                tool: AssistantToolName::MemoryListRecentChanges,
                input: AssistantToolInput::None,
            },
            AssistantToolContextBlock {
                tool: "memory_list_recent_changes",
                label: "Recent stored memory changes".to_string(),
                status: "ok",
                data: json!({
                    "query": null,
                    "fact_count": 1,
                    "entity_count": 1,
                    "facts": [
                        {
                            "id": "fact-1",
                            "memory_key": "fact-1",
                            "memory_type": "user_memory",
                            "topic_key": "memory:people",
                            "title": "favorite color",
                            "content": "Dark green",
                            "weight": 1.0,
                            "created_ts": 1000,
                            "updated_ts": 1000
                        }
                    ],
                    "entities": [
                        {
                            "id": "entity-1",
                            "node_key": "person:rachel",
                            "entity_kind": "person",
                            "label": "Rachel",
                            "identifier": "rachel",
                            "topic_key": "memory:people",
                            "source_chunk_id": null,
                            "access_scope": "user",
                            "ordinal": 1,
                            "created_ts": 3000,
                            "updated_ts": 3000
                        }
                    ]
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Answer
        );
    }

    #[test]
    fn normalizes_conflicting_facts_as_empty_when_none_found() {
        let outcome = normalize_tool_result(
            "What conflicting facts do you have about Rachel?",
            &PlannedToolCall {
                tool: AssistantToolName::MemoryListConflictingFacts,
                input: AssistantToolInput::SystemService {
                    query: "Rachel".to_string(),
                },
            },
            AssistantToolContextBlock {
                tool: "memory_list_conflicting_facts",
                label: "Conflicting stored memory facts".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "total_count": 0,
                    "conflict_group_count": 0,
                    "conflicts": []
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Empty
        );
    }

    #[test]
    fn normalizes_entity_provenance_as_answer() {
        let outcome = normalize_tool_result(
            "Where did you learn about Rachel?",
            &PlannedToolCall {
                tool: AssistantToolName::MemoryGetEntityProvenance,
                input: AssistantToolInput::SystemService {
                    query: "Rachel".to_string(),
                },
            },
            AssistantToolContextBlock {
                tool: "memory_get_entity_provenance",
                label: "Stored entity provenance for Rachel".to_string(),
                status: "ok",
                data: json!({
                    "query": "Rachel",
                    "matched_by": "exact entity search",
                    "entity": {
                        "id": "entity-1",
                        "node_key": "person:rachel",
                        "conversation_id": "conv-1",
                        "turn_id": "turn-1",
                        "entity_kind": "person",
                        "label": "Rachel",
                        "identifier": "rachel",
                        "topic_key": "memory:people",
                        "source_chunk_id": "chunk-1",
                        "access_scope": "user",
                        "ordinal": 1,
                        "created_ts": 3000,
                        "updated_ts": 4000
                    },
                    "source_chunk": {
                        "chunk_key": "chunk-1",
                        "source_kind": "conversation",
                        "source_id": "conv-1",
                        "source_sub_id": "turn-1",
                        "owner_user_id": "user-1",
                        "access_scope": "user",
                        "access_key": null,
                        "topic_key": "memory:people",
                        "title": "Rachel family note",
                        "excerpt": "Rachel is my sister.",
                        "source_ts": 3000,
                        "updated_ts": 4000
                    }
                }),
            },
        );
        assert_eq!(
            outcome.kind,
            crate::ai_assistant::types::AssistantToolOutcomeKind::Answer
        );
    }
}
