'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { apiJson } from '@/lib/api';

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

export default function LibrariesPage() {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [loading, setLoading] = useState(true);
  const [featuredItems, setFeaturedItems] = useState<Item[]>([]);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const libs = await apiJson<Library[]>('/libraries');
        if (cancelled) return;
        setLibraries(libs);
        setLoading(false);

        const itemArrays = await Promise.all(
          libs.slice(0, 4).map((lib) =>
            apiJson<Item[]>(`/libraries/${lib.id}/items`).catch(() => [] as Item[]),
          ),
        );
        if (cancelled) return;
        setFeaturedItems(itemArrays.flat().slice(0, 24));
      } catch {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, []);

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
        <span className="chip">Collection Browser</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Libraries</h1>
        <p className="text-sm muted sm:text-base">
          Explore all configured media directories and jump into items instantly.
        </p>
      </header>

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
              className="tile tile-hover block p-5"
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
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Continue Watching</h2>
        <div className="panel-soft px-4 py-3 text-sm muted">
          Continue watching history appears here after you start playback.
        </div>
      </section>

      {/* Recommended */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Recommended</h2>
        {recommendedItems.length === 0 ? (
          <div className="panel-soft px-4 py-3 text-sm muted">No recommendations yet.</div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-6">
            {recommendedItems.map((item) => (
              <Link key={`rec-${item.id}`} href={`/items/${item.id}`} className="group block">
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
              </Link>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
