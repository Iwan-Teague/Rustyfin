'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { apiJson } from '@/lib/api';
import { clientErrorMessage } from '@/lib/errors';
import { getPublicSystemInfo } from '@/lib/setupApi';
import { listPublicRooms, type PublicRoom } from '@/lib/watchPartyApi';

interface ContinueWatchingItem {
  id: string;
  library_id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
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

export default function HomePage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [setupChecked, setSetupChecked] = useState(false);
  const [setupComplete, setSetupComplete] = useState(true);
  const [loadingData, setLoadingData] = useState(false);
  const [continueWatching, setContinueWatching] = useState<ContinueWatchingItem[]>([]);
  const [publicRooms, setPublicRooms] = useState<PublicRoom[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getPublicSystemInfo()
      .then((info) => {
        if (cancelled) return;
        setSetupComplete(info.setup_completed);
        setSetupChecked(true);
        if (!info.setup_completed) {
          router.replace('/setup');
        }
      })
      .catch(() => {
        if (!cancelled) {
          setSetupChecked(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [router]);

  useEffect(() => {
    if (setupChecked && setupComplete && !authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router, setupChecked, setupComplete]);

  useEffect(() => {
    let cancelled = false;
    if (!setupComplete || authLoading || !me) {
      return () => {
        cancelled = true;
      };
    }

    setLoadingData(true);
    setError(null);

    (async () => {
      try {
        const [continueItems, rooms] = await Promise.all([
          apiJson<ContinueWatchingItem[]>('/playback/continue').catch(
            () => [] as ContinueWatchingItem[],
          ),
          listPublicRooms().catch(() => [] as PublicRoom[]),
        ]);
        if (cancelled) return;
        setContinueWatching(continueItems);
        setPublicRooms(rooms);
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load home view'));
        }
      } finally {
        if (!cancelled) {
          setLoadingData(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [setupComplete, authLoading, me]);

  if (!setupChecked) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Checking setup status...</p>
      </div>
    );
  }

  if (!setupComplete) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Setup is required. Redirecting to setup wizard...</p>
      </div>
    );
  }

  if (authLoading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading your home view...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Redirecting to login...</p>
      </div>
    );
  }

  return (
    <div className="space-y-7 animate-rise">
      <header className="panel space-y-3 p-6 sm:p-8">
        <h1 className="text-3xl font-semibold sm:text-4xl">Welcome back, {me.username}</h1>
        <p className="text-sm muted sm:text-base">
          Pick up where you left off or jump into an open room.
        </p>
      </header>

      {error && (
        <div className="notice-error rounded-xl px-4 py-2 text-sm">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 gap-7 xl:grid-cols-2 xl:items-start">
        <section id="continue-watching" className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-xl font-semibold sm:text-2xl">Continue Watching</h2>
            <Link href="/libraries#continue-watching" className="text-sm text-[var(--orange-soft)]">
              View all
            </Link>
          </div>
          {continueWatching.length === 0 ? (
            <div className="panel-soft px-4 py-3 text-sm muted">
              Start a movie or episode from a library and Rustyfin will keep your place here.
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-4">
              {continueWatching.slice(0, 3).map((item) => {
                const totalMs = item.duration_ms && item.duration_ms > 0 ? item.duration_ms : null;
                const progressPct = totalMs
                  ? Math.max(0, Math.min(100, (item.progress_ms / totalMs) * 100))
                  : 0;
                const progressLabel = totalMs
                  ? `${formatDurationLabel(item.progress_ms / 1000)} / ${formatDurationLabel(totalMs / 1000)}`
                  : `Resume at ${formatDurationLabel(item.progress_ms / 1000)}`;

                return (
                  <Link
                    key={`home-continue-${item.id}`}
                    href={`/player/${item.id}`}
                    className="tile tile-hover block overflow-hidden"
                  >
                    <div className="flex min-h-[9rem] gap-4 p-4">
                      <div className="h-32 w-24 flex-shrink-0 overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--panel)]/65">
                        {item.poster_url ? (
                          <img
                            src={item.poster_url}
                            alt={item.title}
                            className="h-full w-full object-cover"
                          />
                        ) : (
                          <div className="flex h-full items-center justify-center px-2 text-center text-xs muted">
                            {item.kind.toUpperCase()}
                          </div>
                        )}
                      </div>
                      <div className="flex min-w-0 flex-1 flex-col justify-between">
                        <div className="space-y-2">
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0">
                              <p className="truncate text-base font-semibold">{item.title}</p>
                              <p className="text-xs uppercase tracking-[0.24em] text-white/40">
                                {item.kind === 'episode' ? 'Episode' : 'Movie'}
                                {item.year ? ` · ${item.year}` : ''}
                              </p>
                            </div>
                            <span className="chip">Resume</span>
                          </div>
                          <div className="space-y-2">
                            <div className="h-2 overflow-hidden rounded-full bg-white/10">
                              <div
                                className="h-full rounded-full bg-gradient-to-r from-[var(--accent-orange)] via-[var(--accent-pink)] to-[var(--accent-purple)]"
                                style={{ width: `${progressPct || 0}%` }}
                              />
                            </div>
                            <p className="text-xs muted">{progressLabel}</p>
                          </div>
                        </div>
                      </div>
                    </div>
                  </Link>
                );
              })}
            </div>
          )}
        </section>

        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-xl font-semibold sm:text-2xl">Open Rooms</h2>
            <Link href="/rooms" className="text-sm text-[var(--orange-soft)]">
              View all
            </Link>
          </div>
          {loadingData ? (
            <div className="panel-soft px-4 py-3 text-sm muted">Loading open rooms...</div>
          ) : publicRooms.length === 0 ? (
            <div className="panel-soft px-4 py-3 text-sm muted">
              No open rooms right now.
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-3">
              {publicRooms.map((room) => (
                <Link
                  key={room.room_id}
                  href={`/rooms/${room.room_id}`}
                  className="tile tile-hover p-4 flex flex-col gap-2"
                >
                  <div className="flex items-start justify-between gap-2">
                    <p className="font-semibold truncate leading-snug">{room.title}</p>
                  </div>
                  <div className="flex items-center justify-between text-xs muted">
                    <span>Hosted by {room.host_username}</span>
                    <span>{room.member_count} in room</span>
                  </div>
                </Link>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
