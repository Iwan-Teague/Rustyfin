use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use rustfin_core::servers_agent::{
    DiscoveryCandidate, ImportProvisionSpec, ManagedProvisionSpec, ProvisioningResult,
    ServerLifecycleAction, ServerLogLine, ServersAgentDiscoveryScanResponse,
    ServersAgentLogsResponse, SystemdUnitStatus,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Debug, Deserialize)]
struct VanillaVersionManifest {
    versions: Vec<VanillaVersionEntry>,
}

#[derive(Debug, Deserialize)]
struct VanillaVersionEntry {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct VanillaVersionDetails {
    downloads: VanillaVersionDownloads,
}

#[derive(Debug, Deserialize)]
struct VanillaVersionDownloads {
    server: Option<VanillaServerDownload>,
}

#[derive(Debug, Deserialize)]
struct VanillaServerDownload {
    url: String,
}

#[derive(Debug, Clone)]
pub struct NativeRuntimeCapabilities {
    pub status_supported: bool,
    pub lifecycle_supported: bool,
    pub provision_supported: bool,
    pub import_supported: bool,
    pub delete_supported: bool,
    pub reason: Option<String>,
}

fn systemctl_bin() -> String {
    std::env::var("RUSTFIN_SERVERS_SYSTEMCTL_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "systemctl".to_string())
}

fn journalctl_bin() -> String {
    std::env::var("RUSTFIN_SERVERS_JOURNALCTL_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "journalctl".to_string())
}

fn systemd_unit_dir() -> PathBuf {
    std::env::var("RUSTFIN_SERVERS_SYSTEMD_UNIT_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/systemd/system"))
}

fn artifact_cache_root() -> PathBuf {
    std::env::var("RUSTFIN_SERVERS_ARTIFACT_CACHE_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/cache/rustyfin-servers/minecraft/artifacts"))
}

fn instance_root_base_path() -> PathBuf {
    std::env::var("RUSTFIN_SERVERS_INSTANCE_ROOT")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/srv/rustyfin-servers/minecraft/instances"))
}

fn managed_instances_root() -> Option<PathBuf> {
    std::fs::canonicalize(instance_root_base_path()).ok()
}

fn unit_service_user() -> Option<String> {
    std::env::var("RUSTFIN_SERVERS_SYSTEM_USER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn unit_service_group() -> Option<String> {
    std::env::var("RUSTFIN_SERVERS_SYSTEM_GROUP")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn unit_service_identity() -> Option<(String, String)> {
    let user = unit_service_user()?;
    let group = unit_service_group().unwrap_or_else(|| user.clone());
    Some((user, group))
}

fn import_roots() -> Vec<PathBuf> {
    let raw = std::env::var("RUSTFIN_SERVERS_IMPORT_ROOTS")
        .ok()
        .or_else(|| std::env::var("RUSTFIN_DIRECTORY_BROWSE_ROOTS").ok())
        .unwrap_or_else(|| "/srv:/home:/media".to_string());

    let mut roots = Vec::new();
    for segment in raw.split(':') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(trimmed);
        if let Ok(canonical) = std::fs::canonicalize(&candidate) {
            if canonical.is_dir() && !roots.iter().any(|existing| existing == &canonical) {
                roots.push(canonical);
            }
        }
    }
    roots
}

fn discovery_max_depth() -> usize {
    std::env::var("RUSTFIN_SERVERS_DISCOVERY_MAX_DEPTH")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0 && *value <= 12)
        .unwrap_or(6)
}

fn ensure_native_runtime_supported() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("Minecraft native runtime control is only supported on Linux hosts".to_string())
    }
}

pub fn detect_native_runtime_capabilities() -> NativeRuntimeCapabilities {
    if !cfg!(target_os = "linux") {
        return NativeRuntimeCapabilities {
            status_supported: false,
            lifecycle_supported: false,
            provision_supported: false,
            import_supported: false,
            delete_supported: false,
            reason: Some("Minecraft native runtime control requires a Linux host".to_string()),
        };
    }

    let systemctl = systemctl_bin();
    if std::process::Command::new(&systemctl)
        .arg("--version")
        .output()
        .is_err()
    {
        return NativeRuntimeCapabilities {
            status_supported: false,
            lifecycle_supported: false,
            provision_supported: false,
            import_supported: false,
            delete_supported: true,
            reason: Some(
                "This runtime does not expose systemctl, so start/stop/provision/import controls are unavailable here."
                    .to_string(),
            ),
        };
    }

    if !Path::new("/run/systemd/system").exists() {
        return NativeRuntimeCapabilities {
            status_supported: false,
            lifecycle_supported: false,
            provision_supported: false,
            import_supported: false,
            delete_supported: true,
            reason: Some(
                "This runtime is not booted with systemd, so start/stop/provision/import controls are unavailable here."
                    .to_string(),
            ),
        };
    }

    NativeRuntimeCapabilities {
        status_supported: true,
        lifecycle_supported: true,
        provision_supported: true,
        import_supported: true,
        delete_supported: true,
        reason: None,
    }
}

fn parse_properties(stdout: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    values
}

fn parse_unit_status_stdout(stdout: &str) -> SystemdUnitStatus {
    let properties = parse_properties(stdout);
    SystemdUnitStatus {
        load_state: properties
            .get("LoadState")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        active_state: properties
            .get("ActiveState")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        sub_state: properties
            .get("SubState")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        unit_file_state: properties
            .get("UnitFileState")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        result: properties
            .get("Result")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        exec_main_status: properties
            .get("ExecMainStatus")
            .and_then(|value| value.parse::<i64>().ok()),
    }
}

fn run_systemctl_output(args: &[&str]) -> String {
    format!("systemctl {}", args.join(" "))
}

async fn run_systemctl(args: &[&str]) -> Result<(), String> {
    ensure_native_runtime_supported()?;
    let output = Command::new(systemctl_bin())
        .args(args)
        .output()
        .await
        .map_err(|error| format!("failed to launch {}: {error}", run_systemctl_output(args)))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!(
                "{} exited with {}",
                run_systemctl_output(args),
                output.status
            )
        };
        Err(detail)
    }
}

pub async fn run_lifecycle_action(
    unit_name: &str,
    action: ServerLifecycleAction,
) -> Result<(), String> {
    run_systemctl(&[action.as_str(), unit_name]).await
}

pub async fn daemon_reload() -> Result<(), String> {
    run_systemctl(&["daemon-reload"]).await
}

pub async fn sync_unit_enabled(unit_name: &str, enabled: bool) -> Result<(), String> {
    if enabled {
        run_systemctl(&["enable", unit_name]).await
    } else {
        run_systemctl(&["disable", unit_name]).await
    }
}

pub async fn query_unit_status(unit_name: &str) -> Result<SystemdUnitStatus, String> {
    ensure_native_runtime_supported()?;
    let output = Command::new(systemctl_bin())
        .arg("show")
        .arg(unit_name)
        .arg("--no-page")
        .arg("--property=LoadState")
        .arg("--property=ActiveState")
        .arg("--property=SubState")
        .arg("--property=UnitFileState")
        .arg("--property=Result")
        .arg("--property=ExecMainStatus")
        .output()
        .await
        .map_err(|error| format!("failed to query systemctl status: {error}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_unit_status_stdout(&stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("systemctl show exited with {}", output.status)
        };
        Err(detail)
    }
}

fn is_missing_unit_error(detail: &str) -> bool {
    let normalized = detail.trim().to_ascii_lowercase();
    normalized.contains("not loaded")
        || normalized.contains("no such file")
        || normalized.contains("could not be found")
        || normalized.contains("unit ") && normalized.contains(" not found")
}

fn is_systemctl_unavailable_error(detail: &str) -> bool {
    let normalized = detail.trim().to_ascii_lowercase();
    (normalized.contains("failed to launch systemctl")
        && normalized.contains("no such file or directory"))
        || normalized.contains("system has not been booted with systemd")
        || normalized.contains("failed to connect to bus")
}

fn validate_managed_instance_root(instance_root: &Path) -> Result<(), String> {
    let base_root = instance_root_base_path();
    if !instance_root.starts_with(&base_root) {
        return Err(format!(
            "refusing to delete instance root outside managed base path: {}",
            instance_root.display()
        ));
    }
    if instance_root == base_root {
        return Err("refusing to delete the managed instances base root".to_string());
    }
    Ok(())
}

pub async fn delete_managed_instance(unit_name: &str, instance_root: &str) -> Result<(), String> {
    ensure_native_runtime_supported()?;
    let mut systemctl_available = true;

    if let Err(error) = run_systemctl(&["stop", unit_name]).await {
        if is_systemctl_unavailable_error(&error) {
            systemctl_available = false;
        } else if !is_missing_unit_error(&error) {
            return Err(format!(
                "failed to stop systemd unit before delete: {error}"
            ));
        }
    }

    if systemctl_available {
        if let Err(error) = run_systemctl(&["disable", unit_name]).await {
            if is_systemctl_unavailable_error(&error) {
                systemctl_available = false;
            } else if !is_missing_unit_error(&error) {
                return Err(format!(
                    "failed to disable systemd unit before delete: {error}"
                ));
            }
        }
    }

    let unit_path = systemd_unit_dir().join(unit_name.trim());
    let instance_root = PathBuf::from(instance_root.trim());
    validate_managed_instance_root(&instance_root)?;

    tokio::task::spawn_blocking(move || {
        if unit_path.exists() {
            std::fs::remove_file(&unit_path).map_err(|error| {
                format!(
                    "failed to remove systemd unit file {}: {error}",
                    unit_path.display()
                )
            })?;
        }

        if instance_root.exists() {
            std::fs::remove_dir_all(&instance_root).map_err(|error| {
                format!(
                    "failed to remove managed instance root {}: {error}",
                    instance_root.display()
                )
            })?;
        }

        Ok::<(), String>(())
    })
    .await
    .map_err(|error| format!("delete task failed: {error}"))??;

    if systemctl_available {
        if let Err(error) = daemon_reload().await {
            if !is_systemctl_unavailable_error(&error) {
                return Err(format!("failed to reload systemd after delete: {error}"));
            }
        }
    }

    Ok(())
}

fn normalize_probe_host(host: &str) -> String {
    match host.trim() {
        "" | "0.0.0.0" | "::" | "[::]" | "::0" | "*" => "127.0.0.1".to_string(),
        other => other.to_string(),
    }
}

fn flatten_description(value: &Value) -> String {
    match value {
        Value::String(text) => text.to_string(),
        Value::Array(values) => values
            .iter()
            .map(flatten_description)
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(map) => {
            let mut text = String::new();
            if let Some(Value::String(value)) = map.get("text") {
                text.push_str(value);
            }
            if let Some(extra) = map.get("extra") {
                text.push_str(&flatten_description(extra));
            }
            text
        }
        _ => String::new(),
    }
}

fn write_varint(buffer: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut temp = (value & 0b0111_1111) as u8;
        value >>= 7;
        if value != 0 {
            temp |= 0b1000_0000;
        }
        buffer.push(temp);
        if value == 0 {
            break;
        }
    }
}

async fn read_varint(stream: &mut TcpStream) -> Result<i32, String> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        let byte = stream
            .read_u8()
            .await
            .map_err(|error| format!("failed to read varint byte: {error}"))?;
        let value = (byte & 0b0111_1111) as i32;
        result |= value << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err("minecraft status varint was too large".to_string());
        }
        if (byte & 0b1000_0000) == 0 {
            break;
        }
    }
    Ok(result)
}

#[derive(Debug, Deserialize)]
struct MinecraftStatusResponse {
    #[serde(default)]
    version: Option<MinecraftStatusVersion>,
    #[serde(default)]
    players: Option<MinecraftStatusPlayers>,
    #[serde(default)]
    description: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct MinecraftStatusVersion {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    protocol: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct MinecraftStatusPlayers {
    #[serde(default)]
    max: Option<i64>,
    #[serde(default)]
    online: Option<i64>,
}

pub async fn probe_minecraft_server(
    host: &str,
    port: u16,
) -> Result<rustfin_core::servers_agent::MinecraftServerProbe, String> {
    let target_host = normalize_probe_host(host);
    let mut stream = timeout(
        Duration::from_secs(3),
        TcpStream::connect((target_host.as_str(), port)),
    )
    .await
    .map_err(|_| format!("minecraft status probe to {target_host}:{port} timed out"))?
    .map_err(|error| {
        format!("failed to connect to Minecraft server at {target_host}:{port}: {error}")
    })?;

    let protocol_version = std::env::var("RUSTFIN_SERVERS_MINECRAFT_STATUS_PROTOCOL")
        .ok()
        .and_then(|raw| raw.trim().parse::<i32>().ok())
        .unwrap_or(760);

    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0);
    write_varint(&mut handshake, protocol_version);
    write_varint(&mut handshake, target_host.len() as i32);
    handshake.extend_from_slice(target_host.as_bytes());
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);

    let mut handshake_packet = Vec::new();
    write_varint(&mut handshake_packet, handshake.len() as i32);
    handshake_packet.extend_from_slice(&handshake);

    timeout(Duration::from_secs(3), stream.write_all(&handshake_packet))
        .await
        .map_err(|_| "minecraft status handshake write timed out".to_string())?
        .map_err(|error| format!("failed to write Minecraft handshake: {error}"))?;

    let request_packet = [1u8, 0u8];
    timeout(Duration::from_secs(3), stream.write_all(&request_packet))
        .await
        .map_err(|_| "minecraft status request write timed out".to_string())?
        .map_err(|error| format!("failed to write Minecraft status request: {error}"))?;

    let _packet_length = timeout(Duration::from_secs(3), read_varint(&mut stream))
        .await
        .map_err(|_| "minecraft status response timed out".to_string())??;
    let packet_id = timeout(Duration::from_secs(3), read_varint(&mut stream))
        .await
        .map_err(|_| "minecraft status packet id timed out".to_string())??;
    if packet_id != 0 {
        return Err(format!("unexpected Minecraft status packet id {packet_id}"));
    }
    let json_length = timeout(Duration::from_secs(3), read_varint(&mut stream))
        .await
        .map_err(|_| "minecraft status payload length timed out".to_string())??;
    if json_length < 0 || json_length > 1024 * 1024 {
        return Err("minecraft status payload length was invalid".to_string());
    }
    let mut json_buf = vec![0u8; json_length as usize];
    timeout(Duration::from_secs(3), stream.read_exact(&mut json_buf))
        .await
        .map_err(|_| "minecraft status payload read timed out".to_string())?
        .map_err(|error| format!("failed to read Minecraft status payload: {error}"))?;

    let parsed = serde_json::from_slice::<MinecraftStatusResponse>(&json_buf)
        .map_err(|error| format!("failed to decode Minecraft status payload: {error}"))?;

    let description = parsed
        .description
        .as_ref()
        .map(flatten_description)
        .filter(|value| !value.trim().is_empty());

    Ok(rustfin_core::servers_agent::MinecraftServerProbe {
        version_name: parsed.version.as_ref().and_then(|value| value.name.clone()),
        protocol_version: parsed.version.as_ref().and_then(|value| value.protocol),
        online_players: parsed
            .players
            .as_ref()
            .and_then(|value| value.online)
            .unwrap_or(0),
        max_players: parsed.players.as_ref().and_then(|value| value.max),
        description,
    })
}

fn bool_prop(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn desired_server_properties(spec: &ManagedProvisionSpec) -> BTreeMap<&'static str, String> {
    let mut properties = BTreeMap::new();
    properties.insert("allow-flight", bool_prop(spec.allow_flight).to_string());
    properties.insert(
        "enable-command-block",
        bool_prop(spec.enable_command_block).to_string(),
    );
    properties.insert("difficulty", spec.difficulty.clone());
    properties.insert("gamemode", spec.gamemode.clone());
    properties.insert("hardcore", bool_prop(spec.hardcore).to_string());
    properties.insert("level-name", spec.world_name.clone());
    properties.insert("motd", spec.motd.clone());
    properties.insert("online-mode", bool_prop(spec.online_mode).to_string());
    properties.insert("pvp", bool_prop(spec.pvp).to_string());
    properties.insert("server-port", spec.listen_port.to_string());
    properties.insert("white-list", bool_prop(spec.white_list_enabled).to_string());
    properties
}

fn render_server_properties(
    existing: Option<&str>,
    desired: &BTreeMap<&'static str, String>,
) -> String {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    if let Some(existing_content) = existing {
        for line in existing_content.lines() {
            if let Some((raw_key, _)) = line.split_once('=') {
                let key = raw_key.trim();
                if let Some(value) = desired.get(key) {
                    lines.push(format!("{key}={value}"));
                    seen.insert(key.to_string());
                    continue;
                }
            }
            lines.push(line.to_string());
        }
    }

    for (key, value) in desired {
        if !seen.contains(*key) {
            lines.push(format!("{key}={value}"));
        }
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn render_systemd_unit(spec: &ManagedProvisionSpec) -> String {
    let mut unit = String::new();
    unit.push_str("[Unit]\n");
    unit.push_str(&format!(
        "Description=Rustyfin Minecraft Server {}\n",
        spec.display_name
    ));
    unit.push_str("After=network-online.target\n");
    unit.push_str("Wants=network-online.target\n\n");

    unit.push_str("[Service]\n");
    unit.push_str("Type=simple\n");
    if let Some(user) = unit_service_user() {
        unit.push_str(&format!("User={user}\n"));
    }
    if let Some(group) = unit_service_group() {
        unit.push_str(&format!("Group={group}\n"));
    }
    unit.push_str(&format!("WorkingDirectory={}\n", spec.server_work_dir));
    unit.push_str(&format!(
        "ExecStart={} -Xms{}M -Xmx{}M -jar server.jar nogui\n",
        spec.java_path, spec.min_memory_mb, spec.max_memory_mb
    ));
    unit.push_str("ExecStop=/bin/sh -c 'if [ -n \"$MAINPID\" ]; then kill -s INT \"$MAINPID\"; fi'\n");
    unit.push_str("KillSignal=SIGINT\n");
    unit.push_str("SuccessExitStatus=0 143\n");
    unit.push_str("Restart=on-failure\n");
    unit.push_str("RestartSec=5\n");
    unit.push_str("TimeoutStopSec=120\n");
    unit.push_str("StandardOutput=journal\n");
    unit.push_str("StandardError=journal\n\n");
    unit.push_str("[Install]\n");
    unit.push_str("WantedBy=multi-user.target\n");
    unit
}

fn apply_instance_ownership(instance_root: &Path) -> Result<(), String> {
    let Some((user, group)) = unit_service_identity() else {
        return Ok(());
    };

    let status = std::process::Command::new("chown")
        .arg("-R")
        .arg(format!("{user}:{group}"))
        .arg(instance_root)
        .status()
        .map_err(|error| {
            format!(
                "failed to launch chown for {} -> {}:{}: {error}",
                instance_root.display(),
                user,
                group
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "chown -R {}:{} {} exited with {}",
            user,
            group,
            instance_root.display(),
            status
        ))
    }
}

fn validate_import_source_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("source_path is required".to_string());
    }

    let canonical = std::fs::canonicalize(trimmed)
        .map_err(|error| format!("failed to resolve import source path: {error}"))?;
    if !canonical.is_dir() {
        return Err("import source must be a directory".to_string());
    }

    let roots = import_roots();
    if roots.is_empty() {
        return Err("no import roots are configured on this host".to_string());
    }
    if !roots.iter().any(|root| canonical.starts_with(root)) {
        return Err("import source is outside configured server import roots".to_string());
    }

    Ok(canonical)
}

fn ensure_empty_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if entries.next().is_some() {
        return Err(format!(
            "target server directory {} is not empty",
            path.display()
        ));
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest)
        .map_err(|error| format!("failed to create {}: {error}", dest.display()))?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read source entry: {error}"))?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {error}", source_path.display()))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &dest_path)?;
        } else if metadata.is_file() {
            std::fs::copy(&source_path, &dest_path).map_err(|error| {
                format!(
                    "failed to copy {} -> {}: {error}",
                    source_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn detect_imported_server_jar(work_dir: &Path) -> Result<PathBuf, String> {
    let direct = work_dir.join("server.jar");
    if direct.is_file() {
        return Ok(direct);
    }

    let entries = std::fs::read_dir(work_dir)
        .map_err(|error| format!("failed to inspect imported work dir: {error}"))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read imported entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jar") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if file_name.contains("installer") {
            continue;
        }
        candidates.push(path);
    }

    candidates.sort();
    candidates.into_iter().next().ok_or_else(|| {
        "no top-level Minecraft server jar was found in the imported directory".to_string()
    })
}

fn write_runtime_files(
    spec: &ManagedProvisionSpec,
    existing_props: Option<&str>,
    server_jar_source: &Path,
) -> Result<ProvisioningResult, String> {
    let instance_root = PathBuf::from(&spec.instance_root);
    let work_dir = PathBuf::from(&spec.server_work_dir);
    let meta_dir = instance_root.join("meta");
    let backups_dir = instance_root.join("backups");
    let uploads_dir = instance_root.join("uploads");
    let unit_path = systemd_unit_dir().join(&spec.systemd_unit_name);
    let runtime_server_jar = work_dir.join("server.jar");

    std::fs::create_dir_all(&work_dir)
        .map_err(|error| format!("failed to create {}: {error}", work_dir.display()))?;
    std::fs::create_dir_all(&meta_dir)
        .map_err(|error| format!("failed to create {}: {error}", meta_dir.display()))?;
    std::fs::create_dir_all(&backups_dir)
        .map_err(|error| format!("failed to create {}: {error}", backups_dir.display()))?;
    std::fs::create_dir_all(&uploads_dir)
        .map_err(|error| format!("failed to create {}: {error}", uploads_dir.display()))?;
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    if server_jar_source != runtime_server_jar {
        std::fs::copy(server_jar_source, &runtime_server_jar).map_err(|error| {
            format!(
                "failed to copy {} -> {}: {error}",
                server_jar_source.display(),
                runtime_server_jar.display()
            )
        })?;
    }

    let desired_properties = desired_server_properties(spec);
    let server_properties_content = render_server_properties(existing_props, &desired_properties);
    std::fs::write(
        work_dir.join("server.properties"),
        server_properties_content,
    )
    .map_err(|error| {
        format!(
            "failed to write server.properties in {}: {error}",
            work_dir.display()
        )
    })?;
    std::fs::write(work_dir.join("eula.txt"), "eula=true\n").map_err(|error| {
        format!(
            "failed to write eula.txt in {}: {error}",
            work_dir.display()
        )
    })?;

    let instance_manifest = json!({
        "instance_id": spec.instance_id,
        "display_name": spec.display_name,
        "install_mode": spec.install_mode,
        "systemd_unit_name": spec.systemd_unit_name,
        "server_work_dir": spec.server_work_dir,
        "listen_host": spec.listen_host,
        "listen_port": spec.listen_port,
        "server_distribution": spec.server_distribution,
        "minecraft_version": spec.minecraft_version,
    });
    std::fs::write(
        meta_dir.join("instance.json"),
        serde_json::to_string_pretty(&instance_manifest)
            .map_err(|error| format!("failed to encode instance manifest: {error}"))?,
    )
    .map_err(|error| {
        format!(
            "failed to write instance.json in {}: {error}",
            meta_dir.display()
        )
    })?;

    let unit_text = render_systemd_unit(spec);
    std::fs::write(&unit_path, &unit_text).map_err(|error| {
        format!(
            "failed to write systemd unit {}: {error}",
            unit_path.display()
        )
    })?;
    std::fs::write(meta_dir.join("rendered-unit.service"), unit_text).map_err(|error| {
        format!(
            "failed to write rendered-unit.service in {}: {error}",
            meta_dir.display()
        )
    })?;

    apply_instance_ownership(&instance_root)?;

    Ok(ProvisioningResult {
        install_mode: spec.install_mode.clone(),
        unit_path: unit_path.to_string_lossy().into_owned(),
        work_dir: work_dir.to_string_lossy().into_owned(),
        server_jar_path: runtime_server_jar.to_string_lossy().into_owned(),
    })
}

async fn resolve_vanilla_server_download(version: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let manifest: VanillaVersionManifest = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await
        .map_err(|error| format!("failed to fetch vanilla version manifest: {error}"))?
        .error_for_status()
        .map_err(|error| format!("vanilla version manifest request failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("failed to decode vanilla version manifest: {error}"))?;

    let version_url = manifest
        .versions
        .into_iter()
        .find(|entry| entry.id == version)
        .map(|entry| entry.url)
        .ok_or_else(|| format!("Minecraft vanilla version {version} was not found"))?;

    let details: VanillaVersionDetails = client
        .get(version_url)
        .send()
        .await
        .map_err(|error| format!("failed to fetch vanilla version details: {error}"))?
        .error_for_status()
        .map_err(|error| format!("vanilla version details request failed: {error}"))?
        .json()
        .await
        .map_err(|error| format!("failed to decode vanilla version details: {error}"))?;

    details
        .downloads
        .server
        .map(|server| server.url)
        .ok_or_else(|| format!("Minecraft version {version} does not publish a server jar"))
}

async fn resolve_paper_server_download(version: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let metadata: Value = client
        .get(format!(
            "https://api.papermc.io/v2/projects/paper/versions/{version}/builds"
        ))
        .send()
        .await
        .map_err(|error| format!("failed to fetch Paper builds for {version}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Paper builds request failed for {version}: {error}"))?
        .json()
        .await
        .map_err(|error| format!("failed to decode Paper build metadata: {error}"))?;

    let builds = metadata
        .get("builds")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Paper build metadata for {version} did not include builds"))?;

    let mut best_build_num: Option<i64> = None;
    let mut best_download_name: Option<String> = None;
    for build in builds {
        let build_num = build
            .get("build")
            .and_then(Value::as_i64)
            .or_else(|| build.as_i64());
        let Some(build_num) = build_num else {
            continue;
        };
        if best_build_num.is_none_or(|current| build_num > current) {
            best_build_num = Some(build_num);
            best_download_name = build
                .pointer("/downloads/application/name")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    let build_num =
        best_build_num.ok_or_else(|| format!("Paper build metadata for {version} was empty"))?;
    let download_name =
        best_download_name.unwrap_or_else(|| format!("paper-{version}-{build_num}.jar"));

    Ok(format!(
        "https://api.papermc.io/v2/projects/paper/versions/{version}/builds/{build_num}/downloads/{download_name}"
    ))
}

async fn ensure_managed_server_artifact(spec: &ManagedProvisionSpec) -> Result<PathBuf, String> {
    let cache_root = artifact_cache_root();
    let version_dir = cache_root
        .join(spec.server_distribution.trim())
        .join(spec.minecraft_version.trim());
    let artifact_path = version_dir.join("server.jar");
    if artifact_path.is_file() {
        return Ok(artifact_path);
    }

    tokio::fs::create_dir_all(&version_dir)
        .await
        .map_err(|error| {
            format!(
                "failed to create artifact cache {}: {error}",
                version_dir.display()
            )
        })?;

    let download_url = match spec.server_distribution.as_str() {
        "vanilla" => resolve_vanilla_server_download(&spec.minecraft_version).await?,
        "paper" => resolve_paper_server_download(&spec.minecraft_version).await?,
        other => return Err(format!("unsupported managed server distribution: {other}")),
    };

    let temp_path = artifact_path.with_extension("jar.partial");
    let response = reqwest::get(download_url.clone())
        .await
        .map_err(|error| format!("failed to download server artifact: {error}"))?
        .error_for_status()
        .map_err(|error| format!("artifact download failed: {error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("failed to read artifact bytes: {error}"))?;

    tokio::fs::write(&temp_path, &bytes)
        .await
        .map_err(|error| {
            format!(
                "failed to write temporary artifact {}: {error}",
                temp_path.display()
            )
        })?;
    tokio::fs::rename(&temp_path, &artifact_path)
        .await
        .map_err(|error| {
            format!(
                "failed to move artifact into cache {}: {error}",
                artifact_path.display()
            )
        })?;

    Ok(artifact_path)
}

pub async fn provision_managed_instance(
    spec: &ManagedProvisionSpec,
) -> Result<ProvisioningResult, String> {
    ensure_native_runtime_supported()?;
    let artifact_path = ensure_managed_server_artifact(spec).await?;
    let spec_for_task = spec.clone();
    let unit_name = spec.systemd_unit_name.clone();
    let autostart = spec.autostart;
    let result = tokio::task::spawn_blocking(move || {
        let existing_props_path =
            PathBuf::from(&spec_for_task.server_work_dir).join("server.properties");
        let existing_props = std::fs::read_to_string(&existing_props_path).ok();
        write_runtime_files(&spec_for_task, existing_props.as_deref(), &artifact_path)
    })
    .await
    .map_err(|error| format!("managed provisioning task failed: {error}"))??;

    daemon_reload().await?;
    sync_unit_enabled(&unit_name, autostart).await?;
    Ok(result)
}

pub async fn import_existing_instance(
    spec: &ImportProvisionSpec,
) -> Result<ProvisioningResult, String> {
    ensure_native_runtime_supported()?;
    let spec = spec.clone();
    let unit_name = spec.managed.systemd_unit_name.clone();
    let autostart = spec.managed.autostart;
    let result = tokio::task::spawn_blocking(move || {
        let source = validate_import_source_path(&spec.source_path)?;
        let work_dir = PathBuf::from(&spec.managed.server_work_dir);
        let instance_root = PathBuf::from(&spec.managed.instance_root);

        if source.starts_with(&instance_root) {
            return Err("import source cannot be inside this managed instance root".to_string());
        }

        ensure_empty_directory(&work_dir)?;
        std::fs::create_dir_all(&work_dir)
            .map_err(|error| format!("failed to create {}: {error}", work_dir.display()))?;
        copy_dir_recursive(&source, &work_dir)?;

        let detected_jar = detect_imported_server_jar(&work_dir)?;
        let runtime_jar = work_dir.join("server.jar");
        if detected_jar != runtime_jar {
            std::fs::copy(&detected_jar, &runtime_jar).map_err(|error| {
                format!(
                    "failed to normalize imported jar {} -> {}: {error}",
                    detected_jar.display(),
                    runtime_jar.display()
                )
            })?;
        }

        let existing_props_path = work_dir.join("server.properties");
        let existing_props = std::fs::read_to_string(&existing_props_path).ok();
        let mut managed = spec.managed.clone();
        managed.install_mode = "imported".to_string();
        write_runtime_files(&managed, existing_props.as_deref(), &runtime_jar)
    })
    .await
    .map_err(|error| format!("import task failed: {error}"))??;

    daemon_reload().await?;
    sync_unit_enabled(&unit_name, autostart).await?;
    Ok(result)
}

fn parse_journal_line(line: &str) -> Option<ServerLogLine> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let ts_ms = value
        .get("__REALTIME_TIMESTAMP")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<i64>().ok())
        .map(|micros| micros / 1000);
    let priority = value
        .get("PRIORITY")
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = value
        .get("MESSAGE")
        .and_then(|msg| {
            msg.as_str()
                .map(str::to_string)
                .or_else(|| Some(msg.to_string()))
        })
        .unwrap_or_default();
    if message.trim().is_empty() {
        return None;
    }
    Some(ServerLogLine {
        ts_ms,
        priority,
        message,
    })
}

pub async fn query_unit_logs(
    unit_name: &str,
    limit: u32,
) -> Result<ServersAgentLogsResponse, String> {
    ensure_native_runtime_supported()?;
    let bounded_limit = limit.clamp(1, 500);
    let output = Command::new(journalctl_bin())
        .arg("-u")
        .arg(unit_name)
        .arg("--no-pager")
        .arg("-n")
        .arg(bounded_limit.to_string())
        .arg("-o")
        .arg("json")
        .arg("--output-fields=__REALTIME_TIMESTAMP,MESSAGE,PRIORITY")
        .output()
        .await
        .map_err(|error| format!("failed to query journalctl logs: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("journalctl exited with {}", output.status)
        };
        return Err(detail);
    }

    let lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_journal_line)
        .collect();

    Ok(ServersAgentLogsResponse {
        unit_name: unit_name.to_string(),
        lines,
    })
}

fn allowed_scan_root(requested: Option<&str>) -> Result<(Vec<PathBuf>, Option<PathBuf>), String> {
    let roots = import_roots();
    if roots.is_empty() {
        return Err("no import roots are configured on this host".to_string());
    }

    if let Some(requested_root) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        let canonical = std::fs::canonicalize(requested_root)
            .map_err(|error| format!("failed to resolve requested discovery root: {error}"))?;
        if !canonical.is_dir() {
            return Err("requested discovery root is not a directory".to_string());
        }
        if !roots.iter().any(|root| canonical.starts_with(root)) {
            return Err("requested discovery root is outside configured import roots".to_string());
        }
        Ok((vec![canonical.clone()], Some(canonical)))
    } else {
        Ok((roots, None))
    }
}

fn parse_server_properties_file(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|content| parse_properties(&content))
        .unwrap_or_default()
}

fn should_skip_scan_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "logs"
            | "plugins"
            | "mods"
            | "libraries"
            | "versions"
            | "cache"
            | "tmp"
            | "world"
            | "world_nether"
            | "world_the_end"
            | "session.lock"
    )
}

fn top_level_jar_names(path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return names;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        if entry_path.extension().and_then(|ext| ext.to_str()) != Some("jar") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().trim().to_string();
        if file_name.is_empty() {
            continue;
        }
        names.push(file_name);
    }
    names.sort();
    names
}

fn build_discovery_candidate(path: &Path) -> Option<DiscoveryCandidate> {
    let server_props_path = path.join("server.properties");
    let server_properties_present = server_props_path.is_file();
    let top_level_jars = top_level_jar_names(path);
    if !server_properties_present && top_level_jars.is_empty() {
        return None;
    }

    let properties = parse_server_properties_file(&server_props_path);
    let last_modified_ts = std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64);

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    Some(DiscoveryCandidate {
        path: path.to_string_lossy().into_owned(),
        name,
        world_name: properties.get("level-name").cloned(),
        motd: properties.get("motd").cloned(),
        server_properties_present,
        eula_present: path.join("eula.txt").is_file(),
        top_level_jars,
        last_modified_ts,
    })
}

pub async fn scan_discovery_candidates(
    requested_root: Option<String>,
    limit: u32,
) -> Result<ServersAgentDiscoveryScanResponse, String> {
    ensure_native_runtime_supported()?;
    let bounded_limit = limit.clamp(1, 200) as usize;
    let requested_root = requested_root.clone();
    tokio::task::spawn_blocking(move || {
        let (scan_roots, scanned_root) = allowed_scan_root(requested_root.as_deref())?;
        let managed_root = managed_instances_root();
        let mut visited = HashSet::new();
        let mut candidates = Vec::new();
        let mut queue = VecDeque::new();
        let max_depth = discovery_max_depth();

        for root in &scan_roots {
            queue.push_back((root.clone(), 0usize));
        }

        while let Some((dir, depth)) = queue.pop_front() {
            if candidates.len() >= bounded_limit {
                break;
            }
            let Ok(canonical) = std::fs::canonicalize(&dir) else {
                continue;
            };
            if !visited.insert(canonical.clone()) {
                continue;
            }
            if let Some(managed_root) = managed_root.as_ref() {
                if canonical.starts_with(managed_root) {
                    continue;
                }
            }

            if let Some(candidate) = build_discovery_candidate(&canonical) {
                candidates.push(candidate);
                continue;
            }

            if depth >= max_depth {
                continue;
            }

            let Ok(entries) = std::fs::read_dir(&canonical) else {
                continue;
            };
            for entry in entries.flatten() {
                let child_path = entry.path();
                if !child_path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().trim().to_string();
                if name.is_empty() || should_skip_scan_dir(&name) {
                    continue;
                }
                queue.push_back((child_path, depth + 1));
            }
        }

        candidates.sort_by(|left, right| {
            right
                .last_modified_ts
                .cmp(&left.last_modified_ts)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });

        Ok(ServersAgentDiscoveryScanResponse {
            roots: scan_roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect(),
            scanned_root: scanned_root.map(|root| root.to_string_lossy().into_owned()),
            candidates,
        })
    })
    .await
    .map_err(|error| format!("discovery scan task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedProvisionSpec, ServerLifecycleAction, build_discovery_candidate,
        desired_server_properties, parse_journal_line, parse_unit_status_stdout,
        render_server_properties, render_systemd_unit,
    };
    use std::io::Write;

    fn sample_spec() -> ManagedProvisionSpec {
        ManagedProvisionSpec {
            instance_id: "instance-1".to_string(),
            display_name: "Family SMP".to_string(),
            install_mode: "managed".to_string(),
            instance_root: "/srv/rustyfin-servers/minecraft/instances/instance-1".to_string(),
            server_work_dir: "/srv/rustyfin-servers/minecraft/instances/instance-1/server"
                .to_string(),
            systemd_unit_name: "rustyfin-minecraft-instance-1.service".to_string(),
            listen_host: "0.0.0.0".to_string(),
            listen_port: 25565,
            autostart: false,
            server_distribution: "paper".to_string(),
            minecraft_version: "1.21.1".to_string(),
            java_path: "/usr/bin/java".to_string(),
            world_name: "family-world".to_string(),
            gamemode: "survival".to_string(),
            difficulty: "normal".to_string(),
            hardcore: false,
            motd: "Welcome".to_string(),
            min_memory_mb: 1024,
            max_memory_mb: 4096,
            online_mode: true,
            pvp: true,
            allow_flight: false,
            enable_command_block: false,
            white_list_enabled: false,
        }
    }

    #[test]
    fn parse_action_names() {
        assert_eq!(
            ServerLifecycleAction::parse("start"),
            Some(ServerLifecycleAction::Start)
        );
        assert_eq!(
            ServerLifecycleAction::parse("STOP"),
            Some(ServerLifecycleAction::Stop)
        );
        assert_eq!(
            ServerLifecycleAction::parse("restart"),
            Some(ServerLifecycleAction::Restart)
        );
        assert_eq!(ServerLifecycleAction::parse("launch"), None);
    }

    #[test]
    fn parse_systemd_show_output() {
        let status = parse_unit_status_stdout(
            "LoadState=loaded\nActiveState=active\nSubState=running\nUnitFileState=enabled\nResult=success\nExecMainStatus=0\n",
        );

        assert_eq!(status.load_state, "loaded");
        assert_eq!(status.active_state, "active");
        assert_eq!(status.sub_state, "running");
        assert_eq!(status.unit_file_state, "enabled");
        assert_eq!(status.result, "success");
        assert_eq!(status.exec_main_status, Some(0));
    }

    #[test]
    fn server_properties_render_replaces_existing_managed_fields() {
        let desired = desired_server_properties(&sample_spec());
        let rendered = render_server_properties(
            Some("motd=Old\nserver-port=25570\nview-distance=12\n"),
            &desired,
        );
        assert!(rendered.contains("motd=Welcome"));
        assert!(rendered.contains("server-port=25565"));
        assert!(rendered.contains("view-distance=12"));
        assert!(rendered.contains("level-name=family-world"));
    }

    #[test]
    fn systemd_unit_render_includes_java_and_memory_settings() {
        let rendered = render_systemd_unit(&sample_spec());
        assert!(rendered.contains(
            "WorkingDirectory=/srv/rustyfin-servers/minecraft/instances/instance-1/server"
        ));
        assert!(
            rendered.contains("ExecStart=/usr/bin/java -Xms1024M -Xmx4096M -jar server.jar nogui")
        );
        assert!(rendered.contains("ExecStop=/bin/kill -s INT $MAINPID"));
    }

    #[test]
    fn parse_journal_json_line() {
        let parsed = parse_journal_line(
            r#"{"__REALTIME_TIMESTAMP":"1710001000000","MESSAGE":"server started","PRIORITY":"6"}"#,
        )
        .expect("journal line should parse");
        assert_eq!(parsed.ts_ms, Some(1710001000));
        assert_eq!(parsed.priority.as_deref(), Some("6"));
        assert_eq!(parsed.message, "server started");
    }

    #[test]
    fn discovery_candidate_reads_server_properties() {
        let path = std::env::temp_dir().join(format!(
            "rustyfin-servers-host-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("mkdir");
        std::fs::write(
            path.join("server.properties"),
            "level-name=family-world\nmotd=Welcome\n",
        )
        .expect("props");
        let mut jar = std::fs::File::create(path.join("paper.jar")).expect("jar");
        jar.write_all(b"jar").expect("write jar");

        let candidate = build_discovery_candidate(&path).expect("candidate");
        assert_eq!(candidate.world_name.as_deref(), Some("family-world"));
        assert_eq!(candidate.motd.as_deref(), Some("Welcome"));
        assert_eq!(candidate.top_level_jars, vec!["paper.jar".to_string()]);
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn normalize_probe_host_rewrites_wildcards() {
        assert_eq!(super::normalize_probe_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(super::normalize_probe_host("::"), "127.0.0.1");
        assert_eq!(super::normalize_probe_host("192.168.0.10"), "192.168.0.10");
    }

    #[test]
    fn flatten_description_supports_nested_chat_components() {
        let value: serde_json::Value = serde_json::json!({
            "text": "Welcome ",
            "extra": [
                {"text": "to "},
                {"text": "Rustyfin"}
            ]
        });
        assert_eq!(super::flatten_description(&value), "Welcome to Rustyfin");
    }

    #[test]
    fn delete_root_validation_rejects_base_root() {
        let root = std::path::PathBuf::from("/srv/rustyfin-servers/minecraft/instances");
        let error = super::validate_managed_instance_root(&root).expect_err("base root must fail");
        assert!(error.contains("base root"));
    }

    #[test]
    fn delete_root_validation_accepts_child_instance_root() {
        let root = std::path::PathBuf::from("/srv/rustyfin-servers/minecraft/instances/instance-1");
        super::validate_managed_instance_root(&root).expect("child instance root should pass");
    }

    #[test]
    fn missing_unit_error_detection_catches_common_systemd_messages() {
        assert!(super::is_missing_unit_error(
            "Unit rustyfin-minecraft-123.service could not be found."
        ));
        assert!(super::is_missing_unit_error(
            "Unit rustyfin.service not loaded."
        ));
        assert!(super::is_missing_unit_error("No such file or directory"));
    }

    #[test]
    fn systemctl_unavailable_detection_catches_launch_and_bus_failures() {
        assert!(super::is_systemctl_unavailable_error(
            "failed to launch systemctl daemon-reload: No such file or directory (os error 2)"
        ));
        assert!(super::is_systemctl_unavailable_error(
            "System has not been booted with systemd as init system (PID 1). Can't operate."
        ));
        assert!(super::is_systemctl_unavailable_error(
            "Failed to connect to bus: Host is down"
        ));
    }
}
