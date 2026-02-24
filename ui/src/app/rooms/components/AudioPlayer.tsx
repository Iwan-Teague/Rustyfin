'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { apiJson } from '@/lib/api';
import { AudioTrack, QueueEntry, WsAudioStateMessage, listAudioTracks } from '@/lib/watchPartyApi';

type Props = {
  audioState: WsAudioStateMessage;
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

function projectPosition(state: WsAudioStateMessage): number {
  if (!state.playing) return state.position_ms;
  const now = Date.now();
  const elapsed = now - state.server_ts_ms;
  return state.position_ms + Math.max(0, elapsed);
}

export default function AudioPlayer({ audioState, canControl, canSeek, roomId, sendWs }: Props) {
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<AudioTrack[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [projectedPosition, setProjectedPosition] = useState(audioState.position_ms);
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [streamError, setStreamError] = useState('');
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const [scrubPosition, setScrubPosition] = useState(0);

  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queueRef = useRef<HTMLUListElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const loadedTrackIdRef = useRef<string | null>(null);

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
      loadedTrackIdRef.current = null;
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
  }, [audioState.track_id]);

  useEffect(() => {
    return () => {
      const audio = audioRef.current;
      if (!audio) return;
      audio.pause();
      audio.removeAttribute('src');
      audio.load();
    };
  }, []);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !descriptor || !audioState.track_id) return;

    if (loadedTrackIdRef.current !== audioState.track_id) {
      loadedTrackIdRef.current = audioState.track_id;
      audio.src = descriptor.direct_url;
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
  }, [descriptor, audioState.track_id, audioState.playing, projectedPosition, isScrubbing]);

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
        setSearchResults(null);
        return;
      }

      searchTimeoutRef.current = setTimeout(async () => {
        setSearching(true);
        try {
          const results = await listAudioTracks(roomId, query);
          setSearchResults(results);
        } catch {
          setSearchResults([]);
        } finally {
          setSearching(false);
        }
      }, 300);
    },
    [roomId],
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

  const displayList: Array<QueueEntry & { isSearchResult?: boolean }> =
    searchResults !== null
      ? searchResults.map((t) => ({ ...t, track_id: t.id, isSearchResult: true }))
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
          </div>
        </div>
      </section>

      {/* Queue & Search */}
      <section className="panel space-y-3 p-5 sm:p-6">
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-lg font-semibold">
            {searchResults !== null ? 'Search Results' : 'Queue'}
          </h2>
          <input
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            className="input px-3 py-1.5 text-sm"
            placeholder="Search tracks…"
            aria-label="Search tracks"
          />
        </div>

        {searching && (
          <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">Searching…</div>
        )}

        {!searching && searchResults !== null && searchResults.length === 0 && (
          <div className="panel-soft rounded-xl px-3 py-2 text-sm muted">No tracks found.</div>
        )}

        {!searching && displayList.length > 0 && (
          <ul ref={queueRef} className="max-h-80 space-y-1 overflow-y-auto">
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
