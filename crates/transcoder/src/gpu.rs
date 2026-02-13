//! GPU hardware acceleration detection.
//!
//! Probes for available encoders by running `ffmpeg -encoders` and parsing output.

use std::path::Path;

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
        } else if self.qsv {
            Some(HwAccel::Qsv)
        } else if self.vaapi {
            Some(HwAccel::Vaapi)
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
        assert!(matches!(caps.best(), Some(HwAccel::Qsv)));

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
