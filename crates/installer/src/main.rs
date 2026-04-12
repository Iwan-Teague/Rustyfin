use anyhow::{Context, bail};
use chrono::Utc;
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

mod distro;
mod utils;

use crate::utils::{
    HostPlatform, NativeUserContext, command_exists, default_native_linux_target,
    detect_default_ai_backend, detect_host_platform, detect_native_user_context,
    ensure_command_available, ensure_success, resolve_ai_gpu_backend, resolve_command_path,
    run_as_native_user_shell, run_command_capture, run_command_in_dir_as_user,
    run_command_in_dir_as_user_capture, run_root_command, run_root_command_allow_failure,
    run_root_command_capture, run_script, run_script_as_repo_owner, server_features_for_ai_backend,
    stat_value,
};

const SERVERS_AGENT_BIND: &str = "127.0.0.1:8103";
const SERVERS_AGENT_URL: &str = "http://127.0.0.1:8103";
const SERVERS_DEFAULT_JAVA: &str = "/opt/rustyfin/java/current/bin/java";
const ENV_DIR: &str = "/etc/rustyfin";
const ENV_FILE: &str = "/etc/rustyfin/servers-agent.env";
const RUNTIME_DEFAULTS_FILE: &str = "/etc/rustyfin/native-runtime.defaults.sh";
const INSTALL_MANIFEST_PATH: &str = "/var/lib/rustyfin/install-manifest.json";
const MANAGED_JAVA_ROOT: &str = "/opt/rustyfin/java";
const MANAGED_JAVA_CURRENT: &str = "/opt/rustyfin/java/current";
const MANAGED_JAVA_INSTALL_DIR: &str = "/opt/rustyfin/java/temurin-21";
const DEFAULT_AI_MODEL_DIR: &str = "/var/lib/rustyfin/ai/models";
const DEFAULT_BOOTSTRAP_AI_MODEL_URL: &str = "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf";
const DEFAULT_BOOTSTRAP_GEMMA_4_E2B_MODEL_URL: &str =
    "https://huggingface.co/gguf-org/gemma-4-e2b-it-gguf/resolve/main/gemma-4-e2b-it-edited-q4_0.gguf";
const DEFAULT_BOOTSTRAP_GEMMA_4_E4B_MODEL_URL: &str =
    "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q3_K_M.gguf";
const AI_BOOTSTRAP_MODEL_ENV: &str = "RUSTFIN_AI_BOOTSTRAP_MODEL";
const AI_BOOTSTRAP_MODEL_URL_ENV: &str = "RUSTFIN_AI_BOOTSTRAP_MODEL_URL";
const AI_BOOTSTRAP_MODEL_FILE_ENV: &str = "RUSTFIN_AI_BOOTSTRAP_MODEL_FILE";

const DIRECTORY_PICKER_HELPER_SCRIPT: &str = r#"#!/usr/bin/env python3
import json
import os
import platform
import shutil
import subprocess
from http.server import BaseHTTPRequestHandler, HTTPServer

HOST = os.environ.get("RUSTFIN_PICKER_HELPER_HOST", "127.0.0.1")
PORT = int(os.environ.get("RUSTFIN_PICKER_HELPER_PORT", "43110"))

def pick_directory():
    system = platform.system()
    if system == "Linux":
        if shutil.which("zenity"):
            out = subprocess.run(
                ["zenity", "--file-selection", "--directory", "--title=Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "zenity folder picker failed")
        if shutil.which("kdialog"):
            out = subprocess.run(
                ["kdialog", "--getexistingdirectory", ".", "Select a media directory for Rustyfin"],
                capture_output=True,
                text=True,
            )
            if out.returncode == 0:
                return (out.stdout or "").strip()
            if out.returncode == 1:
                return ""
            raise RuntimeError((out.stderr or "").strip() or "kdialog folder picker failed")
        raise RuntimeError("no supported Linux picker found (install zenity or kdialog)")
    raise RuntimeError(f"unsupported host OS for picker helper: {system}")

class Handler(BaseHTTPRequestHandler):
    def _write_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._write_json(200, {"ok": True})
        else:
            self._write_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/pick":
            self._write_json(404, {"error": "not found"})
            return
        try:
            selected = pick_directory()
            if not selected:
                self._write_json(400, {"error": "directory selection cancelled"})
                return
            self._write_json(200, {"path": selected})
        except Exception as exc:
            self._write_json(500, {"error": str(exc)})

    def log_message(self, format, *args):
        return

def main():
    server = HTTPServer((HOST, PORT), Handler)
    server.serve_forever()

if __name__ == "__main__":
    main()
"#;

#[derive(Debug, Clone)]
enum CliCommand {
    Install(InstallOptions),
    InstallNativeSystemd,
    BuildNativeBinaries(BuildNativeBinariesOptions),
    BuildNativeRuntimeArtifacts(BuildNativeRuntimeArtifactsOptions),
    PlanNativeRuntime(PlanNativeRuntimeOptions),
    DeployNative(DeployNativeOptions),
    WriteNativeRuntimeSnapshot(WriteNativeRuntimeSnapshotOptions),
    LaunchNativeRuntime(LaunchNativeRuntimeOptions),
    StopNativeRuntime,
    CleanNativeRuntime(CleanNativeRuntimeOptions),
}

#[derive(Debug, Clone)]
struct InstallOptions {
    skip_prereqs: bool,
    skip_systemd: bool,
}

#[derive(Debug, Clone)]
struct BuildNativeBinariesOptions {
    profile: String,
    output_dir: PathBuf,
    target: Option<String>,
    cache_dir: Option<PathBuf>,
    bins: Vec<String>,
}

#[derive(Debug, Clone)]
struct BuildNativeRuntimeArtifactsOptions {
    profile: String,
    output_dir: PathBuf,
    target: Option<String>,
    cache_dir: PathBuf,
    ui_deps_state_file: PathBuf,
    backend_port: u16,
    calendar_port: u16,
}

#[derive(Debug, Clone)]
struct PlanNativeRuntimeOptions {
    repo_root: PathBuf,
    cache_dir: PathBuf,
    safe_tmp_dir: PathBuf,
    picker_helper_port: u16,
}

#[derive(Debug, Clone)]
struct DeployNativeOptions {
    skip_git_pull: bool,
    foreground: bool,
    no_health_check: bool,
}

#[derive(Debug, Clone)]
struct WriteNativeRuntimeSnapshotOptions {
    output: PathBuf,
}

#[derive(Debug, Clone)]
struct LaunchNativeRuntimeOptions {
    build_only: bool,
    foreground: bool,
    no_health_check: bool,
}

#[derive(Debug, Clone)]
struct CleanNativeRuntimeOptions {
    yes: bool,
}

#[derive(Debug, Clone)]
struct NativeBinaryBuildPolicy {
    rust_toolchain: String,
    gnu_compat_build: bool,
    gnu_glibc_version: String,
    transcription_features: String,
    server_features: String,
}

#[derive(Debug, Clone)]
struct Cli {
    command: CliCommand,
}

#[derive(Debug, Clone)]
struct SystemdInstallConfig {
    main_service_name: String,
    agent_service_name: String,
    post_healthcheck_service_name: String,
    main_service_path: PathBuf,
    agent_service_path: PathBuf,
    post_healthcheck_service_path: PathBuf,
    env_file_path: PathBuf,
    log_dir: PathBuf,
    repo_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct InstallManifest {
    installed_at_utc: String,
    repo_root: String,
    supported_flow: String,
    host: HostPlatform,
    native_user: NativeUserContext,
    install_mode: InstallModeManifest,
    services: ServiceManifest,
    paths: InstallPathManifest,
}

#[derive(Debug, Clone, Serialize)]
struct InstallModeManifest {
    skip_prereqs: bool,
    skip_systemd: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ServiceManifest {
    main_service_name: String,
    agent_service_name: String,
    post_healthcheck_service_name: String,
    systemd_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct InstallPathManifest {
    env_dir: String,
    env_file: String,
    runtime_defaults_file: String,
    log_dir: String,
    manifest_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BootstrapAiModelConfig {
    url: String,
    file_name: String,
    strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeTlsMode {
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpOriginParts {
    scheme: String,
    host: String,
    port: u16,
}

fn main() -> anyhow::Result<()> {
    let cli = parse_args(env::args().skip(1).collect())?;
    let repo_root = repo_root()?;
    let host = detect_host_platform()?;
    match &cli.command {
        CliCommand::Install(options) => {
            let user_context = detect_native_user_context(&repo_root)?;
            let systemd_config = build_systemd_config(&repo_root);
            install(&repo_root, &host, &user_context, &systemd_config, options)
        }
        CliCommand::InstallNativeSystemd => {
            let user_context = detect_native_user_context(&repo_root)?;
            let systemd_config = build_systemd_config(&repo_root);
            install_native_systemd_command(&host, &user_context, &systemd_config)
        }
        CliCommand::BuildNativeBinaries(options) => {
            build_native_binaries(&repo_root, &host, options)
        }
        CliCommand::BuildNativeRuntimeArtifacts(options) => {
            build_native_runtime_artifacts(&repo_root, &host, options)
        }
        CliCommand::PlanNativeRuntime(options) => plan_native_runtime(&host, options),
        CliCommand::DeployNative(options) => {
            let user_context = detect_native_user_context(&repo_root)?;
            deploy_native(&repo_root, &host, &user_context, options)
        }
        CliCommand::WriteNativeRuntimeSnapshot(options) => {
            write_native_runtime_snapshot(&repo_root, options)
        }
        CliCommand::LaunchNativeRuntime(options) => launch_native_runtime(&repo_root, options),
        CliCommand::StopNativeRuntime => stop_native_runtime(&repo_root),
        CliCommand::CleanNativeRuntime(options) => {
            let user_context = detect_native_user_context(&repo_root)?;
            clean_native_runtime(&repo_root, &user_context, options)
        }
    }
}

fn parse_args(args: Vec<String>) -> anyhow::Result<Cli> {
    if args.is_empty() {
        return Ok(Cli {
            command: CliCommand::Install(InstallOptions {
                skip_prereqs: false,
                skip_systemd: false,
            }),
        });
    }

    match args[0].as_str() {
        "install-native-systemd" => {
            parse_install_native_systemd_args(args.into_iter().skip(1).collect())
        }
        "build-native-binaries" => {
            parse_build_native_binaries_args(args.into_iter().skip(1).collect())
        }
        "build-native-runtime-artifacts" => {
            parse_build_native_runtime_artifacts_args(args.into_iter().skip(1).collect())
        }
        "plan-native-runtime" => parse_plan_native_runtime_args(args.into_iter().skip(1).collect()),
        "deploy-native" => parse_deploy_native_args(args.into_iter().skip(1).collect()),
        "write-native-runtime-snapshot" => {
            parse_write_native_runtime_snapshot_args(args.into_iter().skip(1).collect())
        }
        "launch-native-runtime" => {
            parse_launch_native_runtime_args(args.into_iter().skip(1).collect())
        }
        "stop-native-runtime" => parse_stop_native_runtime_args(args.into_iter().skip(1).collect()),
        "clean-native-runtime" => {
            parse_clean_native_runtime_args(args.into_iter().skip(1).collect())
        }
        "install" => parse_install_args(args.into_iter().skip(1).collect()),
        "-h" | "--help" => {
            print_usage();
            std::process::exit(0);
        }
        _ => parse_install_args(args),
    }
}

fn parse_install_native_systemd_args(args: Vec<String>) -> anyhow::Result<Cli> {
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        print_install_native_systemd_usage();
        std::process::exit(0);
    }
    if !args.is_empty() {
        bail!("Unknown argument: {}", args[0]);
    }
    Ok(Cli {
        command: CliCommand::InstallNativeSystemd,
    })
}

fn parse_install_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut skip_prereqs = false;
    let mut skip_systemd = false;

    for arg in args {
        match arg.as_str() {
            "--skip-prereqs" => skip_prereqs = true,
            "--skip-systemd" => skip_systemd = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(Cli {
        command: CliCommand::Install(InstallOptions {
            skip_prereqs,
            skip_systemd,
        }),
    })
}

fn parse_build_native_binaries_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut profile = None;
    let mut output_dir = None;
    let mut target = None;
    let mut cache_dir = None;
    let mut bins = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --profile")?
                    .clone();
                profile = Some(value);
            }
            "--output-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --output-dir")?
                    .clone();
                output_dir = Some(PathBuf::from(value));
            }
            "--target" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --target")?
                    .clone();
                target = Some(value);
            }
            "--cache-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --cache-dir")?
                    .clone();
                cache_dir = Some(PathBuf::from(value));
            }
            "--bin" => {
                index += 1;
                let value = args.get(index).context("Missing value for --bin")?.clone();
                bins.push(value);
            }
            "-h" | "--help" => {
                print_build_native_binaries_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    let profile = profile.context("--profile is required")?;
    let output_dir = output_dir.context("--output-dir is required")?;
    if bins.is_empty() {
        bail!("At least one --bin is required");
    }

    Ok(Cli {
        command: CliCommand::BuildNativeBinaries(BuildNativeBinariesOptions {
            profile,
            output_dir,
            target,
            cache_dir,
            bins,
        }),
    })
}

fn parse_plan_native_runtime_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut repo_root = None;
    let mut cache_dir = None;
    let mut safe_tmp_dir = None;
    let mut picker_helper_port = 43110_u16;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--repo-root" => {
                index += 1;
                let value = args.get(index).context("Missing value for --repo-root")?;
                repo_root = Some(PathBuf::from(value));
            }
            "--cache-dir" => {
                index += 1;
                let value = args.get(index).context("Missing value for --cache-dir")?;
                cache_dir = Some(PathBuf::from(value));
            }
            "--safe-tmp-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --safe-tmp-dir")?;
                safe_tmp_dir = Some(PathBuf::from(value));
            }
            "--picker-helper-port" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --picker-helper-port")?;
                picker_helper_port = value
                    .parse::<u16>()
                    .with_context(|| format!("Invalid picker helper port: {value}"))?;
            }
            "-h" | "--help" => {
                print_plan_native_runtime_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    Ok(Cli {
        command: CliCommand::PlanNativeRuntime(PlanNativeRuntimeOptions {
            repo_root: repo_root.context("--repo-root is required")?,
            cache_dir: cache_dir.context("--cache-dir is required")?,
            safe_tmp_dir: safe_tmp_dir.context("--safe-tmp-dir is required")?,
            picker_helper_port,
        }),
    })
}

fn parse_build_native_runtime_artifacts_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut profile = None;
    let mut output_dir = None;
    let mut target = None;
    let mut cache_dir = None;
    let mut ui_deps_state_file = None;
    let mut backend_port = None;
    let mut calendar_port = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --profile")?
                    .clone();
                profile = Some(value);
            }
            "--output-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --output-dir")?
                    .clone();
                output_dir = Some(PathBuf::from(value));
            }
            "--target" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --target")?
                    .clone();
                target = Some(value);
            }
            "--cache-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --cache-dir")?
                    .clone();
                cache_dir = Some(PathBuf::from(value));
            }
            "--ui-deps-state-file" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --ui-deps-state-file")?
                    .clone();
                ui_deps_state_file = Some(PathBuf::from(value));
            }
            "--backend-port" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --backend-port")?;
                backend_port = Some(
                    value
                        .parse::<u16>()
                        .with_context(|| format!("Invalid backend port: {value}"))?,
                );
            }
            "--calendar-port" => {
                index += 1;
                let value = args
                    .get(index)
                    .context("Missing value for --calendar-port")?;
                calendar_port = Some(
                    value
                        .parse::<u16>()
                        .with_context(|| format!("Invalid calendar port: {value}"))?,
                );
            }
            "-h" | "--help" => {
                print_build_native_runtime_artifacts_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    Ok(Cli {
        command: CliCommand::BuildNativeRuntimeArtifacts(BuildNativeRuntimeArtifactsOptions {
            profile: profile.context("--profile is required")?,
            output_dir: output_dir.context("--output-dir is required")?,
            target,
            cache_dir: cache_dir.context("--cache-dir is required")?,
            ui_deps_state_file: ui_deps_state_file.context("--ui-deps-state-file is required")?,
            backend_port: backend_port.context("--backend-port is required")?,
            calendar_port: calendar_port.context("--calendar-port is required")?,
        }),
    })
}

fn parse_deploy_native_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut skip_git_pull = false;
    let mut foreground = false;
    let mut no_health_check = false;

    for arg in args {
        match arg.as_str() {
            "--skip-git-pull" => skip_git_pull = true,
            "--foreground" => foreground = true,
            "--no-health-check" => no_health_check = true,
            "-h" | "--help" => {
                print_deploy_native_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(Cli {
        command: CliCommand::DeployNative(DeployNativeOptions {
            skip_git_pull,
            foreground,
            no_health_check,
        }),
    })
}

fn parse_write_native_runtime_snapshot_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut output = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--output" => {
                index += 1;
                let value = args.get(index).context("Missing value for --output")?;
                output = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                print_write_native_runtime_snapshot_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    Ok(Cli {
        command: CliCommand::WriteNativeRuntimeSnapshot(WriteNativeRuntimeSnapshotOptions {
            output: output.context("--output is required")?,
        }),
    })
}

fn parse_launch_native_runtime_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut build_only = false;
    let mut foreground = false;
    let mut no_health_check = false;

    for arg in args {
        match arg.as_str() {
            "--build-only" => build_only = true,
            "--foreground" => foreground = true,
            "--no-health-check" => no_health_check = true,
            "-h" | "--help" => {
                print_launch_native_runtime_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(Cli {
        command: CliCommand::LaunchNativeRuntime(LaunchNativeRuntimeOptions {
            build_only,
            foreground,
            no_health_check,
        }),
    })
}

fn parse_clean_native_runtime_args(args: Vec<String>) -> anyhow::Result<Cli> {
    let mut yes = false;

    for arg in args {
        match arg.as_str() {
            "--yes" | "-y" => yes = true,
            "-h" | "--help" => {
                print_clean_native_runtime_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(Cli {
        command: CliCommand::CleanNativeRuntime(CleanNativeRuntimeOptions { yes }),
    })
}

fn parse_stop_native_runtime_args(args: Vec<String>) -> anyhow::Result<Cli> {
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        print_stop_native_runtime_usage();
        std::process::exit(0);
    }
    if !args.is_empty() {
        bail!("Unknown argument: {}", args[0]);
    }
    Ok(Cli {
        command: CliCommand::StopNativeRuntime,
    })
}

fn print_usage() {
    println!(
        "\
Rustyfin installer

Usage:
  cargo run -p rustfin-installer -- [install] [--skip-prereqs] [--skip-systemd]
  cargo run -p rustfin-installer -- install-native-systemd
  cargo run -p rustfin-installer -- build-native-binaries --profile <dev|release|name> --output-dir <dir> [--target <triple>] [--cache-dir <dir>] --bin <name> [--bin <name>...]
  cargo run -p rustfin-installer -- build-native-runtime-artifacts --profile <dev|release|name> --output-dir <dir> --cache-dir <dir> --ui-deps-state-file <path> --backend-port <port> --calendar-port <port> [--target <triple>]
  cargo run -p rustfin-installer -- plan-native-runtime --repo-root <dir> --cache-dir <dir> --safe-tmp-dir <dir> [--picker-helper-port <port>]
  cargo run -p rustfin-installer -- deploy-native [--skip-git-pull] [--foreground] [--no-health-check]
  cargo run -p rustfin-installer -- write-native-runtime-snapshot --output <path>
  cargo run -p rustfin-installer -- launch-native-runtime [--build-only] [--foreground] [--no-health-check]
  cargo run -p rustfin-installer -- stop-native-runtime
  cargo run -p rustfin-installer -- clean-native-runtime --yes

Behavior:
  install           Full native install flow (default)
  --skip-prereqs    Skip host prerequisite installation
  --skip-systemd    Start Rustyfin directly instead of installing systemd services
  install-native-systemd
                    Install or refresh the native Linux systemd units
  build-native-binaries
                    Build and copy Linux binaries using Rust-owned target/toolchain policy
  build-native-runtime-artifacts
                    Build native Rust services plus the Next standalone UI using Rust-owned build policy
  plan-native-runtime
                    Resolve runtime ports, paths, URLs, and DB/network env for start-native.sh
  deploy-native     Stop, update, rebuild, and restart the native Linux runtime
  write-native-runtime-snapshot
                    Persist the current native runtime snapshot from env to disk
  launch-native-runtime
                    Launch the prepared native runtime, write pid files, and run startup health checks
  stop-native-runtime
                    Stop the native runtime child processes and helper listeners
  clean-native-runtime
                    Stop services, reset native runtime state, and wipe PostgreSQL contents

Public first-time Linux install entrypoint:
  ./scripts/install_linux.sh

Current support:
  Full native Linux install is currently implemented for Debian 12, Debian 13,
  Ubuntu 22.04, and Ubuntu 24.04.
  rustfin-installer is the internal Rust installer surface behind that wrapper and
  owns native runtime defaults, systemd env/unit rendering, and install-manifest output.
"
    );
}

fn print_build_native_binaries_usage() {
    println!(
        "\
Rustyfin installer: build-native-binaries

Usage:
  cargo run -p rustfin-installer -- build-native-binaries --profile <dev|release|name> --output-dir <dir> [--target <triple>] [--cache-dir <dir>] --bin <name> [--bin <name>...]
"
    );
}

fn print_build_native_runtime_artifacts_usage() {
    println!(
        "\
Rustyfin installer: build-native-runtime-artifacts

Usage:
  cargo run -p rustfin-installer -- build-native-runtime-artifacts --profile <dev|release|name> --output-dir <dir> --cache-dir <dir> --ui-deps-state-file <path> --backend-port <port> --calendar-port <port> [--target <triple>]
"
    );
}

fn print_install_native_systemd_usage() {
    println!(
        "\
Rustyfin installer: install-native-systemd

Usage:
  cargo run -p rustfin-installer -- install-native-systemd
"
    );
}

fn print_launch_native_runtime_usage() {
    println!(
        "\
Rustyfin installer: launch-native-runtime

Usage:
  cargo run -p rustfin-installer -- launch-native-runtime [--build-only] [--foreground] [--no-health-check]
"
    );
}

fn print_plan_native_runtime_usage() {
    println!(
        "\
Rustyfin installer: plan-native-runtime

Usage:
  cargo run -p rustfin-installer -- plan-native-runtime --repo-root <dir> --cache-dir <dir> --safe-tmp-dir <dir> [--picker-helper-port <port>]
"
    );
}

fn print_deploy_native_usage() {
    println!(
        "\
Rustyfin installer: deploy-native

Usage:
  cargo run -p rustfin-installer -- deploy-native [--skip-git-pull] [--foreground] [--no-health-check]
"
    );
}

fn print_write_native_runtime_snapshot_usage() {
    println!(
        "\
Rustyfin installer: write-native-runtime-snapshot

Usage:
  cargo run -p rustfin-installer -- write-native-runtime-snapshot --output <path>
"
    );
}

fn print_clean_native_runtime_usage() {
    println!(
        "\
Rustyfin installer: clean-native-runtime

Usage:
  cargo run -p rustfin-installer -- clean-native-runtime --yes
"
    );
}

fn print_stop_native_runtime_usage() {
    println!(
        "\
Rustyfin installer: stop-native-runtime

Usage:
  cargo run -p rustfin-installer -- stop-native-runtime
"
    );
}

fn repo_root() -> anyhow::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("failed to resolve repository root from CARGO_MANIFEST_DIR")
}

fn build_systemd_config(repo_root: &Path) -> SystemdInstallConfig {
    let main_service_name =
        env::var("RUSTFIN_SYSTEMD_SERVICE").unwrap_or_else(|_| "rustyfin-native.service".into());
    let agent_service_name = env::var("RUSTFIN_SERVERS_AGENT_SERVICE")
        .unwrap_or_else(|_| "rustfin-servers-agent.service".into());
    let post_healthcheck_service_name = env::var("RUSTFIN_POST_HEALTHCHECK_SERVICE")
        .unwrap_or_else(|_| "rustyfin-post-healthcheck.service".into());
    let log_dir = repo_root.join(".tmp/native-runtime/logs");

    SystemdInstallConfig {
        main_service_path: PathBuf::from(format!("/etc/systemd/system/{main_service_name}")),
        agent_service_path: PathBuf::from(format!("/etc/systemd/system/{agent_service_name}")),
        post_healthcheck_service_path: PathBuf::from(format!(
            "/etc/systemd/system/{post_healthcheck_service_name}"
        )),
        main_service_name,
        agent_service_name,
        post_healthcheck_service_name,
        env_file_path: PathBuf::from(ENV_FILE),
        log_dir,
        repo_root: repo_root.to_path_buf(),
    }
}

fn install(
    repo_root: &Path,
    host: &HostPlatform,
    user_context: &NativeUserContext,
    systemd_config: &SystemdInstallConfig,
    options: &InstallOptions,
) -> anyhow::Result<()> {
    let adapter = crate::distro::resolve_adapter(host);
    if adapter.name() == "unsupported" {
        bail!(
            "rustfin-installer currently supports Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04. Detected: {} {}.",
            host.id.as_deref().unwrap_or("unknown"),
            host.version_id.as_deref().unwrap_or("unknown")
        );
    }

    println!(
        "[rustfin-installer] Host detected: id={}, version={}, id_like={}, arch={}, package_manager={}",
        host.id.as_deref().unwrap_or("unknown"),
        host.version_id.as_deref().unwrap_or("unknown"),
        host.id_like.as_deref().unwrap_or("unknown"),
        host.architecture,
        host.package_manager
    );
    println!(
        "[rustfin-installer] Native runtime user: {} ({})",
        user_context.name, user_context.home
    );
    println!(
        "[rustfin-installer] Using distro adapter: {}",
        adapter.name()
    );

    if !options.skip_prereqs {
        println!("[rustfin-installer] Installing runtime packages...");
        adapter.install_packages(user_context)?;

        println!("[rustfin-installer] Ensuring GPU support (if applicable)...");
        adapter.install_gpu_support(user_context)?;

        println!(
            "[rustfin-installer] Ensuring Rust toolchain for native user {}...",
            user_context.name
        );
        ensure_native_user_rust_toolchain(user_context)?;

        println!("[rustfin-installer] Installing yt-dlp runtime...");
        install_ytdlp_runtime(user_context)?;

        println!("[rustfin-installer] Ensuring PostgreSQL is enabled...");
        ensure_postgresql_ready(user_context)?;

        println!("[rustfin-installer] Configuring PostgreSQL role/database...");
        configure_postgresql(user_context)?;

        println!("[rustfin-installer] Ensuring managed Java 21 runtime...");
        ensure_managed_java_21(user_context)?;
    } else {
        println!("[rustfin-installer] Skipping prerequisite installation.");
    }

    println!("[rustfin-installer] Writing native runtime defaults...");
    write_native_runtime_defaults(repo_root, host, user_context)?;
    println!("[rustfin-installer] Ensuring starter AI models...");
    ensure_starter_ai_model(repo_root, host, user_context)?;

    if options.skip_systemd {
        println!("[rustfin-installer] Starting Rustyfin directly without systemd install.");
        run_script(repo_root, "scripts/start-native.sh", &[])?;
    } else {
        println!("[rustfin-installer] Building native artifacts...");
        run_script(repo_root, "scripts/start-native.sh", &["--build-only"])?;
        println!("[rustfin-installer] Installing native systemd services from Rust installer...");
        install_systemd_units(systemd_config, user_context)?;
        validate_systemd_runtime_start(systemd_config, user_context)?;
    }

    write_install_manifest(repo_root, host, user_context, systemd_config, options)?;
    println!("[rustfin-installer] Install flow completed.");
    Ok(())
}

fn install_native_systemd_command(
    host: &HostPlatform,
    user_context: &NativeUserContext,
    systemd_config: &SystemdInstallConfig,
) -> anyhow::Result<()> {
    let adapter = crate::distro::resolve_adapter(host);
    if adapter.name() == "unsupported" {
        bail!(
            "rustfin-installer currently supports Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04. Detected: {} {}.",
            host.id.as_deref().unwrap_or("unknown"),
            host.version_id.as_deref().unwrap_or("unknown")
        );
    }
    println!("[rustfin-installer] Installing native systemd units...");
    install_systemd_units(systemd_config, user_context)?;
    validate_systemd_runtime_start(systemd_config, user_context)?;
    println!("[rustfin-installer] Native systemd install completed.");
    Ok(())
}

fn ensure_native_user_rust_toolchain(user_context: &NativeUserContext) -> anyhow::Result<()> {
    let cargo_bin = Path::new(&user_context.home).join(".cargo/bin/cargo");
    let rustc_bin = Path::new(&user_context.home).join(".cargo/bin/rustc");
    if cargo_bin.is_file() && rustc_bin.is_file() {
        return Ok(());
    }

    ensure_command_available("curl")?;
    run_as_native_user_shell(
        "curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal",
        user_context,
    )?;

    if cargo_bin.is_file() && rustc_bin.is_file() {
        return Ok(());
    }

    bail!(
        "Rust toolchain was not installed successfully for native user {}",
        user_context.name
    );
}

fn install_ytdlp_runtime(user_context: &NativeUserContext) -> anyhow::Result<()> {
    run_root_command(
        "python3",
        &[
            "-m",
            "pip",
            "install",
            "--break-system-packages",
            "--upgrade",
            "yt-dlp",
        ],
        user_context,
    )
}

fn ensure_postgresql_ready(user_context: &NativeUserContext) -> anyhow::Result<()> {
    ensure_command_available("systemctl")?;
    run_root_command(
        "systemctl",
        &["enable", "--now", "postgresql"],
        user_context,
    )
}

fn configure_postgresql(user_context: &NativeUserContext) -> anyhow::Result<()> {
    let pg_user = env::var("RUSTFIN_PG_USER").unwrap_or_else(|_| "rustfin".to_string());
    let pg_password = env::var("RUSTFIN_PG_PASSWORD").unwrap_or_else(|_| "rustfin".to_string());
    let pg_db = env::var("RUSTFIN_PG_DB").unwrap_or_else(|_| "rustfin".to_string());

    validate_sql_identifier("RUSTFIN_PG_USER", &pg_user)?;
    validate_sql_identifier("RUSTFIN_PG_DB", &pg_db)?;
    let escaped_password = escape_sql_literal(&pg_password);

    let role_sql = format!(
        r#"DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{pg_user}') THEN
    EXECUTE 'CREATE ROLE "{pg_user}" LOGIN PASSWORD ''{escaped_password}''';
  ELSE
    EXECUTE 'ALTER ROLE "{pg_user}" WITH LOGIN PASSWORD ''{escaped_password}''';
  END IF;
END
$$;"#
    );
    run_postgres_command(
        "psql",
        &["-v", "ON_ERROR_STOP=1", "postgres", "-c", role_sql.as_str()],
        user_context,
    )?;

    let db_exists_query = format!("SELECT 1 FROM pg_database WHERE datname='{pg_db}'");
    let db_exists = run_postgres_command_capture(
        "psql",
        &["-tAc", db_exists_query.as_str(), "postgres"],
        user_context,
    )?;
    if db_exists.lines().map(str::trim).any(|line| line == "1") {
        return Ok(());
    }

    run_postgres_command(
        "createdb",
        &["-O", pg_user.as_str(), pg_db.as_str()],
        user_context,
    )
}

fn ensure_managed_java_21(user_context: &NativeUserContext) -> anyhow::Result<()> {
    let managed_java = Path::new(SERVERS_DEFAULT_JAVA);
    if matches!(java_major_version(managed_java)?, Some(21)) {
        println!(
            "[rustfin-installer] Managed Java 21 already available at {}",
            managed_java.display()
        );
        return Ok(());
    }

    install_managed_java_21(user_context)?;
    if !matches!(java_major_version(managed_java)?, Some(21)) {
        bail!(
            "managed Java runtime was installed, but {} is still not Java 21",
            managed_java.display()
        );
    }

    Ok(())
}

fn install_managed_java_21(user_context: &NativeUserContext) -> anyhow::Result<()> {
    ensure_command_available("dpkg")?;
    ensure_command_available("curl")?;
    ensure_command_available("tar")?;

    let dpkg_arch = run_command_capture("dpkg", &["--print-architecture"])?;
    let arch = match dpkg_arch.trim() {
        "arm64" => "aarch64",
        "amd64" => "x64",
        other => bail!("unsupported Debian architecture for managed Java 21 install: {other}"),
    };

    let download_url = format!(
        "https://api.adoptium.net/v3/binary/latest/21/ga/linux/{arch}/jdk/hotspot/normal/eclipse"
    );
    let temp_dir = env::temp_dir().join(format!(
        "rustfin-installer-java-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create {}", temp_dir.display()))?;
    let archive_path = temp_dir.join("temurin21.tar.gz");
    let archive_string = archive_path.display().to_string();
    let install_dir_string = MANAGED_JAVA_INSTALL_DIR.to_string();
    let current_link_string = MANAGED_JAVA_CURRENT.to_string();

    let install_result = (|| -> anyhow::Result<()> {
        let download_status = Command::new("curl")
            .arg("-fsSL")
            .arg(download_url.as_str())
            .arg("-o")
            .arg(archive_path.as_os_str())
            .status()
            .context("failed to download managed Java 21 archive")?;
        ensure_success("curl", download_status)?;

        run_root_command(
            "install",
            &["-d", "-m", "755", MANAGED_JAVA_ROOT],
            user_context,
        )?;
        run_root_command("rm", &["-rf", install_dir_string.as_str()], user_context)?;
        run_root_command("mkdir", &["-p", install_dir_string.as_str()], user_context)?;
        run_root_command(
            "tar",
            &[
                "-xzf",
                archive_string.as_str(),
                "-C",
                install_dir_string.as_str(),
                "--strip-components=1",
            ],
            user_context,
        )?;
        run_root_command(
            "ln",
            &[
                "-sfn",
                install_dir_string.as_str(),
                current_link_string.as_str(),
            ],
            user_context,
        )?;
        run_root_command(
            "chmod",
            &["-R", "a+rX", install_dir_string.as_str()],
            user_context,
        )?;
        run_root_command("test", &["-x", SERVERS_DEFAULT_JAVA], user_context)?;

        Ok(())
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    install_result
}

fn java_major_version(java_bin: &Path) -> anyhow::Result<Option<u32>> {
    if !java_bin.exists() {
        return Ok(None);
    }

    let output = Command::new(java_bin)
        .arg("-version")
        .output()
        .with_context(|| format!("failed to execute {}", java_bin.display()))?;
    if !output.status.success() {
        return Ok(None);
    }

    let version_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(parse_java_major_version(&version_text))
}

fn parse_java_major_version(version_text: &str) -> Option<u32> {
    let quoted = version_text.split('"').nth(1)?;
    let major = quoted.split('.').next()?;
    major.parse().ok()
}

fn write_native_runtime_defaults(
    repo_root: &Path,
    host: &HostPlatform,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    let existing_defaults =
        load_shell_env_map(Path::new(RUNTIME_DEFAULTS_FILE)).unwrap_or_default();
    let public_host = detect_primary_lan_ipv4()?.unwrap_or_else(|| "localhost".to_string());
    let ai_backend = detect_default_ai_backend(host);
    let native_target = default_native_linux_target(host)?;
    let webrtc_ice_servers_json = existing_defaults
        .get("RUSTFIN_WEBRTC_ICE_SERVERS_JSON")
        .cloned()
        .unwrap_or_default();
    let webrtc_stun_url = existing_defaults
        .get("RUSTFIN_WEBRTC_STUN_URL")
        .cloned()
        .unwrap_or_default();
    let webrtc_turn_url = existing_defaults
        .get("RUSTFIN_WEBRTC_TURN_URL")
        .cloned()
        .unwrap_or_default();
    let webrtc_turn_urls = existing_defaults
        .get("RUSTFIN_WEBRTC_TURN_URLS")
        .cloned()
        .unwrap_or_default();
    let webrtc_turn_username = existing_defaults
        .get("RUSTFIN_WEBRTC_TURN_USERNAME")
        .cloned()
        .unwrap_or_default();
    let webrtc_turn_credential = existing_defaults
        .get("RUSTFIN_WEBRTC_TURN_CREDENTIAL")
        .cloned()
        .unwrap_or_default();
    let rendered = format!(
        "# Generated by rustfin-installer\n\
: \"${{RUSTFIN_BACKEND_PORT:=8096}}\"\n\
: \"${{RUSTFIN_CALENDAR_PORT:=8099}}\"\n\
: \"${{RUSTFIN_TMDB_AGENT_PORT:=8100}}\"\n\
: \"${{RUSTFIN_YOUTUBE_AGENT_PORT:=8101}}\"\n\
: \"${{RUSTFIN_TRANSCRIPTION_AGENT_PORT:=8102}}\"\n\
: \"${{RUSTFIN_SERVERS_AGENT_PORT:=8103}}\"\n\
: \"${{RUSTFIN_UI_INTERNAL_PORT:=3001}}\"\n\
: \"${{RUSTFIN_UI_PORT:=3000}}\"\n\
: \"${{RUSTFIN_MEDIA_PATH:={media_path}}}\"\n\
: \"${{RUSTFIN_PUBLIC_HOST:={public_host}}}\"\n\
: \"${{RUSTFIN_EDGE_TLS_MODE:=manual}}\"\n\
: \"${{RUSTFIN_WEBRTC_ICE_SERVERS_JSON:={webrtc_ice_servers_json}}}\"\n\
: \"${{RUSTFIN_WEBRTC_STUN_URL:={webrtc_stun_url}}}\"\n\
: \"${{RUSTFIN_WEBRTC_TURN_URL:={webrtc_turn_url}}}\"\n\
: \"${{RUSTFIN_WEBRTC_TURN_URLS:={webrtc_turn_urls}}}\"\n\
: \"${{RUSTFIN_WEBRTC_TURN_USERNAME:={webrtc_turn_username}}}\"\n\
: \"${{RUSTFIN_WEBRTC_TURN_CREDENTIAL:={webrtc_turn_credential}}}\"\n\
: \"${{RUSTFIN_AI_GPU_BACKEND:={ai_backend}}}\"\n\
: \"${{RUSTFIN_TRANSCODER_HW_ACCEL:=auto}}\"\n\
: \"${{RUSTFIN_TRANSCRIPTION_GPU_MODE:=opencl}}\"\n\
: \"${{RUSTFIN_TRANSCRIPTION_REQUIRE_GPU:=0}}\"\n\
: \"${{RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES:=gpu-opencl}}\"\n\
: \"${{RUSTFIN_SERVERS_DEFAULT_JAVA:={SERVERS_DEFAULT_JAVA}}}\"\n\
: \"${{RUSTFIN_NATIVE_LINUX_TARGET:={native_target}}}\"\n\
: \"${{RUSTFIN_NATIVE_RUNTIME_DIR:={runtime_dir}}}\"\n",
        media_path = user_context.home,
        runtime_dir = repo_root.join(".tmp/native-runtime").display(),
    );

    write_root_owned_file(
        Path::new(RUNTIME_DEFAULTS_FILE),
        &rendered,
        0o644,
        user_context,
    )
}

fn ensure_starter_ai_model(
    repo_root: &Path,
    host: &HostPlatform,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    if !ai_bootstrap_model_enabled() {
        println!(
            "[rustfin-installer] Starter AI model bootstrap disabled via {}.",
            AI_BOOTSTRAP_MODEL_ENV
        );
        return Ok(());
    }

    let requested_ai_backend =
        env::var("RUSTFIN_AI_GPU_BACKEND").unwrap_or_else(|_| "auto".to_string());
    let resolved_ai_backend = resolve_ai_gpu_backend(host, &requested_ai_backend)?;
    if resolved_ai_backend == "disabled" {
        println!(
            "[rustfin-installer] AI inference is disabled for this host/build, but starter AI models will still be seeded during setup."
        );
    }

    let model_dir = ensure_installer_ai_model_dir_ready(user_context)?;
    let bootstrap_models = resolve_bootstrap_ai_model_configs()?;
    if ai_model_dir_contains_gguf(&model_dir)?
        && !ai_model_dir_contains_bootstrap_seed(&model_dir, &bootstrap_models)
    {
        println!(
            "[rustfin-installer] Existing non-starter GGUF model detected in {}; skipping starter model seeding.",
            model_dir.display()
        );
        return Ok(());
    }

    println!(
        "[rustfin-installer] Downloading starter AI models into {}...",
        model_dir.display()
    );

    let total_bootstrap_models = bootstrap_models.len();
    let mut downloaded_models = 0usize;
    let mut failed_models = 0usize;
    for (index, bootstrap) in bootstrap_models.iter().enumerate() {
        let final_path = model_dir.join(&bootstrap.file_name);
        if final_path.exists() {
            println!(
                "[rustfin-installer] Starter AI model already present at {}; skipping.",
                final_path.display()
            );
            continue;
        }

        println!(
            "[rustfin-installer] Downloading starter AI model {}/{}: {} into {}...",
            index + 1,
            total_bootstrap_models,
            bootstrap.file_name,
            model_dir.display()
        );
        match download_starter_ai_model(repo_root, user_context, &model_dir, bootstrap) {
            Ok(downloaded_path) => {
                downloaded_models += 1;
                println!(
                    "[rustfin-installer] Starter AI model ready at {}",
                    downloaded_path.display()
                );
            }
            Err(error) if !bootstrap.strict => {
                failed_models += 1;
                eprintln!(
                    "[rustfin-installer] Warning: failed to download the starter AI model from {}: {error}. Rustyfin will continue installing without that bundled model. Set {}=0 to skip this step or {} to override the primary starter source.",
                    bootstrap.url, AI_BOOTSTRAP_MODEL_ENV, AI_BOOTSTRAP_MODEL_URL_ENV
                );
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to provision the configured starter AI model from {}",
                        bootstrap.url
                    )
                });
            }
        }
    }

    match (downloaded_models, failed_models) {
        (0, 0) => {
            println!(
                "[rustfin-installer] Starter AI models were already present in {}; nothing to download.",
                model_dir.display()
            );
        }
        (_, 0) => {
            println!(
                "[rustfin-installer] Starter AI model seeding completed with {} downloaded model(s).",
                downloaded_models
            );
        }
        (0, failed) => {
            println!(
                "[rustfin-installer] Starter AI model seeding completed with no downloads and {} failed download(s).",
                failed
            );
        }
        (_, failed) => {
            println!(
                "[rustfin-installer] Starter AI model seeding completed with {} downloaded model(s) and {} failed download(s).",
                downloaded_models, failed
            );
        }
    }

    Ok(())
}

fn ai_bootstrap_model_enabled() -> bool {
    parse_ai_bootstrap_model_enabled(env::var(AI_BOOTSTRAP_MODEL_ENV).ok().as_deref())
}

fn parse_ai_bootstrap_model_enabled(value: Option<&str>) -> bool {
    !matches!(
        value.map(|raw| raw.trim().to_ascii_lowercase()),
        Some(ref value)
            if matches!(
                value.as_str(),
                "0" | "false" | "off" | "disabled" | "disable" | "no"
            )
    )
}

fn resolve_bootstrap_ai_model_config() -> anyhow::Result<BootstrapAiModelConfig> {
    resolve_bootstrap_ai_model_config_with_overrides(
        env::var(AI_BOOTSTRAP_MODEL_URL_ENV).ok().as_deref(),
        env::var(AI_BOOTSTRAP_MODEL_FILE_ENV).ok().as_deref(),
    )
}

fn resolve_bootstrap_ai_model_configs() -> anyhow::Result<Vec<BootstrapAiModelConfig>> {
    let primary = resolve_bootstrap_ai_model_config()?;
    resolve_bootstrap_ai_model_configs_with_primary(primary)
}

fn resolve_bootstrap_ai_model_configs_with_overrides(
    url_override: Option<&str>,
    file_override: Option<&str>,
) -> anyhow::Result<Vec<BootstrapAiModelConfig>> {
    let primary = resolve_bootstrap_ai_model_config_with_overrides(url_override, file_override)?;
    resolve_bootstrap_ai_model_configs_with_primary(primary)
}

fn resolve_bootstrap_ai_model_configs_with_primary(
    primary: BootstrapAiModelConfig,
) -> anyhow::Result<Vec<BootstrapAiModelConfig>> {
    let mut configs = Vec::with_capacity(3);
    configs.push(primary);
    configs.push(BootstrapAiModelConfig {
        url: DEFAULT_BOOTSTRAP_GEMMA_4_E2B_MODEL_URL.to_string(),
        file_name: derive_gguf_file_name_from_url(DEFAULT_BOOTSTRAP_GEMMA_4_E2B_MODEL_URL)?,
        strict: false,
    });
    configs.push(BootstrapAiModelConfig {
        url: DEFAULT_BOOTSTRAP_GEMMA_4_E4B_MODEL_URL.to_string(),
        file_name: derive_gguf_file_name_from_url(DEFAULT_BOOTSTRAP_GEMMA_4_E4B_MODEL_URL)?,
        strict: false,
    });
    Ok(deduplicate_bootstrap_ai_model_configs(configs))
}

fn resolve_bootstrap_ai_model_config_with_overrides(
    url_override: Option<&str>,
    file_override: Option<&str>,
) -> anyhow::Result<BootstrapAiModelConfig> {
    let url_override = url_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let file_override = file_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let strict = url_override.is_some() || file_override.is_some();
    let url = url_override
        .clone()
        .unwrap_or_else(|| DEFAULT_BOOTSTRAP_AI_MODEL_URL.to_string());
    let file_name = match file_override {
        Some(file_name) => validate_bootstrap_model_file_name(&file_name)?,
        None => derive_gguf_file_name_from_url(&url)?,
    };

    Ok(BootstrapAiModelConfig {
        url,
        file_name,
        strict,
    })
}

fn deduplicate_bootstrap_ai_model_configs(
    configs: Vec<BootstrapAiModelConfig>,
) -> Vec<BootstrapAiModelConfig> {
    let mut seen_file_names = HashSet::new();
    let mut deduplicated = Vec::with_capacity(configs.len());
    for config in configs {
        if seen_file_names.insert(config.file_name.clone()) {
            deduplicated.push(config);
        }
    }
    deduplicated
}

fn validate_bootstrap_model_file_name(file_name: &str) -> anyhow::Result<String> {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        bail!("{AI_BOOTSTRAP_MODEL_FILE_ENV} must not be empty");
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("{AI_BOOTSTRAP_MODEL_FILE_ENV} must be a plain filename");
    }
    if !trimmed.ends_with(".gguf") {
        bail!("{AI_BOOTSTRAP_MODEL_FILE_ENV} must end with .gguf");
    }
    Ok(trimmed.to_string())
}

fn derive_gguf_file_name_from_url(url: &str) -> anyhow::Result<String> {
    let base = url
        .split(['?', '#'])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("bootstrap AI model URL must not be empty")?;
    let file_name = base
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .context("bootstrap AI model URL must include a filename")?;
    validate_bootstrap_model_file_name(file_name)
}

fn resolve_installer_ai_model_dir() -> PathBuf {
    resolve_installer_ai_model_dir_from_env(env::var("RUSTFIN_AI_MODEL_DIR").ok().as_deref())
}

fn resolve_installer_ai_model_dir_from_env(value: Option<&str>) -> PathBuf {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_AI_MODEL_DIR))
}

fn ensure_installer_ai_model_dir_ready(
    user_context: &NativeUserContext,
) -> anyhow::Result<PathBuf> {
    ensure_command_available("install")?;
    let model_dir = resolve_installer_ai_model_dir();
    let model_dir_string = model_dir.display().to_string();
    run_root_command(
        "install",
        &[
            "-d",
            "-m",
            "755",
            "-o",
            user_context.name.as_str(),
            "-g",
            user_context.name.as_str(),
            model_dir_string.as_str(),
        ],
        user_context,
    )?;
    Ok(model_dir.canonicalize().unwrap_or(model_dir))
}

fn ai_model_dir_contains_gguf(model_dir: &Path) -> anyhow::Result<bool> {
    if !model_dir.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(model_dir)
        .with_context(|| format!("failed to read {}", model_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!(
                "failed to read AI model directory entry in {}",
                model_dir.display()
            )
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("gguf") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ai_model_dir_contains_bootstrap_seed(
    model_dir: &Path,
    bootstrap_models: &[BootstrapAiModelConfig],
) -> bool {
    bootstrap_models
        .iter()
        .any(|bootstrap| model_dir.join(&bootstrap.file_name).is_file())
}

fn download_starter_ai_model(
    repo_root: &Path,
    user_context: &NativeUserContext,
    model_dir: &Path,
    bootstrap: &BootstrapAiModelConfig,
) -> anyhow::Result<PathBuf> {
    ensure_command_available("curl")?;
    let final_path = model_dir.join(&bootstrap.file_name);
    let part_path = model_dir.join(format!("{}.part", bootstrap.file_name));
    let part_path_string = part_path.display().to_string();
    let final_path_string = final_path.display().to_string();
    let model_dir_string = model_dir.display().to_string();
    let _ = remove_if_exists(&part_path);

    let download_result = run_command_in_dir_as_user(
        "curl",
        &[
            "-fL",
            "--retry",
            "3",
            "--retry-delay",
            "2",
            "--connect-timeout",
            "20",
            "--progress-bar",
            "-o",
            part_path_string.as_str(),
            bootstrap.url.as_str(),
        ],
        repo_root,
        &user_context.name,
    );

    if let Err(error) = download_result {
        let _ = remove_if_exists(&part_path);
        return Err(error);
    }

    if !part_path.exists() {
        bail!(
            "starter AI model download did not produce {}",
            part_path.display()
        );
    }

    fs::rename(&part_path, &final_path).with_context(|| {
        format!(
            "failed to move downloaded starter AI model into place at {}",
            final_path.display()
        )
    })?;
    fs::set_permissions(&final_path, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("failed to chmod {}", final_path.display()))?;
    println!(
        "[rustfin-installer] Starter AI model stored at {}",
        final_path_string
    );
    println!(
        "[rustfin-installer] Active AI model directory for first boot: {}",
        model_dir_string
    );
    Ok(final_path)
}

fn build_native_binaries(
    repo_root: &Path,
    host: &HostPlatform,
    options: &BuildNativeBinariesOptions,
) -> anyhow::Result<()> {
    let policy = native_binary_build_policy_from_env();
    build_native_binaries_with_policy(repo_root, host, options, &policy)
}

fn build_native_runtime_artifacts(
    repo_root: &Path,
    host: &HostPlatform,
    options: &BuildNativeRuntimeArtifactsOptions,
) -> anyhow::Result<()> {
    ensure_command_available("cargo")?;
    ensure_command_available("rustc")?;
    ensure_command_available("node")?;
    ensure_command_available("npm")?;

    let requested_ai_backend =
        env::var("RUSTFIN_AI_GPU_BACKEND").unwrap_or_else(|_| "auto".to_string());
    let resolved_ai_backend = resolve_ai_gpu_backend(host, &requested_ai_backend)?;
    let server_features = server_features_for_ai_backend(&resolved_ai_backend).to_string();
    let policy = NativeBinaryBuildPolicy {
        rust_toolchain: env::var("RUSTFIN_NATIVE_RUST_TOOLCHAIN")
            .unwrap_or_else(|_| "stable".to_string()),
        gnu_compat_build: false,
        gnu_glibc_version: env::var("RUSTFIN_NATIVE_GNU_GLIBC_VERSION")
            .unwrap_or_else(|_| "2.36".to_string()),
        transcription_features: env::var("RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES")
            .unwrap_or_default(),
        server_features,
    };

    println!(
        "[rustfin-installer] resolved AI backend for native runtime build: {}",
        resolved_ai_backend
    );
    if !policy.server_features.is_empty() {
        println!(
            "[rustfin-installer] rustfin-server features: {}",
            policy.server_features
        );
    }

    let binary_options = BuildNativeBinariesOptions {
        profile: options.profile.clone(),
        output_dir: options.output_dir.clone(),
        target: options.target.clone(),
        cache_dir: Some(options.cache_dir.clone()),
        bins: vec![
            "rustfin-server".to_string(),
            "rustfin-calendar".to_string(),
            "rustfin-tmdb-agent".to_string(),
            "rustfin-youtube-agent".to_string(),
            "rustfin-transcription-agent".to_string(),
            "rustfin-servers-agent".to_string(),
        ],
    };
    build_native_binaries_with_policy(repo_root, host, &binary_options, &policy)?;
    build_native_ui(repo_root, options)?;
    println!(
        "[rustfin-installer] UI standalone entry: {}",
        repo_root.join("ui/.next/standalone/server.js").display()
    );
    Ok(())
}

fn native_binary_build_policy_from_env() -> NativeBinaryBuildPolicy {
    NativeBinaryBuildPolicy {
        rust_toolchain: env::var("RUSTFIN_NATIVE_RUST_TOOLCHAIN")
            .unwrap_or_else(|_| "stable".to_string()),
        gnu_compat_build: env::var("RUSTFIN_NATIVE_GNU_COMPAT_BUILD")
            .unwrap_or_else(|_| "1".to_string())
            == "1",
        gnu_glibc_version: env::var("RUSTFIN_NATIVE_GNU_GLIBC_VERSION")
            .unwrap_or_else(|_| "2.36".to_string()),
        transcription_features: env::var("RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES")
            .unwrap_or_default(),
        server_features: env::var("RUSTFIN_SERVER_CARGO_FEATURES").unwrap_or_default(),
    }
}

fn build_native_binaries_with_policy(
    repo_root: &Path,
    host: &HostPlatform,
    options: &BuildNativeBinariesOptions,
    policy: &NativeBinaryBuildPolicy,
) -> anyhow::Result<()> {
    ensure_command_available("cargo")?;
    ensure_command_available("rustc")?;

    let target_triple = options.target.clone().unwrap_or_else(|| {
        default_native_linux_target(host)
            .unwrap_or("x86_64-unknown-linux-gnu")
            .to_string()
    });

    let output_dir = absolutize_from_repo(repo_root, &options.output_dir)?;
    let cache_dir = match &options.cache_dir {
        Some(path) => absolutize_from_repo(repo_root, path)?,
        None => {
            let parent = output_dir
                .parent()
                .context("output dir must have a parent directory")?;
            parent.join(".build-cache")
        }
    };
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create {}", cache_dir.display()))?;

    let toolchain = resolve_rust_toolchain(&policy.rust_toolchain)?;
    let rust_host_triple = rust_host_triple(&toolchain)?;
    let target_is_gnu_linux = target_triple.contains("-unknown-linux-gnu");
    let force_gnu_compat_zig = target_is_gnu_linux && policy.gnu_compat_build;
    let use_zigbuild = rust_host_triple != target_triple || force_gnu_compat_zig;
    let zig_target = if use_zigbuild && target_is_gnu_linux && policy.gnu_compat_build {
        if target_triple.contains('.') {
            target_triple.clone()
        } else {
            format!("{target_triple}.{}", policy.gnu_glibc_version)
        }
    } else {
        target_triple.clone()
    };

    if use_zigbuild {
        ensure_command_available("zig")?;
        ensure_command_available("cargo-zigbuild")?;
    }

    ensure_rustup_target(&policy.rust_toolchain, &target_triple)?;
    let zig_env = if use_zigbuild {
        let local = cache_dir.join("zig-local");
        let global = cache_dir.join("zig-global");
        fs::create_dir_all(&local)
            .with_context(|| format!("failed to create {}", local.display()))?;
        fs::create_dir_all(&global)
            .with_context(|| format!("failed to create {}", global.display()))?;
        Some((local, global))
    } else {
        None
    };

    for bin in &options.bins {
        let mut cmd = toolchain.command();
        if use_zigbuild {
            cmd.arg("zigbuild").arg("--target").arg(&zig_target);
        } else {
            cmd.arg("build").arg("--target").arg(&target_triple);
        }
        cmd.arg("--locked");
        match options.profile.as_str() {
            "release" => {
                cmd.arg("--release");
            }
            "dev" | "debug" => {}
            profile => {
                cmd.arg("--profile").arg(profile);
            }
        }
        cmd.arg("--bin").arg(bin);
        if bin == "rustfin-server" && !policy.server_features.is_empty() {
            cmd.arg("--features").arg(&policy.server_features);
        }
        if bin == "rustfin-transcription-agent" && !policy.transcription_features.is_empty() {
            cmd.arg("--features").arg(&policy.transcription_features);
        }
        cmd.current_dir(repo_root);
        cmd.env("CARGO_TARGET_DIR", &cache_dir);
        if bin == "rustfin-server" && policy.server_features.contains("ai-cuda") {
            cmd.env("CUDA_PATH", "/usr/lib/cuda");
            cmd.env(
                "RUSTFLAGS",
                merge_search_paths_into_rustflags(
                    env::var("RUSTFLAGS").ok().as_deref(),
                    &["/usr/lib/x86_64-linux-gnu", "/usr/lib/cuda/lib64"],
                ),
            );
        }
        if let Some((local, global)) = &zig_env {
            cmd.env("ZIG_LOCAL_CACHE_DIR", local);
            cmd.env("ZIG_GLOBAL_CACHE_DIR", global);
        }

        println!(
            "[rustfin-installer] building {} (target={}, profile={})",
            bin, target_triple, options.profile
        );
        if use_zigbuild {
            println!("[rustfin-installer]   zig target: {}", zig_target);
        }
        let status = cmd
            .status()
            .with_context(|| format!("failed to build binary {bin}"))?;
        ensure_success("cargo build", status)?;

        let artifact_profile_dir = match options.profile.as_str() {
            "dev" | "debug" => "debug",
            other => other,
        };
        let preferred_target_dir = if use_zigbuild {
            cache_dir.join(&zig_target)
        } else {
            cache_dir.join(&target_triple)
        };
        let mut artifact = preferred_target_dir.join(artifact_profile_dir).join(bin);
        if use_zigbuild && !artifact.exists() && zig_target != target_triple {
            let fallback = cache_dir
                .join(&target_triple)
                .join(artifact_profile_dir)
                .join(bin);
            if fallback.exists() {
                artifact = fallback;
            }
        }
        if !artifact.exists() {
            bail!("Expected artifact missing: {}", artifact.display());
        }
        let destination = output_dir.join(bin);
        fs::copy(&artifact, &destination).with_context(|| {
            format!(
                "failed to copy artifact {} to {}",
                artifact.display(),
                destination.display()
            )
        })?;
        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&destination, permissions)
            .with_context(|| format!("failed to chmod {}", destination.display()))?;
    }

    println!("[rustfin-installer] output dir: {}", output_dir.display());
    println!("[rustfin-installer] target: {}", target_triple);
    Ok(())
}

fn merge_search_paths_into_rustflags(existing: Option<&str>, search_paths: &[&str]) -> String {
    let mut parts = existing
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();

    for path in search_paths {
        let flag = format!("-L{path}");
        if !parts.iter().any(|part| part == &flag) {
            parts.push(flag);
        }
    }

    parts.join(" ")
}

fn build_native_ui(
    repo_root: &Path,
    options: &BuildNativeRuntimeArtifactsOptions,
) -> anyhow::Result<()> {
    let ui_dir = repo_root.join("ui");
    let package_json = ui_dir.join("package.json");
    let package_lock = ui_dir.join("package-lock.json");
    let node_modules = ui_dir.join("node_modules");
    let deps_state_file = absolutize_from_repo(repo_root, &options.ui_deps_state_file)?;

    let dep_hash = hash_files(&[package_json.as_path(), package_lock.as_path()])?;
    let current_dep_hash = fs::read_to_string(&deps_state_file).unwrap_or_default();
    if !node_modules.is_dir() || current_dep_hash.trim() != dep_hash {
        println!("[rustfin-installer] installing UI dependencies natively...");
        let status = Command::new("npm")
            .arg("ci")
            .current_dir(&ui_dir)
            .status()
            .context("failed to run npm ci")?;
        ensure_success("npm ci", status)?;
        if let Some(parent) = deps_state_file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&deps_state_file, &dep_hash)
            .with_context(|| format!("failed to write {}", deps_state_file.display()))?;
    }

    println!("[rustfin-installer] building Next.js UI natively...");
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&ui_dir)
        .env("NEXT_TELEMETRY_DISABLED", "1")
        .env(
            "RUSTYFIN_API_BASE_URL",
            format!("http://127.0.0.1:{}", options.backend_port),
        )
        .env(
            "RUSTYFIN_CALENDAR_API_BASE_URL",
            format!("http://127.0.0.1:{}", options.calendar_port),
        )
        .status()
        .context("failed to run npm run build")?;
    ensure_success("npm run build", status)?;

    let standalone_public = ui_dir.join(".next/standalone/public");
    let standalone_static = ui_dir.join(".next/standalone/.next/static");
    fs::create_dir_all(&standalone_public)
        .with_context(|| format!("failed to create {}", standalone_public.display()))?;
    fs::create_dir_all(&standalone_static)
        .with_context(|| format!("failed to create {}", standalone_static.display()))?;

    let public_dir = ui_dir.join("public");
    if public_dir.is_dir() {
        copy_dir_contents(&public_dir, &standalone_public)?;
    }
    let static_dir = ui_dir.join(".next/static");
    if static_dir.is_dir() {
        copy_dir_contents(&static_dir, &standalone_static)?;
    }

    Ok(())
}

fn plan_native_runtime(
    host: &HostPlatform,
    options: &PlanNativeRuntimeOptions,
) -> anyhow::Result<()> {
    let repo_root = absolutize_from_repo(Path::new("."), &options.repo_root)?;
    let cache_dir = absolutize_from_repo(&repo_root, &options.cache_dir)?;
    let safe_tmp_dir = absolutize_from_repo(&repo_root, &options.safe_tmp_dir)?;
    let native_target = env::var("RUSTFIN_NATIVE_LINUX_TARGET")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            default_native_linux_target(host)
                .unwrap_or("x86_64-unknown-linux-gnu")
                .to_string()
        });

    let mut enable_servers_agent = env_bool_default("RUSTFIN_ENABLE_SERVERS_AGENT", true);
    let backend_port = pick_free_port(env_u16_default("RUSTFIN_BACKEND_PORT", 8096)?, 200)?;
    let calendar_port = pick_free_port(env_u16_default("RUSTFIN_CALENDAR_PORT", 8099)?, 200)?;
    let tmdb_port = pick_free_port(env_u16_default("RUSTFIN_TMDB_AGENT_PORT", 8100)?, 200)?;
    let youtube_port = pick_free_port(env_u16_default("RUSTFIN_YOUTUBE_AGENT_PORT", 8101)?, 200)?;
    let transcription_port = pick_free_port(
        env_u16_default("RUSTFIN_TRANSCRIPTION_AGENT_PORT", 8102)?,
        200,
    )?;
    let mut servers_agent_port = env_u16_default("RUSTFIN_SERVERS_AGENT_PORT", 8103)?;
    if enable_servers_agent {
        if port_in_use(servers_agent_port) {
            if external_servers_agent_configured() {
                enable_servers_agent = false;
            } else {
                bail!(
                    "Servers agent port {} is already in use and no external servers agent configuration is present.",
                    servers_agent_port
                );
            }
        } else {
            servers_agent_port = pick_free_port(servers_agent_port, 200)?;
        }
    }
    let ui_internal_port = pick_free_port(env_u16_default("RUSTFIN_UI_INTERNAL_PORT", 3001)?, 200)?;
    let ui_port = pick_free_port(env_u16_default("RUSTFIN_UI_PORT", 3000)?, 200)?;

    let public_host = env::var("RUSTFIN_PUBLIC_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            detect_primary_lan_ipv4()
                .ok()
                .flatten()
                .unwrap_or_else(|| "localhost".to_string())
        });
    let public_host_name = normalize_public_host_name(&public_host);
    let edge_tls_mode = resolve_edge_tls_mode()?;

    let browser_backend_origin = normalize_browser_backend_origin(
        env::var("RUSTYFIN_BROWSER_BACKEND_ORIGIN").ok().as_deref(),
        &public_host,
        backend_port,
        ui_port,
    );

    let ws_allowed_origins = normalize_ws_allowed_origins(
        env::var("RUSTFIN_WS_ALLOWED_ORIGINS").ok().as_deref(),
        &browser_backend_origin,
        &public_host,
        ui_port,
    );
    let edge_health_resolve =
        build_edge_health_resolve(&browser_backend_origin).unwrap_or_default();

    let media_path = resolve_media_path(&repo_root)?;
    let directory_picker_helper_url = env::var("RUSTFIN_DIRECTORY_PICKER_HELPER_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("http://127.0.0.1:{}/pick", options.picker_helper_port));
    let edge_tls_hosts = resolve_edge_tls_hosts(&public_host_name);
    let (edge_tls_cert, edge_tls_key) = if edge_tls_mode == EdgeTlsMode::Manual {
        ensure_edge_tls_cert(&safe_tmp_dir, &edge_tls_hosts, &public_host_name)?
    } else {
        (String::new(), String::new())
    };

    let database_url = resolve_database_url()?;
    validate_postgres_url(&database_url)?;
    let database_target_log = redact_postgres_url(&database_url);
    if command_exists("pg_isready") {
        let status = Command::new("pg_isready")
            .arg("-d")
            .arg(&database_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("failed to execute pg_isready")?;
        if !status.success() {
            bail!(
                "PostgreSQL is not ready at {}. Run ./scripts/install_linux.sh first, or start PostgreSQL.",
                database_target_log
            );
        }
    }

    let tmdb_agent_token = resolve_or_generate_env_secret("RUSTFIN_TMDB_AGENT_TOKEN");
    let youtube_agent_token = resolve_or_generate_env_secret("RUSTFIN_YOUTUBE_AGENT_TOKEN");
    let transcription_agent_token =
        resolve_or_generate_env_secret("RUSTFIN_TRANSCRIPTION_AGENT_TOKEN");
    let servers_agent_token = if enable_servers_agent {
        resolve_or_generate_env_secret("RUSTFIN_SERVERS_AGENT_TOKEN")
    } else {
        env::var("RUSTFIN_SERVERS_AGENT_TOKEN").unwrap_or_default()
    };
    let servers_agent_url = if enable_servers_agent {
        env::var("RUSTFIN_SERVERS_AGENT_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("http://127.0.0.1:{servers_agent_port}"))
    } else {
        env::var("RUSTFIN_SERVERS_AGENT_URL").unwrap_or_default()
    };

    let plan = [
        (
            "RUSTFIN_ENABLE_SERVERS_AGENT",
            if enable_servers_agent {
                "1".to_string()
            } else {
                "0".to_string()
            },
        ),
        ("RUSTFIN_NATIVE_TARGET", native_target),
        ("RUSTFIN_BACKEND_PORT", backend_port.to_string()),
        ("RUSTFIN_CALENDAR_PORT", calendar_port.to_string()),
        ("RUSTFIN_TMDB_AGENT_PORT", tmdb_port.to_string()),
        ("RUSTFIN_YOUTUBE_AGENT_PORT", youtube_port.to_string()),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_PORT",
            transcription_port.to_string(),
        ),
        ("RUSTFIN_SERVERS_AGENT_PORT", servers_agent_port.to_string()),
        ("RUSTFIN_UI_INTERNAL_PORT", ui_internal_port.to_string()),
        ("RUSTFIN_UI_PORT", ui_port.to_string()),
        ("RUSTFIN_PUBLIC_HOST", public_host.clone()),
        (
            "RUSTFIN_EDGE_TLS_MODE",
            match edge_tls_mode {
                EdgeTlsMode::Manual => "manual".to_string(),
                EdgeTlsMode::Auto => "auto".to_string(),
            },
        ),
        ("RUSTYFIN_BROWSER_BACKEND_ORIGIN", browser_backend_origin),
        ("RUSTFIN_WS_ALLOWED_ORIGINS", ws_allowed_origins),
        ("RUSTFIN_EDGE_HEALTH_RESOLVE", edge_health_resolve),
        (
            "RUSTFIN_WEBRTC_ICE_SERVERS_JSON",
            env::var("RUSTFIN_WEBRTC_ICE_SERVERS_JSON").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_STUN_URL",
            env::var("RUSTFIN_WEBRTC_STUN_URL").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_URL",
            env::var("RUSTFIN_WEBRTC_TURN_URL").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_URLS",
            env::var("RUSTFIN_WEBRTC_TURN_URLS").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_USERNAME",
            env::var("RUSTFIN_WEBRTC_TURN_USERNAME").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_CREDENTIAL",
            env::var("RUSTFIN_WEBRTC_TURN_CREDENTIAL").unwrap_or_default(),
        ),
        ("RUSTFIN_MEDIA_PATH", media_path.display().to_string()),
        ("RUSTFIN_EDGE_TLS_CERT", edge_tls_cert),
        ("RUSTFIN_EDGE_TLS_KEY", edge_tls_key),
        (
            "RUSTFIN_DIRECTORY_PICKER_HELPER_URL",
            directory_picker_helper_url,
        ),
        ("RUSTFIN_DATABASE_URL", database_url.clone()),
        ("RUSTFIN_DATABASE_TARGET_LOG", database_target_log),
        ("RUSTFIN_BIND", format!("127.0.0.1:{backend_port}")),
        (
            "RUSTFIN_CALENDAR_BIND",
            format!("127.0.0.1:{calendar_port}"),
        ),
        ("RUSTFIN_TMDB_AGENT_BIND", format!("127.0.0.1:{tmdb_port}")),
        (
            "RUSTFIN_YOUTUBE_AGENT_BIND",
            format!("127.0.0.1:{youtube_port}"),
        ),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_BIND",
            format!("127.0.0.1:{transcription_port}"),
        ),
        (
            "RUSTFIN_SERVERS_AGENT_BIND",
            format!("127.0.0.1:{servers_agent_port}"),
        ),
        (
            "RUSTFIN_AUTH_BASE_URL",
            format!("http://127.0.0.1:{backend_port}"),
        ),
        (
            "RUSTFIN_WHISPER_MODEL_PATH",
            cache_dir
                .join("whisper/ggml-small.en.bin")
                .display()
                .to_string(),
        ),
        (
            "RUSTFIN_WHISPER_MODEL_URL",
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin"
                .to_string(),
        ),
        (
            "RUSTFIN_TMDB_AGENT_URL",
            format!("http://127.0.0.1:{tmdb_port}"),
        ),
        (
            "RUSTFIN_YOUTUBE_AGENT_URL",
            format!("http://127.0.0.1:{youtube_port}"),
        ),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_URL",
            format!("http://127.0.0.1:{transcription_port}"),
        ),
        ("RUSTFIN_TMDB_AGENT_TOKEN", tmdb_agent_token),
        ("RUSTFIN_YOUTUBE_AGENT_TOKEN", youtube_agent_token),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_TOKEN",
            transcription_agent_token,
        ),
        ("RUSTFIN_SERVERS_AGENT_TOKEN", servers_agent_token),
        ("RUSTFIN_SERVERS_AGENT_URL", servers_agent_url),
    ];

    for (key, value) in plan {
        println!("{key}={}", shell_quote(&value));
    }

    Ok(())
}

fn deploy_native(
    repo_root: &Path,
    host: &HostPlatform,
    user_context: &NativeUserContext,
    options: &DeployNativeOptions,
) -> anyhow::Result<()> {
    let adapter = crate::distro::resolve_adapter(host);
    if adapter.name() == "unsupported" {
        bail!(
            "rustfin-installer currently supports Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04. Detected: {} {}.",
            host.id.as_deref().unwrap_or("unknown"),
            host.version_id.as_deref().unwrap_or("unknown")
        );
    }
    ensure_command_available("git")?;
    ensure_command_available("cargo")?;
    ensure_command_available("rustc")?;
    ensure_command_available("node")?;
    ensure_command_available("npm")?;

    let systemctl_available = command_exists("systemctl");
    if !systemctl_available {
        println!("[rustfin-installer] systemctl not found; deploy will fall back to direct start.");
    }

    let repo_owner_user = stat_value(repo_root, "%U")?;
    let repo_owner_group = stat_value(repo_root, "%G")?;
    if repo_owner_user.is_empty() || repo_owner_user == "UNKNOWN" {
        bail!(
            "Unable to determine repository owner for {}",
            repo_root.display()
        );
    }

    let main_service_name =
        env::var("RUSTFIN_SYSTEMD_SERVICE").unwrap_or_else(|_| "rustyfin-native.service".into());
    let agent_service_name = env::var("RUSTFIN_SERVERS_AGENT_SERVICE")
        .unwrap_or_else(|_| "rustfin-servers-agent.service".into());
    let systemd_config = build_systemd_config(repo_root);

    if user_context.uses_sudo_for_privileged_steps
        && (service_exists(&main_service_name) || service_exists(&agent_service_name))
    {
        ensure_command_available("sudo")?;
        let status = Command::new("sudo")
            .arg("-v")
            .status()
            .context("failed to refresh sudo credentials for systemd operations")?;
        ensure_success("sudo -v", status)?;
    }

    let branch_name = if options.skip_git_pull {
        None
    } else {
        let branch_name = run_command_in_dir_as_user_capture(
            "git",
            &["rev-parse", "--abbrev-ref", "HEAD"],
            repo_root,
            &repo_owner_user,
        )?;
        if branch_name == "HEAD" {
            bail!("Repository is in detached HEAD state. Check out a branch before deploying.");
        }
        let worktree_status = run_command_in_dir_as_user_capture(
            "git",
            &["status", "--short"],
            repo_root,
            &repo_owner_user,
        )?;
        if !worktree_status.trim().is_empty() {
            bail!("Working tree is not clean. Commit or stash local changes before deploying.");
        }
        Some(branch_name)
    };

    stop_service_if_present(&main_service_name, user_context)?;
    stop_service_if_present(&agent_service_name, user_context)?;

    println!("[rustfin-installer] Stopping any running native runtime processes...");
    if let Err(error) = run_script(repo_root, "scripts/stop-native.sh", &[]) {
        println!(
            "[rustfin-installer] Warning: stop-native.sh reported an error and deploy will continue: {error}"
        );
    }

    if let Some(branch_name) = branch_name.as_deref() {
        println!("[rustfin-installer] Pulling latest {}...", branch_name);
        run_command_in_dir_as_user(
            "git",
            &["pull", "--ff-only", "origin", branch_name],
            repo_root,
            &repo_owner_user,
        )?;
    } else {
        println!("[rustfin-installer] Skipping git pull.");
    }

    println!("[rustfin-installer] Refreshing native runtime defaults...");
    write_native_runtime_defaults(repo_root, host, user_context)?;

    println!("[rustfin-installer] Rebuilding native artifacts...");
    repair_build_artifact_ownership(repo_root, &repo_owner_user, &repo_owner_group, user_context)?;
    run_script_as_repo_owner(
        repo_root,
        "scripts/start-native.sh",
        &["--build-only"],
        &repo_owner_user,
    )?;

    if service_exists(&main_service_name) {
        println!("[rustfin-installer] Refreshing installed native systemd units...");
        install_systemd_units(&systemd_config, user_context)?;
        println!("[rustfin-installer] Native systemd deployment completed.");
    } else {
        println!(
            "[rustfin-installer] No native systemd unit detected; starting runtime directly..."
        );
        let mut start_args = vec!["--no-build"];
        if options.foreground {
            start_args.push("--foreground");
        }
        if options.no_health_check {
            start_args.push("--no-health-check");
        }
        run_script_as_repo_owner(
            repo_root,
            "scripts/start-native.sh",
            &start_args,
            &repo_owner_user,
        )?;
    }

    Ok(())
}

fn write_native_runtime_snapshot(
    repo_root: &Path,
    options: &WriteNativeRuntimeSnapshotOptions,
) -> anyhow::Result<()> {
    let output = absolutize_from_repo(repo_root, &options.output)?;
    write_runtime_snapshot_to_path(&output)
}

fn write_runtime_snapshot_to_path(output: &Path) -> anyhow::Result<()> {
    let database_url = resolve_database_url().unwrap_or_default();
    let keys = [
        ("RUSTFIN_RUNTIME_MODE", "native".to_string()),
        (
            "RUSTFIN_NATIVE_RUNTIME_DIR",
            env::var("RUSTFIN_NATIVE_RUNTIME_DIR").unwrap_or_default(),
        ),
        (
            "RUSTFIN_BACKEND_PORT",
            env::var("RUSTFIN_BACKEND_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_CALENDAR_PORT",
            env::var("RUSTFIN_CALENDAR_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_TMDB_AGENT_PORT",
            env::var("RUSTFIN_TMDB_AGENT_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_YOUTUBE_AGENT_PORT",
            env::var("RUSTFIN_YOUTUBE_AGENT_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_PORT",
            env::var("RUSTFIN_TRANSCRIPTION_AGENT_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_SERVERS_AGENT_PORT",
            env::var("RUSTFIN_SERVERS_AGENT_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_UI_INTERNAL_PORT",
            env::var("RUSTFIN_UI_INTERNAL_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_UI_PORT",
            env::var("RUSTFIN_UI_PORT").unwrap_or_default(),
        ),
        (
            "RUSTFIN_PUBLIC_HOST",
            env::var("RUSTFIN_PUBLIC_HOST").unwrap_or_default(),
        ),
        (
            "RUSTFIN_EDGE_TLS_MODE",
            env::var("RUSTFIN_EDGE_TLS_MODE").unwrap_or_default(),
        ),
        (
            "RUSTFIN_MEDIA_PATH",
            env::var("RUSTFIN_MEDIA_PATH").unwrap_or_default(),
        ),
        (
            "RUSTYFIN_BROWSER_BACKEND_ORIGIN",
            env::var("RUSTYFIN_BROWSER_BACKEND_ORIGIN").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WS_ALLOWED_ORIGINS",
            env::var("RUSTFIN_WS_ALLOWED_ORIGINS").unwrap_or_default(),
        ),
        (
            "RUSTFIN_EDGE_HEALTH_RESOLVE",
            env::var("RUSTFIN_EDGE_HEALTH_RESOLVE").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_ICE_SERVERS_JSON",
            env::var("RUSTFIN_WEBRTC_ICE_SERVERS_JSON").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_STUN_URL",
            env::var("RUSTFIN_WEBRTC_STUN_URL").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_URL",
            env::var("RUSTFIN_WEBRTC_TURN_URL").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_URLS",
            env::var("RUSTFIN_WEBRTC_TURN_URLS").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_USERNAME",
            env::var("RUSTFIN_WEBRTC_TURN_USERNAME").unwrap_or_default(),
        ),
        (
            "RUSTFIN_WEBRTC_TURN_CREDENTIAL",
            env::var("RUSTFIN_WEBRTC_TURN_CREDENTIAL").unwrap_or_default(),
        ),
        (
            "RUSTFIN_DIRECTORY_PICKER_HELPER_URL",
            env::var("RUSTFIN_DIRECTORY_PICKER_HELPER_URL").unwrap_or_default(),
        ),
        ("RUSTFIN_DATABASE_URL", database_url),
        (
            "RUSTFIN_TMDB_AGENT_TOKEN",
            env::var("RUSTFIN_TMDB_AGENT_TOKEN").unwrap_or_default(),
        ),
        (
            "RUSTFIN_YOUTUBE_AGENT_TOKEN",
            env::var("RUSTFIN_YOUTUBE_AGENT_TOKEN").unwrap_or_default(),
        ),
        (
            "RUSTFIN_TRANSCRIPTION_AGENT_TOKEN",
            env::var("RUSTFIN_TRANSCRIPTION_AGENT_TOKEN").unwrap_or_default(),
        ),
        (
            "RUSTFIN_SERVERS_AGENT_TOKEN",
            env::var("RUSTFIN_SERVERS_AGENT_TOKEN").unwrap_or_default(),
        ),
        (
            "RUSTFIN_SERVERS_AGENT_URL",
            env::var("RUSTFIN_SERVERS_AGENT_URL").unwrap_or_default(),
        ),
    ];

    let mut rendered =
        String::from("# Generated by rustfin-installer write-native-runtime-snapshot\n");
    for (key, value) in keys {
        rendered.push_str(key);
        rendered.push('=');
        rendered.push_str(&shell_quote(&value));
        rendered.push('\n');
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(output, rendered).with_context(|| format!("failed to write {}", output.display()))?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod {}", output.display()))?;
    Ok(())
}

fn launch_native_runtime(
    repo_root: &Path,
    options: &LaunchNativeRuntimeOptions,
) -> anyhow::Result<()> {
    ensure_command_available("curl")?;
    ensure_command_available("nohup")?;

    let runtime_paths = resolve_runtime_paths(repo_root)?;
    fs::create_dir_all(&runtime_paths.pid_dir)
        .with_context(|| format!("failed to create {}", runtime_paths.pid_dir.display()))?;
    fs::create_dir_all(&runtime_paths.log_dir)
        .with_context(|| format!("failed to create {}", runtime_paths.log_dir.display()))?;

    let enable_servers_agent = env_bool_default("RUSTFIN_ENABLE_SERVERS_AGENT", true);

    if !options.build_only {
        maybe_start_directory_picker_helper(&runtime_paths)?;
        assert_runtime_not_running(&runtime_paths.pid_dir, enable_servers_agent)?;
    }

    let build_profile =
        env::var("RUSTFIN_RUST_BUILD_PROFILE").unwrap_or_else(|_| "release".to_string());
    let native_target = env::var("RUSTFIN_NATIVE_TARGET")
        .or_else(|_| env::var("RUSTFIN_NATIVE_LINUX_TARGET"))
        .unwrap_or_else(|_| "x86_64-unknown-linux-gnu".to_string());
    let native_bin_dir = repo_root
        .join(".native-bins")
        .join(&native_target)
        .join(&build_profile);

    if options.build_only {
        println!("[rustfin-installer] Native artifacts built successfully.");
        println!(
            "[rustfin-installer] Native binary output dir: {}",
            native_bin_dir.display()
        );
        println!(
            "[rustfin-installer] UI standalone entry: {}",
            repo_root.join("ui/.next/standalone/server.js").display()
        );
        return Ok(());
    }

    let tmdb_agent = native_bin_dir.join("rustfin-tmdb-agent");
    let youtube_agent = native_bin_dir.join("rustfin-youtube-agent");
    let transcription_agent = native_bin_dir.join("rustfin-transcription-agent");
    let servers_agent = native_bin_dir.join("rustfin-servers-agent");
    let server = native_bin_dir.join("rustfin-server");
    let calendar = native_bin_dir.join("rustfin-calendar");

    start_background_process(
        "rustfin-tmdb-agent",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        tmdb_agent.as_path(),
        &[],
        &[],
    )?;
    start_background_process(
        "rustfin-youtube-agent",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        youtube_agent.as_path(),
        &[],
        &[],
    )?;
    start_background_process(
        "rustfin-transcription-agent",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        transcription_agent.as_path(),
        &[],
        &[],
    )?;
    if enable_servers_agent {
        start_background_process(
            "rustfin-servers-agent",
            repo_root,
            &runtime_paths.log_dir,
            &runtime_paths.pid_dir,
            servers_agent.as_path(),
            &[],
            &[],
        )?;
    }
    start_background_process(
        "rustfin",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        server.as_path(),
        &[],
        &[],
    )?;
    start_background_process(
        "rustfin-calendar",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        calendar.as_path(),
        &[],
        &[],
    )?;
    start_background_process(
        "rustfin-ui",
        &repo_root.join("ui/.next/standalone"),
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        Path::new("env"),
        &[
            format!(
                "PORT={}",
                env::var("RUSTFIN_UI_INTERNAL_PORT").unwrap_or_default()
            ),
            "HOSTNAME=127.0.0.1".to_string(),
        ],
        &["node".to_string(), "server.js".to_string()],
    )?;
    let edge_config_path = resolve_edge_caddy_config_path(
        repo_root,
        &runtime_paths.runtime_root.join("config"),
        env::var("RUSTFIN_EDGE_TLS_MODE")
            .unwrap_or_else(|_| "manual".to_string())
            .as_str(),
        env::var("RUSTFIN_PUBLIC_HOST")
            .unwrap_or_else(|_| "localhost".to_string())
            .as_str(),
        env::var("RUSTFIN_UI_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(3000),
        env::var("RUSTFIN_EDGE_TLS_CERT")
            .unwrap_or_default()
            .as_str(),
        env::var("RUSTFIN_EDGE_TLS_KEY")
            .unwrap_or_default()
            .as_str(),
    )?;

    start_background_process(
        "rustfin-edge",
        repo_root,
        &runtime_paths.log_dir,
        &runtime_paths.pid_dir,
        Path::new("caddy"),
        &[],
        &[
            "run".to_string(),
            "--config".to_string(),
            edge_config_path.display().to_string(),
            "--adapter".to_string(),
            "caddyfile".to_string(),
        ],
    )?;

    write_runtime_snapshot_to_path(&runtime_paths.runtime_env_file)?;

    if !options.no_health_check {
        let backend_port = env::var("RUSTFIN_BACKEND_PORT").unwrap_or_else(|_| "8096".to_string());
        let calendar_port =
            env::var("RUSTFIN_CALENDAR_PORT").unwrap_or_else(|_| "8099".to_string());
        let tmdb_port = env::var("RUSTFIN_TMDB_AGENT_PORT").unwrap_or_else(|_| "8100".to_string());
        let youtube_port =
            env::var("RUSTFIN_YOUTUBE_AGENT_PORT").unwrap_or_else(|_| "8101".to_string());
        let transcription_port =
            env::var("RUSTFIN_TRANSCRIPTION_AGENT_PORT").unwrap_or_else(|_| "8102".to_string());
        let servers_agent_port =
            env::var("RUSTFIN_SERVERS_AGENT_PORT").unwrap_or_else(|_| "8103".to_string());
        let ui_internal_port =
            env::var("RUSTFIN_UI_INTERNAL_PORT").unwrap_or_else(|_| "3001".to_string());
        let browser_backend_origin = env::var("RUSTYFIN_BROWSER_BACKEND_ORIGIN")
            .unwrap_or_else(|_| "https://127.0.0.1:3000".to_string());
        let edge_health_resolve = env::var("RUSTFIN_EDGE_HEALTH_RESOLVE").unwrap_or_default();

        let _ = wait_for_http(
            "rustfin",
            &format!("http://127.0.0.1:{backend_port}/health"),
            120,
            false,
        )?;
        let _ = wait_for_http(
            "calendar",
            &format!("http://127.0.0.1:{calendar_port}/health"),
            60,
            false,
        )?;
        let _ = wait_for_http(
            "tmdb-agent",
            &format!("http://127.0.0.1:{tmdb_port}/health"),
            60,
            false,
        )?;
        let _ = wait_for_http(
            "youtube-agent",
            &format!("http://127.0.0.1:{youtube_port}/health"),
            60,
            false,
        )?;
        let _ = wait_for_http(
            "transcription-agent",
            &format!("http://127.0.0.1:{transcription_port}/health"),
            60,
            false,
        )?;
        if enable_servers_agent {
            let _ = wait_for_http(
                "servers-agent",
                &format!("http://127.0.0.1:{servers_agent_port}/health"),
                60,
                false,
            )?;
        }
        let _ = wait_for_http(
            "ui-internal",
            &format!("http://127.0.0.1:{ui_internal_port}"),
            60,
            false,
        )?;
        let _ = wait_for_edge_origin_path(
            "ui-edge",
            &browser_backend_origin,
            "/health",
            &edge_health_resolve,
            60,
        )?;
    }

    println!("[rustfin-installer] Rustyfin native runtime is up.");
    println!(
        "[rustfin-installer] UI: {}",
        env::var("RUSTYFIN_BROWSER_BACKEND_ORIGIN")
            .unwrap_or_else(|_| "https://localhost:3000".to_string())
    );
    println!(
        "[rustfin-installer] Logs: {}",
        runtime_paths.log_dir.display()
    );

    if options.foreground {
        tail_log_files(&runtime_paths.log_dir)?;
    }

    Ok(())
}

fn stop_native_runtime(repo_root: &Path) -> anyhow::Result<()> {
    let runtime_paths = resolve_runtime_paths(repo_root)?;
    let services = [
        "rustfin-edge",
        "rustfin-ui",
        "rustfin-calendar",
        "rustfin",
        "rustfin-servers-agent",
        "rustfin-transcription-agent",
        "rustfin-youtube-agent",
        "rustfin-tmdb-agent",
    ];
    for service in services {
        stop_pid_file_process(&runtime_paths.pid_dir.join(format!("{service}.pid")))?;
    }

    let helper_pid_file = runtime_paths
        .safe_tmp_dir
        .join("directory-picker-helper.pid");
    if helper_pid_file.exists() {
        stop_pid_file_process(&helper_pid_file)?;
    }

    let picker_helper_port =
        env::var("RUSTFIN_PICKER_HELPER_PORT").unwrap_or_else(|_| "43110".to_string());
    if command_exists("lsof") {
        let pids = run_command_capture(
            "lsof",
            &["-ti", &format!("tcp:{picker_helper_port}"), "-sTCP:LISTEN"],
        )
        .unwrap_or_default();
        for pid in pids.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let _ = kill_pid(pid, false);
        }
    }

    println!("[rustfin-installer] Rustyfin native runtime stopped.");
    Ok(())
}

fn clean_native_runtime(
    repo_root: &Path,
    user_context: &NativeUserContext,
    options: &CleanNativeRuntimeOptions,
) -> anyhow::Result<()> {
    if !options.yes {
        bail!("clean-native-runtime is destructive; rerun with --yes");
    }

    let main_service_name =
        env::var("RUSTFIN_SYSTEMD_SERVICE").unwrap_or_else(|_| "rustyfin-native.service".into());
    let agent_service_name = env::var("RUSTFIN_SERVERS_AGENT_SERVICE")
        .unwrap_or_else(|_| "rustfin-servers-agent.service".into());
    stop_service_if_present(&main_service_name, user_context)?;
    stop_service_if_present(&agent_service_name, user_context)?;
    let _ = stop_native_runtime(repo_root);

    let database_url = resolve_clean_database_url(repo_root)?;
    if command_exists("psql") {
        println!("[rustfin-installer] Resetting PostgreSQL schema contents...");
        let sql = "DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public; GRANT ALL ON SCHEMA public TO CURRENT_USER; GRANT ALL ON SCHEMA public TO public;";
        let status = Command::new("psql")
            .arg(&database_url)
            .arg("-v")
            .arg("ON_ERROR_STOP=1")
            .arg("-c")
            .arg(sql)
            .status()
            .context("failed to execute psql for clean-native-runtime")?;
        ensure_success("psql schema reset", status)?;
    } else {
        println!("[rustfin-installer] Warning: psql not found; skipping database reset.");
    }

    let runtime_paths = resolve_runtime_paths(repo_root)?;
    remove_if_exists(&runtime_paths.runtime_env_file)?;
    remove_if_exists(
        &runtime_paths
            .safe_tmp_dir
            .join("directory-picker-helper.py"),
    )?;
    remove_if_exists(
        &runtime_paths
            .safe_tmp_dir
            .join("directory-picker-helper.log"),
    )?;
    remove_if_exists(
        &runtime_paths
            .safe_tmp_dir
            .join("directory-picker-helper.pid"),
    )?;
    remove_dir_if_exists(&runtime_paths.runtime_root)?;
    remove_dir_if_exists(&PathBuf::from("/tmp/rustfin_cache"))?;
    remove_dir_if_exists(&PathBuf::from("/tmp/rustfin_transcode"))?;
    remove_dir_if_exists(&repo_root.join("tests/_runs"))?;

    println!("[rustfin-installer] Native clean install reset complete.");
    Ok(())
}

fn resolve_or_generate_env_secret(key: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(generate_secret_hex)
}

fn push_unique_host(target: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if target
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    {
        return;
    }
    target.push(trimmed.to_string());
}

fn detect_local_hostname_candidates() -> Vec<String> {
    let mut hosts = Vec::new();
    if let Ok(value) = env::var("HOSTNAME") {
        push_unique_host(&mut hosts, value);
    }
    for args in [["hostname"].as_slice(), ["hostname", "-f"].as_slice()] {
        let output = Command::new(args[0]).args(&args[1..]).output();
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let value = String::from_utf8_lossy(&output.stdout);
        push_unique_host(&mut hosts, value.trim());
    }
    hosts.retain(|host| !host.eq_ignore_ascii_case("localhost"));
    hosts
}

fn resolve_edge_tls_hosts(public_host: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    push_unique_host(&mut hosts, "localhost");
    push_unique_host(&mut hosts, "127.0.0.1");
    push_unique_host(&mut hosts, public_host);
    for host in detect_local_hostname_candidates() {
        push_unique_host(&mut hosts, host);
    }
    hosts
}

fn format_edge_tls_subject_alt_names(hosts: &[String]) -> String {
    hosts
        .iter()
        .map(|host| {
            if is_ipv4(host) {
                format!("IP:{host}")
            } else {
                format!("DNS:{host}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn ensure_edge_tls_cert(
    safe_tmp_dir: &Path,
    hosts: &[String],
    common_name: &str,
) -> anyhow::Result<(String, String)> {
    ensure_command_available("openssl")?;
    let cert_dir = safe_tmp_dir.join("edge-tls");
    let cert_path = cert_dir.join("tls.crt");
    let key_path = cert_dir.join("tls.key");
    let meta_path = cert_dir.join("meta.hosts");
    let meta_hosts = hosts.join("\n");

    fs::create_dir_all(&cert_dir)
        .with_context(|| format!("failed to create {}", cert_dir.display()))?;
    let _ = fs::set_permissions(&cert_dir, fs::Permissions::from_mode(0o700));

    let needs_regen = !cert_path.exists()
        || !key_path.exists()
        || fs::read_to_string(&meta_path).ok().as_deref() != Some(meta_hosts.as_str());

    if needs_regen {
        let san = format_edge_tls_subject_alt_names(hosts);
        let _ = fs::remove_file(&cert_path);
        let _ = fs::remove_file(&key_path);
        let status = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-sha256", "-days", "365", "-nodes",
                "-keyout",
            ])
            .arg(&key_path)
            .arg("-out")
            .arg(&cert_path)
            .arg("-subj")
            .arg(format!("/CN={common_name}"))
            .arg("-addext")
            .arg(format!("subjectAltName={san}"))
            .status()
            .context("failed generating local TLS cert")?;
        ensure_success("openssl req", status)?;
        let _ = fs::set_permissions(&cert_path, fs::Permissions::from_mode(0o600));
        let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
        fs::write(&meta_path, &meta_hosts)
            .with_context(|| format!("failed to write {}", meta_path.display()))?;
        let _ = fs::set_permissions(&meta_path, fs::Permissions::from_mode(0o600));
    }

    Ok((
        cert_path.display().to_string(),
        key_path.display().to_string(),
    ))
}

fn resolve_edge_tls_mode() -> anyhow::Result<EdgeTlsMode> {
    let raw = env::var("RUSTFIN_EDGE_TLS_MODE")
        .unwrap_or_else(|_| "manual".to_string())
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "" | "manual" => Ok(EdgeTlsMode::Manual),
        "auto" => Ok(EdgeTlsMode::Auto),
        other => bail!("Unsupported RUSTFIN_EDGE_TLS_MODE={other}. Use manual or auto."),
    }
}

fn strip_host_candidate(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let no_scheme = trimmed
        .split_once("://")
        .map(|(_, value)| value)
        .unwrap_or(trimmed);
    no_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(no_scheme)
        .trim()
        .trim_matches('/')
        .to_string()
}

fn split_host_port(authority: &str) -> (String, Option<u16>) {
    if authority.is_empty() {
        return (String::new(), None);
    }

    if let Some(rest) = authority.strip_prefix('[')
        && let Some((host, suffix)) = rest.split_once(']')
    {
        let port = suffix
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok());
        return (host.to_string(), port);
    }

    if authority.matches(':').count() == 1
        && let Some((host, port)) = authority.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
    {
        return (host.to_string(), Some(port));
    }

    (authority.to_string(), None)
}

fn normalize_public_host_name(public_host: &str) -> String {
    let candidate = strip_host_candidate(public_host);
    let (host, _) = split_host_port(&candidate);
    if host.is_empty() {
        "localhost".to_string()
    } else {
        host
    }
}

fn default_port_for_scheme(scheme: &str) -> u16 {
    match scheme {
        "http" => 80,
        _ => 443,
    }
}

fn format_http_origin(scheme: &str, host: &str, port: u16) -> String {
    let default_port = default_port_for_scheme(scheme);
    if port == default_port {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

fn parse_http_origin(raw: &str) -> Option<HttpOriginParts> {
    let trimmed = raw.trim();
    let (scheme, rest) = if let Some(value) = trimmed.strip_prefix("https://") {
        ("https".to_string(), value)
    } else if let Some(value) = trimmed.strip_prefix("http://") {
        ("http".to_string(), value)
    } else {
        return None;
    };

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let (host, explicit_port) = split_host_port(authority);
    if host.is_empty() {
        return None;
    }

    Some(HttpOriginParts {
        scheme: scheme.clone(),
        host,
        port: explicit_port.unwrap_or_else(|| default_port_for_scheme(&scheme)),
    })
}

fn build_public_browser_origin(public_host: &str, ui_port: u16) -> String {
    let candidate = strip_host_candidate(public_host);
    let (host, explicit_port) = split_host_port(&candidate);
    let host = if host.is_empty() {
        "localhost".to_string()
    } else {
        host
    };
    format_http_origin("https", &host, explicit_port.unwrap_or(ui_port))
}

fn normalize_browser_backend_origin(
    configured_origin: Option<&str>,
    public_host: &str,
    backend_port: u16,
    ui_port: u16,
) -> String {
    let edge_origin = build_public_browser_origin(public_host, ui_port);
    let Some(configured) = configured_origin
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return edge_origin;
    };
    let Some(parsed) = parse_http_origin(configured) else {
        return edge_origin;
    };

    let public_host_name = normalize_public_host_name(public_host);
    let looks_like_legacy_backend_origin = parsed.port == backend_port
        && (parsed.host.eq_ignore_ascii_case(&public_host_name)
            || parsed.host.eq_ignore_ascii_case("localhost")
            || parsed.host == "127.0.0.1");

    if looks_like_legacy_backend_origin {
        edge_origin
    } else {
        format_http_origin(&parsed.scheme, &parsed.host, parsed.port)
    }
}

fn build_legacy_ws_allowed_origins(public_host: &str, ui_port: u16) -> String {
    let mut origins = vec![
        format!("http://localhost:{ui_port}"),
        format!("http://127.0.0.1:{ui_port}"),
        format!("https://localhost:{ui_port}"),
        format!("https://127.0.0.1:{ui_port}"),
    ];
    if public_host != "localhost" && public_host != "127.0.0.1" {
        origins.push(format!("http://{public_host}:{ui_port}"));
        origins.push(format!("https://{public_host}:{ui_port}"));
    }
    origins.join(",")
}

fn normalize_ws_allowed_origins(
    configured_origins: Option<&str>,
    browser_origin: &str,
    public_host: &str,
    ui_port: u16,
) -> String {
    let Some(configured) = configured_origins
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return build_ws_allowed_origins(browser_origin, ui_port);
    };

    if configured == build_legacy_ws_allowed_origins(public_host, ui_port) {
        build_ws_allowed_origins(browser_origin, ui_port)
    } else {
        configured.to_string()
    }
}

fn build_edge_health_resolve(origin: &str) -> Option<String> {
    let parsed = parse_http_origin(origin)?;
    if parsed.host.eq_ignore_ascii_case("localhost")
        || parsed.host == "127.0.0.1"
        || parsed.host == "0.0.0.0"
        || parsed.host == "::1"
        || parsed.host == "[::1]"
        || is_ipv4(&parsed.host)
    {
        return None;
    }
    Some(format!("{}:{}:127.0.0.1", parsed.host, parsed.port))
}

fn build_edge_site_address(
    public_host: &str,
    ui_port: u16,
    edge_tls_mode: EdgeTlsMode,
) -> anyhow::Result<String> {
    match edge_tls_mode {
        EdgeTlsMode::Manual => Ok(format!(":{ui_port}")),
        EdgeTlsMode::Auto => {
            let candidate = strip_host_candidate(public_host);
            let (host, explicit_port) = split_host_port(&candidate);
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") || is_ipv4(&host) {
                bail!(
                    "RUSTFIN_EDGE_TLS_MODE=auto requires RUSTFIN_PUBLIC_HOST to be a real hostname, not {}",
                    if candidate.is_empty() {
                        "<empty>"
                    } else {
                        candidate.as_str()
                    }
                );
            }
            let port = explicit_port.unwrap_or(ui_port);
            Ok(if port == 443 {
                host
            } else {
                format!("{host}:{port}")
            })
        }
    }
}

fn render_auto_https_caddyfile(site_address: &str) -> String {
    format!(
        "{{\n  admin off\n}}\n\n# Native Debian host HTTPS edge for UI + API proxying.\n{site_address} {{\n  encode zstd gzip\n\n  header {{\n    Strict-Transport-Security \"max-age=31536000; includeSubDomains\"\n  }}\n\n  @calendar path /api/v1/calendar/*\n  reverse_proxy @calendar 127.0.0.1:{{$RUSTFIN_CALENDAR_PORT}}\n\n  @backend path /api/* /stream/* /health\n  reverse_proxy @backend 127.0.0.1:{{$RUSTFIN_BACKEND_PORT}}\n\n  reverse_proxy 127.0.0.1:{{$RUSTFIN_UI_INTERNAL_PORT}}\n}}\n"
    )
}

fn resolve_edge_caddy_config_path(
    repo_root: &Path,
    output_dir: &Path,
    edge_tls_mode_raw: &str,
    public_host: &str,
    ui_port: u16,
    edge_tls_cert: &str,
    edge_tls_key: &str,
) -> anyhow::Result<PathBuf> {
    let edge_tls_mode = match edge_tls_mode_raw.trim().to_ascii_lowercase().as_str() {
        "auto" => EdgeTlsMode::Auto,
        _ => EdgeTlsMode::Manual,
    };

    match edge_tls_mode {
        EdgeTlsMode::Manual => {
            let _ = (edge_tls_cert, edge_tls_key);
            Ok(repo_root.join("scripts/caddy/Caddyfile.native"))
        }
        EdgeTlsMode::Auto => {
            fs::create_dir_all(output_dir)
                .with_context(|| format!("failed to create {}", output_dir.display()))?;
            let rendered = render_auto_https_caddyfile(&build_edge_site_address(
                public_host,
                ui_port,
                EdgeTlsMode::Auto,
            )?);
            let path = output_dir.join("Caddyfile.native");
            fs::write(&path, rendered)
                .with_context(|| format!("failed to write {}", path.display()))?;
            Ok(path)
        }
    }
}

fn detect_primary_lan_ipv4() -> anyhow::Result<Option<String>> {
    if !command_exists("ip") {
        return Ok(None);
    }

    let output = Command::new("ip")
        .args(["route", "get", "1.1.1.1"])
        .output()
        .context("failed to probe primary LAN IPv4 address")?;
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    for window in parts.windows(2) {
        if window[0] == "src" && is_ipv4(window[1]) && !window[1].starts_with("127.") {
            return Ok(Some(window[1].to_string()));
        }
    }

    Ok(None)
}

/// Returns true when a CUDA build toolchain is detectable on this host.
fn is_ipv4(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts
        .into_iter()
        .all(|part| !part.is_empty() && part.parse::<u8>().is_ok())
}

fn hash_files(paths: &[&Path]) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    for path in paths {
        let mut file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        hasher.update(path.display().to_string().as_bytes());
        let mut buffer = [0_u8; 8192];
        loop {
            let read = file
                .read(&mut buffer)
                .with_context(|| format!("failed to read {}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

fn copy_dir_contents(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let entry_path = entry.path();
        let dest_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", entry_path.display()))?;
        if file_type.is_dir() {
            copy_dir_contents(&entry_path, &dest_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&entry_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct RuntimePaths {
    safe_tmp_dir: PathBuf,
    runtime_root: PathBuf,
    pid_dir: PathBuf,
    log_dir: PathBuf,
    runtime_env_file: PathBuf,
}

fn resolve_runtime_paths(repo_root: &Path) -> anyhow::Result<RuntimePaths> {
    let runtime_snapshot = load_shell_env_map(&repo_root.join(".rustyfin.runtime.env"))?;
    let safe_tmp_dir = env::var("RUSTFIN_TMPDIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join(".tmp"));
    let runtime_root = env::var("RUSTFIN_NATIVE_RUNTIME_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            runtime_snapshot
                .get("RUSTFIN_NATIVE_RUNTIME_DIR")
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| safe_tmp_dir.join("native-runtime"));

    Ok(RuntimePaths {
        safe_tmp_dir,
        pid_dir: runtime_root.join("pids"),
        log_dir: runtime_root.join("logs"),
        runtime_env_file: repo_root.join(".rustyfin.runtime.env"),
        runtime_root,
    })
}

fn maybe_start_directory_picker_helper(runtime_paths: &RuntimePaths) -> anyhow::Result<()> {
    if !env_bool_default("RUSTFIN_ENABLE_PICKER_HELPER", true) {
        println!(
            "[rustfin-installer] Directory picker helper disabled (RUSTFIN_ENABLE_PICKER_HELPER=0)."
        );
        return Ok(());
    }
    if env::var("DISPLAY")
        .ok()
        .filter(|value| !value.is_empty())
        .is_none()
        && env::var("WAYLAND_DISPLAY")
            .ok()
            .filter(|value| !value.is_empty())
            .is_none()
    {
        println!(
            "[rustfin-installer] No graphical session detected; native directory picker helper not started."
        );
        return Ok(());
    }

    let python = if command_exists("python3") {
        "python3"
    } else if command_exists("python") {
        "python"
    } else {
        println!(
            "[rustfin-installer] Python not found; native directory picker helper not started."
        );
        return Ok(());
    };

    let picker_helper_port =
        env::var("RUSTFIN_PICKER_HELPER_PORT").unwrap_or_else(|_| "43110".to_string());
    let picker_helper_host =
        env::var("RUSTFIN_PICKER_HELPER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let health_url = format!("http://127.0.0.1:{picker_helper_port}/health");
    if http_ready(&health_url, false)? {
        println!(
            "[rustfin-installer] Directory picker helper already running on port {}.",
            picker_helper_port
        );
        return Ok(());
    }

    let helper_pid_file = runtime_paths
        .safe_tmp_dir
        .join("directory-picker-helper.pid");
    cleanup_stale_pid_file(&helper_pid_file)?;
    if helper_pid_file.exists() {
        let existing_pid = fs::read_to_string(&helper_pid_file).unwrap_or_default();
        println!(
            "[rustfin-installer] Directory picker helper already running (pid {}).",
            existing_pid.trim()
        );
        return Ok(());
    }

    let helper_script = runtime_paths
        .safe_tmp_dir
        .join("directory-picker-helper.py");
    let helper_log = runtime_paths
        .safe_tmp_dir
        .join("directory-picker-helper.log");
    fs::create_dir_all(&runtime_paths.safe_tmp_dir)
        .with_context(|| format!("failed to create {}", runtime_paths.safe_tmp_dir.display()))?;
    fs::write(&helper_script, DIRECTORY_PICKER_HELPER_SCRIPT).with_context(|| {
        format!(
            "failed to write directory picker helper {}",
            helper_script.display()
        )
    })?;
    fs::set_permissions(&helper_script, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to chmod {}", helper_script.display()))?;

    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&helper_log)
        .with_context(|| format!("failed to open {}", helper_log.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone {}", helper_log.display()))?;
    let child = Command::new("nohup")
        .arg(python)
        .arg(&helper_script)
        .env("RUSTFIN_PICKER_HELPER_PORT", &picker_helper_port)
        .env("RUSTFIN_PICKER_HELPER_HOST", &picker_helper_host)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .context("failed to start directory picker helper")?;
    fs::write(&helper_pid_file, child.id().to_string())
        .with_context(|| format!("failed to write {}", helper_pid_file.display()))?;

    for _ in 0..20 {
        if http_ready(&health_url, false)? {
            println!(
                "[rustfin-installer] Directory picker helper started on {} (pid {})",
                health_url,
                child.id()
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }

    println!(
        "[rustfin-installer] Warning: directory picker helper did not report healthy; check {}",
        helper_log.display()
    );
    Ok(())
}

fn assert_runtime_not_running(pid_dir: &Path, enable_servers_agent: bool) -> anyhow::Result<()> {
    let mut services = vec![
        "rustfin",
        "rustfin-calendar",
        "rustfin-tmdb-agent",
        "rustfin-youtube-agent",
        "rustfin-transcription-agent",
        "rustfin-ui",
        "rustfin-edge",
    ];
    if enable_servers_agent {
        services.push("rustfin-servers-agent");
    }

    for service in services {
        let pidfile = pid_dir.join(format!("{service}.pid"));
        cleanup_stale_pid_file(&pidfile)?;
        if pidfile.exists() {
            bail!(
                "Native runtime already appears to be running ({}). Stop it first with ./scripts/stop-native.sh",
                service
            );
        }
    }
    Ok(())
}

fn cleanup_stale_pid_file(pidfile: &Path) -> anyhow::Result<()> {
    if !pidfile.exists() {
        return Ok(());
    }
    let pid = fs::read_to_string(pidfile).unwrap_or_default();
    let pid = pid.trim();
    if pid.is_empty()
        || !pid.chars().all(|ch| ch.is_ascii_digit())
        || !pid_matches_pidfile(pidfile, pid)
    {
        let _ = fs::remove_file(pidfile);
    }
    Ok(())
}

fn start_background_process(
    name: &str,
    workdir: &Path,
    log_dir: &Path,
    pid_dir: &Path,
    program: &Path,
    env_pairs: &[String],
    extra_args: &[String],
) -> anyhow::Result<()> {
    let logfile = log_dir.join(format!("{name}.log"));
    let pidfile = pid_dir.join(format!("{name}.pid"));
    cleanup_stale_pid_file(&pidfile)?;
    if pidfile.exists() {
        bail!("{name} is already running");
    }

    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logfile)
        .with_context(|| format!("failed to open {}", logfile.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone {}", logfile.display()))?;

    let mut cmd = Command::new("nohup");
    cmd.arg(program);
    for pair in env_pairs {
        cmd.arg(pair);
    }
    for arg in extra_args {
        cmd.arg(arg);
    }
    let child = cmd
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("failed starting {name}"))?;
    fs::write(&pidfile, child.id().to_string())
        .with_context(|| format!("failed to write {}", pidfile.display()))?;
    thread::sleep(Duration::from_millis(300));
    if !pid_matches_pidfile(&pidfile, &child.id().to_string()) {
        bail!("Failed starting {name}. Check {}", logfile.display());
    }
    println!("[rustfin-installer] Started {} (pid {})", name, child.id());
    Ok(())
}

fn wait_for_http(name: &str, url: &str, attempts: usize, insecure: bool) -> anyhow::Result<bool> {
    for _ in 0..attempts {
        if http_ready(url, insecure)? {
            return Ok(true);
        }
        thread::sleep(Duration::from_secs(1));
    }
    println!(
        "[rustfin-installer] Warning: {} did not become ready: {}",
        name, url
    );
    Ok(false)
}

fn wait_for_edge_origin_path(
    name: &str,
    origin: &str,
    path: &str,
    resolve_override: &str,
    attempts: usize,
) -> anyhow::Result<bool> {
    for _ in 0..attempts {
        if edge_origin_ready(origin, path, resolve_override)? {
            return Ok(true);
        }
        thread::sleep(Duration::from_secs(1));
    }
    println!(
        "[rustfin-installer] Warning: {} did not become ready: {}{}",
        name, origin, path
    );
    Ok(false)
}

fn http_ready(url: &str, insecure: bool) -> anyhow::Result<bool> {
    let mut cmd = Command::new("curl");
    cmd.arg("-fsS");
    if insecure {
        cmd.arg("-k");
    }
    let status = cmd.arg(url).status().context("failed to execute curl")?;
    Ok(status.success())
}

fn edge_origin_ready(origin: &str, path: &str, resolve_override: &str) -> anyhow::Result<bool> {
    let mut cmd = edge_origin_curl_command(origin, path, resolve_override)?;
    let status = cmd.status().context("failed to execute curl")?;
    Ok(status.success())
}

fn edge_origin_curl_command(
    origin: &str,
    path: &str,
    resolve_override: &str,
) -> anyhow::Result<Command> {
    let origin_parts = parse_http_origin(origin)
        .ok_or_else(|| anyhow::anyhow!("invalid edge origin: {origin}"))?;
    let mut cmd = Command::new("curl");
    cmd.arg("-k").arg("-fsS");
    if !resolve_override.trim().is_empty() {
        cmd.arg("--resolve").arg(resolve_override.trim());
    }
    cmd.arg(format!(
        "{}{}",
        format_http_origin(&origin_parts.scheme, &origin_parts.host, origin_parts.port),
        normalize_origin_path(path)
    ));
    Ok(cmd)
}

fn normalize_origin_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn tail_log_files(log_dir: &Path) -> anyhow::Result<()> {
    let mut log_files = Vec::new();
    for entry in
        fs::read_dir(log_dir).with_context(|| format!("failed to read {}", log_dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", log_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("log") {
            log_files.push(path);
        }
    }
    log_files.sort();
    if log_files.is_empty() {
        bail!("No log files found in {}", log_dir.display());
    }
    let status = Command::new("tail")
        .arg("-n")
        .arg("50")
        .arg("-f")
        .args(&log_files)
        .status()
        .context("failed to tail runtime logs")?;
    ensure_success("tail -f", status)
}

fn stop_pid_file_process(pidfile: &Path) -> anyhow::Result<()> {
    if !pidfile.exists() {
        return Ok(());
    }
    let pid = fs::read_to_string(pidfile).unwrap_or_default();
    let pid = pid.trim();
    if !pid.is_empty()
        && pid.chars().all(|ch| ch.is_ascii_digit())
        && pid_matches_pidfile(pidfile, pid)
    {
        println!(
            "[rustfin-installer] Stopping pid {} from {}",
            pid,
            pidfile.display()
        );
        let _ = kill_pid(pid, false);
        for _ in 0..20 {
            if !pid_is_running(pid) {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if pid_is_running(pid) {
            let _ = kill_pid(pid, true);
        }
    }
    let _ = fs::remove_file(pidfile);
    Ok(())
}

fn kill_pid(pid: &str, force: bool) -> anyhow::Result<()> {
    let mut cmd = Command::new("kill");
    if force {
        cmd.arg("-9");
    }
    let status = cmd.arg(pid).status().context("failed to execute kill")?;
    if !status.success() {
        return Ok(());
    }
    Ok(())
}

fn pid_is_running(pid: &str) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn pid_matches_pidfile(pidfile: &Path, pid: &str) -> bool {
    if !pid_is_running(pid) {
        return false;
    }

    let file_name = match pidfile.file_name().and_then(|name| name.to_str()) {
        Some(file_name) => file_name,
        None => return false,
    };

    let cmdline = match process_cmdline(pid) {
        Some(cmdline) => cmdline,
        None => return false,
    };

    cmdline_matches_pidfile_name(file_name, &cmdline)
}

fn process_cmdline(pid: &str) -> Option<String> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&raw)
            .replace('\0', " ")
            .trim()
            .to_string(),
    )
}

fn cmdline_matches_pidfile_name(file_name: &str, cmdline: &str) -> bool {
    match file_name {
        "rustfin.pid" => cmdline_has_executable(cmdline, "rustfin-server"),
        "rustfin-calendar.pid" => cmdline_has_executable(cmdline, "rustfin-calendar"),
        "rustfin-tmdb-agent.pid" => cmdline_has_executable(cmdline, "rustfin-tmdb-agent"),
        "rustfin-youtube-agent.pid" => cmdline_has_executable(cmdline, "rustfin-youtube-agent"),
        "rustfin-transcription-agent.pid" => {
            cmdline_has_executable(cmdline, "rustfin-transcription-agent")
        }
        "rustfin-servers-agent.pid" => cmdline_has_executable(cmdline, "rustfin-servers-agent"),
        "rustfin-ui.pid" => {
            (cmdline_has_executable(cmdline, "node") && cmdline_has_argument(cmdline, "server.js"))
                || cmdline.contains("next-server")
        }
        "rustfin-edge.pid" => {
            cmdline_has_executable(cmdline, "caddy") && cmdline.contains("Caddyfile.native")
        }
        "directory-picker-helper.pid" => {
            cmdline_has_argument(cmdline, "directory-picker-helper.py")
        }
        _ => false,
    }
}

fn cmdline_has_executable(cmdline: &str, expected: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|arg| cmdline_arg_matches(arg, expected))
}

fn cmdline_has_argument(cmdline: &str, expected: &str) -> bool {
    cmdline
        .split_whitespace()
        .any(|arg| cmdline_arg_matches(arg, expected))
}

fn cmdline_arg_matches(arg: &str, expected: &str) -> bool {
    let trimmed = arg.trim_matches(|ch| ch == '"' || ch == '\'');
    if trimmed == expected {
        return true;
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        == Some(expected)
}

fn resolve_clean_database_url(repo_root: &Path) -> anyhow::Result<String> {
    if let Ok(url) = env::var("RUSTFIN_DATABASE_URL")
        && !url.trim().is_empty()
    {
        return Ok(url);
    }

    let runtime_snapshot = load_shell_env_map(&repo_root.join(".rustyfin.runtime.env"))?;
    if let Some(url) = runtime_snapshot.get("RUSTFIN_DATABASE_URL")
        && !url.trim().is_empty()
    {
        return Ok(url.clone());
    }

    let pg_user = env::var("RUSTFIN_PG_USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    let pg_password = env::var("RUSTFIN_PG_PASSWORD")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    let pg_db = env::var("RUSTFIN_PG_DB")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    Ok(format!(
        "postgresql://{}:{}@127.0.0.1:5432/{}",
        pg_user, pg_password, pg_db
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        BootstrapAiModelConfig, DEFAULT_BOOTSTRAP_GEMMA_4_E2B_MODEL_URL,
        DEFAULT_BOOTSTRAP_GEMMA_4_E4B_MODEL_URL, EdgeTlsMode, build_edge_health_resolve,
        build_edge_site_address, build_public_browser_origin, build_ws_allowed_origins,
        cmdline_matches_pidfile_name, derive_gguf_file_name_from_url, format_http_origin,
        merge_search_paths_into_rustflags, normalize_browser_backend_origin,
        normalize_ws_allowed_origins, parse_ai_bootstrap_model_enabled, parse_http_origin,
        resolve_bootstrap_ai_model_config_with_overrides,
        resolve_bootstrap_ai_model_configs_with_overrides, resolve_installer_ai_model_dir_from_env,
    };
    use std::path::PathBuf;

    #[test]
    fn pidfile_match_requires_expected_service_binary() {
        assert!(cmdline_matches_pidfile_name(
            "rustfin-tmdb-agent.pid",
            "/home/tempo/Rustyfin/.native-bins/aarch64-unknown-linux-gnu/dev/rustfin-tmdb-agent"
        ));
        assert!(!cmdline_matches_pidfile_name(
            "rustfin-tmdb-agent.pid",
            "cargo run -p rustfin-installer -- plan-native-runtime"
        ));
    }

    #[test]
    fn pidfile_match_distinguishes_backend_from_servers_agent() {
        assert!(cmdline_matches_pidfile_name(
            "rustfin.pid",
            "/home/tempo/Rustyfin/.native-bins/x86_64-unknown-linux-gnu/dev/rustfin-server"
        ));
        assert!(!cmdline_matches_pidfile_name(
            "rustfin.pid",
            "/home/tempo/Rustyfin/.native-bins/x86_64-unknown-linux-gnu/dev/rustfin-servers-agent"
        ));
    }

    #[test]
    fn pidfile_match_handles_ui_and_edge_wrappers() {
        assert!(cmdline_matches_pidfile_name(
            "rustfin-ui.pid",
            "env PORT=3001 HOSTNAME=127.0.0.1 node server.js"
        ));
        assert!(cmdline_matches_pidfile_name(
            "rustfin-ui.pid",
            "next-server (v15.5.12)"
        ));
        assert!(cmdline_matches_pidfile_name(
            "rustfin-edge.pid",
            "caddy run --config /home/tempo/Rustyfin/scripts/caddy/Caddyfile.native --adapter caddyfile"
        ));
        assert!(!cmdline_matches_pidfile_name(
            "rustfin-ui.pid",
            "node unrelated.js"
        ));
    }

    #[test]
    fn ai_bootstrap_model_enabled_defaults_to_true() {
        assert!(parse_ai_bootstrap_model_enabled(None));
        assert!(parse_ai_bootstrap_model_enabled(Some("")));
        assert!(parse_ai_bootstrap_model_enabled(Some("1")));
    }

    #[test]
    fn ai_bootstrap_model_enabled_honors_disabled_values() {
        for value in ["0", "false", "False", "off", "disabled", "no"] {
            assert!(
                !parse_ai_bootstrap_model_enabled(Some(value)),
                "expected {value} to disable bootstrap"
            );
        }
    }

    #[test]
    fn merge_search_paths_into_rustflags_appends_missing_paths_once() {
        let merged = merge_search_paths_into_rustflags(
            Some("-Ctarget-cpu=native -L/usr/lib/x86_64-linux-gnu"),
            &["/usr/lib/x86_64-linux-gnu", "/usr/lib/cuda/lib64"],
        );
        assert_eq!(
            merged,
            "-Ctarget-cpu=native -L/usr/lib/x86_64-linux-gnu -L/usr/lib/cuda/lib64"
        );
    }

    #[test]
    fn bootstrap_ai_model_config_defaults_to_small_qwen() {
        let config = resolve_bootstrap_ai_model_config_with_overrides(None, None)
            .expect("default bootstrap config");
        assert_eq!(
            config,
            BootstrapAiModelConfig {
                url: "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf".to_string(),
                file_name: "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf".to_string(),
                strict: false,
            }
        );
    }

    #[test]
    fn bootstrap_ai_model_configs_default_to_qwen_plus_two_gemma_starters() {
        let configs = resolve_bootstrap_ai_model_configs_with_overrides(None, None)
            .expect("default bootstrap configs");
        assert_eq!(configs.len(), 3);
        assert_eq!(
            configs[0],
            BootstrapAiModelConfig {
                url: "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf".to_string(),
                file_name: "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf".to_string(),
                strict: false,
            }
        );
        assert_eq!(
            configs[1],
            BootstrapAiModelConfig {
                url: DEFAULT_BOOTSTRAP_GEMMA_4_E2B_MODEL_URL.to_string(),
                file_name: "gemma-4-e2b-it-Q4_0.gguf".to_string(),
                strict: false,
            }
        );
        assert_eq!(
            configs[2],
            BootstrapAiModelConfig {
                url: DEFAULT_BOOTSTRAP_GEMMA_4_E4B_MODEL_URL.to_string(),
                file_name: "gemma-4-E4B-it-Q3_K_M.gguf".to_string(),
                strict: false,
            }
        );
    }

    #[test]
    fn bootstrap_ai_model_config_marks_overrides_strict() {
        let config = resolve_bootstrap_ai_model_config_with_overrides(
            Some("https://example.com/custom.gguf"),
            Some("custom.gguf"),
        )
        .expect("override config");
        assert_eq!(
            config,
            BootstrapAiModelConfig {
                url: "https://example.com/custom.gguf".to_string(),
                file_name: "custom.gguf".to_string(),
                strict: true,
            }
        );
    }

    #[test]
    fn bootstrap_ai_model_config_rejects_non_gguf_file_names() {
        let error = resolve_bootstrap_ai_model_config_with_overrides(None, Some("not-a-model.bin"))
            .expect_err("non-gguf override should fail");
        assert!(error.to_string().contains("must end with .gguf"));
    }

    #[test]
    fn derive_gguf_file_name_from_url_strips_query_and_fragment() {
        let file_name = derive_gguf_file_name_from_url(
            "https://example.com/models/qwen.gguf?download=1#fragment",
        )
        .expect("filename from url");
        assert_eq!(file_name, "qwen.gguf");
    }

    #[test]
    fn resolve_installer_ai_model_dir_uses_default_when_unset() {
        let path = resolve_installer_ai_model_dir_from_env(None);
        assert_eq!(path, PathBuf::from("/var/lib/rustyfin/ai/models"));
    }

    #[test]
    fn resolve_installer_ai_model_dir_prefers_env_value() {
        let path = resolve_installer_ai_model_dir_from_env(Some("/srv/rustyfin/models"));
        assert_eq!(path, PathBuf::from("/srv/rustyfin/models"));
    }

    #[test]
    fn default_browser_origin_uses_https_edge_port() {
        assert_eq!(
            build_public_browser_origin("vault.example.com", 3000),
            "https://vault.example.com:3000"
        );
        assert_eq!(
            build_public_browser_origin("vault.example.com", 443),
            "https://vault.example.com"
        );
    }

    #[test]
    fn ws_allowed_origins_include_exact_browser_origin_and_localhost() {
        let origins = build_ws_allowed_origins("https://vault.example.com:3000", 3000);
        assert!(origins.contains("https://vault.example.com:3000"));
        assert!(origins.contains("https://localhost:3000"));
        assert!(origins.contains("http://127.0.0.1:3000"));
        assert!(!origins.contains("http://vault.example.com:3000"));
    }

    #[test]
    fn normalize_browser_origin_rewrites_legacy_backend_snapshot() {
        assert_eq!(
            normalize_browser_backend_origin(
                Some("http://192.168.0.36:8097"),
                "192.168.0.36",
                8097,
                3008
            ),
            "https://192.168.0.36:3008"
        );
    }

    #[test]
    fn normalize_browser_origin_preserves_explicit_edge_origin() {
        assert_eq!(
            normalize_browser_backend_origin(
                Some("https://vault.example.com:3443"),
                "vault.example.com",
                8097,
                3008
            ),
            "https://vault.example.com:3443"
        );
    }

    #[test]
    fn normalize_ws_allowed_origins_rewrites_legacy_snapshot() {
        let legacy = "http://localhost:3008,http://127.0.0.1:3008,https://localhost:3008,https://127.0.0.1:3008,http://192.168.0.36:3008,https://192.168.0.36:3008";
        assert_eq!(
            normalize_ws_allowed_origins(
                Some(legacy),
                "https://192.168.0.36:3008",
                "192.168.0.36",
                3008
            ),
            "http://localhost:3008,http://127.0.0.1:3008,https://localhost:3008,https://127.0.0.1:3008,https://192.168.0.36:3008"
        );
    }

    #[test]
    fn edge_site_address_uses_hostname_in_auto_mode() {
        assert_eq!(
            build_edge_site_address("vault.example.com", 3000, EdgeTlsMode::Auto)
                .expect("auto https site address"),
            "vault.example.com:3000"
        );
        assert_eq!(
            build_edge_site_address("vault.example.com", 443, EdgeTlsMode::Auto)
                .expect("default https site address"),
            "vault.example.com"
        );
    }

    #[test]
    fn edge_site_address_rejects_ip_auto_mode() {
        let error = build_edge_site_address("192.168.0.36", 3000, EdgeTlsMode::Auto)
            .expect_err("ip auto mode should fail");
        assert!(
            error
                .to_string()
                .contains("requires RUSTFIN_PUBLIC_HOST to be a real hostname")
        );
    }

    #[test]
    fn edge_health_resolve_targets_loopback_for_named_hosts() {
        assert_eq!(
            build_edge_health_resolve("https://vault.example.com:3000"),
            Some("vault.example.com:3000:127.0.0.1".to_string())
        );
        assert_eq!(build_edge_health_resolve("https://127.0.0.1:3000"), None);
    }

    #[test]
    fn parse_http_origin_normalizes_missing_default_port() {
        let parsed = parse_http_origin("https://vault.example.com/path").expect("origin parse");
        assert_eq!(parsed.host, "vault.example.com");
        assert_eq!(parsed.port, 443);
        assert_eq!(
            format_http_origin(&parsed.scheme, &parsed.host, parsed.port),
            "https://vault.example.com"
        );
    }
}

fn load_shell_env_map(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    if !path.exists() {
        return Ok(values);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(body) = trimmed
            .strip_prefix(": \"${")
            .and_then(|value| value.strip_suffix("}\""))
        {
            if let Some((key, value)) = body.split_once(":=") {
                values.insert(key.to_string(), value.to_string());
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        values.insert(key.to_string(), unquote_shell_value(value));
    }
    Ok(values)
}

fn unquote_shell_value(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(inner) = trimmed
        .strip_prefix('\'')
        .and_then(|v| v.strip_suffix('\''))
    {
        return inner.replace("'\"'\"'", "'");
    }
    trimmed.to_string()
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn install_systemd_units(
    config: &SystemdInstallConfig,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    ensure_command_available("systemctl")?;
    if user_context.uses_sudo_for_privileged_steps {
        ensure_command_available("sudo")?;
    }

    fs::create_dir_all(&config.log_dir)
        .with_context(|| format!("failed to create {}", config.log_dir.display()))?;

    let existing_token = read_existing_servers_agent_token(&config.env_file_path, user_context)?
        .unwrap_or_else(generate_secret_hex);
    let env_file = render_servers_agent_env(&existing_token, user_context);
    let agent_unit = render_servers_agent_unit(config);
    let main_unit = render_main_runtime_unit(config, user_context);
    let post_healthcheck_unit = render_post_healthcheck_unit(config);

    write_root_owned_file(&config.env_file_path, &env_file, 0o600, user_context)?;
    write_root_owned_file(&config.agent_service_path, &agent_unit, 0o644, user_context)?;
    write_root_owned_file(&config.main_service_path, &main_unit, 0o644, user_context)?;
    write_root_owned_file(
        &config.post_healthcheck_service_path,
        &post_healthcheck_unit,
        0o644,
        user_context,
    )?;

    run_root_command("systemctl", &["daemon-reload"], user_context)?;
    run_root_command(
        "systemctl",
        &["enable", config.agent_service_name.as_str()],
        user_context,
    )?;
    run_root_command(
        "systemctl",
        &["enable", config.main_service_name.as_str()],
        user_context,
    )?;
    run_root_command(
        "systemctl",
        &["enable", config.post_healthcheck_service_name.as_str()],
        user_context,
    )?;
    let _ = run_root_command_allow_failure(
        "systemctl",
        &["stop", config.main_service_name.as_str()],
        user_context,
    );
    let _ = run_root_command_allow_failure(
        "systemctl",
        &["stop", config.post_healthcheck_service_name.as_str()],
        user_context,
    );
    run_root_command(
        "systemctl",
        &["restart", config.agent_service_name.as_str()],
        user_context,
    )?;
    run_root_command(
        "systemctl",
        &["start", config.main_service_name.as_str()],
        user_context,
    )?;
    run_root_command(
        "systemctl",
        &["start", config.post_healthcheck_service_name.as_str()],
        user_context,
    )?;

    Ok(())
}

fn validate_systemd_runtime_start(
    config: &SystemdInstallConfig,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    ensure_command_available("systemctl")?;
    ensure_command_available("curl")?;

    println!("[rustfin-installer] Validating native systemd runtime startup...");
    wait_for_root_command_success(
        "systemctl",
        &["is-active", "--quiet", config.agent_service_name.as_str()],
        30,
        user_context,
    )?;
    wait_for_root_command_success(
        "systemctl",
        &["is-active", "--quiet", config.main_service_name.as_str()],
        60,
        user_context,
    )?;

    let enable_servers_agent = env_bool_default("RUSTFIN_ENABLE_SERVERS_AGENT", true);
    let backend_port = env::var("RUSTFIN_BACKEND_PORT").unwrap_or_else(|_| "8096".to_string());
    let calendar_port = env::var("RUSTFIN_CALENDAR_PORT").unwrap_or_else(|_| "8099".to_string());
    let tmdb_port = env::var("RUSTFIN_TMDB_AGENT_PORT").unwrap_or_else(|_| "8100".to_string());
    let youtube_port =
        env::var("RUSTFIN_YOUTUBE_AGENT_PORT").unwrap_or_else(|_| "8101".to_string());
    let transcription_port =
        env::var("RUSTFIN_TRANSCRIPTION_AGENT_PORT").unwrap_or_else(|_| "8102".to_string());
    let servers_agent_port =
        env::var("RUSTFIN_SERVERS_AGENT_PORT").unwrap_or_else(|_| "8103".to_string());
    let browser_backend_origin = env::var("RUSTYFIN_BROWSER_BACKEND_ORIGIN")
        .unwrap_or_else(|_| "https://127.0.0.1:3000".to_string());
    let edge_health_resolve = env::var("RUSTFIN_EDGE_HEALTH_RESOLVE").unwrap_or_default();

    let backend_ready = wait_for_http(
        "rustfin",
        &format!("http://127.0.0.1:{backend_port}/health"),
        60,
        false,
    )?;
    let calendar_ready = wait_for_http(
        "calendar",
        &format!("http://127.0.0.1:{calendar_port}/health"),
        60,
        false,
    )?;
    let tmdb_ready = wait_for_http(
        "tmdb-agent",
        &format!("http://127.0.0.1:{tmdb_port}/health"),
        60,
        false,
    )?;
    let youtube_ready = wait_for_http(
        "youtube-agent",
        &format!("http://127.0.0.1:{youtube_port}/health"),
        60,
        false,
    )?;
    let transcription_ready = wait_for_http(
        "transcription-agent",
        &format!("http://127.0.0.1:{transcription_port}/health"),
        60,
        false,
    )?;
    let servers_agent_ready = if enable_servers_agent {
        wait_for_http(
            "servers-agent",
            &format!("http://127.0.0.1:{servers_agent_port}/health"),
            60,
            false,
        )?
    } else {
        true
    };
    let ui_ready = wait_for_edge_origin_path(
        "ui-login",
        &browser_backend_origin,
        "/login",
        &edge_health_resolve,
        60,
    )?;

    if backend_ready
        && calendar_ready
        && tmdb_ready
        && youtube_ready
        && transcription_ready
        && servers_agent_ready
        && ui_ready
    {
        println!("[rustfin-installer] Native systemd runtime validation passed.");
        return Ok(());
    }

    bail!(
        "Native systemd runtime validation failed.\n{}",
        collect_systemd_start_diagnostics(config, user_context)
    )
}

fn wait_for_root_command_success(
    program: &str,
    args: &[&str],
    attempts: usize,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    for _ in 0..attempts {
        let status = run_root_command_allow_failure(program, args, user_context)?;
        if status.success() {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
    bail!(
        "{program} {} did not report success in time",
        args.join(" ")
    );
}

fn collect_systemd_start_diagnostics(
    config: &SystemdInstallConfig,
    user_context: &NativeUserContext,
) -> String {
    let mut output = String::new();

    append_root_command_output(
        &mut output,
        "main-service-status",
        "systemctl",
        &[
            "status",
            config.main_service_name.as_str(),
            "--no-pager",
            "-l",
        ],
        user_context,
    );
    append_root_command_output(
        &mut output,
        "servers-agent-status",
        "systemctl",
        &[
            "status",
            config.agent_service_name.as_str(),
            "--no-pager",
            "-l",
        ],
        user_context,
    );
    append_root_command_output(
        &mut output,
        "post-healthcheck-status",
        "systemctl",
        &[
            "status",
            config.post_healthcheck_service_name.as_str(),
            "--no-pager",
            "-l",
        ],
        user_context,
    );
    append_log_tail(
        &mut output,
        "rustyfin-native-systemd.log",
        &config.log_dir.join("rustyfin-native-systemd.log"),
        80,
    );
    append_log_tail(
        &mut output,
        "rustfin.log",
        &config.log_dir.join("rustfin.log"),
        60,
    );
    append_log_tail(
        &mut output,
        "rustfin-ui.log",
        &config.log_dir.join("rustfin-ui.log"),
        60,
    );

    output
}

fn append_root_command_output(
    output: &mut String,
    label: &str,
    program: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) {
    output.push_str(&format!("== {label} ==\n"));
    match run_root_command_capture(program, args, user_context) {
        Ok(contents) => output.push_str(contents.trim()),
        Err(error) => output.push_str(&format!("failed to capture {label}: {error}")),
    }
    output.push_str("\n\n");
}

fn append_log_tail(output: &mut String, label: &str, path: &Path, max_lines: usize) {
    output.push_str(&format!("== {label} ==\n"));
    match tail_file(path, max_lines) {
        Some(contents) => output.push_str(contents.trim()),
        None => output.push_str("(log unavailable)"),
    }
    output.push_str("\n\n");
}

fn tail_file(path: &Path, max_lines: usize) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Some(lines[start..].join("\n"))
}

fn render_servers_agent_env(token: &str, user_context: &NativeUserContext) -> String {
    format!(
        "RUSTFIN_SERVERS_AGENT_BIND={SERVERS_AGENT_BIND}\n\
RUSTFIN_SERVERS_AGENT_URL={SERVERS_AGENT_URL}\n\
RUSTFIN_SERVERS_AGENT_TOKEN={token}\n\
RUSTFIN_SERVERS_SYSTEM_USER={user}\n\
RUSTFIN_SERVERS_SYSTEM_GROUP={user}\n\
RUSTFIN_SERVERS_DEFAULT_JAVA={SERVERS_DEFAULT_JAVA}\n",
        user = user_context.name
    )
}

fn render_servers_agent_unit(config: &SystemdInstallConfig) -> String {
    let repo_root = config.repo_root.display();
    let log_path = config.log_dir.join("rustfin-servers-agent-systemd.log");
    format!(
        "[Unit]\n\
Description=Rustyfin Privileged Servers Agent\n\
Wants=network-online.target\n\
After=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=root\n\
Group=root\n\
WorkingDirectory={repo_root}\n\
Environment=HOME=/root\n\
Environment=PATH=/usr/local/bin:/usr/bin:/bin\n\
EnvironmentFile=-{env_file}\n\
ExecStart=/usr/bin/bash -lc '{repo_root}/scripts/start-native-servers-agent.sh'\n\
Restart=on-failure\n\
RestartSec=2\n\
StandardOutput=append:{log_path}\n\
StandardError=append:{log_path}\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        env_file = config.env_file_path.display(),
        log_path = log_path.display(),
    )
}

fn render_main_runtime_unit(
    config: &SystemdInstallConfig,
    user_context: &NativeUserContext,
) -> String {
    let repo_root = config.repo_root.display();
    let log_path = config.log_dir.join("rustyfin-native-systemd.log");
    let home = &user_context.home;
    format!(
        "[Unit]\n\
Description=Rustyfin Native Runtime\n\
Wants=network-online.target postgresql.service {agent_service}\n\
After=network-online.target postgresql.service {agent_service}\n\
\n\
[Service]\n\
Type=simple\n\
User={user}\n\
Group={user}\n\
WorkingDirectory={repo_root}\n\
Environment=HOME={home}\n\
Environment=PATH={home}/.cargo/bin:/usr/local/bin:/usr/bin:/bin\n\
Environment=RUSTFIN_ENABLE_SERVERS_AGENT=0\n\
EnvironmentFile=-{env_file}\n\
ExecStart=/usr/bin/env RUSTFIN_ENABLE_SERVERS_AGENT=0 /usr/bin/bash -lc 'source {home}/.cargo/env && exec {repo_root}/scripts/run-native-supervisor.sh'\n\
TimeoutStartSec=0\n\
TimeoutStopSec=120\n\
Restart=on-failure\n\
RestartSec=2\n\
KillMode=control-group\n\
StandardOutput=append:{log_path}\n\
StandardError=append:{log_path}\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        agent_service = config.agent_service_name,
        user = user_context.name,
        env_file = config.env_file_path.display(),
        log_path = log_path.display(),
    )
}

fn render_post_healthcheck_unit(config: &SystemdInstallConfig) -> String {
    let repo_root = config.repo_root.display();
    let log_path = config.log_dir.join("rustyfin-post-healthcheck.log");
    format!(
        "[Unit]\n\
Description=Rustyfin Native Post-Start Healthcheck\n\
Wants=network-online.target remote-fs.target {main_service} {agent_service}\n\
After=network-online.target remote-fs.target {main_service} {agent_service}\n\
\n\
[Service]\n\
Type=oneshot\n\
User=root\n\
Group=root\n\
WorkingDirectory={repo_root}\n\
Environment=HOME=/root\n\
Environment=PATH=/usr/local/bin:/usr/bin:/bin\n\
EnvironmentFile=-{env_file}\n\
ExecStart=/usr/bin/bash -lc '{repo_root}/scripts/run-native-post-healthcheck.sh'\n\
StandardOutput=append:{log_path}\n\
StandardError=append:{log_path}\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        main_service = config.main_service_name,
        agent_service = config.agent_service_name,
        env_file = config.env_file_path.display(),
        log_path = log_path.display(),
    )
}

fn write_install_manifest(
    repo_root: &Path,
    host: &HostPlatform,
    user_context: &NativeUserContext,
    config: &SystemdInstallConfig,
    options: &InstallOptions,
) -> anyhow::Result<()> {
    let manifest = InstallManifest {
        installed_at_utc: Utc::now().to_rfc3339(),
        repo_root: repo_root.display().to_string(),
        supported_flow: "debian12-native".to_string(),
        host: host.clone(),
        native_user: user_context.clone(),
        install_mode: InstallModeManifest {
            skip_prereqs: options.skip_prereqs,
            skip_systemd: options.skip_systemd,
        },
        services: ServiceManifest {
            main_service_name: config.main_service_name.clone(),
            agent_service_name: config.agent_service_name.clone(),
            post_healthcheck_service_name: config.post_healthcheck_service_name.clone(),
            systemd_installed: !options.skip_systemd,
        },
        paths: InstallPathManifest {
            env_dir: ENV_DIR.to_string(),
            env_file: config.env_file_path.display().to_string(),
            runtime_defaults_file: RUNTIME_DEFAULTS_FILE.to_string(),
            log_dir: config.log_dir.display().to_string(),
            manifest_path: INSTALL_MANIFEST_PATH.to_string(),
        },
    };

    let rendered =
        serde_json::to_string_pretty(&manifest).context("failed to serialize install manifest")?;
    write_root_owned_file(
        Path::new(INSTALL_MANIFEST_PATH),
        &format!("{rendered}\n"),
        0o644,
        user_context,
    )
}

fn read_existing_servers_agent_token(
    env_file_path: &Path,
    user_context: &NativeUserContext,
) -> anyhow::Result<Option<String>> {
    let Some(contents) = read_maybe_root_owned_file(env_file_path, user_context)? else {
        return Ok(None);
    };

    Ok(contents
        .lines()
        .find_map(|line| line.strip_prefix("RUSTFIN_SERVERS_AGENT_TOKEN="))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_matches('"').to_string()))
}

fn read_maybe_root_owned_file(
    path: &Path,
    user_context: &NativeUserContext,
) -> anyhow::Result<Option<String>> {
    if user_context.uses_sudo_for_privileged_steps {
        if !path_exists_via_root(path, user_context)? {
            return Ok(None);
        }
        let output = Command::new("sudo")
            .arg("cat")
            .arg(path)
            .output()
            .with_context(|| format!("failed to read {}", path.display()))?;
        if !output.status.success() {
            bail!(
                "failed to read {} via sudo cat (status {:?})",
                path.display(),
                output.status.code()
            );
        }
        return Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()));
    }

    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(Some(raw))
}

fn path_exists_via_root(path: &Path, user_context: &NativeUserContext) -> anyhow::Result<bool> {
    if user_context.uses_sudo_for_privileged_steps {
        let status = Command::new("sudo")
            .arg("test")
            .arg("-f")
            .arg(path)
            .status()
            .with_context(|| format!("failed to test {}", path.display()))?;
        return Ok(status.success());
    }

    Ok(path.exists())
}

fn write_root_owned_file(
    destination: &Path,
    content: &str,
    mode: u32,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    if user_context.uses_sudo_for_privileged_steps {
        ensure_command_available("sudo")?;
        let temp_path = temp_file_path(destination);
        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create temp dir {}", parent.display()))?;
        }
        fs::write(&temp_path, content)
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        let install_mode = format!("{mode:o}");
        let status = Command::new("sudo")
            .arg("install")
            .arg("-D")
            .arg("-m")
            .arg(install_mode)
            .arg(&temp_path)
            .arg(destination)
            .status()
            .with_context(|| format!("failed to install {}", destination.display()))?;
        let _ = fs::remove_file(&temp_path);
        ensure_success("sudo install", status)?;
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(destination, content)
        .with_context(|| format!("failed to write {}", destination.display()))?;
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(destination, permissions)
        .with_context(|| format!("failed to chmod {}", destination.display()))?;
    Ok(())
}

fn temp_file_path(destination: &Path) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rustfin-installer.tmp");
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    env::temp_dir().join(format!("{name}.{nonce}.tmp"))
}

fn run_postgres_command(
    program: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    let status = run_postgres_command_allow_failure(program, args, user_context)
        .with_context(|| format!("failed to execute postgres {program} {}", args.join(" ")))?;
    ensure_success(program, status)
}

fn run_postgres_command_capture(
    program: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) -> anyhow::Result<String> {
    let output = if user_context.uses_sudo_for_privileged_steps {
        ensure_command_available("sudo")?;
        Command::new("sudo")
            .arg("-u")
            .arg("postgres")
            .arg(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to execute sudo -u postgres {program}"))?
    } else {
        Command::new(resolve_command_path("runuser"))
            .arg("-u")
            .arg("postgres")
            .arg("--")
            .arg(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to execute runuser -u postgres -- {program}"))?
    };

    if !output.status.success() {
        bail!(
            "postgres {program} exited with status {:?}",
            output.status.code()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_postgres_command_allow_failure(
    program: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) -> anyhow::Result<ExitStatus> {
    if user_context.uses_sudo_for_privileged_steps {
        ensure_command_available("sudo")?;
        return Command::new("sudo")
            .arg("-u")
            .arg("postgres")
            .arg(program)
            .args(args)
            .status()
            .with_context(|| format!("failed to execute sudo -u postgres {program}"));
    }

    Command::new(resolve_command_path("runuser"))
        .arg("-u")
        .arg("postgres")
        .arg("--")
        .arg(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to execute runuser -u postgres -- {program}"))
}

#[derive(Debug, Clone)]
struct RustToolchainCommand {
    toolchain: Option<String>,
}

impl RustToolchainCommand {
    fn command(&self) -> Command {
        match &self.toolchain {
            Some(toolchain) => {
                let mut command = Command::new("rustup");
                command.arg("run").arg(toolchain).arg("cargo");
                command
            }
            None => Command::new("cargo"),
        }
    }

    fn rustc_command(&self) -> Command {
        match &self.toolchain {
            Some(toolchain) => {
                let mut command = Command::new("rustup");
                command.arg("run").arg(toolchain).arg("rustc");
                command
            }
            None => Command::new("rustc"),
        }
    }
}

fn resolve_rust_toolchain(toolchain: &str) -> anyhow::Result<RustToolchainCommand> {
    if command_exists("rustup") {
        let status = Command::new("rustup")
            .arg("run")
            .arg(toolchain)
            .arg("rustc")
            .arg("-vV")
            .status()
            .with_context(|| format!("failed to probe rustup toolchain {toolchain}"))?;
        if status.success() {
            return Ok(RustToolchainCommand {
                toolchain: Some(toolchain.to_string()),
            });
        }
    }

    Ok(RustToolchainCommand { toolchain: None })
}

fn rust_host_triple(toolchain: &RustToolchainCommand) -> anyhow::Result<String> {
    let output = toolchain
        .rustc_command()
        .arg("-vV")
        .output()
        .context("failed to resolve rustc host triple")?;
    if !output.status.success() {
        bail!("rustc -vV exited with status {:?}", output.status.code());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .context("failed to parse rust host triple")
}

fn ensure_rustup_target(toolchain: &str, target_triple: &str) -> anyhow::Result<()> {
    if !command_exists("rustup") {
        return Ok(());
    }

    let output = Command::new("rustup")
        .args(["target", "list", "--toolchain", toolchain, "--installed"])
        .output()
        .with_context(|| format!("failed to list installed rustup targets for {toolchain}"))?;
    if !output.status.success() {
        bail!(
            "rustup target list exited with status {:?}",
            output.status.code()
        );
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target_triple) {
        return Ok(());
    }

    let status = Command::new("rustup")
        .args(["target", "add", "--toolchain", toolchain, target_triple])
        .status()
        .with_context(|| format!("failed to add rustup target {target_triple}"))?;
    ensure_success("rustup target add", status)
}

fn absolutize_from_repo(repo_root: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(repo_root.join(path))
}

fn env_u16_default(key: &str, default: u16) -> anyhow::Result<u16> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value
            .parse::<u16>()
            .with_context(|| format!("Invalid {key} value: {value}")),
        _ => Ok(default),
    }
}

fn env_bool_default(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"),
        Err(_) => default,
    }
}

fn service_exists(service_name: &str) -> bool {
    if !command_exists("systemctl") {
        return false;
    }

    Command::new("systemctl")
        .arg("cat")
        .arg(service_name)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn stop_service_if_present(
    service_name: &str,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    if !service_exists(service_name) {
        return Ok(());
    }
    println!("[rustfin-installer] Stopping {}...", service_name);
    let _ = run_root_command_allow_failure("systemctl", &["stop", service_name], user_context)?;
    Ok(())
}

fn repair_build_artifact_ownership(
    repo_root: &Path,
    repo_owner_user: &str,
    repo_owner_group: &str,
    user_context: &NativeUserContext,
) -> anyhow::Result<()> {
    let owner_spec = format!("{repo_owner_user}:{repo_owner_group}");
    for path in [
        repo_root.join("ui/.next"),
        repo_root.join("ui/node_modules"),
        repo_root.join(".native-bins"),
        repo_root.join("target"),
        repo_root.join(".tmp"),
    ] {
        if !path.exists() {
            continue;
        }
        let path_string = path.display().to_string();
        run_root_command(
            "chown",
            &["-R", owner_spec.as_str(), path_string.as_str()],
            user_context,
        )?;
    }
    Ok(())
}

fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

fn pick_free_port(preferred: u16, max_hops: u16) -> anyhow::Result<u16> {
    let mut port = preferred;
    let mut hops = 0_u16;
    while port_in_use(port) {
        port = port
            .checked_add(1)
            .context("port search overflow while finding free port")?;
        hops = hops.saturating_add(1);
        if hops > max_hops {
            bail!("Unable to find a free port near {preferred}");
        }
    }
    Ok(port)
}

fn external_servers_agent_configured() -> bool {
    env::var("RUSTFIN_SERVERS_AGENT_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
        && env::var("RUSTFIN_SERVERS_AGENT_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
}

fn build_ws_allowed_origins(browser_origin: &str, ui_port: u16) -> String {
    let mut origins = vec![
        format!("http://localhost:{ui_port}"),
        format!("http://127.0.0.1:{ui_port}"),
        format!("https://localhost:{ui_port}"),
        format!("https://127.0.0.1:{ui_port}"),
    ];
    if let Some(parsed) = parse_http_origin(browser_origin) {
        let exact_origin = format_http_origin(&parsed.scheme, &parsed.host, parsed.port);
        if !origins.iter().any(|existing| existing == &exact_origin) {
            origins.push(exact_origin);
        }
    }
    origins.join(",")
}

fn resolve_media_path(repo_root: &Path) -> anyhow::Result<PathBuf> {
    let configured = env::var("RUSTFIN_MEDIA_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("HOME")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| repo_root.join("media"));
    let media_path = if configured.is_absolute() {
        configured
    } else {
        repo_root.join(configured)
    };
    fs::create_dir_all(&media_path)
        .with_context(|| format!("Failed to create media path: {}", media_path.display()))?;
    let metadata = fs::metadata(&media_path)
        .with_context(|| format!("Failed to stat media path: {}", media_path.display()))?;
    if !metadata.is_dir() {
        bail!(
            "Resolved media path is not a directory: {}",
            media_path.display()
        );
    }
    Ok(media_path)
}

fn resolve_database_url() -> anyhow::Result<String> {
    if let Ok(value) = env::var("RUSTFIN_DATABASE_URL")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }

    let pg_user = env::var("RUSTFIN_PG_USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    let pg_password = env::var("RUSTFIN_PG_PASSWORD")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    let pg_db = env::var("RUSTFIN_PG_DB")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "rustfin".to_string());
    Ok(format!(
        "postgresql://{pg_user}:{pg_password}@127.0.0.1:5432/{pg_db}"
    ))
}

fn validate_postgres_url(database_url: &str) -> anyhow::Result<()> {
    let db_target_lc = database_url.to_ascii_lowercase();
    if db_target_lc.starts_with("postgres://") || db_target_lc.starts_with("postgresql://") {
        return Ok(());
    }
    bail!("RUSTFIN_DATABASE_URL must be a PostgreSQL URL (postgres:// or postgresql://).")
}

fn redact_postgres_url(database_url: &str) -> String {
    let Some((scheme, rest)) = database_url.split_once("://") else {
        return database_url.to_string();
    };
    let Some(at_index) = rest.find('@') else {
        return database_url.to_string();
    };
    format!("{scheme}://<redacted>@{}", &rest[at_index + 1..])
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn generate_secret_hex() -> String {
    let mut bytes = [0_u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn validate_sql_identifier(var_name: &str, value: &str) -> anyhow::Result<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{var_name} must not be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        bail!("{var_name} must be alphanumeric/underscore and start with a letter or underscore");
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        bail!("{var_name} must be alphanumeric/underscore only");
    }
    Ok(())
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}
