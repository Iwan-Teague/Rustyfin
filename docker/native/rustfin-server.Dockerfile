FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /config /cache /transcode /media && \
    chown -R rustfin:rustfin /config /cache /transcode

COPY ${NATIVE_BIN_DIR}/rustfin-server /usr/local/bin/rustfin-server
RUN chmod 755 /usr/local/bin/rustfin-server

USER rustfin

ENV RUSTFIN_DATABASE_URL=postgresql://rustfin:rustfin@postgres:5432/rustfin
ENV RUSTFIN_TRANSCODE_DIR=/transcode
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_BIND=0.0.0.0:8096
ENV RUST_LOG=info

EXPOSE 8096

VOLUME ["/config", "/cache", "/transcode", "/media"]

HEALTHCHECK --interval=30s --timeout=3s \
    CMD curl -f http://localhost:8096/health || exit 1

ENTRYPOINT ["rustfin-server"]
