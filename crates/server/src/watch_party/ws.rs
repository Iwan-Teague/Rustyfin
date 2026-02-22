use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use futures::SinkExt;
use rustfin_core::error::ApiError;

use crate::error::AppError;
use crate::state::AppState;

const MAX_WS_FRAME_BYTES: usize = 32 * 1024;

pub async fn ws_connect(
    State(_state): State<AppState>,
    Path(_room_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    Ok(ws
        .max_frame_size(MAX_WS_FRAME_BYTES)
        .max_message_size(MAX_WS_FRAME_BYTES)
        .on_upgrade(handle_socket))
}

async fn handle_socket(mut socket: WebSocket) {
    let _ = socket
        .send(Message::Text(
            serde_json::json!({
                "type": "error",
                "message": "watch party websocket is not fully configured yet"
            })
            .to_string()
            .into(),
        ))
        .await;
    let _ = socket.close().await;
}

pub fn reject_invalid_origin(origin: Option<&str>, allowed: &[String]) -> Result<(), AppError> {
    if allowed.is_empty() {
        return Ok(());
    }
    let origin = origin.ok_or_else(|| ApiError::Forbidden("missing origin".into()))?;
    if allowed
        .iter()
        .any(|allowed_origin| allowed_origin == origin)
    {
        Ok(())
    } else {
        Err(ApiError::Forbidden("origin is not allowed for websocket".into()).into())
    }
}
