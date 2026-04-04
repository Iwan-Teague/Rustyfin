use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_assistant::executor::{AssistantGroundedExecutor, ExecutorPostStep};
use crate::ai_assistant::outcomes::normalize_tool_result;
use crate::ai_assistant::provider::ToolExecutionProfile;
use crate::ai_assistant::registry::AssistantToolName;
use crate::ai_assistant::types::{
    AssistantGroundingSource, AssistantPlannerMode, AssistantResponseMode,
    AssistantToolContextBlock, PlannedToolCall,
};

use super::corpus::load_jsonl;
use super::report::{EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct ExecutionCase {
    name: String,
    message: String,
    response_mode: AssistantResponseMode,
    initial_calls: Vec<PlannedToolCall>,
    steps: Vec<ExecutionCaseStep>,
    expected_stop_reason: String,
    expected_final_outcome_kind: String,
    expected_attempt_count: u32,
    expected_attempted_tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExecutionCaseStep {
    tool: String,
    label: String,
    status: String,
    data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct ExecutionCaseResult {
    name: String,
    stop_reason_match: bool,
    final_outcome_match: bool,
    attempted_tools_match: bool,
    attempt_count_match: bool,
}

pub fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let cases = load_jsonl::<ExecutionCase>(&fixtures_dir.join("execution_cases.jsonl"))?;
    let mut passing = 0usize;
    let mut details = Vec::new();

    for case in &cases {
        let mut executor = AssistantGroundedExecutor::new(
            &case.message,
            case.response_mode,
            Some(AssistantPlannerMode::DeterministicFallback),
            &case.initial_calls,
            Vec::new(),
            ToolExecutionProfile::full_access(),
        );

        for fixture in &case.steps {
            let Some(step) = executor.next_step() else {
                break;
            };
            let actual_tool = step.call.tool.as_str().to_string();
            if actual_tool != fixture.tool {
                break;
            }
            let tool_name = AssistantToolName::from_str(&fixture.tool).expect("tool fixture");
            let block = AssistantToolContextBlock {
                tool: tool_name.as_str(),
                label: fixture.label.clone(),
                status: match fixture.status.as_str() {
                    "ok" => "ok",
                    _ => "error",
                },
                data: fixture.data.clone(),
            };
            let outcome = normalize_tool_result(&case.message, &step.call, block.clone());
            let post_step = executor.record_step(
                step,
                outcome,
                AssistantGroundingSource {
                    tool: fixture.tool.clone(),
                    label: fixture.label.clone(),
                    access_mode: tool_name.spec().access_mode,
                    risk_tier: tool_name.spec().risk_tier,
                    status: fixture.status.clone(),
                    download_url: None,
                    download_file_name: None,
                    download_media_type: None,
                    download_size_bytes: None,
                },
                10,
            );
            if !matches!(post_step, ExecutorPostStep::Continue) {
                break;
            }
        }

        executor.finalize_bounded_failure();
        let trace = executor.trace();
        let attempted_tools = trace
            .attempts
            .iter()
            .map(|attempt| attempt.tool.clone())
            .collect::<Vec<_>>();
        let stop_reason_match = trace.stop_reason.as_str() == case.expected_stop_reason;
        let final_outcome_match = trace
            .final_outcome_kind
            .map(|kind| kind.as_str().to_string())
            .as_deref()
            == Some(case.expected_final_outcome_kind.as_str());
        let attempted_tools_match = attempted_tools == case.expected_attempted_tools;
        let attempt_count_match = trace.attempts.len() as u32 == case.expected_attempt_count;
        let passed = stop_reason_match
            && final_outcome_match
            && attempted_tools_match
            && attempt_count_match;
        if passed {
            passing += 1;
        }
        details.push(ExecutionCaseResult {
            name: case.name.clone(),
            stop_reason_match,
            final_outcome_match,
            attempted_tools_match,
            attempt_count_match,
        });
    }

    let pass_rate = passing as f64 / cases.len().max(1) as f64;
    let mut metrics = BTreeMap::new();
    metrics.insert("execution_trace_contract_pass_rate".to_string(), pass_rate);

    let thresholds = vec![EvalThreshold {
        metric: "execution_trace_contract_pass_rate".to_string(),
        actual: pass_rate,
        expected: 0.90,
        pass: pass_rate >= 0.90,
    }];

    Ok(EvalSuiteReport {
        name: "execution".to_string(),
        pass: thresholds.iter().all(|threshold| threshold.pass),
        metrics,
        thresholds,
        case_count: cases.len(),
        details: serde_json::to_value(details)?,
    })
}
