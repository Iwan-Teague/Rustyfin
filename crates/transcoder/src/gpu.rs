//! GPU hardware acceleration detection.
//!
//! Probes for available encoders by running `ffmpeg -encoders` and parsing output.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::info;

use crate::HwAccel;

/// Detected GPU capabilities.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuCapabilities {
    pub nvenc: bool,
    pub vaapi: bool,
    pub qsv: bool,
    pub videotoolbox: bool,
}

impl GpuCapabilities {
    /// Pick the best available HW accelerator, or None for CPU.
    pub fn best(&self) -> Option<HwAccel> {
        if self.nvenc {
            Some(HwAccel::Nvenc)
        } else if self.vaapi {
            Some(HwAccel::Vaapi)
        } else if self.qsv {
            Some(HwAccel::Qsv)
        } else if self.videotoolbox {
            Some(HwAccel::VideoToolbox)
        } else {
            None
        }
    }
}

/// Detect available hardware encoders by querying ffmpeg.
pub async fn detect(ffmpeg_path: &Path) -> GpuCapabilities {
    let encoders = match get_encoders(ffmpeg_path).await {
        Ok(s) => s,
        Err(e) => {
            info!(error = %e, "could not query ffmpeg encoders, assuming CPU-only");
            return GpuCapabilities {
                nvenc: false,
                vaapi: false,
                qsv: false,
                videotoolbox: false,
            };
        }
    };

    let caps = GpuCapabilities {
        nvenc: encoders.contains("h264_nvenc"),
        vaapi: encoders.contains("h264_vaapi"),
        qsv: encoders.contains("h264_qsv"),
        videotoolbox: encoders.contains("h264_videotoolbox"),
    };

    info!(?caps, "GPU encoder detection complete");
    caps
}

async fn get_encoders(ffmpeg_path: &Path) -> Result<String, String> {
    let output = tokio::process::Command::new(ffmpeg_path)
        .args(["-hide_banner", "-encoders"])
        .output()
        .await
        .map_err(|e| format!("spawn ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err("ffmpeg -encoders failed".into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Check if a VAAPI device exists (Linux).
pub fn vaapi_device_exists() -> bool {
    Path::new("/dev/dri/renderD128").exists()
}

/// Check if an Intel render node is available for QSV.
pub fn qsv_device_exists() -> bool {
    let dri_root = Path::new("/sys/class/drm");
    let entries = match std::fs::read_dir(dri_root) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("renderD") {
            continue;
        }
        let vendor_path = entry.path().join("device").join("vendor");
        let Ok(vendor) = std::fs::read_to_string(vendor_path) else {
            continue;
        };
        if vendor.trim().eq_ignore_ascii_case("0x8086") {
            return true;
        }
    }
    false
}

fn list_render_nodes() -> Vec<PathBuf> {
    let mut nodes = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return nodes;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("renderD") {
            nodes.push(entry.path());
        }
    }
    nodes.sort();
    nodes
}

async fn run_probe_command(ffmpeg_path: &Path, args: &[&str]) -> Result<(), String> {
    let fut = tokio::process::Command::new(ffmpeg_path)
        .args(args)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(8), fut)
        .await
        .map_err(|_| "probe timed out".to_string())?
        .map_err(|e| format!("spawn ffmpeg probe: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!("ffmpeg probe failed with status {}", output.status))
    } else {
        Err(stderr)
    }
}

async fn probe_vaapi_with_device(ffmpeg_path: &Path, device: &Path) -> Result<(), String> {
    let device = device.to_string_lossy().to_string();
    run_probe_command(
        ffmpeg_path,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-vaapi_device",
            &device,
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x72:rate=1",
            "-frames:v",
            "1",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            "h264_vaapi",
            "-an",
            "-f",
            "null",
            "-",
        ],
    )
    .await
}

async fn probe_qsv_with_device(ffmpeg_path: &Path, device: &Path) -> Result<(), String> {
    let device = device.to_string_lossy().to_string();
    let init_hw = format!("qsv=hw:{device}");
    run_probe_command(
        ffmpeg_path,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-init_hw_device",
            &init_hw,
            "-filter_hw_device",
            "hw",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x72:rate=1",
            "-frames:v",
            "1",
            "-vf",
            "format=nv12,hwupload=extra_hw_frames=64",
            "-c:v",
            "h264_qsv",
            "-an",
            "-f",
            "null",
            "-",
        ],
    )
    .await
}

/// Probe a hardware acceleration mode in the current runtime.
///
/// Returns:
/// - `Ok(Some(path))` for accelerators that require a render node path.
/// - `Ok(None)` for accelerators without a device path.
/// - `Err(reason)` if the mode is unavailable at runtime.
pub async fn probe_runtime(ffmpeg_path: &Path, accel: &HwAccel) -> Result<Option<PathBuf>, String> {
    match accel {
        HwAccel::Nvenc => {
            run_probe_command(
                ffmpeg_path,
                &[
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=128x72:rate=1",
                    "-frames:v",
                    "1",
                    "-c:v",
                    "h264_nvenc",
                    "-an",
                    "-f",
                    "null",
                    "-",
                ],
            )
            .await?;
            Ok(None)
        }
        HwAccel::Vaapi => {
            let nodes = list_render_nodes();
            if nodes.is_empty() {
                return Err("no /dev/dri/renderD* nodes found".into());
            }
            let mut errors = Vec::new();
            for node in nodes {
                match probe_vaapi_with_device(ffmpeg_path, &node).await {
                    Ok(()) => return Ok(Some(node)),
                    Err(err) => errors.push(format!("{}: {err}", node.display())),
                }
            }
            Err(format!(
                "no usable VAAPI render node; probes failed: {}",
                errors.join(" | ")
            ))
        }
        HwAccel::Qsv => {
            let nodes = list_render_nodes();
            if nodes.is_empty() {
                return Err("no /dev/dri/renderD* nodes found".into());
            }
            let mut errors = Vec::new();
            for node in nodes {
                match probe_qsv_with_device(ffmpeg_path, &node).await {
                    Ok(()) => return Ok(Some(node)),
                    Err(err) => errors.push(format!("{}: {err}", node.display())),
                }
            }
            Err(format!(
                "no usable QSV render node; probes failed: {}",
                errors.join(" | ")
            ))
        }
        HwAccel::VideoToolbox => {
            run_probe_command(
                ffmpeg_path,
                &[
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=128x72:rate=1",
                    "-frames:v",
                    "1",
                    "-c:v",
                    "h264_videotoolbox",
                    "-an",
                    "-f",
                    "null",
                    "-",
                ],
            )
            .await?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_accelerator_preference() {
        let caps = GpuCapabilities {
            nvenc: true,
            vaapi: true,
            qsv: false,
            videotoolbox: false,
        };
        assert!(matches!(caps.best(), Some(HwAccel::Nvenc)));

        let caps = GpuCapabilities {
            nvenc: false,
            vaapi: true,
            qsv: true,
            videotoolbox: false,
        };
        assert!(matches!(caps.best(), Some(HwAccel::Vaapi)));

        let caps = GpuCapabilities {
            nvenc: false,
            vaapi: true,
            qsv: false,
            videotoolbox: false,
        };
        assert!(matches!(caps.best(), Some(HwAccel::Vaapi)));

        let caps = GpuCapabilities {
            nvenc: false,
            vaapi: false,
            qsv: false,
            videotoolbox: false,
        };
        assert!(caps.best().is_none());
    }
}
