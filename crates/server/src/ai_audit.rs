use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::ai_role_routing::AiRoleRoutingDecision;

pub const DEFAULT_AI_AUDIT_RETENTION_DAYS: i64 = 30;
pub const MIN_AI_AUDIT_RETENTION_DAYS: i64 = 1;
pub const MAX_AI_AUDIT_RETENTION_DAYS: i64 = 365;
pub const AI_AUDIT_RETENTION_DAYS_ENV: &str = "RUSTFIN_AI_AUDIT_RETENTION_DAYS";
pub const AI_AUDIT_PRUNE_INTERVAL_SECS: u64 = 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAssistantAuditGroundingSource {
    pub tool: String,
    pub label: String,
    pub access_mode: String,
    pub risk_tier: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAssistantAuditToolExecution {
    pub tool: String,
    pub input_summary: String,
    pub status: String,
    pub label: String,
    pub result_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiGroundingVisibility {
    User,
    Shared,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGroundingCitation {
    pub citation_id: String,
    pub source_kind: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_ts_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ts_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGroundingChunk {
    pub id: String,
    pub source_kind: String,
    pub title: String,
    pub excerpt: String,
    pub score: f64,
    pub visibility: AiGroundingVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<AiGroundingCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAssistantAuditEventResponse {
    pub id: String,
    pub trace_id: String,
    pub user_id: String,
    pub username: String,
    pub user_role: String,
    pub model_name: String,
    pub message_preview: String,
    pub history_len: i64,
    pub response_kind: String,
    pub planner: serde_json::Value,
    pub model_routing: Vec<AiRoleRoutingDecision>,
    pub planned_tools: Vec<String>,
    pub executed_tools: Vec<AiAssistantAuditToolExecution>,
    pub grounding_chunks: Vec<AiGroundingChunk>,
    pub grounding_sources: Vec<AiAssistantAuditGroundingSource>,
    pub error_message: Option<String>,
    pub created_ts: i64,
}

pub fn parse_audit_event_row(
    row: rustfin_db::repo::ai_assistant_audit::AiAssistantAuditEventRow,
) -> AiAssistantAuditEventResponse {
    AiAssistantAuditEventResponse {
        id: row.id,
        trace_id: row.trace_id,
        user_id: row.user_id,
        username: row.username,
        user_role: row.user_role,
        model_name: row.model_name,
        message_preview: row.message_preview,
        history_len: row.history_len,
        response_kind: row.response_kind,
        planner: serde_json::from_str(&row.planner_json).unwrap_or_else(|_| serde_json::json!({})),
        model_routing: serde_json::from_str(&row.model_routing_json).unwrap_or_default(),
        planned_tools: serde_json::from_str(&row.planned_tools_json).unwrap_or_default(),
        executed_tools: serde_json::from_str(&row.executed_tools_json).unwrap_or_default(),
        grounding_chunks: serde_json::from_str(&row.grounding_chunks_json).unwrap_or_default(),
        grounding_sources: serde_json::from_str(&row.grounding_sources_json).unwrap_or_default(),
        error_message: row.error_message,
        created_ts: row.created_ts,
    }
}

pub fn normalize_message_preview(message: &str) -> String {
    const MAX_CHARS: usize = 280;

    let normalized = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return "(empty message)".to_string();
    }

    let mut preview = String::new();
    for (index, ch) in normalized.chars().enumerate() {
        if index >= MAX_CHARS {
            preview.push_str("...");
            return preview;
        }
        preview.push(ch);
    }
    preview
}

pub fn audit_retention_days() -> i64 {
    match std::env::var(AI_AUDIT_RETENTION_DAYS_ENV) {
        Ok(value) => normalize_audit_retention_days(Some(value.trim())),
        Err(_) => DEFAULT_AI_AUDIT_RETENTION_DAYS,
    }
}

pub fn normalize_audit_retention_days(raw: Option<&str>) -> i64 {
    match raw {
        Some(trimmed) if !trimmed.is_empty() => match trimmed.parse::<i64>() {
            Ok(days) => days.clamp(MIN_AI_AUDIT_RETENTION_DAYS, MAX_AI_AUDIT_RETENTION_DAYS),
            Err(error) => {
                warn!(
                    env = AI_AUDIT_RETENTION_DAYS_ENV,
                    value = trimmed,
                    error = %error,
                    "invalid AI audit retention env value; falling back to default"
                );
                DEFAULT_AI_AUDIT_RETENTION_DAYS
            }
        },
        _ => DEFAULT_AI_AUDIT_RETENTION_DAYS,
    }
}

pub async fn cleanup_expired_audit_events_once(
    pool: &rustfin_db::DbPool,
) -> Result<u64, crate::error::AppError> {
    let retention_days = audit_retention_days();
    let cutoff_ts = chrono::Utc::now().timestamp() - (retention_days * 86_400);
    rustfin_db::repo::ai_assistant_audit::delete_audit_events_older_than(pool, cutoff_ts)
        .await
        .map_err(|e| rustfin_core::error::ApiError::Internal(format!("db error: {e}")).into())
}

pub async fn run_audit_maintenance_loop(
    pool: rustfin_db::DbPool,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(AI_AUDIT_PRUNE_INTERVAL_SECS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = interval.tick() => {
                match cleanup_expired_audit_events_once(&pool).await {
                    Ok(pruned) if pruned > 0 => {
                        info!(
                            pruned,
                            retention_days = audit_retention_days(),
                            "pruned expired AI assistant audit events"
                        );
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!(error = ?error, "AI assistant audit cleanup failed");
                    }
                }
            }
        }
    }
}

#[cfg(feature = "ai")]
#[derive(Debug, Clone, Copy)]
pub enum AiAssistantAuditResponseKind {
    Clarification,
    UnsupportedWriteRefusal,
    Completed,
    ModelPathError,
    ModelLoadError,
    EngineUnavailable,
    StreamError,
}

#[cfg(feature = "ai")]
impl AiAssistantAuditResponseKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clarification => "clarification",
            Self::UnsupportedWriteRefusal => "unsupported_write_refusal",
            Self::Completed => "completed",
            Self::ModelPathError => "model_path_error",
            Self::ModelLoadError => "model_load_error",
            Self::EngineUnavailable => "engine_unavailable",
            Self::StreamError => "stream_error",
        }
    }
}

#[cfg(feature = "ai")]
#[allow(clippy::too_many_arguments)]
pub async fn persist_chat_audit_event(
    state: &crate::state::AppState,
    user: &crate::auth::AuthUser,
    request: &crate::ai_assistant::AssistantChatRequest,
    trace_id: &str,
    response_kind: AiAssistantAuditResponseKind,
    planned_tools: &[crate::ai_assistant::types::PlannedToolCall],
    grounding_blocks: &[crate::ai_assistant::types::AssistantToolContextBlock],
    grounding_chunks: &[crate::ai_assistant::types::AssistantGroundingChunk],
    grounding_sources: &[crate::ai_assistant::types::AssistantGroundingSource],
    error_message: Option<&str>,
) {
    persist_chat_audit_event_with_planner(
        state,
        user,
        request,
        trace_id,
        response_kind,
        planned_tools,
        grounding_blocks,
        grounding_chunks,
        grounding_sources,
        None,
        error_message,
    )
    .await;
}

#[cfg(feature = "ai")]
#[allow(clippy::too_many_arguments)]
pub async fn persist_chat_audit_event_with_planner(
    state: &crate::state::AppState,
    user: &crate::auth::AuthUser,
    request: &crate::ai_assistant::AssistantChatRequest,
    trace_id: &str,
    response_kind: AiAssistantAuditResponseKind,
    planned_tools: &[crate::ai_assistant::types::PlannedToolCall],
    grounding_blocks: &[crate::ai_assistant::types::AssistantToolContextBlock],
    grounding_chunks: &[crate::ai_assistant::types::AssistantGroundingChunk],
    grounding_sources: &[crate::ai_assistant::types::AssistantGroundingSource],
    planner_debug: Option<&crate::ai_assistant::types::AssistantPlannerDebug>,
    error_message: Option<&str>,
) {
    let role_routing = {
        let guard = state.engine.lock().await;
        guard.role_routing.clone()
    };
    let effective_model_name = role_routing
        .iter()
        .find(|decision| decision.role.eq_ignore_ascii_case("answer"))
        .map(|decision| decision.model_name.as_str())
        .unwrap_or(request.model.as_str());
    let planned_tools_json = serde_json::to_string(
        &planned_tools
            .iter()
            .map(|call| call.tool.as_str().to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let executed_tools_json = serde_json::to_string(
        &planned_tools
            .iter()
            .zip(grounding_blocks.iter())
            .map(|(call, block)| AiAssistantAuditToolExecution {
                tool: call.tool.as_str().to_string(),
                input_summary: input_summary(&call.input),
                status: block.status.to_string(),
                label: block.label.clone(),
                result_count: result_count(block),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let grounding_sources_json = serde_json::to_string(
        &grounding_sources
            .iter()
            .map(|source| AiAssistantAuditGroundingSource {
                tool: source.tool.to_string(),
                label: source.label.clone(),
                access_mode: serde_json::to_string(&source.access_mode)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"')
                    .to_string(),
                risk_tier: serde_json::to_string(&source.risk_tier)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"')
                    .to_string(),
                status: source.status.to_string(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let grounding_chunks_json =
        serde_json::to_string(grounding_chunks).unwrap_or_else(|_| "[]".to_string());
    let planner_json = planner_debug
        .map(serde_json::to_string)
        .transpose()
        .unwrap_or_else(|_| Some("{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    let model_routing_json =
        serde_json::to_string(&role_routing).unwrap_or_else(|_| "[]".to_string());

    let result = rustfin_db::repo::ai_assistant_audit::create_audit_event(
        &state.db,
        rustfin_db::repo::ai_assistant_audit::CreateAiAssistantAuditEventParams {
            trace_id,
            user_id: &user.user_id,
            username: &user.username,
            user_role: &user.role,
            model_name: effective_model_name,
            message_preview: &normalize_message_preview(&request.message),
            history_len: request.history.len() as i64,
            response_kind: response_kind.as_str(),
            planned_tools_json: &planned_tools_json,
            executed_tools_json: &executed_tools_json,
            planner_json: &planner_json,
            model_routing_json: &model_routing_json,
            grounding_chunks_json: &grounding_chunks_json,
            grounding_sources_json: &grounding_sources_json,
            error_message,
        },
    )
    .await;

    if let Err(error) = result {
        warn!(
            trace_id = %trace_id,
            user_id = %user.user_id,
            error = %error,
            "failed to persist AI assistant audit event"
        );
    }
}

#[cfg(feature = "ai")]
fn input_summary(input: &crate::ai_assistant::types::AssistantToolInput) -> String {
    use crate::ai_assistant::types::AssistantToolInput;

    match input {
        AssistantToolInput::None => "none".to_string(),
        AssistantToolInput::CurrentDateTime { location } => format!(
            "current_datetime:location={}",
            location.as_deref().unwrap_or("host")
        ),
        AssistantToolInput::CalendarWindow {
            from_date,
            to_date,
            label,
            query,
        } => format!(
            "calendar:{label}:{from_date}->{to_date}:query={}",
            query.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::CalendarCreateEvent {
            scope,
            title,
            event_date,
            ..
        } => format!("calendar_create_event:scope={scope}:title={title}:date={event_date}"),
        AssistantToolInput::CalendarCreateBirthday {
            scope,
            title,
            event_date,
            birthday_year,
            ..
        } => format!(
            "calendar_create_birthday:scope={scope}:title={title}:date={event_date}:year={birthday_year}"
        ),
        AssistantToolInput::CalendarDeleteEvent {
            event_id,
            title,
            event_date,
            scope,
            event_type,
            ..
        } => format!(
            "calendar_delete_event:id={event_id}:scope={scope}:title={title}:date={event_date}:type={event_type}"
        ),
        AssistantToolInput::DocumentCreateDownload {
            file_name,
            format,
            model_name,
            ..
        } => format!(
            "document_create_download:file_name={file_name}:format={format}:model={model_name}"
        ),
        AssistantToolInput::ConversationArchive {
            conversation_ids,
            selection_label,
            archived,
            ..
        } => format!(
            "conversations_archive:count={}:archived={archived}:selection={selection_label}",
            conversation_ids.len()
        ),
        AssistantToolInput::ConversationDelete {
            conversation_ids,
            selection_label,
            ..
        } => format!(
            "conversations_delete:count={}:selection={selection_label}",
            conversation_ids.len()
        ),
        AssistantToolInput::ConversationMoveToGroup {
            conversation_ids,
            selection_label,
            group_name,
            ..
        } => format!(
            "conversations_move_to_group:count={}:group={group_name}:selection={selection_label}",
            conversation_ids.len()
        ),
        AssistantToolInput::ChannelsFilter { query } => {
            format!("channels:query={}", query.as_deref().unwrap_or("*"))
        }
        AssistantToolInput::DownloadsFilter {
            query,
            availability,
        } => format!(
            "downloads:query={}:availability={}",
            query.as_deref().unwrap_or("*"),
            availability.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::LibrarySearch { query } => format!("library_query:{query}"),
        AssistantToolInput::LibraryRecent { query } => {
            format!("library_recent:query={}", query.as_deref().unwrap_or("*"))
        }
        AssistantToolInput::NetworkInterface { query } => {
            format!("network_interface:query={query}")
        }
        AssistantToolInput::NetworkDefaultRoute { query } => {
            format!(
                "network_default_route:query={}",
                query.as_deref().unwrap_or("*")
            )
        }
        AssistantToolInput::NetworkHostnameAliases { query } => {
            format!(
                "network_hostname_aliases:query={}",
                query.as_deref().unwrap_or("*")
            )
        }
        AssistantToolInput::NetworkDnsServers { query } => {
            format!(
                "network_dns_servers:query={}",
                query.as_deref().unwrap_or("*")
            )
        }
        AssistantToolInput::NetworkRouteDestination { destination } => {
            format!("network_route_destination:{destination}")
        }
        AssistantToolInput::NetworkActiveConnection { query } => {
            format!("network_active_connection:query={query}")
        }
        AssistantToolInput::Weather {
            location,
            forecast_days,
        } => format!(
            "weather:location={}:days={}",
            location,
            forecast_days
                .map(|days| days.to_string())
                .unwrap_or_else(|| "current".to_string())
        ),
        AssistantToolInput::WeatherHistory {
            location,
            start_date,
            end_date,
            label,
        } => format!(
            "weather_history:location={location}:label={label}:range={start_date}->{end_date}"
        ),
        AssistantToolInput::WebSearch { query, category } => match category.as_deref() {
            Some(category) => format!("web_search:{category}:{query}"),
            None => format!("web_search:{query}"),
        },
        AssistantToolInput::WebFetch { url, category } => match category.as_deref() {
            Some(category) => format!("web_fetch:{category}:{url}"),
            None => format!("web_fetch:{url}"),
        },
        AssistantToolInput::RoomsFilter { room_mode, query } => format!(
            "rooms:mode={}:query={}",
            room_mode.as_deref().unwrap_or("*"),
            query.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::DictionaryGetAccountIdentity => {
            "dictionary_account_identity".to_string()
        }
        AssistantToolInput::DictionaryListVisibleWorkspaces => {
            "dictionary_list_visible_workspaces".to_string()
        }
        AssistantToolInput::DictionaryBrowseWorkspacePeople {
            workspace_id,
            query,
            limit,
        } => format!(
            "dictionary_browse_workspace_people:workspace_id={workspace_id}:query={}:limit={}",
            query.as_deref().unwrap_or("*"),
            limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_string())
        ),
        AssistantToolInput::DictionarySearchPeople {
            workspace_id,
            query,
            limit,
        } => format!(
            "dictionary_search_people:workspace_id={workspace_id}:query={query}:limit={}",
            limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_string())
        ),
        AssistantToolInput::DictionaryGetPersonBundle {
            workspace_id,
            person_id,
        } => format!(
            "dictionary_get_person_bundle:workspace_id={workspace_id}:person_id={person_id}"
        ),
        AssistantToolInput::DictionaryResolveRelationshipReference {
            reference,
            workspace_id,
        } => format!(
            "dictionary_resolve_relationship_reference:reference={reference}:workspace_id={}",
            workspace_id.as_deref().unwrap_or("*")
        ),
        AssistantToolInput::SystemService { query } => {
            format!("system_service:query={query}")
        }
        AssistantToolInput::SystemPortConflicts { query } => {
            format!(
                "system_port_conflicts:query={}",
                query.as_deref().unwrap_or("*")
            )
        }
        AssistantToolInput::SystemFailedUnits { query } => {
            format!(
                "system_failed_units:query={}",
                query.as_deref().unwrap_or("*")
            )
        }
        AssistantToolInput::ServerFilter {
            query,
            availability,
        } => format!(
            "servers:query={}:availability={}",
            query.as_deref().unwrap_or("*"),
            availability.as_deref().unwrap_or("*")
        ),
        other => format!("tool_input:{other:?}"),
    }
}

#[cfg(feature = "ai")]
fn result_count(block: &crate::ai_assistant::types::AssistantToolContextBlock) -> Option<u64> {
    block
        .data
        .get("total_count")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            block
                .data
                .get("match_count")
                .and_then(serde_json::Value::as_u64)
        })
        .or_else(|| {
            [
                "events",
                "birthdays",
                "rooms",
                "servers",
                "matches",
                "artifacts",
                "results",
            ]
            .iter()
            .find_map(|key| {
                block
                    .data
                    .get(key)
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.len() as u64)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{
        AiAssistantAuditEventResponse, DEFAULT_AI_AUDIT_RETENTION_DAYS,
        MAX_AI_AUDIT_RETENTION_DAYS, MIN_AI_AUDIT_RETENTION_DAYS, normalize_audit_retention_days,
        normalize_message_preview, parse_audit_event_row,
    };
    use rustfin_db::repo::ai_assistant_audit::AiAssistantAuditEventRow;

    #[test]
    fn normalize_message_preview_collapses_whitespace() {
        assert_eq!(
            normalize_message_preview("  what   is \n on\tmy   calendar? "),
            "what is on my calendar?"
        );
    }

    #[test]
    fn normalize_message_preview_truncates_long_messages() {
        let preview = normalize_message_preview(&"a".repeat(400));
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 283);
    }

    #[test]
    fn normalize_message_preview_handles_empty_message() {
        assert_eq!(normalize_message_preview("   \n\t "), "(empty message)");
    }

    #[test]
    fn audit_retention_uses_default_when_missing() {
        assert_eq!(
            normalize_audit_retention_days(None),
            DEFAULT_AI_AUDIT_RETENTION_DAYS
        );
    }

    #[test]
    fn audit_retention_clamps_low_values() {
        assert_eq!(
            normalize_audit_retention_days(Some("0")),
            MIN_AI_AUDIT_RETENTION_DAYS
        );
    }

    #[test]
    fn audit_retention_clamps_high_values() {
        assert_eq!(
            normalize_audit_retention_days(Some("9999")),
            MAX_AI_AUDIT_RETENTION_DAYS
        );
    }

    #[test]
    fn audit_retention_uses_default_for_blank_and_invalid_values() {
        assert_eq!(
            normalize_audit_retention_days(Some("   ")),
            DEFAULT_AI_AUDIT_RETENTION_DAYS
        );
        assert_eq!(
            normalize_audit_retention_days(Some("not-a-number")),
            DEFAULT_AI_AUDIT_RETENTION_DAYS
        );
    }

    #[test]
    fn parse_audit_event_row_tolerates_invalid_json_payloads() {
        let parsed: AiAssistantAuditEventResponse =
            parse_audit_event_row(AiAssistantAuditEventRow {
                id: "audit-1".to_string(),
                trace_id: "trace-1".to_string(),
                user_id: "user-1".to_string(),
                username: "tester".to_string(),
                user_role: "user".to_string(),
                model_name: "model.gguf".to_string(),
                message_preview: "hello".to_string(),
                history_len: 2,
                response_kind: "completed".to_string(),
                planner_json: "{bad json".to_string(),
                model_routing_json: "{bad json".to_string(),
                planned_tools_json: "{bad json".to_string(),
                executed_tools_json: "{bad json".to_string(),
                grounding_chunks_json: "{bad json".to_string(),
                grounding_sources_json: "{bad json".to_string(),
                error_message: Some("oops".to_string()),
                created_ts: 123,
            });
        assert_eq!(parsed.planner, serde_json::json!({}));
        assert!(parsed.model_routing.is_empty());
        assert!(parsed.planned_tools.is_empty());
        assert!(parsed.executed_tools.is_empty());
        assert!(parsed.grounding_chunks.is_empty());
        assert!(parsed.grounding_sources.is_empty());
        assert_eq!(parsed.error_message.as_deref(), Some("oops"));
    }
}
