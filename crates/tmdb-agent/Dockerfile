FROM rust:bookworm AS builder
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
        cmd="cargo build --release --bin rustfin-tmdb-agent"; \
        built="/app/target/release/rustfin-tmdb-agent"; \
      elif [ "$profile" = "dev" ] || [ "$profile" = "debug" ]; then \
        cmd="cargo build --bin rustfin-tmdb-agent"; \
        built="/app/target/debug/rustfin-tmdb-agent"; \
      else \
        cmd="cargo build --profile $profile --bin rustfin-tmdb-agent"; \
        built="/app/target/$profile/rustfin-tmdb-agent"; \
      fi; \
      if sh -c "$cmd" && [ -f "$built" ] && cp "$built" "$out_dir/rustfin-tmdb-agent"; then \
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
RUN mkdir -p /config /cache && chown -R rustfin:rustfin /config /cache

COPY --from=builder /tmp/rustfin-out/rustfin-tmdb-agent /usr/local/bin/rustfin-tmdb-agent

USER rustfin
EXPOSE 8100

ENV RUSTFIN_TMDB_AGENT_BIND=0.0.0.0:8100
ENV RUSTFIN_DB=/config/rustfin.db
ENV RUSTFIN_CACHE_DIR=/cache

CMD ["rustfin-tmdb-agent"]
