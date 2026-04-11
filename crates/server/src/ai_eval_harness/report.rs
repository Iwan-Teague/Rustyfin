use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::judge_metrics::EvalRubricVerdict;

pub const JSON_CONTRACT_VERSION: &str = "rustyfin.ai_eval_report.v3";
pub const MARKDOWN_CONTRACT_VERSION: &str = "rustyfin.ai_eval_markdown.v3";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalThreshold {
    pub metric: String,
    pub actual: f64,
    pub expected: f64,
    pub pass: bool,
    #[serde(default = "default_true")]
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalGateSeverity {
    Blocker,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvalFailureBucket {
    SchemaInvalid,
    MalformedOutput,
    LengthLimitExceeded,
    RefusalMismatch,
    PrivacyBoundaryViolation,
    ExactAnswerMismatch,
}

impl EvalFailureBucket {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SchemaInvalid => "schema_invalid",
            Self::MalformedOutput => "malformed_output",
            Self::LengthLimitExceeded => "length_limit_exceeded",
            Self::RefusalMismatch => "refusal_mismatch",
            Self::PrivacyBoundaryViolation => "privacy_boundary_violation",
            Self::ExactAnswerMismatch => "exact_answer_mismatch",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalHardGateResult {
    pub gate: String,
    pub severity: EvalGateSeverity,
    pub applicable: bool,
    pub pass: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_bucket: Option<EvalFailureBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCaseVerdict {
    pub case_id: String,
    pub pass: bool,
    pub blocker_failure_count: usize,
    pub hard_gates: Vec<EvalHardGateResult>,
    pub failure_buckets: Vec<EvalFailureBucket>,
    pub metrics: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric: Option<EvalRubricVerdict>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalRunManifest {
    pub run_id: String,
    pub git_sha: String,
    pub base_sha: String,
    pub dataset_version: String,
    pub judge_version: String,
    pub rubric_version: String,
    pub fixture_digest: String,
    pub schema_digest: String,
    pub tool_registry_digest: String,
    pub model_id: String,
    pub backend_kind: String,
    pub seed: u64,
    pub timezone: String,
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalSuiteReport {
    pub name: String,
    pub pass: bool,
    pub metrics: BTreeMap<String, f64>,
    pub thresholds: Vec<EvalThreshold>,
    pub case_count: usize,
    pub failed_case_count: usize,
    pub blocker_failure_count: usize,
    pub rubric_case_count: usize,
    pub rubric_pass_count: usize,
    pub human_review_required_count: usize,
    pub calibration_disagreement_count: usize,
    pub failure_bucket_counts: BTreeMap<String, usize>,
    pub case_verdicts: Vec<EvalCaseVerdict>,
    pub details: serde_json::Value,
}

impl EvalSuiteReport {
    pub fn finalize(
        name: impl Into<String>,
        metrics: BTreeMap<String, f64>,
        thresholds: Vec<EvalThreshold>,
        case_verdicts: Vec<EvalCaseVerdict>,
        details: serde_json::Value,
    ) -> Self {
        let case_count = case_verdicts.len();
        let failed_case_count = case_verdicts.iter().filter(|case| !case.pass).count();
        let blocker_failure_count = case_verdicts
            .iter()
            .map(|case| case.blocker_failure_count)
            .sum();
        let rubric_case_count = case_verdicts
            .iter()
            .filter(|case| case.rubric.is_some())
            .count();
        let rubric_pass_count = case_verdicts
            .iter()
            .filter(|case| {
                case.rubric
                    .as_ref()
                    .map(|rubric| rubric.pass)
                    .unwrap_or(false)
            })
            .count();
        let human_review_required_count = case_verdicts
            .iter()
            .filter(|case| {
                case.rubric
                    .as_ref()
                    .map(|rubric| rubric.requires_human_review)
                    .unwrap_or(false)
            })
            .count();
        let calibration_disagreement_count = case_verdicts
            .iter()
            .filter(|case| {
                case.rubric
                    .as_ref()
                    .and_then(|rubric| rubric.calibration.as_ref())
                    .map(|calibration| !calibration.agreement)
                    .unwrap_or(false)
            })
            .count();
        let mut failure_bucket_counts = BTreeMap::new();
        for verdict in &case_verdicts {
            for bucket in &verdict.failure_buckets {
                *failure_bucket_counts
                    .entry(bucket.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
        let thresholds_pass = thresholds
            .iter()
            .filter(|threshold| threshold.blocking)
            .all(|threshold| threshold.pass);
        let verdicts_pass = case_verdicts.iter().all(|case| case.pass);

        Self {
            name: name.into(),
            pass: thresholds_pass && verdicts_pass,
            metrics,
            thresholds,
            case_count,
            failed_case_count,
            blocker_failure_count,
            rubric_case_count,
            rubric_pass_count,
            human_review_required_count,
            calibration_disagreement_count,
            failure_bucket_counts,
            case_verdicts,
            details,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalReport {
    pub json_contract_version: String,
    pub markdown_contract_version: String,
    pub generated_at: String,
    pub manifest_digest: String,
    pub manifest: EvalRunManifest,
    pub overall_pass: bool,
    pub suite_count: usize,
    pub total_case_count: usize,
    pub failed_case_count: usize,
    pub blocker_failure_count: usize,
    pub rubric_case_count: usize,
    pub rubric_pass_count: usize,
    pub human_review_required_count: usize,
    pub calibration_disagreement_count: usize,
    pub failure_bucket_counts: BTreeMap<String, usize>,
    pub suites: Vec<EvalSuiteReport>,
}

impl EvalReport {
    pub fn new(
        generated_at: impl Into<String>,
        manifest: EvalRunManifest,
        suites: Vec<EvalSuiteReport>,
    ) -> Self {
        let manifest_digest = manifest_digest(&manifest);
        let total_case_count = suites.iter().map(|suite| suite.case_count).sum();
        let failed_case_count = suites.iter().map(|suite| suite.failed_case_count).sum();
        let blocker_failure_count = suites.iter().map(|suite| suite.blocker_failure_count).sum();
        let rubric_case_count = suites.iter().map(|suite| suite.rubric_case_count).sum();
        let rubric_pass_count = suites.iter().map(|suite| suite.rubric_pass_count).sum();
        let human_review_required_count = suites
            .iter()
            .map(|suite| suite.human_review_required_count)
            .sum();
        let calibration_disagreement_count = suites
            .iter()
            .map(|suite| suite.calibration_disagreement_count)
            .sum();
        let mut failure_bucket_counts = BTreeMap::new();
        for suite in &suites {
            for (bucket, count) in &suite.failure_bucket_counts {
                *failure_bucket_counts.entry(bucket.clone()).or_insert(0) += count;
            }
        }

        Self {
            json_contract_version: JSON_CONTRACT_VERSION.to_string(),
            markdown_contract_version: MARKDOWN_CONTRACT_VERSION.to_string(),
            generated_at: generated_at.into(),
            manifest_digest,
            manifest,
            overall_pass: suites.iter().all(|suite| suite.pass),
            suite_count: suites.len(),
            total_case_count,
            failed_case_count,
            blocker_failure_count,
            rubric_case_count,
            rubric_pass_count,
            human_review_required_count,
            calibration_disagreement_count,
            failure_bucket_counts,
            suites,
        }
    }
}

pub fn manifest_digest(manifest: &EvalRunManifest) -> String {
    let bytes = serde_json::to_vec(manifest).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

pub fn write_report_json(path: &Path, report: &EvalReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(report)?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn write_report_markdown(path: &Path, report: &EvalReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = render_markdown_report(report);
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn render_markdown_report(report: &EvalReport) -> String {
    let mut lines = vec![
        "# AI Eval Report".to_string(),
        String::new(),
        format!("Generated: {}", report.generated_at),
        format!("JSON contract: {}", report.json_contract_version),
        format!("Markdown contract: {}", report.markdown_contract_version),
        format!("Manifest digest: {}", report.manifest_digest),
        String::new(),
        format!(
            "Overall result: {}",
            if report.overall_pass { "PASS" } else { "FAIL" }
        ),
        format!("- Suites: {}", report.suite_count),
        format!("- Cases: {}", report.total_case_count),
        format!("- Failed cases: {}", report.failed_case_count),
        format!("- Blocker failures: {}", report.blocker_failure_count),
        format!("- Rubric cases: {}", report.rubric_case_count),
        format!("- Rubric soft passes: {}", report.rubric_pass_count),
        format!(
            "- Human review required: {}",
            report.human_review_required_count
        ),
        String::new(),
        "## Run Manifest".to_string(),
        format!("- Run ID: {}", report.manifest.run_id),
        format!("- Git SHA: {}", report.manifest.git_sha),
        format!("- Base SHA: {}", report.manifest.base_sha),
        format!("- Dataset version: {}", report.manifest.dataset_version),
        format!("- Judge version: {}", report.manifest.judge_version),
        format!("- Rubric version: {}", report.manifest.rubric_version),
        format!("- Fixture digest: {}", report.manifest.fixture_digest),
        format!("- Schema digest: {}", report.manifest.schema_digest),
        format!(
            "- Tool registry digest: {}",
            report.manifest.tool_registry_digest
        ),
        format!("- Model ID: {}", report.manifest.model_id),
        format!("- Backend kind: {}", report.manifest.backend_kind),
        format!("- Seed: {}", report.manifest.seed),
        format!("- Timezone: {}", report.manifest.timezone),
        format!("- Locale: {}", report.manifest.locale),
        String::new(),
    ];

    if !report.failure_bucket_counts.is_empty() {
        lines.push("## Failure Buckets".to_string());
        for (bucket, count) in &report.failure_bucket_counts {
            lines.push(format!("- {}: {}", bucket, count));
        }
        lines.push(String::new());
    }

    for suite in &report.suites {
        lines.push(format!("## {}", suite.name));
        lines.push(format!(
            "- Result: {}",
            if suite.pass { "PASS" } else { "FAIL" }
        ));
        lines.push(format!("- Cases: {}", suite.case_count));
        lines.push(format!("- Failed cases: {}", suite.failed_case_count));
        lines.push(format!(
            "- Blocker failures: {}",
            suite.blocker_failure_count
        ));
        if suite.rubric_case_count > 0 {
            lines.push(format!("- Rubric cases: {}", suite.rubric_case_count));
            lines.push(format!("- Rubric soft passes: {}", suite.rubric_pass_count));
            lines.push(format!(
                "- Human review required: {}",
                suite.human_review_required_count
            ));
            lines.push(format!(
                "- Calibration disagreements: {}",
                suite.calibration_disagreement_count
            ));
        }
        for threshold in &suite.thresholds {
            lines.push(format!(
                "- {} = {:.3} (threshold {:.3}) [{}{}]",
                threshold.metric,
                threshold.actual,
                threshold.expected,
                if threshold.pass { "ok" } else { "fail" },
                if threshold.blocking { "" } else { ", soft" }
            ));
        }

        let failed_cases = suite
            .case_verdicts
            .iter()
            .filter(|case| !case.pass)
            .collect::<Vec<_>>();
        if !failed_cases.is_empty() {
            lines.push("- Failed case verdicts:".to_string());
            for verdict in failed_cases {
                let buckets = if verdict.failure_buckets.is_empty() {
                    "none".to_string()
                } else {
                    verdict
                        .failure_buckets
                        .iter()
                        .map(EvalFailureBucket::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                lines.push(format!(
                    "  - {} [{} blocker failures] ({})",
                    verdict.case_id, verdict.blocker_failure_count, buckets
                ));
            }
        }
        let rubric_soft_failures = suite
            .case_verdicts
            .iter()
            .filter_map(|case| case.rubric.as_ref().map(|rubric| (&case.case_id, rubric)))
            .filter(|(_, rubric)| !rubric.pass || rubric.requires_human_review)
            .collect::<Vec<_>>();
        if !rubric_soft_failures.is_empty() {
            lines.push("- Rubric review cases:".to_string());
            for (case_id, rubric) in rubric_soft_failures {
                let review_reasons = if rubric.review_reasons.is_empty() {
                    "none".to_string()
                } else {
                    rubric
                        .review_reasons
                        .iter()
                        .map(|reason| reason.as_str().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                lines.push(format!(
                    "  - {} [rubric pass: {}, score {:.3}, review: {}]",
                    case_id, rubric.pass, rubric.overall_score, review_reasons
                ));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

pub fn print_summary(report: &EvalReport) {
    println!(
        "AI eval report {}: {}",
        report.generated_at,
        if report.overall_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "manifest {} | {} cases | {} blocker failures",
        report.manifest_digest, report.total_case_count, report.blocker_failure_count
    );
    if report.rubric_case_count > 0 {
        println!(
            "rubric {} cases | {} soft passes | {} review cases",
            report.rubric_case_count, report.rubric_pass_count, report.human_review_required_count
        );
    }
    for suite in &report.suites {
        println!(
            "- {}: {} ({} cases, {} failed cases, {} blocker failures, {} rubric cases)",
            suite.name,
            if suite.pass { "PASS" } else { "FAIL" },
            suite.case_count,
            suite.failed_case_count,
            suite.blocker_failure_count,
            suite.rubric_case_count,
        );
        for threshold in &suite.thresholds {
            println!(
                "  {} = {:.3} (threshold {:.3}) [{}{}]",
                threshold.metric,
                threshold.actual,
                threshold.expected,
                if threshold.pass { "ok" } else { "fail" },
                if threshold.blocking { "" } else { ", soft" }
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::ai_eval_harness::judge_metrics::{
        EvalRubricDimension, EvalRubricDimensionVerdict, EvalRubricFamily, EvalRubricVerdict,
    };

    use super::{
        EvalCaseVerdict, EvalFailureBucket, EvalGateSeverity, EvalHardGateResult, EvalReport,
        EvalRunManifest, EvalSuiteReport, EvalThreshold, JSON_CONTRACT_VERSION,
        MARKDOWN_CONTRACT_VERSION,
    };

    #[test]
    fn manifest_digest_is_stable_for_same_manifest() {
        let manifest = EvalRunManifest {
            run_id: "run-1".to_string(),
            git_sha: "abc".to_string(),
            base_sha: "def".to_string(),
            dataset_version: "dataset-v1".to_string(),
            judge_version: "judge-v1".to_string(),
            rubric_version: "none".to_string(),
            fixture_digest: "sha256:fixtures".to_string(),
            schema_digest: "sha256:schemas".to_string(),
            tool_registry_digest: "sha256:tools".to_string(),
            model_id: "deterministic".to_string(),
            backend_kind: "fixture".to_string(),
            seed: 0,
            timezone: "UTC".to_string(),
            locale: "en-IE".to_string(),
        };
        assert_eq!(
            super::manifest_digest(&manifest),
            super::manifest_digest(&manifest)
        );
    }

    #[test]
    fn markdown_report_keeps_stable_header_contract() {
        let suite = EvalSuiteReport::finalize(
            "planner",
            BTreeMap::new(),
            vec![EvalThreshold {
                metric: "planner_exact_tool_accuracy".to_string(),
                actual: 1.0,
                expected: 0.9,
                pass: true,
                blocking: true,
            }],
            vec![EvalCaseVerdict {
                case_id: "case-1".to_string(),
                pass: true,
                blocker_failure_count: 0,
                hard_gates: vec![EvalHardGateResult {
                    gate: "schema_validity".to_string(),
                    severity: EvalGateSeverity::Blocker,
                    applicable: true,
                    pass: true,
                    message: None,
                    failure_bucket: None,
                }],
                failure_buckets: Vec::new(),
                metrics: BTreeMap::new(),
                rubric: None,
                details: json!({"ok": true}),
            }],
            json!([]),
        );
        let report = EvalReport::new(
            "2026-04-11T12:00:00Z",
            EvalRunManifest {
                run_id: "run-1".to_string(),
                git_sha: "abc".to_string(),
                base_sha: "def".to_string(),
                dataset_version: "dataset-v1".to_string(),
                judge_version: "judge-v1".to_string(),
                rubric_version: "none".to_string(),
                fixture_digest: "sha256:fixtures".to_string(),
                schema_digest: "sha256:schemas".to_string(),
                tool_registry_digest: "sha256:tools".to_string(),
                model_id: "deterministic".to_string(),
                backend_kind: "fixture".to_string(),
                seed: 0,
                timezone: "UTC".to_string(),
                locale: "en-IE".to_string(),
            },
            vec![suite],
        );
        let markdown = super::render_markdown_report(&report);
        assert!(markdown.contains("# AI Eval Report"));
        assert!(markdown.contains("Overall result: PASS"));
        assert!(markdown.contains(JSON_CONTRACT_VERSION));
        assert!(markdown.contains(MARKDOWN_CONTRACT_VERSION));
    }

    #[test]
    fn suite_finalize_counts_failed_buckets() {
        let suite = EvalSuiteReport::finalize(
            "planner",
            BTreeMap::new(),
            Vec::new(),
            vec![EvalCaseVerdict {
                case_id: "case-1".to_string(),
                pass: false,
                blocker_failure_count: 2,
                hard_gates: Vec::new(),
                failure_buckets: vec![
                    EvalFailureBucket::MalformedOutput,
                    EvalFailureBucket::ExactAnswerMismatch,
                ],
                metrics: BTreeMap::new(),
                rubric: None,
                details: json!({}),
            }],
            json!([]),
        );
        assert!(!suite.pass);
        assert_eq!(suite.failed_case_count, 1);
        assert_eq!(suite.blocker_failure_count, 2);
        assert_eq!(
            suite.failure_bucket_counts.get("malformed_output"),
            Some(&1usize)
        );
    }

    #[test]
    fn non_blocking_thresholds_do_not_flip_suite_pass() {
        let rubric = EvalRubricVerdict {
            family: EvalRubricFamily::ResponseQuality,
            prompt_version: "rubric-v1".to_string(),
            schema_version: "schema-v1".to_string(),
            pass: false,
            overall_score: 0.70,
            threshold: 0.75,
            confidence: 0.95,
            rationale: "soft failure".to_string(),
            dimensions: vec![EvalRubricDimensionVerdict {
                dimension: EvalRubricDimension::Concision,
                pass: true,
                score: 0.70,
                confidence: 0.95,
                reason: "acceptable".to_string(),
            }],
            requires_human_review: true,
            review_reasons: Vec::new(),
            calibration: None,
        };
        let suite = EvalSuiteReport::finalize(
            "judge",
            BTreeMap::new(),
            vec![EvalThreshold {
                metric: "rubric_pass_rate".to_string(),
                actual: 0.50,
                expected: 0.75,
                pass: false,
                blocking: false,
            }],
            vec![EvalCaseVerdict {
                case_id: "case-1".to_string(),
                pass: true,
                blocker_failure_count: 0,
                hard_gates: Vec::new(),
                failure_buckets: Vec::new(),
                metrics: BTreeMap::new(),
                rubric: Some(rubric),
                details: json!({}),
            }],
            json!([]),
        );

        assert!(suite.pass);
        assert_eq!(suite.rubric_case_count, 1);
        assert_eq!(suite.rubric_pass_count, 0);
        assert_eq!(suite.human_review_required_count, 1);
    }

    #[test]
    fn hard_gate_failure_still_fails_suite_even_with_passing_rubric() {
        let rubric = EvalRubricVerdict {
            family: EvalRubricFamily::ResponseQuality,
            prompt_version: "rubric-v1".to_string(),
            schema_version: "schema-v1".to_string(),
            pass: true,
            overall_score: 0.91,
            threshold: 0.75,
            confidence: 0.95,
            rationale: "good answer".to_string(),
            dimensions: vec![EvalRubricDimensionVerdict {
                dimension: EvalRubricDimension::Concision,
                pass: true,
                score: 0.91,
                confidence: 0.95,
                reason: "brief".to_string(),
            }],
            requires_human_review: false,
            review_reasons: Vec::new(),
            calibration: None,
        };
        let suite = EvalSuiteReport::finalize(
            "judge",
            BTreeMap::new(),
            vec![EvalThreshold {
                metric: "judge_rubric_pass_semantics_accuracy".to_string(),
                actual: 1.0,
                expected: 1.0,
                pass: true,
                blocking: true,
            }],
            vec![EvalCaseVerdict {
                case_id: "case-1".to_string(),
                pass: false,
                blocker_failure_count: 1,
                hard_gates: vec![EvalHardGateResult {
                    gate: "acl_privacy_boundary".to_string(),
                    severity: EvalGateSeverity::Blocker,
                    applicable: true,
                    pass: false,
                    message: Some("privacy boundary crossed".to_string()),
                    failure_bucket: Some(EvalFailureBucket::PrivacyBoundaryViolation),
                }],
                failure_buckets: vec![EvalFailureBucket::PrivacyBoundaryViolation],
                metrics: BTreeMap::new(),
                rubric: Some(rubric),
                details: json!({}),
            }],
            json!([]),
        );

        assert!(!suite.pass);
        assert_eq!(suite.failed_case_count, 1);
        assert_eq!(suite.rubric_pass_count, 1);
    }
}
