FROM rustlang/rust:nightly-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN set -eu; \
    for attempt in 1 2 3; do \
      if cargo build --release --bin rustfin-calendar; then \
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

COPY --from=builder /app/target/release/rustfin-calendar /usr/local/bin/rustfin-calendar

USER rustfin
EXPOSE 8099

ENV RUSTFIN_CALENDAR_BIND=0.0.0.0:8099
ENV RUSTFIN_DB=/config/rustfin.db
ENV RUSTFIN_AUTH_BASE_URL=http://rustfin:8096

CMD ["rustfin-calendar"]
