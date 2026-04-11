use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::judge_rubric::EvalPairwiseSelection;
use super::report::{
    EvalFailureBucket, EvalHardGateResult, EvalRunManifest, EvalThreshold, manifest_digest,
};

pub const COMPARISON_JSON_CONTRACT_VERSION: &str = "rustyfin.ai_eval_compare_report.v1";
pub const COMPARISON_MARKDOWN_CONTRACT_VERSION: &str = "rustyfin.ai_eval_compare_markdown.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EvalComparisonWinner {
    Baseline,
    Candidate,
    Tie,
    NoWinner,
}

impl EvalComparisonWinner {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Candidate => "candidate",
            Self::Tie => "tie",
            Self::NoWinner => "no_winner",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalComparisonPresentationOrder {
    BaselineFirst,
    CandidateFirst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalComparisonVariant {
    pub label: String,
    pub model_id: String,
    pub prompt_version: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalComparisonOrderVerdict {
    pub order: EvalComparisonPresentationOrder,
    pub raw_selection: EvalPairwiseSelection,
    pub normalized_winner: EvalComparisonWinner,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalComparisonCaseVerdict {
    pub case_id: String,
    pub pass: bool,
    pub blocker_failure_count: usize,
    pub hard_gates: Vec<EvalHardGateResult>,
    pub failure_buckets: Vec<EvalFailureBucket>,
    pub expected_winner: EvalComparisonWinner,
    pub final_winner: EvalComparisonWinner,
    pub order_consistent: bool,
    pub expected_order_consistent: bool,
    pub confidence: f64,
    pub baseline: EvalComparisonVariant,
    pub candidate: EvalComparisonVariant,
    pub presentation_orders: Vec<EvalComparisonOrderVerdict>,
    pub details: serde_json::Value,
}

impl EvalComparisonCaseVerdict {
    pub fn new(
        case_id: impl Into<String>,
        expected_winner: EvalComparisonWinner,
        final_winner: EvalComparisonWinner,
        order_consistent: bool,
        expected_order_consistent: bool,
        confidence: f64,
        hard_gates: Vec<EvalHardGateResult>,
        baseline: EvalComparisonVariant,
        candidate: EvalComparisonVariant,
        presentation_orders: Vec<EvalComparisonOrderVerdict>,
        details: serde_json::Value,
    ) -> Self {
        let failure_buckets = hard_gates
            .iter()
            .filter(|gate| gate.applicable && !gate.pass)
            .filter_map(|gate| gate.failure_bucket.clone())
            .collect::<Vec<_>>();
        let blocker_failure_count = failure_buckets.len();
        let winner_match = final_winner == expected_winner;
        let order_consistency_match = order_consistent == expected_order_consistent;

        Self {
            case_id: case_id.into(),
            pass: blocker_failure_count == 0 && winner_match && order_consistency_match,
            blocker_failure_count,
            hard_gates,
            failure_buckets,
            expected_winner,
            final_winner,
            order_consistent,
            expected_order_consistent,
            confidence,
            baseline,
            candidate,
            presentation_orders,
            details,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalComparisonReport {
    pub json_contract_version: String,
    pub markdown_contract_version: String,
    pub generated_at: String,
    pub manifest_digest: String,
    pub manifest: EvalRunManifest,
    pub comparison_prompt_version: String,
    pub comparison_schema_version: String,
    pub overall_pass: bool,
    pub case_count: usize,
    pub failed_case_count: usize,
    pub blocker_failure_count: usize,
    pub order_disagreement_count: usize,
    pub tie_count: usize,
    pub no_winner_count: usize,
    pub winner_counts: BTreeMap<String, usize>,
    pub thresholds: Vec<EvalThreshold>,
    pub cases: Vec<EvalComparisonCaseVerdict>,
}

impl EvalComparisonReport {
    pub fn new(
        generated_at: impl Into<String>,
        manifest: EvalRunManifest,
        comparison_prompt_version: impl Into<String>,
        comparison_schema_version: impl Into<String>,
        thresholds: Vec<EvalThreshold>,
        cases: Vec<EvalComparisonCaseVerdict>,
    ) -> Self {
        let manifest_digest = manifest_digest(&manifest);
        let case_count = cases.len();
        let failed_case_count = cases.iter().filter(|case| !case.pass).count();
        let blocker_failure_count = cases.iter().map(|case| case.blocker_failure_count).sum();
        let order_disagreement_count = cases.iter().filter(|case| !case.order_consistent).count();
        let tie_count = cases
            .iter()
            .filter(|case| case.final_winner == EvalComparisonWinner::Tie)
            .count();
        let no_winner_count = cases
            .iter()
            .filter(|case| case.final_winner == EvalComparisonWinner::NoWinner)
            .count();
        let mut winner_counts = BTreeMap::new();
        for case in &cases {
            *winner_counts
                .entry(case.final_winner.as_str().to_string())
                .or_insert(0) += 1;
        }
        let thresholds_pass = thresholds
            .iter()
            .filter(|threshold| threshold.blocking)
            .all(|threshold| threshold.pass);

        Self {
            json_contract_version: COMPARISON_JSON_CONTRACT_VERSION.to_string(),
            markdown_contract_version: COMPARISON_MARKDOWN_CONTRACT_VERSION.to_string(),
            generated_at: generated_at.into(),
            manifest_digest,
            manifest,
            comparison_prompt_version: comparison_prompt_version.into(),
            comparison_schema_version: comparison_schema_version.into(),
            overall_pass: thresholds_pass && cases.iter().all(|case| case.pass),
            case_count,
            failed_case_count,
            blocker_failure_count,
            order_disagreement_count,
            tie_count,
            no_winner_count,
            winner_counts,
            thresholds,
            cases,
        }
    }
}

pub fn write_comparison_report_json(path: &Path, report: &EvalComparisonReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(report)?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn write_comparison_report_markdown(path: &Path, report: &EvalComparisonReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = render_comparison_markdown_report(report);
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn render_comparison_markdown_report(report: &EvalComparisonReport) -> String {
    let mut lines = vec![
        "# AI Pairwise Comparison Report".to_string(),
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
        format!("- Cases: {}", report.case_count),
        format!("- Failed cases: {}", report.failed_case_count),
        format!("- Blocker failures: {}", report.blocker_failure_count),
        format!("- Order disagreements: {}", report.order_disagreement_count),
        format!("- Ties: {}", report.tie_count),
        format!("- No winner: {}", report.no_winner_count),
        String::new(),
        "## Run Manifest".to_string(),
        format!("- Run ID: {}", report.manifest.run_id),
        format!("- Git SHA: {}", report.manifest.git_sha),
        format!("- Base SHA: {}", report.manifest.base_sha),
        format!("- Dataset version: {}", report.manifest.dataset_version),
        format!("- Judge version: {}", report.manifest.judge_version),
        format!("- Rubric version: {}", report.manifest.rubric_version),
        format!(
            "- Comparison prompt version: {}",
            report.comparison_prompt_version
        ),
        format!(
            "- Comparison schema version: {}",
            report.comparison_schema_version
        ),
        String::new(),
        "## Thresholds".to_string(),
    ];

    for threshold in &report.thresholds {
        lines.push(format!(
            "- {} = {:.3} (threshold {:.3}) [{}{}]",
            threshold.metric,
            threshold.actual,
            threshold.expected,
            if threshold.pass { "ok" } else { "fail" },
            if threshold.blocking { "" } else { ", soft" }
        ));
    }

    lines.push(String::new());
    lines.push("## Cases".to_string());
    for case in &report.cases {
        lines.push(format!(
            "- {}: {} [{} -> {}]",
            case.case_id,
            if case.pass { "PASS" } else { "FAIL" },
            case.expected_winner.as_str(),
            case.final_winner.as_str()
        ));
        lines.push(format!(
            "  order_consistent={} confidence={:.3}",
            case.order_consistent, case.confidence
        ));
    }

    lines.join("\n")
}

pub fn print_comparison_summary(report: &EvalComparisonReport) {
    println!(
        "AI comparison report {}: {}",
        report.generated_at,
        if report.overall_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "manifest {} | {} cases | {} order disagreements",
        report.manifest_digest, report.case_count, report.order_disagreement_count
    );
    for threshold in &report.thresholds {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        COMPARISON_JSON_CONTRACT_VERSION, COMPARISON_MARKDOWN_CONTRACT_VERSION,
        EvalComparisonCaseVerdict, EvalComparisonOrderVerdict, EvalComparisonPresentationOrder,
        EvalComparisonReport, EvalComparisonVariant, EvalComparisonWinner,
    };
    use crate::ai_eval_harness::judge_rubric::EvalPairwiseSelection;
    use crate::ai_eval_harness::report::{
        EvalGateSeverity, EvalHardGateResult, EvalRunManifest, EvalThreshold,
    };

    #[test]
    fn comparison_markdown_report_keeps_stable_header_contract() {
        let report = EvalComparisonReport::new(
            "2026-04-11T12:00:00Z",
            EvalRunManifest {
                run_id: "run-compare-1".to_string(),
                git_sha: "abc".to_string(),
                base_sha: "def".to_string(),
                dataset_version: "dataset-v1".to_string(),
                judge_version: "judge-v1".to_string(),
                rubric_version: "rubric-v1".to_string(),
                fixture_digest: "sha256:fixtures".to_string(),
                schema_digest: "sha256:schemas".to_string(),
                tool_registry_digest: "sha256:tools".to_string(),
                model_id: "fixture".to_string(),
                backend_kind: "fixture".to_string(),
                seed: 0,
                timezone: "UTC".to_string(),
                locale: "en-IE".to_string(),
            },
            "compare-prompt-v1",
            "compare-schema-v1",
            vec![EvalThreshold {
                metric: "pairwise_winner_accuracy".to_string(),
                actual: 1.0,
                expected: 1.0,
                pass: true,
                blocking: true,
            }],
            vec![EvalComparisonCaseVerdict::new(
                "case-1",
                EvalComparisonWinner::Candidate,
                EvalComparisonWinner::Candidate,
                true,
                true,
                0.91,
                vec![EvalHardGateResult {
                    gate: "schema_validity".to_string(),
                    severity: EvalGateSeverity::Blocker,
                    applicable: true,
                    pass: true,
                    message: None,
                    failure_bucket: None,
                }],
                EvalComparisonVariant {
                    label: "baseline".to_string(),
                    model_id: "baseline-model".to_string(),
                    prompt_version: "baseline-prompt".to_string(),
                    answer: "Baseline answer".to_string(),
                },
                EvalComparisonVariant {
                    label: "candidate".to_string(),
                    model_id: "candidate-model".to_string(),
                    prompt_version: "candidate-prompt".to_string(),
                    answer: "Candidate answer".to_string(),
                },
                vec![EvalComparisonOrderVerdict {
                    order: EvalComparisonPresentationOrder::BaselineFirst,
                    raw_selection: EvalPairwiseSelection::B,
                    normalized_winner: EvalComparisonWinner::Candidate,
                    confidence: 0.91,
                    rationale: "B is better.".to_string(),
                }],
                json!({"winner_match": true}),
            )],
        );

        let markdown = super::render_comparison_markdown_report(&report);
        assert!(markdown.contains(COMPARISON_JSON_CONTRACT_VERSION));
        assert!(markdown.contains(COMPARISON_MARKDOWN_CONTRACT_VERSION));
        assert!(markdown.contains("compare-prompt-v1"));
    }
}
