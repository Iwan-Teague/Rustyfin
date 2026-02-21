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
  thumb_url?: string;
}

interface Child {
  id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
}

export default function ItemPage() {
  const params = useParams();
  const id = params.id as string;
  const [item, setItem] = useState<Item | null>(null);
  const [children, setChildren] = useState<Child[]>([]);

  useEffect(() => {
    apiJson<Item>(`/items/${id}`).then(setItem).catch(() => {});
    apiJson<Child[]>(`/items/${id}/children`).then(setChildren).catch(() => {});
  }, [id]);

  if (!item) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading item...</p>
      </div>
    );
  }

  const isPlayable = item.kind === 'movie' || item.kind === 'episode';

  return (
    <div className="space-y-7 animate-rise">
      {item.backdrop_url && (
        <div className="tile relative h-64 overflow-hidden rounded-2xl sm:h-72">
          <img
            src={item.backdrop_url}
            alt=""
            className="h-full w-full object-cover opacity-45"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-[#1f2535] via-[#1f2535]/50 to-transparent" />
        </div>
      )}

      <div className="flex flex-col gap-6 lg:flex-row">
        {item.poster_url && (
          <div className="w-52 flex-shrink-0">
            <div className="tile overflow-hidden">
              <img src={item.poster_url} alt={item.title} className="w-full" />
            </div>
          </div>
        )}
        <div className="flex-1 space-y-4">
          <div className="space-y-2">
            <span className="chip chip-accent">{item.kind.toUpperCase()}</span>
            <h1 className="text-3xl font-semibold sm:text-4xl">{item.title}</h1>
            {item.year && <span className="text-sm muted">{item.year}</span>}
          </div>

          {item.overview && <p className="max-w-3xl leading-relaxed muted">{item.overview}</p>}

          {isPlayable && (
            <a
              href={`/player/${id}`}
              className="btn-primary inline-flex px-6 py-2.5 text-sm"
            >
              Play Now
            </a>
          )}
        </div>
      </div>

      {children.length > 0 && (
        <section className="space-y-4">
          <h2 className="text-xl font-semibold">
            {item.kind === 'series' ? 'Seasons' : 'Episodes'}
          </h2>
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4 lg:grid-cols-6">
            {children.map((child) => (
              <a
                key={child.id}
                href={`/items/${child.id}`}
                className="tile tile-hover block overflow-hidden"
              >
                {child.poster_url || child.thumb_url ? (
                  <img
                    src={child.poster_url || child.thumb_url}
                    alt={child.title}
                    className="aspect-[2/3] w-full object-cover"
                  />
                ) : (
                  <div className="flex aspect-[2/3] items-center justify-center bg-white/5 px-2 text-xs muted">
                    {child.kind.toUpperCase()}
                  </div>
                )}
                <div className="space-y-1 p-3">
                  <p className="font-medium text-sm">{child.title}</p>
                  {child.kind === 'episode' && (
                    <p className="text-xs muted">Episode</p>
                  )}
                </div>
              </a>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
