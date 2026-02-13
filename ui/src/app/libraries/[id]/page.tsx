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
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    apiJson<Item[]>(`/libraries/${id}/items`)
      .then(setItems)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [id]);

  if (loading) return <p className="text-gray-400">Loading...</p>;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Library</h1>
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4">
        {items.map((item) => (
          <a
            key={item.id}
            href={`/items/${item.id}`}
            className="group block"
          >
            <div className="aspect-[2/3] bg-gray-800 rounded-lg overflow-hidden mb-2">
              {item.poster_url ? (
                <img
                  src={item.poster_url}
                  alt={item.title}
                  className="w-full h-full object-cover group-hover:scale-105 transition"
                />
              ) : (
                <div className="w-full h-full flex items-center justify-center text-gray-600 text-xs">
                  No Poster
                </div>
              )}
            </div>
            <p className="text-sm font-medium truncate">{item.title}</p>
            {item.year && <p className="text-xs text-gray-500">{item.year}</p>}
          </a>
        ))}
      </div>
    </div>
  );
}
