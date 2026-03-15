#![allow(
    clippy::collapsible_if,
    clippy::ptr_arg,
    clippy::should_implement_trait
)]
pub mod account_prefs;
pub mod ai;
pub mod ai_admin;
#[cfg(feature = "ai")]
pub mod ai_enabled;
pub mod ai_storage;
pub mod artwork;
pub mod audit_log;
pub mod auth;
pub mod channels;
pub mod downloads;
pub mod error;
pub mod host_directories;
pub mod job_status;
pub mod library_scan;
pub mod routes;
pub mod runtime_metrics;
#[cfg(feature = "rustyvault")]
pub mod rustyvault_host;
pub mod servers;
pub mod setup;
pub mod state;
pub mod streaming;
pub mod tmdb_sync;
pub mod transcription_agent;
pub mod user_activity;
pub mod user_pipeline;
pub mod watch_party;
