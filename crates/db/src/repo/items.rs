use crate::DbPool;

#[derive(Debug, Clone)]
pub struct ItemRow {
    pub id: String,
    pub library_id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub sort_title: Option<String>,
    pub year: Option<i64>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub logo_url: Option<String>,
    pub thumb_url: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub duration_ms: Option<i64>,
}

pub async fn get_item(pool: &DbPool, item_id: &str) -> Result<Option<ItemRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, library_id, kind, parent_id, title, sort_title, year, overview, \
         poster_url, backdrop_url, logo_url, thumb_url, \
         created_ts, updated_ts FROM item WHERE id = $1",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_item))
}

pub async fn get_children(pool: &DbPool, parent_id: &str) -> Result<Vec<ItemRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT i.id, i.library_id, i.kind, i.parent_id, i.title, i.sort_title, i.year, \
         i.overview, i.poster_url, i.backdrop_url, i.logo_url, i.thumb_url, \
         i.created_ts, i.updated_ts, \
         (SELECT mf2.duration_ms FROM episode_file_map efm2 \
          JOIN media_file mf2 ON mf2.id = efm2.file_id \
          WHERE efm2.episode_item_id = i.id LIMIT 1) AS duration_ms \
         FROM item i \
         WHERE i.parent_id = $1 ORDER BY i.title",
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_item_full).collect())
}

pub async fn get_library_items(
    pool: &DbPool,
    library_id: &str,
) -> Result<Vec<ItemRow>, sqlx::Error> {
    // Return top-level items (no parent) for the library
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, library_id, kind, parent_id, title, sort_title, year, overview, \
         poster_url, backdrop_url, logo_url, thumb_url, \
         created_ts, updated_ts FROM item \
         WHERE library_id = $1 AND parent_id IS NULL ORDER BY title",
    )
    .bind(library_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_item).collect())
}

/// Get the media file ID associated with an item (via episode_file_map).
pub async fn get_item_file_id(pool: &DbPool, item_id: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT file_id FROM episode_file_map WHERE episode_item_id = $1 LIMIT 1")
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| id))
}

/// Get an item ID for a media file.
pub async fn get_item_id_by_file_id(
    pool: &DbPool,
    file_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT episode_item_id FROM episode_file_map WHERE file_id = $1 LIMIT 1")
            .bind(file_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| id))
}

pub async fn get_item_media_path(
    pool: &DbPool,
    item_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT mf.path \
         FROM episode_file_map ef \
         JOIN media_file mf ON mf.id = ef.file_id \
         WHERE ef.episode_item_id = $1 \
         LIMIT 1",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(path,)| path))
}

pub async fn get_first_descendant_media_path(
    pool: &DbPool,
    item_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "WITH RECURSIVE descendants(id, depth) AS (
            SELECT id, 0 FROM item WHERE id = $1
            UNION ALL
            SELECT i.id, d.depth + 1
            FROM item i
            JOIN descendants d ON i.parent_id = d.id
         )
         SELECT mf.path
         FROM descendants d
         JOIN episode_file_map ef ON ef.episode_item_id = d.id
         JOIN media_file mf ON mf.id = ef.file_id
         ORDER BY d.depth ASC
         LIMIT 1",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(path,)| path))
}

pub async fn get_item_artwork(
    pool: &DbPool,
    item_id: &str,
) -> Result<
    Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )>,
    sqlx::Error,
> {
    sqlx::query_as("SELECT poster_url, backdrop_url, logo_url, thumb_url FROM item WHERE id = $1")
        .bind(item_id)
        .fetch_optional(pool)
        .await
}

pub async fn update_item_artwork(
    pool: &DbPool,
    item_id: &str,
    poster_url: Option<&str>,
    backdrop_url: Option<&str>,
    logo_url: Option<&str>,
    thumb_url: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE item
         SET poster_url = $1, backdrop_url = $2, logo_url = $3, thumb_url = $4, updated_ts = $5
         WHERE id = $6",
    )
    .bind(poster_url)
    .bind(backdrop_url)
    .bind(logo_url)
    .bind(thumb_url)
    .bind(chrono::Utc::now().timestamp())
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn row_to_item(
    r: (
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
    ),
) -> ItemRow {
    ItemRow {
        id: r.0,
        library_id: r.1,
        kind: r.2,
        parent_id: r.3,
        title: r.4,
        sort_title: r.5,
        year: r.6,
        overview: r.7,
        poster_url: r.8,
        backdrop_url: r.9,
        logo_url: r.10,
        thumb_url: r.11,
        created_ts: r.12,
        updated_ts: r.13,
        duration_ms: None,
    }
}

fn row_to_item_full(
    r: (
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        i64,
        Option<i64>,
    ),
) -> ItemRow {
    ItemRow {
        id: r.0,
        library_id: r.1,
        kind: r.2,
        parent_id: r.3,
        title: r.4,
        sort_title: r.5,
        year: r.6,
        overview: r.7,
        poster_url: r.8,
        backdrop_url: r.9,
        logo_url: r.10,
        thumb_url: r.11,
        created_ts: r.12,
        updated_ts: r.13,
        duration_ms: r.14,
    }
}

/// Get an image URL for an item by type (poster, backdrop, logo, thumb).
pub async fn get_item_image_url(
    pool: &DbPool,
    item_id: &str,
    image_type: &str,
) -> Result<Option<String>, sqlx::Error> {
    let col = match image_type {
        "poster" => "poster_url",
        "backdrop" => "backdrop_url",
        "logo" => "logo_url",
        "thumb" => "thumb_url",
        _ => return Ok(None),
    };
    let query = format!("SELECT {col} FROM item WHERE id = $1");
    let row: Option<(Option<String>,)> = sqlx::query_as(&query)
        .bind(item_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|(url,)| url))
}
