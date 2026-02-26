use axum::http::HeaderMap;

use crate::error::ApiError;

pub fn normalize_secret<T: AsRef<str>>(value: Option<T>) -> Option<String> {
    value
        .map(|v| v.as_ref().trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn verify_agent_token(
    headers: &HeaderMap,
    expected: Option<&str>,
    agent_name: &str,
) -> Result<(), ApiError> {
    let Some(expected_token) = expected.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(());
    };

    let supplied = headers
        .get("x-agent-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();

    if supplied != expected_token {
        return Err(ApiError::Unauthorized(format!(
            "missing or invalid {agent_name} token"
        )));
    }

    Ok(())
}
