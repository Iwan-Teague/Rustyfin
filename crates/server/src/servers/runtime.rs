use std::collections::HashMap;

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

fn systemctl_bin() -> String {
    std::env::var("RUSTFIN_SERVERS_SYSTEMCTL_BIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "systemctl".to_string())
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

pub async fn run_lifecycle_action(
    unit_name: &str,
    action: ServerLifecycleAction,
) -> Result<(), String> {
    ensure_native_runtime_supported()?;
    let output = Command::new(systemctl_bin())
        .arg(action.as_str())
        .arg(unit_name)
        .output()
        .await
        .map_err(|error| format!("failed to launch systemctl {}: {error}", action.as_str()))?;

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
                "systemctl {} exited with {}",
                action.as_str(),
                output.status
            )
        };
        Err(detail)
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

#[cfg(test)]
mod tests {
    use super::{ServerLifecycleAction, parse_unit_status_stdout};

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
}
