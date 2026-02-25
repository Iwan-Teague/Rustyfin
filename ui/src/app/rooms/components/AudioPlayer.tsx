'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { apiJson } from '@/lib/api';
import {
  AudioTrack,
  QueueEntry,
  WsAudioStateMessage,
  WsOnlineAudioStatusMessage,
  YouTubeSearchResult,
  listAudioTracks,
  queueOnlineAudio,
  searchOnlineAudio,
} from '@/lib/watchPartyApi';

type Props = {
  audioState: WsAudioStateMessage;
  audioSource: 'library' | 'online';
  onlineStatusEvents: WsOnlineAudioStatusMessage[];
  canControl: boolean;
  canSeek: boolean;
  roomId: string;
  sendWs: (payload: Record<string, unknown>) => void;
};

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
};

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

export default function AudioPlayer({
  audioState,
  audioSource,
  onlineStatusEvents,
  canControl,
  canSeek,
  roomId,
  sendWs,
}: Props) {
  const [searchQuery, setSearchQuery] = useState('');
  const [librarySearchResults, setLibrarySearchResults] = useState<AudioTrack[] | null>(null);
  const [onlineSearchResults, setOnlineSearchResults] = useState<YouTubeSearchResult[] | null>(null);
  const [showOnlineSearchResults, setShowOnlineSearchResults] = useState(true);
  const [searching, setSearching] = useState(false);
  const [queueingVideoId, setQueueingVideoId] = useState<string | null>(null);
  const [projectedPosition, setProjectedPosition] = useState(audioState.position_ms);
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [streamError, setStreamError] = useState('');
  const [actionInfo, setActionInfo] = useState('');
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const [scrubPosition, setScrubPosition] = useState(0);

  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const actionInfoTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queueRef = useRef<HTMLUListElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const loadedTrackKeyRef = useRef<string | null>(null);

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

    if (audioSource === 'online') {
      setDescriptor(null);
      if (!audioState.stream_url) {
        setStreamError('Online track stream URL is not ready yet.');
      }
      return;
    }

    setDescriptor(null);
    apiJson<PlaybackDescriptor>(`/items/${audioState.track_id}/playback`)
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
  }, [audioState.track_id, audioState.stream_url, audioSource]);

  useEffect(() => {
    return () => {
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
    setSearchQuery('');
    setLibrarySearchResults(null);
    setOnlineSearchResults(null);
    setShowOnlineSearchResults(true);
  }, [audioSource]);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !audioState.track_id) return;
    const sourceUrl = audioSource === 'online' ? audioState.stream_url || '' : descriptor?.direct_url || '';
    if (!sourceUrl) return;

    const sourceKey = `${audioSource}:${audioState.track_id}:${sourceUrl}`;
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
    audioSource,
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

  const handleSearch = useCallback(
    (query: string) => {
      setSearchQuery(query);

      if (searchTimeoutRef.current) {
        clearTimeout(searchTimeoutRef.current);
      }

      if (!query.trim()) {
        setLibrarySearchResults(null);
        setOnlineSearchResults(null);
        return;
      }

      searchTimeoutRef.current = setTimeout(async () => {
        setSearching(true);
        try {
          if (audioSource === 'online') {
            const results = await searchOnlineAudio(roomId, query, 12);
            setOnlineSearchResults(results);
          } else {
            const results = await listAudioTracks(roomId, query);
            setLibrarySearchResults(results);
          }
        } catch {
          if (audioSource === 'online') {
            setOnlineSearchResults([]);
          } else {
            setLibrarySearchResults([]);
          }
        } finally {
          setSearching(false);
        }
      }, 300);
    },
    [roomId, audioSource],
  );

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

  const progressPct = duration > 0 ? Math.min(100, (effectivePosition / duration) * 100) : 0;
  const recentOnlineStatusEvents = useMemo(() => {
    if (onlineStatusEvents.length === 0) return [];
    return [...onlineStatusEvents].reverse().slice(0, 8);
  }, [onlineStatusEvents]);

  const displayList: Array<QueueEntry & { isSearchResult?: boolean }> =
    librarySearchResults !== null
      ? librarySearchResults.map((t) => ({ ...t, track_id: t.id, isSearchResult: true }))
      : audioState.queue;

  return (
    <div className="space-y-4">
      <audio
        ref={audioRef}
        preload="auto"
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

            {/* Progress bar */}
            <div className="space-y-1">
              <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-white/10">
                <div
                  className="absolute left-0 top-0 h-full rounded-full bg-[var(--orange-soft)]"
                  style={{ width: `${progressPct}%` }}
                />
              </div>
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
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={handleSkipPrev}
                disabled={!canControl}
                className="btn-secondary rounded-full px-3 py-2 text-sm disabled:opacity-40"
                title="Previous"
              >
                ◄◄
              </button>
              <button
                type="button"
                onClick={handlePlayPause}
                disabled={!canControl}
                className="btn-primary rounded-full px-5 py-2 text-sm disabled:opacity-40"
                title={audioState.playing ? 'Pause' : 'Play'}
              >
                {audioState.playing ? '▐▐' : '►'}
              </button>
              <button
                type="button"
                onClick={handleSkipNext}
                disabled={!canControl}
                className="btn-secondary rounded-full px-3 py-2 text-sm disabled:opacity-40"
                title="Next"
              >
                ►►
              </button>
              {!canControl && (
                <span className="text-xs muted">Controls are host-only in this room.</span>
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
        {audioSource === 'online' && (
          <div className="panel-soft space-y-2 rounded-xl px-3 py-3">
            <div className="flex items-center justify-between gap-3">
              <p className="text-xs uppercase tracking-wide muted">Online Pipeline Status</p>
              <p className="text-[11px] muted">Latest {Math.min(recentOnlineStatusEvents.length, 8)} updates</p>
            </div>
            {recentOnlineStatusEvents.length === 0 ? (
              <p className="text-xs muted">
                No pipeline updates yet. Search and queue a track to start the online download flow.
              </p>
            ) : (
              <ul className="max-h-44 space-y-1 overflow-y-auto pr-1">
                {recentOnlineStatusEvents.map((event, idx) => (
                  <li
                    key={`${event.updated_ts_ms}-${event.stage}-${idx}`}
                    className="tile flex items-start gap-2 rounded-lg px-2 py-1.5"
                  >
                    <span className="mt-0.5 text-[11px] muted">
                      {formatStatusTimestamp(event.updated_ts_ms)}
                    </span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] uppercase tracking-wide ${statusBadgeClass(
                        event.status,
                      )}`}
                    >
                      {event.status}
                    </span>
                    <div className="min-w-0 flex-1">
                      <p className="text-[11px] uppercase tracking-wide muted">{event.stage.replaceAll('_', ' ')}</p>
                      <p className="text-xs leading-snug">{event.message}</p>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        {audioSource === 'online' ? (
          <div className="flex items-center gap-2">
            <h2 className="text-lg font-semibold shrink-0">Online Search</h2>
            <input
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              className="input flex-1 px-3 py-1.5 text-sm"
              placeholder="Search YouTube tracks…"
              aria-label="Search online tracks"
            />
            <button
              type="button"
              className="btn-secondary shrink-0 px-3 py-1.5 text-xs"
              onClick={() => setShowOnlineSearchResults((prev) => !prev)}
            >
              {showOnlineSearchResults ? 'Hide Results ▴' : 'Show Results ▾'}
            </button>
          </div>
        ) : (
          <div className="flex items-center justify-between gap-2">
            <h2 className="text-lg font-semibold">
              {librarySearchResults !== null ? 'Search Results' : 'Queue'}
            </h2>
            <input
              value={searchQuery}
              onChange={(e) => handleSearch(e.target.value)}
              className="input px-3 py-1.5 text-sm"
              placeholder="Search tracks…"
              aria-label="Search tracks"
            />
          </div>
        )}

        {searching && (
          <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">Searching…</div>
        )}

        {audioSource === 'online' && !searching && onlineSearchResults !== null && onlineSearchResults.length === 0 && (
          <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">No online tracks found.</div>
        )}

        {audioSource === 'online' &&
          !searching &&
          showOnlineSearchResults &&
          onlineSearchResults !== null &&
          onlineSearchResults.length > 0 && (
          <ul className="max-h-64 space-y-2 overflow-y-auto">
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

        {audioSource === 'online' &&
          !searching &&
          !showOnlineSearchResults &&
          onlineSearchResults !== null &&
          onlineSearchResults.length > 0 && (
            <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">
              Search results hidden.
            </div>
          )}

        {audioSource !== 'online' && !searching && librarySearchResults !== null && librarySearchResults.length === 0 && (
          <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">No tracks found.</div>
        )}

        {audioSource === 'online' && (
          <p className="text-xs uppercase tracking-wide muted">Room Queue</p>
        )}

        {!searching && displayList.length > 0 && (
          <ul ref={queueRef} className={`space-y-1 overflow-y-auto ${audioSource === 'online' ? 'max-h-72' : 'max-h-80'}`}>
            {displayList.map((entry, idx) => {
              const isActive =
                !entry.isSearchResult && idx === audioState.queue_index;
              return (
                <li
                  key={entry.track_id}
                  data-active={isActive ? 'true' : 'false'}
                  className={`tile rounded-xl px-3 py-2 ${isActive ? 'border-[var(--orange-soft)]' : ''}`}
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
        )}
      </section>
    </div>
  );
}
