use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerLifecycleAction {
    Start,
    Stop,
    Restart,
}

impl ServerLifecycleAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    pub fn desired_state(self) -> &'static str {
        match self {
            Self::Start | Self::Restart => "running",
            Self::Stop => "stopped",
        }
    }

    pub fn transitional_observed_state(self) -> &'static str {
        match self {
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Restart => "restarting",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemdUnitStatus {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub unit_file_state: String,
    pub result: String,
    pub exec_main_status: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ManagedProvisionSpec {
    pub instance_id: String,
    pub display_name: String,
    pub install_mode: String,
    pub instance_root: String,
    pub server_work_dir: String,
    pub systemd_unit_name: String,
    pub listen_host: String,
    pub listen_port: i64,
    pub autostart: bool,
    pub server_distribution: String,
    pub minecraft_version: String,
    pub java_path: String,
    pub world_name: String,
    pub gamemode: String,
    pub difficulty: String,
    pub hardcore: bool,
    pub motd: String,
    pub min_memory_mb: i64,
    pub max_memory_mb: i64,
    pub online_mode: bool,
    pub pvp: bool,
    pub allow_flight: bool,
    pub enable_command_block: bool,
    pub white_list_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ImportProvisionSpec {
    pub managed: ManagedProvisionSpec,
    pub source_path: String,
}

#[derive(Debug, Clone)]
pub struct ProvisioningResult {
    pub install_mode: String,
    pub unit_path: String,
    pub work_dir: String,
    pub server_jar_path: String,
}

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

fn systemctl_bin() -> String {
    std::env::var("RUSTFIN_SERVERS_SYSTEMCTL_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "systemctl".to_string())
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

fn ensure_native_runtime_supported() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("Minecraft native runtime control is only supported on Linux hosts".to_string())
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

fn parse_unit_status_stdout(stdout: &str) -> Result<SystemdUnitStatus, String> {
    let properties = parse_properties(stdout);
    let load_state = properties
        .get("LoadState")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let active_state = properties
        .get("ActiveState")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let sub_state = properties
        .get("SubState")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let unit_file_state = properties
        .get("UnitFileState")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let result = properties
        .get("Result")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());
    let exec_main_status = properties
        .get("ExecMainStatus")
        .and_then(|value| value.parse::<i64>().ok());

    Ok(SystemdUnitStatus {
        load_state,
        active_state,
        sub_state,
        unit_file_state,
        result,
        exec_main_status,
    })
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
        parse_unit_status_stdout(&stdout)
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
    unit.push_str("ExecStop=/bin/kill -s INT $MAINPID\n");
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
    std::fs::write(&unit_path, unit_text).map_err(|error| {
        format!(
            "failed to write systemd unit {}: {error}",
            unit_path.display()
        )
    })?;
    std::fs::write(
        meta_dir.join("rendered-unit.service"),
        render_systemd_unit(spec),
    )
    .map_err(|error| {
        format!(
            "failed to write rendered-unit.service in {}: {error}",
            meta_dir.display()
        )
    })?;

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
    let spec = spec.clone();
    tokio::task::spawn_blocking(move || {
        let existing_props_path = PathBuf::from(&spec.server_work_dir).join("server.properties");
        let existing_props = std::fs::read_to_string(&existing_props_path).ok();
        write_runtime_files(&spec, existing_props.as_deref(), &artifact_path)
    })
    .await
    .map_err(|error| format!("managed provisioning task failed: {error}"))?
}

pub async fn import_existing_instance(
    spec: &ImportProvisionSpec,
) -> Result<ProvisioningResult, String> {
    ensure_native_runtime_supported()?;
    let spec = spec.clone();
    tokio::task::spawn_blocking(move || {
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
    .map_err(|error| format!("import task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedProvisionSpec, ServerLifecycleAction, desired_server_properties,
        parse_unit_status_stdout, render_server_properties, render_systemd_unit,
    };

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
        )
        .expect("status should parse");

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
}
