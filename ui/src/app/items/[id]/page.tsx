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
  backdrop_url?: string;
}

interface Child {
  id: string;
  title: string;
  kind: string;
  year?: number;
}

export default function ItemPage() {
  const params = useParams();
  const id = params.id as string;
  const [item, setItem] = useState<Item | null>(null);
  const [children, setChildren] = useState<Child[]>([]);
  const [fileId, setFileId] = useState<string | null>(null);

  useEffect(() => {
    apiJson<Item>(`/items/${id}`).then(setItem).catch(() => {});
    apiJson<Child[]>(`/items/${id}/children`).then(setChildren).catch(() => {});
  }, [id]);

  if (!item) return <p className="text-gray-400">Loading...</p>;

  const isPlayable = item.kind === 'movie' || item.kind === 'episode';

  return (
    <div className="space-y-6">
      {item.backdrop_url && (
        <div className="relative h-64 -mx-6 -mt-8 mb-6 overflow-hidden">
          <img
            src={item.backdrop_url}
            alt=""
            className="w-full h-full object-cover opacity-40"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-gray-950 to-transparent" />
        </div>
      )}

      <div className="flex gap-6">
        {item.poster_url && (
          <div className="w-48 flex-shrink-0">
            <img src={item.poster_url} alt={item.title} className="w-full rounded-lg" />
          </div>
        )}
        <div className="flex-1 space-y-4">
          <h1 className="text-3xl font-bold">{item.title}</h1>
          {item.year && <span className="text-gray-400">{item.year}</span>}
          {item.overview && <p className="text-gray-300 leading-relaxed">{item.overview}</p>}

          {isPlayable && (
            <a
              href={`/player/${id}`}
              className="inline-block px-6 py-2 bg-blue-600 hover:bg-blue-700 rounded font-semibold transition"
            >
              ▶ Play
            </a>
          )}
        </div>
      </div>

      {children.length > 0 && (
        <div>
          <h2 className="text-xl font-semibold mb-4">
            {item.kind === 'series' ? 'Seasons' : 'Episodes'}
          </h2>
          <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3">
            {children.map((child) => (
              <a
                key={child.id}
                href={`/items/${child.id}`}
                className="block p-3 bg-gray-900 rounded border border-gray-800 hover:border-blue-500 transition"
              >
                <p className="font-medium text-sm">{child.title}</p>
                {child.kind === 'episode' && (
                  <p className="text-xs text-gray-500 mt-1">Episode</p>
                )}
              </a>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
