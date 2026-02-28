FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /config /cache && chown -R rustfin:rustfin /config /cache

COPY ${NATIVE_BIN_DIR}/rustfin-tmdb-agent /usr/local/bin/rustfin-tmdb-agent
RUN chmod 755 /usr/local/bin/rustfin-tmdb-agent

USER rustfin

EXPOSE 8100

ENV RUSTFIN_TMDB_AGENT_BIND=0.0.0.0:8100
ENV RUSTFIN_DATABASE_URL=postgresql://rustfin:rustfin@postgres:5432/rustfin
ENV RUSTFIN_CACHE_DIR=/cache

CMD ["rustfin-tmdb-agent"]
