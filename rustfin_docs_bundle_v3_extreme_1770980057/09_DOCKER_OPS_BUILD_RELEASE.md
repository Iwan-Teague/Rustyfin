# Docker, Operations, Build & Release (Extreme Expansion)

Jellyfin container guidance is a good baseline for volume layout:
https://jellyfin.org/docs/general/installation/container/

## 1. Volume layout
- /config (db + keys)
- /cache (images/provider cache)
- /transcode (fast scratch)
- /media (read-only)

## 2. Multi-stage Dockerfile template
(See bundle for detailed example; includes ffmpeg and non-root user.)

## 3. GPU
NVIDIA toolkit:
https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html
