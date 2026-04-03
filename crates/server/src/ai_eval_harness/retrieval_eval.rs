use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::replies::{
    MAX_GROUNDING_PROMPT_CHARS, grounding_chunks_prompt, rank_and_compress_grounding_chunks,
};
use crate::ai_assistant::types::AssistantGroundingChunk;

use super::corpus::load_jsonl;
use super::report::{EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct RetrievalCase {
    name: String,
    #[serde(rename = "question")]
    _question: String,
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

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let cases = load_jsonl::<RetrievalCase>(&fixtures_dir.join("retrieval_cases.jsonl"))?;
    let mut recall_total = 0.0f64;
    let mut mrr_total = 0.0f64;
    let mut prompt_correct = 0usize;
    let mut details = Vec::new();

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

        details.push(RetrievalCaseResult {
            name: case.name.clone(),
            recall_at_5,
            first_hit_rank: first_hit_rank.map(|rank| rank + 1),
            prompt_inclusion_correct,
        });
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
    }];

    Ok(EvalSuiteReport {
        name: "retrieval".to_string(),
        pass: thresholds.iter().all(|threshold| threshold.pass),
        metrics,
        thresholds,
        case_count: cases.len(),
        details: serde_json::to_value(details)?,
    })
}
