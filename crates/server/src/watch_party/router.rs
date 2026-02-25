use axum::Router;
use axum::routing::{delete, get, patch, post};

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
        .route(
            "/rooms",
            get(super::handlers::list_public_rooms).post(super::handlers::create_room),
        )
        .route("/rooms/{room_id}", get(super::handlers::get_room))
        .route("/rooms/{room_id}/join", post(super::handlers::join_room))
        .route("/rooms/{room_id}/leave", post(super::handlers::leave_room))
        .route("/rooms/{room_id}/end", post(super::handlers::end_room))
        .route("/admin/rooms", get(super::handlers::admin_list_rooms))
        .route(
            "/admin/rooms/{room_id}/rename",
            patch(super::handlers::admin_rename_room),
        )
        .route(
            "/admin/rooms/{room_id}/end",
            post(super::handlers::admin_end_room),
        )
        .route(
            "/admin/rooms/{room_id}",
            delete(super::handlers::admin_delete_room),
        )
        .route(
            "/rooms/{room_id}/reconfigure",
            post(super::handlers::reconfigure_room),
        )
        .route(
            "/rooms/{room_id}/invite",
            post(super::handlers::invite_members),
        )
        .route(
            "/rooms/{room_id}/audio/tracks",
            get(super::handlers::list_audio_tracks),
        )
        .route(
            "/rooms/{room_id}/audio/online/search",
            get(super::handlers::search_online_audio),
        )
        .route(
            "/rooms/{room_id}/audio/online/queue",
            post(super::handlers::queue_online_audio),
        )
        .route(
            "/rooms/{room_id}/audio/online/tracks/{track_id}/stream",
            get(super::handlers::stream_online_audio_track),
        )
        .route(
            "/rooms/{room_id}/youtube/search",
            get(super::handlers::search_youtube),
        )
        .route(
            "/rooms/{room_id}/youtube/lookup",
            post(super::handlers::lookup_youtube_videos),
        )
        .route("/invites", get(super::handlers::list_invites))
        .route(
            "/invites/{room_id}/decline",
            post(super::handlers::decline_invite),
        )
}
