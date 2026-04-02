use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAddressSummary {
    pub family: String,
    pub address: String,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkNodeSummary {
    pub name: String,
    pub status: String,
    pub is_loopback: bool,
    pub addresses: Vec<NetworkAddressSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustyfinNetworkAccess {
    pub ui_port: u16,
    pub backend_port: u16,
    pub calendar_port: u16,
    pub preferred_local_interface: Option<String>,
    pub preferred_local_ipv4: Option<String>,
    pub preferred_local_url: Option<String>,
    pub login_url: Option<String>,
    pub ai_url: Option<String>,
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTopologySnapshot {
    pub available: bool,
    pub reason: Option<String>,
    pub host_label: Option<String>,
    pub public_host: Option<String>,
    pub access: RustyfinNetworkAccess,
    pub remote_access_enabled: bool,
    pub trusted_proxy_count: usize,
    pub trusted_proxies: Option<Vec<String>>,
    pub online_node_count: usize,
    pub offline_node_count: usize,
    pub loopback_node_count: usize,
    pub nodes: Vec<NetworkNodeSummary>,
}

impl NetworkTopologySnapshot {
    fn unavailable(
        reason: impl Into<String>,
        host_label: Option<String>,
        public_host: Option<String>,
        access: RustyfinNetworkAccess,
        remote_access_enabled: bool,
        trusted_proxies: Vec<String>,
        include_admin_details: bool,
    ) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
            host_label,
            public_host,
            access,
            remote_access_enabled,
            trusted_proxy_count: trusted_proxies.len(),
            trusted_proxies: include_admin_details.then_some(trusted_proxies),
            online_node_count: 0,
            offline_node_count: 0,
            loopback_node_count: 0,
            nodes: Vec::new(),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct IpAddressShowRow {
    ifname: String,
    #[serde(default)]
    operstate: Option<String>,
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    addr_info: Vec<IpAddressInfo>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Deserialize)]
struct IpAddressInfo {
    family: String,
    local: String,
    #[serde(default)]
    scope: Option<String>,
}

pub async fn collect_network_topology_snapshot(
    state: &AppState,
    include_admin_details: bool,
) -> NetworkTopologySnapshot {
    let ui_port = env_u16("RUSTFIN_UI_PORT", 3000);
    let backend_port = env_u16("RUSTFIN_BACKEND_PORT", 8096);
    let calendar_port = env_u16("RUSTFIN_CALENDAR_PORT", 8099);
    let remote_access_enabled = rustfin_db::repo::settings::get(&state.db, "allow_remote_access")
        .await
        .ok()
        .flatten()
        .is_some_and(|value| value == "true");
    let trusted_proxies = rustfin_db::repo::settings::get(&state.db, "trusted_proxies")
        .await
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();

    let host_label = detect_host_label();
    let public_host = std::env::var("RUSTFIN_PUBLIC_HOST")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    #[cfg(target_os = "linux")]
    {
        match collect_linux_network_nodes().await {
            Ok(nodes) => {
                let access = build_rustyfin_network_access(
                    &nodes,
                    public_host.as_deref(),
                    ui_port,
                    backend_port,
                    calendar_port,
                );
                let online_node_count = nodes.iter().filter(|node| node.status == "online").count();
                let offline_node_count =
                    nodes.iter().filter(|node| node.status == "offline").count();
                let loopback_node_count = nodes
                    .iter()
                    .filter(|node| node.status == "loopback")
                    .count();
                NetworkTopologySnapshot {
                    available: true,
                    reason: None,
                    host_label,
                    public_host,
                    access,
                    remote_access_enabled,
                    trusted_proxy_count: trusted_proxies.len(),
                    trusted_proxies: include_admin_details.then_some(trusted_proxies),
                    online_node_count,
                    offline_node_count,
                    loopback_node_count,
                    nodes,
                }
            }
            Err(error) => {
                let access = build_rustyfin_network_access(
                    &[],
                    public_host.as_deref(),
                    ui_port,
                    backend_port,
                    calendar_port,
                );
                NetworkTopologySnapshot::unavailable(
                    error,
                    host_label,
                    public_host,
                    access,
                    remote_access_enabled,
                    trusted_proxies,
                    include_admin_details,
                )
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let access = build_rustyfin_network_access(
            &[],
            public_host.as_deref(),
            ui_port,
            backend_port,
            calendar_port,
        );
        NetworkTopologySnapshot::unavailable(
            "Host network topology is only available on Linux hosts.",
            host_label,
            public_host,
            access,
            remote_access_enabled,
            trusted_proxies,
            include_admin_details,
        )
    }
}

fn detect_host_label() -> Option<String> {
    if let Ok(value) = std::env::var("HOSTNAME") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    let hostname_path = Path::new("/etc/hostname");
    std::fs::read_to_string(hostname_path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u16(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(default)
}

fn build_rustyfin_network_access(
    nodes: &[NetworkNodeSummary],
    public_host: Option<&str>,
    ui_port: u16,
    backend_port: u16,
    calendar_port: u16,
) -> RustyfinNetworkAccess {
    let preferred_local_access = preferred_local_access_candidate(nodes);
    let preferred_local_interface = preferred_local_access
        .as_ref()
        .map(|candidate| candidate.interface.clone());
    let preferred_local_ipv4 = preferred_local_access.map(|candidate| candidate.address);
    let preferred_local_url = preferred_local_ipv4
        .as_deref()
        .map(|ip| format!("https://{ip}:{ui_port}"));
    let login_url = preferred_local_url
        .as_deref()
        .map(|base| format!("{base}/login"));
    let ai_url = preferred_local_url
        .as_deref()
        .map(|base| format!("{base}/ai"));
    let public_url = public_host
        .filter(|host| should_expose_public_host(host, preferred_local_ipv4.as_deref()))
        .map(|host| format!("https://{host}:{ui_port}"));

    RustyfinNetworkAccess {
        ui_port,
        backend_port,
        calendar_port,
        preferred_local_interface,
        preferred_local_ipv4,
        preferred_local_url,
        login_url,
        ai_url,
        public_url,
    }
}

#[derive(Debug, Clone)]
struct PreferredLocalAccessCandidate {
    interface: String,
    address: String,
}

#[cfg(test)]
fn preferred_local_ipv4(nodes: &[NetworkNodeSummary]) -> Option<String> {
    preferred_local_access_candidate(nodes).map(|candidate| candidate.address)
}

fn preferred_local_access_candidate(
    nodes: &[NetworkNodeSummary],
) -> Option<PreferredLocalAccessCandidate> {
    let mut best: Option<(u8, u8, &str, Ipv4Addr)> = None;

    for node in nodes
        .iter()
        .filter(|node| node.status == "online" && !node.is_loopback)
    {
        for address in node.addresses.iter().filter(|address| {
            address.family == "inet" && !matches!(address.scope.as_deref(), Some("host" | "link"))
        }) {
            let Ok(ip) = address.address.parse::<Ipv4Addr>() else {
                continue;
            };
            if ip.is_loopback() {
                continue;
            }

            let candidate = (
                interface_preference_rank(&node.name),
                ipv4_preference_rank(ip),
                node.name.as_str(),
                ip,
            );
            let should_replace = best.as_ref().is_none_or(|current| candidate < *current);
            if should_replace {
                best = Some(candidate);
            }
        }
    }

    best.map(|(_, _, interface, ip)| PreferredLocalAccessCandidate {
        interface: interface.to_string(),
        address: ip.to_string(),
    })
}

fn should_expose_public_host(public_host: &str, preferred_local_ipv4: Option<&str>) -> bool {
    let trimmed = public_host.trim();
    if trimmed.is_empty() {
        return false;
    }
    if preferred_local_ipv4.is_some_and(|preferred| preferred == trimmed) {
        return false;
    }

    match trimmed.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => !ip.is_private() && !ip.is_loopback() && !ip.is_link_local(),
        Ok(IpAddr::V6(ip)) => !ip.is_loopback() && !ip.is_unicast_link_local(),
        Err(_) => true,
    }
}

fn interface_preference_rank(name: &str) -> u8 {
    if is_preferred_lan_interface_name(name) {
        0
    } else if is_overlay_interface_name(name) {
        2
    } else if is_container_or_virtual_interface_name(name) {
        3
    } else {
        1
    }
}

fn ipv4_preference_rank(ip: Ipv4Addr) -> u8 {
    if ip.is_private() {
        0
    } else if ip.is_link_local() {
        2
    } else {
        1
    }
}

fn is_preferred_lan_interface_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("en")
        || lower.starts_with("eth")
        || lower.starts_with("wl")
        || lower.starts_with("wwan")
        || lower.starts_with("bond")
}

fn is_overlay_interface_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("tailscale")
        || lower.starts_with("wg")
        || lower.starts_with("tun")
        || lower.starts_with("tap")
        || lower.starts_with("zt")
}

fn is_container_or_virtual_interface_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "docker0"
        || lower.starts_with("br-")
        || lower.starts_with("veth")
        || lower.starts_with("virbr")
        || lower.starts_with("cni")
        || lower.starts_with("flannel")
        || lower.starts_with("podman")
        || lower.starts_with("vboxnet")
        || lower.starts_with("vmnet")
}

#[cfg(target_os = "linux")]
async fn collect_linux_network_nodes() -> Result<Vec<NetworkNodeSummary>, String> {
    use tokio::process::Command;

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        Command::new("ip")
            .args(["-json", "address", "show"])
            .output(),
    )
    .await
    .map_err(|_| "timed out while reading host network interfaces".to_string())?
    .map_err(|error| format!("failed to run `ip -json address show`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("`ip -json address show` exited with {}", output.status)
        } else {
            format!("`ip -json address show` failed: {stderr}")
        });
    }

    let rows = parse_linux_network_rows(&output.stdout)?;
    Ok(build_network_nodes_from_rows(rows))
}

#[cfg(target_os = "linux")]
fn parse_linux_network_rows(stdout: &[u8]) -> Result<Vec<IpAddressShowRow>, String> {
    serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse host network interface data: {error}"))
}

#[cfg(target_os = "linux")]
fn build_network_nodes_from_rows(rows: Vec<IpAddressShowRow>) -> Vec<NetworkNodeSummary> {
    let mut nodes = rows
        .into_iter()
        .map(|row| {
            let addresses = row
                .addr_info
                .into_iter()
                .filter(|address| matches!(address.family.as_str(), "inet" | "inet6"))
                .map(|address| NetworkAddressSummary {
                    family: address.family,
                    address: address.local,
                    scope: address.scope,
                })
                .collect::<Vec<_>>();
            let is_loopback = row.flags.iter().any(|flag| flag == "LOOPBACK")
                || row.ifname == "lo"
                || addresses
                    .iter()
                    .all(|address| is_loopback_address(&address.address));
            let status =
                classify_network_node_status(row.operstate.as_deref(), is_loopback, &addresses);

            NetworkNodeSummary {
                name: row.ifname,
                status: status.to_string(),
                is_loopback,
                addresses,
            }
        })
        .collect::<Vec<_>>();

    nodes.sort_by(|left, right| {
        status_rank(&left.status)
            .cmp(&status_rank(&right.status))
            .then_with(|| left.name.cmp(&right.name))
    });
    nodes
}

#[cfg(target_os = "linux")]
fn classify_network_node_status(
    operstate: Option<&str>,
    is_loopback: bool,
    addresses: &[NetworkAddressSummary],
) -> &'static str {
    if is_loopback {
        return "loopback";
    }

    let operstate = operstate.unwrap_or_default();
    if operstate.eq_ignore_ascii_case("UP")
        || operstate.eq_ignore_ascii_case("UNKNOWN") && !addresses.is_empty()
    {
        "online"
    } else if !addresses.is_empty() {
        "online"
    } else {
        "offline"
    }
}

#[cfg(target_os = "linux")]
fn status_rank(status: &str) -> u8 {
    match status {
        "online" => 0,
        "offline" => 1,
        "loopback" => 2,
        _ => 3,
    }
}

#[cfg(target_os = "linux")]
fn is_loopback_address(address: &str) -> bool {
    address == "127.0.0.1" || address == "::1"
}

#[cfg(test)]
mod tests {
    use super::NetworkTopologySnapshot;

    #[cfg(target_os = "linux")]
    use super::{
        IpAddressShowRow, NetworkAddressSummary, NetworkNodeSummary,
        classify_network_node_status, is_loopback_address, parse_linux_network_rows,
        preferred_local_ipv4, status_rank,
    };

    #[test]
    fn unavailable_snapshot_hides_trusted_proxies_for_non_admins() {
        let snapshot = NetworkTopologySnapshot::unavailable(
            "network diagnostics unavailable",
            Some("rustyfin-host".to_string()),
            Some("rustyfin.example".to_string()),
            super::build_rustyfin_network_access(&[], Some("rustyfin.example"), 3008, 8097, 8099),
            true,
            vec!["10.0.0.10".to_string(), "10.0.0.11".to_string()],
            false,
        );
        assert!(!snapshot.available);
        assert_eq!(snapshot.trusted_proxy_count, 2);
        assert_eq!(snapshot.trusted_proxies, None);
        assert_eq!(
            snapshot.reason.as_deref(),
            Some("network diagnostics unavailable")
        );
    }

    #[test]
    fn unavailable_snapshot_exposes_trusted_proxies_for_admins() {
        let snapshot = NetworkTopologySnapshot::unavailable(
            "network diagnostics unavailable",
            Some("rustyfin-host".to_string()),
            Some("rustyfin.example".to_string()),
            super::build_rustyfin_network_access(&[], Some("rustyfin.example"), 3008, 8097, 8099),
            true,
            vec!["10.0.0.10".to_string(), "10.0.0.11".to_string()],
            true,
        );
        assert!(!snapshot.available);
        assert_eq!(snapshot.trusted_proxy_count, 2);
        assert_eq!(
            snapshot.trusted_proxies,
            Some(vec!["10.0.0.10".to_string(), "10.0.0.11".to_string()])
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn loopback_addresses_are_detected() {
        assert!(is_loopback_address("127.0.0.1"));
        assert!(is_loopback_address("::1"));
        assert!(!is_loopback_address("192.168.1.5"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn classify_network_node_status_prefers_loopback() {
        let status = classify_network_node_status(None, true, &[]);
        assert_eq!(status, "loopback");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn classify_network_node_status_marks_up_nodes_online() {
        let addresses = vec![NetworkAddressSummary {
            family: "inet".to_string(),
            address: "192.168.1.5".to_string(),
            scope: Some("global".to_string()),
        }];
        let status = classify_network_node_status(Some("UP"), false, &addresses);
        assert_eq!(status, "online");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn status_rank_orders_online_before_offline() {
        assert!(status_rank("online") < status_rank("offline"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ip_json_shape_parses_expected_fields() {
        let rows: Vec<IpAddressShowRow> = serde_json::from_str(
            r#"[{"ifname":"eth0","operstate":"UP","flags":["BROADCAST","UP"],"addr_info":[{"family":"inet","local":"192.168.1.2","scope":"global"}]}]"#,
        )
        .expect("expected interface JSON to parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ifname, "eth0");
        assert_eq!(rows[0].addr_info[0].local, "192.168.1.2");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn invalid_ip_json_reports_parse_failure() {
        let error =
            parse_linux_network_rows(br#"{"not":"an array"}"#).expect_err("expected parse failure");
        assert!(error.contains("failed to parse host network interface data"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preferred_local_ipv4_prefers_private_online_address() {
        let ip = preferred_local_ipv4(&[
            NetworkNodeSummary {
                name: "eth0".to_string(),
                status: "online".to_string(),
                is_loopback: false,
                addresses: vec![NetworkAddressSummary {
                    family: "inet".to_string(),
                    address: "203.0.113.44".to_string(),
                    scope: Some("global".to_string()),
                }],
            },
            NetworkNodeSummary {
                name: "enp3s0".to_string(),
                status: "online".to_string(),
                is_loopback: false,
                addresses: vec![NetworkAddressSummary {
                    family: "inet".to_string(),
                    address: "192.168.0.36".to_string(),
                    scope: Some("global".to_string()),
                }],
            },
        ]);
        assert_eq!(ip.as_deref(), Some("192.168.0.36"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn preferred_local_ipv4_ignores_container_bridges_when_lan_nic_exists() {
        let ip = preferred_local_ipv4(&[
            NetworkNodeSummary {
                name: "br-76e2dd24505e".to_string(),
                status: "online".to_string(),
                is_loopback: false,
                addresses: vec![NetworkAddressSummary {
                    family: "inet".to_string(),
                    address: "192.168.112.1".to_string(),
                    scope: Some("global".to_string()),
                }],
            },
            NetworkNodeSummary {
                name: "tailscale0".to_string(),
                status: "online".to_string(),
                is_loopback: false,
                addresses: vec![NetworkAddressSummary {
                    family: "inet".to_string(),
                    address: "100.123.146.3".to_string(),
                    scope: Some("global".to_string()),
                }],
            },
            NetworkNodeSummary {
                name: "enp5s0".to_string(),
                status: "online".to_string(),
                is_loopback: false,
                addresses: vec![NetworkAddressSummary {
                    family: "inet".to_string(),
                    address: "192.168.0.36".to_string(),
                    scope: Some("global".to_string()),
                }],
            },
        ]);

        assert_eq!(ip.as_deref(), Some("192.168.0.36"));
    }
}
