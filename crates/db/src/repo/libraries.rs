use crate::DbPool;

fn bool_to_db_int(v: bool) -> i32 {
    if v { 1 } else { 0 }
}

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

#[derive(Debug, Clone)]
pub struct LibrarySettingsRow {
    pub library_id: String,
    pub show_images: bool,
    pub prefer_local_artwork: bool,
    pub fetch_online_artwork: bool,
    pub tmdb_store_in_media_dir: bool,
    pub tmdb_sync_on_new_media: bool,
    pub tmdb_sync_schedule: String,
    pub tmdb_last_sync_ts: Option<i64>,
    pub tmdb_fetch_posters: bool,
    pub tmdb_fetch_backdrops: bool,
    pub tmdb_fetch_metadata: bool,
    pub tmdb_fetch_reviews: bool,
    pub updated_ts: i64,
}

pub async fn create_library(
    pool: &DbPool,
    name: &str,
    kind: &str,
    paths: &[String],
) -> Result<LibraryRow, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO library (id, name, kind, created_ts, updated_ts) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&id)
    .bind(name)
    .bind(kind)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for p in paths {
        let path_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO library_path (id, library_id, path, is_read_only, created_ts) VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(&path_id)
        .bind(&id)
        .bind(p)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO library_settings \
         (library_id, show_images, prefer_local_artwork, fetch_online_artwork, updated_ts) \
         VALUES ($1, 1, 1, 1, $2) \
         ON CONFLICT(library_id) DO NOTHING",
    )
    .bind(&id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(LibraryRow {
        id,
        name: name.to_string(),
        kind: kind.to_string(),
        created_ts: now,
        updated_ts: now,
    })
}

pub async fn list_libraries(pool: &DbPool) -> Result<Vec<LibraryRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, i64, i64)> =
        sqlx::query_as("SELECT id, name, kind, created_ts, updated_ts FROM library ORDER BY name")
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
    pool: &DbPool,
    library_id: &str,
) -> Result<Option<LibraryRow>, sqlx::Error> {
    let row: Option<(String, String, String, i64, i64)> =
        sqlx::query_as("SELECT id, name, kind, created_ts, updated_ts FROM library WHERE id = $1")
            .bind(library_id)
            .fetch_optional(pool)
            .await?;

    Ok(
        row.map(|(id, name, kind, created_ts, updated_ts)| LibraryRow {
            id,
            name,
            kind,
            created_ts,
            updated_ts,
        }),
    )
}

pub async fn update_library(
    pool: &DbPool,
    library_id: &str,
    name: Option<&str>,
) -> Result<bool, sqlx::Error> {
    if let Some(new_name) = name {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query("UPDATE library SET name = $1, updated_ts = $2 WHERE id = $3")
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

pub async fn replace_library_paths(
    pool: &DbPool,
    library_id: &str,
    paths: &[String],
) -> Result<bool, sqlx::Error> {
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM library WHERE id = $1")
        .bind(library_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Ok(false);
    }

    let now = chrono::Utc::now().timestamp();
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM library_path WHERE library_id = $1")
        .bind(library_id)
        .execute(&mut *tx)
        .await?;

    for path in paths {
        let path_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO library_path (id, library_id, path, is_read_only, created_ts) VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(&path_id)
        .bind(library_id)
        .bind(path)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("UPDATE library SET updated_ts = $1 WHERE id = $2")
        .bind(now)
        .bind(library_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(true)
}

pub async fn delete_library(pool: &DbPool, library_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM library WHERE id = $1")
        .bind(library_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_library_paths(
    pool: &DbPool,
    library_id: &str,
) -> Result<Vec<LibraryPathRow>, sqlx::Error> {
    let rows: Vec<(String, String, String, bool, i64)> = sqlx::query_as(
        "SELECT id, library_id, path, (is_read_only <> 0) AS is_read_only, created_ts \
         FROM library_path \
         WHERE library_id = $1",
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

pub async fn get_library_paths_for_libraries(
    pool: &DbPool,
    library_ids: &[String],
) -> Result<Vec<LibraryPathRow>, sqlx::Error> {
    if library_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(1, library_ids.len());
    let sql = format!(
        "SELECT id, library_id, path, (is_read_only <> 0) AS is_read_only, created_ts \
         FROM library_path \
         WHERE library_id IN ({placeholders}) \
         ORDER BY library_id, created_ts, id"
    );

    let mut query = sqlx::query_as::<_, (String, String, String, bool, i64)>(&sql);
    for library_id in library_ids {
        query = query.bind(library_id);
    }
    let rows = query.fetch_all(pool).await?;

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
pub async fn count_library_items(pool: &DbPool, library_id: &str) -> Result<i64, sqlx::Error> {
    // Keep this aligned with GET /libraries/{id}/items, which returns top-level rows only.
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM item WHERE library_id = $1 AND parent_id IS NULL")
            .bind(library_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

pub async fn count_library_items_for_libraries(
    pool: &DbPool,
    library_ids: &[String],
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    if library_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(1, library_ids.len());
    let sql = format!(
        "SELECT library_id, COUNT(*) FROM item \
         WHERE parent_id IS NULL AND library_id IN ({placeholders}) \
         GROUP BY library_id"
    );

    let mut query = sqlx::query_as::<_, (String, i64)>(&sql);
    for library_id in library_ids {
        query = query.bind(library_id);
    }
    query.fetch_all(pool).await
}

/// Get all library paths across all libraries.
pub async fn get_all_library_paths(pool: &DbPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT path FROM library_path")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn get_library_settings(
    pool: &DbPool,
    library_id: &str,
) -> Result<Option<LibrarySettingsRow>, sqlx::Error> {
    let row: Option<(
        String,
        bool,
        bool,
        bool,
        bool,
        bool,
        String,
        Option<i64>,
        bool,
        bool,
        bool,
        bool,
        i64,
    )> = sqlx::query_as(
        "SELECT library_id, \
            (show_images <> 0) AS show_images, \
            (prefer_local_artwork <> 0) AS prefer_local_artwork, \
            (fetch_online_artwork <> 0) AS fetch_online_artwork, \
            (tmdb_store_in_media_dir <> 0) AS tmdb_store_in_media_dir, \
            (tmdb_sync_on_new_media <> 0) AS tmdb_sync_on_new_media, \
            tmdb_sync_schedule, \
            CAST(tmdb_last_sync_ts AS BIGINT) AS tmdb_last_sync_ts, \
            (tmdb_fetch_posters <> 0) AS tmdb_fetch_posters, \
            (tmdb_fetch_backdrops <> 0) AS tmdb_fetch_backdrops, \
            (tmdb_fetch_metadata <> 0) AS tmdb_fetch_metadata, \
            (tmdb_fetch_reviews <> 0) AS tmdb_fetch_reviews, \
            CAST(updated_ts AS BIGINT) AS updated_ts \
         FROM library_settings WHERE library_id = $1",
    )
    .bind(library_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            library_id,
            show_images,
            prefer_local_artwork,
            fetch_online_artwork,
            tmdb_store_in_media_dir,
            tmdb_sync_on_new_media,
            tmdb_sync_schedule,
            tmdb_last_sync_ts,
            tmdb_fetch_posters,
            tmdb_fetch_backdrops,
            tmdb_fetch_metadata,
            tmdb_fetch_reviews,
            updated_ts,
        )| {
            LibrarySettingsRow {
                library_id,
                show_images,
                prefer_local_artwork,
                fetch_online_artwork,
                tmdb_store_in_media_dir,
                tmdb_sync_on_new_media,
                tmdb_sync_schedule,
                tmdb_last_sync_ts,
                tmdb_fetch_posters,
                tmdb_fetch_backdrops,
                tmdb_fetch_metadata,
                tmdb_fetch_reviews,
                updated_ts,
            }
        },
    ))
}

pub async fn get_library_settings_for_libraries(
    pool: &DbPool,
    library_ids: &[String],
) -> Result<Vec<LibrarySettingsRow>, sqlx::Error> {
    if library_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = crate::repo::dollar_placeholders(1, library_ids.len());
    let sql = format!(
        "SELECT library_id, \
                (show_images <> 0) AS show_images, \
                (prefer_local_artwork <> 0) AS prefer_local_artwork, \
                (fetch_online_artwork <> 0) AS fetch_online_artwork, \
                (tmdb_store_in_media_dir <> 0) AS tmdb_store_in_media_dir, \
                (tmdb_sync_on_new_media <> 0) AS tmdb_sync_on_new_media, \
                tmdb_sync_schedule, \
                CAST(tmdb_last_sync_ts AS BIGINT) AS tmdb_last_sync_ts, \
                (tmdb_fetch_posters <> 0) AS tmdb_fetch_posters, \
                (tmdb_fetch_backdrops <> 0) AS tmdb_fetch_backdrops, \
                (tmdb_fetch_metadata <> 0) AS tmdb_fetch_metadata, \
                (tmdb_fetch_reviews <> 0) AS tmdb_fetch_reviews, \
                CAST(updated_ts AS BIGINT) AS updated_ts \
         FROM library_settings \
         WHERE library_id IN ({placeholders})"
    );

    let mut query = sqlx::query_as::<
        _,
        (
            String,
            bool,
            bool,
            bool,
            bool,
            bool,
            String,
            Option<i64>,
            bool,
            bool,
            bool,
            bool,
            i64,
        ),
    >(&sql);
    for library_id in library_ids {
        query = query.bind(library_id);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                library_id,
                show_images,
                prefer_local_artwork,
                fetch_online_artwork,
                tmdb_store_in_media_dir,
                tmdb_sync_on_new_media,
                tmdb_sync_schedule,
                tmdb_last_sync_ts,
                tmdb_fetch_posters,
                tmdb_fetch_backdrops,
                tmdb_fetch_metadata,
                tmdb_fetch_reviews,
                updated_ts,
            )| LibrarySettingsRow {
                library_id,
                show_images,
                prefer_local_artwork,
                fetch_online_artwork,
                tmdb_store_in_media_dir,
                tmdb_sync_on_new_media,
                tmdb_sync_schedule,
                tmdb_last_sync_ts,
                tmdb_fetch_posters,
                tmdb_fetch_backdrops,
                tmdb_fetch_metadata,
                tmdb_fetch_reviews,
                updated_ts,
            },
        )
        .collect())
}

pub async fn upsert_library_settings(
    pool: &DbPool,
    library_id: &str,
    show_images: bool,
    prefer_local_artwork: bool,
    fetch_online_artwork: bool,
    tmdb_store_in_media_dir: bool,
    tmdb_sync_on_new_media: bool,
    tmdb_sync_schedule: &str,
    tmdb_last_sync_ts: Option<i64>,
    tmdb_fetch_posters: bool,
    tmdb_fetch_backdrops: bool,
    tmdb_fetch_metadata: bool,
    tmdb_fetch_reviews: bool,
) -> Result<LibrarySettingsRow, sqlx::Error> {
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO library_settings \
         (library_id, show_images, prefer_local_artwork, fetch_online_artwork, \
          tmdb_store_in_media_dir, tmdb_sync_on_new_media, tmdb_sync_schedule, tmdb_last_sync_ts, \
          tmdb_fetch_posters, tmdb_fetch_backdrops, tmdb_fetch_metadata, tmdb_fetch_reviews, updated_ts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
         ON CONFLICT(library_id) DO UPDATE SET \
           show_images = excluded.show_images, \
           prefer_local_artwork = excluded.prefer_local_artwork, \
           fetch_online_artwork = excluded.fetch_online_artwork, \
           tmdb_store_in_media_dir = excluded.tmdb_store_in_media_dir, \
           tmdb_sync_on_new_media = excluded.tmdb_sync_on_new_media, \
           tmdb_sync_schedule = excluded.tmdb_sync_schedule, \
           tmdb_last_sync_ts = excluded.tmdb_last_sync_ts, \
           tmdb_fetch_posters = excluded.tmdb_fetch_posters, \
           tmdb_fetch_backdrops = excluded.tmdb_fetch_backdrops, \
           tmdb_fetch_metadata = excluded.tmdb_fetch_metadata, \
           tmdb_fetch_reviews = excluded.tmdb_fetch_reviews, \
           updated_ts = excluded.updated_ts",
    )
    .bind(library_id)
    .bind(bool_to_db_int(show_images))
    .bind(bool_to_db_int(prefer_local_artwork))
    .bind(bool_to_db_int(fetch_online_artwork))
    .bind(bool_to_db_int(tmdb_store_in_media_dir))
    .bind(bool_to_db_int(tmdb_sync_on_new_media))
    .bind(tmdb_sync_schedule)
    .bind(tmdb_last_sync_ts)
    .bind(bool_to_db_int(tmdb_fetch_posters))
    .bind(bool_to_db_int(tmdb_fetch_backdrops))
    .bind(bool_to_db_int(tmdb_fetch_metadata))
    .bind(bool_to_db_int(tmdb_fetch_reviews))
    .bind(now)
    .execute(pool)
    .await?;

    Ok(LibrarySettingsRow {
        library_id: library_id.to_string(),
        show_images,
        prefer_local_artwork,
        fetch_online_artwork,
        tmdb_store_in_media_dir,
        tmdb_sync_on_new_media,
        tmdb_sync_schedule: tmdb_sync_schedule.to_string(),
        tmdb_last_sync_ts,
        tmdb_fetch_posters,
        tmdb_fetch_backdrops,
        tmdb_fetch_metadata,
        tmdb_fetch_reviews,
        updated_ts: now,
    })
}

pub async fn touch_tmdb_last_sync_ts(
    pool: &DbPool,
    library_id: &str,
    last_sync_ts: i64,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE library_settings \
         SET tmdb_last_sync_ts = $1, updated_ts = $2 \
         WHERE library_id = $3",
    )
    .bind(last_sync_ts)
    .bind(now)
    .bind(library_id)
    .execute(pool)
    .await?;
    Ok(())
}
