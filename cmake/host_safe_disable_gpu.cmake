# Host-safe override for verification commands on machines without CUDA/ROCm/Vulkan toolchains.
# This keeps all-features static analysis runnable while leaving Linux runtime selection logic intact.

set(GGML_CUDA OFF CACHE BOOL "" FORCE)
set(GGML_HIP OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN OFF CACHE BOOL "" FORCE)

set(WHISPER_CUDA OFF CACHE BOOL "" FORCE)
set(WHISPER_HIPBLAS OFF CACHE BOOL "" FORCE)
set(WHISPER_CLBLAST OFF CACHE BOOL "" FORCE)

set(CMAKE_C_COMPILER "/usr/bin/cc" CACHE FILEPATH "" FORCE)
set(CMAKE_CXX_COMPILER "/usr/bin/c++" CACHE FILEPATH "" FORCE)
