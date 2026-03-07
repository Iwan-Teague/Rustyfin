FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins
ARG RUSTFIN_TRANSCRIPTION_AGENT_CARGO_FEATURES=

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    clinfo \
    curl \
    libclblast1 \
    ocl-icd-libopencl1 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /cache && chown -R rustfin:rustfin /cache

COPY ${NATIVE_BIN_DIR}/rustfin-transcription-agent /usr/local/bin/rustfin-transcription-agent
RUN chmod 755 /usr/local/bin/rustfin-transcription-agent

USER rustfin

EXPOSE 8102

ENV RUSTFIN_TRANSCRIPTION_AGENT_BIND=0.0.0.0:8102
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_TRANSCRIPTION_GPU_MODE=opencl
ENV RUSTFIN_TRANSCRIPTION_REQUIRE_GPU=1
ENV RUSTFIN_WHISPER_MODEL_PATH=/cache/whisper/ggml-small.en.bin
ENV RUSTFIN_WHISPER_MODEL_URL=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
ENV RUSTFIN_TRANSCRIPTION_MAX_PARALLEL_INFERENCES=3
ENV RUSTFIN_TRANSCRIPTION_MAX_WORKERS=6
ENV RUSTFIN_TRANSCRIPTION_MAX_WORKERS_PER_SESSION=8
ENV RUSTFIN_TRANSCRIPTION_THREADS_PER_WORKER=2
ENV RUSTFIN_TRANSCRIPTION_ACQUIRE_TIMEOUT_MS=2500

CMD ["rustfin-transcription-agent"]
