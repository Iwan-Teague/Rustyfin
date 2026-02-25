'use client';

import { useEffect, useMemo, useState } from 'react';
import { apiJson } from '@/lib/api';

export type MediaLibrary = {
  id: string;
  name: string;
  kind: string;
};

export type MediaItemNode = {
  id: string;
  library_id: string;
  title: string;
  kind: string;
  year?: number;
  poster_url?: string;
  thumb_url?: string;
};

type Breadcrumb = {
  id: string;
  title: string;
};

type Props = {
  libraries: MediaLibrary[];
  eligibleLibraryIds: string[];
  selectedLibraryId: string;
  selectedItem: MediaItemNode | null;
  layout?: 'split' | 'stacked';
  noShadow?: boolean;
  surfaceClassName?: string;
  onLibraryChange: (libraryId: string) => void;
  onSelectItem: (item: MediaItemNode | null) => void;
};

export default function MediaPicker({
  libraries,
  eligibleLibraryIds,
  selectedLibraryId,
  selectedItem,
  layout = 'split',
  noShadow = false,
  surfaceClassName = 'panel',
  onLibraryChange,
  onSelectItem,
}: Props) {
  const [breadcrumbs, setBreadcrumbs] = useState<Breadcrumb[]>([]);
  const [items, setItems] = useState<MediaItemNode[]>([]);
  const [search, setSearch] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const eligibleSet = useMemo(() => new Set(eligibleLibraryIds), [eligibleLibraryIds]);

  const visibleLibraries = useMemo(
    () => libraries.filter((library) => eligibleSet.has(library.id) && library.kind !== 'music'),
    [libraries, eligibleSet],
  );

  useEffect(() => {
    setBreadcrumbs([]);
    setItems([]);
    setSearch('');
    setError('');
  }, [selectedLibraryId]);

  useEffect(() => {
    let cancelled = false;

    async function loadItems() {
      if (!selectedLibraryId) {
        setItems([]);
        return;
      }

      setLoading(true);
      setError('');

      try {
        const endpoint = breadcrumbs.length
          ? `/items/${breadcrumbs[breadcrumbs.length - 1].id}/children`
          : `/libraries/${selectedLibraryId}/items`;
        const data = await apiJson<MediaItemNode[]>(endpoint);
        if (!cancelled) {
          setItems(data);
        }
      } catch (err: any) {
        if (!cancelled) {
          setError(err?.message || 'Failed to load media items');
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    void loadItems();

    return () => {
      cancelled = true;
    };
  }, [selectedLibraryId, breadcrumbs]);

  const filteredItems = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return items;
    return items.filter((item) => item.title.toLowerCase().includes(query));
  }, [items, search]);

  const canDrillDown = (item: MediaItemNode) => item.kind === 'series' || item.kind === 'season';
  const isPlayable = (item: MediaItemNode) => item.kind === 'movie' || item.kind === 'episode';

  return (
    <section
      className={`${surfaceClassName} space-y-4 p-5 sm:p-6`}
      style={noShadow ? { boxShadow: 'none' } : undefined}
    >
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Media Selection</h2>
        <p className="text-sm muted">
          Pick a movie or episode that is accessible to everyone invited.
        </p>
      </div>

      <div className={layout === 'stacked' ? 'space-y-3' : 'grid gap-3 md:grid-cols-[1fr_2fr]'}>
        <div className="space-y-2">
          <label htmlFor="watch-party-library" className="block text-xs uppercase tracking-wide muted">
            Library
          </label>
          <select
            id="watch-party-library"
            value={selectedLibraryId}
            onChange={(e) => {
              onLibraryChange(e.target.value);
              onSelectItem(null);
            }}
            className="select px-3 py-2 text-sm"
            aria-label="Library"
          >
            {visibleLibraries.length === 0 && <option value="">No shared libraries</option>}
            {visibleLibraries.map((library) => (
              <option key={library.id} value={library.id}>
                {library.name} ({library.kind === 'tv_shows' ? 'TV' : 'Movies'})
              </option>
            ))}
          </select>

          {selectedItem && (
            <div className="notice-ok rounded-xl px-3 py-2 text-xs">
              Selected: <strong>{selectedItem.title}</strong>
            </div>
          )}
        </div>

        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => {
                if (breadcrumbs.length === 0) return;
                setBreadcrumbs((prev) => prev.slice(0, -1));
              }}
              disabled={breadcrumbs.length === 0}
              className="btn-secondary px-3 py-1.5 text-xs disabled:opacity-50"
            >
              Back
            </button>
            <span className="text-xs muted">
              {breadcrumbs.length === 0
                ? 'Top level'
                : breadcrumbs.map((crumb) => crumb.title).join(' / ')}
            </span>
          </div>

          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="input px-3 py-2 text-sm"
            placeholder="Search titles"
            aria-label="Search media titles"
          />

          {loading ? (
            <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">Loading media…</div>
          ) : error ? (
            <div className="notice-error rounded-xl px-3 py-3 text-sm">{error}</div>
          ) : filteredItems.length === 0 ? (
            <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">No media found at this level.</div>
          ) : (
            <ul className="space-y-2">
              {filteredItems.map((item) => {
                const selected = selectedItem?.id === item.id;
                return (
                  <li
                    key={item.id}
                    className={`tile rounded-xl px-3 py-2 ${selected ? 'border-[var(--orange-soft)]' : ''}`}
                  >
                    <div className="flex items-center gap-3">
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{item.title}</p>
                        <p className="text-xs muted">
                          {item.kind}
                          {item.year ? ` • ${item.year}` : ''}
                        </p>
                      </div>

                      {canDrillDown(item) && (
                        <button
                          type="button"
                          onClick={() => setBreadcrumbs((prev) => [...prev, { id: item.id, title: item.title }])}
                          className="btn-secondary px-3 py-1.5 text-xs"
                        >
                          Open
                        </button>
                      )}

                      {isPlayable(item) && (
                        <button
                          type="button"
                          onClick={() => onSelectItem(item)}
                          className={`px-3 py-1.5 text-xs ${selected ? 'btn-primary' : 'btn-secondary'}`}
                        >
                          {selected ? 'Selected' : 'Select'}
                        </button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}
