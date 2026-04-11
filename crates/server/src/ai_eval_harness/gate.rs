use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::judge::{EvalRunConfig, build_run_manifest, default_generated_at};
use super::judge_reports::{
    COMPARISON_JSON_CONTRACT_VERSION, EvalComparisonReport, write_comparison_report_json,
    write_comparison_report_markdown,
};
use super::report::{
    EvalReport, EvalRunManifest, JSON_CONTRACT_VERSION, write_report_json, write_report_markdown,
};
use super::{run_comparison_mode_with_config, run_mode_with_config};

pub const GATE_SUMMARY_JSON_CONTRACT_VERSION: &str = "rustyfin.ai_eval_gate_summary.v1";
pub const GATE_SUMMARY_MARKDOWN_CONTRACT_VERSION: &str = "rustyfin.ai_eval_gate_markdown.v1";

const REQUIRED_POINTWISE_SUITES: &[&str] = &[
    "judge",
    "planner",
    "retrieval",
    "memory",
    "execution",
    "tasks",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalGateMode {
    Smoke,
    Release,
}

impl EvalGateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Release => "release",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EvalGateConfig {
    pub mode: EvalGateMode,
    pub artifacts_dir: PathBuf,
    pub run_config: EvalRunConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvalGateArtifactPaths {
    pub artifacts_dir: String,
    pub pointwise_json: String,
    pub pointwise_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_markdown: Option<String>,
    pub summary_json: String,
    pub summary_markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalGateSummary {
    pub json_contract_version: String,
    pub markdown_contract_version: String,
    pub generated_at: String,
    pub mode: EvalGateMode,
    pub gate_pass: bool,
    pub manifest_pinned: bool,
    pub deterministic_replay_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_replay_match: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_suites: Vec<String>,
    pub pointwise_overall_pass: bool,
    pub pointwise_failed_case_count: usize,
    pub pointwise_blocker_failure_count: usize,
    pub pointwise_manifest_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_manifest_digest: Option<String>,
    pub pointwise_json_contract_version: String,
    pub pointwise_markdown_contract_version: String,
    pub comparison_informational_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_overall_pass: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_json_contract_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_markdown_contract_version: Option<String>,
    pub manifest: EvalRunManifest,
    pub artifacts: EvalGateArtifactPaths,
}

pub async fn run_judge_gate(
    fixtures_dir: &Path,
    config: EvalGateConfig,
) -> Result<EvalGateSummary> {
    std::fs::create_dir_all(&config.artifacts_dir)
        .with_context(|| format!("failed to create {}", config.artifacts_dir.display()))?;

    let artifacts = artifact_paths(&config.artifacts_dir, config.mode);
    let run_config = match config.mode {
        EvalGateMode::Smoke => normalize_smoke_config(config.run_config),
        EvalGateMode::Release => pin_release_config(fixtures_dir, config.run_config)?,
    };
    let manifest_pinned = manifest_fields_are_pinned(&run_config);

    let pointwise = run_mode_with_config("default", fixtures_dir, run_config.clone()).await?;
    write_report_json(Path::new(&artifacts.pointwise_json), &pointwise)?;
    write_report_markdown(Path::new(&artifacts.pointwise_markdown), &pointwise)?;

    let mut failure_reasons = validate_pointwise_artifacts(
        &pointwise,
        Path::new(&artifacts.pointwise_json),
        Path::new(&artifacts.pointwise_markdown),
    )?;
    let missing_suites = missing_required_suites(&pointwise);
    if !missing_suites.is_empty() {
        failure_reasons.push(format!(
            "pointwise report is missing required suites: {}",
            missing_suites.join(", ")
        ));
    }
    if !pointwise.overall_pass {
        failure_reasons.push(format!(
            "pointwise judge report failed with {} failed cases and {} blocker failures",
            pointwise.failed_case_count, pointwise.blocker_failure_count
        ));
    }

    let (replay_manifest_digest, deterministic_replay_match) = match config.mode {
        EvalGateMode::Smoke => (None, None),
        EvalGateMode::Release => {
            let replay = run_mode_with_config("default", fixtures_dir, run_config.clone()).await?;
            let replay_json = artifacts
                .replay_json
                .as_deref()
                .context("release gate is missing replay JSON artifact path")?;
            let replay_markdown = artifacts
                .replay_markdown
                .as_deref()
                .context("release gate is missing replay markdown artifact path")?;
            write_report_json(Path::new(replay_json), &replay)?;
            write_report_markdown(Path::new(replay_markdown), &replay)?;
            failure_reasons.extend(validate_pointwise_artifacts(
                &replay,
                Path::new(replay_json),
                Path::new(replay_markdown),
            )?);
            let replay_match = pointwise == replay;
            if !replay_match {
                failure_reasons.push(
                    "release replay mismatch: repeated pinned run produced a different report"
                        .to_string(),
                );
            }
            (Some(replay.manifest_digest.clone()), Some(replay_match))
        }
    };

    let (
        comparison_overall_pass,
        comparison_json_contract_version,
        comparison_markdown_contract_version,
    ) = match config.mode {
        EvalGateMode::Smoke => (None, None, None),
        EvalGateMode::Release => {
            let comparison =
                run_comparison_mode_with_config(fixtures_dir, run_config.clone()).await?;
            let comparison_json = artifacts
                .comparison_json
                .as_deref()
                .context("release gate is missing comparison JSON artifact path")?;
            let comparison_markdown = artifacts
                .comparison_markdown
                .as_deref()
                .context("release gate is missing comparison markdown artifact path")?;
            write_comparison_report_json(Path::new(comparison_json), &comparison)?;
            write_comparison_report_markdown(Path::new(comparison_markdown), &comparison)?;
            failure_reasons.extend(validate_comparison_artifacts(
                &comparison,
                Path::new(comparison_json),
                Path::new(comparison_markdown),
            )?);
            (
                Some(comparison.overall_pass),
                Some(comparison.json_contract_version.clone()),
                Some(comparison.markdown_contract_version.clone()),
            )
        }
    };

    let gate_pass = failure_reasons.is_empty();
    let summary = EvalGateSummary {
        json_contract_version: GATE_SUMMARY_JSON_CONTRACT_VERSION.to_string(),
        markdown_contract_version: GATE_SUMMARY_MARKDOWN_CONTRACT_VERSION.to_string(),
        generated_at: run_config
            .generated_at
            .clone()
            .unwrap_or_else(default_generated_at),
        mode: config.mode,
        gate_pass,
        manifest_pinned,
        deterministic_replay_required: matches!(config.mode, EvalGateMode::Release),
        deterministic_replay_match,
        failure_reasons,
        missing_suites,
        pointwise_overall_pass: pointwise.overall_pass,
        pointwise_failed_case_count: pointwise.failed_case_count,
        pointwise_blocker_failure_count: pointwise.blocker_failure_count,
        pointwise_manifest_digest: pointwise.manifest_digest.clone(),
        replay_manifest_digest,
        pointwise_json_contract_version: pointwise.json_contract_version.clone(),
        pointwise_markdown_contract_version: pointwise.markdown_contract_version.clone(),
        comparison_informational_only: true,
        comparison_overall_pass,
        comparison_json_contract_version,
        comparison_markdown_contract_version,
        manifest: pointwise.manifest.clone(),
        artifacts,
    };

    write_gate_summary_json(Path::new(&summary.artifacts.summary_json), &summary)?;
    write_gate_summary_markdown(Path::new(&summary.artifacts.summary_markdown), &summary)?;
    validate_gate_summary_artifacts(
        &summary,
        Path::new(&summary.artifacts.summary_json),
        Path::new(&summary.artifacts.summary_markdown),
    )?;

    Ok(summary)
}

pub fn write_gate_summary_json(path: &Path, summary: &EvalGateSummary) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(summary)?;
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn write_gate_summary_markdown(path: &Path, summary: &EvalGateSummary) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = render_gate_summary_markdown(summary);
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub fn render_gate_summary_markdown(summary: &EvalGateSummary) -> String {
    let mut lines = vec![
        "# AI Judge Gate Summary".to_string(),
        String::new(),
        format!("Generated: {}", summary.generated_at),
        format!("JSON contract: {}", summary.json_contract_version),
        format!("Markdown contract: {}", summary.markdown_contract_version),
        format!("Mode: {}", summary.mode.as_str()),
        format!(
            "Result: {}",
            if summary.gate_pass { "PASS" } else { "FAIL" }
        ),
        format!("Manifest pinned: {}", summary.manifest_pinned),
        format!(
            "Deterministic replay required: {}",
            summary.deterministic_replay_required
        ),
        format!(
            "Deterministic replay match: {}",
            summary
                .deterministic_replay_match
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string())
        ),
        String::new(),
        "## Pointwise Report".to_string(),
        format!("- Overall pass: {}", summary.pointwise_overall_pass),
        format!("- Failed cases: {}", summary.pointwise_failed_case_count),
        format!(
            "- Blocker failures: {}",
            summary.pointwise_blocker_failure_count
        ),
        format!("- Manifest digest: {}", summary.pointwise_manifest_digest),
        format!(
            "- JSON contract: {}",
            summary.pointwise_json_contract_version
        ),
        format!(
            "- Markdown contract: {}",
            summary.pointwise_markdown_contract_version
        ),
        format!("- JSON artifact: {}", summary.artifacts.pointwise_json),
        format!(
            "- Markdown artifact: {}",
            summary.artifacts.pointwise_markdown
        ),
        String::new(),
    ];

    if let Some(replay_manifest_digest) = summary.replay_manifest_digest.as_ref() {
        lines.push("## Replay".to_string());
        lines.push(format!(
            "- Replay manifest digest: {}",
            replay_manifest_digest
        ));
        lines.push(format!(
            "- Replay JSON artifact: {}",
            summary.artifacts.replay_json.as_deref().unwrap_or("n/a")
        ));
        lines.push(format!(
            "- Replay Markdown artifact: {}",
            summary
                .artifacts
                .replay_markdown
                .as_deref()
                .unwrap_or("n/a")
        ));
        lines.push(String::new());
    }

    if let Some(comparison_overall_pass) = summary.comparison_overall_pass {
        lines.push("## Comparison Artifact".to_string());
        lines.push(format!(
            "- Informational only: {}",
            summary.comparison_informational_only
        ));
        lines.push(format!("- Overall pass: {}", comparison_overall_pass));
        lines.push(format!(
            "- JSON contract: {}",
            summary
                .comparison_json_contract_version
                .as_deref()
                .unwrap_or("n/a")
        ));
        lines.push(format!(
            "- Markdown contract: {}",
            summary
                .comparison_markdown_contract_version
                .as_deref()
                .unwrap_or("n/a")
        ));
        lines.push(format!(
            "- JSON artifact: {}",
            summary
                .artifacts
                .comparison_json
                .as_deref()
                .unwrap_or("n/a")
        ));
        lines.push(format!(
            "- Markdown artifact: {}",
            summary
                .artifacts
                .comparison_markdown
                .as_deref()
                .unwrap_or("n/a")
        ));
        lines.push(String::new());
    }

    if !summary.missing_suites.is_empty() {
        lines.push("## Missing Suites".to_string());
        for suite in &summary.missing_suites {
            lines.push(format!("- {}", suite));
        }
        lines.push(String::new());
    }

    if !summary.failure_reasons.is_empty() {
        lines.push("## Failure Reasons".to_string());
        for reason in &summary.failure_reasons {
            lines.push(format!("- {}", reason));
        }
        lines.push(String::new());
    }

    lines.push("## Manifest".to_string());
    lines.push(format!("- Run ID: {}", summary.manifest.run_id));
    lines.push(format!("- Git SHA: {}", summary.manifest.git_sha));
    lines.push(format!("- Base SHA: {}", summary.manifest.base_sha));
    lines.push(format!(
        "- Dataset version: {}",
        summary.manifest.dataset_version
    ));
    lines.push(format!(
        "- Judge version: {}",
        summary.manifest.judge_version
    ));
    lines.push(format!(
        "- Rubric version: {}",
        summary.manifest.rubric_version
    ));
    lines.push(format!(
        "- Fixture digest: {}",
        summary.manifest.fixture_digest
    ));
    lines.push(format!(
        "- Schema digest: {}",
        summary.manifest.schema_digest
    ));
    lines.push(format!(
        "- Tool registry digest: {}",
        summary.manifest.tool_registry_digest
    ));
    lines.push(format!("- Model ID: {}", summary.manifest.model_id));
    lines.push(format!("- Backend kind: {}", summary.manifest.backend_kind));
    lines.push(format!("- Seed: {}", summary.manifest.seed));
    lines.push(format!("- Timezone: {}", summary.manifest.timezone));
    lines.push(format!("- Locale: {}", summary.manifest.locale));
    lines.push(String::new());
    lines.push(format!(
        "- Summary JSON artifact: {}",
        summary.artifacts.summary_json
    ));
    lines.push(format!(
        "- Summary Markdown artifact: {}",
        summary.artifacts.summary_markdown
    ));
    lines.join("\n")
}

fn artifact_paths(root: &Path, mode: EvalGateMode) -> EvalGateArtifactPaths {
    let mode_name = mode.as_str();
    EvalGateArtifactPaths {
        artifacts_dir: root.display().to_string(),
        pointwise_json: root
            .join(format!("ai-judge-{}-pointwise.json", mode_name))
            .display()
            .to_string(),
        pointwise_markdown: root
            .join(format!("ai-judge-{}-pointwise.md", mode_name))
            .display()
            .to_string(),
        replay_json: matches!(mode, EvalGateMode::Release).then(|| {
            root.join("ai-judge-release-replay.json")
                .display()
                .to_string()
        }),
        replay_markdown: matches!(mode, EvalGateMode::Release).then(|| {
            root.join("ai-judge-release-replay.md")
                .display()
                .to_string()
        }),
        comparison_json: matches!(mode, EvalGateMode::Release).then(|| {
            root.join("ai-judge-release-comparison.json")
                .display()
                .to_string()
        }),
        comparison_markdown: matches!(mode, EvalGateMode::Release).then(|| {
            root.join("ai-judge-release-comparison.md")
                .display()
                .to_string()
        }),
        summary_json: root
            .join(format!("ai-judge-{}-summary.json", mode_name))
            .display()
            .to_string(),
        summary_markdown: root
            .join(format!("ai-judge-{}-summary.md", mode_name))
            .display()
            .to_string(),
    }
}

fn normalize_smoke_config(mut config: EvalRunConfig) -> EvalRunConfig {
    if config.generated_at.is_none() {
        config.generated_at = Some(default_generated_at());
    }
    if config.run_id.is_none() {
        config.run_id = Some(format!(
            "judge-smoke-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
    }
    config
}

fn pin_release_config(fixtures_dir: &Path, config: EvalRunConfig) -> Result<EvalRunConfig> {
    let resolved_manifest = build_run_manifest("default", fixtures_dir, &config)?;
    validate_release_manifest(&resolved_manifest)?;
    Ok(EvalRunConfig {
        generated_at: Some(config.generated_at.unwrap_or_else(default_generated_at)),
        run_id: Some(config.run_id.unwrap_or_else(|| {
            format!(
                "judge-release-{}",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            )
        })),
        git_sha: Some(resolved_manifest.git_sha),
        base_sha: Some(resolved_manifest.base_sha),
        dataset_version: Some(resolved_manifest.dataset_version),
        judge_version: Some(resolved_manifest.judge_version),
        rubric_version: Some(resolved_manifest.rubric_version),
        model_id: Some(resolved_manifest.model_id),
        backend_kind: Some(resolved_manifest.backend_kind),
        seed: Some(resolved_manifest.seed),
        timezone: Some(resolved_manifest.timezone),
        locale: Some(resolved_manifest.locale),
    })
}

fn validate_release_manifest(manifest: &EvalRunManifest) -> Result<()> {
    let mut missing = Vec::new();
    if manifest.git_sha.trim().is_empty() || manifest.git_sha == "unknown" {
        missing.push("git_sha");
    }
    if manifest.base_sha.trim().is_empty() || manifest.base_sha == "unknown" {
        missing.push("base_sha");
    }
    if manifest.dataset_version.trim().is_empty() {
        missing.push("dataset_version");
    }
    if manifest.judge_version.trim().is_empty() {
        missing.push("judge_version");
    }
    if manifest.rubric_version.trim().is_empty() {
        missing.push("rubric_version");
    }
    if manifest.model_id.trim().is_empty() {
        missing.push("model_id");
    }
    if manifest.backend_kind.trim().is_empty() {
        missing.push("backend_kind");
    }
    if manifest.timezone.trim().is_empty() {
        missing.push("timezone");
    }
    if manifest.locale.trim().is_empty() {
        missing.push("locale");
    }
    if !missing.is_empty() {
        bail!(
            "release gate requires pinned manifest fields; missing or unknown: {}",
            missing.join(", ")
        );
    }
    Ok(())
}

fn manifest_fields_are_pinned(config: &EvalRunConfig) -> bool {
    config.generated_at.is_some()
        && config.run_id.is_some()
        && config.git_sha.is_some()
        && config.base_sha.is_some()
        && config.dataset_version.is_some()
        && config.judge_version.is_some()
        && config.rubric_version.is_some()
        && config.model_id.is_some()
        && config.backend_kind.is_some()
        && config.seed.is_some()
        && config.timezone.is_some()
        && config.locale.is_some()
}

fn missing_required_suites(report: &EvalReport) -> Vec<String> {
    REQUIRED_POINTWISE_SUITES
        .iter()
        .filter(|required| report.suites.iter().all(|suite| suite.name != **required))
        .map(|suite| (*suite).to_string())
        .collect()
}

fn validate_pointwise_artifacts(
    report: &EvalReport,
    json_path: &Path,
    markdown_path: &Path,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    let json_content = std::fs::read_to_string(json_path)
        .with_context(|| format!("failed to read {}", json_path.display()))?;
    let parsed_report: EvalReport = serde_json::from_str(&json_content)
        .with_context(|| format!("failed to parse {}", json_path.display()))?;
    if parsed_report.json_contract_version != JSON_CONTRACT_VERSION {
        failures.push(format!(
            "pointwise JSON artifact {} has unexpected contract {}",
            json_path.display(),
            parsed_report.json_contract_version
        ));
    }
    if parsed_report.markdown_contract_version != report.markdown_contract_version {
        failures.push(format!(
            "pointwise JSON artifact {} has unexpected markdown contract {}",
            json_path.display(),
            parsed_report.markdown_contract_version
        ));
    }
    if parsed_report.manifest_digest != report.manifest_digest {
        failures.push(format!(
            "pointwise JSON artifact {} has manifest digest {} but expected {}",
            json_path.display(),
            parsed_report.manifest_digest,
            report.manifest_digest
        ));
    }
    if parsed_report.overall_pass != report.overall_pass
        || parsed_report.suite_count != report.suite_count
        || parsed_report.total_case_count != report.total_case_count
        || parsed_report.failed_case_count != report.failed_case_count
        || parsed_report.blocker_failure_count != report.blocker_failure_count
    {
        failures.push(format!(
            "pointwise JSON artifact {} does not match the in-memory summary counts",
            json_path.display()
        ));
    }

    let markdown = std::fs::read_to_string(markdown_path)
        .with_context(|| format!("failed to read {}", markdown_path.display()))?;
    if !markdown.contains("# AI Eval Report") {
        failures.push(format!(
            "pointwise markdown artifact {} is missing the report header",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.json_contract_version) {
        failures.push(format!(
            "pointwise markdown artifact {} is missing the JSON contract version",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.markdown_contract_version) {
        failures.push(format!(
            "pointwise markdown artifact {} is missing the markdown contract version",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.manifest_digest) {
        failures.push(format!(
            "pointwise markdown artifact {} is missing the manifest digest",
            markdown_path.display()
        ));
    }

    Ok(failures)
}

fn validate_comparison_artifacts(
    report: &EvalComparisonReport,
    json_path: &Path,
    markdown_path: &Path,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    let json_content = std::fs::read_to_string(json_path)
        .with_context(|| format!("failed to read {}", json_path.display()))?;
    let parsed_report: EvalComparisonReport = serde_json::from_str(&json_content)
        .with_context(|| format!("failed to parse {}", json_path.display()))?;
    if parsed_report.json_contract_version != COMPARISON_JSON_CONTRACT_VERSION {
        failures.push(format!(
            "comparison JSON artifact {} has unexpected contract {}",
            json_path.display(),
            parsed_report.json_contract_version
        ));
    }
    if parsed_report.markdown_contract_version != report.markdown_contract_version {
        failures.push(format!(
            "comparison JSON artifact {} has unexpected markdown contract {}",
            json_path.display(),
            parsed_report.markdown_contract_version
        ));
    }
    if parsed_report.manifest_digest != report.manifest_digest {
        failures.push(format!(
            "comparison JSON artifact {} has manifest digest {} but expected {}",
            json_path.display(),
            parsed_report.manifest_digest,
            report.manifest_digest
        ));
    }
    if parsed_report.overall_pass != report.overall_pass
        || parsed_report.case_count != report.case_count
        || parsed_report.failed_case_count != report.failed_case_count
        || parsed_report.blocker_failure_count != report.blocker_failure_count
    {
        failures.push(format!(
            "comparison JSON artifact {} does not match the in-memory summary counts",
            json_path.display()
        ));
    }

    let markdown = std::fs::read_to_string(markdown_path)
        .with_context(|| format!("failed to read {}", markdown_path.display()))?;
    if !markdown.contains("# AI Pairwise Comparison Report") {
        failures.push(format!(
            "comparison markdown artifact {} is missing the report header",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.json_contract_version) {
        failures.push(format!(
            "comparison markdown artifact {} is missing the JSON contract version",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.markdown_contract_version) {
        failures.push(format!(
            "comparison markdown artifact {} is missing the markdown contract version",
            markdown_path.display()
        ));
    }
    if !markdown.contains(&report.manifest_digest) {
        failures.push(format!(
            "comparison markdown artifact {} is missing the manifest digest",
            markdown_path.display()
        ));
    }

    Ok(failures)
}

fn validate_gate_summary_artifacts(
    summary: &EvalGateSummary,
    json_path: &Path,
    markdown_path: &Path,
) -> Result<()> {
    let json_content = std::fs::read_to_string(json_path)
        .with_context(|| format!("failed to read {}", json_path.display()))?;
    let parsed_summary: EvalGateSummary = serde_json::from_str(&json_content)
        .with_context(|| format!("failed to parse {}", json_path.display()))?;
    if parsed_summary != *summary {
        bail!(
            "gate summary JSON artifact {} did not round-trip cleanly",
            json_path.display()
        );
    }
    let markdown = std::fs::read_to_string(markdown_path)
        .with_context(|| format!("failed to read {}", markdown_path.display()))?;
    if !markdown.contains("# AI Judge Gate Summary")
        || !markdown.contains(GATE_SUMMARY_JSON_CONTRACT_VERSION)
        || !markdown.contains(GATE_SUMMARY_MARKDOWN_CONTRACT_VERSION)
    {
        bail!(
            "gate summary markdown artifact {} is missing required contract markers",
            markdown_path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{EvalGateConfig, EvalGateMode, pin_release_config, run_judge_gate};
    use crate::ai_eval_harness::corpus::fixtures_dir;
    use crate::ai_eval_harness::judge::{
        BACKEND_KIND, DATASET_VERSION, DEFAULT_SEED, EvalRunConfig, JUDGE_VERSION, MODEL_ID,
        RUBRIC_VERSION, build_run_manifest,
    };

    #[tokio::test]
    async fn smoke_gate_writes_expected_artifacts() {
        let dir = tempdir().unwrap();
        let fixtures = fixtures_dir();
        let summary = run_judge_gate(
            &fixtures,
            EvalGateConfig {
                mode: EvalGateMode::Smoke,
                artifacts_dir: dir.path().to_path_buf(),
                run_config: EvalRunConfig {
                    generated_at: Some("2026-04-11T12:00:00Z".to_string()),
                    run_id: Some("judge-smoke-test".to_string()),
                    git_sha: Some("git-test".to_string()),
                    base_sha: Some("base-test".to_string()),
                    dataset_version: Some("dataset-test".to_string()),
                    judge_version: Some("judge-test".to_string()),
                    rubric_version: Some("rubric-test".to_string()),
                    model_id: Some("model-test".to_string()),
                    backend_kind: Some("backend-test".to_string()),
                    seed: Some(7),
                    timezone: Some("UTC".to_string()),
                    locale: Some("en-IE".to_string()),
                },
            },
        )
        .await
        .unwrap();

        assert!(summary.gate_pass);
        assert!(summary.manifest_pinned);
        assert!(std::fs::metadata(&summary.artifacts.pointwise_json).is_ok());
        assert!(std::fs::metadata(&summary.artifacts.pointwise_markdown).is_ok());
        assert!(std::fs::metadata(&summary.artifacts.summary_json).is_ok());
        assert!(std::fs::metadata(&summary.artifacts.summary_markdown).is_ok());
        assert_eq!(summary.mode, EvalGateMode::Smoke);
        assert_eq!(summary.deterministic_replay_match, None);
    }

    #[tokio::test]
    async fn release_gate_pins_manifest_and_checks_replay() {
        let dir = tempdir().unwrap();
        let fixtures = fixtures_dir();
        let summary = run_judge_gate(
            &fixtures,
            EvalGateConfig {
                mode: EvalGateMode::Release,
                artifacts_dir: dir.path().to_path_buf(),
                run_config: EvalRunConfig {
                    generated_at: Some("2026-04-11T12:00:00Z".to_string()),
                    run_id: Some("judge-release-test".to_string()),
                    ..EvalRunConfig::default()
                },
            },
        )
        .await
        .unwrap();

        assert!(summary.gate_pass);
        assert!(summary.manifest_pinned);
        assert_eq!(summary.deterministic_replay_match, Some(true));
        assert!(summary.artifacts.replay_json.is_some());
        assert!(summary.artifacts.comparison_json.is_some());
        assert!(std::fs::metadata(summary.artifacts.replay_json.as_ref().unwrap()).is_ok());
        assert!(std::fs::metadata(summary.artifacts.comparison_markdown.as_ref().unwrap()).is_ok());
    }

    #[test]
    fn release_pinning_fills_missing_fields() {
        let fixtures = fixtures_dir();
        let baseline_manifest = build_run_manifest(
            "default",
            &fixtures,
            &EvalRunConfig {
                generated_at: Some("2026-04-11T12:00:00Z".to_string()),
                run_id: Some("judge-release-test".to_string()),
                ..EvalRunConfig::default()
            },
        )
        .unwrap();
        let pinned = pin_release_config(
            &fixtures,
            EvalRunConfig {
                generated_at: Some("2026-04-11T12:00:00Z".to_string()),
                run_id: Some("judge-release-test".to_string()),
                ..EvalRunConfig::default()
            },
        )
        .unwrap();

        assert!(pinned.git_sha.is_some());
        assert!(pinned.base_sha.is_some());
        assert_eq!(pinned.dataset_version.as_deref(), Some(DATASET_VERSION));
        assert_eq!(pinned.judge_version.as_deref(), Some(JUDGE_VERSION));
        assert_eq!(pinned.rubric_version.as_deref(), Some(RUBRIC_VERSION));
        assert_eq!(pinned.model_id.as_deref(), Some(MODEL_ID));
        assert_eq!(pinned.backend_kind.as_deref(), Some(BACKEND_KIND));
        assert_eq!(pinned.seed, Some(DEFAULT_SEED));
        assert_eq!(
            pinned.timezone.as_deref(),
            Some(baseline_manifest.timezone.as_str())
        );
        assert_eq!(
            pinned.locale.as_deref(),
            Some(baseline_manifest.locale.as_str())
        );
    }
}
