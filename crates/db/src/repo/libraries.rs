use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct LibraryRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub created_ts: i64,
    pub updated_ts: i64,
}

#[derive(Debug, Clone)]
pub struct LibraryPathRow {
    pub id: String,
    pub library_id: String,
    pub path: String,
    pub is_read_only: bool,
    pub created_ts: i64,
}

pub async fn create_library(
    pool: &SqlitePool,
    name: &str,
    kind: &str,
    paths: &[String],
) -> Result<LibraryRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO library (id, name, kind, created_ts, updated_ts) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(kind)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    for p in paths {
        let path_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO library_path (id, library_id, path, is_read_only, created_ts) VALUES (?, ?, ?, 1, ?)",
        )
        .bind(&path_id)
        .bind(&id)
        .bind(p)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(LibraryRow {
        id,
        name: name.to_string(),
        kind: kind.to_string(),
        created_ts: now,
        updated_ts: now,
    })
}

pub async fn list_libraries(pool: &SqlitePool) -> Result<Vec<LibraryRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, name, kind, created_ts, updated_ts FROM library ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, kind, created_ts, updated_ts)| LibraryRow {
            id,
            name,
            kind,
            created_ts,
            updated_ts,
        })
        .collect())
}

pub async fn get_library(
    pool: &SqlitePool,
    library_id: &str,
) -> Result<Option<LibraryRow>, sqlx::Error> {
    let row: Option<(String, String, String, i64, i64)> = sqlx::query_as(
        "SELECT id, name, kind, created_ts, updated_ts FROM library WHERE id = ?",
    )
    .bind(library_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, name, kind, created_ts, updated_ts)| LibraryRow {
        id,
        name,
        kind,
        created_ts,
        updated_ts,
    }))
}

pub async fn update_library(
    pool: &SqlitePool,
    library_id: &str,
    name: Option<&str>,
) -> Result<bool, sqlx::Error> {
    if let Some(new_name) = name {
        let now = chrono::Utc::now().timestamp();
        let result =
            sqlx::query("UPDATE library SET name = ?, updated_ts = ? WHERE id = ?")
                .bind(new_name)
                .bind(now)
                .bind(library_id)
                .execute(pool)
                .await?;
        Ok(result.rows_affected() > 0)
    } else {
        Ok(false)
    }
}

pub async fn get_library_paths(
    pool: &SqlitePool,
    library_id: &str,
) -> Result<Vec<LibraryPathRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, bool, i64)> = sqlx::query_as(
        "SELECT id, library_id, path, is_read_only, created_ts FROM library_path WHERE library_id = ?",
    )
    .bind(library_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, library_id, path, is_read_only, created_ts)| LibraryPathRow {
                id,
                library_id,
                path,
                is_read_only,
                created_ts,
            },
        )
        .collect())
}

/// Count items belonging to a library.
pub async fn count_library_items(
    pool: &SqlitePool,
    library_id: &str,
) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM item WHERE library_id = ?")
            .bind(library_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

/// Get all library paths across all libraries.
pub async fn get_all_library_paths(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT path FROM library_path")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
