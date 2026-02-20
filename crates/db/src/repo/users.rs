use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use password_hash::rand_core::OsRng;
use sqlx::SqlitePool;

/// User row from the database.
#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_ts: i64,
}

/// Create a new user. Returns the user ID.
pub async fn create_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: &str,
) -> Result<String, crate::DbError> {
    let id = uuid::Uuid::new_v4().to_string();
    let hash = hash_password(password)?;
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO user (id, username, password_hash, role, created_ts) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(username)
    .bind(&hash)
    .bind(role)
    .bind(now)
    .execute(pool)
    .await?;

    // Create default preferences
    sqlx::query("INSERT INTO user_pref (user_id, json, updated_ts) VALUES (?, '{}', ?)")
        .bind(&id)
        .bind(now)
        .execute(pool)
        .await?;

    Ok(id)
}

/// Find user by username.
pub async fn find_by_username(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, created_ts FROM user WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(
        row.map(|(id, username, password_hash, role, created_ts)| UserRow {
            id,
            username,
            password_hash,
            role,
            created_ts,
        }),
    )
}

/// Find user by ID.
pub async fn find_by_id(pool: &SqlitePool, user_id: &str) -> Result<Option<UserRow>, sqlx::Error> {
    let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, created_ts FROM user WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(
        row.map(|(id, username, password_hash, role, created_ts)| UserRow {
            id,
            username,
            password_hash,
            role,
            created_ts,
        }),
    )
}

/// List all users (admin).
pub async fn list_users(pool: &SqlitePool) -> Result<Vec<UserRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, role, created_ts FROM user ORDER BY created_ts",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, username, password_hash, role, created_ts)| UserRow {
            id,
            username,
            password_hash,
            role,
            created_ts,
        })
        .collect())
}

/// Delete a user by ID.
pub async fn delete_user(pool: &SqlitePool, user_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM user WHERE id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Update a user's role.
pub async fn update_user_role(
    pool: &SqlitePool,
    user_id: &str,
    role: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE user SET role = ? WHERE id = ?")
        .bind(role)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Replace library access entries for a user.
pub async fn set_library_access(
    pool: &SqlitePool,
    user_id: &str,
    library_ids: &[String],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM user_library_access WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    if !library_ids.is_empty() {
        let now = chrono::Utc::now().timestamp();
        for library_id in library_ids {
            sqlx::query(
                "INSERT INTO user_library_access (user_id, library_id, created_ts) VALUES (?, ?, ?)",
            )
            .bind(user_id)
            .bind(library_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(())
}

/// Return library IDs a user can access.
pub async fn get_library_access(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT library_id FROM user_library_access WHERE user_id = ? ORDER BY library_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Check whether a user can access a specific library.
pub async fn is_library_allowed(
    pool: &SqlitePool,
    user_id: &str,
    library_id: &str,
) -> Result<bool, sqlx::Error> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM user_library_access WHERE user_id = ? AND library_id = ? LIMIT 1",
    )
    .bind(user_id)
    .bind(library_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// Check if any users exist (for admin bootstrap).
pub async fn count_users(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Get user preferences JSON.
pub async fn get_preferences(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT json FROM user_pref WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(json,)| json))
}

/// Update user preferences JSON.
pub async fn update_preferences(
    pool: &SqlitePool,
    user_id: &str,
    json: &str,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT INTO user_pref (user_id, json, updated_ts) VALUES (?, ?, ?) ON CONFLICT(user_id) DO UPDATE SET json = excluded.json, updated_ts = excluded.updated_ts")
        .bind(user_id)
        .bind(json)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// Verify a password against a stored hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, crate::DbError> {
    let parsed = PasswordHash::new(hash).map_err(|e| crate::DbError::Hash(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

fn hash_password(password: &str) -> Result<String, crate::DbError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| crate::DbError::Hash(e.to_string()))?;
    Ok(hash.to_string())
}
