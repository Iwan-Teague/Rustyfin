use chrono::{TimeZone, Utc};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tracing::warn;

use super::context::AssistantContext;
use super::memory_selector::query_terms;
use super::replies::compact_text;
use super::types::{
    AssistantGroundingChunk, AssistantGroundingCitation, AssistantGroundingVisibility,
};
use crate::state::AppState;

const TRANSCRIPT_HIT_LIMIT: i64 = 5;
const DOWNLOAD_HIT_LIMIT: i64 = 4;
const LIBRARY_HIT_LIMIT: i64 = 5;
const ERROR_HIT_LIMIT: i64 = 4;

fn stable_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(b"|");
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"|");
    }
    let digest = hasher.finalize();
    format!("{prefix}:{}", hex::encode(&digest[..16]))
}

fn dollar_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn query_text(query: Option<&str>) -> Option<String> {
    query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn make_citation(
    source_kind: &str,
    source_id: &str,
    source_sub_id: Option<&str>,
    label: Option<&str>,
    excerpt: Option<&str>,
    started_ts_ms: Option<i64>,
    ended_ts_ms: Option<i64>,
    url: Option<&str>,
) -> AssistantGroundingCitation {
    AssistantGroundingCitation {
        citation_id: stable_id(
            "cite",
            &[
                source_kind,
                source_id,
                source_sub_id.unwrap_or(""),
                label.unwrap_or(""),
                excerpt.unwrap_or(""),
            ],
        ),
        source_kind: source_kind.to_string(),
        source_id: source_id.to_string(),
        source_sub_id: source_sub_id.map(str::to_string),
        label: label.map(str::to_string),
        excerpt: excerpt.map(str::to_string),
        started_ts_ms,
        ended_ts_ms,
        url: url.map(str::to_string),
    }
}

fn make_chunk(
    source_kind: &str,
    title: String,
    excerpt: String,
    score: f64,
    visibility: AssistantGroundingVisibility,
    topic_key: Option<String>,
    owner_user_id: Option<String>,
    source_id: &str,
    source_sub_id: Option<String>,
    citation: AssistantGroundingCitation,
) -> AssistantGroundingChunk {
    let id = stable_id(
        "grounding",
        &[
            source_kind,
            source_id,
            source_sub_id.as_deref().unwrap_or(""),
            topic_key.as_deref().unwrap_or(""),
        ],
    );

    AssistantGroundingChunk {
        id,
        source_kind: source_kind.to_string(),
        title,
        excerpt,
        score,
        visibility,
        topic_key,
        owner_user_id,
        source_id: Some(source_id.to_string()),
        source_sub_id,
        citation: Some(citation),
    }
}

fn topic_family(topic_key: Option<&str>) -> Option<&str> {
    topic_key.and_then(|value| value.split(':').next())
}

fn looks_like_broad_overview(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "anything",
        "changed",
        "lately",
        "recently",
        "what changed",
        "what happened",
        "anything new",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn wants_transcripts(message: &str, topic_key: Option<&str>) -> bool {
    topic_family(topic_key) == Some("transcript")
        || ["transcript", "call", "voice", "discussed", "said"]
            .iter()
            .any(|needle| message.contains(needle))
}

fn wants_downloads(message: &str, topic_key: Option<&str>) -> bool {
    topic_family(topic_key) == Some("downloads")
        || ["download", "artifact", "extension", "package"]
            .iter()
            .any(|needle| message.contains(needle))
}

fn wants_libraries(message: &str, topic_key: Option<&str>) -> bool {
    topic_family(topic_key).is_some_and(|family| {
        family == "library" || family == "libraries" || family == "library_query"
    }) || [
        "library",
        "libraries",
        "movie",
        "show",
        "album",
        "track",
        "media",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn wants_recent_errors(message: &str, topic_key: Option<&str>) -> bool {
    topic_key == Some("admin:recent_errors")
        || ["error", "errors", "failing", "failure", "broken", "problem"]
            .iter()
            .any(|needle| message.contains(needle))
}

fn transcript_window_label(
    started_ts_ms: i64,
    ended_ts_ms: i64,
    session_started_ts: i64,
) -> String {
    let session_started_ms = session_started_ts.saturating_mul(1000);
    let relative_start = (started_ts_ms - session_started_ms).max(0);
    let relative_end = (ended_ts_ms - session_started_ms).max(relative_start);
    format!(
        "{}-{}",
        format_relative_ms(relative_start),
        format_relative_ms(relative_end)
    )
}

fn format_relative_ms(relative_ms: i64) -> String {
    let total_seconds = (relative_ms.max(0) / 1000) as i64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn transcript_session_date(started_ts: i64) -> String {
    Utc.timestamp_opt(started_ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown date".to_string())
}

async fn search_transcript_chunks(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let channels = rustfin_db::repo::channels::list_channels(&state.db).await?;
    let accessible_channel_ids: Vec<_> = channels
        .into_iter()
        .filter(|channel| channel.kind.eq_ignore_ascii_case("voice"))
        .filter(|channel| !channel.is_private || context.is_admin)
        .map(|channel| channel.id)
        .collect();
    if accessible_channel_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = dollar_placeholders(2, accessible_channel_ids.len());
    let sql = format!(
        "SELECT e.id, e.session_id, e.channel_id, e.username, e.started_ts_ms, e.ended_ts_ms, e.text, \
                s.started_ts AS session_started_ts, c.name AS channel_name, \
                ts_rank_cd( \
                    to_tsvector('simple', COALESCE(e.text, '') || ' ' || COALESCE(e.username, '') || ' ' || COALESCE(c.name, '')), \
                    websearch_to_tsquery('simple', $1) \
                ) AS rank \
         FROM channel_transcript_entry e \
         JOIN channel_transcript_session s ON s.id = e.session_id \
         JOIN channel c ON c.id = e.channel_id \
         WHERE s.status = 'completed' \
           AND e.channel_id IN ({placeholders}) \
           AND to_tsvector('simple', COALESCE(e.text, '') || ' ' || COALESCE(e.username, '') || ' ' || COALESCE(c.name, '')) \
               @@ websearch_to_tsquery('simple', $1) \
         ORDER BY rank DESC, e.started_ts_ms DESC \
         LIMIT ${}",
        accessible_channel_ids.len() + 2
    );

    let mut db_query = sqlx::query(&sql).bind(query);
    for channel_id in &accessible_channel_ids {
        db_query = db_query.bind(channel_id);
    }
    let rows = db_query
        .bind(TRANSCRIPT_HIT_LIMIT)
        .fetch_all(&state.db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let entry_id: String = row.get("id");
            let session_id: String = row.get("session_id");
            let channel_id: String = row.get("channel_id");
            let channel_name: String = row.get("channel_name");
            let username: String = row.get("username");
            let started_ts_ms: i64 = row.get("started_ts_ms");
            let ended_ts_ms: i64 = row.get("ended_ts_ms");
            let session_started_ts: i64 = row.get("session_started_ts");
            let text: String = row.get("text");
            let rank: f64 = row.get("rank");
            let excerpt = compact_text(&text, 260);
            let label = format!(
                "{channel_name} · {} [{}]",
                transcript_session_date(session_started_ts),
                transcript_window_label(started_ts_ms, ended_ts_ms, session_started_ts)
            );
            make_chunk(
                "transcript_excerpt",
                format!("{label} · {username}"),
                excerpt.clone(),
                1.6 + rank,
                AssistantGroundingVisibility::User,
                Some(format!("transcript:{channel_id}")),
                Some(context.user_id.clone()),
                &session_id,
                Some(entry_id.clone()),
                make_citation(
                    "transcript_excerpt",
                    &session_id,
                    Some(&entry_id),
                    Some(&label),
                    Some(&excerpt),
                    Some(started_ts_ms),
                    Some(ended_ts_ms),
                    None,
                ),
            )
        })
        .collect())
}

async fn recent_transcript_chunks(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let channels = rustfin_db::repo::channels::list_channels(&state.db).await?;
    let accessible_voice_channels: Vec<_> = channels
        .into_iter()
        .filter(|channel| channel.kind.eq_ignore_ascii_case("voice"))
        .filter(|channel| !channel.is_private || context.is_admin)
        .collect();

    let mut out = Vec::new();
    for channel in accessible_voice_channels.into_iter().take(3) {
        let sessions = rustfin_db::repo::channel_transcripts::list_sessions_for_channel(
            &state.db,
            &channel.id,
            1,
        )
        .await?;
        let Some(session) = sessions
            .into_iter()
            .find(|session| session.status == "completed")
        else {
            continue;
        };
        let entries =
            rustfin_db::repo::channel_transcripts::list_entries_for_session(&state.db, &session.id)
                .await?;
        let Some(entry) = entries.last() else {
            continue;
        };
        let excerpt = compact_text(&entry.text, 220);
        let label = format!(
            "{} · {}",
            channel.name,
            transcript_window_label(entry.started_ts_ms, entry.ended_ts_ms, session.started_ts)
        );
        out.push(make_chunk(
            "transcript_excerpt",
            format!("Recent call excerpt · {}", channel.name),
            excerpt.clone(),
            0.9,
            AssistantGroundingVisibility::User,
            Some(format!("transcript:{}", channel.id)),
            Some(context.user_id.clone()),
            &session.id,
            Some(entry.id.clone()),
            make_citation(
                "transcript_excerpt",
                &session.id,
                Some(&entry.id),
                Some(&label),
                Some(&excerpt),
                Some(entry.started_ts_ms),
                Some(entry.ended_ts_ms),
                None,
            ),
        ));
    }
    Ok(out)
}

async fn search_download_chunks(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, artifact_id, title, summary, detail, availability, version, platform, architecture, updated_ts, external_url, \
                ts_rank_cd( \
                    to_tsvector('simple', COALESCE(title, '') || ' ' || COALESCE(summary, '') || ' ' || COALESCE(detail, '') || ' ' || COALESCE(artifact_id, '')), \
                    websearch_to_tsquery('simple', $1) \
                ) AS rank \
         FROM download_artifact \
         WHERE to_tsvector('simple', COALESCE(title, '') || ' ' || COALESCE(summary, '') || ' ' || COALESCE(detail, '') || ' ' || COALESCE(artifact_id, '')) \
               @@ websearch_to_tsquery('simple', $1) \
         ORDER BY rank DESC, updated_ts DESC, title ASC \
         LIMIT $2",
    )
    .bind(query)
    .bind(DOWNLOAD_HIT_LIMIT)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let source_id: String = row.get("id");
            let artifact_id: String = row.get("artifact_id");
            let title: String = row.get("title");
            let summary: String = row.get("summary");
            let detail: String = row.get("detail");
            let availability: String = row.get("availability");
            let version: Option<String> = row.get("version");
            let platform: String = row.get("platform");
            let architecture: String = row.get("architecture");
            let updated_ts: i64 = row.get("updated_ts");
            let rank: f64 = row.get("rank");
            let external_url: Option<String> = row.get("external_url");
            let excerpt = compact_text(&format!("{summary} {detail}"), 260);
            make_chunk(
                "download_artifact",
                format!(
                    "{} [{}{}{}]",
                    title,
                    availability,
                    version
                        .as_deref()
                        .map(|value| format!(", {value}"))
                        .unwrap_or_default(),
                    if architecture.is_empty() {
                        String::new()
                    } else {
                        format!(", {platform}/{architecture}")
                    }
                ),
                excerpt.clone(),
                1.1 + rank + freshness_score(updated_ts),
                AssistantGroundingVisibility::User,
                Some("downloads:catalog".to_string()),
                Some(context.user_id.clone()),
                &source_id,
                Some(artifact_id.clone()),
                make_citation(
                    "download_artifact",
                    &source_id,
                    Some(&artifact_id),
                    Some(&title),
                    Some(&excerpt),
                    Some(updated_ts * 1000),
                    Some(updated_ts * 1000),
                    external_url.as_deref(),
                ),
            )
        })
        .collect())
}

async fn recent_download_chunks(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, artifact_id, title, summary, detail, availability, updated_ts \
         FROM download_artifact \
         ORDER BY updated_ts DESC, title ASC \
         LIMIT 3",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let source_id: String = row.get("id");
            let artifact_id: String = row.get("artifact_id");
            let title: String = row.get("title");
            let availability: String = row.get("availability");
            let updated_ts: i64 = row.get("updated_ts");
            let excerpt = compact_text(
                &format!(
                    "{} {}",
                    row.get::<String, _>("summary"),
                    row.get::<String, _>("detail")
                ),
                200,
            );
            make_chunk(
                "download_artifact",
                format!("Recent download · {title} [{availability}]"),
                excerpt.clone(),
                0.8 + freshness_score(updated_ts),
                AssistantGroundingVisibility::User,
                Some("downloads:catalog".to_string()),
                Some(context.user_id.clone()),
                &source_id,
                Some(artifact_id.clone()),
                make_citation(
                    "download_artifact",
                    &source_id,
                    Some(&artifact_id),
                    Some(&title),
                    Some(&excerpt),
                    Some(updated_ts * 1000),
                    Some(updated_ts * 1000),
                    None,
                ),
            )
        })
        .collect())
}

async fn search_library_chunks(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(rustfin_db::repo::users::get_library_access(&state.db, &context.user_id).await?)
    };
    if matches!(allowed_library_ids.as_ref(), Some(ids) if ids.is_empty()) {
        return Ok(Vec::new());
    }

    let mut next_param = 2usize;
    let mut sql = String::from(
        "SELECT i.id, i.library_id, i.kind, i.title, i.year, COALESCE(i.overview, '') AS overview, \
                COALESCE(l.name, '') AS library_name, i.updated_ts, \
                ts_rank_cd( \
                    to_tsvector('simple', COALESCE(i.title, '') || ' ' || COALESCE(i.overview, '') || ' ' || COALESCE(l.name, '')), \
                    websearch_to_tsquery('simple', $1) \
                ) AS rank \
         FROM item i \
         JOIN library l ON l.id = i.library_id \
         WHERE i.parent_id IS NULL \
           AND to_tsvector('simple', COALESCE(i.title, '') || ' ' || COALESCE(i.overview, '') || ' ' || COALESCE(l.name, '')) \
               @@ websearch_to_tsquery('simple', $1)",
    );

    if let Some(library_ids) = allowed_library_ids.as_ref() {
        let placeholders = dollar_placeholders(next_param, library_ids.len());
        sql.push_str(&format!(" AND i.library_id IN ({placeholders})"));
        next_param += library_ids.len();
    }

    sql.push_str(&format!(
        " ORDER BY rank DESC, i.updated_ts DESC, i.title ASC LIMIT ${next_param}"
    ));

    let mut db_query = sqlx::query(&sql).bind(query);
    if let Some(library_ids) = allowed_library_ids.as_ref() {
        for library_id in library_ids {
            db_query = db_query.bind(library_id);
        }
    }
    let rows = db_query
        .bind(LIBRARY_HIT_LIMIT)
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let source_id: String = row.get("id");
            let library_id: String = row.get("library_id");
            let kind: String = row.get("kind");
            let title: String = row.get("title");
            let year: Option<i64> = row.get("year");
            let overview: String = row.get("overview");
            let library_name: String = row.get("library_name");
            let updated_ts: i64 = row.get("updated_ts");
            let rank: f64 = row.get("rank");
            let excerpt = compact_text(&overview, 240);
            let year_suffix = year.map(|value| format!(" ({value})")).unwrap_or_default();
            make_chunk(
                "library_item",
                format!("{title}{year_suffix} · {kind} · {library_name}"),
                if excerpt.is_empty() {
                    format!("Library item in {library_name}")
                } else {
                    excerpt.clone()
                },
                1.0 + rank + freshness_score(updated_ts),
                AssistantGroundingVisibility::User,
                Some(format!("library:{library_id}")),
                Some(context.user_id.clone()),
                &source_id,
                None,
                make_citation(
                    "library_item",
                    &source_id,
                    None,
                    Some(&title),
                    Some(if excerpt.is_empty() {
                        &library_name
                    } else {
                        &excerpt
                    }),
                    Some(updated_ts * 1000),
                    Some(updated_ts * 1000),
                    None,
                ),
            )
        })
        .collect())
}

async fn recent_library_chunks(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let allowed_library_ids = if context.is_admin {
        None
    } else {
        Some(rustfin_db::repo::users::get_library_access(&state.db, &context.user_id).await?)
    };
    let items = rustfin_db::repo::items::list_recent_items(
        &state.db,
        allowed_library_ids.as_deref(),
        None,
        3,
    )
    .await?;
    let libraries = rustfin_db::repo::libraries::list_libraries(&state.db).await?;
    let library_names = libraries
        .into_iter()
        .map(|library| (library.id, library.name))
        .collect::<std::collections::HashMap<_, _>>();

    Ok(items
        .into_iter()
        .map(|item| {
            let library_name = library_names
                .get(&item.library_id)
                .cloned()
                .unwrap_or_else(|| "Library".to_string());
            let excerpt = item
                .overview
                .clone()
                .map(|overview| compact_text(&overview, 200))
                .unwrap_or_else(|| format!("Recently added in {library_name}"));
            make_chunk(
                "library_item",
                format!("Recent library item · {} · {}", item.title, library_name),
                excerpt.clone(),
                0.8 + freshness_score(item.created_ts),
                AssistantGroundingVisibility::User,
                Some(format!("library:{}", item.library_id)),
                Some(context.user_id.clone()),
                &item.id,
                None,
                make_citation(
                    "library_item",
                    &item.id,
                    None,
                    Some(&item.title),
                    Some(&excerpt),
                    Some(item.created_ts * 1000),
                    Some(item.created_ts * 1000),
                    None,
                ),
            )
        })
        .collect())
}

async fn search_recent_error_chunks(
    state: &AppState,
    context: &AssistantContext,
    query: &str,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, status, COALESCE(error, '') AS error, COALESCE(payload_json, '') AS payload_json, updated_ts, \
                ts_rank_cd( \
                    to_tsvector('simple', COALESCE(kind, '') || ' ' || COALESCE(status, '') || ' ' || COALESCE(error, '') || ' ' || COALESCE(payload_json, '')), \
                    websearch_to_tsquery('simple', $1) \
                ) AS rank \
         FROM job \
         WHERE status IN ('failed', 'error') \
           AND to_tsvector('simple', COALESCE(kind, '') || ' ' || COALESCE(status, '') || ' ' || COALESCE(error, '') || ' ' || COALESCE(payload_json, '')) \
               @@ websearch_to_tsquery('simple', $1) \
         ORDER BY rank DESC, updated_ts DESC \
         LIMIT $2",
    )
    .bind(query)
    .bind(ERROR_HIT_LIMIT)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let source_id: String = row.get("id");
            let kind: String = row.get("kind");
            let status: String = row.get("status");
            let error_text: String = row.get("error");
            let payload_json: String = row.get("payload_json");
            let updated_ts: i64 = row.get("updated_ts");
            let rank: f64 = row.get("rank");
            let excerpt = compact_text(
                if error_text.trim().is_empty() {
                    &payload_json
                } else {
                    &error_text
                },
                260,
            );
            make_chunk(
                "recent_error",
                format!("{kind} [{status}]"),
                excerpt.clone(),
                1.4 + rank + freshness_score(updated_ts),
                AssistantGroundingVisibility::Admin,
                Some("admin:recent_errors".to_string()),
                Some(context.user_id.clone()),
                &source_id,
                None,
                make_citation(
                    "recent_error",
                    &source_id,
                    None,
                    Some(&kind),
                    Some(&excerpt),
                    Some(updated_ts * 1000),
                    Some(updated_ts * 1000),
                    None,
                ),
            )
        })
        .collect())
}

async fn recent_error_overview_chunks(
    state: &AppState,
    context: &AssistantContext,
) -> Result<Vec<AssistantGroundingChunk>, sqlx::Error> {
    let rows = rustfin_db::repo::jobs::list_jobs_filtered(
        &state.db,
        &["failed", "error"],
        None,
        Some(3),
        None,
    )
    .await?;
    Ok(rows
        .into_iter()
        .map(|job| {
            let excerpt = compact_text(
                job.error
                    .as_deref()
                    .or(job.payload_json.as_deref())
                    .unwrap_or("Recent failed job."),
                220,
            );
            make_chunk(
                "recent_error",
                format!("Recent error · {}", job.kind),
                excerpt.clone(),
                0.95 + freshness_score(job.updated_ts),
                AssistantGroundingVisibility::Admin,
                Some("admin:recent_errors".to_string()),
                Some(context.user_id.clone()),
                &job.id,
                None,
                make_citation(
                    "recent_error",
                    &job.id,
                    None,
                    Some(&job.kind),
                    Some(&excerpt),
                    Some(job.updated_ts * 1000),
                    Some(job.updated_ts * 1000),
                    None,
                ),
            )
        })
        .collect())
}

fn freshness_score(updated_ts: i64) -> f64 {
    let age_hours = ((Utc::now().timestamp() - updated_ts).max(0) as f64) / 3600.0;
    if age_hours <= 1.0 {
        0.45
    } else if age_hours <= 24.0 {
        0.2
    } else if age_hours <= 24.0 * 7.0 {
        0.08
    } else {
        0.0
    }
}

pub async fn search_operational_chunks(
    state: &AppState,
    context: &AssistantContext,
    topic_key: Option<&str>,
    query: Option<&str>,
) -> Vec<AssistantGroundingChunk> {
    let normalized_query = query_text(query);
    let query_lower = normalized_query
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let terms = query_terms(normalized_query.as_deref());
    let broad_overview = looks_like_broad_overview(&query_lower);
    let mut chunks = Vec::new();

    if normalized_query.is_some() && (wants_transcripts(&query_lower, topic_key) || broad_overview)
    {
        match search_transcript_chunks(
            state,
            context,
            normalized_query.as_deref().unwrap_or_default(),
        )
        .await
        {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to search transcript chunks"),
        }
    }

    if normalized_query.is_some() && (wants_downloads(&query_lower, topic_key) || broad_overview) {
        match search_download_chunks(
            state,
            context,
            normalized_query.as_deref().unwrap_or_default(),
        )
        .await
        {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to search download chunks"),
        }
    }

    if normalized_query.is_some() && (wants_libraries(&query_lower, topic_key) || broad_overview) {
        match search_library_chunks(
            state,
            context,
            normalized_query.as_deref().unwrap_or_default(),
        )
        .await
        {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to search library chunks"),
        }
    }

    if context.is_admin
        && normalized_query.is_some()
        && (wants_recent_errors(&query_lower, topic_key) || broad_overview)
    {
        match search_recent_error_chunks(
            state,
            context,
            normalized_query.as_deref().unwrap_or_default(),
        )
        .await
        {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to search recent error chunks"),
        }
    }

    if chunks.is_empty() && broad_overview && terms.len() <= 4 {
        match recent_transcript_chunks(state, context).await {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to load recent transcript overview chunks"),
        }
        match recent_download_chunks(state, context).await {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to load recent download overview chunks"),
        }
        match recent_library_chunks(state, context).await {
            Ok(found) => chunks.extend(found),
            Err(error) => warn!(error = %error, "failed to load recent library overview chunks"),
        }
        if context.is_admin {
            match recent_error_overview_chunks(state, context).await {
                Ok(found) => chunks.extend(found),
                Err(error) => warn!(error = %error, "failed to load recent error overview chunks"),
            }
        }
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broad_overview_detection_matches_recent_change_phrasing() {
        assert!(looks_like_broad_overview(
            "what changed lately on the server"
        ));
        assert!(looks_like_broad_overview("anything new"));
        assert!(!looks_like_broad_overview("weather in cork tomorrow"));
    }

    #[test]
    fn transcript_window_label_formats_relative_ranges() {
        let label = transcript_window_label(65_000, 90_000, 0);
        assert_eq!(label, "01:05-01:30");
    }

    #[test]
    fn topic_family_extracts_prefix() {
        assert_eq!(topic_family(Some("downloads:catalog")), Some("downloads"));
        assert_eq!(topic_family(Some("admin:recent_errors")), Some("admin"));
    }
}
