use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct TranscodeDiagnosticsResponse {
    pub active_sessions: usize,
    pub created_total: u64,
    pub create_failures_total: u64,
    pub create_failures_last_minute: u64,
    pub create_failures_last_five_minutes: u64,
    pub cleaned_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostLoadAverageSnapshot {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostRuntimeSnapshot {
    pub available: bool,
    pub reason: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub logical_cpu_threads: Option<u64>,
    pub physical_cpu_cores: Option<u64>,
    pub cpu_usage_percent: Option<f64>,
    pub estimated_busy_logical_threads: Option<f64>,
    pub total_memory_bytes: Option<u64>,
    pub used_memory_bytes: Option<u64>,
    pub memory_used_percent: Option<f64>,
    pub total_swap_bytes: Option<u64>,
    pub used_swap_bytes: Option<u64>,
    pub swap_used_percent: Option<f64>,
    pub load_average: Option<HostLoadAverageSnapshot>,
}

impl HostRuntimeSnapshot {
    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
            uptime_seconds: None,
            logical_cpu_threads: None,
            physical_cpu_cores: None,
            cpu_usage_percent: None,
            estimated_busy_logical_threads: None,
            total_memory_bytes: None,
            used_memory_bytes: None,
            memory_used_percent: None,
            total_swap_bytes: None,
            used_swap_bytes: None,
            swap_used_percent: None,
            load_average: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDiagnosticsResponse {
    pub host: HostRuntimeSnapshot,
    pub runtime: crate::runtime_metrics::RuntimeMetricsSnapshot,
    pub transcoding: TranscodeDiagnosticsResponse,
}

pub async fn collect_runtime_diagnostics(state: &AppState) -> RuntimeDiagnosticsResponse {
    let runtime = state.runtime_metrics.snapshot();
    let transcoding = TranscodeDiagnosticsResponse {
        active_sessions: state.transcoder.active_count().await,
        created_total: state.transcoder.created_total(),
        create_failures_total: state.transcoder.create_failures_total(),
        create_failures_last_minute: state.transcoder.create_failures_last_minute(),
        create_failures_last_five_minutes: state.transcoder.create_failures_last_five_minutes(),
        cleaned_total: state.transcoder.cleaned_total(),
    };
    let host = collect_host_runtime_snapshot().await;

    RuntimeDiagnosticsResponse {
        host,
        runtime,
        transcoding,
    }
}

pub async fn collect_host_runtime_snapshot() -> HostRuntimeSnapshot {
    #[cfg(target_os = "linux")]
    {
        match tokio::task::spawn_blocking(collect_linux_host_runtime_snapshot).await {
            Ok(snapshot) => snapshot,
            Err(error) => HostRuntimeSnapshot::unavailable(format!(
                "Failed to collect host runtime stats: {error}"
            )),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        HostRuntimeSnapshot::unavailable("Host runtime stats are only available on Linux hosts.")
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_host_runtime_snapshot() -> HostRuntimeSnapshot {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.refresh_cpu_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_cpu_all();

    let logical_cpu_threads = u64::try_from(system.cpus().len()).ok();
    let physical_cpu_cores = system
        .physical_core_count()
        .and_then(|count| u64::try_from(count).ok());
    let cpu_usage_percent = Some(round_metric(system.global_cpu_usage() as f64));
    let estimated_busy_logical_threads = logical_cpu_threads.zip(cpu_usage_percent).map(
        |(logical_cpu_threads, cpu_usage_percent)| {
            round_metric((cpu_usage_percent / 100.0) * logical_cpu_threads as f64)
        },
    );

    // sysinfo 0.33 reports memory values in bytes already.
    let total_memory_bytes = Some(system.total_memory());
    let used_memory_bytes = Some(system.used_memory());
    let memory_used_percent = percentage(system.used_memory(), system.total_memory());
    let total_swap_bytes = Some(system.total_swap());
    let used_swap_bytes = Some(system.used_swap());
    let swap_used_percent = percentage(system.used_swap(), system.total_swap());

    let load_average = sysinfo::System::load_average();

    HostRuntimeSnapshot {
        available: true,
        reason: None,
        uptime_seconds: Some(sysinfo::System::uptime()),
        logical_cpu_threads,
        physical_cpu_cores,
        cpu_usage_percent,
        estimated_busy_logical_threads,
        total_memory_bytes,
        used_memory_bytes,
        memory_used_percent,
        total_swap_bytes,
        used_swap_bytes,
        swap_used_percent,
        load_average: Some(HostLoadAverageSnapshot {
            one: round_metric(load_average.one),
            five: round_metric(load_average.five),
            fifteen: round_metric(load_average.fifteen),
        }),
    }
}

#[cfg(target_os = "linux")]
fn percentage(used: u64, total: u64) -> Option<f64> {
    if total == 0 {
        None
    } else {
        Some(round_metric((used as f64 / total as f64) * 100.0))
    }
}

#[cfg(target_os = "linux")]
fn round_metric(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{percentage, round_metric};

    #[cfg(target_os = "linux")]
    #[test]
    fn percentage_handles_zero_total() {
        assert_eq!(percentage(12, 0), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn percentage_rounds_to_single_decimal() {
        assert_eq!(percentage(1, 3), Some(33.3));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn round_metric_rounds_single_decimal() {
        assert_eq!(round_metric(12.34), 12.3);
        assert_eq!(round_metric(12.35), 12.4);
    }
}
