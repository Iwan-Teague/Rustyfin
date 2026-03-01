'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams } from 'next/navigation';
import { apiFetch, apiJson } from '@/lib/api';
import { clientErrorMessage } from '@/lib/errors';

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
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

type DirectSupportResult = {
  supported: boolean;
  reason?: string;
  tooltip?: string;
};

const HLS_QUALITY_OPTIONS: Array<{ value: 'auto' | number; label: string }> = [
  { value: 'auto', label: 'Auto (Original)' },
  { value: 2160, label: '2160p (4K)' },
  { value: 1440, label: '1440p' },
  { value: 1080, label: '1080p' },
  { value: 720, label: '720p' },
  { value: 480, label: '480p' },
  { value: 360, label: '360p' },
];

const DIRECT_MIME_BY_CONTAINER: Array<[string, string]> = [
  ['mp4', 'video/mp4'],
  ['mov', 'video/quicktime'],
  ['matroska', 'video/x-matroska'],
  ['webm', 'video/webm'],
  ['mpegts', 'video/mp2t'],
  ['mpeg', 'video/mpeg'],
];

function mapCodec(codec?: string): string | null {
  if (!codec) return null;
  const c = codec.toLowerCase();
  if (c === 'h264') return 'avc1.64001F';
  if (c === 'hevc' || c === 'h265') return 'hev1';
  if (c === 'vp9') return 'vp09';
  if (c === 'av1') return 'av01';
  if (c === 'aac') return 'mp4a.40.2';
  if (c === 'opus') return 'opus';
  if (c === 'vorbis') return 'vorbis';
  return null;
}

function buildDirectContentType(info: MediaInfo | null): string | null {
  const container = info?.container?.toLowerCase() || '';
  const mime = DIRECT_MIME_BY_CONTAINER.find(([needle]) => container.includes(needle))?.[1];
  if (!mime) return null;

  const codecs: string[] = [];
  const videoCodec = mapCodec(info?.video?.codec);
  if (videoCodec) codecs.push(videoCodec);

  const firstAudio = info?.audio?.[0];
  const audioCodec = mapCodec(firstAudio?.codec);
  if (audioCodec) codecs.push(audioCodec);

  if (codecs.length > 0) {
    return `${mime}; codecs="${codecs.join(', ')}"`;
  }
  return mime;
}

function buildDirectUnsupportedMessage(contentType: string): string {
  return `Direct Play is not supported for this media type in your browser (${contentType}). Use Transcode (HLS), which is slower but compatible. To use Direct Play, add support for this media type.`;
}

function ensureAudibleVideo(video: HTMLVideoElement): void {
  if (video.muted || video.defaultMuted) {
    video.muted = false;
    video.defaultMuted = false;
  }
  if (!Number.isFinite(video.volume) || video.volume <= 0.01) {
    video.volume = 1;
  }
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

function applyKnownDurationToHlsMediaSource(hls: unknown, durationSeconds: number): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const mediaSource = (hls as { mediaSource?: MediaSource } | null)?.mediaSource;
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

export default function PlayerPage() {
  const params = useParams();
  const id = params.id as string;
  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<any>(null);

  const [mode, setMode] = useState<'direct' | 'hls'>('direct');
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [mediaInfo, setMediaInfo] = useState<MediaInfo | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [loadingDescriptor, setLoadingDescriptor] = useState(true);
  const [startingDirect, setStartingDirect] = useState(false);
  const [startingHls, setStartingHls] = useState(false);
  const [hlsTargetHeight, setHlsTargetHeight] = useState<number | null>(null);
  const [directFallbackTriggered, setDirectFallbackTriggered] = useState(false);
  const [directSupport, setDirectSupport] = useState<DirectSupportResult | null>(null);
  const [directSupportMessage, setDirectSupportMessage] = useState('');
  const [timelineNowSecs, setTimelineNowSecs] = useState(0);
  const [timelineDurationSecs, setTimelineDurationSecs] = useState(0);
  const autoStartedRef = useRef(false);

  const canStartPlayback = Boolean(descriptor?.file_id);
  const directContentType = useMemo(() => buildDirectContentType(mediaInfo), [mediaInfo]);

  const stopSession = useCallback(async (sid: string) => {
    await apiFetch(`/playback/sessions/${sid}/stop`, { method: 'POST' }).catch(() => {});
  }, []);

  const destroyHls = useCallback(() => {
    if (hlsRef.current) {
      try {
        hlsRef.current.destroy();
      } catch {
        // no-op
      }
      hlsRef.current = null;
    }
  }, []);

  const evaluateDirectSupport = useCallback(async (): Promise<DirectSupportResult> => {
    const video = videoRef.current;
    if (!video) return { supported: true };
    if (!directContentType) return { supported: true };

    const unsupported = {
      supported: false,
      reason: buildDirectUnsupportedMessage(directContentType),
      tooltip: `Media type not supported: ${directContentType}`,
    };

    const nav = navigator as Navigator & {
      mediaCapabilities?: {
        decodingInfo?: (config: any) => Promise<{ supported: boolean }>;
      };
    };

    let mediaCapabilitiesUnsupported = false;
    if (nav.mediaCapabilities?.decodingInfo && mediaInfo?.video) {
      try {
        const result = await nav.mediaCapabilities.decodingInfo({
          type: 'file',
          video: {
            contentType: directContentType,
            width: mediaInfo.video.width || 1920,
            height: mediaInfo.video.height || 1080,
            bitrate: (mediaInfo.video.bitrate_kbps || 2000) * 1000,
            framerate: mediaInfo.video.framerate || 24,
          },
        });
        mediaCapabilitiesUnsupported = !result.supported;
      } catch {
        // Fall back to canPlayType below.
      }
    }

    const canPlay = video.canPlayType(directContentType);
    if (canPlay === 'probably' || canPlay === 'maybe') return { supported: true };

    // Some browsers report unsupported for strict codec strings even when
    // container-level MP4 playback works in practice.
    const baseMime = directContentType.split(';', 1)[0]?.trim() || '';
    if (baseMime && baseMime !== directContentType) {
      const canPlayBase = video.canPlayType(baseMime);
      if (canPlayBase === 'probably' || canPlayBase === 'maybe') {
        return {
          supported: true,
          tooltip: `Codec details could not be verified (${directContentType}). Trying Direct Play.`,
        };
      }
    }

    // Keep compatibility checks advisory: we still allow trying Direct Play
    // to match watch-party behavior and only fall back if playback actually fails.
    if (mediaCapabilitiesUnsupported) return unsupported;
    return { supported: true };
  }, [directContentType, mediaInfo]);

  const startHls = useCallback(async (targetHeightOverride?: number | null) => {
    if (!descriptor?.file_id) {
      setError('No media file is attached to this item. Rescan the library and try again.');
      return;
    }

    const video = videoRef.current;
    if (!video) {
      setError('Player is not ready yet.');
      return;
    }
    const shouldResumePlayback = !video.paused;
    ensureAudibleVideo(video);
    const selectedTargetHeight =
      targetHeightOverride !== undefined ? targetHeightOverride : hlsTargetHeight;
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
    const startTimeSecs =
      finiteDuration &&
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
      setMode('hls');
      setDirectSupportMessage('');

      const Hls = (await import('hls.js')).default;
      const canNativeHls = video.canPlayType('application/vnd.apple.mpegurl') !== '';

      // Prefer hls.js when available; native HLS detection can be misleading on Chromium.
      if (Hls.isSupported()) {
        resetVideoSourceForMse(video);
        const hls = new Hls({
          enableWorker: true,
          lowLatencyMode: false,
          // Allow large prebuffer so paused/slow users can preload more than ~30s.
          maxBufferLength: 600,
          maxMaxBufferLength: 1200,
          maxBufferSize: 256 * 1000 * 1000,
          backBufferLength: 180,
          startFragPrefetch: true,
        });
        hlsRef.current = hls;
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
          if (shouldResumePlayback) {
            ensureAudibleVideo(video);
            void video.play().catch(() => {});
          } else {
            video.pause();
          }
        });
        hls.on(Hls.Events.LEVEL_LOADED, () => {
          applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
        });
        hls.on(Hls.Events.ERROR, (_event: any, data: any) => {
          if (data?.fatal) {
            setError(`HLS playback error: ${data.details || 'fatal stream error'}`);
          }
        });
        hls.attachMedia(video);
        hls.loadSource(data.hls_url);
      } else if (canNativeHls) {
        video.src = data.hls_url;
        video.load();
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

  const startDirectPlay = useCallback(async () => {
    if (!descriptor?.file_id) {
      setError('No media file is attached to this item. Rescan the library and try again.');
      return;
    }

    const video = videoRef.current;
    if (!video) {
      setError('Player is not ready yet.');
      return;
    }
    ensureAudibleVideo(video);

    setStartingDirect(true);
    setError('');
    setDirectFallbackTriggered(false);
    try {
      const support = await evaluateDirectSupport();
      setDirectSupport(support);
      if (!support.supported) {
        setDirectSupportMessage(
          support.reason ||
            'Direct Play is not supported for this media type in your browser. Use Transcode (HLS).',
        );
      } else {
        setDirectSupportMessage('');
      }

      destroyHls();
      if (sessionId) {
        await stopSession(sessionId);
        setSessionId(null);
      }
      setMode('direct');
      video.src = descriptor.direct_url;
      video.load();
    } catch (e: unknown) {
      setError(clientErrorMessage(e, 'Direct Play failed; switching to HLS.'));
      await startHls();
    } finally {
      setStartingDirect(false);
    }
  }, [descriptor, destroyHls, evaluateDirectSupport, startHls]);

  useEffect(() => {
    let cancelled = false;
    autoStartedRef.current = false;
    setLoadingDescriptor(true);
    setDescriptor(null);
    setMediaInfo(null);
    setSessionId(null);
    setError('');
    setDirectSupport(null);
    setDirectSupportMessage('');
    setTimelineNowSecs(0);
    setTimelineDurationSecs(0);

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

  useEffect(() => {
    let cancelled = false;
    if (!canStartPlayback) {
      setDirectSupport(null);
      setDirectSupportMessage('');
      return;
    }

    (async () => {
      const support = await evaluateDirectSupport();
      if (cancelled) return;
      setDirectSupport(support);
      if (!support.supported) {
        setDirectSupportMessage(
          support.reason ||
            'Direct Play is not supported for this media type in your browser. Use Transcode (HLS).',
        );
      } else {
        setDirectSupportMessage('');
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [canStartPlayback, evaluateDirectSupport]);

  // Auto-start direct play (load without playing) once the descriptor is ready
  useEffect(() => {
    if (!loadingDescriptor && canStartPlayback && !autoStartedRef.current) {
      autoStartedRef.current = true;
      void startHls().catch(() => {
        void startDirectPlay();
      });
    }
  }, [loadingDescriptor, canStartPlayback, startHls, startDirectPlay]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionId) {
        void stopSession(sessionId);
      }
    };
  }, [destroyHls, sessionId, stopSession]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const interval = setInterval(() => {
      if (video.currentTime > 0) {
        apiFetch('/playback/progress', {
          method: 'POST',
          body: JSON.stringify({
            item_id: id,
            progress_ms: Math.floor(video.currentTime * 1000),
            played: video.ended,
          }),
        }).catch(() => {});
      }
    }, 10000);
    return () => clearInterval(interval);
  }, [id]);

  const directPlayUnsupported = directSupport?.supported === false;
  const directPlayDisabled =
    !canStartPlayback || startingDirect || startingHls;
  const directPlayDisabledReason = !canStartPlayback
    ? 'No playable media file is attached to this item.'
    : directPlayUnsupported
      ? (directSupport?.tooltip ?? 'Compatibility check failed, but you can still try Direct Play.')
      : 'Use browser-native Direct Play';
  const knownDurationFromDescriptorSecs =
    descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0;
  const knownDurationFromProbeSecs =
    mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0;
  const knownDurationSecs = Math.max(knownDurationFromDescriptorSecs, knownDurationFromProbeSecs);
  const effectiveTotalDuration = Math.max(knownDurationSecs, timelineDurationSecs);

  return (
    <div className="space-y-5 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold">Player</h1>
        <p className="text-sm muted">Item ID: {id}</p>
      </header>

      {error && <p className="notice-error rounded-xl px-4 py-2 text-sm">{error}</p>}
      {directSupportMessage && (
        <p className="notice-error rounded-xl px-4 py-2 text-sm">{directSupportMessage}</p>
      )}
      {loadingDescriptor && (
        <p className="panel-soft rounded-xl px-4 py-2 text-sm muted">Preparing playback descriptor…</p>
      )}
      {!loadingDescriptor && !canStartPlayback && (
        <p className="notice-error rounded-xl px-4 py-2 text-sm">
          This item does not currently map to a playable media file. Rescan the library and retry.
        </p>
      )}

      <div className="tile overflow-hidden rounded-2xl border border-white/10 bg-black">
        <video
          ref={videoRef}
          controls
          className="w-full max-h-[80vh]"
          playsInline
          onLoadedMetadata={(event) => {
            const video = event.currentTarget;
            ensureAudibleVideo(video);
            setTimelineNowSecs(Number.isFinite(video.currentTime) ? video.currentTime : 0);
            if (Number.isFinite(video.duration) && video.duration > 0) {
              setTimelineDurationSecs((prev) => Math.max(prev, video.duration));
            }
          }}
          onPlay={(event) => {
            ensureAudibleVideo(event.currentTarget);
          }}
          onTimeUpdate={(event) => {
            const video = event.currentTarget;
            setTimelineNowSecs(Number.isFinite(video.currentTime) ? video.currentTime : 0);
            if (Number.isFinite(video.duration) && video.duration > 0) {
              setTimelineDurationSecs((prev) => Math.max(prev, video.duration));
            }
          }}
          onError={() => {
            if (mode !== 'direct' || directFallbackTriggered || !canStartPlayback) return;
            setDirectFallbackTriggered(true);
            setError('Direct Play failed in this browser. Falling back to HLS.');
            void startHls();
          }}
        />
      </div>
      <div className="px-1 text-xs muted">
        <span>
          Timeline: {formatClock(timelineNowSecs)} / {formatClock(effectiveTotalDuration)}
        </span>
        {mode === 'hls' && knownDurationSecs > 0 && timelineDurationSecs > 0 && timelineDurationSecs < knownDurationSecs - 1 ? (
          <span className="ml-2">
            (buffered window: {formatClock(timelineDurationSecs)})
          </span>
        ) : null}
      </div>

      <div className="panel-soft flex flex-wrap items-center gap-3 px-4 py-4">
        <p className="mr-2 text-sm muted">Mode:</p>
        <span className="chip">
          {mode === 'direct' ? 'Using Direct Play' : 'Using Transcode (HLS)'}
        </span>
        <button
          onClick={() => void startDirectPlay()}
          disabled={directPlayDisabled}
          title={directPlayDisabledReason}
          className={`px-4 py-2 rounded text-sm font-medium transition disabled:opacity-50 disabled:cursor-not-allowed ${
            directPlayUnsupported
              ? 'bg-amber-800/30 text-amber-200 border border-amber-500/50'
              : mode === 'direct'
                ? 'btn-primary'
                : 'btn-secondary'
          }`}
        >
          {startingDirect ? 'Starting…' : 'Direct Play'}
        </button>
        <button
          onClick={() => void startHls()}
          disabled={!canStartPlayback || startingDirect || startingHls}
          className={`px-4 py-2 rounded text-sm font-medium transition disabled:opacity-50 ${
            mode === 'hls' ? 'btn-primary' : 'btn-secondary'
          }`}
        >
          {startingHls ? 'Starting…' : 'Transcode (HLS)'}
        </button>
        <label className="ml-auto flex items-center gap-2 text-xs muted">
          <span>Quality</span>
          <select
            className="select px-2 py-1.5 text-sm"
            value={hlsTargetHeight ?? 'auto'}
            onChange={(event) => {
              const value = event.target.value;
              const nextTargetHeight = value === 'auto' ? null : Number(value);
              setHlsTargetHeight(nextTargetHeight);
              if (mode === 'hls') {
                void startHls(nextTargetHeight);
              }
            }}
            disabled={startingDirect || startingHls}
          >
            {HLS_QUALITY_OPTIONS.map((option) => (
              <option key={option.label} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        {directContentType && (
          <p className="text-xs muted">Direct capability check: {directContentType}</p>
        )}
      </div>
    </div>
  );
}
