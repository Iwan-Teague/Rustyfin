'use client';

import { useEffect, useState } from 'react';
import { useParams } from 'next/navigation';
import { apiJson } from '@/lib/api';

interface Item {
  id: string;
  title: string;
  kind: string;
  year?: number;
  overview?: string;
  poster_url?: string;
}

export default function LibraryPage() {
  const params = useParams();
  const id = params.id as string;
  const [items, setItems] = useState<Item[]>([]);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    apiJson<Item[]>(`/libraries/${id}/items`)
      .then(setItems)
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

  return (
    <div className="space-y-6 animate-rise">
      <header className="space-y-2">
        <span className="chip">Library View</span>
        <h1 className="text-3xl font-semibold">Library</h1>
        <p className="text-sm muted">Library ID: {id}</p>
        <p className="text-sm muted">
          Showing {visibleItems.length} of {items.length} items
        </p>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search titles in this library"
          className="input mt-2 w-full max-w-md px-3 py-2 text-sm"
        />
      </header>

      {visibleItems.length === 0 ? (
        <div className="panel px-6 py-8">
          <p className="text-sm muted">
            {items.length === 0
              ? 'No media items were found in this library yet.'
              : 'No items match your search.'}
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
          {visibleItems.map((item) => (
            <a key={item.id} href={`/items/${item.id}`} className="group block">
              <div className="tile tile-hover aspect-[2/3] overflow-hidden">
                {item.poster_url ? (
                  <img
                    src={item.poster_url}
                    alt={item.title}
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
            </a>
          ))}
        </div>
      )}
    </div>
  );
}
