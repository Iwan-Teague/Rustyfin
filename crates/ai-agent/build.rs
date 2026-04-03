fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" {
        return;
    }

    // llama-cpp-2 reads these env vars during its build.rs to select backends
    if cfg!(feature = "cuda") {
        println!("cargo:rustc-env=LLAMA_CUDA=1");
    }
    if cfg!(feature = "rocm") {
        println!("cargo:rustc-env=LLAMA_HIPBLAS=1");
    }
    if cfg!(feature = "vulkan") {
        println!("cargo:rustc-env=LLAMA_VULKAN=1");
    }
}
