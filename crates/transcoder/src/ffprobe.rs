use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::TranscodeError;

/// Media information extracted via ffprobe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: String,
    pub duration_secs: f64,
    pub bitrate_kbps: Option<u32>,
    pub video: Option<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStream {
    pub index: u32,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: Option<u32>,
    pub framerate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: u32,
    pub codec: String,
    pub channels: u32,
    pub language: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStream {
    pub index: u32,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub is_forced: bool,
    #[serde(default)]
    pub is_default: bool,
}

/// Run ffprobe on a file and parse the JSON output.
pub async fn probe(ffprobe_path: &Path, file: &Path) -> Result<MediaInfo, TranscodeError> {
    let output = tokio::process::Command::new(ffprobe_path)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(file)
        .output()
        .await
        .map_err(|e| TranscodeError::ProbeFailed(format!("spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(TranscodeError::ProbeFailed(stderr.into_owned()));
    }

    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| TranscodeError::ProbeFailed(format!("parse JSON: {e}")))?;

    parse_probe_output(&raw)
}

fn parse_probe_output(raw: &serde_json::Value) -> Result<MediaInfo, TranscodeError> {
    let format = raw
        .get("format")
        .ok_or_else(|| TranscodeError::ProbeFailed("missing 'format'".into()))?;

    let container = format
        .get("format_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let duration_secs: f64 = format
        .get("duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let bitrate_kbps: Option<u32> = format
        .get("bit_rate")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|b| (b / 1000) as u32);

    let streams = raw
        .get("streams")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut video = None;
    let mut audio = Vec::new();
    let mut subtitles = Vec::new();

    for s in &streams {
        let codec_type = s.get("codec_type").and_then(|v| v.as_str()).unwrap_or("");
        let index = s.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let codec = s
            .get("codec_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let tags = s.get("tags");
        let language = tags
            .and_then(|t| t.get("language"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let title = tags
            .and_then(|t| t.get("title"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let disposition = s.get("disposition");
        let is_default = disposition
            .and_then(|d| d.get("default"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            == 1;
        let is_forced = disposition
            .and_then(|d| d.get("forced"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            == 1;

        match codec_type {
            "video" => {
                if video.is_none() {
                    let width = s.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let height = s.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let stream_bitrate = s
                        .get("bit_rate")
                        .and_then(|v| v.as_str())
                        .and_then(|b| b.parse::<u64>().ok())
                        .map(|b| (b / 1000) as u32);
                    let framerate = s
                        .get("r_frame_rate")
                        .and_then(|v| v.as_str())
                        .and_then(|fr| parse_fraction(fr));

                    video = Some(VideoStream {
                        index,
                        codec,
                        width,
                        height,
                        bitrate_kbps: stream_bitrate,
                        framerate,
                    });
                }
            }
            "audio" => {
                let channels = s.get("channels").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
                audio.push(AudioStream {
                    index,
                    codec,
                    channels,
                    language,
                    title,
                    is_default,
                });
            }
            "subtitle" => {
                subtitles.push(SubtitleStream {
                    index,
                    codec,
                    language,
                    title,
                    is_forced,
                    is_default,
                });
            }
            _ => {}
        }
    }

    Ok(MediaInfo {
        container,
        duration_secs,
        bitrate_kbps,
        video,
        audio,
        subtitles,
    })
}

fn parse_fraction(s: &str) -> Option<f64> {
    if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().ok()?;
        let d: f64 = den.parse().ok()?;
        if d > 0.0 { Some(n / d) } else { None }
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_probe_json() {
        let json = serde_json::json!({
            "format": {
                "format_name": "matroska,webm",
                "duration": "7200.123",
                "bit_rate": "5000000"
            },
            "streams": [
                {
                    "index": 0,
                    "codec_type": "video",
                    "codec_name": "h264",
                    "width": 1920,
                    "height": 1080,
                    "r_frame_rate": "24000/1001",
                    "disposition": { "default": 1, "forced": 0 }
                },
                {
                    "index": 1,
                    "codec_type": "audio",
                    "codec_name": "aac",
                    "channels": 6,
                    "tags": { "language": "eng", "title": "Surround" },
                    "disposition": { "default": 1, "forced": 0 }
                },
                {
                    "index": 2,
                    "codec_type": "subtitle",
                    "codec_name": "subrip",
                    "tags": { "language": "eng" },
                    "disposition": { "default": 0, "forced": 0 }
                },
                {
                    "index": 3,
                    "codec_type": "subtitle",
                    "codec_name": "hdmv_pgs_subtitle",
                    "tags": { "language": "eng" },
                    "disposition": { "default": 0, "forced": 1 }
                }
            ]
        });

        let info = parse_probe_output(&json).unwrap();
        assert_eq!(info.container, "matroska,webm");
        assert!((info.duration_secs - 7200.123).abs() < 0.001);
        assert_eq!(info.bitrate_kbps, Some(5000));

        let v = info.video.unwrap();
        assert_eq!(v.codec, "h264");
        assert_eq!(v.width, 1920);
        assert_eq!(v.height, 1080);
        assert!((v.framerate.unwrap() - 23.976).abs() < 0.01);

        assert_eq!(info.audio.len(), 1);
        assert_eq!(info.audio[0].codec, "aac");
        assert_eq!(info.audio[0].channels, 6);
        assert_eq!(info.audio[0].language.as_deref(), Some("eng"));
        assert!(info.audio[0].is_default);

        assert_eq!(info.subtitles.len(), 2);
        assert_eq!(info.subtitles[0].codec, "subrip");
        assert!(!info.subtitles[0].is_forced);
        assert_eq!(info.subtitles[1].codec, "hdmv_pgs_subtitle");
        assert!(info.subtitles[1].is_forced);
    }

    #[test]
    fn parse_fraction_works() {
        assert!((parse_fraction("24000/1001").unwrap() - 23.976).abs() < 0.01);
        assert!((parse_fraction("30").unwrap() - 30.0).abs() < 0.001);
        assert!(parse_fraction("0/0").is_none());
    }
}
