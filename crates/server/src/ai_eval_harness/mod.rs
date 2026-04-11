pub mod comparison_eval;
pub mod corpus;
pub mod execution_eval;
pub mod gate;
pub mod judge;
pub mod judge_eval;
pub mod judge_metrics;
pub mod judge_reports;
pub mod judge_rubric;
pub mod memory_eval;
pub mod planner_eval;
pub mod report;
pub mod retrieval_eval;
pub mod tasks_eval;
pub mod trace_ingest;

use std::path::Path;

use anyhow::{Result, bail};

pub use gate::{EvalGateConfig, EvalGateMode, EvalGateSummary, run_judge_gate};
pub use judge::EvalRunConfig;
pub use judge_reports::{
    EvalComparisonReport, print_comparison_summary, write_comparison_report_json,
    write_comparison_report_markdown,
};
pub use report::{
    EvalReport, EvalRunManifest, EvalSuiteReport, print_summary, write_report_json,
    write_report_markdown,
};

pub async fn run_mode(mode: &str, fixtures_dir: &Path) -> Result<EvalReport> {
    run_mode_with_config(mode, fixtures_dir, EvalRunConfig::default()).await
}

pub async fn run_mode_for_task(mode: &str, fixtures_dir: &Path) -> Result<EvalReport> {
    run_mode_for_task_with_config(mode, fixtures_dir, EvalRunConfig::default()).await
}

pub async fn run_comparison_mode(fixtures_dir: &Path) -> Result<EvalComparisonReport> {
    run_comparison_mode_with_config(fixtures_dir, EvalRunConfig::default()).await
}

pub async fn run_comparison_mode_with_config(
    fixtures_dir: &Path,
    config: EvalRunConfig,
) -> Result<EvalComparisonReport> {
    comparison_eval::run(fixtures_dir, &config)
}

pub async fn run_mode_with_config(
    mode: &str,
    fixtures_dir: &Path,
    config: EvalRunConfig,
) -> Result<EvalReport> {
    run_mode_internal(mode, fixtures_dir, true, &config).await
}

pub async fn run_mode_for_task_with_config(
    mode: &str,
    fixtures_dir: &Path,
    config: EvalRunConfig,
) -> Result<EvalReport> {
    run_mode_internal(mode, fixtures_dir, false, &config).await
}

async fn run_mode_internal(
    mode: &str,
    fixtures_dir: &Path,
    include_self_referential_task_eval: bool,
    config: &EvalRunConfig,
) -> Result<EvalReport> {
    let mut suites = Vec::<EvalSuiteReport>::new();

    match mode {
        "judge" => suites.push(judge_eval::run(fixtures_dir)?),
        "planner" => suites.push(planner_eval::run(fixtures_dir).await?),
        "retrieval" => suites.push(retrieval_eval::run(fixtures_dir)?),
        "memory" => suites.push(memory_eval::run(fixtures_dir)?),
        "execution" => suites.push(execution_eval::run(fixtures_dir)?),
        "tasks" => suites.push(
            tasks_eval::run_with_options(fixtures_dir, include_self_referential_task_eval).await?,
        ),
        "all" | "default" => {
            suites.push(judge_eval::run(fixtures_dir)?);
            suites.push(planner_eval::run(fixtures_dir).await?);
            suites.push(retrieval_eval::run(fixtures_dir)?);
            suites.push(memory_eval::run(fixtures_dir)?);
            suites.push(execution_eval::run(fixtures_dir)?);
            suites.push(
                tasks_eval::run_with_options(fixtures_dir, include_self_referential_task_eval)
                    .await?,
            );
        }
        other => bail!("unknown eval mode: {other}"),
    }

    let manifest = judge::build_run_manifest(mode, fixtures_dir, config)?;
    let generated_at = config
        .generated_at
        .clone()
        .unwrap_or_else(judge::default_generated_at);
    Ok(EvalReport::new(generated_at, manifest, suites))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EvalRunConfig, run_comparison_mode_with_config, run_mode_with_config};
    use crate::ai_eval_harness::judge_reports::COMPARISON_JSON_CONTRACT_VERSION;

    #[tokio::test]
    async fn fixed_manifest_replays_to_same_case_verdicts() {
        let fixtures_dir = super::corpus::fixtures_dir();
        let config = EvalRunConfig {
            generated_at: Some("2026-04-11T12:00:00Z".to_string()),
            run_id: Some("run-1".to_string()),
            git_sha: Some("git-1".to_string()),
            base_sha: Some("base-1".to_string()),
            dataset_version: Some("dataset-v1".to_string()),
            judge_version: Some("judge-v1".to_string()),
            rubric_version: Some("none".to_string()),
            model_id: Some("deterministic".to_string()),
            backend_kind: Some("fixture".to_string()),
            seed: Some(0),
            timezone: Some("UTC".to_string()),
            locale: Some("en-IE".to_string()),
        };

        let left = run_mode_with_config("default", &fixtures_dir, config.clone())
            .await
            .unwrap();
        let right = run_mode_with_config("default", &fixtures_dir, config)
            .await
            .unwrap();

        assert_eq!(left.manifest, right.manifest);
        assert_eq!(left.manifest_digest, right.manifest_digest);
        assert_eq!(left.overall_pass, right.overall_pass);
        assert_eq!(left.blocker_failure_count, right.blocker_failure_count);

        let left_verdicts = left
            .suites
            .iter()
            .map(|suite| {
                json!({
                    "name": suite.name,
                    "pass": suite.pass,
                    "case_verdicts": suite.case_verdicts,
                    "failure_bucket_counts": suite.failure_bucket_counts,
                })
            })
            .collect::<Vec<_>>();
        let right_verdicts = right
            .suites
            .iter()
            .map(|suite| {
                json!({
                    "name": suite.name,
                    "pass": suite.pass,
                    "case_verdicts": suite.case_verdicts,
                    "failure_bucket_counts": suite.failure_bucket_counts,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(left_verdicts, right_verdicts);
    }

    #[tokio::test]
    async fn comparison_mode_stays_separate_from_default_pointwise_report() {
        let fixtures_dir = super::corpus::fixtures_dir();
        let config = EvalRunConfig {
            generated_at: Some("2026-04-11T12:00:00Z".to_string()),
            run_id: Some("run-compare".to_string()),
            git_sha: Some("git-compare".to_string()),
            base_sha: Some("base-compare".to_string()),
            model_id: Some("fixture".to_string()),
            backend_kind: Some("fixture".to_string()),
            seed: Some(0),
            timezone: Some("UTC".to_string()),
            locale: Some("en-IE".to_string()),
            ..EvalRunConfig::default()
        };

        let pointwise = run_mode_with_config("default", &fixtures_dir, config.clone())
            .await
            .unwrap();
        assert!(
            pointwise
                .suites
                .iter()
                .all(|suite| suite.name != "comparison")
        );

        let comparison = run_comparison_mode_with_config(&fixtures_dir, config)
            .await
            .unwrap();
        assert_eq!(
            comparison.json_contract_version,
            COMPARISON_JSON_CONTRACT_VERSION
        );
        assert_ne!(
            comparison.json_contract_version,
            super::report::JSON_CONTRACT_VERSION
        );
    }
}
