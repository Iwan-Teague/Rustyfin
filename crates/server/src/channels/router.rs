use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, patch, post};

use crate::state::AppState;

pub fn channels_router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(super::handlers::list_channels).post(super::handlers::create_channel),
        )
        .route(
            "/{id}",
            patch(super::handlers::update_channel).delete(super::handlers::delete_channel),
        )
        .route(
            "/{id}/messages",
            get(super::handlers::get_messages).post(super::handlers::send_message),
        )
        .route(
            "/{id}/attachments",
            post(super::handlers::upload_attachment_message),
        )
        .route(
            "/attachments/{attachment_id}",
            get(super::handlers::download_attachment),
        )
        .route(
            "/{channel_id}/messages/{message_id}",
            delete(super::handlers::delete_message),
        )
        .route(
            "/{id}/transcription/status",
            get(super::handlers::get_transcription_status),
        )
        .route(
            "/{id}/transcription/sessions",
            get(super::handlers::list_transcription_sessions),
        )
        .route(
            "/{id}/transcription/start",
            post(super::handlers::start_transcription),
        )
        .route(
            "/{id}/transcription/stop",
            post(super::handlers::stop_transcription),
        )
        .route(
            "/{id}/transcription/cancel",
            post(super::handlers::cancel_transcription),
        )
        .route(
            "/{id}/transcription/chunk",
            post(super::handlers::transcribe_chunk),
        )
        .route(
            "/{id}/transcription/recording",
            post(super::handlers::upload_transcription_recording)
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route(
            "/{id}/transcription/sessions/{session_id}/download",
            get(super::handlers::download_transcription),
        )
        .route(
            "/{id}/transcription/sessions/{session_id}",
            delete(super::handlers::delete_transcription_session),
        )
        .route("/ws", get(super::ws::ws_handler))
}
