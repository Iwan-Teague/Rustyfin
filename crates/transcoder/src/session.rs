use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokio::process::Child;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tracing::{info, warn};

use crate::{HwAccel, TranscodeError, TranscoderConfig};

#[derive(Debug, Clone)]
pub struct SessionAccess {
    pub owner_user_id: String,
    pub file_id: String,
}

/// An active HLS transcode session.
pub struct TranscodeSession {
    pub id: String,
    pub input_path: PathBuf,
    pub file_id: String,
    pub owner_user_id: String,
    pub output_dir: PathBuf,
    pub started_at: Instant,
    pub last_ping: Instant,
    _permit: OwnedSemaphorePermit,
    child: Option<Child>,
}

impl TranscodeSession {
    pub fn ping(&mut self) {
        self.last_ping = Instant::now();
    }

    /// Check if master.m3u8 exists (ffmpeg started writing).
    pub fn master_playlist_path(&self) -> PathBuf {
        self.output_dir.join("master.m3u8")
    }

    /// Check if a segment file exists.
    pub fn segment_path(&self, filename: &str) -> PathBuf {
        self.output_dir.join(filename)
    }

    pub fn is_idle(&self, timeout_secs: u64) -> bool {
        self.last_ping.elapsed().as_secs() >= timeout_secs
    }
}

impl Drop for TranscodeSession {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            // Best-effort kill on drop
            let _ = child.start_kill();
        }
    }
}

/// Manages all active transcode sessions.
pub struct SessionManager {
    config: TranscoderConfig,
    sessions: Arc<Mutex<HashMap<String, TranscodeSession>>>,
    semaphore: Arc<Semaphore>,
    metrics: Arc<SessionMetrics>,
}

#[derive(Default)]
struct SessionMetrics {
    created_total: AtomicU64,
    create_failures_total: AtomicU64,
    cleaned_total: AtomicU64,
}

struct SpawnFfmpegOptions<'a> {
    ffmpeg_path: &'a Path,
    input: &'a Path,
    output_dir: &'a Path,
    segment_secs: u32,
    start_time: Option<f64>,
    target_height: Option<u32>,
    video_codec_override: Option<&'a str>,
    hw_accel: Option<&'a HwAccel>,
    hw_device_path: Option<&'a Path>,
}

impl SessionManager {
    pub fn new(config: TranscoderConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            semaphore,
            metrics: Arc::new(SessionMetrics::default()),
        }
    }

    /// Create a new HLS transcode session. Returns the session ID.
    /// Blocks if max concurrent transcodes are running.
    pub async fn create_session(
        &self,
        input_path: PathBuf,
        start_time_secs: Option<f64>,
        target_height: Option<u32>,
        video_codec_override: Option<&str>,
        owner_user_id: String,
        file_id: String,
    ) -> Result<String, TranscodeError> {
        // Free slots from any completed ffmpeg processes before taking a permit.
        self.reap_finished_sessions().await;

        // Replace prior sessions for the same user/file to avoid slot leaks when
        // clients switch sources/modes without stopping the old session.
        self.stop_sessions_for_owner_file(&owner_user_id, &file_id)
            .await;

        // Hold a permit for the full session lifetime to enforce max concurrency.
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| TranscodeError::MaxTranscodesReached(self.config.max_concurrent))?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let output_dir = self.config.transcode_dir.join(&session_id);
        if let Err(err) = tokio::fs::create_dir_all(&output_dir).await {
            self.metrics
                .create_failures_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(err.into());
        }

        let child = match spawn_ffmpeg(SpawnFfmpegOptions {
            ffmpeg_path: &self.config.ffmpeg_path,
            input: &input_path,
            output_dir: &output_dir,
            segment_secs: self.config.segment_secs,
            start_time: start_time_secs,
            target_height,
            video_codec_override,
            hw_accel: self.config.hw_accel.as_ref(),
            hw_device_path: self.config.hw_device_path.as_deref(),
        })
        .await
        {
            Ok(child) => child,
            Err(err) => {
                self.metrics
                    .create_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                let _ = tokio::fs::remove_dir_all(&output_dir).await;
                return Err(err);
            }
        };

        let session = TranscodeSession {
            id: session_id.clone(),
            input_path,
            file_id,
            owner_user_id,
            output_dir,
            started_at: Instant::now(),
            last_ping: Instant::now(),
            _permit: permit,
            child: Some(child),
        };

        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        self.metrics.created_total.fetch_add(1, Ordering::Relaxed);

        info!(session_id = %session_id, "HLS transcode session created");
        Ok(session_id)
    }

    async fn stop_sessions_for_owner_file(&self, owner_user_id: &str, file_id: &str) {
        let removed_sessions = {
            let mut sessions = self.sessions.lock().await;
            let ids_to_remove: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| s.owner_user_id == owner_user_id && s.file_id == file_id)
                .map(|(id, _)| id.clone())
                .collect();

            ids_to_remove
                .into_iter()
                .filter_map(|id| sessions.remove(&id).map(|session| (id, session)))
                .collect::<Vec<_>>()
        };

        for (id, session) in removed_sessions {
            cleanup_removed_session(
                id,
                session,
                Arc::clone(&self.metrics),
                "replaced existing HLS session for same user/file",
            )
            .await;
        }
    }

    async fn reap_finished_sessions(&self) {
        let mut sessions = self.sessions.lock().await;
        let mut ids_to_remove = Vec::new();

        for (id, session) in sessions.iter_mut() {
            let Some(child) = session.child.as_mut() else {
                ids_to_remove.push(id.clone());
                continue;
            };

            match child.try_wait() {
                Ok(Some(status)) => {
                    info!(session_id = %id, ?status, "reaping finished HLS session");
                    ids_to_remove.push(id.clone());
                }
                Ok(None) => {
                    // Still running.
                }
                Err(e) => {
                    warn!(session_id = %id, error = %e, "failed to poll HLS session process; reaping");
                    ids_to_remove.push(id.clone());
                }
            }
        }

        let removed_sessions = ids_to_remove
            .into_iter()
            .filter_map(|id| sessions.remove(&id).map(|session| (id, session)))
            .collect::<Vec<_>>();
        drop(sessions);

        for (id, session) in removed_sessions {
            cleanup_removed_session(
                id,
                session,
                Arc::clone(&self.metrics),
                "reaped finished HLS session",
            )
            .await;
        }
    }

    /// Ping a session (update last_ping) and return if it exists.
    pub async fn ping(&self, session_id: &str) -> bool {
        if let Some(session) = self.sessions.lock().await.get_mut(session_id) {
            session.ping();
            true
        } else {
            false
        }
    }

    /// Get the path to a file within a session's output dir.
    pub async fn get_file_path(
        &self,
        session_id: &str,
        filename: &str,
    ) -> Result<PathBuf, TranscodeError> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| TranscodeError::SessionNotFound(session_id.into()))?;

        let path = session.segment_path(filename);
        Ok(path)
    }

    /// Stop and clean up a session.
    pub async fn stop_session(&self, session_id: &str) -> Result<(), TranscodeError> {
        let removed_session = self.sessions.lock().await.remove(session_id);
        if let Some(session) = removed_session {
            cleanup_removed_session(
                session_id.to_string(),
                session,
                Arc::clone(&self.metrics),
                "HLS session stopped and cleaned up",
            )
            .await;
            Ok(())
        } else {
            Err(TranscodeError::SessionNotFound(session_id.into()))
        }
    }

    /// Clean up idle sessions. Call this periodically.
    pub async fn cleanup_idle(&self) {
        self.reap_finished_sessions().await;
        let timeout = self.config.idle_timeout_secs;
        let idle_sessions = {
            let mut sessions = self.sessions.lock().await;
            let idle_ids: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| s.is_idle(timeout))
                .map(|(id, _)| id.clone())
                .collect();

            idle_ids
                .into_iter()
                .filter_map(|id| sessions.remove(&id).map(|session| (id, session)))
                .collect::<Vec<_>>()
        };

        for (id, session) in idle_sessions {
            cleanup_removed_session(
                id,
                session,
                Arc::clone(&self.metrics),
                "cleaned up idle HLS session",
            )
            .await;
        }
    }

    /// Get active session count.
    pub async fn active_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// List active session IDs.
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.lock().await.keys().cloned().collect()
    }

    pub fn created_total(&self) -> u64 {
        self.metrics.created_total.load(Ordering::Relaxed)
    }

    pub fn create_failures_total(&self) -> u64 {
        self.metrics.create_failures_total.load(Ordering::Relaxed)
    }

    pub fn cleaned_total(&self) -> u64 {
        self.metrics.cleaned_total.load(Ordering::Relaxed)
    }

    pub async fn get_session_access(&self, session_id: &str) -> Option<SessionAccess> {
        self.sessions
            .lock()
            .await
            .get(session_id)
            .map(|s| SessionAccess {
                owner_user_id: s.owner_user_id.clone(),
                file_id: s.file_id.clone(),
            })
    }

    pub fn ffmpeg_path(&self) -> &Path {
        &self.config.ffmpeg_path
    }

    pub fn ffprobe_path(&self) -> &Path {
        &self.config.ffprobe_path
    }

    pub fn hw_accel(&self) -> Option<&HwAccel> {
        self.config.hw_accel.as_ref()
    }

    pub fn hw_device_path(&self) -> Option<&Path> {
        self.config.hw_device_path.as_deref()
    }
}

async fn cleanup_removed_session(
    session_id: String,
    mut session: TranscodeSession,
    metrics: Arc<SessionMetrics>,
    success_message: &'static str,
) {
    if let Some(mut child) = session.child.take() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    match tokio::fs::try_exists(&session.output_dir).await {
        Ok(true) => {
            if let Err(e) = tokio::fs::remove_dir_all(&session.output_dir).await {
                warn!(session_id = %session_id, error = %e, "failed to clean transcode dir");
            }
        }
        Ok(false) => {}
        Err(e) => {
            warn!(
                session_id = %session_id,
                error = %e,
                "failed to check transcode dir before cleanup"
            );
        }
    }

    info!(session_id = %session_id, "{success_message}");
    metrics.cleaned_total.fetch_add(1, Ordering::Relaxed);
}

/// Build and spawn ffmpeg for HLS output.
async fn spawn_ffmpeg(options: SpawnFfmpegOptions<'_>) -> Result<Child, TranscodeError> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-y".into()];
    let active_hw_accel = options.hw_accel;

    // HW accel input flags
    if let Some(hw) = active_hw_accel {
        match hw {
            HwAccel::Nvenc => {
                args.extend(["-hwaccel".into(), "cuda".into()]);
            }
            HwAccel::Vaapi => {
                let device = options
                    .hw_device_path
                    .unwrap_or_else(|| Path::new("/dev/dri/renderD128"))
                    .to_string_lossy()
                    .into_owned();
                args.extend(["-vaapi_device".into(), device]);
            }
            HwAccel::Qsv => {
                args.extend(["-hwaccel".into(), "qsv".into()]);
                if let Some(device) = options.hw_device_path {
                    args.extend(["-qsv_device".into(), device.to_string_lossy().into_owned()]);
                }
            }
            HwAccel::VideoToolbox => {
                args.extend(["-hwaccel".into(), "videotoolbox".into()]);
            }
        }
    }

    // Seek
    if let Some(t) = options.start_time {
        args.extend(["-ss".into(), format!("{t:.3}")]);
    }

    // Input
    args.extend(["-i".into(), options.input.to_string_lossy().into_owned()]);

    // Select the primary video stream and the first audio stream (if present).
    // This avoids ambiguous/default stream selection behavior on some containers.
    args.extend([
        "-map".into(),
        "0:v:0?".into(),
        "-map".into(),
        "0:a:0?".into(),
        "-sn".into(),
        "-dn".into(),
    ]);

    // Filters / format conversion.
    let vf = match active_hw_accel {
        // For VAAPI, decode in software then upload to VAAPI. This is more robust
        // across 10-bit HDR sources than forcing vaapi decode/output.
        Some(HwAccel::Vaapi) => {
            let mut chain = vec!["format=nv12".to_string(), "hwupload".to_string()];
            if let Some(height) = options.target_height {
                chain.push(format!("scale_vaapi=w=-2:h={height}"));
            }
            Some(chain.join(","))
        }
        Some(HwAccel::Qsv) => options
            .target_height
            .map(|height| format!("vpp_qsv=w=-2:h={height}")),
        _ => options
            .target_height
            .map(|height| format!("scale=-2:min(ih\\,{height})")),
    };
    if let Some(filter) = vf {
        args.extend(["-vf".into(), filter]);
    }

    // Video codec
    let vcodec = if let Some(vc) = options.video_codec_override {
        vc.to_string()
    } else if let Some(hw) = active_hw_accel {
        match hw {
            HwAccel::Nvenc => "h264_nvenc".into(),
            HwAccel::Vaapi => "h264_vaapi".into(),
            HwAccel::Qsv => "h264_qsv".into(),
            HwAccel::VideoToolbox => "h264_videotoolbox".into(),
        }
    } else {
        "libx264".into()
    };

    args.extend(["-c:v".into(), vcodec]);
    if matches!(active_hw_accel, Some(HwAccel::Vaapi)) {
        args.extend(["-profile:v".into(), "high".into()]);
    }

    // Video encoding params for software encode
    if active_hw_accel.is_none() && options.video_codec_override.is_none() {
        args.extend([
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "23".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]);
    }

    // Audio: always AAC for HLS compatibility
    args.extend([
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        "48000".into(),
        "-max_muxing_queue_size".into(),
        "4096".into(),
        "-force_key_frames".into(),
        format!("expr:gte(t,n_forced*{})", options.segment_secs),
    ]);

    // HLS output
    let seg_pattern = options.output_dir.join("seg_%05d.ts");
    let master = options.output_dir.join("master.m3u8");

    args.extend([
        "-f".into(),
        "hls".into(),
        "-hls_time".into(),
        options.segment_secs.to_string(),
        "-hls_list_size".into(),
        "0".into(),
        "-hls_playlist_type".into(),
        "event".into(),
        "-hls_segment_filename".into(),
        seg_pattern.to_string_lossy().into_owned(),
        "-hls_flags".into(),
        "independent_segments+append_list".into(),
        master.to_string_lossy().into_owned(),
    ]);

    // Log file
    let log_path = options.output_dir.join("ffmpeg.log");

    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| TranscodeError::FfmpegFailed(format!("create log: {e}")))?;

    let child = tokio::process::Command::new(options.ffmpeg_path)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file))
        .spawn()
        .map_err(|e| TranscodeError::FfmpegFailed(format!("spawn: {e}")))?;

    info!(ffmpeg_path = ?options.ffmpeg_path, ?args, "spawned ffmpeg for HLS");
    Ok(child)
}
