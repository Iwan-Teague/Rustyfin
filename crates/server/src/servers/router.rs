use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub fn servers_router() -> Router<AppState> {
    Router::new()
        .route(
            "/minecraft/instances",
            get(super::handlers::list_minecraft_servers)
                .post(super::handlers::create_minecraft_server),
        )
        .route(
            "/minecraft/instances/{id}",
            get(super::handlers::get_minecraft_server),
        )
        .route(
            "/minecraft/instances/{id}/status",
            get(super::handlers::refresh_minecraft_server_status),
        )
        .route(
            "/minecraft/instances/{id}/provision",
            post(super::handlers::provision_minecraft_server),
        )
        .route(
            "/minecraft/instances/{id}/import",
            post(super::handlers::import_minecraft_server),
        )
        .route(
            "/minecraft/instances/{id}/events",
            get(super::handlers::list_minecraft_server_events),
        )
        .route(
            "/minecraft/instances/{id}/actions/{action}",
            post(super::handlers::request_minecraft_server_action),
        )
}
