use std::collections::VecDeque;

use super::provider::ToolExecutionProfile;
use super::recovery::choose_recovery_step;
use super::synthesis::{collect_retained_evidence, conflicting_evidence_count};
use super::types::{
    AssistantClarificationRequest, AssistantExecutionAttempt, AssistantExecutionBudget,
    AssistantExecutionStopReason, AssistantExecutionTrace, AssistantGroundingSource,
    AssistantPlannerMode, AssistantRecoveryDecision, AssistantResponseMode, AssistantSynthesisMode,
    AssistantToolOutcome, PlannedToolCall,
};

#[derive(Debug, Clone)]
pub struct GroundedExecutionStep {
    pub step_index: u32,
    pub call: PlannedToolCall,
    pub edge_label: Option<String>,
    pub recovery_depth: u8,
    pub is_alternate: bool,
}

#[derive(Debug, Clone)]
pub struct GroundedExecutionRecord {
    pub step: GroundedExecutionStep,
    pub outcome: AssistantToolOutcome,
    pub source: AssistantGroundingSource,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorPostStep {
    Continue,
    Stop,
    AskClarification,
}

pub struct AssistantGroundedExecutor {
    message: String,
    profile: ToolExecutionProfile,
    queue: VecDeque<GroundedExecutionStep>,
    records: Vec<GroundedExecutionRecord>,
    retained_indices: Vec<usize>,
    trace: AssistantExecutionTrace,
    clarification: Option<AssistantClarificationRequest>,
    next_step_index: u32,
}

impl AssistantGroundedExecutor {
    pub fn new(
        message: &str,
        response_mode: AssistantResponseMode,
        planner_mode: Option<AssistantPlannerMode>,
        initial_calls: &[PlannedToolCall],
        used_role_backends: Vec<String>,
        profile: ToolExecutionProfile,
    ) -> Self {
        let budget = AssistantExecutionBudget::for_mode(response_mode);
        let mut queue = VecDeque::new();
        for call in initial_calls
            .iter()
            .take(usize::from(budget.max_tool_steps))
            .cloned()
        {
            let step_index = u32::try_from(queue.len() + 1).unwrap_or(u32::MAX);
            queue.push_back(GroundedExecutionStep {
                step_index,
                call,
                edge_label: None,
                recovery_depth: 0,
                is_alternate: false,
            });
        }

        Self {
            message: message.to_string(),
            profile,
            queue,
            records: Vec::new(),
            retained_indices: Vec::new(),
            trace: AssistantExecutionTrace {
                response_mode,
                budget,
                planner_mode: planner_mode.map(|mode| mode.as_str().to_string()),
                attempts: Vec::new(),
                retained_evidence: Vec::new(),
                stop_reason: AssistantExecutionStopReason::WeakEvidenceOnly,
                final_outcome_kind: None,
                final_answer_path: AssistantSynthesisMode::None,
                planner_pass_count: 1,
                tool_step_count: 0,
                alternate_tool_count: 0,
                recovery_step_count: 0,
                clarification_count: 0,
                conflict_count: 0,
                deterministic_answer_used: false,
                synthesis_used: false,
                used_role_backends,
                outcome_counts: Default::default(),
            },
            clarification: None,
            next_step_index: u32::try_from(initial_calls.len() + 1).unwrap_or(u32::MAX),
        }
    }

    pub fn budget(&self) -> &AssistantExecutionBudget {
        &self.trace.budget
    }

    pub fn next_step(&mut self) -> Option<GroundedExecutionStep> {
        self.queue.pop_front()
    }

    pub fn record_step(
        &mut self,
        step: GroundedExecutionStep,
        outcome: AssistantToolOutcome,
        source: AssistantGroundingSource,
        latency_ms: u64,
    ) -> ExecutorPostStep {
        self.trace.tool_step_count = self.trace.tool_step_count.saturating_add(1);
        self.trace
            .outcome_counts
            .entry(outcome.kind.as_str().to_string())
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
        if step.is_alternate {
            self.trace.alternate_tool_count = self.trace.alternate_tool_count.saturating_add(1);
        }
        if step.recovery_depth > 0 {
            self.trace.recovery_step_count = self.trace.recovery_step_count.saturating_add(1);
        }
        if matches!(
            outcome.kind,
            super::types::AssistantToolOutcomeKind::Conflicting
        ) {
            self.trace.conflict_count = self.trace.conflict_count.saturating_add(1);
        }

        let attempt = AssistantExecutionAttempt {
            step_index: step.step_index,
            tool: outcome.tool.clone(),
            label: outcome.label.clone(),
            domain_family: outcome.domain_family,
            status: outcome.block.status.to_string(),
            outcome_kind: outcome.kind,
            latency_ms,
            args_hash: outcome.args_hash.clone(),
            result_signature: outcome.result_signature.clone(),
            evidence_ids: outcome
                .evidence_items
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            ambiguity_keys: outcome.ambiguity_keys.clone(),
            fallback_edge: step.edge_label.clone(),
            used_alternate: step.is_alternate,
            recovery_depth: step.recovery_depth,
            message: outcome.message.clone(),
        };
        self.trace.attempts.push(attempt);

        let record_index = self.records.len();
        let should_retain = should_retain_outcome(&outcome) || self.queue.is_empty();
        self.records.push(GroundedExecutionRecord {
            step,
            outcome,
            source,
            latency_ms,
        });
        if should_retain {
            self.retained_indices.push(record_index);
        }

        if let Some(last) = self.records.last() {
            self.trace.final_outcome_kind = Some(last.outcome.kind);
        }

        if let Some(request) = self
            .records
            .last()
            .and_then(|record| clarification_request_for_outcome(&record.outcome))
        {
            self.trace.clarification_count = self.trace.clarification_count.saturating_add(1);
            self.trace.stop_reason = AssistantExecutionStopReason::ClarificationRequired;
            self.trace.final_answer_path = AssistantSynthesisMode::Clarification;
            self.clarification = Some(request);
            self.refresh_retained_evidence();
            return ExecutorPostStep::AskClarification;
        }

        if !self.queue.is_empty()
            && !matches!(
                self.records.last().map(|record| record.outcome.kind),
                Some(
                    super::types::AssistantToolOutcomeKind::Denied
                        | super::types::AssistantToolOutcomeKind::FatalError
                )
            )
        {
            self.refresh_retained_evidence();
            return ExecutorPostStep::Continue;
        }

        let latest_record = self.records.last().expect("recorded step");
        match choose_recovery_step(
            &self.message,
            self.trace.response_mode,
            &self.trace.budget,
            &self.trace,
            &latest_record.step.call,
            &latest_record.outcome,
            &self.profile,
        ) {
            AssistantRecoveryDecision::RunNext {
                call,
                edge_label,
                recovery_depth,
                is_alternate,
            } => {
                self.queue.push_back(GroundedExecutionStep {
                    step_index: self.next_step_index,
                    call,
                    edge_label: Some(edge_label),
                    recovery_depth,
                    is_alternate,
                });
                self.next_step_index = self.next_step_index.saturating_add(1);
                self.refresh_retained_evidence();
                ExecutorPostStep::Continue
            }
            AssistantRecoveryDecision::AskClarification { request } => {
                self.trace.clarification_count = self.trace.clarification_count.saturating_add(1);
                self.trace.stop_reason = AssistantExecutionStopReason::ClarificationRequired;
                self.trace.final_answer_path = AssistantSynthesisMode::Clarification;
                self.clarification = Some(request);
                self.refresh_retained_evidence();
                ExecutorPostStep::AskClarification
            }
            AssistantRecoveryDecision::Stop { reason } => {
                self.trace.stop_reason = reason;
                self.refresh_retained_evidence();
                ExecutorPostStep::Stop
            }
            AssistantRecoveryDecision::SynthesizeNow => {
                self.trace.stop_reason = AssistantExecutionStopReason::SufficientAnswer;
                self.trace.final_answer_path = AssistantSynthesisMode::DeterministicSynthesis;
                self.trace.synthesis_used = true;
                self.refresh_retained_evidence();
                ExecutorPostStep::Stop
            }
            AssistantRecoveryDecision::DeterministicReplyNow => {
                self.trace.stop_reason = AssistantExecutionStopReason::DeterministicReply;
                self.trace.final_answer_path = AssistantSynthesisMode::DeterministicReply;
                self.trace.deterministic_answer_used = true;
                self.refresh_retained_evidence();
                ExecutorPostStep::Stop
            }
            AssistantRecoveryDecision::VerifierPass => {
                self.trace.stop_reason = AssistantExecutionStopReason::ConflictUnresolved;
                self.refresh_retained_evidence();
                ExecutorPostStep::Stop
            }
        }
    }

    pub fn finalize_deterministic_reply(&mut self) {
        self.trace.stop_reason = AssistantExecutionStopReason::DeterministicReply;
        self.trace.final_answer_path = AssistantSynthesisMode::DeterministicReply;
        self.trace.deterministic_answer_used = true;
        self.refresh_retained_evidence();
    }

    pub fn finalize_model_answer(&mut self) {
        self.trace.stop_reason = AssistantExecutionStopReason::ModelAnswerCompleted;
        self.trace.final_answer_path = AssistantSynthesisMode::ModelAnswer;
        self.refresh_retained_evidence();
    }

    pub fn finalize_bounded_failure(&mut self) {
        if self.clarification.is_some() {
            self.trace.stop_reason = AssistantExecutionStopReason::ClarificationRequired;
            self.trace.final_answer_path = AssistantSynthesisMode::Clarification;
        } else if self.trace.tool_step_count >= u32::from(self.trace.budget.max_tool_steps) {
            self.trace.stop_reason = AssistantExecutionStopReason::BudgetExhausted;
            self.trace.final_answer_path = AssistantSynthesisMode::BoundedFailure;
        } else if self.trace.stop_reason == AssistantExecutionStopReason::WeakEvidenceOnly {
            self.trace.final_answer_path = AssistantSynthesisMode::BoundedFailure;
        }
        self.refresh_retained_evidence();
    }

    pub fn clarification(&self) -> Option<&AssistantClarificationRequest> {
        self.clarification.as_ref()
    }

    pub fn retained_records(&self) -> Vec<GroundedExecutionRecord> {
        let mut records = self
            .retained_indices
            .iter()
            .filter_map(|index| self.records.get(*index).cloned())
            .collect::<Vec<_>>();
        if records.is_empty() {
            if let Some(last) = self.records.last() {
                records.push(last.clone());
            }
        }
        records
    }

    pub fn all_records(&self) -> &[GroundedExecutionRecord] {
        &self.records
    }

    pub fn trace(&self) -> &AssistantExecutionTrace {
        &self.trace
    }

    fn refresh_retained_evidence(&mut self) {
        let retained_outcomes = self
            .retained_indices
            .iter()
            .filter_map(|index| self.records.get(*index))
            .map(|record| record.outcome.clone())
            .collect::<Vec<_>>();
        self.trace.retained_evidence = collect_retained_evidence(
            &retained_outcomes,
            usize::from(self.trace.budget.max_evidence_items),
        );
        self.trace.conflict_count = self
            .trace
            .conflict_count
            .max(conflicting_evidence_count(&self.trace.retained_evidence));
    }
}

fn should_retain_outcome(outcome: &AssistantToolOutcome) -> bool {
    !matches!(
        outcome.kind,
        super::types::AssistantToolOutcomeKind::Denied
            | super::types::AssistantToolOutcomeKind::FatalError
            | super::types::AssistantToolOutcomeKind::TransientError
    )
}

fn clarification_request_for_outcome(
    outcome: &AssistantToolOutcome,
) -> Option<AssistantClarificationRequest> {
    if matches!(
        outcome.kind,
        super::types::AssistantToolOutcomeKind::ClarificationNeeded
            | super::types::AssistantToolOutcomeKind::Ambiguous
    ) {
        Some(AssistantClarificationRequest {
            message: outcome
                .message
                .clone()
                .unwrap_or_else(|| "I need one more detail before I can answer that.".to_string()),
            missing_field: None,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{AssistantGroundedExecutor, ExecutorPostStep};
    use crate::ai_assistant::provider::ToolExecutionProfile;
    use crate::ai_assistant::registry::AssistantToolName;
    use crate::ai_assistant::types::{
        AssistantGroundingSource, AssistantPlannerMode, AssistantResponseMode,
        AssistantToolContextBlock, AssistantToolInput, PlannedToolCall,
    };
    use serde_json::json;

    fn call(tool: AssistantToolName) -> PlannedToolCall {
        PlannedToolCall {
            tool,
            input: match tool {
                AssistantToolName::WeatherGetForecast => AssistantToolInput::Weather {
                    location: "Cork".to_string(),
                    forecast_days: Some(2),
                },
                _ => AssistantToolInput::None,
            },
        }
    }

    #[test]
    fn instant_mode_limits_initial_queue_to_one_step() {
        let mut executor = AssistantGroundedExecutor::new(
            "test",
            AssistantResponseMode::Instant,
            Some(AssistantPlannerMode::DeterministicFallback),
            &[
                call(AssistantToolName::CalendarGetNextEvent),
                call(AssistantToolName::WeatherGetForecast),
            ],
            Vec::new(),
            ToolExecutionProfile::full_access(),
        );
        assert!(executor.next_step().is_some());
        assert!(executor.next_step().is_none());
    }

    #[test]
    fn executor_records_stop_reason_for_clarification() {
        let mut executor = AssistantGroundedExecutor::new(
            "What's the weather in Cork?",
            AssistantResponseMode::Thinking,
            Some(AssistantPlannerMode::DeterministicFallback),
            &[call(AssistantToolName::WeatherGetForecast)],
            Vec::new(),
            ToolExecutionProfile::full_access(),
        );
        let step = executor.next_step().expect("step");
        let outcome = crate::ai_assistant::outcomes::normalize_tool_result(
            "What's the weather in Cork?",
            &step.call,
            AssistantToolContextBlock {
                tool: "weather_get_forecast",
                label: "Forecast".to_string(),
                status: "ok",
                data: json!({"message":"clarification:I found multiple locations matching \"Cork\". Which one did you mean?"}),
            },
        );
        let status = executor.record_step(
            step,
            outcome,
            AssistantGroundingSource {
                tool: "weather_get_forecast".to_string(),
                label: "Forecast".to_string(),
                access_mode: crate::ai_assistant::types::ToolAccessMode::ReadOnly,
                risk_tier: crate::ai_assistant::types::ToolRiskTier::Moderate,
                status: "ok".to_string(),
                download_url: None,
                download_file_name: None,
                download_media_type: None,
                download_size_bytes: None,
            },
            12,
        );
        assert_eq!(status, ExecutorPostStep::AskClarification);
        assert_eq!(
            executor.trace().stop_reason.as_str(),
            "clarification_required"
        );
    }
}
