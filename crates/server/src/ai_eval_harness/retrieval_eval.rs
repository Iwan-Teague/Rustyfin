use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::replies::{
    MAX_GROUNDING_PROMPT_CHARS, grounding_chunks_prompt, rank_and_compress_grounding_chunks,
};
use crate::ai_assistant::types::AssistantGroundingChunk;

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    MAX_CASE_NAME_CHARS, MAX_CHUNK_TEXT_CHARS, MAX_GROUNDING_CHUNKS_PER_CASE, MAX_REQUIRED_MATCHES,
    finalize_case_verdict, inapplicable_gate, length_gate, pass_gate, visibility_allowed,
};
use super::report::{EvalFailureBucket, EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct RetrievalCase {
    name: String,
    #[serde(default = "default_user_role")]
    user_role: String,
    #[serde(rename = "question")]
    question: String,
    chunks: Vec<AssistantGroundingChunk>,
    required_evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RetrievalCaseResult {
    name: String,
    recall_at_5: f64,
    first_hit_rank: Option<usize>,
    prompt_inclusion_correct: bool,
}

fn default_user_role() -> String {
    "user".to_string()
}

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let spec = FixtureCorpusSpec {
        suite_name: "retrieval",
        fixture_file: "retrieval_cases.jsonl",
        schema_file: "retrieval_cases.schema.json",
    };
    let cases = load_jsonl::<RetrievalCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?;
    let mut recall_total = 0.0f64;
    let mut mrr_total = 0.0f64;
    let mut prompt_correct = 0usize;
    let mut details = Vec::new();
    let mut case_verdicts = Vec::new();

    for case in &cases {
        let mut chunks = case.chunks.clone();
        chunks.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
        });
        let top = rank_and_compress_grounding_chunks(&chunks, 5, MAX_GROUNDING_PROMPT_CHARS);
        let top_ids = top.iter().map(|chunk| chunk.id.clone()).collect::<Vec<_>>();
        let hits = case
            .required_evidence_ids
            .iter()
            .filter(|id| top_ids.iter().any(|candidate| candidate == *id))
            .count();
        let recall_at_5 = hits as f64 / case.required_evidence_ids.len().max(1) as f64;
        let first_hit_rank = top_ids.iter().position(|id| {
            case.required_evidence_ids
                .iter()
                .any(|required| required == id)
        });
        let prompt = grounding_chunks_prompt(&top);
        let prompt_inclusion_correct = case
            .required_evidence_ids
            .iter()
            .all(|required| prompt.contains(required));

        recall_total += recall_at_5;
        if let Some(rank) = first_hit_rank {
            mrr_total += 1.0 / (rank as f64 + 1.0);
        }
        if prompt_inclusion_correct {
            prompt_correct += 1;
        }

        let case_result = RetrievalCaseResult {
            name: case.name.clone(),
            recall_at_5,
            first_hit_rank: first_hit_rank.map(|rank| rank + 1),
            prompt_inclusion_correct,
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
            if case.required_evidence_ids.len() > MAX_REQUIRED_MATCHES {
                violations.push(format!(
                    "required evidence count {} exceeds {}",
                    case.required_evidence_ids.len(),
                    MAX_REQUIRED_MATCHES
                ));
            }
            if case.chunks.len() > MAX_GROUNDING_CHUNKS_PER_CASE {
                violations.push(format!(
                    "chunk count {} exceeds {}",
                    case.chunks.len(),
                    MAX_GROUNDING_CHUNKS_PER_CASE
                ));
            }
            for chunk in &case.chunks {
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
            let invisible = case
                .chunks
                .iter()
                .filter(|chunk| !visibility_allowed(&case.user_role, chunk.visibility))
                .map(|chunk| chunk.id.clone())
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
        let exact_answer_gate = if (case_result.recall_at_5 - 1.0).abs() < f64::EPSILON
            && case_result.prompt_inclusion_correct
        {
            pass_gate("exact_answer_contract")
        } else {
            super::judge::fail_gate(
                "exact_answer_contract",
                EvalFailureBucket::ExactAnswerMismatch,
                format!(
                    "recall_at_5={:.3}, prompt_inclusion_correct={}",
                    case_result.recall_at_5, case_result.prompt_inclusion_correct
                ),
            )
        };

        let mut case_metrics = BTreeMap::new();
        case_metrics.insert("recall_at_5".to_string(), case_result.recall_at_5);
        case_metrics.insert(
            "prompt_inclusion_correct".to_string(),
            if case_result.prompt_inclusion_correct {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "first_hit_rank".to_string(),
            case_result.first_hit_rank.unwrap_or_default() as f64,
        );

        case_verdicts.push(finalize_case_verdict(
            &case.name,
            case_metrics,
            vec![
                pass_gate("schema_validity"),
                inapplicable_gate(
                    "malformed_output",
                    "retrieval fixtures exercise grounded chunk ranking, not generated JSON output",
                ),
                length_gate,
                inapplicable_gate(
                    "refusal_correctness",
                    "retrieval fixtures do not encode refusal flows in phase 1",
                ),
                acl_gate,
                exact_answer_gate,
            ],
            serde_json::to_value(&case_result)?,
        ));
        details.push(case_result);
    }

    let recall_metric = recall_total / cases.len().max(1) as f64;
    let mrr_metric = mrr_total / cases.len().max(1) as f64;
    let prompt_metric = prompt_correct as f64 / cases.len().max(1) as f64;

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "retrieval_required_evidence_recall_at_5".to_string(),
        recall_metric,
    );
    metrics.insert("retrieval_mrr".to_string(), mrr_metric);
    metrics.insert(
        "retrieval_prompt_inclusion_correctness".to_string(),
        prompt_metric,
    );

    let thresholds = vec![EvalThreshold {
        metric: "retrieval_required_evidence_recall_at_5".to_string(),
        actual: recall_metric,
        expected: 0.90,
        pass: recall_metric >= 0.90,
        blocking: true,
    }];

    Ok(EvalSuiteReport::finalize(
        "retrieval",
        metrics,
        thresholds,
        case_verdicts,
        serde_json::to_value(details)?,
    ))
}
