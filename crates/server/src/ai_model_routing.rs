use std::collections::HashSet;

use rustfin_ai_agent::{BackendKind, ModelRole, ModelSelectionSource, RoleModelSelection};
use serde::{Deserialize, Serialize};

use crate::ai_admin::AiRemoteBackendConfig;
use crate::ai_benchmark_recommendations::{
    BenchmarkRecommendationResolution, BenchmarkRecommendationStatus, resolve_profile_for_model,
    resolve_recommended_profile_for_role,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRoutingDecision {
    pub role: ModelRole,
    pub model_name: String,
    pub backend_id: String,
    pub backend_kind: BackendKind,
    pub selection_source: ModelSelectionSource,
    pub recommendation_status: BenchmarkRecommendationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_updated_ts: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoleRouting {
    pub selection: RoleModelSelection,
    pub decision: RoleRoutingDecision,
    pub tuning_profile: Option<rustfin_db::repo::ai_models::AiModelProfileRow>,
}

pub fn resolve_role_routing_plan(
    requested_answer_model: Option<&str>,
    available_models: &[String],
    profiles: &[rustfin_db::repo::ai_models::AiModelProfileRow],
    remote_backend: Option<&AiRemoteBackendConfig>,
    now_ts: i64,
) -> Vec<ResolvedRoleRouting> {
    let available = available_models.iter().cloned().collect::<HashSet<_>>();
    let answer = resolve_answer_role(requested_answer_model, &available, profiles, now_ts);

    ModelRole::all()
        .into_iter()
        .map(|role| {
            if role == ModelRole::Answer {
                return answer.clone();
            }
            if let Some(remote) = remote_backend.filter(|config| role_routes_remote(config, role)) {
                return ResolvedRoleRouting {
                    selection: RoleModelSelection {
                        model_name: remote.model.clone(),
                        source: ModelSelectionSource::Fallback,
                    },
                    decision: RoleRoutingDecision {
                        role,
                        model_name: remote.model.clone(),
                        backend_id: "remote".to_string(),
                        backend_kind: BackendKind::Remote,
                        selection_source: ModelSelectionSource::Fallback,
                        recommendation_status: BenchmarkRecommendationStatus::NotApplicable,
                        recommendation_note: Some(format!(
                            "remote backend routing is configured for the {} role",
                            role.as_str()
                        )),
                        recommendation_model_name: None,
                        recommendation_updated_ts: None,
                    },
                    tuning_profile: None,
                };
            }

            let recommendation =
                resolve_recommended_profile_for_role(profiles, &available, role, now_ts);
            if let Some(profile) = recommendation.profile.clone() {
                return resolved_local_role(
                    role,
                    profile.model_name.clone(),
                    ModelSelectionSource::StoredRecommendation,
                    recommendation,
                );
            }

            if let Some(env_model) =
                role_model_override(role).filter(|model| available.contains(model))
            {
                let tuning = resolve_profile_for_model(profiles, &available, &env_model, now_ts);
                return resolved_local_role(
                    role,
                    env_model,
                    ModelSelectionSource::EnvDefault,
                    tuning,
                );
            }

            let mut fallback = resolved_local_role(
                role,
                answer.selection.model_name.clone(),
                ModelSelectionSource::Fallback,
                resolve_profile_for_model(
                    profiles,
                    &available,
                    &answer.selection.model_name,
                    now_ts,
                ),
            );
            if fallback.decision.recommendation_note.is_none() {
                fallback.decision.recommendation_note = recommendation.note;
            }
            fallback
        })
        .collect()
}

fn resolve_answer_role(
    requested_answer_model: Option<&str>,
    available: &HashSet<String>,
    profiles: &[rustfin_db::repo::ai_models::AiModelProfileRow],
    now_ts: i64,
) -> ResolvedRoleRouting {
    if let Some(model_name) = requested_answer_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .filter(|model_name| available.contains(model_name))
    {
        return resolved_local_role(
            ModelRole::Answer,
            model_name.clone(),
            ModelSelectionSource::ExplicitRequest,
            resolve_profile_for_model(profiles, available, &model_name, now_ts),
        );
    }

    let recommendation =
        resolve_recommended_profile_for_role(profiles, available, ModelRole::Answer, now_ts);
    if let Some(profile) = recommendation.profile.clone() {
        return resolved_local_role(
            ModelRole::Answer,
            profile.model_name.clone(),
            ModelSelectionSource::StoredRecommendation,
            recommendation,
        );
    }

    if let Some(env_model) =
        role_model_override(ModelRole::Answer).filter(|model| available.contains(model))
    {
        return resolved_local_role(
            ModelRole::Answer,
            env_model.clone(),
            ModelSelectionSource::EnvDefault,
            resolve_profile_for_model(profiles, available, &env_model, now_ts),
        );
    }

    let fallback_model = available.iter().next().cloned().unwrap_or_else(|| {
        requested_answer_model
            .unwrap_or_default()
            .trim()
            .to_string()
    });
    let mut resolved = resolved_local_role(
        ModelRole::Answer,
        fallback_model.clone(),
        ModelSelectionSource::Fallback,
        resolve_profile_for_model(profiles, available, &fallback_model, now_ts),
    );
    if resolved.decision.recommendation_note.is_none() && recommendation.note.is_some() {
        resolved.decision.recommendation_note = recommendation.note;
    }
    resolved
}

fn resolved_local_role(
    role: ModelRole,
    model_name: String,
    selection_source: ModelSelectionSource,
    recommendation: BenchmarkRecommendationResolution,
) -> ResolvedRoleRouting {
    let recommendation_model_name = recommendation
        .profile
        .as_ref()
        .map(|profile| profile.model_name.clone());
    let recommendation_updated_ts = recommendation
        .profile
        .as_ref()
        .map(|profile| profile.updated_ts);
    ResolvedRoleRouting {
        selection: RoleModelSelection {
            model_name: model_name.clone(),
            source: selection_source,
        },
        decision: RoleRoutingDecision {
            role,
            model_name,
            backend_id: "local_llama".to_string(),
            backend_kind: BackendKind::Local,
            selection_source,
            recommendation_status: recommendation.status,
            recommendation_note: recommendation.note.clone(),
            recommendation_model_name,
            recommendation_updated_ts,
        },
        tuning_profile: recommendation.profile,
    }
}

fn role_routes_remote(config: &AiRemoteBackendConfig, role: ModelRole) -> bool {
    config.enabled
        && !config.base_url.trim().is_empty()
        && !config.model.trim().is_empty()
        && config
            .route_roles
            .iter()
            .any(|candidate| candidate == role.as_str() || candidate == "all")
}

fn role_model_override(role: ModelRole) -> Option<String> {
    std::env::var(role_model_override_env(role))
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn role_model_override_env(role: ModelRole) -> &'static str {
    match role {
        ModelRole::Planner => "RUSTFIN_AI_PLANNER_MODEL",
        ModelRole::Summarizer => "RUSTFIN_AI_SUMMARIZER_MODEL",
        ModelRole::Answer => "RUSTFIN_AI_ANSWER_MODEL",
        ModelRole::Verifier => "RUSTFIN_AI_VERIFIER_MODEL",
        ModelRole::Worker => "RUSTFIN_AI_WORKER_MODEL",
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_role_routing_plan;
    use crate::ai_admin::AiRemoteBackendConfig;
    use rustfin_ai_agent::{BackendKind, ModelRole, ModelSelectionSource};

    fn profile(
        model_name: &str,
        updated_ts: i64,
        tps: f64,
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
            estimated_model_bytes: 100,
            notes_json: "[]".to_string(),
            last_benchmark_label: "bench".to_string(),
            last_load_duration_ms: 100,
            last_tokens_per_second: tps,
            benchmark_count: 1,
            created_ts: updated_ts,
            updated_ts,
        }
    }

    #[test]
    fn explicit_answer_model_only_applies_to_answer_role() {
        let routes = resolve_role_routing_plan(
            Some("answer.gguf"),
            &["answer.gguf".to_string(), "planner.gguf".to_string()],
            &[profile("planner.gguf", 100, 50.0)],
            None,
            200,
        );
        let answer = routes
            .iter()
            .find(|route| route.decision.role == ModelRole::Answer)
            .unwrap();
        let planner = routes
            .iter()
            .find(|route| route.decision.role == ModelRole::Planner)
            .unwrap();
        assert_eq!(answer.decision.model_name, "answer.gguf");
        assert_eq!(
            answer.decision.selection_source,
            ModelSelectionSource::ExplicitRequest
        );
        assert_eq!(planner.decision.model_name, "planner.gguf");
        assert_eq!(
            planner.decision.selection_source,
            ModelSelectionSource::StoredRecommendation
        );
    }

    #[test]
    fn stale_recommendation_is_ignored_for_planner_role() {
        let routes = resolve_role_routing_plan(
            Some("answer.gguf"),
            &["answer.gguf".to_string(), "planner.gguf".to_string()],
            &[profile("planner.gguf", 0, 50.0)],
            None,
            i64::MAX,
        );
        let planner = routes
            .iter()
            .find(|route| route.decision.role == ModelRole::Planner)
            .unwrap();
        assert_eq!(planner.decision.model_name, "answer.gguf");
        assert_eq!(
            planner.decision.selection_source,
            ModelSelectionSource::Fallback
        );
    }

    #[test]
    fn missing_recommended_model_falls_back_to_answer_model() {
        let routes = resolve_role_routing_plan(
            Some("answer.gguf"),
            &["answer.gguf".to_string()],
            &[profile("planner.gguf", 100, 50.0)],
            None,
            200,
        );
        let planner = routes
            .iter()
            .find(|route| route.decision.role == ModelRole::Planner)
            .unwrap();
        assert_eq!(planner.decision.model_name, "answer.gguf");
        assert_eq!(
            planner.decision.selection_source,
            ModelSelectionSource::Fallback
        );
    }

    #[test]
    fn remote_backend_route_marks_role_as_remote() {
        let routes = resolve_role_routing_plan(
            Some("answer.gguf"),
            &["answer.gguf".to_string()],
            &[],
            Some(&AiRemoteBackendConfig {
                enabled: true,
                base_url: "https://example.com/v1/chat/completions".to_string(),
                model: "remote-planner".to_string(),
                api_key_env: None,
                timeout_secs: 60,
                supports_prompt_cache: false,
                supports_structured_output: true,
                max_parallel_requests: 1,
                overload_fallback: false,
                route_roles: vec!["planner".to_string()],
            }),
            100,
        );
        let planner = routes
            .iter()
            .find(|route| route.decision.role == ModelRole::Planner)
            .unwrap();
        assert_eq!(planner.decision.backend_kind, BackendKind::Remote);
        assert_eq!(planner.decision.model_name, "remote-planner");
    }
}
