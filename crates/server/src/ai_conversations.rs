use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::types::{
    AssistantActivityTraceItem, AssistantFollowUpContext, AssistantGroundingChunk,
    AssistantGroundingSource, AssistantHistoryMessage, AssistantPendingAction,
    AssistantPendingActionStatus, AssistantTurnStats,
};
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const DEFAULT_CONVERSATION_TITLE: &str = "New chat";
const MAX_CONVERSATION_TITLE_CHARS: usize = 80;
const MAX_LIST_CONVERSATIONS: i64 = 200;

#[derive(Debug, Deserialize)]
pub struct ConversationListQuery {
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct CreateConversationRequest {
    pub title: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateConversationRequest {
    pub title: Option<String>,
    pub archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ConversationMessageRequest {
    pub model: String,
    pub message: String,
    #[serde(default)]
    pub confirmation_token: Option<String>,
    #[allow(dead_code)]
    pub client_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub last_message_preview: Option<String>,
    pub last_model_name: Option<String>,
    pub updated_ts: i64,
    pub archived: bool,
}

#[derive(Debug, Serialize)]
pub struct ConversationListResponse {
    pub conversations: Vec<ConversationSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationTurnResponse {
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(default)]
    pub grounding_tools: Vec<String>,
    #[serde(default)]
    pub follow_up_contexts: Vec<AssistantFollowUpContext>,
    #[serde(default)]
    pub grounding_chunks: Vec<AssistantGroundingChunk>,
    #[serde(default)]
    pub grounding_sources: Vec<AssistantGroundingSource>,
    #[serde(default)]
    pub activity_trace: Vec<AssistantActivityTraceItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<AssistantTurnStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_action: Option<AssistantPendingAction>,
    pub created_ts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationDetail {
    pub id: String,
    pub title: String,
    pub archived: bool,
    pub last_message_preview: Option<String>,
    pub last_model_name: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub messages: Vec<ConversationTurnResponse>,
}

#[derive(Debug, Serialize)]
pub struct ConversationResponse {
    pub conversation: ConversationDetail,
}

pub async fn list_conversations(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ConversationListQuery>,
) -> Result<Json<ConversationListResponse>, AppError> {
    let rows = rustfin_db::repo::ai_conversations::list_conversations_for_user(
        &state.db,
        &user.user_id,
        query.include_archived,
        MAX_LIST_CONVERSATIONS,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(Json(ConversationListResponse {
        conversations: rows
            .into_iter()
            .map(conversation_summary_from_row)
            .collect(),
    }))
}

pub async fn create_conversation(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateConversationRequest>,
) -> Result<(StatusCode, Json<ConversationResponse>), AppError> {
    let title = normalize_conversation_title(req.title.as_deref())?;
    let conversation = rustfin_db::repo::ai_conversations::create_conversation(
        &state.db,
        rustfin_db::repo::ai_conversations::CreateAiConversationParams {
            user_id: &user.user_id,
            title: &title,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok((
        StatusCode::CREATED,
        Json(ConversationResponse {
            conversation: ConversationDetail {
                id: conversation.id,
                title: conversation.title,
                archived: conversation.archived,
                last_message_preview: conversation.last_message_preview,
                last_model_name: conversation.last_model_name,
                created_ts: conversation.created_ts,
                updated_ts: conversation.updated_ts,
                messages: Vec::new(),
            },
        }),
    ))
}

pub async fn get_conversation(
    user: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ConversationResponse>, AppError> {
    let detail = load_conversation_detail(&state, &user.user_id, &conversation_id).await?;
    Ok(Json(ConversationResponse {
        conversation: detail,
    }))
}

pub async fn update_conversation(
    user: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<UpdateConversationRequest>,
) -> Result<Json<ConversationResponse>, AppError> {
    if req.title.is_none() && req.archived.is_none() {
        return Err(ApiError::validation(serde_json::json!({
            "title": ["provide a title or archived change"]
        }))
        .into());
    }

    let title = match req.title.as_deref() {
        Some(raw) => Some(normalize_conversation_title(Some(raw))?),
        None => None,
    };

    let conversation = rustfin_db::repo::ai_conversations::update_conversation_for_user(
        &state.db,
        &conversation_id,
        &user.user_id,
        title.as_deref(),
        req.archived,
        None,
        None,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if conversation.is_none() {
        return Err(ApiError::NotFound("conversation not found".into()).into());
    }

    let detail = load_conversation_detail(&state, &user.user_id, &conversation_id).await?;
    Ok(Json(ConversationResponse {
        conversation: detail,
    }))
}

pub async fn delete_conversation(
    user: AuthUser,
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let deleted = rustfin_db::repo::ai_conversations::delete_conversation_for_user(
        &state.db,
        &conversation_id,
        &user.user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    if !deleted {
        return Err(ApiError::NotFound("conversation not found".into()).into());
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn load_conversation_request_context(
    state: &AppState,
    user_id: &str,
    conversation_id: &str,
) -> Result<
    (
        rustfin_db::repo::ai_conversations::AiConversationRow,
        Vec<ConversationTurnResponse>,
        Vec<AssistantHistoryMessage>,
    ),
    AppError,
> {
    let conversation = rustfin_db::repo::ai_conversations::get_conversation_for_user(
        &state.db,
        conversation_id,
        user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let Some(conversation) = conversation else {
        return Err(ApiError::NotFound("conversation not found".into()).into());
    };

    let turns = rustfin_db::repo::ai_conversations::list_turns_for_conversation(
        &state.db,
        conversation_id,
        user_id,
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let messages = turns
        .iter()
        .map(turn_response_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let history = messages
        .iter()
        .map(|message| AssistantHistoryMessage {
            role: message.role.clone(),
            content: message.content.clone(),
            grounding_tools: message.grounding_tools.clone(),
            follow_up_contexts: message.follow_up_contexts.clone(),
            grounding_chunks: message.grounding_chunks.clone(),
        })
        .collect();

    Ok((conversation, messages, history))
}

pub async fn persist_user_turn(
    state: &AppState,
    user_id: &str,
    conversation_id: &str,
    message: &str,
) -> Result<(), AppError> {
    rustfin_db::repo::ai_conversations::create_turn(
        &state.db,
        rustfin_db::repo::ai_conversations::CreateAiConversationTurnParams {
            conversation_id,
            user_id,
            role: "user",
            content: message,
            model_name: None,
            grounding_tools_json: "[]",
            follow_up_contexts_json: "[]",
            grounding_chunks_json: "[]",
            grounding_sources_json: "[]",
            activity_trace_json: "[]",
            stats_json: None,
            pending_action_json: None,
            trace_id: None,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    let title = suggested_conversation_title(message);
    let _ = rustfin_db::repo::ai_conversations::replace_title_if_default(
        &state.db,
        conversation_id,
        user_id,
        &title,
    )
    .await;

    rustfin_db::repo::ai_conversations::touch_conversation_from_turn(
        &state.db,
        conversation_id,
        user_id,
        &crate::ai_audit::normalize_message_preview(message),
        None,
        Some(false),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn persist_assistant_turn(
    state: &AppState,
    user_id: &str,
    conversation_id: &str,
    content: &str,
    model_name: &str,
    grounding_tools: &[String],
    follow_up_contexts: &[AssistantFollowUpContext],
    grounding_chunks: &[AssistantGroundingChunk],
    grounding_sources: &[AssistantGroundingSource],
    activity_trace: &[AssistantActivityTraceItem],
    stats: Option<&AssistantTurnStats>,
    pending_action: Option<&AssistantPendingAction>,
    trace_id: Option<&str>,
) -> Result<String, AppError> {
    let grounding_tools_json = serde_json::to_string(grounding_tools)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let follow_up_contexts_json = serde_json::to_string(follow_up_contexts)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let grounding_chunks_json = serde_json::to_string(grounding_chunks)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let grounding_sources_json = serde_json::to_string(grounding_sources)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let activity_trace_json = serde_json::to_string(activity_trace)
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let stats_json = stats
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;
    let pending_action_json = pending_action
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("json error: {e}")))?;

    let turn = rustfin_db::repo::ai_conversations::create_turn(
        &state.db,
        rustfin_db::repo::ai_conversations::CreateAiConversationTurnParams {
            conversation_id,
            user_id,
            role: "assistant",
            content,
            model_name: Some(model_name),
            grounding_tools_json: &grounding_tools_json,
            follow_up_contexts_json: &follow_up_contexts_json,
            grounding_chunks_json: &grounding_chunks_json,
            grounding_sources_json: &grounding_sources_json,
            activity_trace_json: &activity_trace_json,
            stats_json: stats_json.as_deref(),
            pending_action_json: pending_action_json.as_deref(),
            trace_id,
        },
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    rustfin_db::repo::ai_conversations::touch_conversation_from_turn(
        &state.db,
        conversation_id,
        user_id,
        &crate::ai_audit::normalize_message_preview(content),
        Some(model_name),
        Some(false),
    )
    .await
    .map_err(|e| ApiError::Internal(format!("db error: {e}")))?;

    Ok(turn.id)
}

fn conversation_summary_from_row(
    row: rustfin_db::repo::ai_conversations::AiConversationRow,
) -> ConversationSummary {
    ConversationSummary {
        id: row.id,
        title: row.title,
        last_message_preview: row.last_message_preview,
        last_model_name: row.last_model_name,
        updated_ts: row.updated_ts,
        archived: row.archived,
    }
}

fn normalize_conversation_title(raw: Option<&str>) -> Result<String, AppError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_CONVERSATION_TITLE.to_string());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::validation(serde_json::json!({
            "title": ["cannot be empty"]
        }))
        .into());
    }

    let mut title = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= MAX_CONVERSATION_TITLE_CHARS {
            title.push_str("...");
            return Ok(title);
        }
        title.push(ch);
    }
    Ok(title)
}

fn suggested_conversation_title(message: &str) -> String {
    let preview = crate::ai_audit::normalize_message_preview(message);
    if preview == "(empty message)" {
        return DEFAULT_CONVERSATION_TITLE.to_string();
    }

    let mut title = String::new();
    for (index, ch) in preview.chars().enumerate() {
        if index >= MAX_CONVERSATION_TITLE_CHARS {
            title.push_str("...");
            break;
        }
        title.push(ch);
    }
    title
}

fn parse_json_or_default<T>(raw: &str) -> T
where
    T: Default + for<'de> Deserialize<'de>,
{
    serde_json::from_str(raw).unwrap_or_default()
}

fn turn_response_from_row(
    row: &rustfin_db::repo::ai_conversations::AiConversationTurnRow,
) -> Result<ConversationTurnResponse, AppError> {
    let grounding_tools = parse_json_or_default(&row.grounding_tools_json);
    let follow_up_contexts = parse_json_or_default(&row.follow_up_contexts_json);
    let grounding_chunks = parse_json_or_default(&row.grounding_chunks_json);
    let grounding_sources = parse_json_or_default(&row.grounding_sources_json);
    let activity_trace = parse_json_or_default(&row.activity_trace_json);
    let stats = row
        .stats_json
        .as_deref()
        .map(serde_json::from_str::<AssistantTurnStats>)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("invalid stored assistant stats: {e}")))?;
    let pending_action = row
        .pending_action_json
        .as_deref()
        .map(serde_json::from_str::<AssistantPendingAction>)
        .transpose()
        .map_err(|e| ApiError::Internal(format!("invalid stored assistant pending action: {e}")))?
        .map(|mut pending| {
            if pending.status == AssistantPendingActionStatus::Pending
                && pending.expires_ts < chrono::Utc::now().timestamp()
            {
                pending.status = AssistantPendingActionStatus::Expired;
            }
            pending
        });

    Ok(ConversationTurnResponse {
        id: row.id.clone(),
        role: row.role.clone(),
        content: row.content.clone(),
        model_name: row.model_name.clone(),
        grounding_tools,
        follow_up_contexts,
        grounding_chunks,
        grounding_sources,
        activity_trace,
        stats,
        pending_action,
        created_ts: row.created_ts,
    })
}

async fn load_conversation_detail(
    state: &AppState,
    user_id: &str,
    conversation_id: &str,
) -> Result<ConversationDetail, AppError> {
    let (conversation, messages, _) =
        load_conversation_request_context(state, user_id, conversation_id).await?;

    Ok(ConversationDetail {
        id: conversation.id,
        title: conversation.title,
        archived: conversation.archived,
        last_message_preview: conversation.last_message_preview,
        last_model_name: conversation.last_model_name,
        created_ts: conversation.created_ts,
        updated_ts: conversation.updated_ts,
        messages,
    })
}
