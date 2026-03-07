use rustfin_core::error::ApiError;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTranscribeChunkRequest {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub sample_rate_hz: u32,
    pub started_ts_ms: i64,
    pub ended_ts_ms: i64,
    pub pcm_s16le_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentTranscriptSegment {
    pub started_ts_ms: i64,
    pub ended_ts_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentTranscribeChunkResponse {
    segments: Vec<AgentTranscriptSegment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct AgentSessionControlRequest {
    session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiErrorBody {
    message: String,
    #[serde(default)]
    details: serde_json::Value,
}

fn map_agent_error(prefix: &str, status: reqwest::StatusCode, body: &[u8]) -> ApiError {
    let parsed = serde_json::from_slice::<ApiErrorEnvelope>(body).ok();
    let message = parsed
        .as_ref()
        .map(|v| v.error.message.clone())
        .unwrap_or_else(|| String::from_utf8_lossy(body).trim().to_string());
    let message = if message.is_empty() {
        format!("{prefix}: status {status}")
    } else {
        format!("{prefix}: {message}")
    };
    if status == reqwest::StatusCode::BAD_REQUEST {
        ApiError::BadRequest(message)
    } else if status == reqwest::StatusCode::UNAUTHORIZED {
        ApiError::Unauthorized(message)
    } else if status == reqwest::StatusCode::FORBIDDEN {
        ApiError::Forbidden(message)
    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_seconds = parsed
            .and_then(|v| {
                v.error
                    .details
                    .get("retry_after_seconds")
                    .and_then(|n| n.as_u64())
            })
            .unwrap_or(2);
        ApiError::TooManyRequests {
            retry_after_seconds,
        }
    } else {
        ApiError::Internal(message)
    }
}

async fn post_json<TReq: Serialize, TRes: for<'de> Deserialize<'de>>(
    state: &AppState,
    path: &str,
    body: &TReq,
    error_prefix: &str,
) -> Result<TRes, ApiError> {
    let base = state.transcription_agent_url.trim_end_matches('/');
    let url = format!("{base}{path}");
    let client = reqwest::Client::new();
    let mut request = client.post(url).json(body);
    if let Some(token) = state
        .transcription_agent_token
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        request = request.header("x-agent-token", token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("{error_prefix}: request failed: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.bytes().await.unwrap_or_default();
        return Err(map_agent_error(error_prefix, status, &body));
    }
    response
        .json::<TRes>()
        .await
        .map_err(|e| ApiError::Internal(format!("{error_prefix}: invalid response body: {e}")))
}

pub async fn start_session(state: &AppState, session_id: &str) -> Result<(), ApiError> {
    let body = AgentSessionControlRequest {
        session_id: session_id.to_string(),
    };
    let _: serde_json::Value = post_json(
        state,
        "/v1/sessions/start",
        &body,
        "failed to start transcription session",
    )
    .await?;
    Ok(())
}

pub async fn stop_session(state: &AppState, session_id: &str) -> Result<(), ApiError> {
    let body = AgentSessionControlRequest {
        session_id: session_id.to_string(),
    };
    let _: serde_json::Value = post_json(
        state,
        "/v1/sessions/stop",
        &body,
        "failed to stop transcription session",
    )
    .await?;
    Ok(())
}

pub async fn cancel_session(state: &AppState, session_id: &str) -> Result<(), ApiError> {
    let body = AgentSessionControlRequest {
        session_id: session_id.to_string(),
    };
    let _: serde_json::Value = post_json(
        state,
        "/v1/sessions/cancel",
        &body,
        "failed to cancel transcription session",
    )
    .await?;
    Ok(())
}

pub async fn transcribe_chunk(
    state: &AppState,
    body: &AgentTranscribeChunkRequest,
) -> Result<Vec<AgentTranscriptSegment>, ApiError> {
    let response: AgentTranscribeChunkResponse = post_json(
        state,
        "/v1/transcribe/chunk",
        body,
        "failed to transcribe channel audio chunk",
    )
    .await?;
    Ok(response.segments)
}
