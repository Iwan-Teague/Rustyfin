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

  if (loading) return <p className="text-gray-400">Loading...</p>;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Libraries</h1>
      {libraries.length === 0 ? (
        <p className="text-gray-400">No libraries found. Create one from the admin panel.</p>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {libraries.map((lib) => (
            <a
              key={lib.id}
              href={`/libraries/${lib.id}`}
              className="block p-5 bg-gray-900 rounded-lg border border-gray-800 hover:border-blue-500 transition"
            >
              <h2 className="text-lg font-semibold">{lib.name}</h2>
              <p className="text-gray-400 text-sm mt-1">
                {lib.kind} · {lib.item_count} items
              </p>
            </a>
          ))}
        </div>
      )}
    </div>
  );
}
