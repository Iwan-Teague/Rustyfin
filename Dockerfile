# Multi-stage build for Rustfin server
FROM rustlang/rust:nightly-bookworm AS builder

WORKDIR /app

ENV CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary (with retries for transient registry/network issues)
RUN set -eu; \
    for attempt in 1 2 3; do \
      if cargo build --release --bin rustfin-server; then \
        exit 0; \
      fi; \
      echo "cargo build failed (attempt ${attempt}/3); retrying..."; \
      sleep $((attempt * 5)); \
    done; \
    exit 1

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 rustfin

# Create data directories
RUN mkdir -p /config /cache /transcode /media && \
    chown -R rustfin:rustfin /config /cache /transcode

COPY --from=builder /app/target/release/rustfin-server /usr/local/bin/rustfin-server

USER rustfin

ENV RUSTFIN_DB=/config/rustfin.db
ENV RUSTFIN_TRANSCODE_DIR=/transcode
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_BIND=0.0.0.0:8096
ENV RUST_LOG=info

EXPOSE 8096

VOLUME ["/config", "/cache", "/transcode", "/media"]

HEALTHCHECK --interval=30s --timeout=3s \
    CMD curl -f http://localhost:8096/health || exit 1

ENTRYPOINT ["rustfin-server"]
