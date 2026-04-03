use std::cmp::Ordering;
use std::collections::HashSet;

use super::types::{AssistantGroundingChunk, AssistantGroundingCitation};

pub const MAX_GROUNDING_CHUNKS: usize = 10;
pub const MAX_GROUNDING_PROMPT_CHARS: usize = 5_500;

pub fn rank_and_compress_grounding_chunks(
    chunks: &[AssistantGroundingChunk],
    max_chunks: usize,
    max_chars: usize,
) -> Vec<AssistantGroundingChunk> {
    let mut ranked = chunks.to_vec();
    ranked.sort_by(compare_grounding_chunks);

    let mut seen = HashSet::<String>::new();
    let mut compacted = Vec::new();
    let mut chars_used = 0usize;

    for chunk in ranked {
        if !seen.insert(chunk.id.clone()) {
            continue;
        }

        let chunk_chars = grounding_chunk_prompt_line(&chunk).len();
        if !compacted.is_empty() && chars_used + chunk_chars > max_chars {
            break;
        }

        chars_used += chunk_chars;
        compacted.push(chunk);
        if compacted.len() >= max_chunks {
            break;
        }
    }

    compacted
}

pub fn grounding_chunks_prompt(chunks: &[AssistantGroundingChunk]) -> String {
    if chunks.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "Grounding chunks for this turn. Use these ranked stable IDs and short excerpts as evidence.\n",
    );
    for (index, chunk) in chunks.iter().enumerate() {
        let line = grounding_chunk_prompt_line(chunk);
        out.push_str(&format!("{:02}. {line}\n", index + 1));
    }
    out.trim_end().to_string()
}

pub fn grounding_chunk_prompt_line(chunk: &AssistantGroundingChunk) -> String {
    let visibility = match chunk.visibility {
        super::types::AssistantGroundingVisibility::User => "user",
        super::types::AssistantGroundingVisibility::Shared => "shared",
        super::types::AssistantGroundingVisibility::Admin => "admin",
    };
    let mut parts = vec![
        format!("[{}]", chunk.id),
        chunk.title.trim().to_string(),
        format!("kind={}", chunk.source_kind),
        format!("vis={visibility}"),
        format!("score={:.3}", chunk.score),
        format!("excerpt={}", compact_text(&chunk.excerpt, 260)),
    ];

    if let Some(topic_key) = chunk
        .topic_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("topic={topic_key}"));
    }
    if let Some(source_id) = chunk
        .source_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("source={source_id}"));
    }
    if let Some(source_sub_id) = chunk
        .source_sub_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("sub={source_sub_id}"));
    }
    if let Some(citation) = chunk.citation.as_ref() {
        parts.push(format!("cite={}", citation_brief(citation)));
    }

    parts.join(" | ")
}

pub fn citation_brief(citation: &AssistantGroundingCitation) -> String {
    let mut parts = vec![
        citation.citation_id.clone(),
        format!("{}:{}", citation.source_kind, citation.source_id),
    ];
    if let Some(sub_id) = citation
        .source_sub_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(sub_id.to_string());
    }
    if let (Some(started), Some(ended)) = (citation.started_ts_ms, citation.ended_ts_ms) {
        parts.push(format!("{started}-{ended}"));
    }
    if let Some(label) = citation
        .label
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(compact_text(label, 80));
    }
    parts.join("@")
}

fn compare_grounding_chunks(
    left: &AssistantGroundingChunk,
    right: &AssistantGroundingChunk,
) -> Ordering {
    right
        .score
        .partial_cmp(&left.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.visibility.cmp(&right.visibility))
        .then_with(|| left.source_kind.cmp(&right.source_kind))
        .then_with(|| left.topic_key.cmp(&right.topic_key))
        .then_with(|| left.title.cmp(&right.title))
        .then_with(|| left.id.cmp(&right.id))
}

pub fn compact_text(text: &str, max_chars: usize) -> String {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut out = String::new();
    for (index, ch) in normalized.chars().enumerate() {
        if index >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_assistant::types::{AssistantGroundingCitation, AssistantGroundingVisibility};

    fn chunk(id: &str, score: f64, title: &str, excerpt: &str) -> AssistantGroundingChunk {
        AssistantGroundingChunk {
            id: id.to_string(),
            source_kind: "transcript".to_string(),
            title: title.to_string(),
            excerpt: excerpt.to_string(),
            score,
            visibility: AssistantGroundingVisibility::User,
            topic_key: Some("topic".to_string()),
            owner_user_id: Some("user-1".to_string()),
            source_id: Some("source".to_string()),
            source_sub_id: Some("sub".to_string()),
            citation: Some(AssistantGroundingCitation {
                citation_id: format!("cite-{id}"),
                source_kind: "transcript".to_string(),
                source_id: "source".to_string(),
                source_sub_id: Some("sub".to_string()),
                label: Some(title.to_string()),
                excerpt: Some(excerpt.to_string()),
                started_ts_ms: Some(1000),
                ended_ts_ms: Some(2000),
                url: None,
            }),
        }
    }

    #[test]
    fn compresses_and_sorts_by_score() {
        let ranked = rank_and_compress_grounding_chunks(
            &[
                chunk("b", 0.2, "B", "b"),
                chunk("a", 0.9, "A", "a"),
                chunk("a", 0.1, "A dup", "dup"),
            ],
            10,
            10_000,
        );
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].id, "a");
        assert_eq!(ranked[1].id, "b");
    }

    #[test]
    fn prompt_renders_stable_ids() {
        let prompt = grounding_chunks_prompt(&[chunk("a", 0.9, "Alpha", "This is an excerpt.")]);
        assert!(prompt.contains("[a]"));
        assert!(prompt.contains("kind=transcript"));
        assert!(prompt.contains("excerpt=This is an excerpt."));
    }
}
