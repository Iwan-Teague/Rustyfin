'use client';

import { useEffect, useMemo, useState } from 'react';
import { apiJson } from '@/lib/api';
import { clientErrorMessage } from '@/lib/errors';

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
  applyActionLabel?: string;
  applyActionPendingLabel?: string;
  applyActionDisabled?: boolean;
  applyActionLoading?: boolean;
  onApplyAction?: () => void;
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
  surfaceClassName = 'rf-flat-section',
  applyActionLabel = 'Apply Media',
  applyActionPendingLabel = 'Applying…',
  applyActionDisabled = false,
  applyActionLoading = false,
  onApplyAction,
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
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load media items'));
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
      className={`${surfaceClassName} space-y-4`}
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
            className="rf-flat-input px-3 py-2 text-sm"
            aria-label="Library"
          >
            {visibleLibraries.length === 0 && <option value="">No shared libraries</option>}
            {visibleLibraries.map((library) => (
              <option key={library.id} value={library.id}>
                {library.name} ({library.kind === 'tv_shows' ? 'TV' : 'Movies'})
              </option>
            ))}
          </select>
        </div>

        <div className="space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
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
            {onApplyAction && (
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                onClick={onApplyAction}
                disabled={applyActionLoading || applyActionDisabled}
              >
                {applyActionLoading ? applyActionPendingLabel : applyActionLabel}
              </button>
            )}
          </div>

          <div className="relative">
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="rf-flat-input w-full px-3 py-2 pr-10 text-sm"
              placeholder="Search titles"
              aria-label="Search media titles"
            />
            {search.trim().length > 0 && (
              <button
                type="button"
                onClick={() => setSearch('')}
                className="absolute right-2 top-1/2 inline-flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded-full border border-white/25 text-white/75 transition hover:border-white/50 hover:text-white"
                aria-label="Clear search"
                title="Clear search"
              >
                <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" aria-hidden="true">
                  <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.8" />
                  <path
                    d="M9 9l6 6M15 9l-6 6"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                  />
                </svg>
              </button>
            )}
          </div>

          {loading ? (
            <div className="rf-flat-empty text-sm muted">Loading media…</div>
          ) : error ? (
            <div className="notice-error rounded-xl px-3 py-3 text-sm">{error}</div>
          ) : filteredItems.length === 0 ? (
            <div className="rf-flat-empty text-sm muted">No media found at this level.</div>
          ) : (
            <div className="max-h-[26rem] overflow-y-auto pr-1">
              <ul className="rf-flat-list">
                {filteredItems.map((item) => {
                  const selected = selectedItem?.id === item.id;
                  return (
                    <li
                      key={item.id}
                      className={`rf-flat-row ${selected ? 'border-[var(--orange-soft)]' : ''}`}
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
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
