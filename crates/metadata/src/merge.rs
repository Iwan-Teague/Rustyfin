//! Metadata merge engine.
//!
//! Merge rules:
//! 1. User edits (locked fields) always win.
//! 2. Provider metadata fills in blanks.
//! 3. Multiple providers: first non-null wins (priority order).

use sqlx::SqlitePool;
use tracing::debug;

use crate::ItemMetadata;

/// Merge provider metadata into an item, respecting field locks.
///
/// Returns the merged metadata and which fields were updated.
pub async fn merge_metadata(
    pool: &SqlitePool,
    item_id: &str,
    provider_meta: &ItemMetadata,
) -> Result<MergeResult, sqlx::Error> {
    // Get locked fields for this item
    let locked = get_locked_fields(pool, item_id).await?;

    // Get current metadata from item
    let current = get_current_metadata(pool, item_id).await?;

    let mut merged = current.clone();
    let mut updated_fields = Vec::new();

    // Merge each field if not locked and provider has a value
    macro_rules! merge_field {
        ($field:ident) => {
            if provider_meta.$field.is_some()
                && !locked.contains(&stringify!($field).to_string())
            {
                if current.$field.is_none()
                    || current.$field != provider_meta.$field
                {
                    merged.$field = provider_meta.$field.clone();
                    updated_fields.push(stringify!($field).to_string());
                }
            }
        };
    }

    merge_field!(title);
    merge_field!(original_title);
    merge_field!(sort_title);
    merge_field!(overview);
    merge_field!(tagline);
    merge_field!(year);
    merge_field!(premiere_date);
    merge_field!(end_date);
    merge_field!(runtime_minutes);
    merge_field!(community_rating);
    merge_field!(official_rating);
    merge_field!(genres);
    merge_field!(studios);
    merge_field!(people);
    merge_field!(poster_url);
    merge_field!(backdrop_url);
    merge_field!(logo_url);
    merge_field!(thumb_url);

    if !updated_fields.is_empty() {
        save_metadata(pool, item_id, &merged).await?;
        debug!(item_id, ?updated_fields, "merged metadata");
    }

    Ok(MergeResult {
        metadata: merged,
        updated_fields,
    })
}

#[derive(Debug, Clone)]
pub struct MergeResult {
    pub metadata: ItemMetadata,
    pub updated_fields: Vec<String>,
}

/// Lock a field for an item (user override).
pub async fn lock_field(
    pool: &SqlitePool,
    item_id: &str,
    field_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO item_field_lock (item_id, field, locked, locked_ts) \
         VALUES (?, ?, 1, ?)",
    )
    .bind(item_id)
    .bind(field_name)
    .bind(chrono::Utc::now().timestamp())
    .execute(pool)
    .await?;
    Ok(())
}

/// Unlock a field for an item.
pub async fn unlock_field(
    pool: &SqlitePool,
    item_id: &str,
    field_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM item_field_lock WHERE item_id = ? AND field = ?")
        .bind(item_id)
        .bind(field_name)
        .execute(pool)
        .await?;
    Ok(())
}

async fn get_locked_fields(pool: &SqlitePool, item_id: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT field FROM item_field_lock WHERE item_id = ?")
            .bind(item_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn get_current_metadata(
    pool: &SqlitePool,
    item_id: &str,
) -> Result<ItemMetadata, sqlx::Error> {
    let row: Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT title, sort_title, overview, tagline, year, premiere_date, \
         community_rating, poster_url, backdrop_url \
         FROM item WHERE id = ?",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => Ok(ItemMetadata {
            title: r.0,
            sort_title: r.1,
            overview: r.2,
            tagline: r.3,
            year: r.4.map(|y| y as i32),
            premiere_date: r.5,
            community_rating: r.6,
            poster_url: r.7,
            backdrop_url: r.8,
            ..Default::default()
        }),
        None => Ok(ItemMetadata::default()),
    }
}

async fn save_metadata(
    pool: &SqlitePool,
    item_id: &str,
    meta: &ItemMetadata,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE item SET \
         title = COALESCE(?, title), \
         sort_title = COALESCE(?, sort_title), \
         overview = ?, \
         tagline = ?, \
         year = COALESCE(?, year), \
         premiere_date = ?, \
         community_rating = ?, \
         poster_url = ?, \
         backdrop_url = ?, \
         updated_ts = ? \
         WHERE id = ?",
    )
    .bind(&meta.title)
    .bind(&meta.sort_title)
    .bind(&meta.overview)
    .bind(&meta.tagline)
    .bind(meta.year)
    .bind(&meta.premiere_date)
    .bind(meta.community_rating)
    .bind(&meta.poster_url)
    .bind(&meta.backdrop_url)
    .bind(chrono::Utc::now().timestamp())
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Store a provider ID for an item.
pub async fn set_provider_id(
    pool: &SqlitePool,
    item_id: &str,
    provider: &str,
    provider_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO item_provider_id (item_id, provider, value) \
         VALUES (?, ?, ?) \
         ON CONFLICT(item_id, provider) DO UPDATE SET value = excluded.value",
    )
    .bind(item_id)
    .bind(provider)
    .bind(provider_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get provider IDs for an item.
pub async fn get_provider_ids(
    pool: &SqlitePool,
    item_id: &str,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT provider, value FROM item_provider_id WHERE item_id = ?")
            .bind(item_id)
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn merge_respects_locked_fields() {
        let pool = rustfin_db::connect(":memory:").await.unwrap();
        rustfin_db::migrate::run(&pool).await.unwrap();

        // Create a library first (FK requirement)
        sqlx::query(
            "INSERT INTO library (id, name, kind, created_ts, updated_ts) \
             VALUES ('lib1', 'Test', 'movies', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Create a test item
        let item_id = "test-item-1";
        sqlx::query(
            "INSERT INTO item (id, library_id, kind, title, sort_title, year, created_ts, updated_ts) \
             VALUES (?, 'lib1', 'movie', 'Original Title', 'original title', 2020, 0, 0)",
        )
        .bind(item_id)
        .execute(&pool)
        .await
        .unwrap();

        // Lock the title field
        lock_field(&pool, item_id, "title").await.unwrap();

        // Try to merge new metadata
        let provider_meta = ItemMetadata {
            title: Some("New Title From Provider".into()),
            overview: Some("A great movie".into()),
            year: Some(2021),
            ..Default::default()
        };

        let result = merge_metadata(&pool, item_id, &provider_meta).await.unwrap();

        // Title should NOT be updated (locked)
        assert_eq!(result.metadata.title.as_deref(), Some("Original Title"));
        assert!(!result.updated_fields.contains(&"title".to_string()));

        // Overview should be updated (not locked)
        assert_eq!(result.metadata.overview.as_deref(), Some("A great movie"));
        assert!(result.updated_fields.contains(&"overview".to_string()));
    }

    #[tokio::test]
    async fn provider_ids_crud() {
        let pool = rustfin_db::connect(":memory:").await.unwrap();
        rustfin_db::migrate::run(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO library (id, name, kind, created_ts, updated_ts) \
             VALUES ('lib1', 'Test', 'movies', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let item_id = "test-item-2";
        sqlx::query(
            "INSERT INTO item (id, library_id, kind, title, sort_title, created_ts, updated_ts) \
             VALUES (?, 'lib1', 'movie', 'Test', 'test', 0, 0)",
        )
        .bind(item_id)
        .execute(&pool)
        .await
        .unwrap();

        set_provider_id(&pool, item_id, "tmdb", "12345").await.unwrap();
        set_provider_id(&pool, item_id, "imdb", "tt1234567").await.unwrap();

        let ids = get_provider_ids(&pool, item_id).await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|(p, id)| p == "tmdb" && id == "12345"));
        assert!(ids.iter().any(|(p, id)| p == "imdb" && id == "tt1234567"));

        // Update existing
        set_provider_id(&pool, item_id, "tmdb", "99999").await.unwrap();
        let ids = get_provider_ids(&pool, item_id).await.unwrap();
        assert!(ids.iter().any(|(p, id)| p == "tmdb" && id == "99999"));
    }
}
