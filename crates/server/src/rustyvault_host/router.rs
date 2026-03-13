use axum::Extension;
use axum::Router;
use axum::middleware::from_fn;
use axum::routing::{delete, get, post};

use crate::rustyvault_host::handlers;
use crate::rustyvault_host::middleware::{
    RustyVaultRateLimiters, rustyvault_rate_limit_middleware,
    rustyvault_response_headers_middleware,
};
use crate::state::AppState;

pub fn rustyvault_router() -> Router<AppState> {
    Router::new()
        .route("/config", get(handlers::get_config))
        .route(
            "/preferences",
            get(handlers::get_preferences).patch(handlers::update_preferences),
        )
        .route("/bootstrap", post(handlers::bootstrap_rustyvault))
        .route("/rekey", post(handlers::rekey_rustyvault))
        .route(
            "/items",
            get(handlers::list_items).post(handlers::create_item),
        )
        .route(
            "/items/{id}",
            get(handlers::get_item)
                .put(handlers::replace_item)
                .delete(handlers::delete_item),
        )
        .route("/lookup", post(handlers::lookup_items))
        .route(
            "/device-sessions/pair",
            post(handlers::create_device_session),
        )
        .route(
            "/device-sessions/pair/consume",
            post(handlers::consume_pairing_code),
        )
        .route(
            "/device-sessions/refresh",
            post(handlers::refresh_device_session),
        )
        .route("/device-sessions", get(handlers::list_device_sessions))
        .route(
            "/device-sessions/revoke-others",
            post(handlers::revoke_other_device_sessions),
        )
        .route(
            "/device-sessions/{id}",
            delete(handlers::revoke_device_session),
        )
        .route(
            "/protected-actions/challenge",
            post(handlers::challenge_protected_action),
        )
        .route("/audit", get(handlers::list_audit_events))
        .route("/export", post(handlers::export_rustyvault))
        .route("/import/bitwarden", post(handlers::import_bitwarden))
        .route("/", delete(handlers::destroy_rustyvault))
        .layer(from_fn(rustyvault_rate_limit_middleware))
        .layer(Extension(RustyVaultRateLimiters::new()))
        .layer(from_fn(rustyvault_response_headers_middleware))
}
