use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn watch_party_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(super::handlers::health))
        .route("/rooms/{room_id}/ws", get(super::ws::ws_connect))
        // Stage 4+ endpoints are mounted here as handlers are implemented.
        .route("/users", get(super::handlers::list_inviteable_users))
        .route(
            "/eligible-libraries",
            post(super::handlers::eligible_libraries),
        )
        .route("/rooms", post(super::handlers::create_room))
        .route("/rooms/{room_id}", get(super::handlers::get_room))
        .route("/rooms/{room_id}/join", post(super::handlers::join_room))
        .route("/rooms/{room_id}/leave", post(super::handlers::leave_room))
        .route("/rooms/{room_id}/end", post(super::handlers::end_room))
        .route("/invites", get(super::handlers::list_invites))
        .route(
            "/invites/{room_id}/decline",
            post(super::handlers::decline_invite),
        )
}
