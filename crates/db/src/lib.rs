#![allow(clippy::type_complexity, clippy::empty_line_after_doc_comments)]
pub mod migrate;
pub mod repo;

use sqlx::postgres::PgPoolOptions;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    Postgres,
}

impl DatabaseBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
        }
    }
}

pub type DbPool = sqlx::PgPool;

static ACTIVE_BACKEND: OnceLock<DatabaseBackend> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("password hash error: {0}")]
    Hash(String),
}

pub fn detect_backend(target: &str) -> DatabaseBackend {
    let trimmed = target.trim().to_ascii_lowercase();
    if trimmed.starts_with("postgres://") || trimmed.starts_with("postgresql://") {
        DatabaseBackend::Postgres
    } else {
        panic!("unsupported database target; only PostgreSQL URLs are accepted (got: {target})");
    }
}

pub fn active_backend() -> Option<DatabaseBackend> {
    ACTIVE_BACKEND.get().copied()
}

pub fn normalize_database_target(target: &str) -> (DatabaseBackend, String) {
    let trimmed = target.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        (DatabaseBackend::Postgres, trimmed.to_string())
    } else {
        panic!("unsupported database target; only PostgreSQL URLs are accepted");
    }
}

/// Create a database connection pool from a PostgreSQL URL.
pub async fn connect(target: &str) -> Result<DbPool, sqlx::Error> {
    let (backend, url) = normalize_database_target(target);
    if let Some(existing) = ACTIVE_BACKEND.get().copied() {
        if existing != backend {
            tracing::warn!(
                existing = existing.as_str(),
                attempted = backend.as_str(),
                "database backend mismatch in same process; keeping initial backend selection"
            );
        }
    } else {
        let _ = ACTIVE_BACKEND.set(backend);
    }

    let max_conns: u32 = std::env::var("RUSTFIN_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(15);
    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .connect(&url)
        .await?;
    Ok(pool)
}
