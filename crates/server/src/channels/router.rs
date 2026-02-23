use axum::Router;
use axum::routing::{delete, get, patch};

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
            "/{channel_id}/messages/{message_id}",
            delete(super::handlers::delete_message),
        )
        .route("/ws", get(super::ws::ws_handler))
}
