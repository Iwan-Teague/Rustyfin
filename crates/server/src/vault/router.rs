use axum::Extension;
use axum::Router;
use axum::middleware::from_fn;
use axum::routing::{delete, get, post};

use crate::state::AppState;
use crate::vault::handlers;
use crate::vault::middleware::{
    VaultRateLimiters, vault_rate_limit_middleware, vault_response_headers_middleware,
};

pub fn vault_router() -> Router<AppState> {
    Router::new()
        .route("/config", get(handlers::get_config))
        .route("/extension", get(handlers::get_extension_info))
        .route(
            "/extension/package",
            get(handlers::download_extension_package),
        )
        .route("/bootstrap", post(handlers::bootstrap_vault))
        .route("/rekey", post(handlers::rekey_vault))
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
        .route("/sync", get(handlers::sync_items))
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
        .route(
            "/protected-actions/complete",
            post(handlers::complete_protected_action),
        )
        .route("/audit", get(handlers::list_audit_events))
        .route("/export", post(handlers::export_vault))
        .route("/import/bitwarden", post(handlers::import_bitwarden))
        .route("/", delete(handlers::destroy_vault))
        .layer(from_fn(vault_rate_limit_middleware))
        .layer(Extension(VaultRateLimiters::new()))
        .layer(from_fn(vault_response_headers_middleware))
}
