use sqlx::SqlitePool;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExpectedEpisodeRow {
    pub series_id: String,
    pub season_number: i32,
    pub episode_number: i32,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
}

/// Insert or update an expected episode.
pub async fn upsert_expected_episode(
    pool: &SqlitePool,
    series_id: &str,
    season_number: i32,
    episode_number: i32,
    title: Option<&str>,
    overview: Option<&str>,
    air_date: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO episode_expected (series_id, season_number, episode_number, title, overview, air_date) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(series_id, season_number, episode_number) DO UPDATE SET \
         title = COALESCE(excluded.title, title), \
         overview = COALESCE(excluded.overview, overview), \
         air_date = COALESCE(excluded.air_date, air_date)",
    )
    .bind(series_id)
    .bind(season_number)
    .bind(episode_number)
    .bind(title)
    .bind(overview)
    .bind(air_date)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get all expected episodes for a series.
pub async fn get_expected_episodes(
    pool: &SqlitePool,
    series_id: &str,
) -> Result<Vec<ExpectedEpisodeRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        i32,
        i32,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT series_id, season_number, episode_number, title, overview, air_date \
             FROM episode_expected WHERE series_id = ? \
             ORDER BY season_number, episode_number",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ExpectedEpisodeRow {
            series_id: r.0,
            season_number: r.1,
            episode_number: r.2,
            title: r.3,
            overview: r.4,
            air_date: r.5,
        })
        .collect())
}

/// Get expected episodes for a specific season.
pub async fn get_season_expected(
    pool: &SqlitePool,
    series_id: &str,
    season_number: i32,
) -> Result<Vec<ExpectedEpisodeRow>, sqlx::Error> {
    let rows: Vec<(
        String,
        i32,
        i32,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT series_id, season_number, episode_number, title, overview, air_date \
             FROM episode_expected WHERE series_id = ? AND season_number = ? \
             ORDER BY episode_number",
    )
    .bind(series_id)
    .bind(season_number)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ExpectedEpisodeRow {
            series_id: r.0,
            season_number: r.1,
            episode_number: r.2,
            title: r.3,
            overview: r.4,
            air_date: r.5,
        })
        .collect())
}

/// Get present episode numbers for a series (from actual items).
pub async fn get_present_episodes(
    pool: &SqlitePool,
    series_id: &str,
) -> Result<Vec<(i32, i32)>, sqlx::Error> {
    // Episodes: kind='episode', parent=season, season.parent=series.
    // Season titles are "Season X", episode titles contain "SxxExx" or similar.
    // Use SQL to extract season/episode numbers.
    let rows: Vec<(i32, i32)> = sqlx::query_as(
        "SELECT CAST(REPLACE(LOWER(season_item.title), 'season ', '') AS INTEGER) as snum, \
         CAST(SUBSTR(ep_item.title, INSTR(ep_item.title, 'E') + 1) AS INTEGER) as epnum \
         FROM item ep_item \
         JOIN item season_item ON ep_item.parent_id = season_item.id \
         WHERE season_item.parent_id = ? AND ep_item.kind = 'episode' \
         AND season_item.title LIKE 'Season %'",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Ok(rows)
}

/// Compare expected vs present episodes. Returns missing episode info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MissingEpisode {
    pub season_number: i32,
    pub episode_number: i32,
    pub title: Option<String>,
    pub air_date: Option<String>,
}

pub async fn get_missing_episodes(
    pool: &SqlitePool,
    series_id: &str,
) -> Result<Vec<MissingEpisode>, sqlx::Error> {
    let expected = get_expected_episodes(pool, series_id).await?;
    let present = get_present_episodes(pool, series_id).await?;

    let missing: Vec<MissingEpisode> = expected
        .into_iter()
        .filter(|ep| {
            !present
                .iter()
                .any(|(s, e)| *s == ep.season_number && *e == ep.episode_number)
        })
        .map(|ep| MissingEpisode {
            season_number: ep.season_number,
            episode_number: ep.episode_number,
            title: ep.title,
            air_date: ep.air_date,
        })
        .collect();

    Ok(missing)
}
