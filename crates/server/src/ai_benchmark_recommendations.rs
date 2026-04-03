use std::cmp::Ordering;
use std::collections::HashSet;

use rustfin_ai_agent::ModelRole;
use serde::{Deserialize, Serialize};

pub const DEFAULT_BENCHMARK_RECOMMENDATION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
pub const BENCHMARK_RECOMMENDATION_TTL_ENV: &str = "RUSTFIN_AI_BENCHMARK_RECOMMENDATION_TTL_SECS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkRecommendationStatus {
    Applied,
    Missing,
    Stale,
    ModelMissing,
    NotApplicable,
}

impl BenchmarkRecommendationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::ModelMissing => "model_missing",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BenchmarkRecommendationResolution {
    pub status: BenchmarkRecommendationStatus,
    pub profile: Option<rustfin_db::repo::ai_models::AiModelProfileRow>,
    pub note: Option<String>,
}

pub fn benchmark_recommendation_ttl_secs() -> i64 {
    std::env::var(BENCHMARK_RECOMMENDATION_TTL_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_BENCHMARK_RECOMMENDATION_TTL_SECS)
}

pub fn resolve_profile_for_model(
    profiles: &[rustfin_db::repo::ai_models::AiModelProfileRow],
    available_models: &HashSet<String>,
    model_name: &str,
    now_ts: i64,
) -> BenchmarkRecommendationResolution {
    let Some(profile) = profiles
        .iter()
        .find(|row| row.model_name == model_name)
        .cloned()
    else {
        return BenchmarkRecommendationResolution {
            status: BenchmarkRecommendationStatus::Missing,
            profile: None,
            note: Some(format!(
                "no stored benchmark recommendation for `{model_name}`"
            )),
        };
    };

    if is_profile_stale(profile.updated_ts, now_ts) {
        return BenchmarkRecommendationResolution {
            status: BenchmarkRecommendationStatus::Stale,
            profile: None,
            note: Some(format!(
                "stored benchmark recommendation for `{model_name}` is stale"
            )),
        };
    }

    if !available_models.contains(&profile.model_name) {
        return BenchmarkRecommendationResolution {
            status: BenchmarkRecommendationStatus::ModelMissing,
            profile: None,
            note: Some(format!(
                "stored benchmark recommendation for `{model_name}` references a missing model"
            )),
        };
    }

    BenchmarkRecommendationResolution {
        status: BenchmarkRecommendationStatus::Applied,
        profile: Some(profile),
        note: None,
    }
}

pub fn resolve_recommended_profile_for_role(
    profiles: &[rustfin_db::repo::ai_models::AiModelProfileRow],
    available_models: &HashSet<String>,
    role: ModelRole,
    now_ts: i64,
) -> BenchmarkRecommendationResolution {
    let mut saw_stale = false;
    let mut saw_missing_model = false;

    let mut candidates = profiles
        .iter()
        .filter_map(|profile| {
            if is_profile_stale(profile.updated_ts, now_ts) {
                saw_stale = true;
                return None;
            }
            if !available_models.contains(&profile.model_name) {
                saw_missing_model = true;
                return None;
            }
            Some(profile.clone())
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        let (status, note) = if saw_missing_model {
            (
                BenchmarkRecommendationStatus::ModelMissing,
                "stored benchmark recommendations reference missing local models".to_string(),
            )
        } else if saw_stale {
            (
                BenchmarkRecommendationStatus::Stale,
                "stored benchmark recommendations are stale".to_string(),
            )
        } else {
            (
                BenchmarkRecommendationStatus::Missing,
                "no stored benchmark recommendations exist for this host".to_string(),
            )
        };
        return BenchmarkRecommendationResolution {
            status,
            profile: None,
            note: Some(note),
        };
    }

    candidates.sort_by(|left, right| compare_role_candidates(role, left, right));
    BenchmarkRecommendationResolution {
        status: BenchmarkRecommendationStatus::Applied,
        profile: candidates.into_iter().next(),
        note: None,
    }
}

fn compare_role_candidates(
    role: ModelRole,
    left: &rustfin_db::repo::ai_models::AiModelProfileRow,
    right: &rustfin_db::repo::ai_models::AiModelProfileRow,
) -> Ordering {
    match role {
        ModelRole::Planner => compare_bool(
            left.supports_structured_output,
            right.supports_structured_output,
        )
        .then_with(|| compare_f64(left.last_tokens_per_second, right.last_tokens_per_second))
        .then_with(|| right.last_load_duration_ms.cmp(&left.last_load_duration_ms))
        .then_with(|| right.estimated_model_bytes.cmp(&left.estimated_model_bytes)),
        ModelRole::Summarizer => left
            .summary_max_output
            .cmp(&right.summary_max_output)
            .then_with(|| compare_f64(left.last_tokens_per_second, right.last_tokens_per_second))
            .then_with(|| right.last_load_duration_ms.cmp(&left.last_load_duration_ms)),
        ModelRole::Answer => left
            .context_window
            .cmp(&right.context_window)
            .then_with(|| compare_f64(left.last_tokens_per_second, right.last_tokens_per_second))
            .then_with(|| right.last_load_duration_ms.cmp(&left.last_load_duration_ms)),
        ModelRole::Verifier => compare_bool(
            left.supports_structured_output,
            right.supports_structured_output,
        )
        .then_with(|| compare_f64(left.last_tokens_per_second, right.last_tokens_per_second))
        .then_with(|| right.estimated_model_bytes.cmp(&left.estimated_model_bytes)),
        ModelRole::Worker => left
            .context_window
            .cmp(&right.context_window)
            .then_with(|| compare_f64(left.last_tokens_per_second, right.last_tokens_per_second))
            .then_with(|| right.estimated_model_bytes.cmp(&left.estimated_model_bytes)),
    }
    .reverse()
}

fn compare_bool(left: bool, right: bool) -> Ordering {
    left.cmp(&right)
}

fn compare_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn is_profile_stale(updated_ts: i64, now_ts: i64) -> bool {
    now_ts.saturating_sub(updated_ts) > benchmark_recommendation_ttl_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkRecommendationStatus, resolve_profile_for_model,
        resolve_recommended_profile_for_role,
    };
    use rustfin_ai_agent::ModelRole;
    use std::collections::HashSet;

    fn profile(
        model_name: &str,
        updated_ts: i64,
        last_tokens_per_second: f64,
        estimated_model_bytes: i64,
    ) -> rustfin_db::repo::ai_models::AiModelProfileRow {
        rustfin_db::repo::ai_models::AiModelProfileRow {
            id: format!("profile-{model_name}"),
            host_fingerprint: "host".to_string(),
            model_name: model_name.to_string(),
            model_checksum: format!("checksum-{model_name}"),
            model_path: format!("/models/{model_name}.gguf"),
            context_window: 4096,
            preferred_completion_tokens: 1024,
            planner_max_output: 256,
            summary_max_output: 512,
            safety_headroom: 256,
            warmup_cost_class: "low".to_string(),
            supports_structured_output: true,
            supports_prompt_cache: false,
            recommended_n_threads: 8,
            recommended_n_gpu_layers: 0,
            recommended_split_mode: "none".to_string(),
            recommended_main_gpu: None,
            recommended_device_indices_json: "[]".to_string(),
            estimated_model_bytes,
            notes_json: "[]".to_string(),
            last_benchmark_label: "bench".to_string(),
            last_load_duration_ms: 1_000,
            last_tokens_per_second,
            benchmark_count: 1,
            created_ts: updated_ts,
            updated_ts,
        }
    }

    #[test]
    fn exact_profile_resolution_uses_fresh_available_profile() {
        let profiles = vec![profile("planner.gguf", 1_000, 20.0, 100)];
        let available = HashSet::from(["planner.gguf".to_string()]);
        let resolution = resolve_profile_for_model(&profiles, &available, "planner.gguf", 2_000);
        assert_eq!(resolution.status, BenchmarkRecommendationStatus::Applied);
        assert_eq!(
            resolution
                .profile
                .as_ref()
                .map(|profile| profile.model_name.as_str()),
            Some("planner.gguf")
        );
    }

    #[test]
    fn stale_profile_resolution_is_ignored() {
        let profiles = vec![profile("planner.gguf", 0, 20.0, 100)];
        let available = HashSet::from(["planner.gguf".to_string()]);
        let resolution = resolve_profile_for_model(&profiles, &available, "planner.gguf", i64::MAX);
        assert_eq!(resolution.status, BenchmarkRecommendationStatus::Stale);
        assert!(resolution.profile.is_none());
    }

    #[test]
    fn planner_role_prefers_fastest_fresh_profile() {
        let profiles = vec![
            profile("small.gguf", 100, 48.0, 100),
            profile("large.gguf", 100, 12.0, 400),
        ];
        let available = HashSet::from(["small.gguf".to_string(), "large.gguf".to_string()]);
        let resolution =
            resolve_recommended_profile_for_role(&profiles, &available, ModelRole::Planner, 200);
        assert_eq!(resolution.status, BenchmarkRecommendationStatus::Applied);
        assert_eq!(
            resolution
                .profile
                .as_ref()
                .map(|profile| profile.model_name.as_str()),
            Some("small.gguf")
        );
    }
}
