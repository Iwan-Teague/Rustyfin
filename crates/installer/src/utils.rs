use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

#[derive(Debug, Clone, Serialize)]
pub struct HostPlatform {
    pub id: Option<String>,
    pub version_id: Option<String>,
    pub id_like: Option<String>,
    pub architecture: String,
    pub package_manager: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NativeUserContext {
    pub name: String,
    pub home: String,
    pub repo_owner_user: String,
    pub repo_owner_group: String,
    pub uses_sudo_for_privileged_steps: bool,
}

pub fn detect_host_platform() -> Result<HostPlatform> {
    ensure_linux()?;

    let os_release_path = Path::new("/etc/os-release");
    let mut values = HashMap::new();
    if os_release_path.exists() {
        let raw = fs::read_to_string(os_release_path)
            .with_context(|| format!("failed to read {}", os_release_path.display()))?;
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            values.insert(key.to_string(), trim_os_release_value(value));
        }
    }

    Ok(HostPlatform {
        id: values.get("ID").cloned(),
        version_id: values.get("VERSION_ID").cloned(),
        id_like: values.get("ID_LIKE").cloned(),
        architecture: uname_value("-m")?,
        package_manager: detect_package_manager(),
    })
}

pub fn detect_package_manager() -> String {
    for (cmd, name) in [
        ("apt-get", "apt"),
        ("dnf", "dnf"),
        ("pacman", "pacman"),
        ("zypper", "zypper"),
    ] {
        if command_exists(cmd) {
            return name.to_string();
        }
    }
    "unknown".to_string()
}

pub fn ensure_linux() -> Result<()> {
    let name = uname_value("-s")?;
    if name != "Linux" {
        bail!("rustfin-installer currently supports Linux hosts only");
    }
    Ok(())
}

pub fn detect_native_user_context(repo_root: &Path) -> Result<NativeUserContext> {
    let current_uid = id_value("-u")?;
    let uses_sudo_for_privileged_steps = current_uid != "0";
    let user_name = env::var("RUSTFIN_NATIVE_USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            if current_uid == "0" {
                env::var("SUDO_USER")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| Some("root".to_string()))
            } else {
                env::var("USER")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| Some(id_value("-un").ok()?))
            }
        })
        .context("failed to resolve native install user")?;

    let home = resolve_user_home(&user_name)?;
    let repo_owner_user = stat_value(repo_root, "%U")?;
    let repo_owner_group = stat_value(repo_root, "%G")?;

    Ok(NativeUserContext {
        name: user_name,
        home: home.display().to_string(),
        repo_owner_user,
        repo_owner_group,
        uses_sudo_for_privileged_steps,
    })
}

pub fn ensure_command_available(name: &str) -> Result<()> {
    if command_exists(name) {
        return Ok(());
    }
    bail!("{name} is required for this installer step");
}

pub fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn resolve_command_path(name: &str) -> String {
    for prefix in [
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    ] {
        let candidate = Path::new(prefix).join(name);
        if candidate.exists() {
            return candidate.display().to_string();
        }
    }
    name.to_string()
}

pub fn run_root_command(cmd: &str, args: &[&str], user_context: &NativeUserContext) -> Result<()> {
    let mut command = if user_context.uses_sudo_for_privileged_steps {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(cmd);
        c
    } else {
        Command::new(cmd)
    };

    command.args(args);

    let status = command
        .status()
        .with_context(|| format!("failed to execute {cmd}"))?;

    if !status.success() {
        bail!("command failed: {cmd} {:?}", args);
    }
    Ok(())
}

pub fn run_command_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {cmd}"))?;

    if !output.status.success() {
        bail!(
            "command failed: {cmd} {:?}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_command_in_dir_as_user(
    program: &str,
    args: &[&str],
    current_dir: &Path,
    user_name: &str,
) -> Result<()> {
    let status = run_command_in_dir_as_user_allow_failure(program, args, current_dir, user_name)?;
    ensure_success(program, status)
}

pub fn run_command_in_dir_as_user_capture(
    program: &str,
    args: &[&str],
    current_dir: &Path,
    user_name: &str,
) -> Result<String> {
    let output = if should_run_as_user(user_name)? {
        let user_home = user_home_dir(user_name)?;
        Command::new(resolve_command_path("runuser"))
            .arg("-u")
            .arg(user_name)
            .arg("--")
            .arg(program)
            .args(args)
            .current_dir(current_dir)
            .env("HOME", &user_home)
            .env("USER", user_name)
            .env("LOGNAME", user_name)
            .env_remove("RUSTUP_HOME")
            .env_remove("CARGO_HOME")
            .output()
            .with_context(|| format!("failed to execute {program} as {user_name}"))?
    } else {
        Command::new(program)
            .args(args)
            .current_dir(current_dir)
            .output()
            .with_context(|| format!("failed to execute {program}"))?
    };

    if !output.status.success() {
        bail!("{program} exited with status {:?}", output.status.code());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_command_in_dir_as_user_allow_failure(
    program: &str,
    args: &[&str],
    current_dir: &Path,
    user_name: &str,
) -> Result<ExitStatus> {
    if should_run_as_user(user_name)? {
        let user_home = user_home_dir(user_name)?;
        return Command::new(resolve_command_path("runuser"))
            .arg("-u")
            .arg(user_name)
            .arg("--")
            .arg(program)
            .args(args)
            .current_dir(current_dir)
            .env("HOME", &user_home)
            .env("USER", user_name)
            .env("LOGNAME", user_name)
            .env_remove("RUSTUP_HOME")
            .env_remove("CARGO_HOME")
            .status()
            .with_context(|| format!("failed to execute {program} as {user_name}"));
    }

    Command::new(program)
        .args(args)
        .current_dir(current_dir)
        .status()
        .with_context(|| format!("failed to execute {program}"))
}

pub fn should_run_as_user(user_name: &str) -> Result<bool> {
    Ok(id_value("-u")? == "0" && user_name != "root")
}

pub fn user_home_dir(user_name: &str) -> Result<String> {
    let passwd = run_command_capture("getent", &["passwd", user_name])
        .with_context(|| format!("failed to resolve home directory for {user_name}"))?;
    let mut fields = passwd.split(':');
    let _name = fields.next();
    let _password = fields.next();
    let _uid = fields.next();
    let _gid = fields.next();
    let _gecos = fields.next();
    let home = fields
        .next()
        .filter(|value| !value.trim().is_empty())
        .context("passwd entry missing home directory")?;
    Ok(home.to_string())
}

pub fn ensure_success(step: &str, status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }

    match status.code() {
        Some(code) => bail!("{step} exited with status code {code}"),
        None => bail!("{step} terminated by signal"),
    }
}

pub fn uname_value(flag: &str) -> Result<String> {
    let output = Command::new("uname")
        .arg(flag)
        .output()
        .with_context(|| format!("failed to run uname {flag}"))?;
    if !output.status.success() {
        bail!("uname {flag} exited with status {:?}", output.status.code());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn id_value(flag: &str) -> Result<String> {
    let output = Command::new("id")
        .arg(flag)
        .output()
        .with_context(|| format!("failed to run id {flag}"))?;
    if !output.status.success() {
        bail!("id {flag} exited with status {:?}", output.status.code());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn resolve_user_home(user_name: &str) -> Result<PathBuf> {
    let output = Command::new("getent")
        .arg("passwd")
        .arg(user_name)
        .output()
        .with_context(|| format!("failed to resolve home for user {user_name}"))?;
    if !output.status.success() {
        bail!(
            "getent passwd {user_name} exited with status {:?}",
            output.status.code()
        );
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let home = line
        .trim()
        .split(':')
        .nth(5)
        .filter(|value| !value.is_empty())
        .context("failed to parse user home from getent output")?;
    Ok(PathBuf::from(home))
}

pub fn stat_value(path: &Path, format: &str) -> Result<String> {
    let output = Command::new("stat")
        .arg("-c")
        .arg(format)
        .arg(path)
        .output()
        .with_context(|| format!("failed to stat {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "stat -c {format} {} exited with status {:?}",
            path.display(),
            output.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn trim_os_release_value(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(trimmed);
    unquoted.to_string()
}

pub fn run_script(repo_root: &Path, relative_script: &str, args: &[&str]) -> Result<()> {
    let script_path = repo_root.join(relative_script);
    let status = Command::new(&script_path)
        .args(args)
        .current_dir(repo_root)
        .status()
        .with_context(|| format!("failed to execute {}", script_path.display()))?;
    ensure_success(relative_script, status)
}

pub fn run_script_as_repo_owner(
    repo_root: &Path,
    relative_script: &str,
    args: &[&str],
    repo_owner_user: &str,
) -> Result<()> {
    let script_path = repo_root.join(relative_script);
    let program = script_path
        .to_str()
        .context("script path contains non-utf8 characters")?;
    run_command_in_dir_as_user(program, args, repo_root, repo_owner_user)
}

pub fn run_as_native_user_shell(script: &str, user_context: &NativeUserContext) -> Result<()> {
    let current_uid = id_value("-u")?;
    let status = if current_uid == "0" && user_context.name != "root" {
        let mut cmd = Command::new(resolve_command_path("runuser"));
        cmd.arg("-u")
            .arg(user_context.name.as_str())
            .arg("--")
            .arg("bash")
            .arg("-lc")
            .arg(script)
            .env("HOME", &user_context.home)
            .env("USER", &user_context.name)
            .env("LOGNAME", &user_context.name)
            .env_remove("RUSTUP_HOME")
            .env_remove("CARGO_HOME");
        cmd.status().with_context(|| {
            format!(
                "failed to execute native-user shell for {}",
                user_context.name
            )
        })?
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg("-lc")
            .arg(script)
            .env("HOME", &user_context.home)
            .env("USER", &user_context.name)
            .env("LOGNAME", &user_context.name)
            .env_remove("RUSTUP_HOME")
            .env_remove("CARGO_HOME");
        cmd.status()
            .context("failed to execute native-user shell")?
    };

    ensure_success("native-user shell", status)
}

pub fn run_root_command_allow_failure(
    cmd: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) -> Result<ExitStatus> {
    let mut command = if user_context.uses_sudo_for_privileged_steps {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(cmd);
        c
    } else {
        Command::new(cmd)
    };

    command.args(args);

    command
        .status()
        .with_context(|| format!("failed to execute {cmd}"))
}

pub fn run_root_command_capture(
    program: &str,
    args: &[&str],
    user_context: &NativeUserContext,
) -> Result<String> {
    let output = if user_context.uses_sudo_for_privileged_steps {
        ensure_command_available("sudo")?;
        Command::new("sudo")
            .arg(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to execute sudo {program} {}", args.join(" ")))?
    } else {
        Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("failed to execute {program} {}", args.join(" ")))?
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut combined = String::new();
    if !stdout.trim().is_empty() {
        combined.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(stderr.trim());
    }
    if !output.status.success() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&format!(
            "(command exited with status {:?})",
            output.status.code()
        ));
    }
    Ok(combined)
}

pub fn has_cuda_build_support() -> bool {
    // nvcc available in PATH (distro package manager or toolkit already in PATH)
    if command_exists("nvcc") {
        return true;
    }
    // Common versioned CUDA toolkit prefixes installed by NVIDIA runfile / deb
    let cuda_nvcc_candidates = [
        "/usr/local/cuda/bin/nvcc",
        "/usr/local/cuda-12/bin/nvcc",
        "/usr/local/cuda-12.0/bin/nvcc",
        "/usr/local/cuda-12.1/bin/nvcc",
        "/usr/local/cuda-12.2/bin/nvcc",
        "/usr/local/cuda-12.3/bin/nvcc",
        "/usr/local/cuda-12.4/bin/nvcc",
        "/usr/local/cuda-12.5/bin/nvcc",
        "/usr/local/cuda-12.6/bin/nvcc",
        "/usr/local/cuda-11/bin/nvcc",
        "/usr/local/cuda-11.8/bin/nvcc",
        "/usr/lib/cuda/bin/nvcc",
        "/opt/cuda/bin/nvcc",
    ];
    for path in &cuda_nvcc_candidates {
        if Path::new(path).exists() {
            return true;
        }
    }
    // Toolkit root present — cmake FindCUDA/FindCUDAToolkit can locate nvcc from here
    if Path::new("/usr/local/cuda").exists()
        || Path::new("/usr/local/cuda-12").exists()
        || Path::new("/usr/lib/cuda").exists()
    {
        return true;
    }
    // CUDA headers present — toolkit installed but bin dir not added to PATH yet
    if Path::new("/usr/include/cuda.h").exists()
        || Path::new("/usr/local/cuda/include/cuda.h").exists()
        || Path::new("/usr/lib/cuda/include/cuda.h").exists()
    {
        return true;
    }
    false
}

pub fn detect_default_ai_backend(host: &HostPlatform) -> &'static str {
    if has_cuda_build_support() {
        return "cuda";
    }
    if command_exists("hipcc") || command_exists("rocminfo") || Path::new("/opt/rocm").exists() {
        return "rocm";
    }
    if command_exists("vulkaninfo") {
        return "vulkan";
    }
    if matches!(host.architecture.as_str(), "aarch64" | "arm64") {
        return "disabled";
    }
    // NVIDIA GPU present but CUDA toolkit not installed — fall back to CPU.
    // Install the CUDA toolkit (e.g. nvidia-cuda-toolkit) and re-run to enable GPU acceleration.
    if command_exists("nvidia-smi") {
        eprintln!(
            "[rustfin-installer] Note: NVIDIA GPU detected but CUDA build toolchain not found. AI inference will use CPU. Install the CUDA toolkit and re-deploy to enable GPU acceleration."
        );
    }
    "cpu"
}

pub fn resolve_ai_gpu_backend(host: &HostPlatform, requested: &str) -> anyhow::Result<String> {
    match requested {
        "auto" => Ok(detect_default_ai_backend(host).to_string()),
        "disabled" | "none" | "off" => Ok("disabled".to_string()),
        "cpu" => Ok("cpu".to_string()),
        "cuda" | "rocm" | "vulkan" => Ok(requested.to_string()),
        other => bail!("Unsupported RUSTFIN_AI_GPU_BACKEND value: {other}"),
    }
}

pub fn server_features_for_ai_backend(backend: &str) -> &'static str {
    match backend {
        "disabled" => "",
        "cpu" => "ai-cpu",
        "cuda" => "ai-cuda",
        "rocm" => "ai-rocm",
        "vulkan" => "ai-vulkan",
        _ => "",
    }
}

pub fn default_native_linux_target(host: &HostPlatform) -> anyhow::Result<&'static str> {
    match host.architecture.as_str() {
        "aarch64" | "arm64" => Ok("aarch64-unknown-linux-gnu"),
        "x86_64" | "amd64" => Ok("x86_64-unknown-linux-gnu"),
        other => bail!("unsupported host arch '{other}' for native Linux target selection"),
    }
}

pub fn ensure_cuda_lib_symlinks(user_context: &NativeUserContext) -> Result<()> {
    let cuda_lib64 = Path::new("/usr/lib/cuda/lib64");
    if !cuda_lib64.exists() {
        return Ok(());
    }
    let arch_lib = Path::new("/usr/lib/x86_64-linux-gnu");
    // All static/shared libs needed by llama-cpp-sys-2 when linking with ai-cuda
    let libs = [
        "libcudart_static.a",
        "libcudart.so",
        "libcudart.so.12",
        "libcublas_static.a",
        "libcublasLt_static.a",
        "libcublas.so",
        "libcublasLt.so",
        "libculibos.a",
    ];
    for lib in &libs {
        let src = arch_lib.join(lib);
        let dst = cuda_lib64.join(lib);
        if src.exists() && !dst.exists() {
            let _ = run_root_command(
                "ln",
                &[
                    "-sf",
                    src.to_str().unwrap_or(""),
                    dst.to_str().unwrap_or(""),
                ],
                user_context,
            )?;
        }
    }
    // Persist RUSTFLAGS so future cargo builds (including deploy-native) can find
    // the CUDA static libs in /usr/lib/cuda/lib64 without manual env setup.
    let env_d_dir = Path::new("/etc/environment.d");
    if env_d_dir.exists() {
        let env_file = env_d_dir.join("50-rustyfin-cuda.conf");
        if !env_file.exists() {
            let env_content = concat!(
                "# Written by rustfin-installer: paths for Ubuntu apt CUDA toolkit layout\n",
                "CUDA_PATH=/usr/lib/cuda\n",
                "RUSTFLAGS=-L/usr/lib/x86_64-linux-gnu -L/usr/lib/cuda/lib64\n",
            );
            let _ = run_root_command(
                "bash",
                &[
                    "-c",
                    &format!("printf '%s' '{}' > {}", env_content, env_file.display()),
                ],
                user_context,
            )?;
        }
    }
    Ok(())
}
