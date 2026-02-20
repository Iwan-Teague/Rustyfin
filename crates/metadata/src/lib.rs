#![allow(clippy::type_complexity)]
pub mod merge;
pub mod provider;
pub mod tmdb;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("not found")]
    NotFound,
    #[error("db error: {0}")]
    Db(#[from] sqlx::Error),
}

/// Metadata fields that can be set on an item.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemMetadata {
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub sort_title: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
    pub year: Option<i32>,
    pub premiere_date: Option<String>,
    pub end_date: Option<String>,
    pub runtime_minutes: Option<i32>,
    pub community_rating: Option<f64>,
    pub official_rating: Option<String>, // MPAA, BBFC, etc.
    pub genres: Option<Vec<String>>,
    pub studios: Option<Vec<String>>,
    pub people: Option<Vec<PersonInfo>>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub logo_url: Option<String>,
    pub thumb_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersonInfo {
    pub name: String,
    pub role: String, // "Actor", "Director", etc.
    pub character: Option<String>,
    pub thumb_url: Option<String>,
}

/// Episode metadata from a provider (for expected episodes).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpisodeInfo {
    pub season_number: i32,
    pub episode_number: i32,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub still_url: Option<String>,
}
