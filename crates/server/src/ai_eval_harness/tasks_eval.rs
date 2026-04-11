use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ai_tasks::executor::run_task_with_store;
use crate::ai_tasks::store::{AiTaskStore, MemoryAiTaskStore};
use crate::ai_tasks::types::{
    AiTaskInput, AiTaskPhase, AiTaskStatus, CreateAiTaskRequest, TaskUserContext,
};
use crate::state::{AppState, RustyVaultRuntimeState};

use super::corpus::{FixtureCorpusSpec, fixture_path, load_jsonl, schema_path};
use super::judge::{
    MAX_ARTIFACT_TEXT_CHARS, MAX_CASE_NAME_CHARS, MAX_REQUIRED_MATCHES, finalize_case_verdict,
    inapplicable_gate, length_gate, pass_gate,
};
use super::report::{EvalFailureBucket, EvalSuiteReport, EvalThreshold};

#[derive(Debug, Clone, Deserialize)]
struct TaskCase {
    name: String,
    #[serde(flatten)]
    request: CreateAiTaskRequest,
    #[serde(default)]
    expected_worker_profiles: Vec<String>,
    required_artifact_substrings: Vec<String>,
    allowed_runtime_budget_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct TaskCaseResult {
    name: String,
    completed: bool,
    artifact_generated: bool,
    artifact_content_match: bool,
    artifact_readable: bool,
    runtime_budget_pass: bool,
    worker_split_match: bool,
    verification_seen: bool,
}

pub async fn run(fixtures_dir: &Path) -> Result<EvalSuiteReport> {
    run_with_options(fixtures_dir, true).await
}

pub async fn run_with_options(
    fixtures_dir: &Path,
    include_eval_run_case: bool,
) -> Result<EvalSuiteReport> {
    let spec = FixtureCorpusSpec {
        suite_name: "tasks",
        fixture_file: "task_cases.jsonl",
        schema_file: "task_cases.schema.json",
    };
    let cases = load_jsonl::<TaskCase>(
        &fixture_path(fixtures_dir, &spec),
        &schema_path(fixtures_dir, &spec),
    )?
    .into_iter()
    .filter(|case| {
        include_eval_run_case || !matches!(case.request.input, AiTaskInput::AiEvalRun { .. })
    })
    .collect::<Vec<_>>();
    let mut completion_hits = 0usize;
    let mut artifact_hits = 0usize;
    let mut verification_hits = 0usize;
    let mut details = Vec::new();
    let mut case_verdicts = Vec::new();

    for case in &cases {
        let state = test_state();
        let store = MemoryAiTaskStore::default();
        let task = store
            .create_task(&user(), &case.request)
            .await
            .map_err(anyhow::Error::msg)?;
        let started = Instant::now();
        Box::pin(run_task_with_store(state, store.clone(), &task.id))
            .await
            .map_err(anyhow::Error::msg)?;
        let runtime_ms = started.elapsed().as_millis();
        let runtime_budget_pass = runtime_ms <= case.allowed_runtime_budget_ms as u128;
        let completed = store
            .get_task(&task.id)
            .await
            .map_err(anyhow::Error::msg)?
            .map(|task| task.status == AiTaskStatus::Completed)
            .unwrap_or(false);
        let checkpoints = store
            .list_checkpoints(&task.id)
            .await
            .map_err(anyhow::Error::msg)?;
        let verification_seen = checkpoints
            .iter()
            .any(|checkpoint| checkpoint.phase == AiTaskPhase::Verifying);
        let artifact = store
            .get_task(&task.id)
            .await
            .map_err(anyhow::Error::msg)?
            .and_then(|task| task.artifacts.first().cloned());
        let artifact_generated = artifact.is_some();
        let (artifact_text, artifact_readable) = if let Some(artifact) = artifact.as_ref() {
            match std::fs::read_to_string(&artifact.storage_path) {
                Ok(text) => (text, true),
                Err(_) => (String::new(), false),
            }
        } else {
            (String::new(), false)
        };
        let worker_split_match = if case.expected_worker_profiles.is_empty() {
            true
        } else {
            let profiles = checkpoints
                .iter()
                .find(|checkpoint| checkpoint.phase == AiTaskPhase::Planning)
                .and_then(|checkpoint| checkpoint.payload.get("worker_plan"))
                .and_then(serde_json::Value::as_array)
                .map(|workers| {
                    workers
                        .iter()
                        .filter_map(|worker| {
                            worker
                                .get("worker_profile")
                                .and_then(serde_json::Value::as_str)
                        })
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            case.expected_worker_profiles
                .iter()
                .all(|expected| profiles.iter().any(|profile| profile == expected))
        };
        let artifact_content_match = case
            .required_artifact_substrings
            .iter()
            .all(|needle| artifact_text.contains(needle));

        if completed && artifact_content_match && runtime_budget_pass {
            completion_hits += 1;
        }
        if artifact_generated {
            artifact_hits += 1;
        }
        if verification_seen {
            verification_hits += 1;
        }

        let case_result = TaskCaseResult {
            name: case.name.clone(),
            completed: completed && artifact_content_match && runtime_budget_pass,
            artifact_generated,
            artifact_content_match,
            artifact_readable,
            runtime_budget_pass,
            worker_split_match,
            verification_seen,
        };

        let length_gate = {
            let mut violations = Vec::new();
            let gate = length_gate(
                "case_name_length",
                case.name.chars().count(),
                MAX_CASE_NAME_CHARS,
            );
            if !gate.pass {
                violations.push(gate.message.clone().unwrap_or_default());
            }
            if case.required_artifact_substrings.len() > MAX_REQUIRED_MATCHES {
                violations.push(format!(
                    "required artifact substring count {} exceeds {}",
                    case.required_artifact_substrings.len(),
                    MAX_REQUIRED_MATCHES
                ));
            }
            if artifact_text.chars().count() > MAX_ARTIFACT_TEXT_CHARS {
                violations.push(format!(
                    "artifact text length {} exceeds {}",
                    artifact_text.chars().count(),
                    MAX_ARTIFACT_TEXT_CHARS
                ));
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
        let malformed_output_gate = if !artifact_generated {
            super::judge::fail_gate(
                "malformed_output",
                EvalFailureBucket::MalformedOutput,
                "task did not produce a markdown artifact",
            )
        } else if !artifact_readable {
            super::judge::fail_gate(
                "malformed_output",
                EvalFailureBucket::MalformedOutput,
                "task artifact could not be read as UTF-8 text",
            )
        } else {
            pass_gate("malformed_output")
        };
        let exact_answer_gate =
            if artifact_generated && artifact_content_match && worker_split_match {
                pass_gate("exact_answer_contract")
            } else {
                super::judge::fail_gate(
                    "exact_answer_contract",
                    EvalFailureBucket::ExactAnswerMismatch,
                    format!(
                        "artifact_generated={}, artifact_content_match={}, worker_split_match={}",
                        artifact_generated, artifact_content_match, worker_split_match
                    ),
                )
            };

        let mut case_metrics = BTreeMap::new();
        case_metrics.insert(
            "completed".to_string(),
            if case_result.completed { 1.0 } else { 0.0 },
        );
        case_metrics.insert(
            "artifact_generated".to_string(),
            if case_result.artifact_generated {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "artifact_content_match".to_string(),
            if case_result.artifact_content_match {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "worker_split_match".to_string(),
            if case_result.worker_split_match {
                1.0
            } else {
                0.0
            },
        );
        case_metrics.insert(
            "verification_seen".to_string(),
            if case_result.verification_seen {
                1.0
            } else {
                0.0
            },
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
                    "task fixtures do not encode refusal semantics in phase 1",
                ),
                inapplicable_gate(
                    "acl_privacy_boundary",
                    "task harness fixtures are not role-scoped in phase 1",
                ),
                exact_answer_gate,
            ],
            serde_json::to_value(&case_result)?,
        ));
        details.push(case_result);
    }

    let cancel_resume_correct = cancellation_resume_correctness().await;
    let completion_metric = completion_hits as f64 / cases.len().max(1) as f64;
    let artifact_metric = artifact_hits as f64 / cases.len().max(1) as f64;
    let verification_metric = verification_hits as f64 / cases.len().max(1) as f64;
    let cancel_resume_metric = if cancel_resume_correct { 1.0 } else { 0.0 };

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "task_completion_success_rate".to_string(),
        completion_metric,
    );
    metrics.insert(
        "task_verifier_correction_rate".to_string(),
        verification_metric,
    );
    metrics.insert(
        "task_cancellation_resume_correctness".to_string(),
        cancel_resume_metric,
    );
    metrics.insert(
        "task_artifact_generation_success".to_string(),
        artifact_metric,
    );

    let thresholds = vec![
        EvalThreshold {
            metric: "task_completion_success_rate".to_string(),
            actual: completion_metric,
            expected: 0.80,
            pass: completion_metric >= 0.80,
            blocking: true,
        },
        EvalThreshold {
            metric: "task_cancellation_resume_correctness".to_string(),
            actual: cancel_resume_metric,
            expected: 1.0,
            pass: cancel_resume_metric == 1.0,
            blocking: true,
        },
    ];

    Ok(EvalSuiteReport::finalize(
        "tasks",
        metrics,
        thresholds,
        case_verdicts,
        serde_json::to_value(details)?,
    ))
}

async fn cancellation_resume_correctness() -> bool {
    let state = test_state();
    let store = MemoryAiTaskStore::default();
    let task = match store
        .create_task(
            &user(),
            &CreateAiTaskRequest {
                requested_model: None,
                input: AiTaskInput::DeepResearchReport {
                    objective: "".to_string(),
                    max_workers: Some(2),
                },
            },
        )
        .await
    {
        Ok(task) => task,
        Err(_) => return false,
    };

    let cancelled = match store.request_cancel(&task.id, &user().user_id).await {
        Ok(Some(task)) => task,
        _ => return false,
    };
    if cancelled.status != AiTaskStatus::Cancelled || cancelled.phase != AiTaskPhase::Cancelled {
        return false;
    }
    let resumed = match store.resume_task(&task.id, &user().user_id).await {
        Ok(Some(task)) => task,
        _ => return false,
    };
    if resumed.status != AiTaskStatus::Queued || resumed.phase != AiTaskPhase::Queued {
        return false;
    }
    run_task_with_store(state, store.clone(), &task.id)
        .await
        .is_ok()
        && store
            .get_task(&task.id)
            .await
            .ok()
            .flatten()
            .map(|task| task.status == AiTaskStatus::Completed)
            .unwrap_or(false)
}

fn user() -> TaskUserContext {
    TaskUserContext {
        user_id: "user-1".to_string(),
        username: "tester".to_string(),
        role: "user".to_string(),
        is_admin: false,
    }
}

fn test_state() -> AppState {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1/rustfin_test")
        .expect("lazy postgres pool");
    let tc_config = rustfin_transcoder::TranscoderConfig::default();
    let ffmpeg_path = tc_config.ffmpeg_path.clone();
    let ffprobe_path = tc_config.ffprobe_path.clone();
    let transcoder =
        std::sync::Arc::new(rustfin_transcoder::session::SessionManager::new(tc_config));
    let (events_tx, _) = tokio::sync::broadcast::channel(8);

    AppState {
        db: pool,
        rustyvault: RustyVaultRuntimeState::available(),
        jwt_secret: "eval-secret".to_string(),
        http: reqwest::Client::builder().build().unwrap(),
        runtime_metrics: crate::runtime_metrics::RuntimeMetrics::new(),
        tmdb_agent_url: "http://127.0.0.1:8100".to_string(),
        tmdb_agent_token: None,
        youtube_agent_url: "http://127.0.0.1:8101".to_string(),
        youtube_agent_token: None,
        transcription_agent_url: "http://127.0.0.1:8102".to_string(),
        transcription_agent_token: None,
        servers_agent_url: None,
        servers_agent_token: None,
        model_dir: std::sync::Arc::new(tokio::sync::RwLock::new(std::env::temp_dir())),
        engine: std::sync::Arc::new(tokio::sync::Mutex::new(crate::ai::EngineState::default())),
        transcoder,
        ffmpeg_path,
        ffprobe_path,
        transcoder_hw_accel: None,
        transcoder_hw_accel_required: false,
        cache_dir: std::env::temp_dir()
            .join(format!("rustyfin-ai-evals-cache-{}", uuid::Uuid::new_v4())),
        watch_party_audio_dir: std::env::temp_dir()
            .join(format!("rustyfin-ai-evals-audio-{}", uuid::Uuid::new_v4())),
        events: events_tx,
        watch_party: std::sync::Arc::new(crate::watch_party::manager::WatchPartyManager::new()),
        channel_manager: std::sync::Arc::new(crate::channels::manager::ChannelManager::new()),
    }
}
