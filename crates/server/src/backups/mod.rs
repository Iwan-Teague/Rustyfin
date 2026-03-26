pub mod handlers;
pub mod repo;
pub mod scheduler;
pub mod service;

pub fn router(state: crate::state::AppState) -> axum::Router<crate::state::AppState> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/policies", get(handlers::list_policies).post(handlers::create_policy))
        .route("/jobs", get(handlers::list_jobs).post(handlers::create_backup_job))
        .route("/jobs/:id/restore", post(handlers::restore_backup))
        .with_state(state)
}
