# Streaming, Transcoding, and GPU (Extreme Expansion)

## 1. Decision engine
Direct Play → Remux → Transcode (HLS).

## 2. Range (Direct Play)
RFC 7233: https://www.rfc-editor.org/rfc/rfc7233  
Accept-Ranges header: https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Accept-Ranges

## 3. HLS (adaptive streaming)
RFC 8216: https://www.rfc-editor.org/rfc/rfc8216  
Apple authoring guidance: https://developer.apple.com/documentation/http-live-streaming/hls-authoring-specification-for-apple-devices

## 4. Web playback
hls.js guidance:
https://github.com/video-dev/hls.js/

## 5. GPU in Docker
NVIDIA toolkit:
https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html

Intel/AMD: map `/dev/dri`.
