'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { apiJson } from '@/lib/api';
import {
  AudioTrack,
  WsAudioStateMessage,
  WsOnlineAudioStatusMessage,
  YouTubeSearchResult,
  listAudioTracks,
  queueLocalAudio,
  queueOnlineAudio,
  searchOnlineAudio,
} from '@/lib/watchPartyApi';

type Props = {
  audioState: WsAudioStateMessage;
  onlineStatusEvents: WsOnlineAudioStatusMessage[];
  canControl: boolean;
  canSeek: boolean;
  roomId: string;
  sendWs: (payload: Record<string, unknown>) => void;
  musicLibraries?: { id: string; name: string }[];
  currentAudioLibraryId?: string;
  canConfigureLocalLibrary?: boolean;
  configuringLocalLibrary?: boolean;
  onConfigureLocalLibrary?: (libraryId: string) => void;
};

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
};

type RepeatMode = 'none' | 'track' | 'queue';

function formatMs(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

function formatStatusTimestamp(tsMs: number): string {
  if (!Number.isFinite(tsMs)) return '--:--:--';
  return new Date(tsMs).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

function statusBadgeClass(status: string): string {
  if (status === 'success') return 'bg-emerald-500/20 text-emerald-200';
  if (status === 'error') return 'bg-red-500/20 text-red-200';
  return 'bg-white/10 text-white/80';
}

function projectPosition(state: WsAudioStateMessage): number {
  if (!state.playing) return state.position_ms;
  const now = Date.now();
  const elapsed = now - state.server_ts_ms;
  return state.position_ms + Math.max(0, elapsed);
}

function normalizeRepeatMode(value: WsAudioStateMessage['repeat_mode']): RepeatMode {
  if (value === 'track' || value === 'queue') return value;
  return 'none';
}

function parseQueueTrackRef(trackId: string): { source: 'local' | 'online'; id: string } {
  if (trackId.startsWith('local:')) {
    return { source: 'local', id: trackId.slice('local:'.length) };
  }
  if (trackId.startsWith('online:')) {
    return { source: 'online', id: trackId.slice('online:'.length) };
  }
  // Legacy queue entries without prefix are treated as local item IDs.
  return { source: 'local', id: trackId };
}

export default function AudioPlayer({
  audioState,
  onlineStatusEvents,
  canControl,
  canSeek,
  roomId,
  sendWs,
  musicLibraries = [],
  currentAudioLibraryId = '',
  canConfigureLocalLibrary = false,
  configuringLocalLibrary = false,
  onConfigureLocalLibrary,
}: Props) {
  const [onlineSearchQuery, setOnlineSearchQuery] = useState('');
  const [localSearchQuery, setLocalSearchQuery] = useState('');
  const [librarySearchResults, setLibrarySearchResults] = useState<AudioTrack[] | null>(null);
  const [onlineSearchResults, setOnlineSearchResults] = useState<YouTubeSearchResult[] | null>(null);
  const [showOnlineSearchResults, setShowOnlineSearchResults] = useState(true);
  const [searchingOnline, setSearchingOnline] = useState(false);
  const [searchingLocal, setSearchingLocal] = useState(false);
  const [queueingVideoId, setQueueingVideoId] = useState<string | null>(null);
  const [queueingLocalTrackId, setQueueingLocalTrackId] = useState<string | null>(null);
  const [projectedPosition, setProjectedPosition] = useState(audioState.position_ms);
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [streamError, setStreamError] = useState('');
  const [actionInfo, setActionInfo] = useState('');
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const [scrubPosition, setScrubPosition] = useState(0);
  const [draggedQueueIndex, setDraggedQueueIndex] = useState<number | null>(null);

  const onlineSearchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const localSearchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const actionInfoTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queueRef = useRef<HTMLUListElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const loadedTrackKeyRef = useRef<string | null>(null);
  const hasOfflineLibrary = currentAudioLibraryId.trim().length > 0;

  // Project position forward in real time
  useEffect(() => {
    if (isScrubbing) return;
    setProjectedPosition(projectPosition(audioState));

    if (!audioState.playing) return;

    const interval = setInterval(() => {
      setProjectedPosition(projectPosition(audioState));
    }, 500);

    return () => clearInterval(interval);
  }, [audioState, isScrubbing]);

  useEffect(() => {
    let cancelled = false;

    if (!audioState.track_id) {
      setDescriptor(null);
      setStreamError('');
      loadedTrackKeyRef.current = null;
      const audio = audioRef.current;
      if (audio) {
        audio.pause();
        audio.removeAttribute('src');
        audio.load();
      }
      return;
    }

    setStreamError('');
    setAutoplayBlocked(false);

    const parsedTrack = parseQueueTrackRef(audioState.track_id);
    const activeTrack =
      parsedTrack.source === 'local' && audioState.stream_url
        ? { source: 'online' as const, id: parsedTrack.id }
        : parsedTrack;
    if (activeTrack.source === 'online') {
      setDescriptor(null);
      if (!audioState.stream_url) {
        setStreamError('Online track stream URL is not ready yet.');
      }
      return;
    }

    setDescriptor(null);
    apiJson<PlaybackDescriptor>(`/items/${activeTrack.id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
      })
      .catch((err: any) => {
        if (cancelled) return;
        setDescriptor(null);
        setStreamError(
          err?.message || 'Failed to load track stream. Verify the item has a mapped media file.',
        );
      });

    return () => {
      cancelled = true;
    };
  }, [audioState.track_id, audioState.stream_url]);

  useEffect(() => {
    return () => {
      if (onlineSearchTimeoutRef.current) {
        clearTimeout(onlineSearchTimeoutRef.current);
      }
      if (localSearchTimeoutRef.current) {
        clearTimeout(localSearchTimeoutRef.current);
      }
      if (actionInfoTimeoutRef.current) {
        clearTimeout(actionInfoTimeoutRef.current);
      }
      const audio = audioRef.current;
      if (!audio) return;
      audio.pause();
      audio.removeAttribute('src');
      audio.load();
    };
  }, []);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !audioState.track_id) return;
    const parsedTrack = parseQueueTrackRef(audioState.track_id);
    const activeTrack =
      parsedTrack.source === 'local' && audioState.stream_url
        ? { source: 'online' as const, id: parsedTrack.id }
        : parsedTrack;
    const sourceUrl =
      activeTrack.source === 'online' ? audioState.stream_url || '' : descriptor?.direct_url || '';
    if (!sourceUrl) return;

    const sourceKey = `${activeTrack.source}:${audioState.track_id}:${sourceUrl}`;
    if (loadedTrackKeyRef.current !== sourceKey) {
      loadedTrackKeyRef.current = sourceKey;
      audio.src = sourceUrl;
      audio.load();
    }

    if (!isScrubbing) {
      const targetSeconds = projectedPosition / 1000;
      if (Number.isFinite(targetSeconds) && Math.abs(audio.currentTime - targetSeconds) > 1.2) {
        audio.currentTime = targetSeconds;
      }
    }

    if (audioState.playing) {
      if (audio.paused) {
        void audio.play().then(
          () => setAutoplayBlocked(false),
          () => setAutoplayBlocked(true),
        );
      }
    } else {
      if (!audio.paused) audio.pause();
      setAutoplayBlocked(false);
    }
  }, [
    descriptor,
    audioState.track_id,
    audioState.playing,
    audioState.stream_url,
    projectedPosition,
    isScrubbing,
  ]);

  // Auto-scroll queue to current track
  useEffect(() => {
    const list = queueRef.current;
    if (!list) return;
    const activeItem = list.querySelector('[data-active="true"]') as HTMLElement | null;
    if (activeItem) {
      activeItem.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }
  }, [audioState.queue_index]);

  const handleOnlineSearch = useCallback(
    (query: string) => {
      setOnlineSearchQuery(query);

      if (onlineSearchTimeoutRef.current) {
        clearTimeout(onlineSearchTimeoutRef.current);
      }

      if (!query.trim()) {
        setOnlineSearchResults(null);
        return;
      }

      onlineSearchTimeoutRef.current = setTimeout(async () => {
        setSearchingOnline(true);
        try {
          const results = await searchOnlineAudio(roomId, query, 12);
          setOnlineSearchResults(results);
        } catch {
          setOnlineSearchResults([]);
        } finally {
          setSearchingOnline(false);
        }
      }, 300);
    },
    [roomId],
  );

  const handleLocalSearch = useCallback(
    (query: string) => {
      setLocalSearchQuery(query);

      if (localSearchTimeoutRef.current) {
        clearTimeout(localSearchTimeoutRef.current);
      }

      if (!hasOfflineLibrary) {
        setLibrarySearchResults([]);
        return;
      }

      if (!query.trim()) {
        setLibrarySearchResults(null);
        return;
      }

      localSearchTimeoutRef.current = setTimeout(async () => {
        setSearchingLocal(true);
        try {
          const results = await listAudioTracks(roomId, query, 'local');
          setLibrarySearchResults(results);
        } catch {
          setLibrarySearchResults([]);
        } finally {
          setSearchingLocal(false);
        }
      }, 300);
    },
    [hasOfflineLibrary, roomId],
  );

  const clearOnlineSearch = useCallback(() => {
    if (onlineSearchTimeoutRef.current) {
      clearTimeout(onlineSearchTimeoutRef.current);
      onlineSearchTimeoutRef.current = null;
    }
    setOnlineSearchQuery('');
    setSearchingOnline(false);
    setOnlineSearchResults(null);
    setShowOnlineSearchResults(true);
  }, []);

  const clearLocalSearch = useCallback(() => {
    if (localSearchTimeoutRef.current) {
      clearTimeout(localSearchTimeoutRef.current);
      localSearchTimeoutRef.current = null;
    }
    setLocalSearchQuery('');
    setSearchingLocal(false);
    setLibrarySearchResults(null);
  }, []);

  const handleSkipPrev = () => sendWs({ type: 'skip_prev' });
  const handleSkipNext = () => sendWs({ type: 'skip_next' });
  const handlePlayPause = () => {
    if (audioState.playing) {
      sendWs({ type: 'pause', position_ms: projectedPosition });
    } else {
      sendWs({ type: 'play', position_ms: projectedPosition });
    }
  };
  const handlePlayTrack = (trackId: string) => sendWs({ type: 'play_track', track_id: trackId });
  const handleQueueOnlineTrack = useCallback(
    async (videoId: string, playNow: boolean) => {
      setQueueingVideoId(`${videoId}:${playNow ? 'play' : 'queue'}`);
      setStreamError('');
      setActionInfo('');
      try {
        const response = await queueOnlineAudio(roomId, videoId, playNow);
        if (response.already_downloaded) {
          const infoText = playNow
            ? 'Using cached room audio. Playing immediately without re-downloading.'
            : 'Track already downloaded for this room. Added from cache.';
          setActionInfo(infoText);
          if (actionInfoTimeoutRef.current) {
            clearTimeout(actionInfoTimeoutRef.current);
          }
          actionInfoTimeoutRef.current = setTimeout(() => {
            setActionInfo((current) => (current === infoText ? '' : current));
            actionInfoTimeoutRef.current = null;
          }, 5000);
        }
      } catch (err: any) {
        setStreamError(err?.message || 'Failed to queue online audio track');
      } finally {
        setQueueingVideoId(null);
      }
    },
    [roomId],
  );
  const handleQueueLocalTrack = useCallback(
    async (trackId: string, playNow: boolean) => {
      setQueueingLocalTrackId(`${trackId}:${playNow ? 'play' : 'queue'}`);
      setStreamError('');
      setActionInfo('');
      try {
        const response = await queueLocalAudio(roomId, trackId, playNow);
        if (response.already_queued) {
          const infoText = playNow
            ? 'Track already existed in queue. Jumped playback to this track.'
            : 'Track already exists in the room queue.';
          setActionInfo(infoText);
          if (actionInfoTimeoutRef.current) {
            clearTimeout(actionInfoTimeoutRef.current);
          }
          actionInfoTimeoutRef.current = setTimeout(() => {
            setActionInfo((current) => (current === infoText ? '' : current));
            actionInfoTimeoutRef.current = null;
          }, 5000);
        }
      } catch (err: any) {
        setStreamError(err?.message || 'Failed to queue local track');
      } finally {
        setQueueingLocalTrackId(null);
      }
    },
    [roomId],
  );
  const duration = audioState.duration_ms ?? 0;
  const effectivePosition = isScrubbing ? scrubPosition : projectedPosition;

  const commitSeek = (positionMs: number) => {
    if (!canSeek || duration <= 0) return;
    const clamped = Math.max(0, Math.min(duration, Math.floor(positionMs)));
    setProjectedPosition(clamped);
    setScrubPosition(clamped);
    setIsScrubbing(false);
    sendWs({ type: 'seek', position_ms: clamped });
  };

  const handleSeekStart = () => {
    if (!canSeek || duration <= 0) return;
    setIsScrubbing(true);
    setScrubPosition(effectivePosition);
  };

  const handleSeekChange = (value: string) => {
    if (!canSeek || duration <= 0) return;
    const next = Number(value);
    if (!Number.isFinite(next)) return;
    setIsScrubbing(true);
    setScrubPosition(next);
  };

  const handleSeekCommit = (value: string) => {
    if (!canSeek || duration <= 0) return;
    const next = Number(value);
    if (!Number.isFinite(next)) return;
    commitSeek(next);
  };

  const handleEnableAudio = () => {
    const audio = audioRef.current;
    if (!audio) return;
    void audio.play().then(
      () => setAutoplayBlocked(false),
      () => setAutoplayBlocked(true),
    );
  };
  const repeatMode = normalizeRepeatMode(audioState.repeat_mode);
  const shuffleEnabled = !!audioState.shuffle_enabled;
  const repeatLabel = repeatMode === 'track' ? 'Song' : repeatMode === 'queue' ? 'Queue' : 'Off';

  const handleToggleShuffle = useCallback(() => {
    if (!canControl) return;
    sendWs({ type: 'set_audio_shuffle', enabled: !shuffleEnabled });
  }, [canControl, sendWs, shuffleEnabled]);

  const handleCycleRepeat = useCallback(() => {
    if (!canControl) return;
    const next: RepeatMode =
      repeatMode === 'none' ? 'track' : repeatMode === 'track' ? 'queue' : 'none';
    sendWs({ type: 'set_audio_repeat_mode', mode: next });
  }, [canControl, repeatMode, sendWs]);

  const handleTrackEnded = useCallback(() => {
    if (!canControl) return;
    sendWs({
      type: 'track_ended',
      position_ms: duration > 0 ? duration : Math.max(0, Math.floor(effectivePosition)),
    });
  }, [canControl, sendWs, duration, effectivePosition]);

  const handleQueueDragStart = useCallback(
    (index: number, event: React.DragEvent<HTMLLIElement>) => {
      if (!canControl) return;
      setDraggedQueueIndex(index);
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', String(index));
    },
    [canControl],
  );

  const handleQueueDragOver = useCallback(
    (index: number, event: React.DragEvent<HTMLLIElement>) => {
      if (!canControl || draggedQueueIndex === null || draggedQueueIndex === index) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = 'move';
    },
    [canControl, draggedQueueIndex],
  );

  const handleQueueDrop = useCallback(
    (toIndex: number, event: React.DragEvent<HTMLLIElement>) => {
      if (!canControl) return;
      event.preventDefault();
      const fromRaw = event.dataTransfer.getData('text/plain');
      const parsedFrom = Number.parseInt(fromRaw, 10);
      const fromIndex = Number.isFinite(parsedFrom) ? parsedFrom : draggedQueueIndex;
      setDraggedQueueIndex(null);

      if (
        fromIndex === null ||
        fromIndex === toIndex ||
        fromIndex < 0 ||
        toIndex < 0 ||
        fromIndex >= audioState.queue.length ||
        toIndex >= audioState.queue.length
      ) {
        return;
      }

      sendWs({
        type: 'reorder_audio_queue',
        from_index: fromIndex,
        to_index: toIndex,
      });
    },
    [canControl, draggedQueueIndex, audioState.queue.length, sendWs],
  );

  const handleQueueDragEnd = useCallback(() => {
    setDraggedQueueIndex(null);
  }, []);

  const latestOnlineStatusEvent = useMemo<WsOnlineAudioStatusMessage | null>(() => {
    if (onlineStatusEvents.length === 0) return null;
    return onlineStatusEvents[onlineStatusEvents.length - 1] ?? null;
  }, [onlineStatusEvents]);

  useEffect(() => {
    setLocalSearchQuery('');
    setLibrarySearchResults(null);
    setSearchingLocal(false);
  }, [currentAudioLibraryId]);

  return (
    <div className="space-y-4">
      <audio
        ref={audioRef}
        preload="auto"
        onEnded={handleTrackEnded}
        onError={() => {
          setStreamError(
            'Unable to play this track in the browser via direct stream. Use supported audio codecs for direct playback.',
          );
        }}
      />
      {/* Now Playing */}
      <section className="panel p-5 sm:p-6">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:gap-6">
          {/* Album Art */}
          <div className="mx-auto flex-shrink-0 sm:mx-0">
            {audioState.album_art_url ? (
              <img
                src={audioState.album_art_url}
                alt={audioState.album || 'Album art'}
                className="h-40 w-40 rounded-xl object-cover shadow-md sm:h-48 sm:w-48"
              />
            ) : (
              <div className="flex h-40 w-40 items-center justify-center rounded-xl bg-white/5 sm:h-48 sm:w-48">
                <span className="text-lg opacity-50">No artwork</span>
              </div>
            )}
          </div>

          {/* Track info + controls */}
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            <div>
              <p className="truncate text-xl font-semibold">{audioState.title || 'Unknown Track'}</p>
              <p className="truncate text-sm muted">
                {audioState.artist || 'Unknown Artist'}
                {audioState.album ? ` • ${audioState.album}` : ''}
              </p>
            </div>

            {/* Timeline */}
            <div className="space-y-1">
              <input
                type="range"
                min={0}
                max={duration > 0 ? duration : 0}
                step={500}
                value={duration > 0 ? effectivePosition : 0}
                onMouseDown={handleSeekStart}
                onTouchStart={handleSeekStart}
                onChange={(e) => handleSeekChange(e.target.value)}
                onMouseUp={(e) => handleSeekCommit(e.currentTarget.value)}
                onTouchEnd={(e) => handleSeekCommit(e.currentTarget.value)}
                onKeyUp={(e) => handleSeekCommit((e.currentTarget as HTMLInputElement).value)}
                onBlur={(e) => {
                  if (!isScrubbing) return;
                  handleSeekCommit(e.currentTarget.value);
                }}
                disabled={!canSeek || duration <= 0}
                className="w-full accent-[var(--orange-soft)] disabled:opacity-40"
                aria-label="Seek timeline"
              />
              <div className="flex justify-between text-xs muted">
                <span>{formatMs(effectivePosition)}</span>
                <span>{duration > 0 ? formatMs(duration) : '--:--'}</span>
              </div>
              {!canSeek && (
                <div className="text-xs muted">Seeking is host-only in this room.</div>
              )}
            </div>

            {/* Playback controls */}
            <div className="flex flex-col items-center gap-2">
              <div className="flex flex-wrap items-center justify-center gap-3">
                <button
                  type="button"
                  onClick={handleSkipPrev}
                  disabled={!canControl}
                  className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 disabled:opacity-40"
                  title="Previous"
                >
                  <svg
                    viewBox="0 0 24 24"
                    className="h-4 w-4"
                    fill="currentColor"
                    aria-hidden="true"
                  >
                    <path d="M11.8 6.2c.7-.5 1.7 0 1.7.9V17c0 .9-1 1.4-1.7.9l-6-4.9a1.1 1.1 0 0 1 0-1.7l6-5.1Z" />
                    <path d="M18.2 6.2c.7-.5 1.7 0 1.7.9V17c0 .9-1 1.4-1.7.9l-6-4.9a1.1 1.1 0 0 1 0-1.7l6-5.1Z" />
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={handlePlayPause}
                  disabled={!canControl}
                  className="btn-primary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 disabled:opacity-40"
                  title={audioState.playing ? 'Pause' : 'Play'}
                >
                  {audioState.playing ? (
                    <svg
                      viewBox="0 0 24 24"
                      className="h-4 w-4"
                      fill="currentColor"
                      aria-hidden="true"
                    >
                      <rect x="6.5" y="5.5" width="4" height="13" rx="1" />
                      <rect x="13.5" y="5.5" width="4" height="13" rx="1" />
                    </svg>
                  ) : (
                    <svg
                      viewBox="0 0 24 24"
                      className="h-4 w-4"
                      fill="currentColor"
                      aria-hidden="true"
                    >
                      <path d="M8 5.8c0-.9 1-1.5 1.8-1l8.6 5.3c.8.5.8 1.6 0 2.1l-8.6 5.3c-.8.5-1.8-.1-1.8-1V5.8z" />
                    </svg>
                  )}
                </button>
                <button
                  type="button"
                  onClick={handleSkipNext}
                  disabled={!canControl}
                  className="btn-secondary inline-flex h-10 w-10 items-center justify-center rounded-full p-0 disabled:opacity-40"
                  title="Next"
                >
                  <svg
                    viewBox="0 0 24 24"
                    className="h-4 w-4"
                    fill="currentColor"
                    aria-hidden="true"
                  >
                    <path d="M12.2 6.2c-.7-.5-1.7 0-1.7.9V17c0 .9 1 1.4 1.7.9l6-4.9a1.1 1.1 0 0 0 0-1.7l-6-5.1Z" />
                    <path d="M5.8 6.2c-.7-.5-1.7 0-1.7.9V17c0 .9 1 1.4 1.7.9l6-4.9a1.1 1.1 0 0 0 0-1.7l-6-5.1Z" />
                  </svg>
                </button>
              </div>
              <div className="flex flex-wrap items-center justify-center gap-2">
                <button
                  type="button"
                  onClick={handleToggleShuffle}
                  disabled={!canControl}
                  className={`inline-flex h-10 w-10 items-center justify-center rounded-full p-0 disabled:opacity-40 ${shuffleEnabled ? 'btn-primary' : 'btn-secondary'}`}
                  title={`Shuffle ${shuffleEnabled ? 'on' : 'off'}`}
                  aria-label={`Shuffle ${shuffleEnabled ? 'on' : 'off'}`}
                >
                  <svg
                    viewBox="0 0 24 24"
                    className="h-[18px] w-[18px]"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    shapeRendering="geometricPrecision"
                    aria-hidden="true"
                  >
                    <path d="M4 7h2.5c1.7 0 2.6.6 3.8 2.2l4 5.6c1.2 1.6 2.1 2.2 3.8 2.2H20" />
                    <path d="M4 17h2.5c1.7 0 2.6-.6 3.8-2.2l1-1.4" />
                    <path d="M17 4l3 3-3 3" />
                    <path d="M17 14l3 3-3 3" />
                    {!shuffleEnabled && (
                      <path d="M4.5 4.5l15 15" />
                    )}
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={handleCycleRepeat}
                  disabled={!canControl}
                  className={`relative inline-flex h-10 w-10 items-center justify-center rounded-full p-0 disabled:opacity-40 ${repeatMode === 'none' ? 'btn-secondary' : 'btn-primary'}`}
                  title={`Repeat mode: ${repeatLabel}. Click to cycle (Off → Song → Queue)`}
                  aria-label={`Repeat mode: ${repeatLabel}`}
                >
                  <svg
                    viewBox="0 0 24 24"
                    className="h-[18px] w-[18px]"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    shapeRendering="geometricPrecision"
                    aria-hidden="true"
                  >
                    <path d="M7 7h9a3 3 0 0 1 3 3v1" />
                    <path d="M16 5l4 3-4 3" />
                    <path d="M17 17H8a3 3 0 0 1-3-3v-1" />
                    <path d="M8 15l-4 3 4 3" />
                    {repeatMode === 'none' && (
                      <path d="M4.5 4.5l15 15" />
                    )}
                  </svg>
                  {repeatMode === 'track' && (
                    <span className="absolute -right-1 -top-1 inline-flex h-4 min-w-4 items-center justify-center rounded-full border border-[var(--border)] bg-black/75 px-1 text-[10px] font-semibold leading-none">
                      1
                    </span>
                  )}
                  {repeatMode === 'queue' && (
                    <span className="absolute -right-1 -top-1 inline-flex h-4 w-4 items-center justify-center rounded-full border border-[var(--border)] bg-black/75">
                      <svg viewBox="0 0 12 12" className="h-2.5 w-2.5" fill="none" aria-hidden="true">
                        <path d="M2 3h8M2 6h8M2 9h8" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                      </svg>
                    </span>
                  )}
                </button>
              </div>
              {!canControl && (
                <span className="text-center text-xs muted">Controls are host-only in this room.</span>
              )}
            </div>

            {autoplayBlocked && (
              <div className="notice-error rounded-xl px-3 py-2 text-xs">
                Browser autoplay blocked audio output. Click Enable Audio to continue listening.
                <button
                  type="button"
                  className="btn-secondary ml-3 px-3 py-1 text-xs"
                  onClick={handleEnableAudio}
                >
                  Enable Audio
                </button>
              </div>
            )}
            {streamError && (
              <div className="notice-error rounded-xl px-3 py-2 text-xs">{streamError}</div>
            )}
            {actionInfo && (
              <div className="notice-ok rounded-xl px-3 py-2 text-xs">{actionInfo}</div>
            )}
          </div>
        </div>
      </section>

      {/* Queue & Search */}
      <section className="panel space-y-3 p-5 sm:p-6">
        <div className="panel-soft space-y-2 rounded-xl px-3 py-3">
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs uppercase tracking-wide muted">Online Pipeline Status</p>
          </div>
          {!latestOnlineStatusEvent ? (
            <p className="text-xs muted">
              No pipeline updates yet. Search and queue a track to start the online download flow.
            </p>
          ) : (
            <div className="tile flex items-start gap-2 rounded-lg px-2 py-1.5">
              <span className="mt-0.5 text-[11px] muted">
                {formatStatusTimestamp(latestOnlineStatusEvent.updated_ts_ms)}
              </span>
              <span
                className={`rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide ${statusBadgeClass(
                  latestOnlineStatusEvent.status,
                )}`}
              >
                {latestOnlineStatusEvent.status}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-[11px] uppercase tracking-wide muted">
                  {latestOnlineStatusEvent.stage.replaceAll('_', ' ')}
                </p>
                <p className="text-xs leading-snug">{latestOnlineStatusEvent.message}</p>
              </div>
            </div>
          )}
        </div>

        <div className="grid gap-4 lg:grid-cols-2 lg:items-start">
          <div className="space-y-4">
            <div className="space-y-3">
              <p className="text-xs uppercase tracking-wide muted">Online Search</p>
              <div className="flex items-center gap-2">
                <div className="relative flex-1">
                  <input
                    value={onlineSearchQuery}
                    onChange={(e) => handleOnlineSearch(e.target.value)}
                    className="input w-full px-3 py-1.5 pr-10 text-sm"
                    placeholder="Search YouTube tracks…"
                    aria-label="Search online tracks"
                  />
                  {onlineSearchQuery.trim().length > 0 && (
                    <button
                      type="button"
                      onClick={clearOnlineSearch}
                      className="absolute right-2 top-1/2 inline-flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-full border border-white/25 text-white/75 transition hover:border-white/50 hover:text-white"
                      aria-label="Clear search"
                      title="Clear search"
                    >
                      <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
                        <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.8" />
                        <path
                          d="M9 9l6 6M15 9l-6 6"
                          stroke="currentColor"
                          strokeWidth="1.8"
                          strokeLinecap="round"
                        />
                      </svg>
                    </button>
                  )}
                </div>
                <button
                  type="button"
                  className="btn-secondary shrink-0 px-3 py-1.5 text-xs"
                  onClick={() => setShowOnlineSearchResults((prev) => !prev)}
                >
                  {showOnlineSearchResults ? 'Hide Results ▴' : 'Show Results ▾'}
                </button>
              </div>

              {searchingOnline && (
                <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">Searching…</div>
              )}

              {!searchingOnline && onlineSearchResults !== null && onlineSearchResults.length === 0 && (
                <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">No online tracks found.</div>
              )}

              {!searchingOnline &&
                showOnlineSearchResults &&
                onlineSearchResults !== null &&
                onlineSearchResults.length > 0 && (
                  <ul className="max-h-56 space-y-2 overflow-y-auto">
                    {onlineSearchResults.map((result) => {
                      const queueKey = `${result.video_id}:queue`;
                      const playKey = `${result.video_id}:play`;
                      const busy = queueingVideoId === queueKey || queueingVideoId === playKey;
                      return (
                        <li key={result.video_id} className="tile rounded-xl px-3 py-2">
                          <div className="flex items-center gap-3">
                            <img
                              src={result.thumbnail_url}
                              alt={result.title}
                              className="h-12 w-20 rounded-md object-cover"
                            />
                            <div className="min-w-0 flex-1">
                              <p className="truncate text-sm font-medium">{result.title}</p>
                              <p className="truncate text-xs muted">{result.channel}</p>
                            </div>
                            <div className="flex items-center gap-2">
                              <button
                                type="button"
                                className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
                                disabled={busy}
                                onClick={() => void handleQueueOnlineTrack(result.video_id, false)}
                              >
                                Queue
                              </button>
                              <button
                                type="button"
                                className="btn-primary px-3 py-1 text-xs disabled:opacity-50"
                                disabled={busy || !canControl}
                                onClick={() => void handleQueueOnlineTrack(result.video_id, true)}
                              >
                                Play now
                              </button>
                            </div>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
            </div>

            <div className="space-y-3">
              <p className="text-xs uppercase tracking-wide muted">Offline Search</p>
              {canConfigureLocalLibrary && (
                <div className="space-y-2">
                  <label
                    htmlFor="offline-audio-library"
                    className="block text-xs uppercase tracking-wide muted"
                  >
                    Offline Library
                  </label>
                  <select
                    id="offline-audio-library"
                    className="select px-3 py-1.5 text-sm"
                    value={currentAudioLibraryId}
                    onChange={(event) => onConfigureLocalLibrary?.(event.target.value)}
                    disabled={configuringLocalLibrary}
                  >
                    <option value="">No local library (online only)</option>
                    {musicLibraries.map((library) => (
                      <option key={library.id} value={library.id}>
                        {library.name}
                      </option>
                    ))}
                  </select>
                  {configuringLocalLibrary && (
                    <p className="text-xs muted">Applying offline library…</p>
                  )}
                </div>
              )}
              {!canConfigureLocalLibrary && hasOfflineLibrary && (
                <div className="panel-soft rounded-xl px-3 py-2 text-xs muted">
                  Offline library active in this room.
                </div>
              )}
              {!canConfigureLocalLibrary && !hasOfflineLibrary && (
                <div className="panel-soft rounded-xl px-3 py-2 text-xs muted">
                  No offline library configured for this room.
                </div>
              )}
              <div className="relative">
                <input
                  value={localSearchQuery}
                  onChange={(e) => handleLocalSearch(e.target.value)}
                  className="input w-full px-3 py-1.5 pr-10 text-sm"
                  placeholder={
                    hasOfflineLibrary
                      ? 'Search local tracks…'
                      : 'Select an offline library to search local tracks…'
                  }
                  aria-label="Search local tracks"
                  disabled={!hasOfflineLibrary}
                />
                {localSearchQuery.trim().length > 0 && (
                  <button
                    type="button"
                    onClick={clearLocalSearch}
                    className="absolute right-2 top-1/2 inline-flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-full border border-white/25 text-white/75 transition hover:border-white/50 hover:text-white"
                    aria-label="Clear search"
                    title="Clear search"
                  >
                    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
                      <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.8" />
                      <path
                        d="M9 9l6 6M15 9l-6 6"
                        stroke="currentColor"
                        strokeWidth="1.8"
                        strokeLinecap="round"
                      />
                    </svg>
                  </button>
                )}
              </div>

              {searchingLocal && (
                <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">Searching…</div>
              )}

              {!searchingLocal && librarySearchResults !== null && librarySearchResults.length === 0 && (
                <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">
                  {hasOfflineLibrary
                    ? 'No local tracks found.'
                    : 'No local library configured for this room.'}
                </div>
              )}

              {!searchingLocal && librarySearchResults !== null && librarySearchResults.length > 0 && (
                <ul className="max-h-56 space-y-2 overflow-y-auto">
                  {librarySearchResults.map((track) => {
                    const queueKey = `${track.id}:queue`;
                    const playKey = `${track.id}:play`;
                    const busy =
                      queueingLocalTrackId === queueKey || queueingLocalTrackId === playKey;
                    return (
                      <li key={track.id} className="tile rounded-xl px-3 py-2">
                        <div className="flex items-center gap-3">
                          <div className="min-w-0 flex-1">
                            <p className="truncate text-sm font-medium">{track.title}</p>
                            <p className="truncate text-xs muted">
                              {track.artist}
                              {track.album ? ` • ${track.album}` : ''}
                              {track.duration_ms ? ` • ${formatMs(track.duration_ms)}` : ''}
                            </p>
                          </div>
                          <div className="flex items-center gap-2">
                            <button
                              type="button"
                              className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
                              disabled={busy}
                              onClick={() => void handleQueueLocalTrack(track.id, false)}
                            >
                              Queue
                            </button>
                            <button
                              type="button"
                              className="btn-primary px-3 py-1 text-xs disabled:opacity-50"
                              disabled={busy || !canControl}
                              onClick={() => void handleQueueLocalTrack(track.id, true)}
                            >
                              Play now
                            </button>
                          </div>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>

          <div className="space-y-3">
            <p className="text-xs uppercase tracking-wide muted">Room Queue</p>
            {audioState.queue.length > 0 ? (
              <ul ref={queueRef} className="max-h-[34rem] space-y-1 overflow-y-auto">
                {audioState.queue.map((entry, idx) => {
                  const isActive = idx === audioState.queue_index;
                  return (
                    <li
                      key={entry.track_id}
                      data-active={isActive ? 'true' : 'false'}
                      className={`tile rounded-xl px-3 py-2 ${isActive ? 'border-[var(--orange-soft)]' : ''} ${canControl ? 'cursor-move' : ''} ${draggedQueueIndex === idx ? 'border-dashed border-[var(--purple-strong)] opacity-70' : ''}`}
                      style={{ boxShadow: 'none' }}
                      draggable={canControl}
                      onDragStart={(event) => handleQueueDragStart(idx, event)}
                      onDragOver={(event) => handleQueueDragOver(idx, event)}
                      onDrop={(event) => handleQueueDrop(idx, event)}
                      onDragEnd={handleQueueDragEnd}
                    >
                      <div className="flex items-center gap-3">
                        <span className="w-5 flex-shrink-0 text-center text-xs muted">
                          {isActive ? '►' : idx + 1}
                        </span>
                        <div className="min-w-0 flex-1">
                          <p className="truncate text-sm font-medium">{entry.title}</p>
                          <p className="truncate text-xs muted">
                            {entry.artist}
                            {entry.album ? ` • ${entry.album}` : ''}
                            {entry.duration_ms ? ` • ${formatMs(entry.duration_ms)}` : ''}
                          </p>
                        </div>
                        {canControl && (
                          <button
                            type="button"
                            onClick={() => handlePlayTrack(entry.track_id)}
                            className={`px-3 py-1 text-xs ${isActive ? 'btn-primary' : 'btn-secondary'}`}
                          >
                            {isActive ? 'Playing' : 'Play'}
                          </button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">
                Room queue is empty.
              </div>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
