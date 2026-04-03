use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use futures::stream::{self, BoxStream};
use rustfin_ai_agent::{
    BackendCapabilities, BackendKind, ChatChunk, ChatMessage, PromptBackend, PromptCacheHint,
    SamplingParams,
};
use serde::{Deserialize, Serialize};

use crate::ai_assistant::types::AssistantHistoryMessage;
use crate::ai_assistant::{AssistantPlannerMode, plan_tool_calls_with_model_assist};
use crate::auth::AuthUser;

use super::corpus::load_jsonl;
use super::report::{EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct PlannerCase {
    name: String,
    user_role: String,
    message: String,
    #[serde(default)]
    history: Vec<AssistantHistoryMessage>,
    model_responses: Vec<String>,
    expected_tools: Vec<String>,
    expected_mode: String,
    #[serde(default)]
    forbidden_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PlannerCaseResult {
    name: String,
    actual_tools: Vec<String>,
    actual_mode: String,
    exact_tool_match: bool,
    mode_match: bool,
    forbidden_violations: Vec<String>,
    repair_succeeded: bool,
    used_fallback: bool,
}

struct MockPromptBackend {
    responses: Mutex<VecDeque<String>>,
}

impl MockPromptBackend {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

impl PromptBackend for MockPromptBackend {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Local
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::Local,
            supports_streaming: true,
            supports_prompt_cache: false,
            supports_structured_output: true,
            can_degrade: true,
            max_parallel_requests: 1,
        }
    }

    fn chat_stream_boxed(
        &self,
        _messages: Vec<ChatMessage>,
        _sampling: SamplingParams,
        _prompt_cache: Option<PromptCacheHint>,
    ) -> BoxStream<'static, Result<ChatChunk, rustfin_ai_agent::AiError>> {
        let response = self
            .responses
            .lock()
            .expect("mock prompt backend lock")
            .pop_front()
            .unwrap_or_else(|| "{\"mode\":\"tool_plan\",\"tools\":[]}".to_string());
        Box::pin(stream::iter(vec![
            Ok(ChatChunk::Token(response)),
            Ok(ChatChunk::Done),
        ]))
    }
}

pub async fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    let cases = load_jsonl::<PlannerCase>(&fixtures_dir.join("planner_cases.jsonl"))?;
    let mut exact_matches = 0usize;
    let mut mode_matches = 0usize;
    let mut forbidden_tool_violations = 0usize;
    let mut repair_cases = 0usize;
    let mut repair_successes = 0usize;
    let mut fallback_cases = 0usize;
    let mut details = Vec::<PlannerCaseResult>::new();

    for case in &cases {
        let backend = MockPromptBackend::new(case.model_responses.clone());
        let user = AuthUser {
            user_id: "eval-user".to_string(),
            username: "eval".to_string(),
            role: case.user_role.clone(),
        };
        let planned =
            plan_tool_calls_with_model_assist(&backend, &user, &case.message, &case.history).await;

        let actual_tools = planned
            .calls
            .iter()
            .map(|call| call.tool.as_str().to_string())
            .collect::<Vec<_>>();
        let expected_tools = case.expected_tools.iter().cloned().collect::<HashSet<_>>();
        let actual_tools_set = actual_tools.iter().cloned().collect::<HashSet<_>>();
        let exact_tool_match = actual_tools_set == expected_tools;
        let actual_mode = planned.mode.as_str().to_string();
        let mode_match = actual_mode == case.expected_mode;
        let violations = actual_tools
            .iter()
            .filter(|tool| {
                case.forbidden_tools
                    .iter()
                    .any(|forbidden| forbidden == *tool)
            })
            .cloned()
            .collect::<Vec<_>>();
        if planned.debug.repair_attempt_count > 0 {
            repair_cases += 1;
            if planned.debug.used_repaired_response {
                repair_successes += 1;
            }
        }
        if planned.mode != AssistantPlannerMode::ModelStructured {
            fallback_cases += 1;
        }
        if exact_tool_match {
            exact_matches += 1;
        }
        if mode_match {
            mode_matches += 1;
        }
        forbidden_tool_violations += violations.len();
        details.push(PlannerCaseResult {
            name: case.name.clone(),
            actual_tools,
            actual_mode,
            exact_tool_match,
            mode_match,
            forbidden_violations: violations,
            repair_succeeded: planned.debug.used_repaired_response,
            used_fallback: planned.mode != AssistantPlannerMode::ModelStructured,
        });
    }

    let exact_accuracy = exact_matches as f64 / cases.len().max(1) as f64;
    let mode_accuracy = mode_matches as f64 / cases.len().max(1) as f64;
    let repair_success_rate = if repair_cases == 0 {
        1.0
    } else {
        repair_successes as f64 / repair_cases as f64
    };
    let fallback_rate = fallback_cases as f64 / cases.len().max(1) as f64;
    let forbidden_metric = forbidden_tool_violations as f64;

    let mut metrics = BTreeMap::new();
    metrics.insert("planner_exact_tool_accuracy".to_string(), exact_accuracy);
    metrics.insert("planner_mode_accuracy".to_string(), mode_accuracy);
    metrics.insert(
        "planner_forbidden_tool_violations".to_string(),
        forbidden_metric,
    );
    metrics.insert(
        "planner_repair_success_rate".to_string(),
        repair_success_rate,
    );
    metrics.insert(
        "planner_deterministic_fallback_rate".to_string(),
        fallback_rate,
    );

    let thresholds = vec![
        EvalThreshold {
            metric: "planner_exact_tool_accuracy".to_string(),
            actual: exact_accuracy,
            expected: 0.90,
            pass: exact_accuracy >= 0.90,
        },
        EvalThreshold {
            metric: "planner_forbidden_tool_violations".to_string(),
            actual: forbidden_metric,
            expected: 0.0,
            pass: forbidden_metric == 0.0,
        },
    ];

    Ok(EvalSuiteReport {
        name: "planner".to_string(),
        pass: thresholds.iter().all(|threshold| threshold.pass),
        metrics,
        thresholds,
        case_count: cases.len(),
        details: serde_json::to_value(details)?,
    })
}
