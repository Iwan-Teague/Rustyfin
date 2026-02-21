'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { apiJson } from '@/lib/api';
import { getPublicSystemInfo } from '@/lib/setupApi';

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

  const [setupChecked, setSetupChecked] = useState(false);
  const [setupComplete, setSetupComplete] = useState(true);
  const [loadingData, setLoadingData] = useState(false);
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [featuredItems, setFeaturedItems] = useState<Item[]>([]);
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
        const libs = await apiJson<Library[]>('/libraries');
        if (cancelled) return;
        setLibraries(libs);

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

  const recommendedItems = useMemo(() => {
    const preferred = featuredItems.filter(
      (item) => item.kind === 'movie' || item.kind === 'episode',
    );
    return preferred.length > 0 ? preferred.slice(0, 6) : featuredItems.slice(0, 6);
  }, [featuredItems]);

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
        <span className="chip chip-accent">Home</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Sign in to open your server home</h1>
        <p className="text-sm muted sm:text-base">
          Setup is complete. Sign in to see libraries, continue watching, up next, and recommended
          content.
        </p>
        <a href="/login" className="btn-primary inline-flex px-5 py-2.5 text-sm">
          Go to Login
        </a>
      </section>
    );
  }

  return (
    <div className="space-y-7 animate-rise">
      <header className="panel space-y-3 p-6 sm:p-8">
        <span className="chip chip-accent">Home</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Welcome back, {me.username}</h1>
        <p className="text-sm muted sm:text-base">
          Browse your libraries, jump into what&apos;s next, and pick up where you left off.
        </p>
      </header>

      {error && (
        <div className="notice-error rounded-xl px-4 py-2 text-sm">
          {error}
        </div>
      )}

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold sm:text-2xl">Libraries</h2>
          <a href="/libraries" className="text-sm text-[var(--orange-soft)]">
            View all
          </a>
        </div>
        {loadingData ? (
          <div className="panel-soft px-4 py-3 text-sm muted">Loading libraries...</div>
        ) : libraries.length === 0 ? (
          <div className="panel px-6 py-6">
            <p className="text-sm muted">No libraries are configured yet.</p>
            {me.role === 'admin' && (
              <a href="/admin" className="btn-primary mt-3 inline-flex px-4 py-2 text-sm">
                Create Library
              </a>
            )}
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            {libraries.map((library) => (
              <a key={library.id} href={`/libraries/${library.id}`} className="tile tile-hover block p-4">
                <div className="flex items-center justify-between gap-2">
                  <h3 className="text-lg font-semibold">{library.name}</h3>
                  <span className="chip">{library.kind === 'tv_shows' ? 'TV' : 'Movies'}</span>
                </div>
                <p className="mt-2 text-sm muted">{library.item_count} items</p>
              </a>
            ))}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Continue Watching</h2>
        <div className="panel-soft px-4 py-3 text-sm muted">
          Continue watching history appears here after you start playback.
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Up Next</h2>
        {upNextItems.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">No items available yet.</div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
            {upNextItems.map((item) => (
              <a key={`up-${item.id}`} href={`/items/${item.id}`} className="group block">
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
              </a>
            ))}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Recommended</h2>
        {recommendedItems.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">No recommendations yet.</div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
            {recommendedItems.map((item) => (
              <a key={`rec-${item.id}`} href={`/items/${item.id}`} className="group block">
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
                {item.year && <p className="text-xs muted">{item.year}</p>}
              </a>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

