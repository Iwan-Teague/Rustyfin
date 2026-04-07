use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::Utc;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::timeout;

const COMMAND_TIMEOUT_SECS: u64 = 4;
const SLOW_COMMAND_TIMEOUT_SECS: u64 = 6;
const MAX_LIST_ITEMS: usize = 128;

macro_rules! linux_tool_wrapper {
    ($name:ident, $linux_fn:ident, $label:expr) => {
        pub async fn $name() -> Result<(String, Value), String> {
            #[cfg(target_os = "linux")]
            {
                $linux_fn().await
            }

            #[cfg(not(target_os = "linux"))]
            {
                Ok(unavailable_result(
                    $label,
                    "This diagnostics tool is only available on Linux hosts.",
                ))
            }
        }
    };
}

linux_tool_wrapper!(
    system_get_kernel_info,
    collect_linux_kernel_info,
    "Rustyfin host kernel and OS information"
);
linux_tool_wrapper!(
    system_get_cpu_topology,
    collect_linux_cpu_topology,
    "Rustyfin host CPU topology"
);
linux_tool_wrapper!(
    system_get_temperature_sensors,
    collect_linux_temperature_sensors,
    "Rustyfin host temperature sensors"
);
linux_tool_wrapper!(
    system_get_block_device_inventory,
    collect_linux_block_device_inventory,
    "Rustyfin host block device inventory"
);
linux_tool_wrapper!(
    system_get_filesystem_table,
    collect_linux_filesystem_table,
    "Rustyfin host filesystem table"
);
linux_tool_wrapper!(
    system_get_gpu_inventory,
    collect_linux_gpu_inventory,
    "Rustyfin host GPU inventory"
);
linux_tool_wrapper!(
    system_get_pci_devices,
    collect_linux_pci_devices,
    "Rustyfin host PCI device inventory"
);
linux_tool_wrapper!(
    system_get_usb_devices,
    collect_linux_usb_devices,
    "Rustyfin host USB device inventory"
);
linux_tool_wrapper!(
    system_get_boot_log_summary,
    collect_linux_boot_log_summary,
    "Rustyfin host boot log summary"
);
linux_tool_wrapper!(
    system_get_journal_summary,
    collect_linux_journal_summary,
    "Rustyfin host journal summary"
);
linux_tool_wrapper!(
    network_get_route_table,
    collect_linux_route_table,
    "Rustyfin host route table"
);
linux_tool_wrapper!(
    network_get_active_connections,
    collect_linux_active_connections,
    "Rustyfin host active connections"
);
linux_tool_wrapper!(
    network_get_interface_counters,
    collect_linux_interface_counters,
    "Rustyfin host interface counters"
);
linux_tool_wrapper!(
    network_get_wifi_status,
    collect_linux_wifi_status,
    "Rustyfin host Wi-Fi status"
);
linux_tool_wrapper!(
    network_get_vpn_status,
    collect_linux_vpn_status,
    "Rustyfin host VPN status"
);
pub async fn system_get_process_detail(_query: &str) -> Result<(String, Value), String> {
    #[cfg(target_os = "linux")]
    {
        collect_linux_process_detail(query).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(unavailable_result(
            "Rustyfin host process detail",
            "This diagnostics tool is only available on Linux hosts.",
        ))
    }
}

pub async fn system_get_listener_detail(_query: &str) -> Result<(String, Value), String> {
    #[cfg(target_os = "linux")]
    {
        collect_linux_listener_detail(query).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(unavailable_result(
            "Rustyfin host listener detail",
            "This diagnostics tool is only available on Linux hosts.",
        ))
    }
}

pub async fn system_get_disk_usage_detail(_query: &str) -> Result<(String, Value), String> {
    #[cfg(target_os = "linux")]
    {
        collect_linux_disk_usage_detail(query).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(unavailable_result(
            "Rustyfin host disk usage detail",
            "This diagnostics tool is only available on Linux hosts.",
        ))
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn unavailable_result(label: &str, reason: impl Into<String>) -> (String, Value) {
    (
        label.to_string(),
        json!({
            "available": false,
            "observed_at": now_rfc3339(),
            "reason": reason.into(),
        }),
    )
}

fn available_result(label: &str, data: Value) -> (String, Value) {
    (
        label.to_string(),
        json!({
            "available": true,
            "observed_at": now_rfc3339(),
            "data": data,
        }),
    )
}

async fn run_linux_command(
    program: &str,
    args: &[&str],
    timeout_secs: u64,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let child = command
        .spawn()
        .map_err(|error| format!("failed to spawn {program}: {error}"))?;

    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| format!("{program} timed out after {timeout_secs}s"))?
        .map_err(|error| format!("failed to wait for {program}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("{program} exited with status {}", output.status)
        } else {
            format!("{program} exited with status {}: {stderr}", output.status)
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn trim_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn limited<T>(items: Vec<T>) -> Vec<T> {
    if items.len() > MAX_LIST_ITEMS {
        items.into_iter().take(MAX_LIST_ITEMS).collect()
    } else {
        items
    }
}

fn parse_kv_output(output: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if !key.is_empty() && !value.is_empty() {
            values.insert(key.to_string(), value.to_string());
        }
    }
    values
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_os_release() -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let Ok(raw) = std::fs::read_to_string("/etc/os-release") else {
        return values;
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        values.insert(key.trim().to_string(), value);
    }

    values
}

fn parse_cpuinfo() -> Value {
    let raw = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let mut logical_cpu_count = 0u64;
    let mut physical_pairs = BTreeSet::new();
    let mut vendor_id = None::<String>;
    let mut model_name = None::<String>;
    let mut cpu_mhz = None::<String>;
    let mut current_physical_id = None::<String>;
    let mut current_core_id = None::<String>;

    for line in raw.lines() {
        if line.starts_with("processor\t:") || line.starts_with("processor :") {
            logical_cpu_count += 1;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "vendor_id" if vendor_id.is_none() => vendor_id = Some(value.to_string()),
                "model name" if model_name.is_none() => model_name = Some(value.to_string()),
                "cpu MHz" if cpu_mhz.is_none() => cpu_mhz = Some(value.to_string()),
                "physical id" => current_physical_id = Some(value.to_string()),
                "core id" => current_core_id = Some(value.to_string()),
                _ => {}
            }
            if current_physical_id.is_some() && current_core_id.is_some() {
                physical_pairs.insert((
                    current_physical_id.clone().unwrap_or_default(),
                    current_core_id.clone().unwrap_or_default(),
                ));
                current_core_id = None;
            }
        }
    }

    json!({
        "logical_cpu_count": logical_cpu_count,
        "physical_core_count": physical_pairs.len(),
        "vendor_id": vendor_id,
        "model_name": model_name,
        "cpu_mhz": cpu_mhz,
    })
}

fn decode_mountinfo_field(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let mut digits = String::with_capacity(3);
            for _ in 0..3 {
                if let Some(next) = chars.next() {
                    digits.push(next);
                }
            }
            if digits.len() == 3
                && digits.chars().all(|digit| digit.is_ascii_digit())
                && let Ok(value) = u8::from_str_radix(&digits, 8)
            {
                decoded.push(value as char);
                continue;
            }
            decoded.push('\\');
            decoded.push_str(&digits);
        } else {
            decoded.push(ch);
        }
    }
    decoded
}

#[derive(Debug, Clone)]
struct MountInfoEntry {
    mount_id: u64,
    parent_id: u64,
    major_minor: String,
    root: String,
    mount_point: String,
    options: String,
    fs_type: String,
    source: String,
    super_options: String,
}

fn parse_mountinfo() -> Vec<MountInfoEntry> {
    let raw = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    raw.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let mut left_parts = left.split_whitespace();
            let mount_id = left_parts.next()?.parse::<u64>().ok()?;
            let parent_id = left_parts.next()?.parse::<u64>().ok()?;
            let major_minor = left_parts.next()?.to_string();
            let root = decode_mountinfo_field(left_parts.next()?);
            let mount_point = decode_mountinfo_field(left_parts.next()?);
            let options = left_parts.next()?.to_string();

            let mut right_parts = right.split_whitespace();
            let fs_type = right_parts.next()?.to_string();
            let source = right_parts
                .next()
                .map(decode_mountinfo_field)
                .unwrap_or_default();
            let super_options = right_parts.collect::<Vec<_>>().join(" ");

            Some(MountInfoEntry {
                mount_id,
                parent_id,
                major_minor,
                root,
                mount_point,
                options,
                fs_type,
                source,
                super_options,
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct StatvfsSnapshot {
    total_bytes: u64,
    free_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
    used_percent: Option<f64>,
}

fn statvfs_snapshot(path: &Path) -> Option<StatvfsSnapshot> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let block_size = if stat.f_frsize > 0 {
        stat.f_frsize as u64
    } else {
        stat.f_bsize as u64
    };
    let total_blocks = stat.f_blocks as u64;
    let free_blocks = stat.f_bfree as u64;
    let avail_blocks = stat.f_bavail as u64;
    let total_bytes = total_blocks.saturating_mul(block_size);
    let free_bytes = free_blocks.saturating_mul(block_size);
    let available_bytes = avail_blocks.saturating_mul(block_size);
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    let used_percent = if total_bytes == 0 {
        None
    } else {
        Some((used_bytes as f64 / total_bytes as f64) * 100.0)
    };

    Some(StatvfsSnapshot {
        total_bytes,
        free_bytes,
        available_bytes,
        used_bytes,
        used_percent,
    })
}

async fn collect_linux_kernel_info() -> Result<(String, Value), String> {
    let uname = run_linux_command("uname", &["-srmo"], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    let kernel_release = run_linux_command("uname", &["-r"], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    let machine = run_linux_command("uname", &["-m"], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    let hostname = read_trimmed("/etc/hostname")
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty());
    let os_release = read_os_release();
    let proc_version = read_trimmed("/proc/version");
    let observed_at = now_rfc3339();

    let available = uname.is_some() || kernel_release.is_some() || proc_version.is_some();
    let label = "Rustyfin host kernel and OS information";
    let data = json!({
        "available": available,
        "observed_at": observed_at,
        "hostname": hostname,
        "kernel_release": kernel_release,
        "machine": machine,
        "uname": uname,
        "proc_version": proc_version,
        "os_release": os_release,
    });

    if available {
        Ok(available_result(label, data))
    } else {
        Ok(unavailable_result(
            label,
            "Unable to collect kernel or OS details from the host.",
        ))
    }
}

async fn collect_linux_cpu_topology() -> Result<(String, Value), String> {
    let summary = parse_cpuinfo();
    let lscpu_text = run_linux_command("lscpu", &[], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    let mut key_values = BTreeMap::<String, String>::new();
    let mut cpu_rows = Vec::<Value>::new();

    if let Some(text) = lscpu_text.as_deref() {
        key_values = parse_kv_output(text);
    }

    if let Ok(rows) = run_linux_command(
        "lscpu",
        &["-p=CPU,CORE,SOCKET,NODE,ONLINE"],
        COMMAND_TIMEOUT_SECS,
    )
    .await
    {
        for line in rows.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split(',');
            let cpu = fields.next().map(str::trim).unwrap_or_default();
            let core = fields.next().map(str::trim).unwrap_or_default();
            let socket = fields.next().map(str::trim).unwrap_or_default();
            let node = fields.next().map(str::trim).unwrap_or_default();
            let online = fields.next().map(str::trim).unwrap_or_default();
            cpu_rows.push(json!({
                "cpu": cpu,
                "core": core,
                "socket": socket,
                "node": node,
                "online": online == "Y" || online == "1",
            }));
        }
    }
    cpu_rows = limited(cpu_rows);

    let logical_cpu_count = key_values
        .get("CPU(s)")
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| summary.get("logical_cpu_count").and_then(Value::as_u64));
    let physical_core_count = key_values
        .get("Core(s) per socket")
        .and_then(|value| value.parse::<u64>().ok())
        .zip(
            key_values
                .get("Socket(s)")
                .and_then(|value| value.parse::<u64>().ok()),
        )
        .map(|(cores_per_socket, sockets)| cores_per_socket.saturating_mul(sockets))
        .or_else(|| summary.get("physical_core_count").and_then(Value::as_u64));

    let label = "Rustyfin host CPU topology";
    let data = json!({
        "available": true,
        "observed_at": now_rfc3339(),
        "lscpu": key_values,
        "logical_cpu_count": logical_cpu_count,
        "physical_core_count": physical_core_count,
        "cpu_rows": cpu_rows,
        "cpuinfo_fallback": summary,
    });

    Ok(available_result(label, data))
}

async fn collect_linux_temperature_sensors() -> Result<(String, Value), String> {
    let mut sensors = Vec::<Value>::new();

    if let Ok(entries) = std::fs::read_dir("/sys/class/hwmon") {
        for hwmon in entries.flatten().take(32) {
            let path = hwmon.path();
            let name = read_trimmed(path.join("name")).unwrap_or_else(|| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("hwmon")
                    .to_string()
            });
            for temp_index in 1..=16 {
                let input_path = path.join(format!("temp{temp_index}_input"));
                if !input_path.exists() {
                    continue;
                }
                let current_c = read_trimmed(&input_path)
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|millidegrees| (millidegrees / 1000.0 * 10.0).round() / 10.0);
                let label = read_trimmed(path.join(format!("temp{temp_index}_label")))
                    .unwrap_or_else(|| format!("{name} sensor {temp_index}"));
                let max_c = read_trimmed(path.join(format!("temp{temp_index}_max")))
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|millidegrees| (millidegrees / 1000.0 * 10.0).round() / 10.0);
                let critical_c = read_trimmed(path.join(format!("temp{temp_index}_crit")))
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|millidegrees| (millidegrees / 1000.0 * 10.0).round() / 10.0);
                if let Some(current_c) = current_c {
                    sensors.push(json!({
                        "source": "hwmon",
                        "device": name,
                        "label": label,
                        "current_c": current_c,
                        "max_c": max_c,
                        "critical_c": critical_c,
                    }));
                }
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir("/sys/class/thermal") {
        for zone in entries.flatten().take(64) {
            let path = zone.path();
            let zone_type = read_trimmed(path.join("type")).unwrap_or_else(|| {
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("thermal_zone")
                    .to_string()
            });
            let current_c = read_trimmed(path.join("temp"))
                .and_then(|value| value.parse::<f64>().ok())
                .map(|millidegrees| (millidegrees / 1000.0 * 10.0).round() / 10.0);
            if let Some(current_c) = current_c {
                sensors.push(json!({
                    "source": "thermal_zone",
                    "device": zone_type.clone(),
                    "label": zone_type,
                    "current_c": current_c,
                    "max_c": Option::<f64>::None,
                    "critical_c": Option::<f64>::None,
                }));
            }
        }
    }

    sensors = limited(sensors);
    let label = "Rustyfin host temperature sensors";
    let data = json!({
        "available": true,
        "sensor_count": sensors.len(),
        "sensors": sensors,
        "note": if sensors.is_empty() {
            "No temperature sensors were exposed by hwmon or thermal zones."
        } else {
            "Temperature sensors were collected from hwmon and thermal zones."
        },
    });
    Ok(available_result(label, data))
}

async fn collect_linux_block_device_inventory() -> Result<(String, Value), String> {
    let output = run_linux_command(
        "lsblk",
        &[
            "-J",
            "-b",
            "-o",
            "NAME,KNAME,TYPE,SIZE,MODEL,SERIAL,FSTYPE,MOUNTPOINTS,ROTA,TRAN,RM,RO,STATE,PKNAME,UUID,WWN,PHY-SEC,LOG-SEC",
        ],
        COMMAND_TIMEOUT_SECS,
    )
    .await;

    match output {
        Ok(stdout) => {
            let parsed: Value = serde_json::from_str(&stdout)
                .map_err(|error| format!("failed to parse lsblk JSON: {error}"))?;
            let devices = parsed
                .get("blockdevices")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let label = "Rustyfin host block device inventory";
            let data = json!({
                "available": true,
                "command": "lsblk -J -b",
                "device_count": devices.len(),
                "blockdevices": devices,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result(
            "Rustyfin host block device inventory",
            error,
        )),
    }
}

async fn collect_linux_filesystem_table() -> Result<(String, Value), String> {
    let mounts = parse_mountinfo();
    let mut entries = Vec::<Value>::new();

    for mount in mounts.into_iter().take(MAX_LIST_ITEMS) {
        let mount_point = PathBuf::from(&mount.mount_point);
        let stats = statvfs_snapshot(&mount_point);
        let stats_json = stats.as_ref().map(|snapshot| {
            json!({
                "total_bytes": snapshot.total_bytes,
                "free_bytes": snapshot.free_bytes,
                "available_bytes": snapshot.available_bytes,
                "used_bytes": snapshot.used_bytes,
                "used_percent": snapshot.used_percent,
            })
        });
        entries.push(json!({
            "mount_id": mount.mount_id,
            "parent_id": mount.parent_id,
            "major_minor": mount.major_minor,
            "root": mount.root,
            "mount_point": mount.mount_point,
            "options": mount.options,
            "filesystem_type": mount.fs_type,
            "source": mount.source,
            "super_options": mount.super_options,
            "stats": stats_json,
        }));
    }

    let label = "Rustyfin host filesystem table";
    let data = json!({
        "available": true,
        "mount_count": entries.len(),
        "filesystems": entries,
    });
    Ok(available_result(label, data))
}

async fn collect_linux_gpu_inventory() -> Result<(String, Value), String> {
    let lspci = run_linux_command("lspci", &["-Dnnk"], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    let pci_display_devices = lspci
        .as_deref()
        .map(|output| {
            output
                .split("\n\n")
                .map(str::trim)
                .filter(|block| {
                    let lower = block.to_ascii_lowercase();
                    lower.contains("vga compatible controller")
                        || lower.contains("3d controller")
                        || lower.contains("display controller")
                        || lower.contains("graphics controller")
                })
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut drm_cards = Vec::<Value>::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten().take(32) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("card") || name.contains("-") {
                continue;
            }
            let path = entry.path();
            let device = path.join("device");
            let uevent = read_trimmed(device.join("uevent")).unwrap_or_default();
            let mut driver = None::<String>;
            let mut vendor = None::<String>;
            let mut device_id = None::<String>;
            for line in uevent.lines() {
                if let Some(rest) = line.strip_prefix("DRIVER=") {
                    driver = Some(rest.to_string());
                } else if let Some(rest) = line.strip_prefix("PCI_ID=") {
                    let mut parts = rest.split(':');
                    vendor = parts.next().map(str::to_string);
                    device_id = parts.next().map(str::to_string);
                }
            }
            drm_cards.push(json!({
                "card": name,
                "driver": driver,
                "vendor_id": vendor,
                "device_id": device_id,
            }));
        }
    }

    let nvidia_smi = run_linux_command(
        "nvidia-smi",
        &[
            "--query-gpu=name,driver_version,memory.total,memory.free,memory.used",
            "--format=csv,noheader,nounits",
        ],
        COMMAND_TIMEOUT_SECS,
    )
    .await
    .ok()
    .map(|output| trim_lines(&output));
    let gpu_count = pci_display_devices.len().max(drm_cards.len());

    let label = "Rustyfin host GPU inventory";
    let data = json!({
        "available": true,
        "pci_display_devices": limited(pci_display_devices),
        "drm_cards": drm_cards,
        "nvidia_smi": nvidia_smi,
        "gpu_count": gpu_count,
    });
    Ok(available_result(label, data))
}

async fn collect_linux_pci_devices() -> Result<(String, Value), String> {
    match run_linux_command("lspci", &["-Dnn"], COMMAND_TIMEOUT_SECS).await {
        Ok(output) => {
            let entries = limited(trim_lines(&output));
            let label = "Rustyfin host PCI device inventory";
            let data = json!({
                "available": true,
                "device_count": entries.len(),
                "devices": entries,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result(
            "Rustyfin host PCI device inventory",
            error,
        )),
    }
}

async fn collect_linux_usb_devices() -> Result<(String, Value), String> {
    match run_linux_command("lsusb", &[], COMMAND_TIMEOUT_SECS).await {
        Ok(output) => {
            let entries = limited(trim_lines(&output));
            let label = "Rustyfin host USB device inventory";
            let data = json!({
                "available": true,
                "device_count": entries.len(),
                "devices": entries,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result(
            "Rustyfin host USB device inventory",
            error,
        )),
    }
}

async fn collect_linux_boot_log_summary() -> Result<(String, Value), String> {
    match run_linux_command(
        "journalctl",
        &["-b", "--no-pager", "-n", "120", "-o", "short-iso"],
        SLOW_COMMAND_TIMEOUT_SECS,
    )
    .await
    {
        Ok(output) => {
            let lines = limited(trim_lines(&output));
            let label = "Rustyfin host boot log summary";
            let data = json!({
                "available": true,
                "line_count": lines.len(),
                "lines": lines,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result("Rustyfin host boot log summary", error)),
    }
}

async fn collect_linux_journal_summary() -> Result<(String, Value), String> {
    match run_linux_command(
        "journalctl",
        &[
            "-b",
            "-p",
            "warning..alert",
            "--no-pager",
            "-n",
            "120",
            "-o",
            "short-iso",
        ],
        SLOW_COMMAND_TIMEOUT_SECS,
    )
    .await
    {
        Ok(output) => {
            let lines = limited(trim_lines(&output));
            let label = "Rustyfin host journal summary";
            let data = json!({
                "available": true,
                "line_count": lines.len(),
                "lines": lines,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result("Rustyfin host journal summary", error)),
    }
}

async fn collect_linux_route_table() -> Result<(String, Value), String> {
    if let Ok(output) = run_linux_command(
        "ip",
        &["-j", "route", "show", "table", "main"],
        COMMAND_TIMEOUT_SECS,
    )
    .await
    {
        if let Ok(routes) = serde_json::from_str::<Value>(&output) {
            let route_count = routes.as_array().map(Vec::len).unwrap_or(0);
            let default_routes = routes
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|route| route.get("dst").is_none())
                .collect::<Vec<_>>();
            let label = "Rustyfin host route table";
            let data = json!({
                "available": true,
                "source": "ip -j route show table main",
                "route_count": route_count,
                "default_routes": default_routes,
                "routes": routes,
            });
            return Ok(available_result(label, data));
        }
    }

    let raw = std::fs::read_to_string("/proc/net/route")
        .map_err(|error| format!("failed to read /proc/net/route: {error}"))?;
    let mut routes = Vec::<Value>::new();
    for line in raw.lines().skip(1) {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 11 {
            continue;
        }
        routes.push(json!({
            "iface": columns[0],
            "destination_hex": columns[1],
            "gateway_hex": columns[2],
            "flags": columns[3],
            "refcnt": columns[4],
            "use": columns[5],
            "metric": columns[6],
            "mask_hex": columns[7],
            "mtu": columns[8],
            "window": columns[9],
            "irtt": columns[10],
        }));
    }

    let label = "Rustyfin host route table";
    let data = json!({
        "available": true,
        "source": "/proc/net/route",
        "route_count": routes.len(),
        "routes": routes,
    });
    Ok(available_result(label, data))
}

async fn collect_linux_active_connections() -> Result<(String, Value), String> {
    match run_linux_command("ss", &["-H", "-tunap"], SLOW_COMMAND_TIMEOUT_SECS).await {
        Ok(output) => {
            let entries = limited(trim_lines(&output));
            let label = "Rustyfin host active connections";
            let data = json!({
                "available": true,
                "source": "ss -H -tunap",
                "connection_count": entries.len(),
                "connections": entries,
            });
            Ok(available_result(label, data))
        }
        Err(error) => Ok(unavailable_result(
            "Rustyfin host active connections",
            error,
        )),
    }
}

fn read_interface_sysfs_value(interface: &str, file: &str) -> Option<String> {
    read_trimmed(Path::new("/sys/class/net").join(interface).join(file))
}

fn parse_proc_net_dev() -> Vec<Value> {
    let raw = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    let mut interfaces = Vec::<Value>::new();
    for line in raw.lines().skip(2) {
        let Some((name, stats)) = line.split_once(':') else {
            continue;
        };
        let interface = name.trim();
        let columns = stats
            .split_whitespace()
            .filter_map(|column| column.parse::<u64>().ok())
            .collect::<Vec<_>>();
        if columns.len() < 16 {
            continue;
        }
        interfaces.push(json!({
            "name": interface,
            "rx_bytes": columns[0],
            "rx_packets": columns[1],
            "rx_errors": columns[2],
            "rx_dropped": columns[3],
            "rx_fifo": columns[4],
            "rx_frame": columns[5],
            "rx_compressed": columns[6],
            "rx_multicast": columns[7],
            "tx_bytes": columns[8],
            "tx_packets": columns[9],
            "tx_errors": columns[10],
            "tx_dropped": columns[11],
            "tx_fifo": columns[12],
            "tx_colls": columns[13],
            "tx_carrier": columns[14],
            "tx_compressed": columns[15],
            "operstate": read_interface_sysfs_value(interface, "operstate"),
            "mtu": read_interface_sysfs_value(interface, "mtu").and_then(|value| value.parse::<u64>().ok()),
            "speed_mbps": read_interface_sysfs_value(interface, "speed").and_then(|value| value.parse::<i64>().ok()).filter(|value| *value > 0),
            "carrier": read_interface_sysfs_value(interface, "carrier").and_then(|value| value.parse::<u64>().ok()),
            "mac_address": read_interface_sysfs_value(interface, "address"),
        }));
    }
    interfaces
}

async fn collect_linux_interface_counters() -> Result<(String, Value), String> {
    let interfaces = limited(parse_proc_net_dev());
    let label = "Rustyfin host interface counters";
    let data = json!({
        "available": true,
        "interface_count": interfaces.len(),
        "interfaces": interfaces,
    });
    Ok(available_result(label, data))
}

fn parse_wifi_proc() -> Vec<Value> {
    let raw = std::fs::read_to_string("/proc/net/wireless").unwrap_or_default();
    let mut interfaces = Vec::<Value>::new();
    for line in raw.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let interface = name.trim();
        let values = rest
            .split_whitespace()
            .filter_map(|token| token.trim_end_matches('.').parse::<f64>().ok())
            .collect::<Vec<_>>();
        if values.len() < 3 {
            continue;
        }
        interfaces.push(json!({
            "name": interface,
            "link_quality": values[0],
            "signal_level": values[1],
            "noise_level": values[2],
        }));
    }
    interfaces
}

async fn collect_linux_wifi_status() -> Result<(String, Value), String> {
    let mut interfaces = parse_wifi_proc();
    let iw_output = run_linux_command("iw", &["dev"], COMMAND_TIMEOUT_SECS)
        .await
        .ok();
    if let Some(output) = iw_output.as_deref() {
        let mut current: Option<Value> = None;
        for line in output.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("Interface ") {
                if let Some(entry) = current.take() {
                    interfaces.push(entry);
                }
                current = Some(json!({
                    "name": name.trim(),
                    "ssid": Option::<String>::None,
                    "phy": Option::<String>::None,
                    "type": Option::<String>::None,
                    "addr": Option::<String>::None,
                }));
            } else if let Some(entry) = current.as_mut() {
                if let Some(ssid) = line.strip_prefix("ssid ") {
                    entry["ssid"] = json!(ssid.trim());
                } else if let Some(phy) = line.strip_prefix("phy#") {
                    entry["phy"] = json!(phy.trim());
                } else if let Some(kind) = line.strip_prefix("type ") {
                    entry["type"] = json!(kind.trim());
                } else if let Some(addr) = line.strip_prefix("addr ") {
                    entry["addr"] = json!(addr.trim());
                }
            }
        }
        if let Some(entry) = current.take() {
            interfaces.push(entry);
        }
    }

    interfaces = limited(interfaces);
    let label = "Rustyfin host Wi-Fi status";
    let data = json!({
        "available": true,
        "wireless_interface_count": interfaces.len(),
        "wireless_interfaces": interfaces,
        "note": if interfaces.is_empty() {
            "No wireless interfaces were detected."
        } else {
            "Wireless interfaces were detected from /proc/net/wireless and iw dev when available."
        },
    });
    Ok(available_result(label, data))
}

fn is_tunnel_like_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("wg")
        || lower.starts_with("tun")
        || lower.starts_with("tap")
        || lower.starts_with("utun")
        || lower.starts_with("tailscale")
        || lower.starts_with("zt")
        || lower.starts_with("vpn")
        || lower.contains("wireguard")
}

async fn collect_linux_vpn_status() -> Result<(String, Value), String> {
    let mut tunnel_interfaces = Vec::<Value>::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten().take(128) {
            let interface = entry.file_name().to_string_lossy().to_string();
            if !is_tunnel_like_interface(&interface) {
                continue;
            }
            tunnel_interfaces.push(json!({
                "name": interface,
                "operstate": read_interface_sysfs_value(&interface, "operstate"),
                "mac_address": read_interface_sysfs_value(&interface, "address"),
                "mtu": read_interface_sysfs_value(&interface, "mtu").and_then(|value| value.parse::<u64>().ok()),
            }));
        }
    }

    let wireguard_interfaces =
        run_linux_command("wg", &["show", "interfaces"], COMMAND_TIMEOUT_SECS)
            .await
            .ok()
            .map(|output| {
                trim_lines(&output)
                    .into_iter()
                    .flat_map(|line| {
                        line.split_whitespace()
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

    let label = "Rustyfin host VPN status";
    let data = json!({
        "available": true,
        "tunnel_interface_count": tunnel_interfaces.len(),
        "tunnel_interfaces": tunnel_interfaces,
        "wireguard_interfaces": wireguard_interfaces,
        "note": if tunnel_interfaces.is_empty() && wireguard_interfaces.is_empty() {
            "No tunnel, VPN, or WireGuard interfaces were detected."
        } else {
            "Tunnel-like interfaces or WireGuard state were detected from the local host."
        },
    });
    Ok(available_result(label, data))
}

fn parse_ps_output(output: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(ppid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let user = fields.next().unwrap_or_default().to_string();
        let state = fields.next().unwrap_or_default().to_string();
        let cpu_percent = fields
            .next()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        let mem_percent = fields
            .next()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        let elapsed_secs = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let command = fields.next().unwrap_or_default().to_string();
        let args = fields.collect::<Vec<_>>().join(" ");
        rows.push(json!({
            "pid": pid,
            "ppid": ppid,
            "user": user,
            "state": state,
            "cpu_percent": cpu_percent,
            "mem_percent": mem_percent,
            "elapsed_secs": elapsed_secs,
            "command": command,
            "args": args,
            "raw_line": line,
        }));
    }
    rows
}

async fn collect_linux_process_detail(query: &str) -> Result<(String, Value), String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("missing process detail query".to_string());
    }

    let output = run_linux_command(
        "ps",
        &[
            "-ww",
            "-eo",
            "pid=,ppid=,user=,stat=,pcpu=,pmem=,etimes=,comm=,args=",
        ],
        COMMAND_TIMEOUT_SECS,
    )
    .await?;
    let mut processes = parse_ps_output(&output);
    let query_lower = query.to_ascii_lowercase();
    let pid_exact = query.parse::<u32>().ok();

    processes.retain(|process| {
        if pid_exact
            .is_some_and(|pid| process.get("pid").and_then(Value::as_u64) == Some(pid as u64))
        {
            return true;
        }
        let haystack = [
            process
                .get("user")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            process
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            process
                .get("args")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .join(" ")
        .to_ascii_lowercase();
        haystack.contains(&query_lower)
    });

    if processes.is_empty() {
        return Err(format!("no process matched \"{query}\""));
    }

    processes.sort_by(|left, right| {
        right
            .get("cpu_percent")
            .and_then(Value::as_f64)
            .partial_cmp(&left.get("cpu_percent").and_then(Value::as_f64))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.get("pid")
                    .and_then(Value::as_u64)
                    .cmp(&right.get("pid").and_then(Value::as_u64))
            })
    });

    let matched_by = if pid_exact.is_some() {
        "pid_exact"
    } else {
        "query_contains"
    };
    let total_count = processes.len();
    let processes = limited(processes);

    Ok((
        format!("Process detail for \"{query}\""),
        json!({
            "available": true,
            "observed_at": now_rfc3339(),
            "query": query,
            "matched_by": matched_by,
            "total_count": total_count,
            "processes": processes,
        }),
    ))
}

fn parse_ss_listener_output(output: &str) -> Vec<Value> {
    let mut listeners = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let protocol = fields.next().unwrap_or_default().to_string();
        let state = fields.next().unwrap_or_default().to_string();
        let recv_q = fields.next().unwrap_or_default().to_string();
        let send_q = fields.next().unwrap_or_default().to_string();
        let local_address = fields.next().unwrap_or_default().to_string();
        let peer_address = fields.next().unwrap_or_default().to_string();
        let process = fields.collect::<Vec<_>>().join(" ");
        let local_port = local_address
            .trim_matches(|ch| matches!(ch, '[' | ']'))
            .rsplit(':')
            .next()
            .and_then(|value| {
                value
                    .trim_matches(|ch: char| !ch.is_ascii_digit())
                    .parse::<u16>()
                    .ok()
            });
        listeners.push(json!({
            "protocol": protocol,
            "state": state,
            "recv_q": recv_q,
            "send_q": send_q,
            "local_address": local_address,
            "local_port": local_port,
            "peer_address": if peer_address == "*:*" { None::<String> } else { Some(peer_address) },
            "process": process,
            "raw_line": line,
        }));
    }
    listeners
}

async fn collect_linux_listener_detail(query: &str) -> Result<(String, Value), String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("missing listener detail query".to_string());
    }

    let output = run_linux_command("ss", &["-H", "-tulnp"], COMMAND_TIMEOUT_SECS).await?;
    let mut listeners = parse_ss_listener_output(&output);
    let query_lower = query.to_ascii_lowercase();
    let port_exact = query.parse::<u16>().ok();

    listeners.retain(|listener| {
        if port_exact.is_some_and(|port| {
            listener.get("local_port").and_then(Value::as_u64) == Some(port as u64)
        }) {
            return true;
        }
        let haystack = [
            listener
                .get("protocol")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            listener
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            listener
                .get("local_address")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            listener
                .get("peer_address")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            listener
                .get("process")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ]
        .join(" ")
        .to_ascii_lowercase();
        haystack.contains(&query_lower)
    });

    if listeners.is_empty() {
        return Err(format!("no listener matched \"{query}\""));
    }

    listeners.sort_by(|left, right| {
        left.get("protocol")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("protocol")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .then_with(|| {
                left.get("local_port")
                    .and_then(Value::as_u64)
                    .cmp(&right.get("local_port").and_then(Value::as_u64))
            })
    });

    let matched_by = if port_exact.is_some() {
        "port_exact"
    } else {
        "query_contains"
    };
    let total_count = listeners.len();
    let listeners = limited(listeners);

    Ok((
        format!("Listener detail for \"{query}\""),
        json!({
            "available": true,
            "observed_at": now_rfc3339(),
            "query": query,
            "matched_by": matched_by,
            "total_count": total_count,
            "listeners": listeners,
        }),
    ))
}

fn resolve_disk_usage_target<'a>(
    query: &str,
    mounts: &'a [MountInfoEntry],
) -> Option<(&'a MountInfoEntry, &'static str)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    if let Some(path) = mounts.iter().find(|mount| mount.mount_point == query) {
        return Some((path, "mount_point_exact"));
    }
    if let Some(path) = mounts.iter().find(|mount| mount.source == query) {
        return Some((path, "source_exact"));
    }
    if let Some(path) = mounts
        .iter()
        .find(|mount| mount.mount_point.ends_with(query) || mount.source.ends_with(query))
    {
        return Some((path, "suffix_match"));
    }

    let query_lower = query.to_ascii_lowercase();
    mounts
        .iter()
        .find(|mount| {
            mount
                .mount_point
                .to_ascii_lowercase()
                .contains(&query_lower)
                || mount.source.to_ascii_lowercase().contains(&query_lower)
                || mount.fs_type.to_ascii_lowercase().contains(&query_lower)
        })
        .map(|mount| (mount, "contains_match"))
}

async fn collect_linux_disk_usage_detail(query: &str) -> Result<(String, Value), String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("missing disk usage detail query".to_string());
    }

    let mounts = parse_mountinfo();
    let (mount, matched_by) = if Path::new(&query).exists() {
        let canonical = std::fs::canonicalize(&query).unwrap_or_else(|_| PathBuf::from(&query));
        let canonical_str = canonical.to_string_lossy().to_string();
        if let Some((mount, matched_by)) = resolve_disk_usage_target(&canonical_str, &mounts) {
            (mount, matched_by)
        } else if let Some((mount, matched_by)) = resolve_disk_usage_target(&query, &mounts) {
            (mount, matched_by)
        } else {
            return Err(format!("no mounted filesystem matched \"{query}\""));
        }
    } else if let Some((mount, matched_by)) = resolve_disk_usage_target(&query, &mounts) {
        (mount, matched_by)
    } else {
        return Err(format!("no mounted filesystem matched \"{query}\""));
    };

    let stats = statvfs_snapshot(Path::new(&mount.mount_point))
        .or_else(|| statvfs_snapshot(Path::new(&query)))
        .ok_or_else(|| format!("unable to inspect disk usage for \"{query}\""))?;

    Ok((
        format!("Disk usage detail for \"{}\"", mount.mount_point),
        json!({
            "available": true,
            "observed_at": now_rfc3339(),
            "query": query,
            "matched_by": matched_by,
            "mount_point": mount.mount_point,
            "source": mount.source,
            "fs_type": mount.fs_type,
            "root": mount.root,
            "mount_id": mount.mount_id,
            "parent_id": mount.parent_id,
            "major_minor": mount.major_minor,
            "options": mount.options,
            "super_options": mount.super_options,
            "total_bytes": stats.total_bytes,
            "free_bytes": stats.free_bytes,
            "available_bytes": stats.available_bytes,
            "used_bytes": stats.used_bytes,
            "used_percent": stats.used_percent,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        MountInfoEntry, decode_mountinfo_field, is_tunnel_like_interface, parse_kv_output,
        parse_proc_net_dev, parse_ps_output, parse_ss_listener_output, parse_wifi_proc,
        resolve_disk_usage_target,
    };

    #[test]
    fn decode_mountinfo_unescapes_space() {
        assert_eq!(decode_mountinfo_field("foo\\040bar"), "foo bar");
    }

    #[test]
    fn parse_kv_output_parses_key_values() {
        let values = parse_kv_output("Key: Value\nIgnored line\nOther: Thing");
        assert_eq!(values.get("Key").map(String::as_str), Some("Value"));
        assert_eq!(values.get("Other").map(String::as_str), Some("Thing"));
    }

    #[test]
    fn tunnel_name_detection_matches_common_patterns() {
        assert!(is_tunnel_like_interface("wg0"));
        assert!(is_tunnel_like_interface("tun0"));
        assert!(is_tunnel_like_interface("tailscale0"));
        assert!(!is_tunnel_like_interface("eth0"));
    }

    #[test]
    fn proc_net_dev_parser_handles_interface_lines() {
        let interfaces = parse_proc_net_dev();
        assert!(interfaces.iter().all(|value| value.get("name").is_some()));
    }

    #[test]
    fn wifi_proc_parser_handles_absent_or_present_data() {
        let interfaces = parse_wifi_proc();
        assert!(interfaces.iter().all(|value| value.get("name").is_some()));
    }

    #[test]
    fn process_parser_extracts_rows() {
        let rows = parse_ps_output("1 0 root S 0.1 0.2 100 init /sbin/init\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get("pid").and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            rows[0].get("command").and_then(serde_json::Value::as_str),
            Some("init")
        );
    }

    #[test]
    fn listener_parser_extracts_rows() {
        let rows = parse_ss_listener_output(
            "tcp LISTEN 0 4096 0.0.0.0:3008 0.0.0.0:* users:((\"rustyfin\",pid=123,fd=5))\n",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .get("local_port")
                .and_then(serde_json::Value::as_u64),
            Some(3008)
        );
    }

    #[test]
    fn disk_usage_target_prefers_exact_mounts() {
        let mounts = vec![MountInfoEntry {
            mount_id: 1,
            parent_id: 0,
            major_minor: "8:1".to_string(),
            root: "/".to_string(),
            mount_point: "/srv/media".to_string(),
            options: "rw".to_string(),
            fs_type: "ext4".to_string(),
            source: "/dev/sda1".to_string(),
            super_options: "rw".to_string(),
        }];
        let (mount, matched_by) =
            resolve_disk_usage_target("/srv/media", &mounts).expect("mount should match");
        assert_eq!(mount.mount_point, "/srv/media");
        assert_eq!(matched_by, "mount_point_exact");
    }
}
