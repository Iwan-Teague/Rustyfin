use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::types::AssistantGroundingVisibility;

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    EvalRunConfig, MAX_CASE_NAME_CHARS, MAX_CHUNK_TEXT_CHARS, MAX_GROUNDING_CHUNKS_PER_CASE,
    MAX_MODEL_OUTPUT_CHARS, MAX_PROMPT_CHARS, build_run_manifest, default_generated_at, fail_gate,
    length_gate, pass_gate, visibility_allowed,
};
use super::judge_metrics::EvalRubricFamily;
use super::judge_reports::{
    EvalComparisonCaseVerdict, EvalComparisonOrderVerdict, EvalComparisonPresentationOrder,
    EvalComparisonReport, EvalComparisonVariant, EvalComparisonWinner,
};
use super::judge_rubric::{
    PAIRWISE_PROMPT_VERSION, PAIRWISE_RESPONSE_SCHEMA_VERSION, ParsedPairwiseResponse,
    RubricEvidenceChunk, build_pairwise_prompt, parse_pairwise_response,
};
use super::report::{EvalFailureBucket, EvalThreshold};

pub const COMPARISON_DATASET_VERSION: &str = "rustyfin_ai_pairwise_corpora_v1";
pub const COMPARISON_JUDGE_VERSION: &str = "rustyfin_ai_pairwise_phase3_v1";

#[derive(Debug, Clone, Deserialize)]
struct ComparisonCase {
    name: String,
    domain: String,
    mode: String,
    prompt: String,
    reference_answer: String,
    #[serde(default = "default_rubric_family")]
    rubric_family: EvalRubricFamily,
    #[serde(default = "default_user_role")]
    user_role: String,
    #[serde(default)]
    evidence_chunks: Vec<ComparisonEvidenceChunk>,
    baseline: ComparisonVariantInput,
    candidate: ComparisonVariantInput,
    pairwise_forward_response: serde_json::Value,
    pairwise_flipped_response: serde_json::Value,
    expected_winner: EvalComparisonWinner,
    expected_order_consistent: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ComparisonVariantInput {
    label: String,
    model_id: String,
    prompt_version: String,
    assistant_answer: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ComparisonEvidenceChunk {
    id: String,
    text: String,
    visibility: AssistantGroundingVisibility,
}

#[derive(Debug, Clone, Serialize)]
struct ComparisonCaseResult {
    name: String,
    domain: String,
    mode: String,
    winner_match: bool,
    order_consistency_match: bool,
    expected_winner: EvalComparisonWinner,
    final_winner: EvalComparisonWinner,
    expected_order_consistent: bool,
    order_consistent: bool,
    forward_pairwise_prompt: String,
    flipped_pairwise_prompt: String,
    pairwise_prompt_version: String,
    pairwise_schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forward_parse_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flipped_parse_error: Option<String>,
}

fn default_rubric_family() -> EvalRubricFamily {
    EvalRubricFamily::ResponseQuality
}

fn default_user_role() -> String {
    "user".to_string()
}

pub fn run(fixtures_dir: &Path, config: &EvalRunConfig) -> Result<EvalComparisonReport> {
    let spec = FixtureCorpusSpec {
        suite_name: "comparison",
        fixture_file: "comparison_cases.jsonl",
        schema_file: "comparison_cases.schema.json",
    };
    let cases = load_jsonl::<ComparisonCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?;

    let mut winner_matches = 0usize;
    let mut order_consistency_matches = 0usize;
    let mut case_verdicts = Vec::new();

    for case in &cases {
        let evidence = case
            .evidence_chunks
            .iter()
            .map(|chunk| RubricEvidenceChunk {
                id: chunk.id.clone(),
                text: chunk.text.clone(),
            })
            .collect::<Vec<_>>();
        let forward_prompt = build_pairwise_prompt(
            case.rubric_family,
            &case.prompt,
            &case.baseline.assistant_answer,
            &case.candidate.assistant_answer,
            &case.reference_answer,
            &evidence,
        );
        let flipped_prompt = build_pairwise_prompt(
            case.rubric_family,
            &case.prompt,
            &case.candidate.assistant_answer,
            &case.baseline.assistant_answer,
            &case.reference_answer,
            &evidence,
        );

        let forward_raw = serde_json::to_string(&case.pairwise_forward_response)?;
        let flipped_raw = serde_json::to_string(&case.pairwise_flipped_response)?;
        let parsed_forward = parse_pairwise_response(&forward_raw);
        let parsed_flipped = parse_pairwise_response(&flipped_raw);

        let malformed_output_gate = match (&parsed_forward, &parsed_flipped) {
            (Ok(_), Ok(_)) => pass_gate("malformed_output"),
            _ => {
                let mut errors = Vec::new();
                if let Err(error) = &parsed_forward {
                    errors.push(format!("forward: {error}"));
                }
                if let Err(error) = &parsed_flipped {
                    errors.push(format!("flipped: {error}"));
                }
                fail_gate(
                    "malformed_output",
                    EvalFailureBucket::MalformedOutput,
                    errors.join("; "),
                )
            }
        };

        let length_limit_gate = {
            let mut violations = Vec::new();
            for gate in [
                length_gate(
                    "case_name_length",
                    case.name.chars().count(),
                    MAX_CASE_NAME_CHARS,
                ),
                length_gate(
                    "prompt_length",
                    case.prompt.chars().count(),
                    MAX_PROMPT_CHARS,
                ),
                length_gate(
                    "reference_answer_length",
                    case.reference_answer.chars().count(),
                    MAX_MODEL_OUTPUT_CHARS,
                ),
                length_gate(
                    "baseline_answer_length",
                    case.baseline.assistant_answer.chars().count(),
                    MAX_MODEL_OUTPUT_CHARS,
                ),
                length_gate(
                    "candidate_answer_length",
                    case.candidate.assistant_answer.chars().count(),
                    MAX_MODEL_OUTPUT_CHARS,
                ),
            ] {
                if !gate.pass {
                    violations.push(gate.message.unwrap_or_default());
                }
            }
            if case.evidence_chunks.len() > MAX_GROUNDING_CHUNKS_PER_CASE {
                violations.push(format!(
                    "evidence chunk count {} exceeds {}",
                    case.evidence_chunks.len(),
                    MAX_GROUNDING_CHUNKS_PER_CASE
                ));
            }
            for chunk in &case.evidence_chunks {
                if chunk.text.chars().count() > MAX_CHUNK_TEXT_CHARS {
                    violations.push(format!(
                        "chunk {} length {} exceeds {}",
                        chunk.id,
                        chunk.text.chars().count(),
                        MAX_CHUNK_TEXT_CHARS
                    ));
                }
            }
            if violations.is_empty() {
                pass_gate("length_limit")
            } else {
                fail_gate(
                    "length_limit",
                    EvalFailureBucket::LengthLimitExceeded,
                    violations.join("; "),
                )
            }
        };

        let acl_gate = if case
            .evidence_chunks
            .iter()
            .all(|chunk| visibility_allowed(&case.user_role, chunk.visibility))
        {
            pass_gate("acl_privacy_boundary")
        } else {
            fail_gate(
                "acl_privacy_boundary",
                EvalFailureBucket::PrivacyBoundaryViolation,
                "one or more evidence chunks exceed the declared user role visibility",
            )
        };

        let (final_winner, order_consistent, confidence, presentation_orders) =
            match (&parsed_forward, &parsed_flipped) {
                (Ok(forward), Ok(flipped)) => aggregate_pairwise_verdicts(forward, flipped),
                _ => (EvalComparisonWinner::NoWinner, false, 0.0, Vec::new()),
            };

        let winner_match = final_winner == case.expected_winner;
        let order_consistency_match = order_consistent == case.expected_order_consistent;
        if winner_match {
            winner_matches += 1;
        }
        if order_consistency_match {
            order_consistency_matches += 1;
        }

        let details = ComparisonCaseResult {
            name: case.name.clone(),
            domain: case.domain.clone(),
            mode: case.mode.clone(),
            winner_match,
            order_consistency_match,
            expected_winner: case.expected_winner,
            final_winner,
            expected_order_consistent: case.expected_order_consistent,
            order_consistent,
            forward_pairwise_prompt: forward_prompt,
            flipped_pairwise_prompt: flipped_prompt,
            pairwise_prompt_version: PAIRWISE_PROMPT_VERSION.to_string(),
            pairwise_schema_version: PAIRWISE_RESPONSE_SCHEMA_VERSION.to_string(),
            forward_parse_error: parsed_forward.as_ref().err().cloned(),
            flipped_parse_error: parsed_flipped.as_ref().err().cloned(),
        };

        case_verdicts.push(EvalComparisonCaseVerdict::new(
            &case.name,
            case.expected_winner,
            final_winner,
            order_consistent,
            case.expected_order_consistent,
            confidence,
            vec![
                pass_gate("schema_validity"),
                malformed_output_gate,
                length_limit_gate,
                acl_gate,
            ],
            EvalComparisonVariant {
                label: case.baseline.label.clone(),
                model_id: case.baseline.model_id.clone(),
                prompt_version: case.baseline.prompt_version.clone(),
                answer: case.baseline.assistant_answer.clone(),
            },
            EvalComparisonVariant {
                label: case.candidate.label.clone(),
                model_id: case.candidate.model_id.clone(),
                prompt_version: case.candidate.prompt_version.clone(),
                answer: case.candidate.assistant_answer.clone(),
            },
            presentation_orders,
            serde_json::to_value(&details)?,
        ));
    }

    let total = cases.len().max(1) as f64;
    let winner_accuracy = winner_matches as f64 / total;
    let order_consistency_accuracy = order_consistency_matches as f64 / total;
    let thresholds = vec![
        EvalThreshold {
            metric: "pairwise_winner_accuracy".to_string(),
            actual: winner_accuracy,
            expected: 1.0,
            pass: (winner_accuracy - 1.0).abs() < f64::EPSILON,
            blocking: true,
        },
        EvalThreshold {
            metric: "pairwise_order_consistency_accuracy".to_string(),
            actual: order_consistency_accuracy,
            expected: 1.0,
            pass: (order_consistency_accuracy - 1.0).abs() < f64::EPSILON,
            blocking: true,
        },
    ];

    let mut manifest_config = config.clone();
    if manifest_config.dataset_version.is_none() {
        manifest_config.dataset_version = Some(COMPARISON_DATASET_VERSION.to_string());
    }
    if manifest_config.judge_version.is_none() {
        manifest_config.judge_version = Some(COMPARISON_JUDGE_VERSION.to_string());
    }
    let manifest = build_run_manifest("comparison", fixtures_dir, &manifest_config)?;
    let generated_at = config
        .generated_at
        .clone()
        .unwrap_or_else(default_generated_at);

    Ok(EvalComparisonReport::new(
        generated_at,
        manifest,
        PAIRWISE_PROMPT_VERSION,
        PAIRWISE_RESPONSE_SCHEMA_VERSION,
        thresholds,
        case_verdicts,
    ))
}

fn normalize_selection(
    selection: super::judge_rubric::EvalPairwiseSelection,
    order: EvalComparisonPresentationOrder,
) -> EvalComparisonWinner {
    match selection {
        super::judge_rubric::EvalPairwiseSelection::A => match order {
            EvalComparisonPresentationOrder::BaselineFirst => EvalComparisonWinner::Baseline,
            EvalComparisonPresentationOrder::CandidateFirst => EvalComparisonWinner::Candidate,
        },
        super::judge_rubric::EvalPairwiseSelection::B => match order {
            EvalComparisonPresentationOrder::BaselineFirst => EvalComparisonWinner::Candidate,
            EvalComparisonPresentationOrder::CandidateFirst => EvalComparisonWinner::Baseline,
        },
        super::judge_rubric::EvalPairwiseSelection::Tie => EvalComparisonWinner::Tie,
        super::judge_rubric::EvalPairwiseSelection::NoWinner => EvalComparisonWinner::NoWinner,
    }
}

fn aggregate_pairwise_verdicts(
    forward: &ParsedPairwiseResponse,
    flipped: &ParsedPairwiseResponse,
) -> (
    EvalComparisonWinner,
    bool,
    f64,
    Vec<EvalComparisonOrderVerdict>,
) {
    let forward_winner = normalize_selection(
        forward.selection,
        EvalComparisonPresentationOrder::BaselineFirst,
    );
    let flipped_winner = normalize_selection(
        flipped.selection,
        EvalComparisonPresentationOrder::CandidateFirst,
    );
    let order_consistent = forward_winner == flipped_winner;
    let final_winner = if order_consistent {
        forward_winner
    } else {
        EvalComparisonWinner::NoWinner
    };
    let confidence = if order_consistent {
        forward.confidence.min(flipped.confidence)
    } else {
        0.0
    };

    (
        final_winner,
        order_consistent,
        confidence,
        vec![
            EvalComparisonOrderVerdict {
                order: EvalComparisonPresentationOrder::BaselineFirst,
                raw_selection: forward.selection,
                normalized_winner: forward_winner,
                confidence: forward.confidence,
                rationale: forward.rationale.clone(),
            },
            EvalComparisonOrderVerdict {
                order: EvalComparisonPresentationOrder::CandidateFirst,
                raw_selection: flipped.selection,
                normalized_winner: flipped_winner,
                confidence: flipped.confidence,
                rationale: flipped.rationale.clone(),
            },
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{
        EvalComparisonPresentationOrder, EvalComparisonWinner, aggregate_pairwise_verdicts,
        normalize_selection,
    };
    use crate::ai_eval_harness::judge_rubric::{EvalPairwiseSelection, ParsedPairwiseResponse};

    #[test]
    fn swapped_order_keeps_candidate_winner_stable() {
        let (winner, order_consistent, confidence, _) = aggregate_pairwise_verdicts(
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::B,
                confidence: 0.93,
                rationale: "B is more direct.".to_string(),
            },
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::A,
                confidence: 0.91,
                rationale: "A is more direct.".to_string(),
            },
        );

        assert_eq!(winner, EvalComparisonWinner::Candidate);
        assert!(order_consistent);
        assert_eq!(confidence, 0.91);
    }

    #[test]
    fn conflicting_order_results_degrade_to_no_winner() {
        let (winner, order_consistent, confidence, _) = aggregate_pairwise_verdicts(
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::A,
                confidence: 0.88,
                rationale: "A wins.".to_string(),
            },
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::A,
                confidence: 0.87,
                rationale: "A wins again.".to_string(),
            },
        );

        assert_eq!(winner, EvalComparisonWinner::NoWinner);
        assert!(!order_consistent);
        assert_eq!(confidence, 0.0);
    }

    #[test]
    fn tie_is_preserved_when_both_orders_tie() {
        let (winner, order_consistent, _, _) = aggregate_pairwise_verdicts(
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::Tie,
                confidence: 0.75,
                rationale: "Equivalent answers.".to_string(),
            },
            &ParsedPairwiseResponse {
                selection: EvalPairwiseSelection::Tie,
                confidence: 0.74,
                rationale: "Equivalent answers.".to_string(),
            },
        );

        assert_eq!(winner, EvalComparisonWinner::Tie);
        assert!(order_consistent);
    }

    #[test]
    fn normalization_maps_presentation_order_to_canonical_winner() {
        assert_eq!(
            normalize_selection(
                EvalPairwiseSelection::A,
                EvalComparisonPresentationOrder::BaselineFirst,
            ),
            EvalComparisonWinner::Baseline
        );
        assert_eq!(
            normalize_selection(
                EvalPairwiseSelection::A,
                EvalComparisonPresentationOrder::CandidateFirst,
            ),
            EvalComparisonWinner::Candidate
        );
    }
}
