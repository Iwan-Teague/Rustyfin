'use client';

import { useEffect, useState } from 'react';
import { apiJson } from '@/lib/api';

interface Library {
  id: string;
  name: string;
  kind: string;
  item_count: number;
}

export default function LibrariesPage() {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    apiJson<Library[]>('/libraries')
      .then(setLibraries)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading libraries...</p>
      </div>
    );
  }

  return (
    <div className="space-y-6 animate-rise">
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
          <a href="/admin" className="btn-primary mt-4 px-5 py-2 text-sm">
            Open Admin
          </a>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
          {libraries.map((lib) => (
            <a
              key={lib.id}
              href={`/libraries/${lib.id}`}
              className="tile tile-hover block p-5"
            >
              <div className="flex items-center justify-between gap-4">
                <h2 className="text-lg font-semibold">{lib.name}</h2>
                <span className="chip">{lib.kind === 'tv_shows' ? 'TV' : 'Movies'}</span>
              </div>
              <p className="mt-2 text-sm muted">
                {lib.kind} · {lib.item_count} items
              </p>
            </a>
          ))}
        </div>
      )}
    </div>
  );
}
