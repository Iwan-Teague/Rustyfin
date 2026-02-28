FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /config && chown -R rustfin:rustfin /config

COPY ${NATIVE_BIN_DIR}/rustfin-calendar /usr/local/bin/rustfin-calendar
RUN chmod 755 /usr/local/bin/rustfin-calendar

USER rustfin

EXPOSE 8099

ENV RUSTFIN_CALENDAR_BIND=0.0.0.0:8099
ENV RUSTFIN_DATABASE_URL=postgresql://rustfin:rustfin@postgres:5432/rustfin
ENV RUSTFIN_AUTH_BASE_URL=http://rustfin:8096

CMD ["rustfin-calendar"]
