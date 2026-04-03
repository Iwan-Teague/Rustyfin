use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EvalThreshold {
    pub metric: String,
    pub actual: f64,
    pub expected: f64,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalSuiteReport {
    pub name: String,
    pub pass: bool,
    pub metrics: BTreeMap<String, f64>,
    pub thresholds: Vec<EvalThreshold>,
    pub case_count: usize,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub generated_at: String,
    pub overall_pass: bool,
    pub suites: Vec<EvalSuiteReport>,
}

impl EvalReport {
    pub fn new(suites: Vec<EvalSuiteReport>) -> Self {
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            overall_pass: suites.iter().all(|suite| suite.pass),
            suites,
        }
    }
}

pub fn write_report_json(path: &Path, report: &EvalReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(report)?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn print_summary(report: &EvalReport) {
    println!(
        "AI eval report {}: {}",
        report.generated_at,
        if report.overall_pass { "PASS" } else { "FAIL" }
    );
    for suite in &report.suites {
        println!(
            "- {}: {} ({} cases)",
            suite.name,
            if suite.pass { "PASS" } else { "FAIL" },
            suite.case_count
        );
        for threshold in &suite.thresholds {
            println!(
                "  {} = {:.3} (threshold {:.3}) [{}]",
                threshold.metric,
                threshold.actual,
                threshold.expected,
                if threshold.pass { "ok" } else { "fail" }
            );
        }
    }
}
