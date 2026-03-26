'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import {
  downloadCatalogArtifactPackage,
  getDownloadsCatalog,
  type DownloadArtifact,
} from '@/lib/downloadsApi';
import { clientErrorMessage } from '@/lib/errors';

function downloadBlob(filename: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

function availabilityLabel(value: DownloadArtifact['availability']) {
  if (value === 'available') return 'Available';
  if (value === 'unavailable') return 'Unavailable';
  return 'Planned';
}

function PlatformSection({
  title,
  items,
  onDownload,
  pendingId,
}: {
  title: string;
  items: DownloadArtifact[];
  onDownload: (artifact: DownloadArtifact) => Promise<void>;
  pendingId: string | null;
}) {
  if (items.length === 0) return null;

  return (
    <section className="space-y-4">
      <h2 className="text-xl font-semibold">{title}</h2>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        {items.map((item) => (
          <article key={item.id} className="tile flex flex-col justify-between space-y-4 px-5 py-5">
            <div className="space-y-2">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h3 className="text-lg font-semibold">{item.title}</h3>
                  <div className="mt-1 flex flex-wrap gap-2">
                    {item.version && <span className="chip chip-accent">v{item.version}</span>}
                    <span className="chip">{availabilityLabel(item.availability)}</span>
                    {item.channel !== 'stable' && <span className="chip">{item.channel}</span>}
                  </div>
                </div>
              </div>
              <p className="text-sm muted">{item.summary}</p>
              {item.detail && <p className="text-xs text-slate-400">{item.detail}</p>}
            </div>

            <div className="flex flex-wrap gap-3 pt-2">
              {item.availability === 'available' ? (
                <>
                  {item.distribution_mode === 'external_store' && item.external_url ? (
                    <a
                      href={item.external_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="btn-primary w-full sm:w-auto px-4 py-2 text-sm text-center"
                    >
                      Get on Store
                    </a>
                  ) : (
                    <button
                      type="button"
                      className="btn-primary w-full sm:w-auto px-4 py-2 text-sm"
                      disabled={pendingId === item.id}
                      onClick={() => onDownload(item)}
                    >
                      {pendingId === item.id ? 'Downloading...' : 'Download Package'}
                    </button>
                  )}
                  {item.setup_path && (
                    <Link
                      href={item.setup_path}
                      className="btn-secondary w-full sm:w-auto px-4 py-2 text-sm text-center"
                    >
                      Setup
                    </Link>
                  )}
                </>
              ) : (
                <button type="button" className="btn-secondary w-full sm:w-auto px-4 py-2 text-sm opacity-65" disabled>
                  Coming Soon
                </button>
              )}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

export default function DownloadsPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [catalog, setCatalog] = useState<DownloadArtifact[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [downloadPendingId, setDownloadPendingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    let cancelled = false;
    if (authLoading || !me) return;

    setCatalogLoading(true);
    getDownloadsCatalog()
      .then((response) => {
        if (!cancelled) setCatalog(response.items);
      })
      .catch((err) => {
        if (!cancelled) setError(clientErrorMessage(err, 'Could not load downloads.'));
      })
      .finally(() => {
        if (!cancelled) setCatalogLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [authLoading, me]);

  async function handleDownload(artifact: DownloadArtifact) {
    setDownloadPendingId(artifact.id);
    setMessage(null);
    setError(null);
    try {
      const { blob, filename } = await downloadCatalogArtifactPackage(artifact);
      downloadBlob(filename, blob);
      setMessage(`${artifact.title} downloaded.`);
    } catch (err) {
      setError(clientErrorMessage(err, 'Download failed.'));
    } finally {
      setDownloadPendingId(null);
    }
  }

  if (authLoading) {
    return <div className="panel-soft animate-rise px-5 py-4"><p className="text-sm muted">Loading...</p></div>;
  }

  if (!me) {
    return <div className="panel-soft animate-rise px-5 py-4"><p className="text-sm muted">Redirecting...</p></div>;
  }

  const desktop = catalog.filter((i) => ['windows', 'macos', 'linux'].includes(i.platform));
  const mobile = catalog.filter((i) => ['android', 'ios'].includes(i.platform));
  const other = catalog.filter((i) => !['windows', 'macos', 'linux', 'android', 'ios'].includes(i.platform));

  return (
    <div className="space-y-8 animate-rise">
      <header className="panel overflow-hidden p-6 sm:p-8">
        <div className="space-y-4">
          <span className="chip border-[var(--border-strong)] bg-black/20 text-white/90">
            Official Releases
          </span>
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold sm:text-4xl">Downloads</h1>
            <p className="max-w-3xl text-sm muted sm:text-base">
              Get official Rustyfin clients and extensions directly from your host.
            </p>
          </div>
        </div>
      </header>

      {message && <div className="panel-soft px-4 py-3 text-sm text-[var(--ok)]">{message}</div>}
      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      {catalogLoading ? (
        <div className="panel-soft px-5 py-8 text-center muted">Loading catalog...</div>
      ) : (
        <div className="space-y-10">
          <PlatformSection title="Desktop" items={desktop} onDownload={handleDownload} pendingId={downloadPendingId} />
          <PlatformSection title="Mobile" items={mobile} onDownload={handleDownload} pendingId={downloadPendingId} />
          <PlatformSection title="Other" items={other} onDownload={handleDownload} pendingId={downloadPendingId} />
          
          {catalog.length === 0 && (
             <div className="panel-soft px-5 py-8 text-center muted">
               No downloads available at this time.
             </div>
          )}
        </div>
      )}
    </div>
  );
}
