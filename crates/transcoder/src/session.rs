use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
    _permit: Option<OwnedSemaphorePermit>,
    child: Option<Child>,
}

impl TranscodeSession {
    pub fn ping(&mut self) {
        self.last_ping = Instant::now();
    }

    fn mark_finished(&mut self) {
        self.child = None;
        self._permit = None;
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
    create_failure_window: StdMutex<VecDeque<Instant>>,
}

/// How an HLS session should treat the source streams.
///
/// `Remux` copies the elementary streams into MPEG-TS instead of re-encoding,
/// which is only valid when the codecs are already browser/TS-compatible and no
/// resolution change is requested. The audio variant records whether the source
/// audio can also be copied (already AAC) or must be re-encoded to AAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemuxPlan {
    /// Fully re-encode video (and audio) — the default transcode path.
    #[default]
    Transcode,
    /// Copy H.264 video into TS; copy audio when already AAC, else encode AAC.
    Remux { copy_audio: bool },
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
    remux: RemuxPlan,
    /// Probed source framerate, used only to size the software-encode GOP so each
    /// HLS segment holds exactly one keyframe. `None` falls back to a sane default.
    source_framerate: Option<f64>,
}

const FFMPEG_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FFMPEG_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const FFMPEG_LOG_TAIL_LINES: usize = 8;

enum FfmpegStartupStatus {
    Running,
    Exited(std::process::ExitStatus),
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
    #[allow(clippy::too_many_arguments)]
    pub async fn create_session(
        &self,
        input_path: PathBuf,
        start_time_secs: Option<f64>,
        target_height: Option<u32>,
        video_codec_override: Option<&str>,
        remux: RemuxPlan,
        source_framerate: Option<f64>,
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
            record_transcode_failure(&self.metrics);
            return Err(err.into());
        }

        let child = match self
            .spawn_session_ffmpeg_with_fallback(
                &session_id,
                &input_path,
                &output_dir,
                start_time_secs,
                target_height,
                video_codec_override,
                remux,
                source_framerate,
            )
            .await
        {
            Ok(child) => child,
            Err(err) => {
                self.metrics
                    .create_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                record_transcode_failure(&self.metrics);
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
            _permit: Some(permit),
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

    #[allow(clippy::too_many_arguments)]
    async fn spawn_session_ffmpeg_with_fallback(
        &self,
        session_id: &str,
        input_path: &Path,
        output_dir: &Path,
        start_time_secs: Option<f64>,
        target_height: Option<u32>,
        video_codec_override: Option<&str>,
        remux: RemuxPlan,
        source_framerate: Option<f64>,
    ) -> Result<Child, TranscodeError> {
        // Remux copies streams and never touches a hardware encoder, so skip the
        // HW path entirely; there is nothing to fall back from.
        let configured_hw = match remux {
            RemuxPlan::Remux { .. } => None,
            RemuxPlan::Transcode => self.config.hw_accel.as_ref(),
        };
        let mut child = spawn_ffmpeg(SpawnFfmpegOptions {
            ffmpeg_path: &self.config.ffmpeg_path,
            input: input_path,
            output_dir,
            segment_secs: self.config.segment_secs,
            start_time: start_time_secs,
            target_height,
            video_codec_override,
            hw_accel: configured_hw,
            hw_device_path: self.config.hw_device_path.as_deref(),
            remux,
            source_framerate,
        })
        .await?;

        match wait_for_ffmpeg_startup(&mut child, &output_dir.join("master.m3u8")).await? {
            FfmpegStartupStatus::Running => return Ok(child),
            FfmpegStartupStatus::Exited(status) => {
                let startup_tail = read_ffmpeg_log_tail(output_dir, FFMPEG_LOG_TAIL_LINES);
                if let Some(hw) = configured_hw {
                    warn!(
                        session_id = %session_id,
                        ?hw,
                        ?status,
                        ffmpeg_log_tail = startup_tail.as_deref().unwrap_or(""),
                        "HLS ffmpeg exited during startup; retrying with software fallback"
                    );
                } else {
                    return Err(build_startup_error(status, startup_tail.as_deref(), false));
                }
            }
        }

        reset_output_dir(output_dir).await?;
        let mut fallback_child = spawn_ffmpeg(SpawnFfmpegOptions {
            ffmpeg_path: &self.config.ffmpeg_path,
            input: input_path,
            output_dir,
            segment_secs: self.config.segment_secs,
            start_time: start_time_secs,
            target_height,
            // Fallback should prioritize compatibility over preserving optional overrides.
            video_codec_override: None,
            hw_accel: None,
            hw_device_path: None,
            // Only the HW encode path can fail at startup; remux is already
            // HW-free, so this branch is reached with remux == Transcode. Keep
            // the plan as-is for correctness if that ever changes.
            remux,
            source_framerate,
        })
        .await?;

        match wait_for_ffmpeg_startup(&mut fallback_child, &output_dir.join("master.m3u8")).await? {
            FfmpegStartupStatus::Running => {
                info!(
                    session_id = %session_id,
                    "HLS software fallback started after hardware startup failure"
                );
                Ok(fallback_child)
            }
            FfmpegStartupStatus::Exited(status) => {
                let fallback_tail = read_ffmpeg_log_tail(output_dir, FFMPEG_LOG_TAIL_LINES);
                Err(build_startup_error(status, fallback_tail.as_deref(), true))
            }
        }
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
                continue;
            };

            match child.try_wait() {
                Ok(Some(status)) => {
                    session.mark_finished();
                    info!(
                        session_id = %id,
                        ?status,
                        "HLS session transcoder finished; retaining output until idle cleanup"
                    );
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

    pub fn create_failures_last_minute(&self) -> u64 {
        transcode_failure_window_counts(&self.metrics).0
    }

    pub fn create_failures_last_five_minutes(&self) -> u64 {
        transcode_failure_window_counts(&self.metrics).1
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

fn build_startup_error(
    status: std::process::ExitStatus,
    ffmpeg_log_tail: Option<&str>,
    was_fallback: bool,
) -> TranscodeError {
    let phase = if was_fallback {
        "software fallback"
    } else {
        "initial"
    };
    let mut message = format!("ffmpeg exited during {phase} startup with status {status}");
    if let Some(tail) = ffmpeg_log_tail.filter(|value| !value.is_empty()) {
        message.push_str("; ffmpeg log: ");
        message.push_str(tail);
    }
    TranscodeError::FfmpegFailed(message)
}

async fn wait_for_ffmpeg_startup(
    child: &mut Child,
    master_playlist_path: &Path,
) -> Result<FfmpegStartupStatus, TranscodeError> {
    let started = Instant::now();
    loop {
        if master_playlist_path.exists() {
            return Ok(FfmpegStartupStatus::Running);
        }

        match child.try_wait() {
            Ok(Some(status)) => return Ok(FfmpegStartupStatus::Exited(status)),
            Ok(None) => {}
            Err(error) => {
                return Err(TranscodeError::FfmpegFailed(format!(
                    "failed to poll ffmpeg startup: {error}"
                )));
            }
        }

        if started.elapsed() >= FFMPEG_STARTUP_TIMEOUT {
            return Ok(FfmpegStartupStatus::Running);
        }
        tokio::time::sleep(FFMPEG_STARTUP_POLL_INTERVAL).await;
    }
}

async fn reset_output_dir(output_dir: &Path) -> Result<(), TranscodeError> {
    if tokio::fs::try_exists(output_dir).await? {
        tokio::fs::remove_dir_all(output_dir).await?;
    }
    tokio::fs::create_dir_all(output_dir).await?;
    Ok(())
}

fn read_ffmpeg_log_tail(output_dir: &Path, max_lines: usize) -> Option<String> {
    let content = std::fs::read_to_string(output_dir.join("ffmpeg.log")).ok()?;
    let mut lines: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" | "))
    }
}

fn record_transcode_failure(metrics: &SessionMetrics) {
    let now = Instant::now();
    let mut timestamps = lock_or_recover(&metrics.create_failure_window);
    timestamps.push_back(now);
    while let Some(front) = timestamps.front() {
        if now.duration_since(*front).as_secs() > 5 * 60 {
            timestamps.pop_front();
        } else {
            break;
        }
    }
}

fn transcode_failure_window_counts(metrics: &SessionMetrics) -> (u64, u64) {
    let now = Instant::now();
    let mut timestamps = lock_or_recover(&metrics.create_failure_window);
    while let Some(front) = timestamps.front() {
        if now.duration_since(*front).as_secs() > 5 * 60 {
            timestamps.pop_front();
        } else {
            break;
        }
    }

    let mut last_minute = 0;
    let mut last_five_minutes = 0;
    for timestamp in timestamps.iter() {
        let elapsed = now.duration_since(*timestamp).as_secs();
        if elapsed <= 5 * 60 {
            last_five_minutes += 1;
        }
        if elapsed <= 60 {
            last_minute += 1;
        }
    }
    (last_minute, last_five_minutes)
}

fn lock_or_recover<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn recommended_hw_video_bitrate_kbps(target_height: Option<u32>) -> (u32, u32, u32) {
    let normalized_height = target_height.unwrap_or(1080);
    if normalized_height <= 360 {
        return (900, 1300, 1800);
    }
    if normalized_height <= 480 {
        return (1400, 2100, 2800);
    }
    if normalized_height <= 720 {
        return (2800, 4200, 5600);
    }
    if normalized_height <= 1080 {
        return (6000, 9000, 12000);
    }
    if normalized_height <= 1440 {
        return (10000, 15000, 20000);
    }
    (18000, 27000, 36000)
}

/// Framerate guess used only as a GOP fallback when the source framerate is
/// unknown. Capping the GOP at one keyframe per segment lets every segment start
/// on a keyframe (independent segments), which is required for clean HLS seeking
/// and faster first-frame decode.
const DEFAULT_FALLBACK_FPS: f64 = 30.0;

/// GOP length (frames) targeting exactly one keyframe per HLS segment.
/// `framerate` is the probed source fps; falls back to `DEFAULT_FALLBACK_FPS`
/// when ffprobe could not determine it. Clamped to at least 1.
fn keyframe_interval_frames(framerate: Option<f64>, segment_secs: u32) -> u32 {
    let fps = framerate
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_FALLBACK_FPS);
    let frames = (fps * segment_secs as f64).round();
    if frames.is_finite() && frames >= 1.0 {
        frames as u32
    } else {
        1
    }
}

/// Remux branch: copy elementary streams into MPEG-TS instead of re-encoding.
/// Only reached for browser-compatible H.264 video with AAC/MP3 audio, and never
/// when a resolution change is requested (a scale cannot be a stream copy).
fn build_remux_args(args: &mut Vec<String>, copy_audio: bool) {
    // Copy H.264 video untouched. `h264_mp4toannexb` rewrites MP4-style (AVCC,
    // length-prefixed) NAL units into Annex-B start codes, which MPEG-TS HLS
    // segments require; without it, copying H.264 from an MP4/MOV/MKV into TS
    // produces unplayable segments.
    args.extend([
        "-c:v".into(),
        "copy".into(),
        "-bsf:v".into(),
        "h264_mp4toannexb".into(),
    ]);

    if copy_audio {
        // Source audio is already AAC — copy it straight through.
        args.extend(["-c:a".into(), "copy".into()]);
    } else {
        // MP3 (or other allowed) audio: re-encode only the audio to AAC so the
        // TS/HLS stream is universally decodable, while still copying video.
        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "128k".into(),
            "-ac".into(),
            "2".into(),
            "-ar".into(),
            "48000".into(),
        ]);
    }

    // Segment on the source's existing keyframes. We cannot insert new IDR frames
    // when copying video, so force_key_frames is intentionally omitted; the HLS
    // muxer splits at the next keyframe at/after each `hls_time` boundary (set in
    // the shared HLS tail via -hls_time).
    args.extend(["-max_muxing_queue_size".into(), "4096".into()]);
}

/// Encode branch: full re-encode (software or hardware), tuned for low
/// time-to-first-frame. Carries the original codec/filter/bitrate selection plus
/// VID-4 fast-startup flags.
fn build_encode_args(
    args: &mut Vec<String>,
    options: &SpawnFfmpegOptions<'_>,
    active_hw_accel: Option<&HwAccel>,
) {
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
        _ => Some(match options.target_height {
            Some(height) => format!("scale=-2:min(ih\\,{height})"),
            None => "scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string(),
        }),
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
    if active_hw_accel.is_some() {
        let (target_kbps, maxrate_kbps, bufsize_kbps) =
            recommended_hw_video_bitrate_kbps(options.target_height);
        args.extend([
            "-b:v".into(),
            format!("{target_kbps}k"),
            "-maxrate".into(),
            format!("{maxrate_kbps}k"),
            "-bufsize".into(),
            format!("{bufsize_kbps}k"),
        ]);
    }

    // Hardware low-latency presets (VID-4). Only applied to the default encoder
    // selection; a caller-supplied video_codec_override is left untouched so we
    // never inject preset flags the override may not accept.
    if options.video_codec_override.is_none() {
        match active_hw_accel {
            Some(HwAccel::Nvenc) => {
                // p1 = fastest NVENC preset; `ll` = low-latency tuning. Lowest
                // encoder queue depth → fastest first segment.
                args.extend(["-preset".into(), "p1".into(), "-tune".into(), "ll".into()]);
            }
            Some(HwAccel::Qsv) => {
                // `veryfast` is the fastest h264_qsv preset and is universally
                // supported. We deliberately do NOT enable `-low_power` (LPM):
                // it only works on specific Intel GPUs and hard-fails encoder
                // init elsewhere, which would silently drop us to the software
                // fallback — a worse outcome than a slightly deeper queue.
                args.extend(["-preset".into(), "veryfast".into()]);
            }
            // VAAPI: no portable low-latency knob. `-low_power` is Intel-iGPU-only
            // and fails init on AMD/older Intel, so we leave the encoder defaults
            // intact rather than risk regressing the HW path into software.
            // VideoToolbox likewise approximates realtime already with no portable
            // low-latency flag. Leave both as-is.
            Some(HwAccel::Vaapi) | Some(HwAccel::VideoToolbox) | None => {}
        }
    }

    if matches!(active_hw_accel, Some(HwAccel::Vaapi)) {
        args.extend(["-profile:v".into(), "high".into()]);
    }
    if matches!(active_hw_accel, Some(HwAccel::Nvenc)) {
        // H.264 NVENC cannot emit 10-bit output. Force 8-bit pixel format so
        // Main10 HEVC sources transcode successfully instead of failing at init.
        args.extend(["-pix_fmt".into(), "yuv420p".into()]);
    }

    // Video encoding params for software encode
    if active_hw_accel.is_none() && options.video_codec_override.is_none() {
        let gop = keyframe_interval_frames(options.source_framerate, options.segment_secs);
        args.extend([
            "-preset".into(),
            "veryfast".into(),
            // zerolatency disables frame look-ahead/B-frame reordering buffers so
            // x264 emits frames immediately → much faster time-to-first-segment.
            "-tune".into(),
            "zerolatency".into(),
            "-crf".into(),
            "23".into(),
            // Cap GOP to one keyframe per segment and disable scene-cut keyframes
            // so segment boundaries are predictable and every segment is
            // independently decodable (works with -force_key_frames below).
            "-g".into(),
            gop.to_string(),
            "-sc_threshold".into(),
            "0".into(),
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
}

/// Build and spawn ffmpeg for HLS output.
async fn spawn_ffmpeg(options: SpawnFfmpegOptions<'_>) -> Result<Child, TranscodeError> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-y".into()];
    // Remux never engages a hardware encoder (it copies streams), so the caller
    // already nulled hw_accel for that plan. Bind it here for the encode branch.
    let active_hw_accel = match options.remux {
        RemuxPlan::Remux { .. } => None,
        RemuxPlan::Transcode => options.hw_accel,
    };

    // HW accel input flags (encode path only; copy never decodes on the GPU).
    if let Some(hw) = active_hw_accel {
        match hw {
            HwAccel::Nvenc => {
                // Decode in software for broader codec/profile compatibility, while
                // still using NVENC for accelerated H.264 encode.
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

    match options.remux {
        RemuxPlan::Remux { copy_audio } => {
            build_remux_args(&mut args, copy_audio);
        }
        RemuxPlan::Transcode => {
            build_encode_args(&mut args, &options, active_hw_accel);
        }
    }

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{
        RemuxPlan, SessionManager, TranscoderConfig, build_encode_args, build_remux_args,
        keyframe_interval_frames, recommended_hw_video_bitrate_kbps,
    };
    use crate::HwAccel;

    fn create_fake_ffmpeg_script(exit_delay_secs: u64) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rf_transcoder_fake_ffmpeg_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        #[cfg(unix)]
        {
            let script = dir.join("fake_ffmpeg.sh");
            let content = format!(
                r#"#!/usr/bin/env bash
set -euo pipefail

out="${{@: -1}}"
seg_pattern=""
for ((i=1; i<=$#; i++)); do
  arg="${{!i}}"
  if [[ "$arg" == "-hls_segment_filename" ]]; then
    j=$((i+1))
    seg_pattern="${{!j}}"
  fi
done

mkdir -p "$(dirname "$out")"
cat > "$out" <<'EOF'
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:4.0,
seg_00000.ts
EOF

if [[ -n "$seg_pattern" ]]; then
  seg="${{seg_pattern//%05d/00000}}"
  mkdir -p "$(dirname "$seg")"
  printf 'FAKE_TS' > "$seg"
fi

sleep {exit_delay_secs}
"#
            );
            std::fs::write(&script, content).unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
            script
        }

        #[cfg(windows)]
        {
            let ps_script = dir.join("fake_ffmpeg.ps1");
            let ps_content = format!(
                r#"
$args_list = $args
$out = $args_list[$args_list.Count - 1]
$seg_pattern = ""
for ($i = 0; $i -lt $args_list.Count; $i++) {{
    if ($args_list[$i] -eq "-hls_segment_filename" -and ($i + 1) -lt $args_list.Count) {{
        $seg_pattern = $args_list[$i + 1]
    }}
}}

$out_dir = Split-Path -Parent $out
if ($out_dir -and !(Test-Path $out_dir)) {{
    New-Item -ItemType Directory -Path $out_dir -Force | Out-Null
}}

$playlist = @"
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:4.0,
seg_00000.ts
"@
Set-Content -Path $out -Value $playlist -NoNewline

if ($seg_pattern -ne "") {{
    $seg = $seg_pattern -replace "%05d", "00000"
    $seg_dir = Split-Path -Parent $seg
    if ($seg_dir -and !(Test-Path $seg_dir)) {{
        New-Item -ItemType Directory -Path $seg_dir -Force | Out-Null
    }}
    Set-Content -Path $seg -Value "FAKE_TS" -NoNewline
}}

Start-Sleep -Seconds {exit_delay_secs}
"#
            );
            std::fs::write(&ps_script, ps_content).unwrap();

            let cmd_script = dir.join("fake_ffmpeg.cmd");
            let cmd_content = format!(
                "@echo off\r\npowershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\" %*\r\n",
                ps_script.to_string_lossy()
            );
            std::fs::write(&cmd_script, cmd_content).unwrap();
            cmd_script
        }
    }

    fn test_config(
        exit_delay_secs: u64,
        idle_timeout_secs: u64,
        max_concurrent: usize,
    ) -> TranscoderConfig {
        TranscoderConfig {
            ffmpeg_path: create_fake_ffmpeg_script(exit_delay_secs),
            ffprobe_path: PathBuf::from("ffprobe"),
            transcode_dir: std::env::temp_dir().join(format!(
                "rf_transcoder_test_output_{}",
                uuid::Uuid::new_v4()
            )),
            max_concurrent,
            idle_timeout_secs,
            ..Default::default()
        }
    }

    /// Locate the value following the first occurrence of `flag` in an arg list.
    fn arg_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    }

    fn encode_options(
        target_height: Option<u32>,
        framerate: Option<f64>,
    ) -> super::SpawnFfmpegOptions<'static> {
        super::SpawnFfmpegOptions {
            ffmpeg_path: std::path::Path::new("ffmpeg"),
            input: std::path::Path::new("in.mkv"),
            output_dir: std::path::Path::new("/tmp/out"),
            segment_secs: 2,
            start_time: None,
            target_height,
            video_codec_override: None,
            hw_accel: None,
            hw_device_path: None,
            remux: RemuxPlan::Transcode,
            source_framerate: framerate,
        }
    }

    #[test]
    fn keyframe_interval_uses_source_fps_and_segment_secs() {
        // 24 fps * 2s = 48 frames/GOP.
        assert_eq!(keyframe_interval_frames(Some(24.0), 2), 48);
        // Fractional fps rounds.
        assert_eq!(keyframe_interval_frames(Some(23.976), 2), 48);
        // Missing/invalid fps falls back to DEFAULT_FALLBACK_FPS (30) -> 30*2.
        assert_eq!(keyframe_interval_frames(None, 2), 60);
        assert_eq!(keyframe_interval_frames(Some(0.0), 2), 60);
        assert_eq!(keyframe_interval_frames(Some(f64::NAN), 2), 60);
        // Never zero even with a tiny segment/fps product.
        assert_eq!(keyframe_interval_frames(Some(0.1), 1), 1);
    }

    #[test]
    fn remux_copies_video_and_audio_when_aac() {
        let mut args = Vec::new();
        build_remux_args(&mut args, true);
        assert_eq!(arg_after(&args, "-c:v"), Some("copy"));
        // H.264 in MP4/MKV uses AVCC; TS needs Annex-B start codes.
        assert_eq!(arg_after(&args, "-bsf:v"), Some("h264_mp4toannexb"));
        assert_eq!(arg_after(&args, "-c:a"), Some("copy"));
        // Copying must not re-encode audio.
        assert!(!args.iter().any(|a| a == "-b:a"));
    }

    #[test]
    fn remux_reencodes_audio_to_aac_when_not_aac() {
        let mut args = Vec::new();
        build_remux_args(&mut args, false);
        assert_eq!(arg_after(&args, "-c:v"), Some("copy"));
        assert_eq!(arg_after(&args, "-bsf:v"), Some("h264_mp4toannexb"));
        assert_eq!(arg_after(&args, "-c:a"), Some("aac"));
        assert_eq!(arg_after(&args, "-b:a"), Some("128k"));
    }

    #[test]
    fn software_encode_has_fast_start_flags() {
        let opts = encode_options(None, Some(24.0));
        let mut args = Vec::new();
        build_encode_args(&mut args, &opts, None);
        assert_eq!(arg_after(&args, "-c:v"), Some("libx264"));
        assert_eq!(arg_after(&args, "-tune"), Some("zerolatency"));
        assert_eq!(arg_after(&args, "-sc_threshold"), Some("0"));
        // GOP = 24 * 2 = 48.
        assert_eq!(arg_after(&args, "-g"), Some("48"));
        // Per-segment IDR insertion is still requested for the encode path.
        assert!(args.iter().any(|a| a == "-force_key_frames"));
        // Audio re-encoded to AAC.
        assert_eq!(arg_after(&args, "-c:a"), Some("aac"));
    }

    #[test]
    fn nvenc_encode_uses_low_latency_preset_and_8bit() {
        let opts = encode_options(Some(1080), Some(24.0));
        let mut args = Vec::new();
        build_encode_args(&mut args, &opts, Some(&HwAccel::Nvenc));
        assert_eq!(arg_after(&args, "-c:v"), Some("h264_nvenc"));
        assert_eq!(arg_after(&args, "-preset"), Some("p1"));
        assert_eq!(arg_after(&args, "-tune"), Some("ll"));
        // NVENC must stay 8-bit so Main10 HEVC sources don't fail at init.
        assert_eq!(arg_after(&args, "-pix_fmt"), Some("yuv420p"));
        // zerolatency is an x264-only tune; must not leak into the HW path.
        assert!(!args.iter().any(|a| a == "zerolatency"));
    }

    #[test]
    fn hw_bitrate_ladder_defaults_to_1080_profile_for_auto() {
        assert_eq!(recommended_hw_video_bitrate_kbps(None), (6000, 9000, 12000));
    }

    #[test]
    fn hw_bitrate_ladder_scales_with_target_height() {
        assert_eq!(
            recommended_hw_video_bitrate_kbps(Some(720)),
            (2800, 4200, 5600)
        );
        assert_eq!(
            recommended_hw_video_bitrate_kbps(Some(2160)),
            (18000, 27000, 36000)
        );
    }

    #[tokio::test]
    async fn completed_sessions_keep_artifacts_until_idle_cleanup_and_release_slots() {
        let manager = SessionManager::new(test_config(1, 60, 1));

        let first_id = manager
            .create_session(
                PathBuf::from("movie-a.mkv"),
                None,
                None,
                None,
                RemuxPlan::Transcode,
                None,
                "user-1".to_string(),
                "file-a".to_string(),
            )
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(1200)).await;
        manager.cleanup_idle().await;

        let first_segment = manager
            .get_file_path(&first_id, "seg_00000.ts")
            .await
            .unwrap();
        assert!(
            first_segment.exists(),
            "finished session segment should remain readable"
        );
        assert_eq!(manager.active_count().await, 1);

        let second_id = manager
            .create_session(
                PathBuf::from("movie-b.mkv"),
                None,
                None,
                None,
                RemuxPlan::Transcode,
                None,
                "user-1".to_string(),
                "file-b".to_string(),
            )
            .await
            .unwrap();
        assert_ne!(first_id, second_id);
    }

    #[tokio::test]
    async fn completed_sessions_are_removed_after_idle_timeout() {
        let manager = SessionManager::new(test_config(1, 1, 1));

        let session_id = manager
            .create_session(
                PathBuf::from("movie-a.mkv"),
                None,
                None,
                None,
                RemuxPlan::Transcode,
                None,
                "user-1".to_string(),
                "file-a".to_string(),
            )
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(manager.ping(&session_id).await);
        manager.cleanup_idle().await;
        assert!(
            manager
                .get_file_path(&session_id, "seg_00000.ts")
                .await
                .is_ok()
        );

        tokio::time::sleep(Duration::from_millis(1200)).await;
        manager.cleanup_idle().await;
        assert!(
            manager
                .get_file_path(&session_id, "seg_00000.ts")
                .await
                .is_err()
        );
        assert_eq!(manager.active_count().await, 0);
    }
}
