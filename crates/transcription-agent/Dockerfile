FROM rustlang/rust:nightly-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*

ENV WHISPER_DONT_GENERATE_BINDINGS=1
ENV CARGO_BUILD_JOBS=2

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN set -eu; \
    for attempt in 1 2 3; do \
      if cargo build --bin rustfin-transcription-agent; then \
        exit 0; \
      fi; \
      echo "cargo build failed (attempt ${attempt}/3); retrying..."; \
      sleep $((attempt * 5)); \
    done; \
    exit 1

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /cache && chown -R rustfin:rustfin /cache

COPY --from=builder /app/target/debug/rustfin-transcription-agent /usr/local/bin/rustfin-transcription-agent

USER rustfin
EXPOSE 8102

ENV RUSTFIN_TRANSCRIPTION_AGENT_BIND=0.0.0.0:8102
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_WHISPER_MODEL_PATH=/cache/whisper/ggml-base.en.bin
ENV RUSTFIN_WHISPER_MODEL_URL=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin

CMD ["rustfin-transcription-agent"]
