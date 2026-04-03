use crate::distro::DistroAdapter;
use crate::utils::{
    NativeUserContext, command_exists, ensure_command_available, ensure_cuda_lib_symlinks,
    has_cuda_build_support, run_root_command,
};
use anyhow::Result;

pub struct DebianAdapter {
    _version: String,
}

impl DebianAdapter {
    pub fn new(version: &str) -> Self {
        Self {
            _version: version.to_string(),
        }
    }
}

const RUNTIME_PACKAGES: &[&str] = &[
    "build-essential",
    "ca-certificates",
    "caddy",
    "clang",
    "clinfo",
    "cmake",
    "curl",
    "default-jre-headless",
    "ffmpeg",
    "git",
    "iproute2",
    "jq",
    "libclblast-dev",
    "libclang-dev",
    "libpq-dev",
    "libsqlite3-dev",
    "libssl-dev",
    "lsof",
    "nodejs",
    "npm",
    "ocl-icd-libopencl1",
    "ocl-icd-opencl-dev",
    "openssl",
    "pkg-config",
    "postgresql",
    "postgresql-client",
    "sudo",
    "python3",
    "python3-pip",
    "python3-venv",
];

impl DistroAdapter for DebianAdapter {
    fn name(&self) -> &str {
        "debian"
    }

    fn install_packages(&self, user_context: &NativeUserContext) -> Result<()> {
        ensure_command_available("apt-get")?;

        println!("[rustfin-installer] Updating apt repositories...");
        run_root_command("apt-get", &["update"], user_context)?;

        println!("[rustfin-installer] Installing Debian runtime packages...");
        let mut install_args = vec!["install", "-y"];
        install_args.extend(RUNTIME_PACKAGES.iter().copied());

        run_root_command("apt-get", &install_args, user_context)?;
        Ok(())
    }

    fn install_gpu_support(&self, user_context: &NativeUserContext) -> Result<()> {
        // Only attempt if an NVIDIA GPU is present
        if !command_exists("nvidia-smi") {
            return Ok(());
        }
        // Already have build support — nothing to do
        if has_cuda_build_support() {
            println!("[rustfin-installer] CUDA build support already available.");
            // Still ensure library symlinks exist (Ubuntu apt puts libs in a non-standard path)
            ensure_cuda_lib_symlinks(user_context)?;
            return Ok(());
        }
        println!(
            "[rustfin-installer] NVIDIA GPU detected but CUDA toolkit not found. Installing nvidia-cuda-toolkit..."
        );
        let result = run_root_command(
            "apt-get",
            &["install", "-y", "nvidia-cuda-toolkit"],
            user_context,
        );
        match result {
            Ok(_) => {
                println!("[rustfin-installer] CUDA toolkit installed successfully.");
                ensure_cuda_lib_symlinks(user_context)?;
            }
            Err(e) => {
                eprintln!(
                    "[rustfin-installer] Warning: CUDA toolkit install failed: {}. GPU-accelerated AI backend will be unavailable. Install manually: sudo apt-get install nvidia-cuda-toolkit",
                    e
                );
            }
        }
        Ok(())
    }
}
