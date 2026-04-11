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

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    MAX_CASE_NAME_CHARS, MAX_MODEL_OUTPUT_CHARS, MAX_PROMPT_CHARS, fail_gate,
    finalize_case_verdict, inapplicable_gate, length_gate, parse_model_json_object, pass_gate,
    tool_allowed_for_role,
};
use super::report::{EvalFailureBucket, EvalSuiteReport, EvalThreshold};

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
    let spec = FixtureCorpusSpec {
        suite_name: "planner",
        fixture_file: "planner_cases.jsonl",
        schema_file: "planner_cases.schema.json",
    };
    let cases = load_jsonl::<PlannerCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?;
    let mut exact_matches = 0usize;
    let mut mode_matches = 0usize;
    let mut forbidden_tool_violations = 0usize;
    let mut repair_cases = 0usize;
    let mut repair_successes = 0usize;
    let mut fallback_cases = 0usize;
    let mut details = Vec::<PlannerCaseResult>::new();
    let mut case_verdicts = Vec::new();

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
        let case_result = PlannerCaseResult {
            name: case.name.clone(),
            actual_tools,
            actual_mode,
            exact_tool_match,
            mode_match,
            forbidden_violations: violations,
            repair_succeeded: planned.debug.used_repaired_response,
            used_fallback: planned.mode != AssistantPlannerMode::ModelStructured,
        };

        let malformed_output_gate = case
            .model_responses
            .iter()
            .find_map(|response| parse_model_json_object(response).err())
            .map(|error| {
                fail_gate(
                    "malformed_output",
                    EvalFailureBucket::MalformedOutput,
                    error,
                )
            })
            .unwrap_or_else(|| pass_gate("malformed_output"));
        let length_gate = {
            let mut violations = Vec::new();
            for gate in [
                length_gate(
                    "case_name_length",
                    case.name.chars().count(),
                    MAX_CASE_NAME_CHARS,
                ),
                length_gate(
                    "message_length",
                    case.message.chars().count(),
                    MAX_PROMPT_CHARS,
                ),
            ] {
                if !gate.pass {
                    violations.push(gate.message.clone().unwrap_or_default());
                }
            }
            for response in &case.model_responses {
                let gate = length_gate(
                    "model_output_length",
                    response.chars().count(),
                    MAX_MODEL_OUTPUT_CHARS,
                );
                if !gate.pass {
                    violations.push(gate.message.clone().unwrap_or_default());
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
        let acl_gate = {
            let disallowed_tools = case_result
                .actual_tools
                .iter()
                .filter(|tool| !tool_allowed_for_role(tool, &case.user_role))
                .cloned()
                .collect::<Vec<_>>();
            if disallowed_tools.is_empty() {
                pass_gate("acl_privacy_boundary")
            } else {
                fail_gate(
                    "acl_privacy_boundary",
                    EvalFailureBucket::PrivacyBoundaryViolation,
                    format!(
                        "planned tools exceed role {}: {}",
                        case.user_role,
                        disallowed_tools.join(", ")
                    ),
                )
            }
        };
        let exact_answer_gate = if case_result.exact_tool_match
            && case_result.mode_match
            && case_result.forbidden_violations.is_empty()
        {
            pass_gate("exact_answer_contract")
        } else {
            fail_gate(
                "exact_answer_contract",
                EvalFailureBucket::ExactAnswerMismatch,
                format!(
                    "tool_match={}, mode_match={}, forbidden_violations={}",
                    case_result.exact_tool_match,
                    case_result.mode_match,
                    case_result.forbidden_violations.len()
                ),
            )
        };

        let mut case_metrics = BTreeMap::new();
        case_metrics.insert(
            "exact_tool_match".to_string(),
            if case_result.exact_tool_match {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "mode_match".to_string(),
            if case_result.mode_match { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "repair_succeeded".to_string(),
            if case_result.repair_succeeded {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "used_fallback".to_string(),
            if case_result.used_fallback { 1.0 } else { 0.0 },
        );

        case_verdicts.push(finalize_case_verdict(
            &case.name,
            case_metrics,
            vec![
                pass_gate("schema_validity"),
                malformed_output_gate,
                length_gate,
                inapplicable_gate(
                    "refusal_correctness",
                    "planner fixtures do not encode refusal verdicts in phase 1",
                ),
                acl_gate,
                exact_answer_gate,
            ],
            serde_json::to_value(&case_result)?,
        ));
        details.push(case_result);
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
            blocking: true,
        },
        EvalThreshold {
            metric: "planner_forbidden_tool_violations".to_string(),
            actual: forbidden_metric,
            expected: 0.0,
            pass: forbidden_metric == 0.0,
            blocking: true,
        },
    ];

    Ok(EvalSuiteReport::finalize(
        "planner",
        metrics,
        thresholds,
        case_verdicts,
        serde_json::to_value(details)?,
    ))
}
