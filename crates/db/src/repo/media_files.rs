use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct MediaFileRow {
    pub id: String,
    pub path: String,
    pub size_bytes: i64,
    pub mtime_ts: i64,
    pub container: Option<String>,
    pub duration_ms: Option<i64>,
    pub stream_info_json: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
}

pub async fn get_media_file(
    pool: &SqlitePool,
    file_id: &str,
) -> Result<Option<MediaFileRow>, sqlx::Error> {
    let row: Option<(
        String,
        String,
        i64,
        i64,
        Option<String>,
        Option<i64>,
        Option<String>,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT id, path, size_bytes, mtime_ts, container, duration_ms, stream_info_json, \
         created_ts, updated_ts FROM media_file WHERE id = ?",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| MediaFileRow {
        id: r.0,
        path: r.1,
        size_bytes: r.2,
        mtime_ts: r.3,
        container: r.4,
        duration_ms: r.5,
        stream_info_json: r.6,
        created_ts: r.7,
        updated_ts: r.8,
    }))
}
