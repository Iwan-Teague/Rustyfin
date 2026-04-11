use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::types::{AssistantGroundingVisibility, ToolRoleRequirement};

use super::corpus;
use super::judge_metrics::EvalRubricVerdict;
use super::report::{
    EvalCaseVerdict, EvalFailureBucket, EvalGateSeverity, EvalHardGateResult, EvalRunManifest,
};

pub const DATASET_VERSION: &str = "rustyfin_ai_suite_corpora_v2";
pub const JUDGE_VERSION: &str = "rustyfin_ai_judge_phase2_v1";
pub const RUBRIC_VERSION: &str = super::judge_rubric::RUBRIC_PROMPT_VERSION;
pub const MODEL_ID: &str = "deterministic_fixture_harness";
pub const BACKEND_KIND: &str = "deterministic_fixture";
pub const DEFAULT_SEED: u64 = 0;
pub const DEFAULT_TIMEZONE: &str = "UTC";
pub const DEFAULT_LOCALE: &str = "en-IE";
pub const MAX_CASE_NAME_CHARS: usize = 128;
pub const MAX_PROMPT_CHARS: usize = 512;
pub const MAX_MODEL_OUTPUT_CHARS: usize = 4 * 1024;
pub const MAX_LABEL_CHARS: usize = 256;
pub const MAX_REQUIRED_MATCHES: usize = 16;
pub const MAX_GROUNDING_CHUNKS_PER_CASE: usize = 32;
pub const MAX_CHUNK_TEXT_CHARS: usize = 2 * 1024;
pub const MAX_ARTIFACT_TEXT_CHARS: usize = 64 * 1024;

#[derive(Debug, Clone, Default)]
pub struct EvalRunConfig {
    pub generated_at: Option<String>,
    pub run_id: Option<String>,
    pub git_sha: Option<String>,
    pub base_sha: Option<String>,
    pub dataset_version: Option<String>,
    pub judge_version: Option<String>,
    pub rubric_version: Option<String>,
    pub model_id: Option<String>,
    pub backend_kind: Option<String>,
    pub seed: Option<u64>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
}

pub fn default_generated_at() -> String {
    Utc::now().to_rfc3339()
}

pub fn build_run_manifest(
    mode: &str,
    fixtures_dir: &Path,
    config: &EvalRunConfig,
) -> Result<EvalRunManifest> {
    let specs = corpus::fixture_specs_for_mode(mode)?;
    Ok(EvalRunManifest {
        run_id: config
            .run_id
            .clone()
            .unwrap_or_else(|| format!("eval-{}", Utc::now().format("%Y%m%dT%H%M%SZ"))),
        git_sha: config.git_sha.clone().unwrap_or_else(detect_git_sha),
        base_sha: config.base_sha.clone().unwrap_or_else(detect_base_sha),
        dataset_version: config
            .dataset_version
            .clone()
            .unwrap_or_else(|| DATASET_VERSION.to_string()),
        judge_version: config
            .judge_version
            .clone()
            .unwrap_or_else(|| JUDGE_VERSION.to_string()),
        rubric_version: config
            .rubric_version
            .clone()
            .unwrap_or_else(|| RUBRIC_VERSION.to_string()),
        fixture_digest: corpus::fixture_digest(fixtures_dir, &specs)?,
        schema_digest: corpus::schema_digest(fixtures_dir, &specs)?,
        tool_registry_digest: tool_registry_digest(),
        model_id: config
            .model_id
            .clone()
            .unwrap_or_else(|| MODEL_ID.to_string()),
        backend_kind: config
            .backend_kind
            .clone()
            .unwrap_or_else(|| BACKEND_KIND.to_string()),
        seed: config.seed.unwrap_or(DEFAULT_SEED),
        timezone: config
            .timezone
            .clone()
            .unwrap_or_else(|| default_timezone()),
        locale: config.locale.clone().unwrap_or_else(|| default_locale()),
    })
}

pub fn pass_gate(gate: &str) -> EvalHardGateResult {
    EvalHardGateResult {
        gate: gate.to_string(),
        severity: EvalGateSeverity::Blocker,
        applicable: true,
        pass: true,
        message: None,
        failure_bucket: None,
    }
}

pub fn pass_gate_with_message(gate: &str, message: impl Into<String>) -> EvalHardGateResult {
    EvalHardGateResult {
        gate: gate.to_string(),
        severity: EvalGateSeverity::Blocker,
        applicable: true,
        pass: true,
        message: Some(message.into()),
        failure_bucket: None,
    }
}

pub fn inapplicable_gate(gate: &str, message: impl Into<String>) -> EvalHardGateResult {
    EvalHardGateResult {
        gate: gate.to_string(),
        severity: EvalGateSeverity::Blocker,
        applicable: false,
        pass: true,
        message: Some(message.into()),
        failure_bucket: None,
    }
}

pub fn fail_gate(
    gate: &str,
    failure_bucket: EvalFailureBucket,
    message: impl Into<String>,
) -> EvalHardGateResult {
    EvalHardGateResult {
        gate: gate.to_string(),
        severity: EvalGateSeverity::Blocker,
        applicable: true,
        pass: false,
        message: Some(message.into()),
        failure_bucket: Some(failure_bucket),
    }
}

pub fn finalize_case_verdict(
    case_id: &str,
    metrics: BTreeMap<String, f64>,
    hard_gates: Vec<EvalHardGateResult>,
    details: serde_json::Value,
) -> EvalCaseVerdict {
    finalize_case_verdict_with_rubric(case_id, metrics, hard_gates, None, details)
}

pub fn finalize_case_verdict_with_rubric(
    case_id: &str,
    metrics: BTreeMap<String, f64>,
    hard_gates: Vec<EvalHardGateResult>,
    rubric: Option<EvalRubricVerdict>,
    details: serde_json::Value,
) -> EvalCaseVerdict {
    let failure_buckets = hard_gates
        .iter()
        .filter(|gate| gate.applicable && !gate.pass)
        .filter_map(|gate| gate.failure_bucket.clone())
        .collect::<Vec<_>>();
    let blocker_failure_count = failure_buckets.len();

    EvalCaseVerdict {
        case_id: case_id.to_string(),
        pass: blocker_failure_count == 0,
        blocker_failure_count,
        hard_gates,
        failure_buckets,
        metrics,
        rubric,
        details,
    }
}

pub fn length_gate(field: &str, actual_len: usize, max_len: usize) -> EvalHardGateResult {
    if actual_len <= max_len {
        pass_gate_with_message(field, format!("{} <= {}", actual_len, max_len))
    } else {
        fail_gate(
            field,
            EvalFailureBucket::LengthLimitExceeded,
            format!("length {} exceeds limit {}", actual_len, max_len),
        )
    }
}

pub fn visibility_allowed(role: &str, visibility: AssistantGroundingVisibility) -> bool {
    match visibility {
        AssistantGroundingVisibility::Admin => is_admin_role(role),
        AssistantGroundingVisibility::User | AssistantGroundingVisibility::Shared => true,
    }
}

pub fn tool_allowed_for_role(tool_name: &str, role: &str) -> bool {
    let Some(tool) = AssistantToolName::from_str(tool_name) else {
        return false;
    };
    match tool.spec().required_role {
        ToolRoleRequirement::AnyAuthenticatedUser => true,
        ToolRoleRequirement::AdminOnly => is_admin_role(role),
    }
}

pub fn is_admin_role(role: &str) -> bool {
    role.eq_ignore_ascii_case("admin")
}

pub fn parse_model_json_object(output: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(output).map_err(|error| format!("invalid JSON: {error}"))?;
    if !value.is_object() {
        return Err("model output must be a JSON object".to_string());
    }
    Ok(value)
}

pub fn tool_registry_digest() -> String {
    let entries = AssistantToolName::all()
        .iter()
        .map(|tool| {
            let spec = tool.spec();
            json!({
                "name": tool.as_str(),
                "summary": spec.summary,
                "access_mode": spec.access_mode,
                "risk_tier": spec.risk_tier,
                "required_role": spec.required_role,
                "confirmation": spec.confirmation,
                "timeout_ms": spec.timeout_ms,
                "max_result_bytes": spec.max_result_bytes,
            })
        })
        .collect::<Vec<_>>();
    hash_json_value(&serde_json::Value::Array(entries))
}

fn hash_json_value(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn default_timezone() -> String {
    std::env::var("RUSTFIN_AI_EVAL_TIMEZONE")
        .ok()
        .or_else(|| std::env::var("TZ").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TIMEZONE.to_string())
}

fn default_locale() -> String {
    std::env::var("RUSTFIN_AI_EVAL_LOCALE")
        .ok()
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LANG").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string())
}

fn detect_git_sha() -> String {
    git_rev_parse(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string())
}

fn detect_base_sha() -> String {
    git_rev_parse(&["merge-base", "HEAD", "main"])
        .or_else(|| git_rev_parse(&["merge-base", "HEAD", "origin/main"]))
        .unwrap_or_else(detect_git_sha)
}

fn git_rev_parse(args: &[&str]) -> Option<String> {
    let repo_root = repo_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(test)]
mod tests {
    use crate::ai_assistant::types::AssistantGroundingVisibility;

    use super::{
        EvalFailureBucket, MAX_CASE_NAME_CHARS, build_run_manifest, fail_gate, length_gate,
        parse_model_json_object, tool_allowed_for_role, visibility_allowed,
    };

    #[test]
    fn length_gate_fails_when_limit_is_exceeded() {
        let gate = length_gate(
            "message_length",
            MAX_CASE_NAME_CHARS + 1,
            MAX_CASE_NAME_CHARS,
        );
        assert!(!gate.pass);
        assert_eq!(
            gate.failure_bucket,
            Some(EvalFailureBucket::LengthLimitExceeded)
        );
    }

    #[test]
    fn model_json_parser_rejects_non_object_payloads() {
        let error = parse_model_json_object("[]").expect_err("array should fail");
        assert!(error.contains("JSON object"));
    }

    #[test]
    fn visibility_checks_enforce_admin_only_chunks() {
        assert!(visibility_allowed(
            "user",
            AssistantGroundingVisibility::Shared
        ));
        assert!(!visibility_allowed(
            "user",
            AssistantGroundingVisibility::Admin
        ));
        assert!(visibility_allowed(
            "admin",
            AssistantGroundingVisibility::Admin
        ));
    }

    #[test]
    fn tool_role_checks_use_registry_specs() {
        assert!(tool_allowed_for_role("weather_get_forecast", "user"));
        assert!(!tool_allowed_for_role(
            "system_get_host_runtime_summary",
            "user"
        ));
        assert!(tool_allowed_for_role(
            "system_get_host_runtime_summary",
            "admin"
        ));
    }

    #[test]
    fn manifest_builder_is_stable_for_fixed_overrides() {
        let fixtures_dir = crate::ai_eval_harness::corpus::fixtures_dir();
        let config = super::EvalRunConfig {
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

        let left = build_run_manifest("planner", &fixtures_dir, &config).unwrap();
        let right = build_run_manifest("planner", &fixtures_dir, &config).unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn fail_gate_marks_bucket() {
        let gate = fail_gate(
            "exact_answer_contract",
            EvalFailureBucket::ExactAnswerMismatch,
            "mismatch",
        );
        assert!(!gate.pass);
        assert_eq!(
            gate.failure_bucket,
            Some(EvalFailureBucket::ExactAnswerMismatch)
        );
    }
}
