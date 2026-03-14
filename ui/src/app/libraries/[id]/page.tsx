'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import Image from 'next/image';
import { apiJson } from '@/lib/api';

interface Library {
  id: string;
  name: string;
  kind: string;
}

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
}

export default function LibraryPage() {
  const params = useParams();
  const id = params.id as string;
  const [library, setLibrary] = useState<Library | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      apiJson<Library>(`/libraries/${id}`),
      apiJson<Item[]>(`/libraries/${id}/items`),
    ])
      .then(([lib, libItems]) => {
        setLibrary(lib);
        setItems(libItems);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [id]);

  const q = query.trim().toLowerCase();
  const visibleItems = q
    ? items.filter((item) => item.title.toLowerCase().includes(q))
    : items;

  if (loading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading library...</p>
      </div>
    );
  }

  const isMusic = library?.kind === 'music';

  return (
    <div className="space-y-6 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold">{library?.name ?? 'Library'}</h1>
        <p className="text-sm muted">
          Showing {visibleItems.length} of {items.length} {isMusic ? 'artists' : 'items'}
        </p>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={isMusic ? 'Search artists…' : 'Search titles in this library'}
          className="input mt-2 w-full max-w-md px-3 py-2 text-sm"
        />
      </header>

      {visibleItems.length === 0 ? (
        <div className="panel px-6 py-8">
          <p className="text-sm muted">
            {items.length === 0
              ? 'No media items were found in this library yet. Try scanning the library.'
              : 'No items match your search.'}
          </p>
        </div>
      ) : isMusic ? (
        /* ── Music: square artist grid ── */
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
          {visibleItems.map((item) => (
            <Link key={item.id} href={`/items/${item.id}`} className="group block">
              <div className="tile tile-hover aspect-square overflow-hidden">
                {item.poster_url ? (
                  <Image
                    src={item.poster_url}
                    alt={item.title}
                    width={320}
                    height={480}
                    unoptimized
                    className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                  />
                ) : (
                  <div className="flex h-full w-full items-center justify-center text-4xl muted">
                    ♪
                  </div>
                )}
              </div>
              <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
              <p className="text-xs muted">Artist</p>
            </Link>
          ))}
        </div>
      ) : (
        /* ── Video: existing poster grid ── */
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
          {visibleItems.map((item) => (
            <Link key={item.id} href={`/items/${item.id}`} className="group block">
              <div className="tile tile-hover aspect-[2/3] overflow-hidden">
                {item.poster_url ? (
                  <Image
                    src={item.poster_url}
                    alt={item.title}
                    width={320}
                    height={480}
                    unoptimized
                    className="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                  />
                ) : (
                  <div className="flex h-full w-full items-center justify-center text-xs muted">
                    No Poster
                  </div>
                )}
              </div>
              <p className="mt-2 truncate text-sm font-medium">{item.title}</p>
              {item.year && <p className="text-xs muted">{item.year}</p>}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
