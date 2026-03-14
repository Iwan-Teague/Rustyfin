'use client';

import Image from 'next/image';
import { useMusicPlayer } from '@/lib/musicPlayerContext';

function formatSeconds(s: number): string {
  if (!isFinite(s) || s < 0) return '0:00';
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60).toString().padStart(2, '0');
  return `${m}:${sec}`;
}

export default function MiniPlayer() {
  const {
    queue,
    currentTrack,
    playing,
    progress,
    duration,
    volume,
    currentIndex,
    playPause,
    seek,
    next,
    prev,
    setVolume,
    stop,
  } = useMusicPlayer();

  if (queue.length === 0 || !currentTrack) return null;

  return (
    <div className="fixed bottom-0 left-0 right-0 z-40 flex flex-col gap-2 border-t border-[var(--border)] bg-[var(--surface)] px-3 py-2 sm:flex-row sm:items-center sm:gap-4 sm:px-4">
      {/* Album art + track info */}
      <div className="flex min-w-0 items-center gap-3 sm:w-56 sm:shrink-0">
        {currentTrack.albumArtUrl ? (
          <Image
            src={currentTrack.albumArtUrl}
            alt={currentTrack.albumTitle}
            width={40}
            height={40}
            unoptimized
            className="h-10 w-10 shrink-0 rounded object-cover"
          />
        ) : (
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-white/10 text-lg">
            ♪
          </div>
        )}
        <div className="mini-player-track-info min-w-0">
          <p className="text-sm font-medium truncate">{currentTrack.title}</p>
          <p className="text-xs muted truncate">{currentTrack.artist}</p>
        </div>
      </div>

      {/* Controls + seek */}
      <div className="flex min-w-0 flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-3">
          <button
            onClick={prev}
            disabled={currentIndex === 0}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Previous"
            aria-label="Previous track"
          >
            ⏮
          </button>
          <button
            onClick={playPause}
            className="btn-primary px-3 py-1.5 text-sm rounded-full"
            title={playing ? 'Pause' : 'Play'}
            aria-label={playing ? 'Pause playback' : 'Start playback'}
          >
            {playing ? '⏸' : '▶'}
          </button>
          <button
            onClick={next}
            disabled={currentIndex >= queue.length - 1}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Next"
            aria-label="Next track"
          >
            ⏭
          </button>
        </div>
        <div className="flex w-full max-w-md items-center gap-2">
          <span className="text-xs muted w-8 text-right shrink-0">{formatSeconds(progress)}</span>
          <input
            type="range"
            min={0}
            max={duration || 1}
            step={0.1}
            value={progress}
            onChange={(e) => seek(Number(e.target.value))}
            className="flex-1 h-1 accent-[var(--orange-soft)]"
            aria-label="Playback timeline"
          />
          <span className="text-xs muted w-8 shrink-0">{formatSeconds(duration)}</span>
        </div>
      </div>

      {/* Volume + stop */}
      <div className="mini-player-volume flex w-full items-center gap-2 justify-end sm:w-32 sm:shrink-0">
        <span className="text-sm muted">Volume:</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={volume}
          onChange={(e) => setVolume(Number(e.target.value))}
          className="w-16 h-1 accent-[var(--orange-soft)]"
          aria-label="Volume"
        />
        <button
          onClick={stop}
          className="btn-ghost px-2 py-1 text-xs muted"
          title="Stop"
          aria-label="Stop playback"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
