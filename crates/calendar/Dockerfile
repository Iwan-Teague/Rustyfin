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
        cmd="cargo build --release --bin rustfin-calendar"; \
        built="/app/target/release/rustfin-calendar"; \
      elif [ "$profile" = "dev" ] || [ "$profile" = "debug" ]; then \
        cmd="cargo build --bin rustfin-calendar"; \
        built="/app/target/debug/rustfin-calendar"; \
      else \
        cmd="cargo build --profile $profile --bin rustfin-calendar"; \
        built="/app/target/$profile/rustfin-calendar"; \
      fi; \
      if sh -c "$cmd" && [ -f "$built" ] && cp "$built" "$out_dir/rustfin-calendar"; then \
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
RUN mkdir -p /config && chown -R rustfin:rustfin /config

COPY --from=builder /tmp/rustfin-out/rustfin-calendar /usr/local/bin/rustfin-calendar

USER rustfin
EXPOSE 8099

ENV RUSTFIN_CALENDAR_BIND=0.0.0.0:8099
ENV RUSTFIN_DB=/config/rustfin.db
ENV RUSTFIN_AUTH_BASE_URL=http://rustfin:8096

CMD ["rustfin-calendar"]
