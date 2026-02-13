use serde::{Deserialize, Serialize};

use crate::ffprobe::MediaInfo;

/// What a client can handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCaps {
    pub containers: Vec<String>,
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
    pub max_bitrate_kbps: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
}

impl Default for ClientCaps {
    fn default() -> Self {
        Self {
            containers: vec![
                "mp4".into(),
                "matroska".into(),
                "webm".into(),
                "mov".into(),
            ],
            video_codecs: vec!["h264".into(), "hevc".into(), "vp9".into(), "av1".into()],
            audio_codecs: vec![
                "aac".into(),
                "mp3".into(),
                "opus".into(),
                "ac3".into(),
                "eac3".into(),
                "flac".into(),
            ],
            max_bitrate_kbps: None,
            max_width: None,
            max_height: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayMethod {
    DirectPlay,
    Remux,
    Transcode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TranscodeReason {
    ContainerNotSupported,
    VideoCodecNotSupported,
    AudioCodecNotSupported,
    VideoBitrateTooHigh,
    VideoResolutionTooHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayDecision {
    pub method: PlayMethod,
    pub reasons: Vec<TranscodeReason>,
    pub transcode_video: bool,
    pub transcode_audio: bool,
}

/// Decide how to play a media file given client capabilities.
pub fn decide(media: &MediaInfo, caps: &ClientCaps) -> PlayDecision {
    let mut reasons = Vec::new();
    let mut transcode_video = false;
    let mut transcode_audio = false;

    // Check container
    let container_ok = caps.containers.iter().any(|c| media.container.contains(c));

    if !container_ok {
        reasons.push(TranscodeReason::ContainerNotSupported);
    }

    // Check video
    if let Some(ref v) = media.video {
        let codec_ok = caps
            .video_codecs
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&v.codec));
        if !codec_ok {
            reasons.push(TranscodeReason::VideoCodecNotSupported);
            transcode_video = true;
        }

        if let (Some(max_br), Some(vbr)) = (caps.max_bitrate_kbps, v.bitrate_kbps) {
            if vbr > max_br {
                reasons.push(TranscodeReason::VideoBitrateTooHigh);
                transcode_video = true;
            }
        }

        if let Some(max_w) = caps.max_width {
            if v.width > max_w {
                reasons.push(TranscodeReason::VideoResolutionTooHigh);
                transcode_video = true;
            }
        }
        if let Some(max_h) = caps.max_height {
            if v.height > max_h {
                if !reasons.contains(&TranscodeReason::VideoResolutionTooHigh) {
                    reasons.push(TranscodeReason::VideoResolutionTooHigh);
                }
                transcode_video = true;
            }
        }
    }

    // Check audio
    if let Some(a) = media.audio.first() {
        let codec_ok = caps
            .audio_codecs
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&a.codec));
        if !codec_ok {
            reasons.push(TranscodeReason::AudioCodecNotSupported);
            transcode_audio = true;
        }
    }

    let method = if reasons.is_empty() {
        PlayMethod::DirectPlay
    } else if !transcode_video && !transcode_audio {
        // Only container mismatch → remux
        PlayMethod::Remux
    } else {
        PlayMethod::Transcode
    };

    PlayDecision {
        method,
        reasons,
        transcode_video,
        transcode_audio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffprobe::{AudioStream, VideoStream};

    fn test_media() -> MediaInfo {
        MediaInfo {
            container: "matroska,webm".into(),
            duration_secs: 3600.0,
            bitrate_kbps: Some(5000),
            video: Some(VideoStream {
                index: 0,
                codec: "h264".into(),
                width: 1920,
                height: 1080,
                bitrate_kbps: Some(4000),
                framerate: Some(23.976),
            }),
            audio: vec![AudioStream {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                language: Some("eng".into()),
                title: None,
                is_default: true,
            }],
            subtitles: vec![],
        }
    }

    #[test]
    fn direct_play_when_all_compatible() {
        let media = test_media();
        let caps = ClientCaps::default();
        let d = decide(&media, &caps);
        assert_eq!(d.method, PlayMethod::DirectPlay);
        assert!(d.reasons.is_empty());
    }

    #[test]
    fn transcode_when_codec_unsupported() {
        let mut media = test_media();
        media.video.as_mut().unwrap().codec = "mpeg2video".into();
        let caps = ClientCaps::default();
        let d = decide(&media, &caps);
        assert_eq!(d.method, PlayMethod::Transcode);
        assert!(d.transcode_video);
        assert!(d.reasons.contains(&TranscodeReason::VideoCodecNotSupported));
    }

    #[test]
    fn remux_when_only_container_mismatch() {
        let mut media = test_media();
        media.container = "avi".into();
        let caps = ClientCaps::default();
        let d = decide(&media, &caps);
        assert_eq!(d.method, PlayMethod::Remux);
        assert!(d.reasons.contains(&TranscodeReason::ContainerNotSupported));
    }

    #[test]
    fn transcode_when_bitrate_too_high() {
        let media = test_media();
        let caps = ClientCaps {
            max_bitrate_kbps: Some(2000),
            ..ClientCaps::default()
        };
        let d = decide(&media, &caps);
        assert_eq!(d.method, PlayMethod::Transcode);
        assert!(d.reasons.contains(&TranscodeReason::VideoBitrateTooHigh));
    }

    #[test]
    fn transcode_when_resolution_too_high() {
        let media = test_media();
        let caps = ClientCaps {
            max_width: Some(1280),
            max_height: Some(720),
            ..ClientCaps::default()
        };
        let d = decide(&media, &caps);
        assert_eq!(d.method, PlayMethod::Transcode);
        assert!(d.reasons.contains(&TranscodeReason::VideoResolutionTooHigh));
    }
}
