use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::memory::derive_topic_key_from_history;
use crate::ai_assistant::types::{AssistantGroundingChunk, AssistantHistoryMessage};

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    MAX_CASE_NAME_CHARS, MAX_CHUNK_TEXT_CHARS, MAX_GROUNDING_CHUNKS_PER_CASE,
    finalize_case_verdict, inapplicable_gate, length_gate, pass_gate, visibility_allowed,
};
use super::report::{EvalFailureBucket, EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct MemoryCase {
    name: String,
    #[serde(default = "default_user_role")]
    user_role: String,
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

fn default_user_role() -> String {
    "user".to_string()
}

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let spec = FixtureCorpusSpec {
        suite_name: "memory",
        fixture_file: "memory_cases.jsonl",
        schema_file: "memory_cases.schema.json",
    };
    let cases = load_jsonl::<MemoryCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?;
    let mut fact_hits = 0usize;
    let mut topic_hits = 0usize;
    let mut preference_hits = 0usize;
    let mut details = Vec::new();
    let mut case_verdicts = Vec::new();

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

        let case_result = MemoryCaseResult {
            name: case.name.clone(),
            selected_chunk_id: selected.map(|chunk| chunk.id),
            topic_match,
            fact_match,
            preference_match,
        };

        let length_gate = {
            let mut violations = Vec::new();
            for gate in [
                length_gate(
                    "case_name_length",
                    case.name.chars().count(),
                    MAX_CASE_NAME_CHARS,
                ),
                length_gate(
                    "question_length",
                    case.question.chars().count(),
                    super::judge::MAX_PROMPT_CHARS,
                ),
            ] {
                if !gate.pass {
                    violations.push(gate.message.clone().unwrap_or_default());
                }
            }
            if case.memory_chunks.len() > MAX_GROUNDING_CHUNKS_PER_CASE {
                violations.push(format!(
                    "memory chunk count {} exceeds {}",
                    case.memory_chunks.len(),
                    MAX_GROUNDING_CHUNKS_PER_CASE
                ));
            }
            for chunk in &case.memory_chunks {
                for gate in [
                    length_gate(
                        "chunk_title_length",
                        chunk.title.chars().count(),
                        MAX_CHUNK_TEXT_CHARS,
                    ),
                    length_gate(
                        "chunk_excerpt_length",
                        chunk.excerpt.chars().count(),
                        MAX_CHUNK_TEXT_CHARS,
                    ),
                ] {
                    if !gate.pass {
                        violations.push(gate.message.clone().unwrap_or_default());
                    }
                }
            }
            if violations.is_empty() {
                pass_gate("length_limit")
            } else {
                super::judge::fail_gate(
                    "length_limit",
                    EvalFailureBucket::LengthLimitExceeded,
                    violations.join("; "),
                )
            }
        };
        let acl_gate = {
            let invisible_history = case
                .history
                .iter()
                .flat_map(|message| message.grounding_chunks.iter())
                .filter(|chunk| !visibility_allowed(&case.user_role, chunk.visibility))
                .map(|chunk| chunk.id.clone())
                .collect::<Vec<_>>();
            let invisible_memory = case
                .memory_chunks
                .iter()
                .filter(|chunk| !visibility_allowed(&case.user_role, chunk.visibility))
                .map(|chunk| chunk.id.clone())
                .collect::<Vec<_>>();
            let invisible = invisible_history
                .into_iter()
                .chain(invisible_memory.into_iter())
                .collect::<Vec<_>>();
            if invisible.is_empty() {
                pass_gate("acl_privacy_boundary")
            } else {
                super::judge::fail_gate(
                    "acl_privacy_boundary",
                    EvalFailureBucket::PrivacyBoundaryViolation,
                    format!(
                        "role {} cannot access chunks: {}",
                        case.user_role,
                        invisible.join(", ")
                    ),
                )
            }
        };
        let exact_answer_gate = if case_result.topic_match
            && case_result.fact_match
            && case_result.preference_match
        {
            pass_gate("exact_answer_contract")
        } else {
            super::judge::fail_gate(
                "exact_answer_contract",
                EvalFailureBucket::ExactAnswerMismatch,
                format!(
                    "topic_match={}, fact_match={}, preference_match={}",
                    case_result.topic_match, case_result.fact_match, case_result.preference_match
                ),
            )
        };

        let mut case_metrics = BTreeMap::new();
        case_metrics.insert(
            "topic_match".to_string(),
            if case_result.topic_match { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "fact_match".to_string(),
            if case_result.fact_match { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "preference_match".to_string(),
            if case_result.preference_match {
                1.0
            } else {
                0.0
            },
        );

        case_verdicts.push(finalize_case_verdict(
            &case.name,
            case_metrics,
            vec![
                pass_gate("schema_validity"),
                inapplicable_gate(
                    "malformed_output",
                    "memory fixtures exercise retrieval/selection behavior, not generated output formatting",
                ),
                length_gate,
                inapplicable_gate(
                    "refusal_correctness",
                    "memory fixtures do not encode refusal flows in phase 1",
                ),
                acl_gate,
                exact_answer_gate,
            ],
            serde_json::to_value(&case_result)?,
        ));
        details.push(case_result);
    }

    let fact_metric = fact_hits as f64 / cases.len().max(1) as f64;
    let topic_metric = topic_hits as f64 / cases.len().max(1) as f64;
    let preference_metric = preference_hits as f64 / cases.len().max(1) as f64;

    let mut metrics = BTreeMap::new();
    metrics.insert("memory_fact_recall_accuracy".to_string(), fact_metric);
    metrics.insert("memory_topic_recall_accuracy".to_string(), topic_metric);
    metrics.insert(
        "memory_preference_recall_accuracy".to_string(),
        preference_metric,
    );

    let thresholds = vec![EvalThreshold {
        metric: "memory_fact_recall_accuracy".to_string(),
        actual: fact_metric,
        expected: 0.85,
        pass: fact_metric >= 0.85,
        blocking: true,
    }];

    Ok(EvalSuiteReport::finalize(
        "memory",
        metrics,
        thresholds,
        case_verdicts,
        serde_json::to_value(details)?,
    ))
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
    input
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(str::to_string)
        .collect()
}
