'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams } from 'next/navigation';
import { apiFetch, apiJson } from '@/lib/api';

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
  hls_start_url: string;
  media_info_url: string;
};

type PlaybackSession = {
  session_id: string;
  hls_url: string;
};

type MediaInfo = {
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
  const [directFallbackTriggered, setDirectFallbackTriggered] = useState(false);

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

  const evaluateDirectSupport = useCallback(async () => {
    const video = videoRef.current;
    if (!video) return true;
    if (!directContentType) return true;

    const nav = navigator as Navigator & {
      mediaCapabilities?: {
        decodingInfo?: (config: any) => Promise<{ supported: boolean }>;
      };
    };

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
        if (!result.supported) return false;
      } catch {
        // Fall back to canPlayType below.
      }
    }

    const canPlay = video.canPlayType(directContentType);
    return canPlay === 'probably' || canPlay === 'maybe';
  }, [directContentType, mediaInfo]);

  const startHls = useCallback(async () => {
    if (!descriptor?.file_id) {
      setError('No media file is attached to this item. Rescan the library and try again.');
      return;
    }

    const video = videoRef.current;
    if (!video) {
      setError('Player is not ready yet.');
      return;
    }

    setStartingHls(true);
    setError('');
    try {
      if (sessionId) {
        await stopSession(sessionId);
        setSessionId(null);
      }

      const data = await apiJson<PlaybackSession>(descriptor.hls_start_url, {
        method: 'POST',
        body: JSON.stringify({ file_id: descriptor.file_id }),
      });

      destroyHls();
      setSessionId(data.session_id);
      setMode('hls');

      if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = data.hls_url;
        await video.play().catch(() => {});
      } else {
        const Hls = (await import('hls.js')).default;
        if (!Hls.isSupported()) {
          throw new Error('HLS playback is not supported in this browser.');
        }
        const hls = new Hls();
        hlsRef.current = hls;
        hls.on(Hls.Events.ERROR, (_event: any, data: any) => {
          if (data?.fatal) {
            setError(`HLS playback error: ${data.details || 'fatal stream error'}`);
          }
        });
        hls.loadSource(data.hls_url);
        hls.attachMedia(video);
      }
    } catch (e: any) {
      setError(e.message || 'Failed to start HLS playback.');
    } finally {
      setStartingHls(false);
    }
  }, [descriptor, destroyHls, sessionId, stopSession]);

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

    setStartingDirect(true);
    setError('');
    setDirectFallbackTriggered(false);
    try {
      const canDirect = await evaluateDirectSupport();
      if (!canDirect) {
        setError('Direct Play is not supported for this media in your browser. Switching to HLS.');
        await startHls();
        return;
      }

      destroyHls();
      setMode('direct');
      video.src = descriptor.direct_url;
      await video.play().catch(() => {});
    } catch (e: any) {
      setError(e.message || 'Direct Play failed; switching to HLS.');
      await startHls();
    } finally {
      setStartingDirect(false);
    }
  }, [descriptor, destroyHls, evaluateDirectSupport, startHls]);

  useEffect(() => {
    let cancelled = false;
    setLoadingDescriptor(true);
    setDescriptor(null);
    setMediaInfo(null);
    setSessionId(null);
    setError('');

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
      .catch((e: any) => {
        if (!cancelled) setError(e.message || 'Failed to load playback descriptor.');
      })
      .finally(() => {
        if (!cancelled) setLoadingDescriptor(false);
      });

    return () => {
      cancelled = true;
    };
  }, [id]);

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

  return (
    <div className="space-y-5 animate-rise">
      <header className="space-y-2">
        <span className="chip">Playback Console</span>
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

      <div className="tile overflow-hidden rounded-2xl border border-white/10 bg-black">
        <video
          ref={videoRef}
          controls
          autoPlay
          className="w-full max-h-[80vh]"
          playsInline
          onError={() => {
            if (mode !== 'direct' || directFallbackTriggered || !canStartPlayback) return;
            setDirectFallbackTriggered(true);
            setError('Direct Play failed in this browser. Falling back to HLS.');
            void startHls();
          }}
        />
      </div>

      <div className="panel-soft flex flex-wrap items-center gap-3 px-4 py-4">
        <p className="mr-2 text-sm muted">Mode:</p>
        <button
          onClick={() => void startDirectPlay()}
          disabled={!canStartPlayback || startingDirect || startingHls}
          className={`px-4 py-2 rounded text-sm font-medium transition disabled:opacity-50 ${
            mode === 'direct' ? 'btn-primary' : 'btn-secondary'
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
        {directContentType && (
          <p className="text-xs muted">Direct capability check: {directContentType}</p>
        )}
      </div>
    </div>
  );
}
