'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { apiJson } from '@/lib/api';
import { getPublicSystemInfo } from '@/lib/setupApi';
import { listPublicRooms, type PublicRoom } from '@/lib/watchPartyApi';

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
}

export default function HomePage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();
  const { voiceSession, channels, voicePresence } = useChannels();

  const [setupChecked, setSetupChecked] = useState(false);
  const [setupComplete, setSetupComplete] = useState(true);
  const [loadingData, setLoadingData] = useState(false);
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [featuredItems, setFeaturedItems] = useState<Item[]>([]);
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
        const [libs, rooms] = await Promise.all([
          apiJson<Library[]>('/libraries'),
          listPublicRooms().catch(() => [] as PublicRoom[]),
        ]);
        if (cancelled) return;
        setLibraries(libs);
        setPublicRooms(rooms);

        const libraryItems = await Promise.all(
          libs.slice(0, 4).map((lib) =>
            apiJson<Item[]>(`/libraries/${lib.id}/items`).catch(() => [] as Item[]),
          ),
        );
        if (cancelled) return;

        const flattened = libraryItems.flat().slice(0, 24);
        setFeaturedItems(flattened);
      } catch (err: any) {
        if (!cancelled) {
          setError(err?.message || 'Failed to load home view');
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

  const upNextItems = useMemo(() => {
    const episodes = featuredItems.filter((item) => item.kind === 'episode');
    return episodes.length > 0 ? episodes.slice(0, 6) : featuredItems.slice(0, 6);
  }, [featuredItems]);

  // Voice channels that currently have at least one participant
  const activeVoiceChannels = useMemo(() => {
    return channels.filter(
      (c) => c.kind === 'voice' && (voicePresence[c.id]?.length ?? 0) > 0,
    );
  }, [channels, voicePresence]);

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
      <section className="panel animate-rise space-y-4 p-6 sm:p-8">
        <h1 className="text-3xl font-semibold sm:text-4xl">Sign in to open your server home</h1>
        <p className="text-sm muted sm:text-base">
          Setup is complete. Sign in to see libraries, active channels, and watch parties.
        </p>
        <Link href="/login" className="btn-primary inline-flex px-5 py-2.5 text-sm">
          Go to Login
        </Link>
      </section>
    );
  }

  return (
    <div className="space-y-7 animate-rise">
      <header className="panel space-y-3 p-6 sm:p-8">
        <div className="flex flex-wrap items-center gap-2">
          {voiceSession && (
            <Link
              href="/channels"
              className="chip flex items-center gap-1.5 text-xs border-green-500/50 text-green-400"
            >
              <span className="h-2 w-2 rounded-full bg-green-400 animate-pulse" />
              Connected · {voiceSession.channelName}
            </Link>
          )}
        </div>
        <h1 className="text-3xl font-semibold sm:text-4xl">Welcome back, {me.username}</h1>
        <p className="text-sm muted sm:text-base">
          Browse your libraries, jump into what&apos;s next, or join something live.
        </p>
      </header>

      {error && (
        <div className="notice-error rounded-xl px-4 py-2 text-sm">
          {error}
        </div>
      )}

      {/* Quick navigation tiles */}
      <section>
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <Link href="/libraries" className="tile tile-hover p-5 flex flex-col gap-1">
            <span className="font-semibold">Libraries</span>
            <span className="text-xs muted">
              {libraries.length > 0 ? `${libraries.length} librar${libraries.length === 1 ? 'y' : 'ies'}` : 'Browse media'}
            </span>
          </Link>

          <Link href="/channels" className="tile tile-hover p-5 flex flex-col gap-1">
            <span className="font-semibold">Channels</span>
            {voiceSession ? (
              <span className="flex items-center gap-1 text-xs text-green-400">
                <span className="h-1.5 w-1.5 rounded-full bg-green-400 animate-pulse" />
                {voiceSession.channelName}
              </span>
            ) : (
              <span className="text-xs muted">Voice &amp; text</span>
            )}
          </Link>

          <Link href="/watch-party" className="tile tile-hover p-5 flex flex-col gap-1">
            <span className="font-semibold">Watch Party</span>
            <span className="text-xs muted">Watch together</span>
          </Link>

          {me.role === 'admin' && (
            <Link href="/admin" className="tile tile-hover p-5 flex flex-col gap-1">
              <span className="font-semibold">Admin</span>
              <span className="text-xs muted">Manage server</span>
            </Link>
          )}
        </div>
      </section>

      {/* Libraries detail */}
      {loadingData ? (
        <div className="panel-soft px-4 py-3 text-sm muted">Loading libraries...</div>
      ) : libraries.length > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-xl font-semibold sm:text-2xl">Libraries</h2>
            <Link href="/libraries" className="text-sm text-[var(--orange-soft)]">View all</Link>
          </div>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            {libraries.map((library) => (
              <Link key={library.id} href={`/libraries/${library.id}`} className="tile tile-hover block p-4">
                <div className="flex items-center justify-between gap-2">
                  <h3 className="text-lg font-semibold">{library.name}</h3>
                  <span className="chip">{library.kind === 'tv_shows' ? 'TV' : library.kind === 'music' ? 'Music' : 'Movies'}</span>
                </div>
                <p className="mt-2 text-sm muted">{library.item_count} items</p>
              </Link>
            ))}
          </div>
        </section>
      )}

      {/* Active Voice Channels */}
      {activeVoiceChannels.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-xl font-semibold sm:text-2xl">Active Voice Channels</h2>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {activeVoiceChannels.map((ch) => {
              const members = voicePresence[ch.id] ?? [];
              return (
                <Link
                  key={ch.id}
                  href={`/channels?channel=${ch.id}`}
                  className="tile tile-hover p-4 flex flex-col gap-2"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-green-400 text-base leading-none">🔊</span>
                    <span className="font-semibold truncate">{ch.name}</span>
                    <span className="ml-auto chip text-xs">{members.length} online</span>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {members.map((u) => (
                      <span key={u.user_id} className="chip text-xs">
                        {u.username}
                      </span>
                    ))}
                  </div>
                </Link>
              );
            })}
          </div>
        </section>
      )}

      {/* Active Watch Parties */}
      {publicRooms.length > 0 && (
        <section className="space-y-3">
          <div className="flex items-center justify-between">
            <h2 className="text-xl font-semibold sm:text-2xl">Active Watch Parties</h2>
            <Link href="/watch-party" className="text-sm text-[var(--orange-soft)]">View all</Link>
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {publicRooms.map((room) => (
              <Link
                key={room.room_id}
                href={`/watch-party/rooms/${room.room_id}`}
                className="tile tile-hover p-4 flex flex-col gap-2"
              >
                <div className="flex items-start justify-between gap-2">
                  <p className="font-semibold truncate leading-snug">{room.title}</p>
                  <div className="flex gap-1 shrink-0">
                    <span className="chip text-xs">{room.room_mode === 'audio' ? '🎵' : '🎬'}</span>
                    {room.password_required && <span className="chip text-xs">🔒</span>}
                  </div>
                </div>
                <div className="flex items-center justify-between text-xs muted">
                  <span>Hosted by {room.host_username}</span>
                  <span>{room.member_count} watching</span>
                </div>
              </Link>
            ))}
          </div>
        </section>
      )}

      {/* Up Next */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Up Next</h2>
        {upNextItems.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">No items available yet.</div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
            {upNextItems.map((item) => (
              <Link key={`up-${item.id}`} href={`/items/${item.id}`} className="group block">
                <div className="tile tile-hover aspect-[2/3] overflow-hidden">
                  {item.poster_url ? (
                    <img
                      src={item.poster_url}
                      alt={item.title}
                      className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                    />
                  ) : (
                    <div className="flex h-full items-center justify-center px-2 text-center text-xs muted">
                      {item.kind.toUpperCase()}
                    </div>
                  )}
                </div>
                <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
