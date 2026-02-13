# Deep Dive: How Jellyfin Works (and what to steal for a Rust reimplementation)

Jellyfin is a full media system: library management, metadata, multi-user auth, device-aware sessions, streaming (direct play/remux/transcode), and GPU-accelerated transcoding through FFmpeg.

This document does two things:

1) **Explains how Jellyfin’s server is put together** (with concrete pointers into the actual code paths and API shapes).
2) **Gives you a Rust-first architecture** (code-heavy skeletons you can actually start from).

---

## Table of contents

- [Big picture](#big-picture)
- [Hosting + boot](#hosting--boot)
- [Data/config/cache paths](#dataconfigcache-paths)
- [Database + entities](#database--entities)
- [Users + auth + permissions](#users--auth--permissions)
- [Sessions + remote control + playstate](#sessions--remote-control--playstate)
- [Library scanning](#library-scanning)
- [Metadata pipeline](#metadata-pipeline)
- [Streaming decision engine](#streaming-decision-engine)
- [Streaming protocols: progressive + HLS](#streaming-protocols-progressive--hls)
- [Transcoding pipeline + job lifecycle](#transcoding-pipeline--job-lifecycle)
- [Subtitles and attachments](#subtitles-and-attachments)
- [Hardware acceleration](#hardware-acceleration)
- [Plugins](#plugins)
- [Build-your-own in Rust](#build-your-own-in-rust)
- [Rust code skeletons](#rust-code-skeletons)
- [Reference links](#reference-links)

---

## Big picture

### “What Jellyfin is” in one diagram

```
                 ┌───────────────────────────────────────────┐
                 │               Jellyfin Server             │
                 │    (ASP.NET Core API + internal services) │
                 ├───────────────────────────────────────────┤
 Clients         │  Users/Auth  ─────────┐                    │
 (Web, TV,       │  Sessions/Devices     │                    │
 Mobile, etc.) ──┼─▶ Library + Metadata  │                    │
                 │  Media probe (ffprobe)│                    │
                 │  Stream decision      │                    │
                 │  Streaming endpoints  │──▶ Direct bytes / HLS playlists
                 │  Transcode manager    │──▶ ffmpeg processes (CPU/GPU)
                 └───────────────────────────────────────────┘
                                  │
                                  ▼
                     ┌─────────────────────────┐
                     │  Persistent state       │
                     │  - Config files         │
                     │  - SQLite DB (EF Core)  │
                     │  - Metadata cache       │
                     │  - Transcode cache      │
                     └─────────────────────────┘
```

### The “core trick” Jellyfin uses

When a client wants to play something, Jellyfin:

1. Identifies the **user** and **device**
2. Loads a **DeviceProfile** (capabilities)
3. Reads/probes the **MediaSource** (container/codecs/bitrate/subtitles)
4. Runs a decision engine (DirectPlay / DirectStream(remux) / Transcode)
5. If transcoding is needed, starts and supervises an **ffmpeg process**
6. Serves output via progressive stream or HLS, while tracking session heartbeats

Everything else is support structure around that loop.

---

## Hosting + boot

Jellyfin is a .NET server using **ASP.NET Core** and the Generic Host. The practical implications:

- Controllers are “thin” HTTP surfaces.
- The heavy logic lives in services injected via dependency injection.
- Configuration is merged from CLI/env/config files.
- The web server is Kestrel, with common ports **8096 (HTTP)** and **8920 (HTTPS)**.

If you build your own:
- Choose a Rust web framework with middleware + DI-ish “state injection” patterns (Axum works well).
- Keep a clear separation between **API layer** and **core services**.

---

## Data/config/cache paths

Jellyfin’s docs recommend separating **data**, **config**, and **cache** (especially on Linux via XDG-style locations). Your clone should do the same or you will eventually suffer (usually when transcodes fill up your SSD).

Clone-friendly layout:

```
/var/lib/myfin/          # data: authoritative state
  db.sqlite3
  library/
  metadata/

 /var/cache/myfin/       # cache: disposable
  transcodes/
  images/
  temp/

 /etc/myfin/             # config
  server.toml
  encoding.toml
  devices/
```

---

## Database + entities

Jellyfin uses **SQLite via Entity Framework Core**. Think of it as two main worlds:

1) **Library entities** (movies/episodes/tracks, plus stream info, chapters, images, etc.)
2) **Account/session entities** (users, API keys, devices, user data like playback progress)

For a clone, you don’t need Jellyfin’s exact schema, but you do need the same conceptual separation:

- `items`: logical media entities
- `media_sources`: physical file versions and probe results
- `images`: cached poster/backdrop/thumb metadata
- `users`, `api_keys`, `devices`
- `sessions`: active playback sessions
- `user_item_state`: progress ticks, played/unplayed, favorite

---

## Users + auth + permissions

A Jellyfin-ish system needs:

- **Authentication**: prove who you are
- **Device identity**: prove what device you are
- **Authorization**: prove what you’re allowed to do

### A practical auth model for your clone

- Use **JWT bearer tokens** (standard) or a Jellyfin-like token + custom header.
- Keep permissions explicit:
  - stream audio/video
  - transcode audio/video (separate!)
  - manage libraries
  - manage users
  - admin

---

## Sessions + remote control + playstate

Jellyfin tracks “what is playing” using sessions.

A key detail: the server uses session heartbeats to decide when to **kill transcoding jobs**.

It also supports remote control (“tell this TV to play that movie”), which is basically:
- user A issues a command
- server forwards to session B
- client B executes

For your clone, implement:
- session registry
- playback start/stop/progress endpoints
- a simple remote-command channel (WebSocket or long-poll)

---

## Library scanning

Library scanning is one of those things that seems easy until it eats your weekend.

The real problem:
- filenames are lies
- folder structures are semi-standards, not standards
- you need stable IDs so metadata and user progress survive renames
- scanning must be incremental and resilient

A sane approach:
- scanning pipeline with stages
- store “fingerprints” (path + size + mtime + inode if available)
- probe media lazily if possible (or on-demand)

---

## Metadata pipeline

Jellyfin uses provider orchestration: local metadata (like NFO) + remote metadata (TMDb, etc.) + image providers + savers.

What’s worth copying is the *control flow*, not the specific providers:

- “refresh metadata” is parameterized:
  - validate-only vs full refresh
  - replace all metadata/images
  - automated refresh intervals

### Concrete Jellyfin code clue: MetadataService orchestration

In Jellyfin’s `MetadataService.RefreshMetadata(...)`, you can see the steps:

- validate/remove images depending on options
- run metadata providers if needed
- refresh remote images if appropriate
- update `DateLastRefreshed`
- persist changes to repository

That’s exactly the orchestration you want, regardless of language.

---

## Streaming decision engine

This is the engine room.

### Direct play vs direct stream vs transcode

- **DirectPlay**: send the original file/container/streams
- **DirectStream**: remux container (e.g., MKV → TS) but copy codecs
- **Transcode**: re-encode (audio/video), maybe burn subtitles

### Concrete Jellyfin clue: `StreamBuilder` and explicit transcode reasons

Jellyfin’s `StreamBuilder` declares categories of “reasons” that trigger transcode vs direct stream.

Here’s a trimmed excerpt (C#) showing that structure and the codec restrictions for HLS:

```csharp
internal const TranscodeReason ContainerReasons =
    TranscodeReason.ContainerNotSupported | TranscodeReason.ContainerBitrateExceedsLimit;

internal const TranscodeReason AudioCodecReasons =
    TranscodeReason.AudioBitrateNotSupported |
    TranscodeReason.AudioChannelsNotSupported |
    TranscodeReason.AudioProfileNotSupported |
    TranscodeReason.AudioSampleRateNotSupported |
    TranscodeReason.SecondaryAudioNotSupported |
    TranscodeReason.AudioBitDepthNotSupported |
    TranscodeReason.AudioIsExternal;

internal const TranscodeReason VideoCodecReasons =
    TranscodeReason.VideoResolutionNotSupported |
    TranscodeReason.AnamorphicVideoNotSupported |
    TranscodeReason.InterlacedVideoNotSupported |
    TranscodeReason.VideoBitDepthNotSupported |
    TranscodeReason.VideoBitrateNotSupported |
    TranscodeReason.VideoFramerateNotSupported |
    TranscodeReason.VideoLevelNotSupported |
    TranscodeReason.RefFramesNotSupported |
    TranscodeReason.VideoRangeTypeNotSupported |
    TranscodeReason.VideoProfileNotSupported;

private static readonly string[] _supportedHlsVideoCodecs = ["h264", "hevc", "vp9", "av1"];
private static readonly string[] _supportedHlsAudioCodecsTs = ["aac", "ac3", "eac3", "mp3"];
private static readonly string[] _supportedHlsAudioCodecsMp4 =
    ["aac", "ac3", "eac3", "mp3", "alac", "flac", "opus", "dts", "truehd"];
```

That tells you something important: Jellyfin’s decision engine is not “a vibe”. It is *codified*, explainable, and debuggable.

### Copy this design

In your clone:
- enumerate reasons (debug gold)
- include the reasons in the playback info returned to clients
- log them when transcoding starts

---

## Streaming protocols: progressive + HLS

Jellyfin exposes a lot of endpoints, but the patterns are:

### 1) Progressive HTTP streaming
- Typically used for direct play or “single-file” transcodes.
- Supports seeking via HTTP range.

### 2) HLS streaming
- Produces `.m3u8` playlists + segments (`.ts` or fragmented `.mp4`)

Concrete Jellyfin clue: in `UniversalAudioController`, when HLS is selected, it sets:

- segment container restricted to `ts` or `mp4`
- `EnableAdaptiveBitrateStreaming = false` (single-variant HLS by default)

Trimmed excerpt:

```csharp
var supportedHlsContainers = new[] { "ts", "mp4" };
// fallback to mpegts if device reports weird value
var requestedSegmentContainer = Array.Exists(supportedHlsContainers, element =>
    string.Equals(element, transcodingContainer, StringComparison.OrdinalIgnoreCase))
        ? transcodingContainer
        : "ts";

var dynamicHlsRequestDto = new HlsAudioRequestDto {
    Container = ".m3u8",
    SegmentContainer = segmentContainer,
    SubtitleMethod = SubtitleDeliveryMethod.Hls,
    EnableAdaptiveBitrateStreaming = false,
    // ...
};
```

So: Jellyfin uses HLS primarily as “chunked delivery for a single chosen encode” rather than full ABR ladder generation.

---

## Transcoding pipeline + job lifecycle

Jellyfin runs FFmpeg as an external process and manages it as a job.

### Concrete Jellyfin clue: FFmpeg path precedence + capability interrogation

In `MediaEncoder.SetFFmpegPath()` Jellyfin’s logic is:

- CLI/env override
- config fallback
- system PATH fallback
- validate
- then interrogate ffmpeg for:
  - encoders/decoders
  - filters
  - hwaccels
  - version gates for features

Trimmed excerpt:

```csharp
// Precedence is: CLI/Env var > Config > $PATH.
var ffmpegPath = _startupOptionFFmpegPath;
if (string.IsNullOrEmpty(ffmpegPath)) {
    ffmpegPath = _configurationManager.GetEncodingOptions().EncoderAppPath;
    if (string.IsNullOrEmpty(ffmpegPath)) {
        ffmpegPath = "ffmpeg";
    }
}
```

### Concrete Jellyfin clue: job management, pings, and kill timers

In `TranscodeManager.PingTranscodingJob(...)` Jellyfin updates a job’s `LastPingDate` and starts a kill timer.
- Progressive jobs get tighter ping windows.
- HLS jobs get longer ping windows.

Trimmed excerpt:

```csharp
var timerDuration = 10000;
if (job.Type != TranscodingJobType.Progressive) {
    timerDuration = 60000;
}
job.PingTimeout = timerDuration;
job.LastPingDate = DateTime.UtcNow;
```

Then if the timer fires without recent pings, it kills the job and deletes partial outputs.

### Concrete Jellyfin clue: starting ffmpeg + subtitle attachment extraction

In `TranscodeManager.StartFfMpeg(...)`, before spawning the ffmpeg process, Jellyfin:

- enforces user permissions for video transcoding
- ensures output directory exists
- extracts attachments when burning subtitles (fonts, etc.)
- logs the exact ffmpeg command line
- classifies log file prefix (Transcode/Remux/DirectStream)

Trimmed excerpt:

```csharp
if (state.VideoRequest is not null && !EncodingHelper.IsCopyCodec(state.OutputVideoCodec)) {
    // check user permission EnableVideoPlaybackTranscoding
}

if (state.SubtitleStream is not null && (Encode || AlwaysBurnInSubtitleWhenTranscoding)) {
    await _attachmentExtractor.ExtractAllAttachments(...);
}

var process = new Process {
    StartInfo = new ProcessStartInfo {
        FileName = _mediaEncoder.EncoderPath,
        Arguments = commandLineArguments,
        RedirectStandardError = true,
        RedirectStandardInput = true,
        // ...
    }
};
```

### Segment cleanup

Jellyfin’s `TranscodingSegmentCleaner` periodically deletes old HLS segments behind the playback position when segment deletion is enabled.

Trimmed excerpt:

```csharp
_timer = new Timer(TimerCallback, null, 20000, 20000);

if (enableSegmentDeletion) {
    var idxMaxToDelete = (downloadPositionSeconds - segmentKeepSeconds) / _segmentLength;
    if (idxMaxToDelete > 0) {
        await DeleteSegmentFiles(_job, 0, idxMaxToDelete, 1500);
    }
}
```

This is a *huge* operational detail: without cleanup, HLS transcoding can eat storage fast.

---

## Subtitles and attachments

Subtitles are a trap. Jellyfin handles multiple delivery methods:

- embedded vs external subtitles
- burn-in vs extract vs sidecar delivery
- attachment extraction for fonts (especially ASS/SSA)

A pragmatic clone path:

1) Support external **SRT** and convert to **WebVTT** for web playback.
2) For everything else, burn-in via ffmpeg.
3) Later: implement richer subtitle selection and delivery modes.

---

## Hardware acceleration

Jellyfin supports many hwaccel modes (VAAPI, QSV, NVENC, VideoToolbox, etc.). The real design point is:

- determine what the system supports
- choose ffmpeg flags accordingly
- fall back to software if hwaccel fails

Operational advice for your clone:
- make hwaccel **optional and per-platform**
- include “hwaccel probe results” in diagnostics endpoints
- store working presets (hwaccel is brittle across driver versions)

---

## Plugins

Jellyfin has a plugin ecosystem. For your clone:
- design your internal architecture with **traits** (interfaces) so you can add plugin implementations later
- don’t implement plugin loading until your core is stable

Rust-friendly plugin options:
- WASM plugins (safe-ish, portable)
- dynamic library plugins (fast, more dangerous ABI surface)

---

# Build-your-own in Rust

A realistic Rust architecture mirroring Jellyfin:

- `myfin-api` (Axum REST API)
- `myfin-core` (domain model + decision engine)
- `myfin-db` (SQL migrations + repositories)
- `myfin-media` (ffprobe parsing + ffmpeg orchestration + HLS generator)
- `myfin-jobs` (scheduled tasks, scanning pipeline)

---

# Rust code skeletons

These are intentionally “starter but serious”: minimal dependencies, clear boundaries.

## Common types

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone)]
pub struct AuthToken(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    StreamAudio,
    StreamVideo,
    TranscodeAudio,
    TranscodeVideo,
    ManageLibrary,
    ManageUsers,
    Admin,
}
```

## Decision engine types (copy the “reasons” pattern)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeReason {
    ContainerNotSupported,
    ContainerBitrateExceedsLimit,
    VideoCodecNotSupported,
    AudioCodecNotSupported,
    VideoResolutionNotSupported,
    VideoBitDepthNotSupported,
    VideoProfileNotSupported,
    VideoLevelNotSupported,
    VideoBitrateNotSupported,
    AudioChannelsNotSupported,
    SubtitleNotSupported,
}

#[derive(Debug, Clone)]
pub enum PlayMethod {
    DirectPlay,
    DirectStream { remux_container: String },
    Transcode(TranscodePlan),
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub method: PlayMethod,
    pub reasons: Vec<TranscodeReason>,
    pub selected_source_id: String,
}
```

## Device profile

```rust
#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub name: String,
    pub max_video_bitrate: Option<u32>,
    pub max_audio_bitrate: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub containers: Vec<String>,
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
    pub subtitle_formats: Vec<String>,
}
```

## Media source (from ffprobe)

```rust
#[derive(Debug, Clone)]
pub struct MediaSource {
    pub source_id: String,
    pub path: String,
    pub container: String,
    pub bitrate: Option<u32>,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
}

#[derive(Debug, Clone)]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub bit_depth: Option<u8>,
    pub bitrate: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct AudioStream {
    pub codec: String,
    pub channels: u8,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub codec: String, // "srt", "ass", "pgs"
    pub is_external: bool,
}
```

## Decision function (minimal but mirrors Jellyfin’s logic style)

```rust
pub fn decide_playback(profile: &DeviceProfile, src: &MediaSource) -> Decision {
    let mut reasons = Vec::new();

    // container supported?
    if !profile.containers.iter().any(|c| c.eq_ignore_ascii_case(&src.container)) {
        reasons.push(TranscodeReason::ContainerNotSupported);
    }

    // bitrate cap?
    if let (Some(max), Some(br)) = (profile.max_video_bitrate, src.bitrate) {
        if br > max {
            reasons.push(TranscodeReason::ContainerBitrateExceedsLimit);
        }
    }

    // video checks
    if let Some(v) = &src.video {
        if !profile.video_codecs.iter().any(|c| c.eq_ignore_ascii_case(&v.codec)) {
            reasons.push(TranscodeReason::VideoCodecNotSupported);
        }
        if let (Some(mw), Some(mh)) = (profile.max_width, profile.max_height) {
            if v.width > mw || v.height > mh {
                reasons.push(TranscodeReason::VideoResolutionNotSupported);
            }
        }
    }

    // audio checks
    if let Some(a) = &src.audio {
        if !profile.audio_codecs.iter().any(|c| c.eq_ignore_ascii_case(&a.codec)) {
            reasons.push(TranscodeReason::AudioCodecNotSupported);
        }
        if a.channels > 2 {
            reasons.push(TranscodeReason::AudioChannelsNotSupported);
        }
    }

    if reasons.is_empty() {
        return Decision {
            method: PlayMethod::DirectPlay,
            reasons,
            selected_source_id: src.source_id.clone(),
        };
    }

    let only_container = reasons.iter().all(|r| *r == TranscodeReason::ContainerNotSupported);
    if only_container {
        return Decision {
            method: PlayMethod::DirectStream { remux_container: "ts".into() },
            reasons,
            selected_source_id: src.source_id.clone(),
        };
    }

    Decision {
        method: PlayMethod::Transcode(TranscodePlan::default_h264_aac_hls()),
        reasons,
        selected_source_id: src.source_id.clone(),
    }
}
```

## ffprobe JSON parsing

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Ffprobe {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    format_name: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    bit_rate: Option<String>,
    channels: Option<u8>,
    sample_rate: Option<String>,
}
```

## Transcode plan + ffmpeg args (HLS single-variant)

```rust
#[derive(Debug, Clone)]
pub struct TranscodePlan {
    pub video_codec: String, // "libx264"
    pub audio_codec: String, // "aac"
    pub video_bitrate: Option<u32>,
    pub audio_bitrate: Option<u32>,
    pub segment_seconds: u32,
}

impl TranscodePlan {
    pub fn default_h264_aac_hls() -> Self {
        Self {
            video_codec: "libx264".into(),
            audio_codec: "aac".into(),
            video_bitrate: Some(3_000_000),
            audio_bitrate: Some(160_000),
            segment_seconds: 4,
        }
    }

    pub fn ffmpeg_args(&self, input: &str, out_m3u8: &str) -> Vec<String> {
        let mut args = vec![
            "-hide_banner".into(),
            "-loglevel".into(), "warning".into(),
            "-i".into(), input.into(),
            "-c:v".into(), self.video_codec.clone(),
            "-c:a".into(), self.audio_codec.clone(),
        ];

        if let Some(vb) = self.video_bitrate {
            args.extend(["-b:v".into(), vb.to_string()]);
        }
        if let Some(ab) = self.audio_bitrate {
            args.extend(["-b:a".into(), ab.to_string()]);
        }

        args.extend([
            "-f".into(), "hls".into(),
            "-hls_time".into(), self.segment_seconds.to_string(),
            "-hls_playlist_type".into(), "event".into(),
            "-hls_flags".into(), "independent_segments".into(),
            out_m3u8.into(),
        ]);

        args
    }
}
```

## Transcode job manager (pings + kill timer)

```rust
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{process::Child, sync::Mutex, time::Instant};

#[derive(Debug)]
pub struct Job {
    pub session_id: String,
    pub device_id: String,
    pub last_ping: Instant,
    pub child: Child,
    pub output_playlist: String,
}

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Mutex<HashMap<String, Job>>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub async fn start_ffmpeg_hls(&self, session_id: String, device_id: String, input: String, out: String, plan: TranscodePlan) -> anyhow::Result<()> {
        use tokio::process::Command;

        let mut child = Command::new("ffmpeg")
            .args(plan.ffmpeg_args(&input, &out))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let job = Job {
            session_id: session_id.clone(),
            device_id,
            last_ping: Instant::now(),
            child,
            output_playlist: out,
        };

        self.inner.lock().await.insert(session_id, job);
        Ok(())
    }

    pub async fn ping(&self, session_id: &str) {
        if let Some(job) = self.inner.lock().await.get_mut(session_id) {
            job.last_ping = Instant::now();
        }
    }

    pub async fn watchdog(self) {
        let timeout = Duration::from_secs(60); // similar spirit to Jellyfin’s longer HLS ping window
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let mut to_kill = Vec::new();
            {
                let map = self.inner.lock().await;
                for (sid, job) in map.iter() {
                    if job.last_ping.elapsed() > timeout {
                        to_kill.push(sid.clone());
                    }
                }
            }

            for sid in to_kill {
                let mut map = self.inner.lock().await;
                if let Some(mut job) = map.remove(&sid) {
                    let _ = job.child.kill().await;
                    // TODO: delete partial segments/playlists for sid
                }
            }
        }
    }
}
```

---

## Playback speed

Most playback speed control is client-side (HTML5 `playbackRate`, native players, etc.). Server-side speed control is possible only by transcoding with `atempo` + `setpts`, which is expensive and often undesirable. Treat it as a client feature unless you have a very specific reason.

---

# Reference links

These are the main primary sources used when writing this deep dive:

- Jellyfin OpenAPI spec (stable):  
  https://api.jellyfin.org/openapi/jellyfin-openapi-stable.json

- Jellyfin server source (raw GitHub):
  - Stream decision engine:  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/MediaBrowser.Model/Dlna/StreamBuilder.cs
  - HLS controller:  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/Jellyfin.Api/Controllers/DynamicHlsController.cs
  - Universal audio streaming controller:  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/Jellyfin.Api/Controllers/UniversalAudioController.cs
  - FFmpeg/ffprobe capability handling:  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/MediaBrowser.MediaEncoding/Encoder/MediaEncoder.cs
  - Transcoding job manager (ffmpeg process supervisor):  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/MediaBrowser.MediaEncoding/Transcoding/TranscodeManager.cs
  - Segment deletion/cleanup logic:  
    https://raw.githubusercontent.com/jellyfin/jellyfin/master/MediaBrowser.Controller/MediaEncoding/TranscodingSegmentCleaner.cs

- Jellyfin docs:
  - Configuration / paths:  
    https://jellyfin.org/docs/general/administration/configuration/
  - Metadata:  
    https://jellyfin.org/docs/general/server/media/metadata/
  - NFO metadata:  
    https://jellyfin.org/docs/general/server/media/nfo/
