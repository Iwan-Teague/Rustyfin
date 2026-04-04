use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::warn;

use crate::ai_admin::AiRemoteBackendConfig;

pub use rustfin_ai_agent::LlamaEngineParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurnPriority {
    Interactive,
    BackgroundTask,
    AdminBenchmark,
}

impl TurnPriority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::BackgroundTask => "background_task",
            Self::AdminBenchmark => "admin_benchmark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverloadState {
    Normal,
    Constrained,
    Degraded,
    Overloaded,
}

impl OverloadState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Constrained => "constrained",
            Self::Degraded => "degraded",
            Self::Overloaded => "overloaded",
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlannerBackendSelection {
    Local,
    Remote(AiRemoteBackendConfig),
}

#[derive(Debug, Clone)]
pub struct SchedulerDecision {
    pub queue_wait_ms: u64,
    pub overload_state: OverloadState,
    pub max_generation_tokens: u32,
    pub prefer_deterministic_planner: bool,
    pub planner_backend: PlannerBackendSelection,
}

#[derive(Debug, Clone)]
pub struct SchedulerSnapshot {
    pub max_concurrent_turns: u64,
    pub queue_limit: u64,
    pub active_turns: u64,
    pub queued_turns: u64,
    pub overload_state: String,
    pub warm_pool_bytes: u64,
    pub warm_pool_budget_bytes: u64,
    pub active_by_priority: Vec<PriorityCountSnapshot>,
    pub queued_by_priority: Vec<PriorityCountSnapshot>,
    pub warm_models: Vec<WarmModelSnapshot>,
    pub rejected_turns_total: u64,
    pub degraded_turns_total: u64,
}

#[derive(Debug, Clone)]
pub struct PriorityCountSnapshot {
    pub priority: String,
    pub count: u64,
}

#[derive(Debug, Clone)]
pub struct WarmModelSnapshot {
    pub model_name: String,
    pub estimated_bytes: u64,
    pub loaded_ts_ms: i64,
    pub last_used_ts_ms: i64,
    pub load_count: u64,
}

#[derive(Clone)]
struct WarmModelEntry {
    model_name: String,
    engine: rustfin_ai_agent::LlamaEngine,
    estimated_bytes: u64,
    loaded_ts_ms: i64,
    last_used_ts_ms: i64,
    load_count: u64,
}

#[derive(Clone)]
struct SchedulerState {
    active_turns: u64,
    queued_turns: u64,
    active_by_priority: HashMap<TurnPriority, u64>,
    queued_by_priority: HashMap<TurnPriority, u64>,
    warm_models: HashMap<String, WarmModelEntry>,
    warm_model_order: VecDeque<String>,
    warm_pool_bytes: u64,
    rejected_turns_total: u64,
    degraded_turns_total: u64,
    last_overload_state: OverloadState,
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self {
            active_turns: 0,
            queued_turns: 0,
            active_by_priority: HashMap::new(),
            queued_by_priority: HashMap::new(),
            warm_models: HashMap::new(),
            warm_model_order: VecDeque::new(),
            warm_pool_bytes: 0,
            rejected_turns_total: 0,
            degraded_turns_total: 0,
            last_overload_state: OverloadState::Normal,
        }
    }
}

pub struct TurnLease {
    scheduler: Arc<TurnScheduler>,
    _permit: OwnedSemaphorePermit,
    priority: TurnPriority,
}

impl Drop for TurnLease {
    fn drop(&mut self) {
        self.scheduler.finish_turn(self.priority);
    }
}

pub struct TurnScheduler {
    semaphore: Arc<Semaphore>,
    queue_limit: u64,
    warm_pool_budget_bytes: u64,
    state: Mutex<SchedulerState>,
    remote_backend: Mutex<Option<AiRemoteBackendConfig>>,
}

impl Default for TurnScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnScheduler {
    pub fn new() -> Self {
        let total_memory_bytes = total_system_memory_bytes().unwrap_or(8 * 1024 * 1024 * 1024);
        let max_concurrent_turns = std::env::var("RUSTFIN_AI_MAX_CONCURRENT_TURNS")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| {
                let logical_threads = std::thread::available_parallelism()
                    .map(|value| value.get())
                    .unwrap_or(4);
                logical_threads.clamp(2, 4)
            });
        let queue_limit = std::env::var("RUSTFIN_AI_MAX_QUEUE_DEPTH")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(16);
        let warm_pool_budget_bytes = std::env::var("RUSTFIN_AI_WARM_POOL_MAX_BYTES")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| default_warm_pool_budget_bytes(total_memory_bytes));

        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent_turns)),
            queue_limit,
            warm_pool_budget_bytes,
            state: Mutex::new(SchedulerState::default()),
            remote_backend: Mutex::new(parse_remote_backend_config()),
        }
    }

    pub fn set_remote_backend(&self, config: Option<AiRemoteBackendConfig>) {
        let mut guard = self
            .remote_backend
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *guard = config;
    }

    pub fn remote_backend(&self) -> Option<AiRemoteBackendConfig> {
        self.remote_backend
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub async fn acquire_model(
        self: &Arc<Self>,
        priority: TurnPriority,
        model_name: &str,
        model_path: PathBuf,
        params: LlamaEngineParams,
        estimated_bytes: u64,
    ) -> Result<
        (
            TurnLease,
            rustfin_ai_agent::LlamaEngine,
            u64,
            u64,
            SchedulerDecision,
        ),
        String,
    > {
        let queue_started = Instant::now();
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.queued_turns = state.queued_turns.saturating_add(1);
            *state.queued_by_priority.entry(priority).or_default() += 1;
            if state.queued_turns > self.queue_limit {
                state.queued_turns = state.queued_turns.saturating_sub(1);
                if let Some(count) = state.queued_by_priority.get_mut(&priority) {
                    *count = count.saturating_sub(1);
                }
                state.rejected_turns_total = state.rejected_turns_total.saturating_add(1);
                return Err(format!(
                    "AI queue is full ({} queued turns limit reached)",
                    self.queue_limit
                ));
            }
        }

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "AI scheduler stopped".to_string())?;
        let queue_wait_ms = queue_started.elapsed().as_millis() as u64;

        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.queued_turns = state.queued_turns.saturating_sub(1);
            if let Some(count) = state.queued_by_priority.get_mut(&priority) {
                *count = count.saturating_sub(1);
            }
            state.active_turns = state.active_turns.saturating_add(1);
            *state.active_by_priority.entry(priority).or_default() += 1;
        }

        let (engine, load_duration_ms) = match self
            .load_or_reuse_model(model_name, model_path, params, estimated_bytes)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.finish_turn(priority);
                return Err(error);
            }
        };

        let overload_state = self.compute_overload_state();
        let decision = SchedulerDecision {
            queue_wait_ms,
            overload_state,
            max_generation_tokens: match overload_state {
                OverloadState::Normal => 2048,
                OverloadState::Constrained => 1536,
                OverloadState::Degraded => 1024,
                OverloadState::Overloaded => 768,
            },
            prefer_deterministic_planner: matches!(
                overload_state,
                OverloadState::Degraded | OverloadState::Overloaded
            ),
            planner_backend: match self.remote_backend() {
                Some(remote)
                    if remote.should_route_planner_remote()
                        && (remote
                            .route_roles
                            .iter()
                            .any(|role| role == "planner" || role == "all")
                            || (remote.overload_fallback
                                && matches!(
                                    overload_state,
                                    OverloadState::Degraded | OverloadState::Overloaded
                                ))) =>
                {
                    PlannerBackendSelection::Remote(remote)
                }
                _ => PlannerBackendSelection::Local,
            },
        };

        let lease = TurnLease {
            scheduler: Arc::clone(self),
            _permit: permit,
            priority,
        };

        Ok((lease, engine, load_duration_ms, queue_wait_ms, decision))
    }

    pub async fn acquire_aux_model(
        self: &Arc<Self>,
        model_name: &str,
        model_path: PathBuf,
        params: LlamaEngineParams,
        estimated_bytes: u64,
    ) -> Result<(rustfin_ai_agent::LlamaEngine, u64), String> {
        self.load_or_reuse_model(model_name, model_path, params, estimated_bytes)
            .await
    }

    pub fn snapshot(&self) -> SchedulerSnapshot {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        SchedulerSnapshot {
            max_concurrent_turns: self.semaphore.available_permits() as u64 + state.active_turns,
            queue_limit: self.queue_limit,
            active_turns: state.active_turns,
            queued_turns: state.queued_turns,
            overload_state: state.last_overload_state.as_str().to_string(),
            warm_pool_bytes: state.warm_pool_bytes,
            warm_pool_budget_bytes: self.warm_pool_budget_bytes,
            active_by_priority: priority_counts(&state.active_by_priority),
            queued_by_priority: priority_counts(&state.queued_by_priority),
            warm_models: state
                .warm_model_order
                .iter()
                .filter_map(|model_name| state.warm_models.get(model_name))
                .map(|entry| WarmModelSnapshot {
                    model_name: entry.model_name.clone(),
                    estimated_bytes: entry.estimated_bytes,
                    loaded_ts_ms: entry.loaded_ts_ms,
                    last_used_ts_ms: entry.last_used_ts_ms,
                    load_count: entry.load_count,
                })
                .collect(),
            rejected_turns_total: state.rejected_turns_total,
            degraded_turns_total: state.degraded_turns_total,
        }
    }

    fn finish_turn(&self, priority: TurnPriority) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active_turns = state.active_turns.saturating_sub(1);
        if let Some(count) = state.active_by_priority.get_mut(&priority) {
            *count = count.saturating_sub(1);
        }
    }

    fn compute_overload_state(&self) -> OverloadState {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let warm_ratio = if self.warm_pool_budget_bytes == 0 {
            0.0
        } else {
            state.warm_pool_bytes as f64 / self.warm_pool_budget_bytes as f64
        };
        let active = state.active_turns;
        let queued = state.queued_turns;
        let overload = if queued > self.queue_limit / 2 || warm_ratio > 0.95 {
            OverloadState::Overloaded
        } else if queued > 2 || active > 2 || warm_ratio > 0.8 {
            OverloadState::Degraded
        } else if active > 1 || warm_ratio > 0.6 {
            OverloadState::Constrained
        } else {
            OverloadState::Normal
        };
        if matches!(
            overload,
            OverloadState::Degraded | OverloadState::Overloaded
        ) {
            state.degraded_turns_total = state.degraded_turns_total.saturating_add(1);
        }
        state.last_overload_state = overload;
        overload
    }

    async fn load_or_reuse_model(
        self: &Arc<Self>,
        model_name: &str,
        model_path: PathBuf,
        params: LlamaEngineParams,
        estimated_bytes: u64,
    ) -> Result<(rustfin_ai_agent::LlamaEngine, u64), String> {
        if let Some(engine) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = state.warm_models.get_mut(model_name) {
                entry.last_used_ts_ms = now_ms();
                entry.load_count = entry.load_count.saturating_add(1);
                let engine = entry.engine.clone();
                touch_warm_model_order(&mut state.warm_model_order, model_name);
                Some(engine)
            } else {
                None
            }
        } {
            return Ok((engine, 0));
        }

        let model_name_owned = model_name.to_string();
        let load_started = Instant::now();
        let model_path_for_load = model_path.clone();
        let params_for_load = params.clone();
        let engine = tokio::task::spawn_blocking(move || {
            rustfin_ai_agent::LlamaEngine::load(&model_path_for_load, params_for_load)
        })
        .await
        .map_err(|error| format!("failed to join model load task: {error}"))?
        .map_err(|error| error.to_string())?;
        let load_duration_ms = load_started.elapsed().as_millis() as u64;

        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            evict_until_fits(&mut state, self.warm_pool_budget_bytes, estimated_bytes);
            state.warm_pool_bytes = state.warm_pool_bytes.saturating_add(estimated_bytes);
            state.warm_models.insert(
                model_name_owned.clone(),
                WarmModelEntry {
                    model_name: model_name_owned.clone(),
                    engine: engine.clone(),
                    estimated_bytes,
                    loaded_ts_ms: now_ms(),
                    last_used_ts_ms: now_ms(),
                    load_count: 1,
                },
            );
            touch_warm_model_order(&mut state.warm_model_order, &model_name_owned);
        }

        Ok((engine, load_duration_ms))
    }
}

fn priority_counts(map: &HashMap<TurnPriority, u64>) -> Vec<PriorityCountSnapshot> {
    let mut out = map
        .iter()
        .map(|(priority, count)| PriorityCountSnapshot {
            priority: priority.as_str().to_string(),
            count: *count,
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.priority.cmp(&right.priority));
    out
}

fn touch_warm_model_order(order: &mut VecDeque<String>, model_name: &str) {
    if let Some(index) = order.iter().position(|name| name == model_name) {
        order.remove(index);
    }
    order.push_back(model_name.to_string());
}

fn evict_until_fits(state: &mut SchedulerState, budget_bytes: u64, incoming_bytes: u64) {
    if budget_bytes == 0 {
        return;
    }
    while state.warm_pool_bytes.saturating_add(incoming_bytes) > budget_bytes {
        let Some(oldest) = state.warm_model_order.pop_front() else {
            break;
        };
        if let Some(entry) = state.warm_models.remove(&oldest) {
            state.warm_pool_bytes = state.warm_pool_bytes.saturating_sub(entry.estimated_bytes);
            warn!(
                model = %oldest,
                estimated_bytes = entry.estimated_bytes,
                "evicted warm AI model to respect warm pool budget"
            );
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn default_warm_pool_budget_bytes(total_memory_bytes: u64) -> u64 {
    (total_memory_bytes / 4).max(1024 * 1024 * 1024)
}

fn normalize_sysinfo_memory_bytes(raw_bytes: u64) -> u64 {
    raw_bytes
}

fn total_system_memory_bytes() -> Option<u64> {
    let mut system = sysinfo::System::new_all();
    system.refresh_memory();
    Some(normalize_sysinfo_memory_bytes(system.total_memory()))
}

fn parse_remote_backend_config() -> Option<AiRemoteBackendConfig> {
    let base_url = std::env::var("RUSTFIN_AI_REMOTE_BACKEND_URL").ok()?;
    let model = std::env::var("RUSTFIN_AI_REMOTE_BACKEND_MODEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let api_key_env = std::env::var("RUSTFIN_AI_REMOTE_BACKEND_API_KEY_ENV")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let timeout_secs = std::env::var("RUSTFIN_AI_REMOTE_BACKEND_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(120);
    let supports_prompt_cache = std::env::var("RUSTFIN_AI_REMOTE_PROMPT_CACHE")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);
    let supports_structured_output = std::env::var("RUSTFIN_AI_REMOTE_STRUCTURED_OUTPUT")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true);
    let max_parallel_requests = std::env::var("RUSTFIN_AI_REMOTE_MAX_PARALLEL_REQUESTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);

    Some(AiRemoteBackendConfig {
        enabled: true,
        base_url,
        model,
        api_key_env,
        timeout_secs,
        supports_prompt_cache,
        supports_structured_output,
        max_parallel_requests,
        overload_fallback: true,
        route_roles: vec!["planner".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_starts_empty() {
        let scheduler = TurnScheduler::new();
        let snapshot = scheduler.snapshot();
        assert_eq!(snapshot.active_turns, 0);
        assert_eq!(snapshot.queued_turns, 0);
        assert_eq!(snapshot.warm_pool_bytes, 0);
        assert_eq!(snapshot.overload_state, OverloadState::Normal.as_str());
        assert!(snapshot.warm_models.is_empty());
    }

    #[test]
    fn default_warm_pool_budget_uses_quarter_of_total_memory() {
        let total_memory_bytes = 32 * 1024 * 1024 * 1024_u64;
        assert_eq!(
            default_warm_pool_budget_bytes(total_memory_bytes),
            8 * 1024 * 1024 * 1024_u64
        );
    }

    #[test]
    fn normalize_sysinfo_memory_bytes_keeps_byte_values_unchanged() {
        let raw_bytes = 31_300_000_000_u64;
        assert_eq!(normalize_sysinfo_memory_bytes(raw_bytes), raw_bytes);
    }

    #[test]
    fn remote_backend_configuration_round_trips() {
        let scheduler = TurnScheduler::new();
        let config = AiRemoteBackendConfig {
            enabled: true,
            base_url: "https://example.invalid/v1".to_string(),
            model: "test-model".to_string(),
            api_key_env: Some("RUSTFIN_TEST_REMOTE_KEY".to_string()),
            timeout_secs: 30,
            supports_prompt_cache: true,
            supports_structured_output: true,
            max_parallel_requests: 2,
            overload_fallback: true,
            route_roles: vec!["planner".to_string()],
        };

        scheduler.set_remote_backend(Some(config.clone()));
        let loaded = scheduler
            .remote_backend()
            .expect("remote backend should be set");
        assert_eq!(loaded.base_url, config.base_url);
        assert_eq!(loaded.model, config.model);
        assert_eq!(loaded.route_roles, config.route_roles);
    }
}
