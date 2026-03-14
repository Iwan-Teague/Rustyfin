'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import Image from 'next/image';
import { apiJson } from '@/lib/api';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { clientErrorMessage } from '@/lib/errors';

interface Library {
  id: string;
  name: string;
  kind: string;
  item_count: number;
}

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
}

interface ContinueWatchingItem {
  id: string;
  library_id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
  progress_ms: number;
  duration_ms?: number | null;
  last_played_ts: number;
}

function formatDurationLabel(totalSeconds: number): string {
  const safeTotal = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(safeTotal / 3600);
  const minutes = Math.floor((safeTotal % 3600) / 60);
  const seconds = safeTotal % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

export default function LibrariesPage() {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [loading, setLoading] = useState(true);
  const [featuredItems, setFeaturedItems] = useState<Item[]>([]);
  const [continueWatching, setContinueWatching] = useState<ContinueWatchingItem[]>([]);
  const [dismissingContinueItemIds, setDismissingContinueItemIds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      setError(null);
      try {
        const [libs, continueItems] = await Promise.all([
          apiJson<Library[]>('/libraries'),
          apiJson<ContinueWatchingItem[]>('/playback/continue').catch(
            () => [] as ContinueWatchingItem[],
          ),
        ]);
        if (cancelled) return;
        setLibraries(libs);
        setContinueWatching(continueItems);
        setLoading(false);

        const itemArrays = await Promise.all(
          libs.slice(0, 4).map((lib) =>
            apiJson<Item[]>(`/libraries/${lib.id}/items`).catch(() => [] as Item[]),
          ),
        );
        if (cancelled) return;
        setFeaturedItems(itemArrays.flat().slice(0, 24));
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load libraries view'));
          setLoading(false);
        }
      }
    })();

    return () => { cancelled = true; };
  }, []);

  async function handleDismissContinueItem(itemId: string) {
    if (dismissingContinueItemIds.includes(itemId)) return;
    setDismissingContinueItemIds((prev) => [...prev, itemId]);
    setError(null);
    try {
      await apiJson<{ ok: boolean }>('/playback/progress', {
        method: 'POST',
        body: JSON.stringify({
          item_id: itemId,
          progress_ms: 0,
          played: false,
        }),
      });
      const target = findDataDeleteTarget('data-libraries-continue-id', itemId);
      await playTelegramDeleteAnimation(target);
      setContinueWatching((prev) => prev.filter((item) => item.id !== itemId));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to remove from Continue Watching'));
    } finally {
      setDismissingContinueItemIds((prev) => prev.filter((id) => id !== itemId));
    }
  }

  const recommendedItems = useMemo(() => {
    const preferred = featuredItems.filter(
      (item) => item.kind === 'movie' || item.kind === 'episode',
    );
    return preferred.length > 0 ? preferred.slice(0, 6) : featuredItems.slice(0, 6);
  }, [featuredItems]);

  if (loading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading libraries...</p>
      </div>
    );
  }

  return (
    <div className="space-y-8 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold sm:text-4xl">Libraries</h1>
        <p className="text-sm muted sm:text-base">
          Explore all configured media directories and jump into items instantly.
        </p>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      {libraries.length === 0 ? (
        <div className="panel px-6 py-8">
          <p className="text-sm muted">No libraries found. Create one from the admin panel.</p>
          <Link href="/admin" className="btn-primary mt-4 px-5 py-2 text-sm">
            Open Admin
          </Link>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
          {libraries.map((lib) => (
            <Link
              key={lib.id}
              href={`/libraries/${lib.id}`}
              className="tile tile-hover library-card-sheen block p-5"
            >
              <div className="flex items-center justify-between gap-4">
                <h2 className="text-lg font-semibold">{lib.name}</h2>
                <span className="chip">
                  {lib.kind === 'tv_shows' ? 'TV' : lib.kind === 'music' ? 'Music' : 'Movies'}
                </span>
              </div>
              <p className="mt-2 text-sm muted">
                {lib.kind} · {lib.item_count} items
              </p>
            </Link>
          ))}
        </div>
      )}

      {/* Continue Watching */}
      <section id="continue-watching" className="space-y-3 scroll-mt-24">
        <h2 className="text-xl font-semibold sm:text-2xl">Continue Watching</h2>
        {continueWatching.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">
            Start a movie or episode from a library and Rustyfin will keep your place here.
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {continueWatching.map((item) => {
              const totalMs = item.duration_ms && item.duration_ms > 0 ? item.duration_ms : null;
              const progressPct = totalMs
                ? Math.max(0, Math.min(100, (item.progress_ms / totalMs) * 100))
                : 0;
              const progressLabel = totalMs
                ? `${formatDurationLabel(item.progress_ms / 1000)} / ${formatDurationLabel(totalMs / 1000)}`
                : `Resume at ${formatDurationLabel(item.progress_ms / 1000)}`;
              const dismissing = dismissingContinueItemIds.includes(item.id);
              const thumbnailUrl = item.thumb_url ?? item.poster_url;

              return (
                <div
                  key={`continue-${item.id}`}
                  className="relative"
                  data-libraries-continue-id={item.id}
                >
                  <button
                    type="button"
                    className="btn-ghost absolute right-3 top-3 z-20 h-7 w-7 rounded-full border border-white/20 bg-black/60 p-0 text-white/80 hover:text-white disabled:cursor-not-allowed disabled:opacity-45"
                    onClick={() => void handleDismissContinueItem(item.id)}
                    disabled={dismissing}
                    aria-label={`Remove ${item.title} from Continue Watching`}
                    title="Remove from Continue Watching"
                  >
                    {dismissing ? '…' : '×'}
                  </button>
                  <Link href={`/player/${item.id}`} className="group tile tile-hover media-tile-hover block overflow-hidden">
                    <div className="flex min-h-[9rem] gap-4 p-4">
                      <div className="h-20 w-36 flex-shrink-0 overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--panel)]/65 sm:h-24 sm:w-40">
                        {thumbnailUrl ? (
                          <Image
                            src={thumbnailUrl}
                            alt={item.title}
                            width={320}
                            height={180}
                            unoptimized
                            className="rf-media-zoom-image h-full w-full object-cover"
                          />
                        ) : (
                          <div className="flex h-full items-center justify-center px-2 text-center text-xs muted">
                            {item.kind.toUpperCase()}
                          </div>
                        )}
                      </div>
                      <div className="flex min-w-0 flex-1 flex-col justify-between">
                        <div className="space-y-2">
                          <div className="flex items-start gap-3">
                            <div className="min-w-0">
                              <p className="truncate text-base font-semibold">{item.title}</p>
                              <p className="text-xs uppercase tracking-[0.24em] text-white/60">
                                {item.kind === 'episode' ? 'Episode' : 'Movie'}
                                {item.year ? ` · ${item.year}` : ''}
                              </p>
                            </div>
                          </div>
                          <div className="space-y-2">
                            <div className="rf-progress-track">
                              <div
                                className="rf-progress-fill"
                                style={{ width: `${progressPct || 0}%` }}
                              />
                            </div>
                            <p className="text-xs muted">{progressLabel}</p>
                          </div>
                        </div>
                      </div>
                    </div>
                  </Link>
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* Recommended */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Recommended</h2>
        {recommendedItems.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">No recommendations yet.</div>
        ) : (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {recommendedItems.map((item) => (
              <Link key={`rec-${item.id}`} href={`/items/${item.id}`} className="group block">
                <div className="tile tile-hover media-tile-hover aspect-video overflow-hidden">
                  {(item.thumb_url ?? item.poster_url) ? (
                    <Image
                      src={item.thumb_url ?? item.poster_url ?? ''}
                      alt={item.title}
                      width={320}
                      height={180}
                      unoptimized
                      className="rf-media-zoom-image h-full w-full object-cover"
                    />
                  ) : (
                    <div className="flex h-full items-center justify-center px-2 text-center text-xs muted">
                      {item.kind.toUpperCase()}
                    </div>
                  )}
                </div>
                <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
                {item.year && <p className="text-xs muted">{item.year}</p>}
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
