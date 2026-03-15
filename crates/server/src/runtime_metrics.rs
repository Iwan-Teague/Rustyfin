use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub enum JobFamily {
    LibraryScan,
    TmdbSync,
    ServerOperation,
    AdminAudit,
    Other,
}

#[derive(Debug, Clone, Copy)]
pub enum AgentKind {
    Servers,
    Tmdb,
    Transcription,
    YouTube,
}

#[derive(Default)]
struct JobCounters {
    enqueued_total: AtomicU64,
    running_total: AtomicU64,
    active_running: AtomicU64,
    completed_total: AtomicU64,
    failed_total: AtomicU64,
    failure_window: FailureWindow,
}

#[derive(Default)]
struct AgentCounters {
    calls_total: AtomicU64,
    calls_succeeded_total: AtomicU64,
    calls_failed_total: AtomicU64,
    calls_in_flight: AtomicU64,
    failure_window: FailureWindow,
}

#[derive(Default)]
struct FailureWindow {
    timestamps: Mutex<VecDeque<Instant>>,
}

#[derive(Default)]
pub struct RuntimeMetrics {
    started_at: std::sync::OnceLock<Instant>,
    jobs_total: JobCounters,
    jobs_library_scan: JobCounters,
    jobs_tmdb_sync: JobCounters,
    jobs_server_operations: JobCounters,
    jobs_admin_audit: JobCounters,
    jobs_other: JobCounters,
    channels_ws_active: AtomicU64,
    channels_ws_connections_total: AtomicU64,
    watch_party_ws_active: AtomicU64,
    watch_party_ws_connections_total: AtomicU64,
    servers_agent: AgentCounters,
    tmdb_agent: AgentCounters,
    transcription_agent: AgentCounters,
    youtube_agent: AgentCounters,
    ai_assistant_chats: AgentCounters,
    ai_assistant_tools: AgentCounters,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobCountersSnapshot {
    pub enqueued_total: u64,
    pub running_total: u64,
    pub active_running: u64,
    pub completed_total: u64,
    pub failed_total: u64,
    pub failures_last_minute: u64,
    pub failures_last_five_minutes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobsSnapshot {
    pub total: JobCountersSnapshot,
    pub library_scan: JobCountersSnapshot,
    pub tmdb_sync: JobCountersSnapshot,
    pub server_operations: JobCountersSnapshot,
    pub admin_audit: JobCountersSnapshot,
    pub other: JobCountersSnapshot,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebSocketSnapshot {
    pub active: u64,
    pub connections_total: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WebSocketsSnapshot {
    pub channels: WebSocketSnapshot,
    pub watch_party: WebSocketSnapshot,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentSnapshot {
    pub calls_total: u64,
    pub calls_succeeded_total: u64,
    pub calls_failed_total: u64,
    pub calls_in_flight: u64,
    pub failures_last_minute: u64,
    pub failures_last_five_minutes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentsSnapshot {
    pub servers: AgentSnapshot,
    pub tmdb: AgentSnapshot,
    pub transcription: AgentSnapshot,
    pub youtube: AgentSnapshot,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AssistantSnapshot {
    pub chats: AgentSnapshot,
    pub tools: AgentSnapshot,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeMetricsSnapshot {
    pub uptime_seconds: u64,
    pub jobs: JobsSnapshot,
    pub websockets: WebSocketsSnapshot,
    pub agents: AgentsSnapshot,
    pub assistant: AssistantSnapshot,
}

pub struct ActiveWebSocketGuard {
    metrics: Arc<RuntimeMetrics>,
    kind: WebSocketKind,
}

pub struct AgentCallGuard {
    metrics: Arc<RuntimeMetrics>,
    kind: AgentKind,
    succeeded: AtomicBool,
}

pub struct AssistantCallGuard {
    metrics: Arc<RuntimeMetrics>,
    kind: AssistantCallKind,
    succeeded: AtomicBool,
}

#[derive(Debug, Clone, Copy)]
enum WebSocketKind {
    Channels,
    WatchParty,
}

#[derive(Debug, Clone, Copy)]
enum AssistantCallKind {
    Chat,
    Tool,
}

impl RuntimeMetrics {
    pub fn new() -> Arc<Self> {
        let metrics = Arc::new(Self::default());
        let _ = metrics.started_at.set(Instant::now());
        metrics
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.started_at
            .get()
            .map(|started| started.elapsed().as_secs())
            .unwrap_or(0)
    }

    pub fn record_job_enqueued(&self, family: JobFamily) {
        increment_job_counter(&self.jobs_total.enqueued_total);
        increment_job_counter(&job_counters(self, family).enqueued_total);
    }

    pub fn record_job_running(&self, family: JobFamily) {
        increment_job_counter(&self.jobs_total.running_total);
        increment_job_counter(&self.jobs_total.active_running);
        let counters = job_counters(self, family);
        increment_job_counter(&counters.running_total);
        increment_job_counter(&counters.active_running);
    }

    pub fn record_job_completed(&self, family: JobFamily) {
        increment_job_counter(&self.jobs_total.completed_total);
        decrement_job_counter(&self.jobs_total.active_running);
        let counters = job_counters(self, family);
        increment_job_counter(&counters.completed_total);
        decrement_job_counter(&counters.active_running);
    }

    pub fn record_job_failed(&self, family: JobFamily) {
        increment_job_counter(&self.jobs_total.failed_total);
        decrement_job_counter(&self.jobs_total.active_running);
        self.jobs_total.failure_window.record_failure();
        let counters = job_counters(self, family);
        increment_job_counter(&counters.failed_total);
        decrement_job_counter(&counters.active_running);
        counters.failure_window.record_failure();
    }

    pub fn track_channels_ws_connection(self: &Arc<Self>) -> ActiveWebSocketGuard {
        increment_job_counter(&self.channels_ws_connections_total);
        increment_job_counter(&self.channels_ws_active);
        ActiveWebSocketGuard {
            metrics: Arc::clone(self),
            kind: WebSocketKind::Channels,
        }
    }

    pub fn track_watch_party_ws_connection(self: &Arc<Self>) -> ActiveWebSocketGuard {
        increment_job_counter(&self.watch_party_ws_connections_total);
        increment_job_counter(&self.watch_party_ws_active);
        ActiveWebSocketGuard {
            metrics: Arc::clone(self),
            kind: WebSocketKind::WatchParty,
        }
    }

    pub fn start_agent_call(self: &Arc<Self>, kind: AgentKind) -> AgentCallGuard {
        let counters = agent_counters(self, kind);
        increment_job_counter(&counters.calls_total);
        increment_job_counter(&counters.calls_in_flight);
        AgentCallGuard {
            metrics: Arc::clone(self),
            kind,
            succeeded: AtomicBool::new(false),
        }
    }

    pub fn start_ai_chat_request(self: &Arc<Self>) -> AssistantCallGuard {
        let counters = assistant_counters(self, AssistantCallKind::Chat);
        increment_job_counter(&counters.calls_total);
        increment_job_counter(&counters.calls_in_flight);
        AssistantCallGuard {
            metrics: Arc::clone(self),
            kind: AssistantCallKind::Chat,
            succeeded: AtomicBool::new(false),
        }
    }

    pub fn start_ai_tool_call(self: &Arc<Self>) -> AssistantCallGuard {
        let counters = assistant_counters(self, AssistantCallKind::Tool);
        increment_job_counter(&counters.calls_total);
        increment_job_counter(&counters.calls_in_flight);
        AssistantCallGuard {
            metrics: Arc::clone(self),
            kind: AssistantCallKind::Tool,
            succeeded: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        RuntimeMetricsSnapshot {
            uptime_seconds: self.uptime_seconds(),
            jobs: JobsSnapshot {
                total: snapshot_job_counters(&self.jobs_total),
                library_scan: snapshot_job_counters(&self.jobs_library_scan),
                tmdb_sync: snapshot_job_counters(&self.jobs_tmdb_sync),
                server_operations: snapshot_job_counters(&self.jobs_server_operations),
                admin_audit: snapshot_job_counters(&self.jobs_admin_audit),
                other: snapshot_job_counters(&self.jobs_other),
            },
            websockets: WebSocketsSnapshot {
                channels: WebSocketSnapshot {
                    active: self.channels_ws_active.load(Ordering::Relaxed),
                    connections_total: self.channels_ws_connections_total.load(Ordering::Relaxed),
                },
                watch_party: WebSocketSnapshot {
                    active: self.watch_party_ws_active.load(Ordering::Relaxed),
                    connections_total: self
                        .watch_party_ws_connections_total
                        .load(Ordering::Relaxed),
                },
            },
            agents: AgentsSnapshot {
                servers: snapshot_agent_counters(&self.servers_agent),
                tmdb: snapshot_agent_counters(&self.tmdb_agent),
                transcription: snapshot_agent_counters(&self.transcription_agent),
                youtube: snapshot_agent_counters(&self.youtube_agent),
            },
            assistant: AssistantSnapshot {
                chats: snapshot_agent_counters(&self.ai_assistant_chats),
                tools: snapshot_agent_counters(&self.ai_assistant_tools),
            },
        }
    }
}

impl AgentCallGuard {
    pub fn mark_success(&self) {
        self.succeeded.store(true, Ordering::Relaxed);
    }
}

impl AssistantCallGuard {
    pub fn mark_success(&self) {
        self.succeeded.store(true, Ordering::Relaxed);
    }
}

impl Drop for ActiveWebSocketGuard {
    fn drop(&mut self) {
        match self.kind {
            WebSocketKind::Channels => decrement_job_counter(&self.metrics.channels_ws_active),
            WebSocketKind::WatchParty => decrement_job_counter(&self.metrics.watch_party_ws_active),
        }
    }
}

impl Drop for AgentCallGuard {
    fn drop(&mut self) {
        let counters = agent_counters(&self.metrics, self.kind);
        decrement_job_counter(&counters.calls_in_flight);
        if self.succeeded.load(Ordering::Relaxed) {
            increment_job_counter(&counters.calls_succeeded_total);
        } else {
            increment_job_counter(&counters.calls_failed_total);
            counters.failure_window.record_failure();
        }
    }
}

impl Drop for AssistantCallGuard {
    fn drop(&mut self) {
        let counters = assistant_counters(&self.metrics, self.kind);
        decrement_job_counter(&counters.calls_in_flight);
        if self.succeeded.load(Ordering::Relaxed) {
            increment_job_counter(&counters.calls_succeeded_total);
        } else {
            increment_job_counter(&counters.calls_failed_total);
            counters.failure_window.record_failure();
        }
    }
}

fn increment_job_counter(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

fn decrement_job_counter(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_sub(1))
    });
}

fn job_counters(metrics: &RuntimeMetrics, family: JobFamily) -> &JobCounters {
    match family {
        JobFamily::LibraryScan => &metrics.jobs_library_scan,
        JobFamily::TmdbSync => &metrics.jobs_tmdb_sync,
        JobFamily::ServerOperation => &metrics.jobs_server_operations,
        JobFamily::AdminAudit => &metrics.jobs_admin_audit,
        JobFamily::Other => &metrics.jobs_other,
    }
}

fn agent_counters(metrics: &RuntimeMetrics, kind: AgentKind) -> &AgentCounters {
    match kind {
        AgentKind::Servers => &metrics.servers_agent,
        AgentKind::Tmdb => &metrics.tmdb_agent,
        AgentKind::Transcription => &metrics.transcription_agent,
        AgentKind::YouTube => &metrics.youtube_agent,
    }
}

fn assistant_counters(metrics: &RuntimeMetrics, kind: AssistantCallKind) -> &AgentCounters {
    match kind {
        AssistantCallKind::Chat => &metrics.ai_assistant_chats,
        AssistantCallKind::Tool => &metrics.ai_assistant_tools,
    }
}

fn snapshot_job_counters(counters: &JobCounters) -> JobCountersSnapshot {
    let failure_window = counters.failure_window.snapshot();
    JobCountersSnapshot {
        enqueued_total: counters.enqueued_total.load(Ordering::Relaxed),
        running_total: counters.running_total.load(Ordering::Relaxed),
        active_running: counters.active_running.load(Ordering::Relaxed),
        completed_total: counters.completed_total.load(Ordering::Relaxed),
        failed_total: counters.failed_total.load(Ordering::Relaxed),
        failures_last_minute: failure_window.last_minute,
        failures_last_five_minutes: failure_window.last_five_minutes,
    }
}

fn snapshot_agent_counters(counters: &AgentCounters) -> AgentSnapshot {
    let failure_window = counters.failure_window.snapshot();
    AgentSnapshot {
        calls_total: counters.calls_total.load(Ordering::Relaxed),
        calls_succeeded_total: counters.calls_succeeded_total.load(Ordering::Relaxed),
        calls_failed_total: counters.calls_failed_total.load(Ordering::Relaxed),
        calls_in_flight: counters.calls_in_flight.load(Ordering::Relaxed),
        failures_last_minute: failure_window.last_minute,
        failures_last_five_minutes: failure_window.last_five_minutes,
    }
}

impl FailureWindow {
    fn record_failure(&self) {
        const MAX_WINDOW_SECONDS: u64 = 5 * 60;
        let now = Instant::now();
        let mut timestamps = lock_or_recover(&self.timestamps);
        timestamps.push_back(now);
        while let Some(front) = timestamps.front() {
            if now.duration_since(*front).as_secs() > MAX_WINDOW_SECONDS {
                timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn snapshot(&self) -> FailureWindowSnapshot {
        let now = Instant::now();
        let mut timestamps = lock_or_recover(&self.timestamps);
        while let Some(front) = timestamps.front() {
            if now.duration_since(*front).as_secs() > 5 * 60 {
                timestamps.pop_front();
            } else {
                break;
            }
        }

        let mut last_minute = 0;
        let mut last_five_minutes = 0;
        for timestamp in timestamps.iter() {
            let elapsed = now.duration_since(*timestamp).as_secs();
            if elapsed <= 5 * 60 {
                last_five_minutes += 1;
            }
            if elapsed <= 60 {
                last_minute += 1;
            }
        }

        FailureWindowSnapshot {
            last_minute,
            last_five_minutes,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FailureWindowSnapshot {
    last_minute: u64,
    last_five_minutes: u64,
}

fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, JobFamily, RuntimeMetrics};

    #[test]
    fn tracks_job_counters_by_family() {
        let metrics = RuntimeMetrics::new();

        metrics.record_job_enqueued(JobFamily::LibraryScan);
        metrics.record_job_running(JobFamily::LibraryScan);
        metrics.record_job_completed(JobFamily::LibraryScan);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.jobs.total.enqueued_total, 1);
        assert_eq!(snapshot.jobs.total.running_total, 1);
        assert_eq!(snapshot.jobs.total.completed_total, 1);
        assert_eq!(snapshot.jobs.total.active_running, 0);
        assert_eq!(snapshot.jobs.library_scan.completed_total, 1);
    }

    #[test]
    fn websocket_guard_updates_active_counts() {
        let metrics = RuntimeMetrics::new();
        {
            let _guard = metrics.track_channels_ws_connection();
            let snapshot = metrics.snapshot();
            assert_eq!(snapshot.websockets.channels.active, 1);
            assert_eq!(snapshot.websockets.channels.connections_total, 1);
        }
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.websockets.channels.active, 0);
    }

    #[test]
    fn agent_call_guard_tracks_outcomes() {
        let metrics = RuntimeMetrics::new();
        {
            let guard = metrics.start_agent_call(AgentKind::Servers);
            guard.mark_success();
        }
        {
            let _guard = metrics.start_agent_call(AgentKind::Tmdb);
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.agents.servers.calls_total, 1);
        assert_eq!(snapshot.agents.servers.calls_succeeded_total, 1);
        assert_eq!(snapshot.agents.servers.calls_failed_total, 0);
        assert_eq!(snapshot.agents.tmdb.calls_failed_total, 1);
    }

    #[test]
    fn job_failure_windows_track_recent_failures() {
        let metrics = RuntimeMetrics::new();

        metrics.record_job_running(JobFamily::LibraryScan);
        metrics.record_job_failed(JobFamily::LibraryScan);
        metrics.record_job_running(JobFamily::LibraryScan);
        metrics.record_job_failed(JobFamily::LibraryScan);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.jobs.total.failed_total, 2);
        assert_eq!(snapshot.jobs.total.failures_last_minute, 2);
        assert_eq!(snapshot.jobs.total.failures_last_five_minutes, 2);
        assert_eq!(snapshot.jobs.library_scan.failures_last_minute, 2);
        assert_eq!(snapshot.jobs.library_scan.failures_last_five_minutes, 2);
    }

    #[test]
    fn agent_failure_windows_track_recent_failures() {
        let metrics = RuntimeMetrics::new();
        {
            let _guard = metrics.start_agent_call(AgentKind::YouTube);
        }
        {
            let _guard = metrics.start_agent_call(AgentKind::YouTube);
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.agents.youtube.calls_failed_total, 2);
        assert_eq!(snapshot.agents.youtube.failures_last_minute, 2);
        assert_eq!(snapshot.agents.youtube.failures_last_five_minutes, 2);
    }

    #[test]
    fn assistant_call_guards_track_chat_and_tool_outcomes() {
        let metrics = RuntimeMetrics::new();
        {
            let guard = metrics.start_ai_chat_request();
            guard.mark_success();
        }
        {
            let _guard = metrics.start_ai_tool_call();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.assistant.chats.calls_total, 1);
        assert_eq!(snapshot.assistant.chats.calls_succeeded_total, 1);
        assert_eq!(snapshot.assistant.chats.calls_failed_total, 0);
        assert_eq!(snapshot.assistant.tools.calls_total, 1);
        assert_eq!(snapshot.assistant.tools.calls_failed_total, 1);
    }
}
