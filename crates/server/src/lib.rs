#![allow(
    clippy::collapsible_if,
    clippy::ptr_arg,
    clippy::should_implement_trait
)]
pub mod artwork;
pub mod auth;
pub mod channels;
pub mod error;
pub mod library_scan;
pub mod routes;
pub mod setup;
pub mod state;
pub mod streaming;
pub mod tmdb_sync;
pub mod transcription_agent;
pub mod user_pipeline;
pub mod watch_party;
