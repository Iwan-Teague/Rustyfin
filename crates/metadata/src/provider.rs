use crate::{EpisodeInfo, ItemMetadata, MetadataError};

/// A metadata provider that can search and fetch metadata.
#[async_trait::async_trait]
pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &str;

    /// Search for a movie by title and optional year.
    async fn search_movie(
        &self,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<SearchResult>, MetadataError>;

    /// Search for a TV series by title.
    async fn search_series(
        &self,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<SearchResult>, MetadataError>;

    /// Get full metadata for a movie by provider ID.
    async fn get_movie(&self, provider_id: &str) -> Result<ItemMetadata, MetadataError>;

    /// Get full metadata for a TV series by provider ID.
    async fn get_series(&self, provider_id: &str) -> Result<ItemMetadata, MetadataError>;

    /// Get season metadata (episode list) for a TV series.
    async fn get_season_episodes(
        &self,
        series_provider_id: &str,
        season_number: i32,
    ) -> Result<Vec<EpisodeInfo>, MetadataError>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub provider_id: String,
    pub title: String,
    pub year: Option<i32>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
}
