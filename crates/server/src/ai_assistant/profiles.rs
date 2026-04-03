use rustfin_ai_agent::SamplingParams;
use serde::Serialize;

use super::types::AssistantResponseMode;

pub const PLANNER_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_AI_MAX_CONCURRENT_REQUESTS: u64 = 2;

#[derive(Debug, Clone, Serialize)]
pub struct AssistantTurnBudget {
    pub context_length_tokens: u32,
    pub prompt_budget_tokens: u32,
    pub reserved_completion_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct AssistantModelProfile {
    pub response_mode: AssistantResponseMode,
    pub turn_budget: AssistantTurnBudget,
    pub planner_sampling: SamplingParams,
    pub answer_sampling: SamplingParams,
    pub memory_sampling: SamplingParams,
    pub artifact_verifier_sampling: SamplingParams,
    pub overload_max_concurrent_requests: u64,
}

pub fn assistant_model_profile(
    response_mode: AssistantResponseMode,
    context_window_tokens: u32,
) -> AssistantModelProfile {
    AssistantModelProfile {
        response_mode,
        turn_budget: AssistantTurnBudget {
            context_length_tokens: context_window_tokens,
            prompt_budget_tokens: prompt_budget_tokens(response_mode, context_window_tokens),
            reserved_completion_tokens: response_mode_completion_reserve_tokens(
                response_mode,
                context_window_tokens,
            ),
        },
        planner_sampling: planner_sampling_params(),
        answer_sampling: answer_sampling_params(response_mode),
        memory_sampling: memory_sampling_params(),
        artifact_verifier_sampling: artifact_verifier_sampling_params(),
        overload_max_concurrent_requests: configured_ai_max_concurrent_requests(),
    }
}

pub fn configured_ai_max_concurrent_requests() -> u64 {
    std::env::var("RUSTFIN_AI_MAX_CONCURRENT_REQUESTS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AI_MAX_CONCURRENT_REQUESTS)
}

pub fn planner_sampling_params() -> SamplingParams {
    SamplingParams {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 20,
        repeat_penalty: 1.05,
        max_tokens: 320,
        max_duration_ms: Some(12_000),
    }
}

pub fn memory_sampling_params() -> SamplingParams {
    SamplingParams {
        temperature: 0.15,
        top_p: 0.9,
        top_k: 32,
        repeat_penalty: 1.05,
        max_tokens: 384,
        max_duration_ms: Some(15_000),
    }
}

pub fn artifact_verifier_sampling_params() -> SamplingParams {
    SamplingParams {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 24,
        repeat_penalty: 1.05,
        max_tokens: 256,
        max_duration_ms: Some(12_000),
    }
}

pub fn answer_sampling_params(response_mode: AssistantResponseMode) -> SamplingParams {
    match response_mode {
        AssistantResponseMode::Instant => SamplingParams {
            temperature: 0.35,
            top_p: 0.9,
            top_k: 24,
            repeat_penalty: 1.08,
            max_tokens: 640,
            max_duration_ms: None,
        },
        AssistantResponseMode::Thinking => SamplingParams {
            temperature: 0.55,
            top_p: 0.92,
            top_k: 40,
            repeat_penalty: 1.1,
            max_tokens: 1536,
            max_duration_ms: None,
        },
        AssistantResponseMode::Extended => SamplingParams {
            temperature: 0.6,
            top_p: 0.94,
            top_k: 48,
            repeat_penalty: 1.08,
            max_tokens: u32::MAX,
            max_duration_ms: Some(30 * 60 * 1000),
        },
    }
}

pub fn response_mode_completion_reserve_tokens(
    response_mode: AssistantResponseMode,
    context_window_tokens: u32,
) -> u32 {
    let dynamic = match response_mode {
        AssistantResponseMode::Instant => context_window_tokens / 6,
        AssistantResponseMode::Thinking => context_window_tokens / 4,
        AssistantResponseMode::Extended => context_window_tokens / 3,
    };

    match response_mode {
        AssistantResponseMode::Instant => dynamic.clamp(384, 640),
        AssistantResponseMode::Thinking => dynamic.clamp(768, 1536),
        AssistantResponseMode::Extended => dynamic.clamp(1024, 4096),
    }
}

pub fn prompt_budget_tokens(
    response_mode: AssistantResponseMode,
    context_window_tokens: u32,
) -> u32 {
    const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 192;

    context_window_tokens.saturating_sub(
        response_mode_completion_reserve_tokens(response_mode, context_window_tokens)
            + CONTEXT_SAFETY_MARGIN_TOKENS,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_AI_MAX_CONCURRENT_REQUESTS, assistant_model_profile,
        configured_ai_max_concurrent_requests, prompt_budget_tokens,
    };
    use crate::ai_assistant::types::AssistantResponseMode;

    #[test]
    fn prompt_budget_reserves_more_space_for_slower_modes() {
        let context_window = 8192;

        assert!(
            prompt_budget_tokens(AssistantResponseMode::Instant, context_window)
                > prompt_budget_tokens(AssistantResponseMode::Thinking, context_window)
        );
        assert!(
            prompt_budget_tokens(AssistantResponseMode::Thinking, context_window)
                > prompt_budget_tokens(AssistantResponseMode::Extended, context_window)
        );
    }

    #[test]
    fn extended_mode_profile_uses_long_running_answer_budget() {
        let profile = assistant_model_profile(AssistantResponseMode::Extended, 16384);
        assert_eq!(
            profile.answer_sampling.max_duration_ms,
            Some(30 * 60 * 1000)
        );
        assert_eq!(profile.answer_sampling.max_tokens, u32::MAX);
        assert!(profile.turn_budget.reserved_completion_tokens >= 1024);
    }

    #[test]
    fn overload_limit_defaults_to_small_local_concurrency() {
        assert_eq!(
            configured_ai_max_concurrent_requests(),
            DEFAULT_AI_MAX_CONCURRENT_REQUESTS
        );
    }
}
