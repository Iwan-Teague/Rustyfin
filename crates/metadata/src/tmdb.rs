//! TMDB (The Movie Database) provider client.
//!
//! Uses TMDB API v3: https://developer.themoviedb.org/docs

use tracing::debug;

use crate::provider::{MetadataProvider, SearchResult};
use crate::{EpisodeInfo, ItemMetadata, MetadataError, PersonInfo};

const BASE_URL: &str = "https://api.themoviedb.org/3";
const IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
}

impl TmdbClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    async fn get_json(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value, MetadataError> {
        let mut all_params = vec![("api_key", self.api_key.as_str())];
        all_params.extend_from_slice(params);

        let url = format!("{BASE_URL}{path}");
        debug!(url = %url, "TMDB request");

        let resp = self
            .client
            .get(&url)
            .query(&all_params)
            .send()
            .await
            .map_err(|e| MetadataError::Network(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(MetadataError::NotFound);
        }

        if !resp.status().is_success() {
            return Err(MetadataError::Provider(format!(
                "TMDB returned {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| MetadataError::Provider(format!("parse JSON: {e}")))
    }
}

#[async_trait::async_trait]
impl MetadataProvider for TmdbClient {
    fn name(&self) -> &str {
        "tmdb"
    }

    async fn search_movie(
        &self,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<SearchResult>, MetadataError> {
        let mut params = vec![("query", title)];
        let year_str = year.map(|y| y.to_string());
        if let Some(ref y) = year_str {
            params.push(("year", y));
        }

        let data = self.get_json("/search/movie", &params).await?;
        let results = data["results"].as_array().cloned().unwrap_or_default();

        Ok(results
            .iter()
            .take(10)
            .map(|r| SearchResult {
                provider_id: r["id"].as_u64().unwrap_or(0).to_string(),
                title: r["title"].as_str().unwrap_or("Unknown").to_string(),
                year: r["release_date"]
                    .as_str()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse().ok()),
                overview: r["overview"].as_str().map(|s| s.to_string()),
                poster_url: r["poster_path"]
                    .as_str()
                    .map(|p| format!("{IMAGE_BASE}/w500{p}")),
            })
            .collect())
    }

    async fn search_series(
        &self,
        title: &str,
        year: Option<i32>,
    ) -> Result<Vec<SearchResult>, MetadataError> {
        let mut params = vec![("query", title)];
        let year_str = year.map(|y| y.to_string());
        if let Some(ref y) = year_str {
            params.push(("first_air_date_year", y));
        }

        let data = self.get_json("/search/tv", &params).await?;
        let results = data["results"].as_array().cloned().unwrap_or_default();

        Ok(results
            .iter()
            .take(10)
            .map(|r| SearchResult {
                provider_id: r["id"].as_u64().unwrap_or(0).to_string(),
                title: r["name"].as_str().unwrap_or("Unknown").to_string(),
                year: r["first_air_date"]
                    .as_str()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse().ok()),
                overview: r["overview"].as_str().map(|s| s.to_string()),
                poster_url: r["poster_path"]
                    .as_str()
                    .map(|p| format!("{IMAGE_BASE}/w500{p}")),
            })
            .collect())
    }

    async fn get_movie(&self, provider_id: &str) -> Result<ItemMetadata, MetadataError> {
        let data = self
            .get_json(
                &format!("/movie/{provider_id}"),
                &[("append_to_response", "credits")],
            )
            .await?;

        Ok(parse_movie_metadata(&data))
    }

    async fn get_series(&self, provider_id: &str) -> Result<ItemMetadata, MetadataError> {
        let data = self
            .get_json(
                &format!("/tv/{provider_id}"),
                &[("append_to_response", "credits")],
            )
            .await?;

        Ok(parse_series_metadata(&data))
    }

    async fn get_season_episodes(
        &self,
        series_provider_id: &str,
        season_number: i32,
    ) -> Result<Vec<EpisodeInfo>, MetadataError> {
        let data = self
            .get_json(
                &format!("/tv/{series_provider_id}/season/{season_number}"),
                &[],
            )
            .await?;

        let episodes = data["episodes"].as_array().cloned().unwrap_or_default();

        Ok(episodes
            .iter()
            .map(|ep| EpisodeInfo {
                season_number: ep["season_number"].as_i64().unwrap_or(0) as i32,
                episode_number: ep["episode_number"].as_i64().unwrap_or(0) as i32,
                title: ep["name"].as_str().map(|s| s.to_string()),
                overview: ep["overview"].as_str().map(|s| s.to_string()),
                air_date: ep["air_date"].as_str().map(|s| s.to_string()),
                still_url: ep["still_path"]
                    .as_str()
                    .map(|p| format!("{IMAGE_BASE}/w300{p}")),
            })
            .collect())
    }
}

fn parse_movie_metadata(data: &serde_json::Value) -> ItemMetadata {
    let people = extract_credits(data.get("credits"));

    ItemMetadata {
        title: data["title"].as_str().map(|s| s.to_string()),
        original_title: data["original_title"].as_str().map(|s| s.to_string()),
        sort_title: data["title"].as_str().map(|s| s.to_string()),
        overview: data["overview"].as_str().map(|s| s.to_string()),
        tagline: data["tagline"].as_str().map(|s| s.to_string()),
        year: data["release_date"]
            .as_str()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok()),
        premiere_date: data["release_date"].as_str().map(|s| s.to_string()),
        end_date: None,
        runtime_minutes: data["runtime"].as_i64().map(|r| r as i32),
        community_rating: data["vote_average"].as_f64(),
        official_rating: None, // TMDB doesn't directly provide MPAA ratings in basic endpoint
        genres: data["genres"].as_array().map(|gs| {
            gs.iter()
                .filter_map(|g| g["name"].as_str().map(|s| s.to_string()))
                .collect()
        }),
        studios: data["production_companies"].as_array().map(|cs| {
            cs.iter()
                .filter_map(|c| c["name"].as_str().map(|s| s.to_string()))
                .collect()
        }),
        people: if people.is_empty() {
            None
        } else {
            Some(people)
        },
        poster_url: data["poster_path"]
            .as_str()
            .map(|p| format!("{IMAGE_BASE}/original{p}")),
        backdrop_url: data["backdrop_path"]
            .as_str()
            .map(|p| format!("{IMAGE_BASE}/original{p}")),
        logo_url: None,
        thumb_url: None,
    }
}

fn parse_series_metadata(data: &serde_json::Value) -> ItemMetadata {
    let people = extract_credits(data.get("credits"));

    ItemMetadata {
        title: data["name"].as_str().map(|s| s.to_string()),
        original_title: data["original_name"].as_str().map(|s| s.to_string()),
        sort_title: data["name"].as_str().map(|s| s.to_string()),
        overview: data["overview"].as_str().map(|s| s.to_string()),
        tagline: data["tagline"].as_str().map(|s| s.to_string()),
        year: data["first_air_date"]
            .as_str()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok()),
        premiere_date: data["first_air_date"].as_str().map(|s| s.to_string()),
        end_date: data["last_air_date"].as_str().map(|s| s.to_string()),
        runtime_minutes: data["episode_run_time"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_i64())
            .map(|r| r as i32),
        community_rating: data["vote_average"].as_f64(),
        official_rating: None,
        genres: data["genres"].as_array().map(|gs| {
            gs.iter()
                .filter_map(|g| g["name"].as_str().map(|s| s.to_string()))
                .collect()
        }),
        studios: data["production_companies"].as_array().map(|cs| {
            cs.iter()
                .filter_map(|c| c["name"].as_str().map(|s| s.to_string()))
                .collect()
        }),
        people: if people.is_empty() {
            None
        } else {
            Some(people)
        },
        poster_url: data["poster_path"]
            .as_str()
            .map(|p| format!("{IMAGE_BASE}/original{p}")),
        backdrop_url: data["backdrop_path"]
            .as_str()
            .map(|p| format!("{IMAGE_BASE}/original{p}")),
        logo_url: None,
        thumb_url: None,
    }
}

fn extract_credits(credits: Option<&serde_json::Value>) -> Vec<PersonInfo> {
    let mut people = Vec::new();

    if let Some(credits) = credits {
        // Cast
        if let Some(cast) = credits["cast"].as_array() {
            for person in cast.iter().take(20) {
                people.push(PersonInfo {
                    name: person["name"].as_str().unwrap_or("").to_string(),
                    role: "Actor".to_string(),
                    character: person["character"].as_str().map(|s| s.to_string()),
                    thumb_url: person["profile_path"]
                        .as_str()
                        .map(|p| format!("{IMAGE_BASE}/w185{p}")),
                });
            }
        }

        // Crew (directors only)
        if let Some(crew) = credits["crew"].as_array() {
            for person in crew {
                if person["job"].as_str() == Some("Director") {
                    people.push(PersonInfo {
                        name: person["name"].as_str().unwrap_or("").to_string(),
                        role: "Director".to_string(),
                        character: None,
                        thumb_url: person["profile_path"]
                            .as_str()
                            .map(|p| format!("{IMAGE_BASE}/w185{p}")),
                    });
                }
            }
        }
    }

    people
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_movie_metadata_from_json() {
        let json = serde_json::json!({
            "title": "Inception",
            "original_title": "Inception",
            "overview": "A thief who steals corporate secrets...",
            "tagline": "Your mind is the scene of the crime.",
            "release_date": "2010-07-16",
            "runtime": 148,
            "vote_average": 8.4,
            "poster_path": "/poster.jpg",
            "backdrop_path": "/backdrop.jpg",
            "genres": [
                { "id": 28, "name": "Action" },
                { "id": 878, "name": "Science Fiction" }
            ],
            "production_companies": [
                { "name": "Warner Bros." }
            ],
            "credits": {
                "cast": [
                    { "name": "Leonardo DiCaprio", "character": "Cobb", "profile_path": "/leo.jpg" }
                ],
                "crew": [
                    { "name": "Christopher Nolan", "job": "Director", "profile_path": "/nolan.jpg" }
                ]
            }
        });

        let meta = parse_movie_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("Inception"));
        assert_eq!(meta.year, Some(2010));
        assert_eq!(meta.runtime_minutes, Some(148));
        assert!((meta.community_rating.unwrap() - 8.4).abs() < 0.01);
        assert_eq!(meta.genres.as_ref().unwrap().len(), 2);
        assert!(meta.poster_url.as_ref().unwrap().contains("/poster.jpg"));

        let people = meta.people.unwrap();
        assert_eq!(people.len(), 2);
        assert_eq!(people[0].name, "Leonardo DiCaprio");
        assert_eq!(people[0].role, "Actor");
        assert_eq!(people[1].name, "Christopher Nolan");
        assert_eq!(people[1].role, "Director");
    }

    #[test]
    fn parse_series_metadata_from_json() {
        let json = serde_json::json!({
            "name": "Breaking Bad",
            "original_name": "Breaking Bad",
            "overview": "A high school chemistry teacher...",
            "first_air_date": "2008-01-20",
            "last_air_date": "2013-09-29",
            "vote_average": 9.5,
            "poster_path": "/bb.jpg",
            "genres": [
                { "name": "Drama" }
            ]
        });

        let meta = parse_series_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("Breaking Bad"));
        assert_eq!(meta.year, Some(2008));
        assert_eq!(meta.end_date.as_deref(), Some("2013-09-29"));
    }
}
