use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};

const WEATHER_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const WEATHER_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const LOCATION_QUERY_MAX_CHARS: usize = 80;
const FORECAST_DAY_MIN: u8 = 1;
const FORECAST_DAY_MAX: u8 = 7;
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const WEATHER_USER_AGENT: &str = concat!("Rustyfin-AI-Weather/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize)]
pub struct PublicWeatherCurrentSummary {
    pub source: &'static str,
    pub location_query: String,
    pub resolved_location: String,
    pub timezone: String,
    pub observed_at: String,
    pub condition: String,
    pub temperature_c: f64,
    pub apparent_temperature_c: Option<f64>,
    pub humidity_percent: Option<f64>,
    pub wind_speed_kmh: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicWeatherForecastDay {
    pub date: String,
    pub condition: String,
    pub temperature_min_c: Option<f64>,
    pub temperature_max_c: Option<f64>,
    pub precipitation_probability_max_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicWeatherForecastSummary {
    pub source: &'static str,
    pub location_query: String,
    pub resolved_location: String,
    pub timezone: String,
    pub current: PublicWeatherCurrentSummary,
    pub forecast_days: Vec<PublicWeatherForecastDay>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    #[serde(default)]
    results: Vec<GeocodingResult>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResult {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    timezone: String,
    current: Option<ForecastCurrent>,
    daily: Option<ForecastDaily>,
}

#[derive(Debug, Deserialize)]
struct ForecastCurrent {
    time: String,
    temperature_2m: f64,
    #[serde(default)]
    apparent_temperature: Option<f64>,
    #[serde(default)]
    relative_humidity_2m: Option<f64>,
    #[serde(default)]
    weather_code: Option<i32>,
    #[serde(default)]
    wind_speed_10m: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ForecastDaily {
    #[serde(default)]
    time: Vec<String>,
    #[serde(default)]
    weather_code: Vec<Option<i32>>,
    #[serde(default)]
    temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

pub async fn fetch_public_weather_current(
    location_query: &str,
) -> Result<PublicWeatherCurrentSummary, String> {
    let query = normalize_location_query(location_query)?;
    let client = weather_client()?;
    let resolved = geocode_location(&client, &query).await?;
    let forecast = fetch_forecast(&client, &resolved, 1).await?;
    let current = build_current_summary(&query, &resolved, &forecast)?;
    Ok(current)
}

pub async fn fetch_public_weather_forecast(
    location_query: &str,
    forecast_days: Option<u8>,
) -> Result<PublicWeatherForecastSummary, String> {
    let query = normalize_location_query(location_query)?;
    let forecast_days = forecast_days
        .unwrap_or(3)
        .clamp(FORECAST_DAY_MIN, FORECAST_DAY_MAX);
    let client = weather_client()?;
    let resolved = geocode_location(&client, &query).await?;
    let forecast = fetch_forecast(&client, &resolved, forecast_days).await?;
    let current = build_current_summary(&query, &resolved, &forecast)?;
    let forecast_days = build_forecast_days(&forecast);
    Ok(PublicWeatherForecastSummary {
        source: "open_meteo",
        location_query: query,
        resolved_location: format_resolved_location(&resolved),
        timezone: forecast.timezone,
        current,
        forecast_days,
    })
}

fn weather_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(WEATHER_CONNECT_TIMEOUT)
        .timeout(WEATHER_REQUEST_TIMEOUT)
        .user_agent(WEATHER_USER_AGENT)
        .build()
        .map_err(|error| format!("failed to build public weather client: {error}"))
}

async fn geocode_location(
    client: &reqwest::Client,
    query: &str,
) -> Result<GeocodingResult, String> {
    let url = Url::parse_with_params(
        GEOCODING_URL,
        &[
            ("name", query),
            ("count", "5"),
            ("language", "en"),
            ("format", "json"),
        ],
    )
    .map_err(|error| format!("failed to build weather geocoding URL: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("weather geocoding request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "weather geocoding request failed with status {}",
            response.status()
        ));
    }
    let payload = response
        .json::<GeocodingResponse>()
        .await
        .map_err(|error| format!("failed to parse weather geocoding response: {error}"))?;
    payload
        .results
        .into_iter()
        .next()
        .ok_or_else(|| format!("no public weather location matched \"{query}\""))
}

async fn fetch_forecast(
    client: &reqwest::Client,
    location: &GeocodingResult,
    forecast_days: u8,
) -> Result<ForecastResponse, String> {
    let latitude = location.latitude.to_string();
    let longitude = location.longitude.to_string();
    let forecast_days = forecast_days.to_string();
    let url = Url::parse_with_params(
        FORECAST_URL,
        &[
            ("latitude", latitude.as_str()),
            ("longitude", longitude.as_str()),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m",
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
            ),
            ("forecast_days", forecast_days.as_str()),
            ("timezone", "auto"),
        ],
    )
    .map_err(|error| format!("failed to build weather forecast URL: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("weather forecast request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "weather forecast request failed with status {}",
            response.status()
        ));
    }
    response
        .json::<ForecastResponse>()
        .await
        .map_err(|error| format!("failed to parse weather forecast response: {error}"))
}

fn build_current_summary(
    location_query: &str,
    resolved: &GeocodingResult,
    forecast: &ForecastResponse,
) -> Result<PublicWeatherCurrentSummary, String> {
    let current = forecast
        .current
        .as_ref()
        .ok_or_else(|| "public weather response did not include current conditions".to_string())?;
    Ok(PublicWeatherCurrentSummary {
        source: "open_meteo",
        location_query: location_query.to_string(),
        resolved_location: format_resolved_location(resolved),
        timezone: forecast.timezone.clone(),
        observed_at: current.time.clone(),
        condition: weather_code_description(current.weather_code).to_string(),
        temperature_c: current.temperature_2m,
        apparent_temperature_c: current.apparent_temperature,
        humidity_percent: current.relative_humidity_2m,
        wind_speed_kmh: current.wind_speed_10m,
    })
}

fn build_forecast_days(forecast: &ForecastResponse) -> Vec<PublicWeatherForecastDay> {
    let Some(daily) = forecast.daily.as_ref() else {
        return Vec::new();
    };
    daily
        .time
        .iter()
        .enumerate()
        .map(|(index, date)| PublicWeatherForecastDay {
            date: date.clone(),
            condition: weather_code_description(daily.weather_code.get(index).copied().flatten())
                .to_string(),
            temperature_min_c: daily.temperature_2m_min.get(index).copied().flatten(),
            temperature_max_c: daily.temperature_2m_max.get(index).copied().flatten(),
            precipitation_probability_max_percent: daily
                .precipitation_probability_max
                .get(index)
                .copied()
                .flatten(),
        })
        .collect()
}

fn normalize_location_query(raw_query: &str) -> Result<String, String> {
    let query = raw_query.trim();
    if query.is_empty() {
        return Err("public weather location is required".to_string());
    }
    if query.chars().count() > LOCATION_QUERY_MAX_CHARS {
        return Err(format!(
            "public weather location must be <= {LOCATION_QUERY_MAX_CHARS} characters"
        ));
    }
    Ok(query.to_string())
}

fn format_resolved_location(location: &GeocodingResult) -> String {
    let mut parts = vec![location.name.trim().to_string()];
    if let Some(admin1) = location
        .admin1
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !parts.iter().any(|part| part.eq_ignore_ascii_case(admin1)) {
            parts.push(admin1.to_string());
        }
    }
    if let Some(country) = location
        .country
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !parts.iter().any(|part| part.eq_ignore_ascii_case(country)) {
            parts.push(country.to_string());
        }
    }
    parts.join(", ")
}

fn weather_code_description(code: Option<i32>) -> &'static str {
    match code.unwrap_or(-1) {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 | 77 => "Snow",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown conditions",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ForecastDaily, ForecastResponse, GeocodingResult, build_forecast_days,
        format_resolved_location, normalize_location_query, weather_code_description,
    };

    #[test]
    fn normalize_location_query_rejects_blank_values() {
        let error = normalize_location_query("   ").expect_err("expected validation error");
        assert!(error.contains("required"));
    }

    #[test]
    fn format_resolved_location_keeps_unique_parts() {
        let location = GeocodingResult {
            name: "Dublin".to_string(),
            latitude: 0.0,
            longitude: 0.0,
            country: Some("Ireland".to_string()),
            admin1: Some("Leinster".to_string()),
        };
        assert_eq!(
            format_resolved_location(&location),
            "Dublin, Leinster, Ireland"
        );
    }

    #[test]
    fn weather_code_description_maps_common_values() {
        assert_eq!(weather_code_description(Some(0)), "Clear sky");
        assert_eq!(weather_code_description(Some(63)), "Rain");
        assert_eq!(weather_code_description(Some(95)), "Thunderstorm");
    }

    #[test]
    fn build_forecast_days_zips_daily_arrays() {
        let forecast = ForecastResponse {
            timezone: "Europe/Dublin".to_string(),
            current: None,
            daily: Some(ForecastDaily {
                time: vec!["2026-03-15".to_string(), "2026-03-16".to_string()],
                weather_code: vec![Some(1), Some(63)],
                temperature_2m_max: vec![Some(11.0), Some(13.0)],
                temperature_2m_min: vec![Some(4.0), Some(7.0)],
                precipitation_probability_max: vec![Some(10.0), Some(80.0)],
            }),
        };
        let days = build_forecast_days(&forecast);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].condition, "Mainly clear");
        assert_eq!(days[1].condition, "Rain");
        assert_eq!(days[1].precipitation_probability_max_percent, Some(80.0));
    }
}
