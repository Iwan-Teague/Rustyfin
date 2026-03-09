'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type Hls from 'hls.js';
import type { ErrorData, LevelDetails, LevelLoadedData, LevelUpdatedData } from 'hls.js';
import { useParams } from 'next/navigation';
import { apiFetch, apiJson } from '@/lib/api';
import { clientErrorMessage } from '@/lib/errors';

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  hls_start_url: string;
  media_info_url: string;
  duration_ms?: number | null;
};

type PlaybackSession = {
  session_id: string;
  hls_url: string;
};

type MediaInfo = {
  duration_secs?: number;
  container?: string;
  video?: {
    codec?: string;
    width?: number;
    height?: number;
    bitrate_kbps?: number;
    framerate?: number;
  } | null;
  audio?: Array<{
    codec?: string;
    channels?: number;
  }>;
};

function formatClock(totalSeconds?: number): string {
  if (!Number.isFinite(totalSeconds) || !totalSeconds || totalSeconds < 0) return '0:00';
  const whole = Math.floor(totalSeconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const seconds = whole % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

function formatPlaybackRateLabel(rate: number): string {
  if (!Number.isFinite(rate) || rate <= 0) return '1x';
  return `${rate.toFixed(rate % 1 === 0 ? 0 : 2)}x`;
}

const HLS_QUALITY_OPTIONS: Array<{ value: 'auto' | number; label: string }> = [
  { value: 'auto', label: 'Auto (Original)' },
  { value: 2160, label: '2160p (4K)' },
  { value: 1440, label: '1440p' },
  { value: 1080, label: '1080p' },
  { value: 720, label: '720p' },
  { value: 480, label: '480p' },
  { value: 360, label: '360p' },
];

type VideoAudioState = {
  muted: boolean;
  volume: number;
};

function clampVideoVolume(value: number): number {
  if (!Number.isFinite(value)) return 1;
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

function readVideoAudioState(video: HTMLVideoElement): VideoAudioState {
  return {
    muted: video.muted,
    volume: clampVideoVolume(video.volume),
  };
}

function applyVideoAudioState(video: HTMLVideoElement, state: VideoAudioState): void {
  video.volume = clampVideoVolume(state.volume);
  video.muted = state.muted;
}

function shouldResumeFromCurrentSource(video: HTMLVideoElement, fileId: string): boolean {
  const src = `${video.currentSrc || video.src || ''}`.toLowerCase();
  if (!src) return false;
  return src.includes(`/stream/files/${fileId.toLowerCase()}`);
}

function resetVideoSourceForMse(video: HTMLVideoElement): void {
  // Clear any existing direct/native source before attaching hls.js.
  video.pause();
  video.removeAttribute('src');
  video.load();
}

function resolveHlsMediaSource(hls: unknown): MediaSource | null {
  const candidate = hls as {
    mediaSource?: unknown;
    bufferController?: { mediaSource?: unknown };
    coreComponents?: Array<{ mediaSource?: unknown }>;
  } | null;
  if (!candidate) return null;
  if (candidate.mediaSource instanceof MediaSource) return candidate.mediaSource;
  if (candidate.bufferController?.mediaSource instanceof MediaSource) {
    return candidate.bufferController.mediaSource;
  }
  if (Array.isArray(candidate.coreComponents)) {
    for (const component of candidate.coreComponents) {
      if (component?.mediaSource instanceof MediaSource) {
        return component.mediaSource;
      }
    }
  }
  return null;
}

function applyKnownDurationToHlsMediaSource(hls: unknown, durationSeconds: number): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const mediaSource = resolveHlsMediaSource(hls);
  if (!mediaSource || typeof mediaSource.duration !== 'number') return;
  try {
    if (mediaSource.readyState === 'open') {
      const current = mediaSource.duration;
      if (!Number.isFinite(current) || current < durationSeconds - 0.5) {
        mediaSource.duration = durationSeconds;
      }
    }
  } catch {
    // Some browsers may reject duration writes while updating source buffers.
  }
}

function forceKnownDurationInLevelDetails(
  levelData: LevelLoadedData | LevelUpdatedData | unknown,
  durationSeconds: number,
): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const details = (levelData as { details?: LevelDetails } | null)?.details;
  if (!details) return;
  const mutableDetails = details as LevelDetails & {
    totalduration: number;
    edge: number;
  };
  const currentTotal =
    typeof mutableDetails.totalduration === 'number' && Number.isFinite(mutableDetails.totalduration)
      ? mutableDetails.totalduration
      : 0;
  if (currentTotal < durationSeconds) {
    mutableDetails.totalduration = durationSeconds;
    mutableDetails.edge = durationSeconds;
  }
}

function installKnownDurationEnforcer(hls: unknown, durationSeconds: number): () => void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return () => {};
  }
  const tick = () => applyKnownDurationToHlsMediaSource(hls, durationSeconds);
  tick();
  const timer = window.setInterval(tick, 500);
  return () => window.clearInterval(timer);
}

type StartHlsOptions = {
  targetHeightOverride?: number | null;
  seekTimeOverrideSecs?: number;
};

type IconProps = {
  className?: string;
};

function PlayIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
      <path d="M8 6.25c0-1.02 1.12-1.65 2-1.13l8 4.75a1.3 1.3 0 0 1 0 2.26l-8 4.75A1.3 1.3 0 0 1 8 15.75v-9.5Z" />
    </svg>
  );
}

function PauseIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
      <path d="M7.5 5.75A1.25 1.25 0 0 1 8.75 4.5h1.5A1.25 1.25 0 0 1 11.5 5.75v12.5a1.25 1.25 0 0 1-1.25 1.25h-1.5A1.25 1.25 0 0 1 7.5 18.25V5.75Zm5 0a1.25 1.25 0 0 1 1.25-1.25h1.5A1.25 1.25 0 0 1 16.5 5.75v12.5a1.25 1.25 0 0 1-1.25 1.25h-1.5a1.25 1.25 0 0 1-1.25-1.25V5.75Z" />
    </svg>
  );
}

function Volume2Icon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M11 5 6.5 9H3.75A1.25 1.25 0 0 0 2.5 10.25v3.5A1.25 1.25 0 0 0 3.75 15H6.5L11 19V5Z" />
      <path d="M15 9.5a4 4 0 0 1 0 5" />
      <path d="M17.75 7.25a7 7 0 0 1 0 9.5" />
    </svg>
  );
}

function VolumeXIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M11 5 6.5 9H3.75A1.25 1.25 0 0 0 2.5 10.25v3.5A1.25 1.25 0 0 0 3.75 15H6.5L11 19V5Z" />
      <path d="m15.5 10.5 4 4" />
      <path d="m19.5 10.5-4 4" />
    </svg>
  );
}

function SettingsIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M10.4 3.2h3.2l.55 2.05c.35.1.68.24 1 .4l1.93-.94 2.26 2.26-.94 1.93c.16.32.3.65.4 1l2.05.55v3.2l-2.05.55c-.1.35-.24.68-.4 1l.94 1.93-2.26 2.26-1.93-.94c-.32.16-.65.3-1 .4l-.55 2.05h-3.2l-.55-2.05a6.4 6.4 0 0 1-1-.4l-1.93.94-2.26-2.26.94-1.93a6.4 6.4 0 0 1-.4-1L3.2 13.6v-3.2l2.05-.55c.1-.35.24-.68.4-1L4.7 6.92l2.26-2.26 1.93.94c.32-.16.65-.3 1-.4l.55-2.05Z" />
      <circle cx="12" cy="12" r="2.8" />
    </svg>
  );
}

function FullscreenIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8 3.75H5.75A2 2 0 0 0 3.75 5.75V8" />
      <path d="M16 3.75h2.25a2 2 0 0 1 2 2V8" />
      <path d="M20.25 16v2.25a2 2 0 0 1-2 2H16" />
      <path d="M8 20.25H5.75a2 2 0 0 1-2-2V16" />
    </svg>
  );
}

function FullscreenExitIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M9 3.75H5.75A2 2 0 0 0 3.75 5.75V9" />
      <path d="M15 3.75h3.25a2 2 0 0 1 2 2V9" />
      <path d="M20.25 15v3.25a2 2 0 0 1-2 2H15" />
      <path d="M3.75 15v3.25a2 2 0 0 0 2 2H9" />
      <path d="m9.5 9.5-3-3" />
      <path d="M14.5 9.5 17.5 6.5" />
      <path d="m9.5 14.5-3 3" />
      <path d="m14.5 14.5 3 3" />
    </svg>
  );
}

export default function PlayerPage() {
  const params = useParams();
  const id = params.id as string;
  const playerShellRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<Hls | null>(null);

  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [mediaInfo, setMediaInfo] = useState<MediaInfo | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loadingDescriptor, setLoadingDescriptor] = useState(true);
  const [startingHls, setStartingHls] = useState(false);
  const [hlsTargetHeight, setHlsTargetHeight] = useState<number | null>(null);
  const [timelineNowSecs, setTimelineNowSecs] = useState(0);
  const [timelineDurationSecs, setTimelineDurationSecs] = useState(0);
  const [hlsSessionStartOffsetSecs, setHlsSessionStartOffsetSecs] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isMuted, setIsMuted] = useState(false);
  const [playbackRate, setPlaybackRate] = useState(1);
  const [showPlayerSettings, setShowPlayerSettings] = useState(false);
  const autoStartedRef = useRef(false);
  const audioStateRef = useRef<VideoAudioState>({ muted: false, volume: 1 });

  const canStartPlayback = Boolean(descriptor?.file_id);

  const stopSession = useCallback(async (sid: string) => {
    await apiFetch(`/playback/sessions/${sid}/stop`, { method: 'POST' }).catch(() => {});
  }, []);

  const destroyHls = useCallback(() => {
    if (hlsRef.current) {
      const stopDurationEnforcer =
        (hlsRef.current as { __stopKnownDurationEnforcer?: () => void }).__stopKnownDurationEnforcer;
      if (typeof stopDurationEnforcer === 'function') {
        try {
          stopDurationEnforcer();
        } catch {
          // no-op
        }
      }
      try {
        hlsRef.current.destroy();
      } catch {
        // no-op
      }
      hlsRef.current = null;
    }
  }, []);

  const startHls = useCallback(async (options?: StartHlsOptions) => {
    if (!descriptor?.file_id) {
      setError('No media file is attached to this item. Rescan the library and try again.');
      return;
    }

    const video = videoRef.current;
    if (!video) {
      setError('Player is not ready yet.');
      return;
    }
    audioStateRef.current = readVideoAudioState(video);
    const preservedAudioState = audioStateRef.current;
    const shouldResumePlayback = !video.paused;
    const selectedTargetHeight =
      options?.targetHeightOverride !== undefined ? options.targetHeightOverride : hlsTargetHeight;
    const knownDurationSeconds = Math.max(
      descriptor.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
      mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
    );
    const canResumeFromCurrentSource = shouldResumeFromCurrentSource(video, descriptor.file_id);
    const currentTime =
      canResumeFromCurrentSource && Number.isFinite(video.currentTime) && video.currentTime >= 0
        ? video.currentTime
        : 0;
    const finiteDuration =
      canResumeFromCurrentSource && Number.isFinite(video.duration) && video.duration > 0;
    const explicitSeekTime =
      options?.seekTimeOverrideSecs !== undefined &&
      Number.isFinite(options.seekTimeOverrideSecs) &&
      options.seekTimeOverrideSecs >= 0
        ? options.seekTimeOverrideSecs
        : undefined;
    const startTimeSecs =
      explicitSeekTime !== undefined
        ? explicitSeekTime
        : finiteDuration &&
            currentTime > 0.5 &&
            currentTime < (video.duration as number) - 0.5
          ? currentTime
          : undefined;

    setStartingHls(true);
    setError('');
    try {
      if (sessionId) {
        await stopSession(sessionId);
        setSessionId(null);
      }

      const data = await apiJson<PlaybackSession>(descriptor.hls_start_url, {
        method: 'POST',
        body: JSON.stringify({
          file_id: descriptor.file_id,
          start_time_secs: startTimeSecs,
          target_height: selectedTargetHeight ?? undefined,
        }),
      });

      destroyHls();
      setSessionId(data.session_id);
      setHlsSessionStartOffsetSecs(startTimeSecs ?? 0);

      const Hls = (await import('hls.js')).default;
      const canNativeHls = video.canPlayType('application/vnd.apple.mpegurl') !== '';

      // Prefer hls.js when available; native HLS detection can be misleading on Chromium.
      if (Hls.isSupported()) {
        resetVideoSourceForMse(video);
        const hls = new Hls({
          enableWorker: true,
          lowLatencyMode: false,
          liveDurationInfinity: false,
          // Allow large prebuffer so paused/slow users can preload more than ~30s.
          maxBufferLength: 600,
          maxMaxBufferLength: 1200,
          maxBufferSize: 256 * 1000 * 1000,
          backBufferLength: 180,
          startFragPrefetch: true,
          manifestLoadingTimeOut: 45000,
          levelLoadingTimeOut: 45000,
          fragLoadingTimeOut: 45000,
          manifestLoadingMaxRetry: 10,
          levelLoadingMaxRetry: 10,
          fragLoadingMaxRetry: 8,
          manifestLoadingRetryDelay: 1000,
          levelLoadingRetryDelay: 1000,
          fragLoadingRetryDelay: 1000,
          manifestLoadingMaxRetryTimeout: 10000,
          levelLoadingMaxRetryTimeout: 10000,
          fragLoadingMaxRetryTimeout: 8000,
        });
        const stopDurationEnforcer = installKnownDurationEnforcer(hls, knownDurationSeconds);
        (hls as { __stopKnownDurationEnforcer?: () => void }).__stopKnownDurationEnforcer =
          stopDurationEnforcer;
        hlsRef.current = hls;
        let networkRecoveries = 0;
        const reinforceKnownDuration = (data?: unknown) => {
          if (data) {
            forceKnownDurationInLevelDetails(data, knownDurationSeconds);
          }
          applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
          window.setTimeout(() => {
            applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
          }, 0);
        };
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          reinforceKnownDuration();
          applyVideoAudioState(video, preservedAudioState);
          if (shouldResumePlayback) {
            void video.play().catch(() => {});
          } else {
            video.pause();
          }
        });
        hls.on(Hls.Events.LEVEL_LOADED, (_event: unknown, data: LevelLoadedData) => {
          reinforceKnownDuration(data);
        });
        hls.on(Hls.Events.LEVEL_UPDATED, (_event: unknown, data: LevelUpdatedData) => {
          reinforceKnownDuration(data);
        });
        hls.on(Hls.Events.FRAG_BUFFERED, () => {
          reinforceKnownDuration();
        });
        hls.on(Hls.Events.ERROR, (_event: unknown, data: ErrorData) => {
          if (!data?.fatal) return;
          const errorType = data?.type;
          if (errorType === Hls.ErrorTypes.NETWORK_ERROR && networkRecoveries < 10) {
            networkRecoveries += 1;
            try {
              hls.startLoad();
              return;
            } catch {
              // fall through to error
            }
          } else if (errorType === Hls.ErrorTypes.MEDIA_ERROR) {
            try {
              hls.recoverMediaError();
              return;
            } catch {
              // fall through to error
            }
          }
          setError(`HLS playback error: ${data.details || 'fatal stream error'}`);
        });
        hls.attachMedia(video);
        hls.loadSource(data.hls_url);
      } else if (canNativeHls) {
        video.src = data.hls_url;
        video.load();
        applyVideoAudioState(video, preservedAudioState);
        if (!shouldResumePlayback) {
          video.pause();
        }
      } else {
        throw new Error('HLS playback is not supported in this browser.');
      }
    } catch (e: unknown) {
      setError(clientErrorMessage(e, 'Failed to start HLS playback.'));
    } finally {
      setStartingHls(false);
    }
  }, [descriptor, destroyHls, sessionId, stopSession, hlsTargetHeight, mediaInfo]);

  useEffect(() => {
    let cancelled = false;
    autoStartedRef.current = false;
    setLoadingDescriptor(true);
    setDescriptor(null);
    setMediaInfo(null);
    setSessionId(null);
    setError('');
    setTimelineNowSecs(0);
    setTimelineDurationSecs(0);
    setHlsSessionStartOffsetSecs(0);

    apiJson<PlaybackDescriptor>(`/items/${id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
        return apiJson<MediaInfo>(data.media_info_url)
          .then((info) => {
            if (!cancelled) setMediaInfo(info);
          })
          .catch(() => {
            // Media info improves decision quality, but playback should still proceed.
          });
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(clientErrorMessage(e, 'Failed to load playback descriptor.'));
      })
      .finally(() => {
        if (!cancelled) setLoadingDescriptor(false);
      });

    return () => {
      cancelled = true;
    };
  }, [id]);

  // Auto-start HLS once the descriptor is ready
  useEffect(() => {
    if (!loadingDescriptor && canStartPlayback && !autoStartedRef.current) {
      autoStartedRef.current = true;
      void startHls();
    }
  }, [loadingDescriptor, canStartPlayback, startHls]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionId) {
        void stopSession(sessionId);
      }
    };
  }, [destroyHls, sessionId, stopSession]);

  useEffect(() => {
    const hls = hlsRef.current as { __stopKnownDurationEnforcer?: () => void } | null;
    if (!hls) return;

    const nextDuration = Math.max(
      descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
      mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
    );
    if (!Number.isFinite(nextDuration) || nextDuration <= 0) return;

    const previousStop = hls.__stopKnownDurationEnforcer;
    if (typeof previousStop === 'function') {
      try {
        previousStop();
      } catch {
        // no-op
      }
    }

    const stop = installKnownDurationEnforcer(hls, nextDuration);
    hls.__stopKnownDurationEnforcer = stop;
    applyKnownDurationToHlsMediaSource(hls, nextDuration);

    return () => {
      if (hls.__stopKnownDurationEnforcer === stop) {
        try {
          stop();
        } catch {
          // no-op
        }
      }
    };
  }, [descriptor?.duration_ms, mediaInfo?.duration_secs]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const syncState = () => {
      setIsPlaying(!video.paused && !video.ended);
      setIsMuted(video.muted || video.volume <= 0);
      setPlaybackRate(Number.isFinite(video.playbackRate) && video.playbackRate > 0 ? video.playbackRate : 1);
      setTimelineNowSecs(
        Number.isFinite(video.currentTime)
          ? hlsSessionStartOffsetSecs + video.currentTime
          : hlsSessionStartOffsetSecs,
      );
      if (Number.isFinite(video.duration) && video.duration > 0) {
        setTimelineDurationSecs((prev) => Math.max(prev, video.duration));
      }
    };

    const handleFullscreenChange = () => {
      // Re-render the control bar if fullscreen state changes.
      syncState();
    };

    syncState();
    video.addEventListener('play', syncState);
    video.addEventListener('pause', syncState);
    video.addEventListener('ended', syncState);
    video.addEventListener('ratechange', syncState);
    video.addEventListener('volumechange', syncState);
    video.addEventListener('loadedmetadata', syncState);
    video.addEventListener('durationchange', syncState);
    video.addEventListener('timeupdate', syncState);
    document.addEventListener('fullscreenchange', handleFullscreenChange);

    return () => {
      video.removeEventListener('play', syncState);
      video.removeEventListener('pause', syncState);
      video.removeEventListener('ended', syncState);
      video.removeEventListener('ratechange', syncState);
      video.removeEventListener('volumechange', syncState);
      video.removeEventListener('loadedmetadata', syncState);
      video.removeEventListener('durationchange', syncState);
      video.removeEventListener('timeupdate', syncState);
      document.removeEventListener('fullscreenchange', handleFullscreenChange);
    };
  }, [hlsSessionStartOffsetSecs]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const interval = setInterval(() => {
      if (video.currentTime > 0) {
        apiFetch('/playback/progress', {
          method: 'POST',
          body: JSON.stringify({
            item_id: id,
            progress_ms: Math.floor((hlsSessionStartOffsetSecs + video.currentTime) * 1000),
            played: video.ended,
          }),
        }).catch(() => {});
      }
    }, 10000);
    return () => clearInterval(interval);
  }, [hlsSessionStartOffsetSecs, id]);

  const knownDurationFromDescriptorSecs =
    descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0;
  const knownDurationFromProbeSecs =
    mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0;
  const knownDurationSecs = Math.max(knownDurationFromDescriptorSecs, knownDurationFromProbeSecs);
  const effectiveTotalDuration = Math.max(knownDurationSecs, timelineDurationSecs);
  const effectiveSeekLimit = effectiveTotalDuration > 0 ? effectiveTotalDuration : timelineNowSecs;
  const seekValue = Math.min(timelineNowSecs, effectiveSeekLimit || 0);
  const isFullscreen = typeof document !== 'undefined' && document.fullscreenElement === playerShellRef.current;
  const hlsBufferedWindowEndSecs = hlsSessionStartOffsetSecs + timelineDurationSecs;

  const togglePlayback = useCallback(async () => {
    const video = videoRef.current;
    if (!video) return;
    try {
      if (video.paused || video.ended) {
        await video.play();
      } else {
        video.pause();
      }
    } catch (e: unknown) {
      setError(clientErrorMessage(e, 'Playback control failed.'));
    }
  }, []);

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
    audioStateRef.current = readVideoAudioState(video);
    setIsMuted(video.muted || video.volume <= 0);
  }, []);

  const handleSeek = useCallback(
    async (targetSeconds: number) => {
      const video = videoRef.current;
      if (!video) return;

      const safeTarget = Math.max(0, Math.min(targetSeconds, effectiveTotalDuration || targetSeconds));
      if (!Number.isFinite(safeTarget)) return;

      if (
        descriptor?.file_id &&
        (safeTarget < Math.max(0, hlsSessionStartOffsetSecs - 1) ||
          (timelineDurationSecs > 0 && safeTarget > hlsBufferedWindowEndSecs + 1))
      ) {
        await startHls({
          targetHeightOverride: hlsTargetHeight,
          seekTimeOverrideSecs: safeTarget,
        });
        return;
      }

      video.currentTime = Math.max(0, safeTarget - hlsSessionStartOffsetSecs);
      setTimelineNowSecs(safeTarget);
    },
    [
      descriptor?.file_id,
      effectiveTotalDuration,
      hlsBufferedWindowEndSecs,
      hlsSessionStartOffsetSecs,
      hlsTargetHeight,
      startHls,
      timelineDurationSecs,
    ],
  );

  const toggleFullscreen = useCallback(async () => {
    const shell = playerShellRef.current;
    if (!shell) return;
    try {
      if (document.fullscreenElement === shell) {
        await document.exitFullscreen();
      } else {
        await shell.requestFullscreen();
      }
    } catch (e: unknown) {
      setError(clientErrorMessage(e, 'Fullscreen toggle failed.'));
    }
  }, []);

  const setPlayerRate = useCallback((rate: number) => {
    const video = videoRef.current;
    if (!video) return;
    video.playbackRate = rate;
    setPlaybackRate(rate);
  }, []);

  return (
    <div className="space-y-5 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold">Player</h1>
        <p className="text-sm muted">Item ID: {id}</p>
      </header>

      {error && <p className="notice-error rounded-xl px-4 py-2 text-sm">{error}</p>}
      {loadingDescriptor && (
        <p className="panel-soft rounded-xl px-4 py-2 text-sm muted">Preparing playback descriptor…</p>
      )}
      {!loadingDescriptor && !canStartPlayback && (
        <p className="notice-error rounded-xl px-4 py-2 text-sm">
          This item does not currently map to a playable media file. Rescan the library and retry.
        </p>
      )}

      <div
        ref={playerShellRef}
        className="tile overflow-hidden rounded-2xl border border-white/10 bg-black"
      >
        <video
          ref={videoRef}
          className="w-full max-h-[80vh]"
          playsInline
          preload="metadata"
          onLoadedMetadata={(event) => {
            const video = event.currentTarget;
            audioStateRef.current = readVideoAudioState(video);
            setTimelineNowSecs(
              Number.isFinite(video.currentTime)
                ? hlsSessionStartOffsetSecs + video.currentTime
                : hlsSessionStartOffsetSecs,
            );
            if (Number.isFinite(video.duration) && video.duration > 0) {
              setTimelineDurationSecs((prev) => Math.max(prev, video.duration));
            }
          }}
          onPlay={(event) => {
            const video = event.currentTarget;
            applyVideoAudioState(video, audioStateRef.current);
          }}
          onVolumeChange={(event) => {
            audioStateRef.current = readVideoAudioState(event.currentTarget);
          }}
          onTimeUpdate={(event) => {
            const video = event.currentTarget;
            setTimelineNowSecs(
              Number.isFinite(video.currentTime)
                ? hlsSessionStartOffsetSecs + video.currentTime
                : hlsSessionStartOffsetSecs,
            );
            if (Number.isFinite(video.duration) && video.duration > 0) {
              setTimelineDurationSecs((prev) => Math.max(prev, video.duration));
            }
          }}
          onError={() => {
            setError('HLS playback failed. Refresh the page and retry.');
          }}
        />
        <div className="border-t border-white/10 bg-[linear-gradient(180deg,rgba(24,28,40,0.96),rgba(17,20,28,0.98))] px-3 py-3">
          <div className="flex flex-wrap items-center gap-3">
            <button
              type="button"
              onClick={() => void togglePlayback()}
              disabled={!canStartPlayback}
              className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm disabled:cursor-not-allowed disabled:opacity-50"
              aria-label={isPlaying ? 'Pause playback' : 'Play playback'}
              title={isPlaying ? 'Pause playback' : 'Play playback'}
            >
              {isPlaying ? <PauseIcon /> : <PlayIcon />}
            </button>
            <button
              type="button"
              onClick={toggleMute}
              disabled={!canStartPlayback}
              className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm disabled:cursor-not-allowed disabled:opacity-50"
              aria-label={isMuted ? 'Unmute audio' : 'Mute audio'}
              title={isMuted ? 'Unmute audio' : 'Mute audio'}
            >
              {isMuted ? <VolumeXIcon /> : <Volume2Icon />}
            </button>
            <span className="min-w-[4.75rem] text-right text-sm tabular-nums text-white/85">
              {formatClock(timelineNowSecs)}
            </span>
            <input
              type="range"
              min={0}
              max={effectiveSeekLimit > 0 ? effectiveSeekLimit : 0}
              step={0.1}
              value={seekValue}
              onChange={(event) => {
                const next = Number(event.target.value);
                setTimelineNowSecs(next);
              }}
              onMouseUp={(event) => {
                void handleSeek(Number((event.target as HTMLInputElement).value));
              }}
              onTouchEnd={(event) => {
                void handleSeek(Number((event.target as HTMLInputElement).value));
              }}
              onKeyUp={(event) => {
                void handleSeek(Number((event.target as HTMLInputElement).value));
              }}
              aria-label="Seek video"
              className="h-2 min-w-[12rem] flex-1 accent-[var(--orange)]"
              disabled={!canStartPlayback || effectiveSeekLimit <= 0}
            />
            <span className="min-w-[4.75rem] text-sm tabular-nums text-white/85">
              {formatClock(effectiveTotalDuration)}
            </span>
            <div className="relative ml-auto flex items-center gap-2">
              <button
                type="button"
                onClick={() => setShowPlayerSettings((current) => !current)}
                className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm"
                aria-label="Playback settings"
                title="Playback settings"
              >
                <SettingsIcon />
              </button>
              <button
                type="button"
                onClick={() => void toggleFullscreen()}
                className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm"
                aria-label={isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
                title={isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
              >
                {isFullscreen ? <FullscreenExitIcon /> : <FullscreenIcon />}
              </button>
              {showPlayerSettings ? (
                <div className="absolute right-0 top-[calc(100%+0.6rem)] z-20 w-56 rounded-2xl border border-white/10 bg-[rgba(24,28,40,0.98)] p-3 shadow-[0_20px_40px_rgba(0,0,0,0.45)] backdrop-blur">
                  <label className="flex flex-col gap-1 text-xs muted">
                    <span>Playback speed</span>
                    <select
                      className="select px-2 py-2 text-sm"
                      value={playbackRate}
                      onChange={(event) => setPlayerRate(Number(event.target.value))}
                    >
                      {[0.5, 0.75, 1, 1.25, 1.5, 2].map((rate) => (
                        <option key={rate} value={rate}>
                          {formatPlaybackRateLabel(rate)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="mt-3 flex flex-col gap-1 text-xs muted">
                    <span>Quality</span>
                    <select
                      className="select px-2 py-2 text-sm"
                      value={hlsTargetHeight ?? 'auto'}
                      onChange={(event) => {
                        const value = event.target.value;
                        const nextTargetHeight = value === 'auto' ? null : Number(value);
                        setHlsTargetHeight(nextTargetHeight);
                        void startHls({ targetHeightOverride: nextTargetHeight });
                      }}
                      disabled={startingHls}
                    >
                      {HLS_QUALITY_OPTIONS.map((option) => (
                        <option key={option.label} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
              ) : null}
            </div>
          </div>
        </div>
      </div>
      <div className="px-1 text-xs muted">
        <span>
          Timeline: {formatClock(timelineNowSecs)} / {formatClock(effectiveTotalDuration)}
        </span>
        {knownDurationSecs > 0 && timelineDurationSecs > 0 && hlsBufferedWindowEndSecs < knownDurationSecs - 1 ? (
          <span className="ml-2">
            (buffered through: {formatClock(hlsBufferedWindowEndSecs)})
          </span>
        ) : null}
      </div>
    </div>
  );
}
