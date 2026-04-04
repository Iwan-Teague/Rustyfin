'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import Image from 'next/image';
import { apiJson } from '@/lib/api';
import { useMusicPlayer, type MusicTrack } from '@/lib/musicPlayerContext';

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  overview?: string;
  poster_url?: string;
  backdrop_url?: string;
  thumb_url?: string;
  parent_id?: string;
  library_id: string;
}

interface Child {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
  duration_ms?: number;
}

function formatDuration(ms?: number): string {
  if (!ms) return '';
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = (total % 60).toString().padStart(2, '0');
  return `${m}:${s}`;
}

export default function ItemPage() {
  const params = useParams();
  const id = params.id as string;
  const [item, setItem] = useState<Item | null>(null);
  const [children, setChildren] = useState<Child[]>([]);
  const [parentItem, setParentItem] = useState<Item | null>(null);
  const { playQueue, currentTrack, playing, playPause } = useMusicPlayer();

  useEffect(() => {
    setItem(null);
    setChildren([]);
    setParentItem(null);
    apiJson<Item>(`/items/${id}`).then(setItem).catch(() => {});
    apiJson<Child[]>(`/items/${id}/children`).then(setChildren).catch(() => {});
  }, [id]);

  // Fetch parent for album pages (to get artist name)
  useEffect(() => {
    if (item?.kind === 'album' && item.parent_id) {
      apiJson<Item>(`/items/${item.parent_id}`).then(setParentItem).catch(() => {});
    }
  }, [item]);

  if (!item) {
    return (
      <div className="rf-flat-empty animate-rise px-5 py-4">
        <p className="text-sm muted">Loading...</p>
      </div>
    );
  }

  // ── Artist page ────────────────────────────────────────────────────────────
  if (item.kind === 'artist') {
    return (
      <div className="rf-flat-page rf-flat-scope animate-rise">
        <header className="rf-flat-header">
          <h1 className="text-3xl font-semibold">{item.title}</h1>
          {item.overview && <p className="max-w-3xl leading-relaxed muted">{item.overview}</p>}
        </header>

        {children.length > 0 && (
          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Albums</h2>
            <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
              {children.map((album) => (
                <Link key={album.id} href={`/items/${album.id}`} className="group block">
                  <div className="tile tile-hover aspect-square overflow-hidden">
                    {album.poster_url ? (
                      <Image
                        src={album.poster_url}
                        alt={album.title}
                        width={320}
                        height={320}
                        unoptimized
                        className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                      />
                    ) : (
                      <div className="flex h-full w-full items-center justify-center text-4xl muted">
                        ♪
                      </div>
                    )}
                  </div>
                  <p className="mt-2 truncate text-sm font-medium">{album.title}</p>
                  {album.year && <p className="text-xs muted">{album.year}</p>}
                </Link>
              ))}
            </div>
          </section>
        )}
      </div>
    );
  }

  // ── Album page ─────────────────────────────────────────────────────────────
  if (item.kind === 'album') {
    const artistName = parentItem?.title ?? '';

    function buildQueue(tracks: Child[], albumArtUrl?: string): MusicTrack[] {
      return tracks.map((t) => ({
        id: t.id,
        title: t.title,
        artist: artistName,
        albumTitle: item!.title,
        albumArtUrl,
        durationMs: t.duration_ms,
      }));
    }

    const albumArtUrl = item.poster_url ?? undefined;

    return (
      <div className="rf-flat-page rf-flat-scope animate-rise">
        <div className="flex flex-col gap-6 sm:flex-row">
          {/* Album art */}
          <div className="w-48 shrink-0">
            <div className="tile overflow-hidden aspect-square">
              {albumArtUrl ? (
                <Image
                  src={albumArtUrl}
                  alt={item.title}
                  width={320}
                  height={320}
                  unoptimized
                  className="h-full w-full object-cover"
                />
              ) : (
                <div className="flex h-full w-full items-center justify-center text-5xl muted">
                  ♪
                </div>
              )}
            </div>
          </div>

          {/* Album meta */}
          <div className="flex-1 space-y-3">
            <div className="space-y-1">
              <h1 className="text-3xl font-semibold">{item.title}</h1>
              {artistName && (
                <Link href={`/items/${item.parent_id}`} className="text-sm muted hover:underline">
                  {artistName}
                </Link>
              )}
              {item.year && <p className="text-sm muted">{item.year}</p>}
            </div>

            {children.length > 0 && (
              <button
                onClick={() => playQueue(buildQueue(children, albumArtUrl), 0)}
                className="btn-primary px-5 py-2 text-sm"
              >
                ▶ Play Album
              </button>
            )}
          </div>
        </div>

        {/* Track list */}
        {children.length > 0 && (
          <section className="space-y-1">
            {children.map((track, idx) => {
              const isPlaying = currentTrack?.id === track.id && playing;
              const isCurrent = currentTrack?.id === track.id;
              return (
                <div
                  key={track.id}
                  className={[
                    'flex items-center gap-3 px-3 py-2 rounded-lg group cursor-pointer select-none',
                    isCurrent ? 'bg-white/10' : 'hover:bg-white/5',
                  ].join(' ')}
                  onClick={() => {
                    if (isCurrent) {
                      playPause();
                    } else {
                      playQueue(buildQueue(children, albumArtUrl), idx);
                    }
                  }}
                >
                  <span className="w-6 text-center text-xs muted shrink-0">
                    {isPlaying ? '▶' : (isCurrent ? '❙❙' : idx + 1)}
                  </span>
                  <span className={['flex-1 text-sm truncate', isCurrent ? 'text-[var(--orange-soft)]' : ''].join(' ')}>
                    {track.title}
                  </span>
                  <span className="text-xs muted shrink-0">{formatDuration(track.duration_ms)}</span>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      if (isCurrent) {
                        playPause();
                      } else {
                        playQueue(buildQueue(children, albumArtUrl), idx);
                      }
                    }}
                    className="btn-ghost px-2 py-0.5 text-xs opacity-0 group-hover:opacity-100"
                  >
                    {isPlaying ? '⏸' : '▶'}
                  </button>
                </div>
              );
            })}
          </section>
        )}
      </div>
    );
  }

  // ── Track page (direct navigation) ────────────────────────────────────────
  if (item.kind === 'track') {
    return (
      <div className="rf-flat-page rf-flat-scope animate-rise">
        <h1 className="text-3xl font-semibold">{item.title}</h1>
        {item.parent_id && (
          <Link href={`/items/${item.parent_id}`} className="text-sm muted hover:underline block">
            ← Back to album
          </Link>
        )}
        <button
          onClick={() =>
            playQueue([
              {
                id: item.id,
                title: item.title,
                artist: '',
                albumTitle: '',
                albumArtUrl: item.poster_url ?? undefined,
              },
            ])
          }
          className="btn-primary px-6 py-2.5 text-sm"
        >
          ▶ Play
        </button>
      </div>
    );
  }

  // ── Default: movies / series / episodes (existing behaviour) ──────────────
  const isPlayable = item.kind === 'movie' || item.kind === 'episode';
  const displayChildren = (() => {
    if (item.kind !== 'series') return children;
    const seasonChildren = children.filter((child) => child.kind === 'season');
    return seasonChildren.length > 0 ? seasonChildren : children;
  })();
  const childSectionLabel =
    item.kind === 'series' && displayChildren.some((child) => child.kind === 'season')
      ? 'Seasons'
      : 'Episodes';
  const hasEpisodeChildren = displayChildren.some((child) => child.kind === 'episode');
  const childGridClass = hasEpisodeChildren
    ? 'grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4'
    : 'grid grid-cols-2 gap-3 md:grid-cols-4 lg:grid-cols-6';

  return (
    <div className="rf-flat-scope relative isolate">
      {item.backdrop_url && (
        <div className="pointer-events-none fixed inset-0 z-0 overflow-hidden" aria-hidden="true">
          <Image
            src={item.backdrop_url}
            alt=""
            fill
            sizes="100vw"
            unoptimized
            className="object-cover opacity-35 scale-[1.04]"
          />
          <div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_0%,rgba(38,48,72,0.18),rgba(18,22,34,0.7)_62%)]" />
          <div className="absolute inset-0 bg-gradient-to-b from-[#111826]/35 via-[#1a2234]/56 to-[#1f2535]/90" />
        </div>
      )}

      <div className="relative z-10 rf-flat-page animate-rise">
        <div className="flex flex-col gap-6 lg:flex-row">
          {item.poster_url && (
            <div className="w-64 max-w-full flex-shrink-0 sm:w-72">
              <div className="tile overflow-hidden">
                <Image src={item.poster_url} alt={item.title} width={384} height={576} unoptimized className="w-full" />
              </div>
            </div>
          )}
          <div className="flex-1 space-y-4">
            <div className="space-y-2">
              <h1 className="text-3xl font-semibold sm:text-4xl">{item.title}</h1>
              {item.year && <span className="text-sm muted">{item.year}</span>}
            </div>

            {item.overview && <p className="max-w-3xl leading-relaxed muted">{item.overview}</p>}

            {isPlayable && (
              <Link
                href={`/player/${id}`}
                className="btn-primary inline-flex px-6 py-2.5 text-sm"
              >
                Play Now
              </Link>
            )}
          </div>
        </div>

        {displayChildren.length > 0 && (
          <section className="space-y-4">
            <h2 className="text-xl font-semibold">{childSectionLabel}</h2>
            <div className={childGridClass}>
              {displayChildren.map((child) => (
                <Link key={child.id} href={`/items/${child.id}`} className="tile tile-hover block overflow-hidden">
                  {(() => {
                    const isEpisodeChild = child.kind === 'episode';
                    const childPoster = child.poster_url ?? child.thumb_url;
                    const artworkAspectClass = isEpisodeChild ? 'aspect-video' : 'aspect-[2/3]';
                    if (!childPoster) {
                      return (
                        <div className={`flex ${artworkAspectClass} items-center justify-center bg-white/5 px-2 text-xs muted`}>
                          {child.kind.toUpperCase()}
                        </div>
                      );
                    }
                    return (
                      <Image
                        src={childPoster}
                        alt={child.title}
                        width={isEpisodeChild ? 480 : 320}
                        height={isEpisodeChild ? 270 : 480}
                        unoptimized
                        className={`${artworkAspectClass} w-full object-cover`}
                      />
                    );
                  })()}
                  <div className="space-y-1 p-3">
                    <p className="font-medium text-sm">{child.title}</p>
                    {child.kind === 'episode' && (
                      <p className="text-xs muted">Episode</p>
                    )}
                  </div>
                </Link>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
