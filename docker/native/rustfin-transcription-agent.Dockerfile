FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /cache && chown -R rustfin:rustfin /cache

COPY ${NATIVE_BIN_DIR}/rustfin-transcription-agent /usr/local/bin/rustfin-transcription-agent
RUN chmod 755 /usr/local/bin/rustfin-transcription-agent

USER rustfin

EXPOSE 8102

ENV RUSTFIN_TRANSCRIPTION_AGENT_BIND=0.0.0.0:8102
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_WHISPER_MODEL_PATH=/cache/whisper/ggml-base.en.bin
ENV RUSTFIN_WHISPER_MODEL_URL=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin

CMD ["rustfin-transcription-agent"]
