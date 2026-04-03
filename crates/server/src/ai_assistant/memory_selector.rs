use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantMemoryKind {
    UserPreference,
    DurableFact,
    EnvironmentFact,
    Runbook,
    ToolGotcha,
    OpenLoop,
}

impl AssistantMemoryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserPreference => "user_preference",
            Self::DurableFact => "durable_fact",
            Self::EnvironmentFact => "environment_fact",
            Self::Runbook => "runbook",
            Self::ToolGotcha => "tool_gotcha",
            Self::OpenLoop => "open_loop",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "user_preference" => Some(Self::UserPreference),
            "durable_fact" => Some(Self::DurableFact),
            "environment_fact" => Some(Self::EnvironmentFact),
            "runbook" => Some(Self::Runbook),
            "tool_gotcha" => Some(Self::ToolGotcha),
            "open_loop" => Some(Self::OpenLoop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMemoryMetadata {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ts: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sub_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_chunk_id: Option<String>,
}

impl Default for AssistantMemoryMetadata {
    fn default() -> Self {
        Self {
            tags: Vec::new(),
            confidence: 0.75,
            expires_ts: None,
            source_kind: None,
            source_id: None,
            source_sub_id: None,
            source_chunk_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssistantMemorySelectorCandidate<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub topic_key: Option<&'a str>,
    pub weight: f64,
    pub lexical_rank: f64,
    pub memory_kind: AssistantMemoryKind,
    pub updated_ts: i64,
    pub metadata: AssistantMemoryMetadata,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantMemorySelection {
    pub score: f64,
    pub reasons: Vec<String>,
}

pub fn score_memory_candidate(
    candidate: &AssistantMemorySelectorCandidate<'_>,
    requested_topic_key: Option<&str>,
    query: Option<&str>,
) -> Option<AssistantMemorySelection> {
    if candidate
        .metadata
        .expires_ts
        .is_some_and(|expires_ts| expires_ts <= Utc::now().timestamp())
    {
        return None;
    }

    let mut score = candidate.lexical_rank + candidate.weight + candidate.metadata.confidence;
    let mut reasons = Vec::new();

    if let Some(requested_topic_key) = requested_topic_key {
        if candidate.topic_key == Some(requested_topic_key) {
            score += 1.4;
            reasons.push("topic_match".to_string());
        }
    }

    let query_terms = query_terms(query);
    if !query_terms.is_empty() {
        let title_lower = candidate.title.to_ascii_lowercase();
        let content_lower = candidate.content.to_ascii_lowercase();
        let matched_tags = candidate
            .metadata
            .tags
            .iter()
            .filter(|tag| query_terms.iter().any(|term| tag.contains(term)))
            .count() as f64;
        if matched_tags > 0.0 {
            score += matched_tags * 0.35;
            reasons.push("tag_match".to_string());
        }

        let exact_title_matches = query_terms
            .iter()
            .filter(|term| title_lower.contains(term.as_str()))
            .count() as f64;
        if exact_title_matches > 0.0 {
            score += exact_title_matches * 0.55;
            reasons.push("title_match".to_string());
        }

        let content_matches = query_terms
            .iter()
            .filter(|term| content_lower.contains(term.as_str()))
            .count() as f64;
        if content_matches > 0.0 {
            score += content_matches * 0.2;
            reasons.push("content_match".to_string());
        }
    }

    score += recency_bonus(candidate.memory_kind, candidate.updated_ts);
    reasons.push(candidate.memory_kind.as_str().to_string());
    Some(AssistantMemorySelection { score, reasons })
}

pub fn query_terms(query: Option<&str>) -> Vec<String> {
    query
        .unwrap_or_default()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 3)
        .filter(|term| {
            !matches!(
                term.as_str(),
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "from"
                    | "that"
                    | "this"
                    | "have"
                    | "what"
                    | "when"
                    | "where"
                    | "which"
                    | "your"
                    | "about"
                    | "show"
                    | "tell"
                    | "into"
                    | "then"
                    | "than"
                    | "were"
                    | "will"
                    | "would"
            )
        })
        .collect()
}

fn recency_bonus(kind: AssistantMemoryKind, updated_ts: i64) -> f64 {
    let age_hours = ((Utc::now().timestamp() - updated_ts).max(0) as f64) / 3600.0;
    match kind {
        AssistantMemoryKind::DurableFact => {
            if age_hours <= 24.0 {
                0.45
            } else if age_hours <= 24.0 * 30.0 {
                0.25
            } else {
                0.08
            }
        }
        AssistantMemoryKind::UserPreference => {
            if age_hours <= 24.0 * 14.0 {
                0.4
            } else {
                0.15
            }
        }
        AssistantMemoryKind::EnvironmentFact
        | AssistantMemoryKind::Runbook
        | AssistantMemoryKind::ToolGotcha
        | AssistantMemoryKind::OpenLoop => {
            if age_hours <= 1.0 {
                0.7
            } else if age_hours <= 24.0 {
                0.35
            } else if age_hours <= 24.0 * 7.0 {
                0.12
            } else {
                0.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(kind: AssistantMemoryKind) -> AssistantMemorySelectorCandidate<'static> {
        AssistantMemorySelectorCandidate {
            title: "Galway transcript excerpt",
            content: "The team discussed the Galway weather rollout.",
            topic_key: Some("transcript:galway"),
            weight: 1.0,
            lexical_rank: 0.8,
            memory_kind: kind,
            updated_ts: Utc::now().timestamp(),
            metadata: AssistantMemoryMetadata {
                tags: vec!["galway".to_string(), "weather".to_string()],
                confidence: 0.9,
                expires_ts: None,
                source_kind: Some("channels_get_transcript_summary".to_string()),
                source_id: Some("session-1".to_string()),
                source_sub_id: Some("entry-2".to_string()),
                source_chunk_id: Some("chunk-1".to_string()),
            },
        }
    }

    #[test]
    fn query_terms_drop_short_fillers() {
        let terms = query_terms(Some("What was said about Galway in the call?"));
        assert!(terms.contains(&"galway".to_string()));
        assert!(terms.contains(&"call".to_string()));
        assert!(!terms.contains(&"the".to_string()));
    }

    #[test]
    fn selector_prefers_topic_and_tag_matches() {
        let selected = score_memory_candidate(
            &candidate(AssistantMemoryKind::DurableFact),
            Some("transcript:galway"),
            Some("Galway weather"),
        )
        .expect("memory should remain eligible");
        assert!(selected.score > 3.0);
        assert!(selected.reasons.contains(&"topic_match".to_string()));
        assert!(selected.reasons.contains(&"tag_match".to_string()));
    }

    #[test]
    fn selector_skips_expired_memory() {
        let mut candidate = candidate(AssistantMemoryKind::EnvironmentFact);
        candidate.metadata.expires_ts = Some(Utc::now().timestamp() - 1);
        assert!(score_memory_candidate(&candidate, None, Some("weather")).is_none());
    }
}
