use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdUnitStatus {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub unit_file_state: String,
    pub result: String,
    pub exec_main_status: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProvisionSpec {
    pub managed: ManagedProvisionSpec,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningResult {
    pub install_mode: String,
    pub unit_path: String,
    pub work_dir: String,
    pub server_jar_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentStatusRequest {
    pub unit_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentProbeRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentLifecycleRequest {
    pub unit_name: String,
    pub action: ServerLifecycleAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentProvisionRequest {
    pub spec: ManagedProvisionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentImportRequest {
    pub spec: ImportProvisionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentDeleteRequest {
    pub unit_name: String,
    pub instance_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentLogsRequest {
    pub unit_name: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentDiscoveryScanRequest {
    pub root_path: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentAckResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftServerProbe {
    pub version_name: Option<String>,
    pub protocol_version: Option<i32>,
    pub online_players: i64,
    pub max_players: Option<i64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerLogLine {
    pub ts_ms: Option<i64>,
    pub priority: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentLogsResponse {
    pub unit_name: String,
    pub lines: Vec<ServerLogLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryCandidate {
    pub path: String,
    pub name: String,
    pub world_name: Option<String>,
    pub motd: Option<String>,
    pub server_properties_present: bool,
    pub eula_present: bool,
    pub top_level_jars: Vec<String>,
    pub last_modified_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServersAgentDiscoveryScanResponse {
    pub roots: Vec<String>,
    pub scanned_root: Option<String>,
    pub candidates: Vec<DiscoveryCandidate>,
}
