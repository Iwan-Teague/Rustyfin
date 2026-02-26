FROM rustlang/rust:nightly-bookworm AS builder
WORKDIR /app
ARG RUST_BUILD_PROFILE=dev

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN set -eu; \
    out_dir="/tmp/rustfin-out"; \
    mkdir -p "$out_dir"; \
    profile="${RUST_BUILD_PROFILE:-dev}"; \
    for attempt in 1 2 3; do \
      if [ "$profile" = "release" ]; then \
        cmd="cargo build --release --bin rustfin-youtube-agent"; \
        built="/app/target/release/rustfin-youtube-agent"; \
      elif [ "$profile" = "dev" ] || [ "$profile" = "debug" ]; then \
        cmd="cargo build --bin rustfin-youtube-agent"; \
        built="/app/target/debug/rustfin-youtube-agent"; \
      else \
        cmd="cargo build --profile $profile --bin rustfin-youtube-agent"; \
        built="/app/target/$profile/rustfin-youtube-agent"; \
      fi; \
      if sh -c "$cmd" && [ -f "$built" ] && cp "$built" "$out_dir/rustfin-youtube-agent"; then \
        exit 0; \
      fi; \
      echo "cargo build failed (attempt ${attempt}/3); retrying..."; \
      sleep $((attempt * 5)); \
    done; \
    exit 1

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl ffmpeg \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /cache && chown -R rustfin:rustfin /cache

COPY --from=builder /tmp/rustfin-out/rustfin-youtube-agent /usr/local/bin/rustfin-youtube-agent

USER rustfin
EXPOSE 8101

ENV RUSTFIN_YOUTUBE_AGENT_BIND=0.0.0.0:8101
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_FFMPEG_PATH=ffmpeg
ENV RUSTFIN_FFPROBE_PATH=ffprobe

CMD ["rustfin-youtube-agent"]
