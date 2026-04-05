'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import type Hls from 'hls.js';
import type { ErrorData, LevelDetails, LevelLoadedData, LevelUpdatedData } from 'hls.js';
import { useParams } from 'next/navigation';
import VideoPlayerSurface, {
  filterPlaybackQualityOptions,
  normalizePlaybackQualitySelection,
  resolveAutoPlaybackTargetHeight,
} from '@/app/components/VideoPlayerSurface';
import { apiFetch, apiJson } from '@/lib/api';
import { readBrowserToken } from '@/lib/browserAuth';
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

type PlayState = {
  item_id: string;
  played: boolean;
  progress_ms: number;
  last_played_ts?: number | null;
  favorite: boolean;
};

type ItemSummary = {
  id: string;
  title: string;
  kind: string;
  parent_id?: string | null;
  poster_url?: string | null;
  backdrop_url?: string | null;
  thumb_url?: string | null;
};

function shouldResumeFromCurrentSource(video: HTMLVideoElement, fileId: string): boolean {
  const src = `${video.currentSrc || video.src || ''}`.toLowerCase();
  if (!src) return false;
  return src.includes(`/stream/hls/`) || src.includes(`/stream/files/${fileId.toLowerCase()}`);
}

function resetVideoSourceForMse(video: HTMLVideoElement): void {
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

function readBufferedWindowDuration(video: HTMLVideoElement | null): number {
  if (!video) return 0;
  try {
    const buffered = video.buffered;
    if (buffered && buffered.length > 0) {
      const end = buffered.end(buffered.length - 1);
      if (Number.isFinite(end) && end > 0) {
        return end;
      }
    }
  } catch {
    // Some browsers can throw while buffered ranges are mutating.
  }
  return Number.isFinite(video.currentTime) && video.currentTime > 0 ? video.currentTime : 0;
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

function extractPlaylistWindowDuration(
  levelData: LevelLoadedData | LevelUpdatedData | unknown,
): number {
  const details = (levelData as { details?: LevelDetails } | null)?.details as
    | (LevelDetails & {
        fragments?: Array<{
          end?: number;
          start?: number;
          duration?: number;
        }>;
      })
    | undefined;
  const fragments = details?.fragments;
  if (!Array.isArray(fragments) || fragments.length === 0) {
    return 0;
  }
  const last = fragments[fragments.length - 1];
  const fromEnd = typeof last?.end === 'number' && Number.isFinite(last.end) ? last.end : 0;
  const fromStartDuration =
    typeof last?.start === 'number' &&
    Number.isFinite(last.start) &&
    typeof last?.duration === 'number' &&
    Number.isFinite(last.duration)
      ? last.start + last.duration
      : 0;
  return Math.max(fromEnd, fromStartDuration, 0);
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

async function waitForVideoMetadata(
  video: HTMLVideoElement,
  timeoutMs = 5000,
): Promise<void> {
  if (video.readyState >= 1) return;

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      video.removeEventListener('loadedmetadata', finish);
      resolve();
    };

    video.addEventListener('loadedmetadata', finish);
    window.setTimeout(finish, timeoutMs);
  });
}

async function waitForVideoFrameData(
  video: HTMLVideoElement,
  timeoutMs = 4000,
): Promise<void> {
  if (video.readyState >= 2) return;

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      video.removeEventListener('loadeddata', finish);
      video.removeEventListener('canplay', finish);
      resolve();
    };

    video.addEventListener('loadeddata', finish);
    video.addEventListener('canplay', finish);
    window.setTimeout(finish, timeoutMs);
  });
}

async function ensurePreviewFrame(video: HTMLVideoElement): Promise<void> {
  await waitForVideoMetadata(video);
  await waitForVideoFrameData(video);

  try {
    const previewTarget = video.currentTime > 0.001 ? video.currentTime : 0.001;
    if (Math.abs(video.currentTime - previewTarget) > 0.0005) {
      video.currentTime = previewTarget;
    }
  } catch {
    // Some browsers block tiny seeks while the first segment is still warming up.
  }
}

async function attemptPlayWithWarmup(video: HTMLVideoElement): Promise<void> {
  try {
    await video.play();
    return;
  } catch {
    // Fall back to waiting for the first decodable frame, then retry.
  }

  await ensurePreviewFrame(video).catch(() => {});

  if (!video.paused) {
    return;
  }

  try {
    await video.play();
  } catch {
    // Leave the preview frame painted even if autoplay is blocked.
  }
}

async function ensurePausedPreviewFrame(video: HTMLVideoElement): Promise<boolean> {
  await ensurePreviewFrame(video).catch(() => {});
  try {
    video.pause();
  } catch {
    // no-op
  }
  return video.readyState >= 2;
}

function normalizeSessionStartTimeSeconds(
  requestedSeconds: number | undefined,
  knownDurationSeconds: number,
): number | undefined {
  if (requestedSeconds === undefined || !Number.isFinite(requestedSeconds)) {
    return undefined;
  }

  const sanitized = Math.max(0, requestedSeconds);
  if (!Number.isFinite(knownDurationSeconds) || knownDurationSeconds <= 0) {
    return sanitized;
  }

  const safeMax = Math.max(0, knownDurationSeconds - 0.5);
  return Math.min(sanitized, safeMax);
}

function resolveResumeStartTimeSeconds(
  requestedSeconds: number,
  knownDurationSeconds: number,
): number | undefined {
  if (!Number.isFinite(knownDurationSeconds) || knownDurationSeconds <= 0) {
    return undefined;
  }

  const normalized = normalizeSessionStartTimeSeconds(requestedSeconds, knownDurationSeconds);
  if (normalized === undefined) return undefined;
  if (normalized >= Math.max(0, knownDurationSeconds - 1)) {
    return undefined;
  }
  return normalized;
}

type StartHlsOptions = {
  targetHeightOverride?: number | null;
  seekTimeOverrideSecs?: number;
  autoPlayOnReady?: boolean;
};

function fallbackDownloadName(itemId: string, targetHeight: number | null): string {
  if (targetHeight && targetHeight > 0) {
    return `rustyfin-${itemId}-${targetHeight}p.mp4`;
  }
  return `rustyfin-${itemId}.bin`;
}

function extractDownloadFilename(header: string | null, fallback: string): string {
  if (!header) return fallback;
  const utf8Match = header.match(/filename\*=UTF-8''([^;]+)/i);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch {
      return utf8Match[1];
    }
  }
  const basicMatch = header.match(/filename="?([^";]+)"?/i);
  if (basicMatch?.[1]) {
    return basicMatch[1];
  }
  return fallback;
}

export default function PlayerPage() {
  const params = useParams();
  const id = params.id as string;
  const playerShellRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<Hls | null>(null);
  const sessionIdRef = useRef<string | null>(null);

  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [mediaInfo, setMediaInfo] = useState<MediaInfo | null>(null);
  const [playState, setPlayState] = useState<PlayState | null>(null);
  const [item, setItem] = useState<ItemSummary | null>(null);
  const [seriesTitle, setSeriesTitle] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loadingDescriptor, setLoadingDescriptor] = useState(true);
  const [loadingPlayState, setLoadingPlayState] = useState(true);
  const [startingHls, setStartingHls] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [hlsTargetHeight, setHlsTargetHeight] = useState<number | null>(null);
  const [hlsSessionStartOffsetSecs, setHlsSessionStartOffsetSecs] = useState(0);
  const [hlsAvailableWindowDurationSecs, setHlsAvailableWindowDurationSecs] = useState(0);
  const autoStartedRef = useRef(false);
  const requestedPlaybackRef = useRef(false);

  const canStartPlayback = Boolean(descriptor?.file_id);
  const sourceVideoHeight = mediaInfo?.video?.height && mediaInfo.video.height > 0 ? mediaInfo.video.height : null;
  const qualityOptions = filterPlaybackQualityOptions(sourceVideoHeight);
  const selectedQualityValue: 'auto' | number = hlsTargetHeight ?? 'auto';

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

  const startHls = useCallback(
    async (options?: StartHlsOptions) => {
      if (!descriptor?.file_id) {
        setError('No media file is attached to this item. Rescan the library and try again.');
        return;
      }

      const video = videoRef.current;
      if (!video) {
        setError('Player is not ready yet.');
        return;
      }

      const selectedTargetHeight =
        options?.targetHeightOverride !== undefined
          ? options.targetHeightOverride
          : hlsTargetHeight ?? resolveAutoPlaybackTargetHeight(sourceVideoHeight);
      if (options?.autoPlayOnReady !== undefined) {
        requestedPlaybackRef.current = options.autoPlayOnReady;
      }
      const shouldAutoPlay = requestedPlaybackRef.current;
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
      const requestedStartTimeSecs =
        explicitSeekTime !== undefined
          ? explicitSeekTime
          : finiteDuration &&
              currentTime > 0.5 &&
              currentTime < (video.duration as number) - 0.5
            ? currentTime
            : undefined;
      const startTimeSecs = normalizeSessionStartTimeSeconds(
        requestedStartTimeSecs,
        knownDurationSeconds,
      );

      setStartingHls(true);
      setError('');
      try {
        if (sessionIdRef.current) {
          await stopSession(sessionIdRef.current);
          sessionIdRef.current = null;
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
        sessionIdRef.current = data.session_id;
        setHlsSessionStartOffsetSecs(startTimeSecs ?? 0);
        setHlsAvailableWindowDurationSecs(0);

        const Hls = (await import('hls.js')).default;
        const canNativeHls = video.canPlayType('application/vnd.apple.mpegurl') !== '';
        video.preload = 'auto';
        video.autoplay = shouldAutoPlay;

        if (Hls.isSupported()) {
          resetVideoSourceForMse(video);
          const hls = new Hls({
            enableWorker: true,
            lowLatencyMode: false,
            liveDurationInfinity: false,
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
          let playbackKickPending = true;
          let playbackKickInFlight = false;
          const reinforceKnownDuration = (data?: unknown) => {
            const playlistWindowDuration = extractPlaylistWindowDuration(data);
            const reportedWindowDuration = Math.max(
              playlistWindowDuration,
              readBufferedWindowDuration(video),
            );
            if (reportedWindowDuration > 0) {
              setHlsAvailableWindowDurationSecs((current) =>
                reportedWindowDuration > current ? reportedWindowDuration : current,
              );
            }
            if (data) {
              forceKnownDurationInLevelDetails(data, knownDurationSeconds);
            }
            applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            window.setTimeout(() => {
              applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            }, 0);
          };
          const kickPlaybackOrPreview = async () => {
            if (!playbackKickPending || playbackKickInFlight) return;
            playbackKickInFlight = true;
            try {
              if (requestedPlaybackRef.current) {
                await attemptPlayWithWarmup(video);
                playbackKickPending = false;
                return;
              }
              const previewReady = await ensurePausedPreviewFrame(video);
              if (previewReady) {
                playbackKickPending = false;
              }
            } finally {
              playbackKickInFlight = false;
            }
          };
          hls.on(Hls.Events.MANIFEST_PARSED, () => {
            reinforceKnownDuration();
            void kickPlaybackOrPreview();
          });
          hls.on(Hls.Events.LEVEL_LOADED, (_event: unknown, data: LevelLoadedData) => {
            reinforceKnownDuration(data);
          });
          hls.on(Hls.Events.LEVEL_UPDATED, (_event: unknown, data: LevelUpdatedData) => {
            reinforceKnownDuration(data);
          });
          hls.on(Hls.Events.FRAG_BUFFERED, () => {
            reinforceKnownDuration();
            if (!playbackKickPending) return;
            if (requestedPlaybackRef.current && !video.paused) return;
            void kickPlaybackOrPreview();
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
          hls.startLoad(0);
        } else if (canNativeHls) {
          video.src = data.hls_url;
          video.load();
          if (requestedPlaybackRef.current) {
            await attemptPlayWithWarmup(video);
          } else {
            await ensurePausedPreviewFrame(video);
          }
        } else {
          throw new Error('HLS playback is not supported in this browser.');
        }
      } catch (e: unknown) {
        setError(clientErrorMessage(e, 'Failed to start HLS playback.'));
      } finally {
        setStartingHls(false);
      }
    },
    [descriptor, destroyHls, hlsTargetHeight, mediaInfo, sourceVideoHeight, stopSession],
  );

  const handlePlaybackToggleRequest = useCallback(async () => {
    const video = videoRef.current;
    if (!video) return;

    const currentlyWantsPlayback = requestedPlaybackRef.current;
    requestedPlaybackRef.current = !currentlyWantsPlayback;

    if (requestedPlaybackRef.current) {
      await attemptPlayWithWarmup(video);
      return;
    }

    try {
      video.pause();
    } catch {
      // no-op
    }
    await ensurePausedPreviewFrame(video).catch(() => {});
    try {
      video.pause();
    } catch {
      // no-op
    }
  }, []);

  const handleSeek = useCallback(
    async (targetSeconds: number) => {
      const video = videoRef.current;
      if (!video) return;

      const shouldResumeAfterSeek = requestedPlaybackRef.current || (!video.paused && !video.ended);
      try {
        video.pause();
      } catch {
        // no-op
      }

      const knownDurationSeconds = Math.max(
        descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
        mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
      );
      const effectiveDurationSeconds = Math.max(knownDurationSeconds, video.duration || 0);
      const safeTarget = Math.max(0, Math.min(targetSeconds, effectiveDurationSeconds || targetSeconds));
      if (!Number.isFinite(safeTarget)) return;

      const currentWindowDuration = Math.max(
        hlsAvailableWindowDurationSecs,
        readBufferedWindowDuration(video),
      );
      const bufferedWindowEndSecs = hlsSessionStartOffsetSecs + currentWindowDuration;
      if (
        descriptor?.file_id &&
        (safeTarget < Math.max(0, hlsSessionStartOffsetSecs - 1) ||
          safeTarget > bufferedWindowEndSecs + 1)
      ) {
        await startHls({
          targetHeightOverride: hlsTargetHeight,
          seekTimeOverrideSecs: safeTarget,
          autoPlayOnReady: shouldResumeAfterSeek,
        });
        return;
      }

      video.currentTime = Math.max(0, safeTarget - hlsSessionStartOffsetSecs);
      await waitForVideoFrameData(video).catch(() => {});
      if (shouldResumeAfterSeek) {
        await attemptPlayWithWarmup(video);
      }
    },
    [
      descriptor?.duration_ms,
      descriptor?.file_id,
      hlsAvailableWindowDurationSecs,
      hlsSessionStartOffsetSecs,
      hlsTargetHeight,
      mediaInfo?.duration_secs,
      startHls,
    ],
  );

  const handleDownload = useCallback(async () => {
    if (!descriptor?.file_id) return;
    setDownloading(true);
    setError('');
    try {
      const search = new URLSearchParams();
      if (hlsTargetHeight && hlsTargetHeight > 0) {
        search.set('target_height', String(hlsTargetHeight));
      }
      const suffix = search.toString();
      const path = `/playback/download/${descriptor.file_id}${suffix ? `?${suffix}` : ''}`;
      const res = await apiFetch(path, { method: 'GET' });
      if (!res.ok) {
        throw new Error(clientErrorMessage(await res.text(), `Download failed: ${res.status}`));
      }
      const blob = await res.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = downloadUrl;
      anchor.download = extractDownloadFilename(
        res.headers.get('content-disposition'),
        fallbackDownloadName(id, hlsTargetHeight),
      );
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(downloadUrl);
    } catch (e: unknown) {
      setError(clientErrorMessage(e, 'Failed to download media.'));
    } finally {
      setDownloading(false);
    }
  }, [descriptor?.file_id, hlsTargetHeight, id]);

  const sendProgressSnapshot = useCallback(
    async (keepalive = false) => {
      const video = videoRef.current;
      if (!video) return;

      const currentTime = Number.isFinite(video.currentTime) ? video.currentTime : 0;
      const progressMs = Math.max(
        0,
        Math.floor((hlsSessionStartOffsetSecs + currentTime) * 1000),
      );
      if (progressMs <= 0 && !video.ended) {
        return;
      }

      const token = readBrowserToken();
      const headers = new Headers({ 'Content-Type': 'application/json' });
      if (token) {
        headers.set('Authorization', `Bearer ${token}`);
      }

      await fetch('/api/v1/playback/progress', {
        method: 'POST',
        headers,
        body: JSON.stringify({
          item_id: id,
          progress_ms: progressMs,
          played: video.ended,
          playback_session_id: sessionIdRef.current,
        }),
        keepalive,
      }).catch(() => {});
    },
    [hlsSessionStartOffsetSecs, id],
  );

  useEffect(() => {
    let cancelled = false;
    autoStartedRef.current = false;
    requestedPlaybackRef.current = false;
    setLoadingDescriptor(true);
    setLoadingPlayState(true);
    setDescriptor(null);
    setMediaInfo(null);
    setPlayState(null);
    setItem(null);
    setSeriesTitle(null);
    sessionIdRef.current = null;
    setError('');
    setHlsSessionStartOffsetSecs(0);
    setHlsAvailableWindowDurationSecs(0);

    apiJson<ItemSummary>(`/items/${id}`)
      .then(async (data) => {
        if (cancelled) return;
        setItem(data);

        if (data.kind !== 'episode' || !data.parent_id) {
          setSeriesTitle(null);
          return;
        }

        try {
          const parent = await apiJson<ItemSummary>(`/items/${data.parent_id}`);
          if (cancelled) return;

          if (parent.kind === 'series') {
            setSeriesTitle(parent.title || null);
            return;
          }

          if (parent.kind === 'season' && parent.parent_id) {
            const grandParent = await apiJson<ItemSummary>(`/items/${parent.parent_id}`).catch(
              () => null,
            );
            if (cancelled) return;
            if (grandParent?.kind === 'series' && grandParent.title) {
              setSeriesTitle(grandParent.title);
              return;
            }
          }

          setSeriesTitle(null);
        } catch {
          if (!cancelled) setSeriesTitle(null);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setItem(null);
          setSeriesTitle(null);
        }
      });

    apiJson<PlaybackDescriptor>(`/items/${id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
        return apiJson<MediaInfo>(data.media_info_url)
          .then((info) => {
            if (!cancelled) setMediaInfo(info);
          })
          .catch(() => {
            // best-effort
          });
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(clientErrorMessage(e, 'Failed to load playback descriptor.'));
      })
      .finally(() => {
        if (!cancelled) setLoadingDescriptor(false);
      });

    apiJson<PlayState>(`/playback/state/${id}`)
      .then((data) => {
        if (!cancelled) setPlayState(data);
      })
      .catch(() => {
        if (!cancelled) setPlayState(null);
      })
      .finally(() => {
        if (!cancelled) setLoadingPlayState(false);
      });

    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    if (!loadingDescriptor && !loadingPlayState && canStartPlayback && !autoStartedRef.current) {
      autoStartedRef.current = true;
      const knownDurationSeconds =
        descriptor?.duration_ms && descriptor.duration_ms > 0
          ? descriptor.duration_ms / 1000
          : mediaInfo?.duration_secs && mediaInfo.duration_secs > 0
            ? mediaInfo.duration_secs
            : 0;
      const rawResumeSeconds =
        playState && !playState.played && playState.progress_ms > 0
          ? playState.progress_ms / 1000
          : undefined;
      const resumeSeconds =
        rawResumeSeconds !== undefined
          ? resolveResumeStartTimeSeconds(rawResumeSeconds, knownDurationSeconds)
          : undefined;
      requestedPlaybackRef.current = false;
      void startHls(
        resumeSeconds !== undefined
          ? { seekTimeOverrideSecs: resumeSeconds, autoPlayOnReady: false }
          : { autoPlayOnReady: false },
      );
    }
  }, [
    canStartPlayback,
    descriptor?.duration_ms,
    loadingDescriptor,
    loadingPlayState,
    mediaInfo?.duration_secs,
    playState,
    startHls,
  ]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionIdRef.current) {
        void stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }
    };
  }, [destroyHls, stopSession]);

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
    const normalized = normalizePlaybackQualitySelection(hlsTargetHeight, sourceVideoHeight);
    if (normalized !== hlsTargetHeight) {
      setHlsTargetHeight(normalized);
    }
  }, [hlsTargetHeight, sourceVideoHeight]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const interval = setInterval(() => {
      if (video.currentTime > 0 || video.ended) {
        void sendProgressSnapshot();
      }
    }, 10000);
    return () => clearInterval(interval);
  }, [sendProgressSnapshot]);

  useEffect(() => {
    const flushProgress = () => {
      void sendProgressSnapshot(true);
    };

    window.addEventListener('pagehide', flushProgress);
    return () => {
      window.removeEventListener('pagehide', flushProgress);
      flushProgress();
    };
  }, [sendProgressSnapshot]);

  const knownDurationSecs = Math.max(
    descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
    mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
  );
  const playerTitle = item?.title?.trim() || 'Player';
  const showTitle = seriesTitle?.trim() || null;
  const loadingArtworkUrl = item?.thumb_url ?? item?.poster_url ?? item?.backdrop_url ?? null;

  return (
    <div className="rf-flat-page animate-rise">
      <header className="rf-flat-header">
        <h1 className="text-3xl font-semibold">{playerTitle}</h1>
        {showTitle && <p className="text-sm muted">{showTitle}</p>}
      </header>

      {error && <p className="notice-error rounded-xl px-4 py-2 text-sm">{error}</p>}
      {loadingDescriptor && (
        <p className="rf-flat-empty px-4 py-2 text-sm muted">Preparing playback descriptor…</p>
      )}
      {!loadingDescriptor && !canStartPlayback && (
        <p className="notice-error rounded-xl px-4 py-2 text-sm">
          This item does not currently map to a playable media file. Rescan the library and retry.
        </p>
      )}

      <VideoPlayerSurface
        shellRef={playerShellRef}
        videoRef={videoRef}
        playbackKey={id}
        artworkUrl={loadingArtworkUrl}
        artworkAlt={playerTitle}
        enableSpacebarToggle
        canStartPlayback={canStartPlayback}
        knownDurationSecs={knownDurationSecs}
        bufferedWindowEndSecs={
          hlsSessionStartOffsetSecs + Math.max(hlsAvailableWindowDurationSecs, readBufferedWindowDuration(videoRef.current))
        }
        sessionStartOffsetSecs={hlsSessionStartOffsetSecs}
        qualityValue={selectedQualityValue}
        qualityOptions={qualityOptions}
        qualityDisabled={startingHls}
        onPlaybackToggleRequest={handlePlaybackToggleRequest}
        onQualityChange={(value) => {
          const nextTargetHeight = value === 'auto' ? null : value;
          const video = videoRef.current;
          const currentAbsoluteSeconds =
            video && Number.isFinite(video.currentTime) && video.currentTime >= 0
              ? hlsSessionStartOffsetSecs + video.currentTime
              : undefined;
          setHlsTargetHeight(nextTargetHeight);
          void startHls({
            targetHeightOverride: nextTargetHeight,
            seekTimeOverrideSecs:
              currentAbsoluteSeconds !== undefined && currentAbsoluteSeconds > 0.25
                ? currentAbsoluteSeconds
                : undefined,
          });
        }}
        onSeekRequest={handleSeek}
        onDownload={handleDownload}
        downloading={downloading}
        downloadDisabled={startingHls || !descriptor?.file_id}
        playbackEnabled={canStartPlayback}
        seekEnabled={canStartPlayback}
        maxViewportHeightClassName="max-h-[80vh]"
        videoElementProps={{
          preload: 'metadata',
          onError: () => {
            setError('HLS playback failed. Refresh the page and retry.');
          },
        }}
      />
    </div>
  );
}
