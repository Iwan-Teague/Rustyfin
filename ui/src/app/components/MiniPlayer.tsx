'use client';

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
    <div className="fixed bottom-0 left-0 right-0 z-40 border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2 flex items-center gap-4">
      {/* Album art + track info */}
      <div className="flex items-center gap-3 min-w-0 w-56 shrink-0">
        {currentTrack.albumArtUrl ? (
          <img
            src={currentTrack.albumArtUrl}
            alt={currentTrack.albumTitle}
            className="w-10 h-10 rounded object-cover shrink-0"
          />
        ) : (
          <div className="w-10 h-10 rounded bg-white/10 flex items-center justify-center shrink-0 text-lg">
            ♪
          </div>
        )}
        <div className="min-w-0">
          <p className="text-sm font-medium truncate">{currentTrack.title}</p>
          <p className="text-xs muted truncate">{currentTrack.artist}</p>
        </div>
      </div>

      {/* Controls + seek */}
      <div className="flex-1 flex flex-col items-center gap-1 min-w-0">
        <div className="flex items-center gap-3">
          <button
            onClick={prev}
            disabled={currentIndex === 0}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Previous"
          >
            ⏮
          </button>
          <button
            onClick={playPause}
            className="btn-primary px-3 py-1.5 text-sm rounded-full"
            title={playing ? 'Pause' : 'Play'}
          >
            {playing ? '⏸' : '▶'}
          </button>
          <button
            onClick={next}
            disabled={currentIndex >= queue.length - 1}
            className="btn-ghost px-2 py-1 text-base disabled:opacity-30"
            title="Next"
          >
            ⏭
          </button>
        </div>
        <div className="flex items-center gap-2 w-full max-w-md">
          <span className="text-xs muted w-8 text-right shrink-0">{formatSeconds(progress)}</span>
          <input
            type="range"
            min={0}
            max={duration || 1}
            step={0.1}
            value={progress}
            onChange={(e) => seek(Number(e.target.value))}
            className="flex-1 h-1 accent-[var(--orange-soft)]"
          />
          <span className="text-xs muted w-8 shrink-0">{formatSeconds(duration)}</span>
        </div>
      </div>

      {/* Volume + stop */}
      <div className="flex items-center gap-2 w-32 shrink-0 justify-end">
        <span className="text-sm muted">Volume:</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={volume}
          onChange={(e) => setVolume(Number(e.target.value))}
          className="w-16 h-1 accent-[var(--orange-soft)]"
        />
        <button
          onClick={stop}
          className="btn-ghost px-2 py-1 text-xs muted"
          title="Stop"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
