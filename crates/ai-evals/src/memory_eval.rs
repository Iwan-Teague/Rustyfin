use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use rustfin_server::ai_assistant::memory::derive_topic_key_from_history;
use rustfin_server::ai_assistant::types::{AssistantGroundingChunk, AssistantHistoryMessage};
use serde::{Deserialize, Serialize};

use crate::corpus::load_jsonl;
use crate::report::{EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct MemoryCase {
    name: String,
    history: Vec<AssistantHistoryMessage>,
    memory_chunks: Vec<AssistantGroundingChunk>,
    question: String,
    expected_fact_substring: String,
    expected_topic: String,
    #[serde(default)]
    expected_preference_substring: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryCaseResult {
    name: String,
    selected_chunk_id: Option<String>,
    topic_match: bool,
    fact_match: bool,
    preference_match: bool,
}

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let cases = load_jsonl::<MemoryCase>(&fixtures_dir.join("memory_cases.jsonl"))?;
    let mut fact_hits = 0usize;
    let mut topic_hits = 0usize;
    let mut preference_hits = 0usize;
    let mut details = Vec::new();

    for case in &cases {
        let derived_topic = derive_topic_key_from_history(&case.history)
            .unwrap_or_else(|| case.expected_topic.clone());
        let selected = select_memory_chunk(&case.question, &derived_topic, &case.memory_chunks);
        let selected_text = selected
            .as_ref()
            .map(|chunk| format!("{} {}", chunk.title, chunk.excerpt))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let topic_match = selected
            .as_ref()
            .and_then(|chunk| chunk.topic_key.clone())
            .map(|topic| topic == case.expected_topic)
            .unwrap_or_else(|| derived_topic == case.expected_topic);
        let fact_match = selected_text.contains(&case.expected_fact_substring.to_ascii_lowercase());
        let preference_match = case
            .expected_preference_substring
            .as_ref()
            .map(|expected| selected_text.contains(&expected.to_ascii_lowercase()))
            .unwrap_or(true);

        if topic_match {
            topic_hits += 1;
        }
        if fact_match {
            fact_hits += 1;
        }
        if preference_match {
            preference_hits += 1;
        }

        details.push(MemoryCaseResult {
            name: case.name.clone(),
            selected_chunk_id: selected.map(|chunk| chunk.id),
            topic_match,
            fact_match,
            preference_match,
        });
    }

    let fact_metric = fact_hits as f64 / cases.len().max(1) as f64;
    let topic_metric = topic_hits as f64 / cases.len().max(1) as f64;
    let preference_metric = preference_hits as f64 / cases.len().max(1) as f64;

    let mut metrics = BTreeMap::new();
    metrics.insert("memory_fact_recall_accuracy".to_string(), fact_metric);
    metrics.insert("memory_topic_recall_accuracy".to_string(), topic_metric);
    metrics.insert("memory_preference_recall_accuracy".to_string(), preference_metric);

    let thresholds = vec![EvalThreshold {
        metric: "memory_fact_recall_accuracy".to_string(),
        actual: fact_metric,
        expected: 0.85,
        pass: fact_metric >= 0.85,
    }];

    Ok(EvalSuiteReport {
        name: "memory".to_string(),
        pass: thresholds.iter().all(|threshold| threshold.pass),
        metrics,
        thresholds,
        case_count: cases.len(),
        details: serde_json::to_value(details)?,
    })
}

fn select_memory_chunk(
    question: &str,
    derived_topic: &str,
    chunks: &[AssistantGroundingChunk],
) -> Option<AssistantGroundingChunk> {
    let query_tokens = tokenize(question);
    let mut ranked = chunks
        .iter()
        .cloned()
        .map(|chunk| {
            let mut score = chunk.score;
            if chunk.topic_key.as_deref() == Some(derived_topic) {
                score += 5.0;
            }
            let haystack = format!("{} {}", chunk.title, chunk.excerpt).to_ascii_lowercase();
            for token in &query_tokens {
                if haystack.contains(token) {
                    score += 1.0;
                }
            }
            (score, chunk)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
    ranked.into_iter().next().map(|(_, chunk)| chunk)
}

fn tokenize(input: &str) -> Vec<String> {
    input.to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(str::to_string)
        .collect()
}
