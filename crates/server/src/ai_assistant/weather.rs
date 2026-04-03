use std::{collections::HashSet, time::Duration};

use chrono::NaiveDate;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::types::{
    AssistantToolContextBlock, decode_assistant_clarification_message,
    encode_assistant_clarification_message,
};

const WEATHER_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const WEATHER_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const LOCATION_QUERY_MAX_CHARS: usize = 80;
const FORECAST_DAY_MIN: u8 = 1;
const FORECAST_DAY_MAX: u8 = 7;
const HISTORY_DAY_MAX: i64 = 92;
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const WEATHER_USER_AGENT: &str = concat!("Rustyfin-AI-Weather/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWeatherCurrentSummary {
    pub source: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWeatherForecastDay {
    pub date: String,
    pub condition: String,
    pub temperature_min_c: Option<f64>,
    pub temperature_max_c: Option<f64>,
    pub precipitation_probability_max_percent: Option<f64>,
    pub precipitation_sum_mm: Option<f64>,
    pub precipitation_hours: Option<f64>,
    pub rain_sum_mm: Option<f64>,
    pub showers_sum_mm: Option<f64>,
    pub snowfall_sum_cm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWeatherForecastSummary {
    pub source: String,
    pub location_query: String,
    pub resolved_location: String,
    pub timezone: String,
    pub current: PublicWeatherCurrentSummary,
    pub forecast_days: Vec<PublicWeatherForecastDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicWeatherHistorySummary {
    pub source: String,
    pub location_query: String,
    pub resolved_location: String,
    pub timezone: String,
    pub from_date: String,
    pub to_date: String,
    pub history_days: Vec<PublicWeatherForecastDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicLocationTimezoneSummary {
    pub source: String,
    pub location_query: String,
    pub resolved_location: String,
    pub timezone: String,
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    #[serde(default)]
    results: Vec<GeocodingResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct GeocodingResult {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    timezone: Option<String>,
    country: Option<String>,
    admin1: Option<String>,
    #[serde(default)]
    admin2: Option<String>,
    #[serde(default)]
    admin3: Option<String>,
    #[serde(default)]
    admin4: Option<String>,
}

#[derive(Debug)]
enum GeocodingSelection {
    Match(GeocodingResult),
    Ambiguous(Vec<GeocodingResult>),
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
    #[serde(default)]
    precipitation_sum: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_hours: Vec<Option<f64>>,
    #[serde(default)]
    rain_sum: Vec<Option<f64>>,
    #[serde(default)]
    showers_sum: Vec<Option<f64>>,
    #[serde(default)]
    snowfall_sum: Vec<Option<f64>>,
}

pub async fn fetch_public_weather_current(
    location_query: &str,
) -> Result<PublicWeatherCurrentSummary, String> {
    let client = weather_client()?;
    fetch_public_weather_current_with_endpoints(
        &client,
        location_query,
        GEOCODING_URL,
        FORECAST_URL,
    )
    .await
}

pub async fn fetch_public_weather_forecast(
    location_query: &str,
    forecast_days: Option<u8>,
) -> Result<PublicWeatherForecastSummary, String> {
    let client = weather_client()?;
    fetch_public_weather_forecast_with_endpoints(
        &client,
        location_query,
        forecast_days,
        GEOCODING_URL,
        FORECAST_URL,
    )
    .await
}

pub async fn fetch_public_weather_history(
    location_query: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<PublicWeatherHistorySummary, String> {
    let client = weather_client()?;
    fetch_public_weather_history_with_endpoints(
        &client,
        location_query,
        start_date,
        end_date,
        GEOCODING_URL,
        FORECAST_URL,
    )
    .await
}

pub async fn resolve_public_location_timezone(
    location_query: &str,
) -> Result<PublicLocationTimezoneSummary, String> {
    let client = weather_client()?;
    let query = normalize_location_query(location_query)?;
    let resolved = geocode_location_from_base(&client, &query, GEOCODING_URL).await?;
    let timezone = resolved
        .timezone
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("no public timezone matched \"{query}\""))?;
    Ok(PublicLocationTimezoneSummary {
        source: "open_meteo".to_string(),
        location_query: query,
        resolved_location: format_resolved_location(&resolved),
        timezone,
    })
}

async fn fetch_public_weather_current_with_endpoints(
    client: &reqwest::Client,
    location_query: &str,
    geocoding_url: &str,
    forecast_url: &str,
) -> Result<PublicWeatherCurrentSummary, String> {
    let query = normalize_location_query(location_query)?;
    let resolved = geocode_location_from_base(client, &query, geocoding_url).await?;
    let forecast = fetch_forecast_from_base(client, &resolved, 1, forecast_url).await?;
    build_current_summary(&query, &resolved, &forecast)
}

async fn fetch_public_weather_forecast_with_endpoints(
    client: &reqwest::Client,
    location_query: &str,
    forecast_days: Option<u8>,
    geocoding_url: &str,
    forecast_url: &str,
) -> Result<PublicWeatherForecastSummary, String> {
    let query = normalize_location_query(location_query)?;
    let forecast_days = forecast_days
        .unwrap_or(3)
        .clamp(FORECAST_DAY_MIN, FORECAST_DAY_MAX);
    let resolved = geocode_location_from_base(client, &query, geocoding_url).await?;
    let forecast = fetch_forecast_from_base(client, &resolved, forecast_days, forecast_url).await?;
    let current = build_current_summary(&query, &resolved, &forecast)?;
    let forecast_days = build_forecast_days(&forecast);
    Ok(PublicWeatherForecastSummary {
        source: "open_meteo".to_string(),
        location_query: query,
        resolved_location: format_resolved_location(&resolved),
        timezone: forecast.timezone,
        current,
        forecast_days,
    })
}

async fn fetch_public_weather_history_with_endpoints(
    client: &reqwest::Client,
    location_query: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    geocoding_url: &str,
    forecast_url: &str,
) -> Result<PublicWeatherHistorySummary, String> {
    if end_date < start_date {
        return Err(
            "public weather history end date must not be before the start date".to_string(),
        );
    }
    if (end_date - start_date).num_days() > HISTORY_DAY_MAX {
        return Err(format!(
            "public weather history is limited to the last {HISTORY_DAY_MAX} days"
        ));
    }

    let query = normalize_location_query(location_query)?;
    let resolved = geocode_location_from_base(client, &query, geocoding_url).await?;
    let forecast =
        fetch_history_from_base(client, &resolved, start_date, end_date, forecast_url).await?;
    let history_days = build_forecast_days(&forecast);
    Ok(PublicWeatherHistorySummary {
        source: "open_meteo".to_string(),
        location_query: query,
        resolved_location: format_resolved_location(&resolved),
        timezone: forecast.timezone,
        from_date: start_date.format("%F").to_string(),
        to_date: end_date.format("%F").to_string(),
        history_days,
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

async fn geocode_location_from_base(
    client: &reqwest::Client,
    query: &str,
    geocoding_url: &str,
) -> Result<GeocodingResult, String> {
    let mut ambiguous_matches: Option<Vec<GeocodingResult>> = None;

    for candidate in geocoding_query_variants(query) {
        let url = Url::parse_with_params(
            geocoding_url,
            &[
                ("name", candidate.as_str()),
                ("count", "10"),
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
        match select_geocoding_result(query, payload.results) {
            Some(GeocodingSelection::Match(result)) => return Ok(result),
            Some(GeocodingSelection::Ambiguous(options)) => {
                if ambiguous_matches.is_none() {
                    ambiguous_matches = Some(options);
                }
            }
            None => {}
        }
    }

    if let Some(options) = ambiguous_matches {
        return Err(encode_assistant_clarification_message(
            &format_ambiguous_location_message(query, &options),
        ));
    }

    Err(format!("no public weather location matched \"{query}\""))
}

async fn fetch_forecast_from_base(
    client: &reqwest::Client,
    location: &GeocodingResult,
    forecast_days: u8,
    forecast_url: &str,
) -> Result<ForecastResponse, String> {
    let latitude = location.latitude.to_string();
    let longitude = location.longitude.to_string();
    let forecast_days = forecast_days.to_string();
    let url = Url::parse_with_params(
        forecast_url,
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

async fn fetch_history_from_base(
    client: &reqwest::Client,
    location: &GeocodingResult,
    start_date: NaiveDate,
    end_date: NaiveDate,
    forecast_url: &str,
) -> Result<ForecastResponse, String> {
    let latitude = location.latitude.to_string();
    let longitude = location.longitude.to_string();
    let start_date = start_date.format("%F").to_string();
    let end_date = end_date.format("%F").to_string();
    let url = Url::parse_with_params(
        forecast_url,
        &[
            ("latitude", latitude.as_str()),
            ("longitude", longitude.as_str()),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,precipitation_sum,precipitation_hours,rain_sum,showers_sum,snowfall_sum",
            ),
            ("start_date", start_date.as_str()),
            ("end_date", end_date.as_str()),
            ("timezone", "auto"),
        ],
    )
    .map_err(|error| format!("failed to build weather history URL: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("weather history request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "weather history request failed with status {}",
            response.status()
        ));
    }
    response
        .json::<ForecastResponse>()
        .await
        .map_err(|error| format!("failed to parse weather history response: {error}"))
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
        source: "open_meteo".to_string(),
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
            precipitation_sum_mm: daily.precipitation_sum.get(index).copied().flatten(),
            precipitation_hours: daily.precipitation_hours.get(index).copied().flatten(),
            rain_sum_mm: daily.rain_sum.get(index).copied().flatten(),
            showers_sum_mm: daily.showers_sum.get(index).copied().flatten(),
            snowfall_sum_cm: daily.snowfall_sum.get(index).copied().flatten(),
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

fn geocoding_query_variants(query: &str) -> Vec<String> {
    let mut variants = Vec::new();
    push_unique_variant(&mut variants, query.trim());
    push_unique_variant(
        &mut variants,
        &replace_case_insensitive(query, " in county ", ", County "),
    );
    push_unique_variant(
        &mut variants,
        &replace_case_insensitive(query, " in ", ", "),
    );
    push_unique_variant(
        &mut variants,
        &replace_case_insensitive(query, "County ", ""),
    );
    push_unique_variant(
        &mut variants,
        &replace_case_insensitive(
            &replace_case_insensitive(query, " in county ", ", "),
            "County ",
            "",
        ),
    );
    if let Some(core_name) = query
        .split(',')
        .next()
        .map(str::trim)
        .and_then(|segment| segment.split(" in ").next())
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        push_unique_variant(&mut variants, core_name);
    }
    push_progressive_prefix_variants(&mut variants, query);
    variants
}

fn push_unique_variant(variants: &mut Vec<String>, candidate: &str) {
    let normalized = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || variants.iter().any(|value| value == &normalized) {
        return;
    }
    variants.push(normalized);
}

fn push_progressive_prefix_variants(variants: &mut Vec<String>, query: &str) {
    let tokens = query
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() <= 2 {
        return;
    }

    for keep in (1..tokens.len()).rev() {
        push_unique_variant(variants, &tokens[..keep].join(" "));
    }
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if let Some(index) = lower.find(&needle_lower) {
        let end = index + needle.len();
        format!("{}{}{}", &haystack[..index], replacement, &haystack[end..])
    } else {
        haystack.to_string()
    }
}

#[cfg(test)]
fn select_best_geocoding_result(
    query: &str,
    results: Vec<GeocodingResult>,
) -> Option<GeocodingResult> {
    match select_geocoding_result(query, results) {
        Some(GeocodingSelection::Match(result)) => Some(result),
        _ => None,
    }
}

fn select_geocoding_result(
    query: &str,
    results: Vec<GeocodingResult>,
) -> Option<GeocodingSelection> {
    if results.is_empty() {
        return None;
    }

    let query_text = normalize_location_text(query);
    let mut ranked = results
        .into_iter()
        .map(|result| (geocoding_match_score(query, &result), result))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0));

    if let Some((_, result)) = ranked
        .iter()
        .find(|(_, result)| is_exact_country_query_match(&query_text, result))
    {
        return Some(GeocodingSelection::Match(result.clone()));
    }

    let exact_name_matches = unique_geocoding_results(
        ranked
            .iter()
            .filter(|(_, result)| normalize_location_text(&result.name) == query_text)
            .map(|(_, result)| result.clone())
            .collect(),
    );
    if exact_name_matches.len() > 1 {
        return Some(GeocodingSelection::Ambiguous(exact_name_matches));
    }

    ranked
        .into_iter()
        .next()
        .map(|(_, result)| GeocodingSelection::Match(result))
}

fn is_exact_country_query_match(query_text: &str, result: &GeocodingResult) -> bool {
    let country = normalize_location_text(result.country.as_deref().unwrap_or_default());
    !country.is_empty() && query_text == country
}

fn unique_geocoding_results(results: Vec<GeocodingResult>) -> Vec<GeocodingResult> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for result in results {
        let key = format_resolved_location(&result).to_ascii_lowercase();
        if seen.insert(key) {
            unique.push(result);
        }
    }
    unique
}

fn format_ambiguous_location_message(query: &str, options: &[GeocodingResult]) -> String {
    let choices = options
        .iter()
        .take(5)
        .map(format_resolved_location)
        .collect::<Vec<_>>()
        .join("; ");
    format!("I found multiple locations matching \"{query}\": {choices}. Which one did you mean?")
}

fn geocoding_match_score(query: &str, result: &GeocodingResult) -> i32 {
    let query_text = normalize_location_text(query);
    let name = normalize_location_text(&result.name);
    let country = normalize_location_text(result.country.as_deref().unwrap_or_default());
    let combined = normalize_location_text(&format!(
        "{} {} {} {} {} {}",
        result.name,
        result.admin1.as_deref().unwrap_or_default(),
        result.admin2.as_deref().unwrap_or_default(),
        result.admin3.as_deref().unwrap_or_default(),
        result.admin4.as_deref().unwrap_or_default(),
        result.country.as_deref().unwrap_or_default()
    ));
    let result_tokens = location_tokens(&combined);

    let mut score = 0;
    if !name.is_empty() && query_text == name {
        score += 200;
    }
    if !country.is_empty() && query_text == country {
        score += 220;
        if name == country {
            score += 120;
        }
    }
    if !name.is_empty() && query_text.contains(&name) {
        score += 80;
    }
    if !name.is_empty() && name.contains(&query_text) {
        score += 40;
    }

    for token in location_tokens(&query_text) {
        if result_tokens.contains(&token) {
            score += if name.split_whitespace().any(|part| part == token) {
                25
            } else {
                12
            };
        } else {
            score -= 8;
        }
    }

    score
}

fn normalize_location_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == ' ' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn location_tokens(value: &str) -> HashSet<&str> {
    value
        .split_whitespace()
        .filter(|token| {
            !matches!(
                *token,
                "the" | "in" | "county" | "co" | "co." | "province" | "state" | "region"
            )
        })
        .collect()
}

pub fn deterministic_weather_reply(
    message: &str,
    grounding_blocks: &[AssistantToolContextBlock],
) -> Option<String> {
    if grounding_blocks.len() != 1 {
        return None;
    }
    let block = grounding_blocks.first()?;
    match block.tool {
        "weather_get_current" => Some(if block.status == "ok" {
            format_current_reply(
                message,
                serde_json::from_value::<PublicWeatherCurrentSummary>(block.data.clone()).ok()?,
            )
        } else {
            format_weather_error(block)
        }),
        "weather_get_forecast" => Some(if block.status == "ok" {
            format_forecast_reply(
                message,
                serde_json::from_value::<PublicWeatherForecastSummary>(block.data.clone()).ok()?,
            )
        } else {
            format_weather_error(block)
        }),
        "weather_get_history" => Some(if block.status == "ok" {
            format_history_reply(
                message,
                serde_json::from_value::<PublicWeatherHistorySummary>(block.data.clone()).ok()?,
            )
        } else {
            format_weather_error(block)
        }),
        _ => None,
    }
}

fn format_resolved_location(location: &GeocodingResult) -> String {
    let mut parts = vec![location.name.trim().to_string()];
    if let Some(admin2) = location
        .admin2
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !parts.iter().any(|part| part.eq_ignore_ascii_case(admin2)) {
            parts.push(admin2.to_string());
        }
    }
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

fn format_weather_error(block: &AssistantToolContextBlock) -> String {
    if block.status == "clarification" {
        return block
            .data
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                block
                    .data
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .and_then(decode_assistant_clarification_message)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Which location did you mean?".to_string());
    }

    block
        .data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(|message| {
            decode_assistant_clarification_message(message)
                .map(str::to_string)
                .unwrap_or_else(|| format!("I couldn't load that weather data: {message}."))
        })
        .unwrap_or_else(|| "I couldn't load that weather data.".to_string())
}

fn format_current_reply(message: &str, current: PublicWeatherCurrentSummary) -> String {
    let mut parts = vec![format!(
        "Current weather in {}: {}, {}.",
        current.resolved_location,
        current.condition,
        format_temperature(current.temperature_c)
    )];
    if let Some(apparent) = current.apparent_temperature_c {
        parts.push(format!("Feels like {}.", format_temperature(apparent)));
    }
    if let Some(humidity) = current.humidity_percent {
        parts.push(format!("Humidity is {}.", format_percent(humidity)));
    }
    if let Some(wind) = current.wind_speed_kmh {
        parts.push(format!("Wind is {} km/h.", format_decimal(wind)));
    }
    if message.to_ascii_lowercase().contains("rain")
        && !current.condition.to_ascii_lowercase().contains("rain")
    {
        parts.push("There is no grounded sign of rain in the current conditions.".to_string());
    }
    parts.join(" ")
}

fn format_forecast_reply(message: &str, forecast: PublicWeatherForecastSummary) -> String {
    let lower = message.to_ascii_lowercase();
    if forecast.forecast_days.is_empty() {
        return format!(
            "I loaded the forecast for {}, but the provider did not return any daily forecast rows.",
            forecast.resolved_location
        );
    }

    let focus_day = if lower.contains("tomorrow") && forecast.forecast_days.len() >= 2 {
        &forecast.forecast_days[1]
    } else {
        &forecast.forecast_days[0]
    };

    if lower.contains("rain") || lower.contains("raining") || lower.contains("precip") {
        let probability = focus_day
            .precipitation_probability_max_percent
            .unwrap_or(0.0);
        return format!(
            "{} in {} on {}. Condition: {}. High {}, low {}. Precipitation probability up to {}.",
            rain_probability_sentence(probability),
            forecast.resolved_location,
            focus_day.date,
            focus_day.condition,
            optional_temperature(focus_day.temperature_max_c),
            optional_temperature(focus_day.temperature_min_c),
            format_percent(probability)
        );
    }

    if forecast.forecast_days.len() == 1 {
        return format!(
            "Forecast for {} on {}: {}. High {}, low {}. Precipitation probability up to {}.",
            forecast.resolved_location,
            focus_day.date,
            focus_day.condition,
            optional_temperature(focus_day.temperature_max_c),
            optional_temperature(focus_day.temperature_min_c),
            format_percent(
                focus_day
                    .precipitation_probability_max_percent
                    .unwrap_or(0.0)
            )
        );
    }

    let warmest = forecast
        .forecast_days
        .iter()
        .filter_map(|day| day.temperature_max_c)
        .fold(None::<f64>, |acc, value| {
            Some(acc.map_or(value, |best| best.max(value)))
        });
    let coldest = forecast
        .forecast_days
        .iter()
        .filter_map(|day| day.temperature_min_c)
        .fold(None::<f64>, |acc, value| {
            Some(acc.map_or(value, |best| best.min(value)))
        });
    let wettest = forecast.forecast_days.iter().max_by(|left, right| {
        left.precipitation_probability_max_percent
            .unwrap_or(0.0)
            .total_cmp(&right.precipitation_probability_max_percent.unwrap_or(0.0))
    });
    let wettest_summary = wettest.map(|day| {
        format!(
            "Highest rain signal is {} on {}.",
            format_percent(day.precipitation_probability_max_percent.unwrap_or(0.0)),
            day.date
        )
    });

    format!(
        "Forecast for {} over the next {} days: highs up to {}, lows down to {}. {} {}",
        forecast.resolved_location,
        forecast.forecast_days.len(),
        optional_temperature(warmest),
        optional_temperature(coldest),
        wettest_summary.unwrap_or_else(|| "No precipitation signal was returned.".to_string()),
        format!(
            "The first day shown is {} on {}.",
            forecast.forecast_days[0].condition, forecast.forecast_days[0].date
        )
    )
}

fn format_history_reply(message: &str, history: PublicWeatherHistorySummary) -> String {
    let lower = message.to_ascii_lowercase();
    let Some(day) = history.history_days.first() else {
        return format!(
            "I loaded weather history for {}, but the provider did not return any daily history rows.",
            history.resolved_location
        );
    };

    if lower.contains("rain") || lower.contains("raining") || lower.contains("precip") {
        let precipitation = total_precipitation_mm(day);
        if precipitation > 0.0 || day.precipitation_hours.unwrap_or(0.0) > 0.0 {
            return format!(
                "Yes. It rained in {} on {}: {} total precipitation over {} hours. Condition: {}. High {}, low {}.",
                history.resolved_location,
                day.date,
                format_mm(precipitation),
                format_hours(day.precipitation_hours.unwrap_or(0.0)),
                day.condition,
                optional_temperature(day.temperature_max_c),
                optional_temperature(day.temperature_min_c)
            );
        }
        return format!(
            "No grounded rain is indicated for {} on {}. Condition: {}. High {}, low {}.",
            history.resolved_location,
            day.date,
            day.condition,
            optional_temperature(day.temperature_max_c),
            optional_temperature(day.temperature_min_c)
        );
    }

    format!(
        "Weather in {} on {}: {}. High {}, low {}. Total precipitation {} over {} hours.",
        history.resolved_location,
        day.date,
        day.condition,
        optional_temperature(day.temperature_max_c),
        optional_temperature(day.temperature_min_c),
        format_mm(total_precipitation_mm(day)),
        format_hours(day.precipitation_hours.unwrap_or(0.0))
    )
}

fn total_precipitation_mm(day: &PublicWeatherForecastDay) -> f64 {
    day.precipitation_sum_mm
        .or(day.rain_sum_mm)
        .or(day.showers_sum_mm)
        .unwrap_or(0.0)
}

fn rain_probability_sentence(probability: f64) -> &'static str {
    if probability >= 60.0 {
        "Rain looks likely"
    } else if probability >= 25.0 {
        "Rain is possible"
    } else {
        "Rain looks unlikely"
    }
}

fn optional_temperature(value: Option<f64>) -> String {
    value
        .map(format_temperature)
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_temperature(value: f64) -> String {
    format!("{}C", format_decimal(value))
}

fn format_percent(value: f64) -> String {
    format!("{}%", format_decimal(value))
}

fn format_mm(value: f64) -> String {
    format!("{} mm", format_decimal(value))
}

fn format_hours(value: f64) -> String {
    format!("{} hours", format_decimal(value))
}

fn format_decimal(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded.fract()).abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ForecastDaily, ForecastResponse, GeocodingResult, GeocodingSelection,
        PublicWeatherCurrentSummary, PublicWeatherForecastDay, PublicWeatherForecastSummary,
        PublicWeatherHistorySummary, build_forecast_days, deterministic_weather_reply,
        fetch_public_weather_current_with_endpoints, fetch_public_weather_forecast_with_endpoints,
        format_resolved_location, geocoding_query_variants, normalize_location_query,
        select_best_geocoding_result, select_geocoding_result, weather_client,
        weather_code_description,
    };
    use axum::{
        Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Clone)]
    struct WeatherTestState {
        geocode_status: StatusCode,
        geocode_body: serde_json::Value,
        forecast_status: StatusCode,
        forecast_body: String,
    }

    async fn spawn_weather_test_server(
        state: WeatherTestState,
    ) -> (String, tokio::task::JoinHandle<()>) {
        async fn geocode_handler(
            State(state): State<Arc<Mutex<WeatherTestState>>>,
        ) -> impl IntoResponse {
            let guard = state.lock().await;
            (guard.geocode_status, Json(guard.geocode_body.clone()))
        }

        async fn forecast_handler(
            State(state): State<Arc<Mutex<WeatherTestState>>>,
        ) -> impl IntoResponse {
            let guard = state.lock().await;
            (
                guard.forecast_status,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                guard.forecast_body.clone(),
            )
        }

        let state = Arc::new(Mutex::new(state));
        let app = Router::new()
            .route("/geocode", get(geocode_handler))
            .route("/forecast", get(forecast_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

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
            timezone: None,
            country: Some("Ireland".to_string()),
            admin1: Some("Leinster".to_string()),
            admin2: None,
            admin3: None,
            admin4: None,
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
                precipitation_sum: vec![Some(0.0), Some(4.2)],
                precipitation_hours: vec![Some(0.0), Some(5.0)],
                rain_sum: vec![Some(0.0), Some(4.2)],
                showers_sum: vec![Some(0.0), Some(0.0)],
                snowfall_sum: vec![Some(0.0), Some(0.0)],
            }),
        };
        let days = build_forecast_days(&forecast);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].condition, "Mainly clear");
        assert_eq!(days[1].condition, "Rain");
        assert_eq!(days[1].precipitation_probability_max_percent, Some(80.0));
        assert_eq!(days[1].precipitation_sum_mm, Some(4.2));
    }

    #[test]
    fn geocoding_query_variants_rewrite_county_locations() {
        let variants = geocoding_query_variants("Campile in County Wexford, Ireland");
        assert!(variants.contains(&"Campile in County Wexford, Ireland".to_string()));
        assert!(variants.contains(&"Campile, County Wexford, Ireland".to_string()));
        assert!(variants.contains(&"Campile, Wexford, Ireland".to_string()));
        assert!(variants.contains(&"Campile".to_string()));
    }

    #[test]
    fn geocoding_query_variants_reduce_multi_part_follow_up_to_core_name() {
        let variants = geocoding_query_variants("Cork Muster Ireland");
        assert!(variants.contains(&"Cork Muster Ireland".to_string()));
        assert!(variants.contains(&"Cork Muster".to_string()));
        assert!(variants.contains(&"Cork".to_string()));
    }

    #[test]
    fn geocoding_result_selection_prefers_matching_admin_area() {
        let selected = select_best_geocoding_result(
            "Campile, Wexford, Ireland",
            vec![
                GeocodingResult {
                    name: "Campile".to_string(),
                    latitude: 0.0,
                    longitude: 0.0,
                    timezone: None,
                    country: Some("Ireland".to_string()),
                    admin1: Some("Leinster".to_string()),
                    admin2: Some("Wexford".to_string()),
                    admin3: None,
                    admin4: None,
                },
                GeocodingResult {
                    name: "Campile".to_string(),
                    latitude: 1.0,
                    longitude: 1.0,
                    timezone: None,
                    country: Some("Ireland".to_string()),
                    admin1: Some("Munster".to_string()),
                    admin2: Some("Cork".to_string()),
                    admin3: None,
                    admin4: None,
                },
            ],
        )
        .expect("expected geocoding selection");
        assert_eq!(selected.admin2.as_deref(), Some("Wexford"));
    }

    #[test]
    fn geocoding_result_selection_prefers_exact_country_match_for_country_query() {
        let selected = select_best_geocoding_result(
            "Italy",
            vec![
                GeocodingResult {
                    name: "Italy".to_string(),
                    latitude: 42.0,
                    longitude: -77.0,
                    timezone: Some("America/New_York".to_string()),
                    country: Some("United States".to_string()),
                    admin1: Some("New York".to_string()),
                    admin2: Some("Yates".to_string()),
                    admin3: None,
                    admin4: None,
                },
                GeocodingResult {
                    name: "Italy".to_string(),
                    latitude: 41.8719,
                    longitude: 12.5674,
                    timezone: Some("Europe/Rome".to_string()),
                    country: Some("Italy".to_string()),
                    admin1: None,
                    admin2: None,
                    admin3: None,
                    admin4: None,
                },
            ],
        )
        .expect("expected geocoding selection");
        assert_eq!(selected.country.as_deref(), Some("Italy"));
        assert_eq!(selected.timezone.as_deref(), Some("Europe/Rome"));
    }

    #[test]
    fn geocoding_result_selection_prefers_country_match_for_typoed_region_query() {
        let selected = select_best_geocoding_result(
            "Cork Muster Ireland",
            vec![
                GeocodingResult {
                    name: "Cork".to_string(),
                    latitude: 51.8985,
                    longitude: -8.4756,
                    timezone: Some("Europe/Dublin".to_string()),
                    country: Some("Ireland".to_string()),
                    admin1: Some("Munster".to_string()),
                    admin2: Some("County Cork".to_string()),
                    admin3: None,
                    admin4: None,
                },
                GeocodingResult {
                    name: "Cork".to_string(),
                    latitude: 33.1040,
                    longitude: -83.9690,
                    timezone: Some("America/New_York".to_string()),
                    country: Some("United States".to_string()),
                    admin1: Some("Georgia".to_string()),
                    admin2: Some("Butts".to_string()),
                    admin3: None,
                    admin4: None,
                },
            ],
        )
        .expect("expected geocoding selection");
        assert_eq!(selected.country.as_deref(), Some("Ireland"));
    }

    #[test]
    fn geocoding_result_selection_marks_bare_same_name_queries_as_ambiguous() {
        let selection = select_geocoding_result(
            "Galway",
            vec![
                GeocodingResult {
                    name: "Galway".to_string(),
                    latitude: 53.2707,
                    longitude: -9.0568,
                    timezone: Some("Europe/Dublin".to_string()),
                    country: Some("Ireland".to_string()),
                    admin1: Some("Connacht".to_string()),
                    admin2: None,
                    admin3: None,
                    admin4: None,
                },
                GeocodingResult {
                    name: "Galway".to_string(),
                    latitude: 43.0167,
                    longitude: -73.8462,
                    timezone: Some("America/New_York".to_string()),
                    country: Some("United States".to_string()),
                    admin1: Some("New York".to_string()),
                    admin2: Some("Saratoga".to_string()),
                    admin3: None,
                    admin4: None,
                },
            ],
        )
        .expect("expected geocoding selection");

        match selection {
            GeocodingSelection::Ambiguous(options) => {
                let labels = options
                    .iter()
                    .map(format_resolved_location)
                    .collect::<Vec<_>>();
                assert!(labels.contains(&"Galway, Connacht, Ireland".to_string()));
                assert!(labels.contains(&"Galway, Saratoga, New York, United States".to_string()));
            }
            GeocodingSelection::Match(_) => panic!("expected ambiguity for bare Galway query"),
        }
    }

    #[test]
    fn deterministic_weather_reply_uses_grounded_forecast_data() {
        let reply = deterministic_weather_reply(
            "Will it rain in Galway today?",
            &[super::AssistantToolContextBlock {
                tool: "weather_get_forecast",
                label: "1-day weather forecast for Galway, Connacht, Ireland".to_string(),
                status: "ok",
                data: serde_json::to_value(PublicWeatherForecastSummary {
                    source: "open_meteo".to_string(),
                    location_query: "Galway".to_string(),
                    resolved_location: "Galway, Connacht, Ireland".to_string(),
                    timezone: "Europe/Dublin".to_string(),
                    current: PublicWeatherCurrentSummary {
                        source: "open_meteo".to_string(),
                        location_query: "Galway".to_string(),
                        resolved_location: "Galway, Connacht, Ireland".to_string(),
                        timezone: "Europe/Dublin".to_string(),
                        observed_at: "2026-04-02T10:00".to_string(),
                        condition: "Cloudy".to_string(),
                        temperature_c: 11.0,
                        apparent_temperature_c: Some(10.0),
                        humidity_percent: Some(84.0),
                        wind_speed_kmh: Some(12.0),
                    },
                    forecast_days: vec![PublicWeatherForecastDay {
                        date: "2026-04-02".to_string(),
                        condition: "Rain".to_string(),
                        temperature_min_c: Some(7.0),
                        temperature_max_c: Some(12.0),
                        precipitation_probability_max_percent: Some(80.0),
                        precipitation_sum_mm: Some(5.0),
                        precipitation_hours: Some(6.0),
                        rain_sum_mm: Some(5.0),
                        showers_sum_mm: Some(0.0),
                        snowfall_sum_cm: Some(0.0),
                    }],
                })
                .unwrap(),
            }],
        )
        .expect("expected deterministic reply");
        assert!(reply.contains("Rain looks likely"));
        assert!(reply.contains("Galway, Connacht, Ireland"));
    }

    #[test]
    fn deterministic_weather_reply_uses_grounded_history_data() {
        let reply = deterministic_weather_reply(
            "Did it rain yesterday in Galway?",
            &[super::AssistantToolContextBlock {
                tool: "weather_get_history",
                label: "Recent weather history for Galway, Connacht, Ireland".to_string(),
                status: "ok",
                data: serde_json::to_value(PublicWeatherHistorySummary {
                    source: "open_meteo".to_string(),
                    location_query: "Galway".to_string(),
                    resolved_location: "Galway, Connacht, Ireland".to_string(),
                    timezone: "Europe/Dublin".to_string(),
                    from_date: "2026-04-01".to_string(),
                    to_date: "2026-04-01".to_string(),
                    history_days: vec![PublicWeatherForecastDay {
                        date: "2026-04-01".to_string(),
                        condition: "Rain".to_string(),
                        temperature_min_c: Some(6.0),
                        temperature_max_c: Some(11.0),
                        precipitation_probability_max_percent: Some(95.0),
                        precipitation_sum_mm: Some(7.8),
                        precipitation_hours: Some(8.0),
                        rain_sum_mm: Some(7.8),
                        showers_sum_mm: Some(0.0),
                        snowfall_sum_cm: Some(0.0),
                    }],
                })
                .unwrap(),
            }],
        )
        .expect("expected deterministic history reply");
        assert!(reply.contains("Yes. It rained"));
        assert!(reply.contains("7.8 mm"));
    }

    #[tokio::test]
    async fn weather_current_reports_geocoding_status_failures() {
        let (base, handle) = spawn_weather_test_server(WeatherTestState {
            geocode_status: StatusCode::SERVICE_UNAVAILABLE,
            geocode_body: json!({ "error": true }),
            forecast_status: StatusCode::OK,
            forecast_body: "{}".to_string(),
        })
        .await;
        let client = weather_client().unwrap();
        let error = fetch_public_weather_current_with_endpoints(
            &client,
            "Dublin",
            &format!("{base}/geocode"),
            &format!("{base}/forecast"),
        )
        .await
        .expect_err("expected downstream geocoding failure");
        assert!(error.contains("weather geocoding request failed with status 503"));
        handle.abort();
    }

    #[tokio::test]
    async fn weather_forecast_reports_invalid_downstream_json() {
        let (base, handle) = spawn_weather_test_server(WeatherTestState {
            geocode_status: StatusCode::OK,
            geocode_body: json!({
                "results": [{
                    "name": "Dublin",
                    "latitude": 53.3498,
                    "longitude": -6.2603,
                    "country": "Ireland",
                    "admin1": "Leinster"
                }]
            }),
            forecast_status: StatusCode::OK,
            forecast_body: "{not valid json".to_string(),
        })
        .await;
        let client = weather_client().unwrap();
        let error = fetch_public_weather_forecast_with_endpoints(
            &client,
            "Dublin",
            Some(2),
            &format!("{base}/geocode"),
            &format!("{base}/forecast"),
        )
        .await
        .expect_err("expected invalid forecast JSON failure");
        assert!(error.contains("failed to parse weather forecast response"));
        handle.abort();
    }

    #[tokio::test]
    async fn weather_current_reports_missing_current_conditions() {
        let (base, handle) = spawn_weather_test_server(WeatherTestState {
            geocode_status: StatusCode::OK,
            geocode_body: json!({
                "results": [{
                    "name": "Dublin",
                    "latitude": 53.3498,
                    "longitude": -6.2603,
                    "country": "Ireland",
                    "admin1": "Leinster"
                }]
            }),
            forecast_status: StatusCode::OK,
            forecast_body: json!({
                "timezone": "Europe/Dublin",
                "daily": {
                    "time": ["2026-03-16"],
                    "weather_code": [1],
                    "temperature_2m_max": [10.0],
                    "temperature_2m_min": [3.0],
                    "precipitation_probability_max": [10.0]
                }
            })
            .to_string(),
        })
        .await;
        let client = weather_client().unwrap();
        let error = fetch_public_weather_current_with_endpoints(
            &client,
            "Dublin",
            &format!("{base}/geocode"),
            &format!("{base}/forecast"),
        )
        .await
        .expect_err("expected missing current conditions failure");
        assert!(error.contains("did not include current conditions"));
        handle.abort();
    }

    #[tokio::test]
    async fn weather_current_requests_clarification_for_ambiguous_same_name_location() {
        let (base, handle) = spawn_weather_test_server(WeatherTestState {
            geocode_status: StatusCode::OK,
            geocode_body: json!({
                "results": [
                    {
                        "name": "Galway",
                        "latitude": 53.2707,
                        "longitude": -9.0568,
                        "country": "Ireland",
                        "admin1": "Connacht",
                        "timezone": "Europe/Dublin"
                    },
                    {
                        "name": "Galway",
                        "latitude": 43.0167,
                        "longitude": -73.8462,
                        "country": "United States",
                        "admin1": "New York",
                        "admin2": "Saratoga",
                        "timezone": "America/New_York"
                    }
                ]
            }),
            forecast_status: StatusCode::OK,
            forecast_body: "{}".to_string(),
        })
        .await;
        let client = weather_client().unwrap();
        let error = fetch_public_weather_current_with_endpoints(
            &client,
            "Galway",
            &format!("{base}/geocode"),
            &format!("{base}/forecast"),
        )
        .await
        .expect_err("expected ambiguity clarification");
        assert!(error.contains("clarification:"));
        assert!(error.contains("multiple locations matching \"Galway\""));
        assert!(error.contains("Galway, Connacht, Ireland"));
        assert!(error.contains("Galway, Saratoga, New York, United States"));
        handle.abort();
    }
}
