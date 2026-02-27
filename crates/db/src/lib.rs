#![allow(clippy::type_complexity, clippy::empty_line_after_doc_comments)]
pub mod migrate;
pub mod repo;

use sqlx::any::AnyPoolOptions;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackend {
    Sqlite,
    Postgres,
}

impl DatabaseBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

pub type DbPool = sqlx::AnyPool;

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
        DatabaseBackend::Sqlite
    }
}

pub fn active_backend() -> Option<DatabaseBackend> {
    ACTIVE_BACKEND.get().copied()
}

pub fn normalize_database_target(target: &str) -> (DatabaseBackend, String) {
    let trimmed = target.trim();
    let backend = detect_backend(trimmed);
    if backend == DatabaseBackend::Postgres {
        return (DatabaseBackend::Postgres, trimmed.to_string());
    }

    let sqlite_url = if trimmed.starts_with("sqlite:") {
        trimmed.to_string()
    } else if trimmed == ":memory:" {
        "sqlite::memory:".to_string()
    } else if trimmed.starts_with('/') {
        format!("sqlite://{trimmed}")
    } else {
        format!("sqlite://{trimmed}")
    };

    (DatabaseBackend::Sqlite, sqlite_url)
}

/// Create a database connection pool.
///
/// Accepts either:
/// - PostgreSQL URL (`postgres://` / `postgresql://`)
/// - SQLite URL (`sqlite:`)
/// - SQLite legacy path (`/path/to/file.db`, `file.db`, `:memory:`)
pub async fn connect(target: &str) -> Result<DbPool, sqlx::Error> {
    sqlx::any::install_default_drivers();

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
    if backend == DatabaseBackend::Sqlite {
        let maybe_path = if target.trim().starts_with("sqlite:") || target.trim() == ":memory:" {
            None
        } else {
            Some(target.trim())
        };
        if let Some(path) = maybe_path {
            if let Some(parent) = Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
    }

    let is_sqlite_memory = backend == DatabaseBackend::Sqlite
        && (target.trim() == ":memory:"
            || url == "sqlite::memory:"
            || url.starts_with("sqlite::memory:?"));

    let max_connections = if backend == DatabaseBackend::Sqlite {
        if is_sqlite_memory {
            // In-memory SQLite databases are connection-local.
            // Use a single connection to keep schema/data visible across queries.
            1
        } else {
            5
        }
    } else {
        15
    };

    let pool = AnyPoolOptions::new()
        .max_connections(max_connections)
        .connect(&url)
        .await?;

    // Keep SQLite runtime behavior close to previous defaults.
    if backend == DatabaseBackend::Sqlite {
        let _ = sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await;
        let _ = sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await;
    }

    Ok(pool)
}
