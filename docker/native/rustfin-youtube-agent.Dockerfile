FROM debian:bookworm-slim

ARG NATIVE_BIN_DIR=.tmp/native-bins

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl ffmpeg nodejs python3 python3-pip \
    && python3 -m pip install --break-system-packages --no-cache-dir --upgrade yt-dlp \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 rustfin
RUN mkdir -p /cache && chown -R rustfin:rustfin /cache

COPY ${NATIVE_BIN_DIR}/rustfin-youtube-agent /usr/local/bin/rustfin-youtube-agent
RUN chmod 755 /usr/local/bin/rustfin-youtube-agent

USER rustfin

EXPOSE 8101

ENV RUSTFIN_YOUTUBE_AGENT_BIND=0.0.0.0:8101
ENV RUSTFIN_CACHE_DIR=/cache
ENV RUSTFIN_FFMPEG_PATH=ffmpeg
ENV RUSTFIN_FFPROBE_PATH=ffprobe
ENV RUSTFIN_YTDLP_PATH=yt-dlp

CMD ["rustfin-youtube-agent"]
