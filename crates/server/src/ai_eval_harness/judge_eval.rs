use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::types::AssistantGroundingVisibility;

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    MAX_CASE_NAME_CHARS, MAX_CHUNK_TEXT_CHARS, MAX_GROUNDING_CHUNKS_PER_CASE,
    MAX_MODEL_OUTPUT_CHARS, MAX_PROMPT_CHARS, finalize_case_verdict_with_rubric, inapplicable_gate,
    length_gate, pass_gate, visibility_allowed,
};
use super::judge_metrics::{EvalRubricCalibrationInput, EvalRubricFamily, build_rubric_verdict};
use super::judge_rubric::{
    RUBRIC_PROMPT_VERSION, RUBRIC_RESPONSE_SCHEMA_VERSION, RubricEvidenceChunk,
    build_rubric_prompt, parse_rubric_response,
};
use super::report::{EvalFailureBucket, EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct JudgeCase {
    name: String,
    domain: String,
    mode: String,
    prompt: String,
    assistant_answer: String,
    reference_answer: String,
    #[serde(default = "default_rubric_family")]
    rubric_family: EvalRubricFamily,
    #[serde(default = "default_user_role")]
    user_role: String,
    #[serde(default)]
    evidence_chunks: Vec<JudgeEvidenceChunk>,
    rubric_response: serde_json::Value,
    human_labels: EvalRubricCalibrationInput,
    expected_rubric_pass: bool,
    expected_review_required: bool,
    expected_calibration_agreement: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct JudgeEvidenceChunk {
    id: String,
    text: String,
    visibility: AssistantGroundingVisibility,
}

#[derive(Debug, Clone, Serialize)]
struct JudgeCaseResult {
    name: String,
    domain: String,
    mode: String,
    rubric_pass_match: bool,
    review_required_match: bool,
    calibration_agreement_match: bool,
    expected_rubric_pass: bool,
    expected_review_required: bool,
    expected_calibration_agreement: bool,
    rubric_prompt_version: String,
    rubric_schema_version: String,
    rubric_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parse_error: Option<String>,
}

fn default_rubric_family() -> EvalRubricFamily {
    EvalRubricFamily::ResponseQuality
}

fn default_user_role() -> String {
    "user".to_string()
}

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let spec = FixtureCorpusSpec {
        suite_name: "judge",
        fixture_file: "judge_cases.jsonl",
        schema_file: "judge_cases.schema.json",
    };
    let cases = load_jsonl::<JudgeCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?;

    let mut rubric_pass_matches = 0usize;
    let mut review_required_matches = 0usize;
    let mut calibration_agreement_matches = 0usize;
    let mut details = Vec::new();
    let mut case_verdicts = Vec::new();

    for case in &cases {
        let rubric_prompt = build_rubric_prompt(
            case.rubric_family,
            &case.prompt,
            &case.assistant_answer,
            &case.reference_answer,
            &case
                .evidence_chunks
                .iter()
                .map(|chunk| RubricEvidenceChunk {
                    id: chunk.id.clone(),
                    text: chunk.text.clone(),
                })
                .collect::<Vec<_>>(),
        );
        let rubric_response_raw = serde_json::to_string(&case.rubric_response)?;
        let parsed = parse_rubric_response(&rubric_response_raw, case.rubric_family);
        let rubric = parsed.as_ref().ok().map(|parsed| {
            build_rubric_verdict(
                case.rubric_family,
                RUBRIC_PROMPT_VERSION,
                RUBRIC_RESPONSE_SCHEMA_VERSION,
                parsed.dimensions.clone(),
                parsed.rationale.clone(),
                Some(case.human_labels.clone()),
            )
        });

        let rubric_pass_match = rubric
            .as_ref()
            .map(|rubric| rubric.pass == case.expected_rubric_pass)
            .unwrap_or(false);
        let review_required_match = rubric
            .as_ref()
            .map(|rubric| rubric.requires_human_review == case.expected_review_required)
            .unwrap_or(false);
        let calibration_agreement_match = rubric
            .as_ref()
            .and_then(|rubric| rubric.calibration.as_ref())
            .map(|calibration| calibration.agreement == case.expected_calibration_agreement)
            .unwrap_or(false);

        if rubric_pass_match {
            rubric_pass_matches += 1;
        }
        if review_required_match {
            review_required_matches += 1;
        }
        if calibration_agreement_match {
            calibration_agreement_matches += 1;
        }

        let case_result = JudgeCaseResult {
            name: case.name.clone(),
            domain: case.domain.clone(),
            mode: case.mode.clone(),
            rubric_pass_match,
            review_required_match,
            calibration_agreement_match,
            expected_rubric_pass: case.expected_rubric_pass,
            expected_review_required: case.expected_review_required,
            expected_calibration_agreement: case.expected_calibration_agreement,
            rubric_prompt_version: RUBRIC_PROMPT_VERSION.to_string(),
            rubric_schema_version: RUBRIC_RESPONSE_SCHEMA_VERSION.to_string(),
            rubric_prompt,
            parse_error: parsed.as_ref().err().cloned(),
        };

        let length_limit_gate = {
            let mut violations = Vec::new();
            let name_gate = length_gate(
                "case_name_length",
                case.name.chars().count(),
                MAX_CASE_NAME_CHARS,
            );
            if !name_gate.pass {
                violations.push(name_gate.message.clone().unwrap_or_default());
            }
            let prompt_gate = length_gate(
                "prompt_length",
                case.prompt.chars().count(),
                MAX_PROMPT_CHARS,
            );
            if !prompt_gate.pass {
                violations.push(prompt_gate.message.clone().unwrap_or_default());
            }
            let answer_gate = length_gate(
                "assistant_answer_length",
                case.assistant_answer.chars().count(),
                MAX_MODEL_OUTPUT_CHARS,
            );
            if !answer_gate.pass {
                violations.push(answer_gate.message.clone().unwrap_or_default());
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
                super::judge::fail_gate(
                    "length_limit",
                    EvalFailureBucket::LengthLimitExceeded,
                    violations.join("; "),
                )
            }
        };
        let malformed_output_gate = if let Err(error) = &parsed {
            super::judge::fail_gate(
                "malformed_output",
                EvalFailureBucket::MalformedOutput,
                error.clone(),
            )
        } else {
            pass_gate("malformed_output")
        };
        let acl_gate = if case
            .evidence_chunks
            .iter()
            .all(|chunk| visibility_allowed(&case.user_role, chunk.visibility))
        {
            pass_gate("acl_privacy_boundary")
        } else {
            super::judge::fail_gate(
                "acl_privacy_boundary",
                EvalFailureBucket::PrivacyBoundaryViolation,
                "one or more evidence chunks exceed the declared user role visibility",
            )
        };

        let mut case_metrics = BTreeMap::new();
        case_metrics.insert(
            "rubric_overall_score".to_string(),
            rubric
                .as_ref()
                .map(|rubric| rubric.overall_score)
                .unwrap_or(0.0),
        );
        case_metrics.insert(
            "rubric_pass_match".to_string(),
            if rubric_pass_match { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "review_required_match".to_string(),
            if review_required_match { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "calibration_agreement_match".to_string(),
            if calibration_agreement_match {
                1.0
            } else {
                0.0
            },
        );

        case_verdicts.push(finalize_case_verdict_with_rubric(
            &case.name,
            case_metrics,
            vec![
                pass_gate("schema_validity"),
                malformed_output_gate,
                length_limit_gate,
                inapplicable_gate(
                    "refusal_correctness",
                    "rubric calibration cases do not encode refusal flows in phase 2",
                ),
                acl_gate,
                inapplicable_gate(
                    "exact_answer_contract",
                    "rubric cases are judged through calibration thresholds rather than exact-answer gates",
                ),
            ],
            rubric,
            serde_json::to_value(&case_result)?,
        ));
        details.push(case_result);
    }

    let total = cases.len().max(1) as f64;
    let rubric_pass_match_rate = rubric_pass_matches as f64 / total;
    let review_required_match_rate = review_required_matches as f64 / total;
    let calibration_agreement_match_rate = calibration_agreement_matches as f64 / total;
    let average_rubric_score = case_verdicts
        .iter()
        .filter_map(|case| case.rubric.as_ref().map(|rubric| rubric.overall_score))
        .sum::<f64>()
        / total;

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "judge_rubric_pass_semantics_accuracy".to_string(),
        rubric_pass_match_rate,
    );
    metrics.insert(
        "judge_review_routing_accuracy".to_string(),
        review_required_match_rate,
    );
    metrics.insert(
        "judge_calibration_agreement_accuracy".to_string(),
        calibration_agreement_match_rate,
    );
    metrics.insert(
        "judge_average_rubric_score".to_string(),
        average_rubric_score,
    );

    let thresholds = vec![
        EvalThreshold {
            metric: "judge_rubric_pass_semantics_accuracy".to_string(),
            actual: rubric_pass_match_rate,
            expected: 1.0,
            pass: (rubric_pass_match_rate - 1.0).abs() < f64::EPSILON,
            blocking: true,
        },
        EvalThreshold {
            metric: "judge_review_routing_accuracy".to_string(),
            actual: review_required_match_rate,
            expected: 1.0,
            pass: (review_required_match_rate - 1.0).abs() < f64::EPSILON,
            blocking: true,
        },
        EvalThreshold {
            metric: "judge_calibration_agreement_accuracy".to_string(),
            actual: calibration_agreement_match_rate,
            expected: 1.0,
            pass: (calibration_agreement_match_rate - 1.0).abs() < f64::EPSILON,
            blocking: true,
        },
    ];

    Ok(EvalSuiteReport::finalize(
        "judge",
        metrics,
        thresholds,
        case_verdicts,
        serde_json::to_value(details)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{JudgeCase, run};
    use crate::ai_eval_harness::corpus::{
        FixtureCorpusSpec, fixture_path, fixtures_dir, load_jsonl, schema_path,
    };

    #[test]
    fn judge_fixtures_load_with_human_calibration_labels() {
        let fixtures = fixtures_dir();
        let spec = FixtureCorpusSpec {
            suite_name: "judge",
            fixture_file: "judge_cases.jsonl",
            schema_file: "judge_cases.schema.json",
        };
        let cases = load_jsonl::<JudgeCase>(
            &fixture_path(&fixtures, &spec),
            &schema_path(&fixtures, &spec),
        )
        .unwrap();

        assert!(!cases.is_empty());
        assert!(
            cases
                .iter()
                .all(|case| case.human_labels.dimensions.len() == 4),
            "every judge case should carry four human calibration labels"
        );
    }

    #[test]
    fn judge_suite_matches_review_routing_expectations() {
        let fixtures = fixtures_dir();
        let suite = run(&fixtures).unwrap();

        assert!(suite.pass);
        assert_eq!(suite.case_count, 5);
        assert_eq!(suite.human_review_required_count, 3);
        assert!(
            suite.thresholds.iter().all(|threshold| threshold.pass),
            "judge suite thresholds should match the calibration fixture expectations"
        );
    }
}
