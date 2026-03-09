'use client';

import {
  type RefObject,
  type VideoHTMLAttributes,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';

export type VideoQualityOption = { value: 'auto' | number; label: string };

export const VIDEO_QUALITY_OPTIONS: VideoQualityOption[] = [
  { value: 'auto', label: 'Auto (Original)' },
  { value: 2160, label: '2160p (4K)' },
  { value: 1440, label: '1440p' },
  { value: 1080, label: '1080p' },
  { value: 720, label: '720p' },
  { value: 480, label: '480p' },
  { value: 360, label: '360p' },
];

type IconProps = {
  className?: string;
};

type VideoPlayerSurfaceProps = {
  shellRef?: RefObject<HTMLDivElement | null>;
  videoRef: RefObject<HTMLVideoElement | null>;
  videoElementProps?: VideoHTMLAttributes<HTMLVideoElement>;
  canStartPlayback: boolean;
  knownDurationSecs: number;
  bufferedWindowEndSecs?: number | null;
  sessionStartOffsetSecs?: number;
  qualityValue: 'auto' | number;
  qualityOptions: VideoQualityOption[];
  qualityDisabled?: boolean;
  onQualityChange: (value: 'auto' | number) => void;
  onSeekRequest: (targetSeconds: number) => void | Promise<void>;
  onDownload?: () => void | Promise<void>;
  downloading?: boolean;
  downloadDisabled?: boolean;
  playbackEnabled?: boolean;
  seekEnabled?: boolean;
  playbackDisabledReason?: string | null;
  statusText?: string | null;
  maxViewportHeightClassName?: string;
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

function clampVolume(value: number): number {
  if (!Number.isFinite(value)) return 1;
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

export function filterPlaybackQualityOptions(sourceHeight?: number | null): VideoQualityOption[] {
  if (!sourceHeight || sourceHeight <= 0) {
    return VIDEO_QUALITY_OPTIONS;
  }
  return VIDEO_QUALITY_OPTIONS.filter(
    (option) => option.value === 'auto' || option.value <= sourceHeight,
  );
}

export function normalizePlaybackQualitySelection(
  selected: number | null,
  sourceHeight?: number | null,
): number | null {
  if (!selected || !sourceHeight || sourceHeight <= 0 || selected <= sourceHeight) {
    return selected;
  }
  const fallback = VIDEO_QUALITY_OPTIONS.find(
    (option) => option.value !== 'auto' && option.value <= sourceHeight,
  );
  return typeof fallback?.value === 'number' ? fallback.value : null;
}

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
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M11 5 6.5 9H3.75A1.25 1.25 0 0 0 2.5 10.25v3.5A1.25 1.25 0 0 0 3.75 15H6.5L11 19V5Z" />
      <path d="M15 9.5a4 4 0 0 1 0 5" />
      <path d="M17.75 7.25a7 7 0 0 1 0 9.5" />
    </svg>
  );
}

function VolumeXIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M11 5 6.5 9H3.75A1.25 1.25 0 0 0 2.5 10.25v3.5A1.25 1.25 0 0 0 3.75 15H6.5L11 19V5Z" />
      <path d="m15.5 10.5 4 4" />
      <path d="m19.5 10.5-4 4" />
    </svg>
  );
}

function SettingsIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M10.4 3.2h3.2l.55 2.05c.35.1.68.24 1 .4l1.93-.94 2.26 2.26-.94 1.93c.16.32.3.65.4 1l2.05.55v3.2l-2.05.55c-.1.35-.24.68-.4 1l.94 1.93-2.26 2.26-1.93-.94c-.32.16-.65.3-1 .4l-.55 2.05h-3.2l-.55-2.05a6.4 6.4 0 0 1-1-.4l-1.93.94-2.26-2.26.94-1.93a6.4 6.4 0 0 1-.4-1L3.2 13.6v-3.2l2.05-.55c.1-.35.24-.68.4-1L4.7 6.92l2.26-2.26 1.93.94c.32-.16.65-.3 1-.4l.55-2.05Z" />
      <circle cx="12" cy="12" r="2.8" />
    </svg>
  );
}

function FullscreenIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M8 3.75H5.75A2 2 0 0 0 3.75 5.75V8" />
      <path d="M16 3.75h2.25a2 2 0 0 1 2 2V8" />
      <path d="M20.25 16v2.25a2 2 0 0 1-2 2H16" />
      <path d="M8 20.25H5.75a2 2 0 0 1-2-2V16" />
    </svg>
  );
}

function FullscreenExitIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
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

function DownloadIcon({ className = 'h-5 w-5' }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M12 4.5v10.25" />
      <path d="m8.25 11.75 3.75 3.75 3.75-3.75" />
      <path d="M4.5 18.25h15" />
    </svg>
  );
}

export default function VideoPlayerSurface({
  shellRef,
  videoRef,
  videoElementProps,
  canStartPlayback,
  knownDurationSecs,
  bufferedWindowEndSecs,
  sessionStartOffsetSecs = 0,
  qualityValue,
  qualityOptions,
  qualityDisabled = false,
  onQualityChange,
  onSeekRequest,
  onDownload,
  downloading = false,
  downloadDisabled = false,
  playbackEnabled = true,
  seekEnabled = true,
  playbackDisabledReason,
  statusText,
  maxViewportHeightClassName = 'max-h-[80vh]',
}: VideoPlayerSurfaceProps) {
  const localShellRef = useRef<HTMLDivElement>(null);
  const activeShellRef = shellRef ?? localShellRef;
  const clickTimerRef = useRef<number | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isMuted, setIsMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [playbackRate, setPlaybackRate] = useState(1);
  const [currentPositionSecs, setCurrentPositionSecs] = useState(0);
  const [rawDurationSecs, setRawDurationSecs] = useState(0);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [pendingSeekSecs, setPendingSeekSecs] = useState<number | null>(null);

  const syncFromVideo = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;

    const safeVolume = clampVolume(video.volume);
    setIsPlaying(!video.paused && !video.ended);
    setIsMuted(video.muted || safeVolume <= 0);
    setVolume(safeVolume);
    setPlaybackRate(
      Number.isFinite(video.playbackRate) && video.playbackRate > 0 ? video.playbackRate : 1,
    );
    setCurrentPositionSecs(
      Number.isFinite(video.currentTime)
        ? sessionStartOffsetSecs + video.currentTime
        : sessionStartOffsetSecs,
    );
    if (Number.isFinite(video.duration) && video.duration > 0) {
      setRawDurationSecs(video.duration);
    }
  }, [sessionStartOffsetSecs, videoRef]);

  useEffect(() => {
    syncFromVideo();
    const video = videoRef.current;
    if (!video) return;

    const handleSync = () => syncFromVideo();
    const handleFullscreenChange = () => {
      setIsFullscreen(document.fullscreenElement === activeShellRef.current);
      syncFromVideo();
    };
    video.addEventListener('play', handleSync);
    video.addEventListener('pause', handleSync);
    video.addEventListener('ended', handleSync);
    video.addEventListener('ratechange', handleSync);
    video.addEventListener('volumechange', handleSync);
    video.addEventListener('loadedmetadata', handleSync);
    video.addEventListener('durationchange', handleSync);
    video.addEventListener('timeupdate', handleSync);
    document.addEventListener('fullscreenchange', handleFullscreenChange);

    return () => {
      video.removeEventListener('play', handleSync);
      video.removeEventListener('pause', handleSync);
      video.removeEventListener('ended', handleSync);
      video.removeEventListener('ratechange', handleSync);
      video.removeEventListener('volumechange', handleSync);
      video.removeEventListener('loadedmetadata', handleSync);
      video.removeEventListener('durationchange', handleSync);
      video.removeEventListener('timeupdate', handleSync);
      document.removeEventListener('fullscreenchange', handleFullscreenChange);
    };
  }, [syncFromVideo, videoRef]);

  useEffect(() => {
    return () => {
      if (clickTimerRef.current !== null) {
        window.clearTimeout(clickTimerRef.current);
        clickTimerRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    setPendingSeekSecs(null);
  }, [knownDurationSecs, sessionStartOffsetSecs]);

  const effectiveBufferedWindowEndSecs = useMemo(() => {
    if (typeof bufferedWindowEndSecs === 'number' && Number.isFinite(bufferedWindowEndSecs)) {
      return bufferedWindowEndSecs;
    }
    return sessionStartOffsetSecs + rawDurationSecs;
  }, [bufferedWindowEndSecs, rawDurationSecs, sessionStartOffsetSecs]);

  const effectiveDurationSecs = useMemo(() => {
    return Math.max(knownDurationSecs, effectiveBufferedWindowEndSecs);
  }, [effectiveBufferedWindowEndSecs, knownDurationSecs]);

  const effectiveSeekValue = pendingSeekSecs ?? Math.min(currentPositionSecs, effectiveDurationSecs || 0);

  const commitSeek = useCallback(async () => {
    if (pendingSeekSecs === null) return;
    await onSeekRequest(pendingSeekSecs);
    setPendingSeekSecs(null);
  }, [onSeekRequest, pendingSeekSecs]);

  const togglePlayback = useCallback(async () => {
    if (!playbackEnabled) return;
    const video = videoRef.current;
    if (!video) return;
    if (video.paused || video.ended) {
      await video.play().catch(() => {});
    } else {
      video.pause();
    }
  }, [playbackEnabled, videoRef]);

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
    syncFromVideo();
  }, [syncFromVideo, videoRef]);

  const updateVolume = useCallback(
    (nextValue: number) => {
      const video = videoRef.current;
      if (!video) return;
      const nextVolume = clampVolume(nextValue);
      video.volume = nextVolume;
      if (nextVolume > 0) {
        video.muted = false;
      }
      syncFromVideo();
    },
    [syncFromVideo, videoRef],
  );

  const toggleFullscreen = useCallback(async () => {
    const shell = activeShellRef.current;
    if (!shell) return;
    try {
      if (document.fullscreenElement === shell) {
        await document.exitFullscreen();
      } else {
        await shell.requestFullscreen();
      }
    } catch {
      // no-op
    }
  }, [activeShellRef]);

  const updatePlaybackRate = useCallback(
    (nextRate: number) => {
      const video = videoRef.current;
      if (!video) return;
      video.playbackRate = nextRate;
      syncFromVideo();
    },
    [syncFromVideo, videoRef],
  );

  const handleVideoSingleClick = useCallback(() => {
    if (!canStartPlayback || !playbackEnabled) return;
    if (clickTimerRef.current !== null) {
      window.clearTimeout(clickTimerRef.current);
    }
    clickTimerRef.current = window.setTimeout(() => {
      clickTimerRef.current = null;
      void togglePlayback();
    }, 220);
  }, [canStartPlayback, playbackEnabled, togglePlayback]);

  const handleVideoDoubleClick = useCallback(() => {
    if (clickTimerRef.current !== null) {
      window.clearTimeout(clickTimerRef.current);
      clickTimerRef.current = null;
    }
    void toggleFullscreen();
  }, [toggleFullscreen]);

  return (
    <div
      ref={activeShellRef}
      className={
        isFullscreen
          ? 'flex h-screen w-screen flex-col overflow-hidden bg-black'
          : 'tile overflow-hidden rounded-2xl border border-white/10 bg-black'
      }
    >
      <div
        className={
          isFullscreen
            ? 'relative flex min-h-0 flex-1 cursor-pointer items-center justify-center bg-black'
            : 'relative cursor-pointer'
        }
        onClick={() => {
          handleVideoSingleClick();
        }}
        onDoubleClick={() => {
          handleVideoDoubleClick();
        }}
      >
        <video
          ref={videoRef}
          {...videoElementProps}
          controls={false}
          preload={videoElementProps?.preload ?? 'auto'}
          playsInline
          className={
            `${
              isFullscreen
                ? 'h-full w-full max-h-full object-contain'
                : `w-full cursor-pointer ${maxViewportHeightClassName}`
            } ${(videoElementProps?.className ?? '').trim()}`.trim()
          }
        />
      </div>
      <div className="shrink-0 border-t border-white/10 bg-[linear-gradient(180deg,rgba(24,28,40,0.96),rgba(17,20,28,0.98))] px-3 py-3">
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="button"
            onClick={() => void togglePlayback()}
            disabled={!canStartPlayback || !playbackEnabled}
            aria-label={isPlaying ? 'Pause playback' : 'Play playback'}
            title={playbackDisabledReason ?? (isPlaying ? 'Pause playback' : 'Play playback')}
            className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm disabled:cursor-not-allowed disabled:opacity-50"
          >
            {isPlaying ? <PauseIcon /> : <PlayIcon />}
          </button>

          <div className="flex min-w-[10rem] items-center gap-2">
            <button
              type="button"
              onClick={toggleMute}
              disabled={!canStartPlayback}
              aria-label={isMuted ? 'Unmute audio' : 'Mute audio'}
              title={isMuted ? 'Unmute audio' : 'Mute audio'}
              className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm disabled:cursor-not-allowed disabled:opacity-50"
            >
              {isMuted ? <VolumeXIcon /> : <Volume2Icon />}
            </button>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={isMuted ? 0 : volume}
              onChange={(event) => updateVolume(Number(event.target.value))}
              aria-label="Volume"
              className="h-2 w-28 accent-[var(--orange)]"
              disabled={!canStartPlayback}
            />
          </div>

          <span className="min-w-[4.75rem] text-right text-sm tabular-nums text-white/85">
            {formatClock(currentPositionSecs)}
          </span>
          <input
            type="range"
            min={0}
            max={effectiveDurationSecs > 0 ? effectiveDurationSecs : 0}
            step={0.1}
            value={effectiveSeekValue}
            onChange={(event) => setPendingSeekSecs(Number(event.target.value))}
            onMouseUp={() => {
              void commitSeek();
            }}
            onTouchEnd={() => {
              void commitSeek();
            }}
            onKeyUp={() => {
              void commitSeek();
            }}
            aria-label="Seek video"
            className="h-2 min-w-[12rem] flex-1 accent-[var(--orange)]"
            disabled={!canStartPlayback || !seekEnabled || effectiveDurationSecs <= 0}
            title={seekEnabled ? 'Seek video' : playbackDisabledReason ?? 'Seeking disabled'}
          />
          <span className="min-w-[4.75rem] text-sm tabular-nums text-white/85">
            {formatClock(effectiveDurationSecs)}
          </span>

          <div className="ml-auto flex items-center gap-2">
            {onDownload ? (
              <button
                type="button"
                onClick={() => void onDownload()}
                disabled={downloadDisabled || downloading || !canStartPlayback}
                aria-label="Download current quality"
                title="Download current quality"
                className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm disabled:cursor-not-allowed disabled:opacity-50"
              >
                <DownloadIcon />
              </button>
            ) : null}
            <div className="relative">
              <button
                type="button"
                onClick={() => setShowSettings((current) => !current)}
                className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm"
                aria-label="Playback settings"
                title="Playback settings"
              >
                <SettingsIcon />
              </button>
              {showSettings ? (
                <div className="absolute bottom-[calc(100%+0.6rem)] right-0 z-20 w-56 rounded-2xl border border-white/10 bg-[rgba(24,28,40,0.98)] p-3 shadow-[0_20px_40px_rgba(0,0,0,0.45)] backdrop-blur">
                  <label className="flex flex-col gap-1 text-xs muted">
                    <span>Playback speed</span>
                    <select
                      className="select px-2 py-2 text-sm"
                      value={playbackRate}
                      onChange={(event) => updatePlaybackRate(Number(event.target.value))}
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
                      value={qualityValue}
                      onChange={(event) => {
                        const raw = event.target.value;
                        onQualityChange(raw === 'auto' ? 'auto' : Number(raw));
                      }}
                      disabled={qualityDisabled}
                    >
                      {qualityOptions.map((option) => (
                        <option key={option.label} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
              ) : null}
            </div>
            <button
              type="button"
              onClick={() => void toggleFullscreen()}
              className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 text-sm"
              aria-label={isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
              title={isFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
            >
              {isFullscreen ? <FullscreenExitIcon /> : <FullscreenIcon />}
            </button>
          </div>
        </div>
        {(statusText || (knownDurationSecs > 0 && effectiveBufferedWindowEndSecs < knownDurationSecs - 1)) && (
          <div className="mt-3 flex flex-wrap items-center gap-2 text-xs muted">
            {statusText ? <span>{statusText}</span> : null}
            {knownDurationSecs > 0 && effectiveBufferedWindowEndSecs < knownDurationSecs - 1 ? (
              <span>Buffered through {formatClock(effectiveBufferedWindowEndSecs)}</span>
            ) : null}
          </div>
        )}
      </div>
    </div>
  );
}
