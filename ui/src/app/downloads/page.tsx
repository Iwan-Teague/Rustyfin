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

function installModeLabel(value?: string | null) {
  if (value === 'download_zip_extract_then_load_unpacked') {
    return 'Load unpacked';
  }
  return 'Manual install';
}

function availabilityLabel(value: DownloadArtifact['availability']) {
  if (value === 'available') return 'Available now';
  if (value === 'unavailable') return 'Unavailable';
  return 'Planned';
}

export default function DownloadsPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [catalog, setCatalog] = useState<DownloadArtifact[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [downloadPending, setDownloadPending] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [catalogWarning, setCatalogWarning] = useState<string | null>(null);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    let cancelled = false;

    if (authLoading) {
      return () => {
        cancelled = true;
      };
    }

    if (!me) {
      setCatalogLoading(false);
      return () => {
        cancelled = true;
      };
    }

    setCatalogLoading(true);
    setCatalogWarning(null);

    getDownloadsCatalog()
      .then((response) => {
        if (!cancelled) {
          setCatalog(response.items);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setCatalog([]);
          setCatalogWarning(clientErrorMessage(err, 'Could not load the current download catalog.'));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setCatalogLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [authLoading, me]);

  const currentRelease = catalog.find((item) => item.availability !== 'planned') || null;
  const plannedDownloads = catalog.filter((item) => item.availability === 'planned');
  const availableNowCount = catalog.filter((item) => item.availability === 'available').length;

  async function handleArtifactDownload(artifact: DownloadArtifact) {
    setDownloadPending(true);
    setMessage(null);
    setError(null);
    try {
      const { blob, filename } = await downloadCatalogArtifactPackage(artifact);
      downloadBlob(filename, blob);
      setMessage(`${artifact.title} downloaded.`);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to download the selected package.'));
    } finally {
      setDownloadPending(false);
    }
  }

  if (authLoading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading downloads...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Redirecting to login...</p>
      </div>
    );
  }

  return (
    <div className="space-y-7 animate-rise">
      <header className="panel overflow-hidden p-6 sm:p-8">
        <div className="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
          <div className="space-y-4">
            <span className="chip border-[var(--border-strong)] bg-black/20 text-white/90">
              Official Rustyfin downloads
            </span>
            <div className="space-y-2">
              <h1 className="text-3xl font-semibold sm:text-4xl">Downloads</h1>
              <p className="max-w-3xl text-sm muted sm:text-base">
                Get official Rustyfin packages from one place. Current host-available releases appear here,
                and future first-party applications and companion downloads can land here without moving the
                install flow again.
              </p>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="panel-soft min-w-[11rem] px-4 py-4">
              <p className="text-xs uppercase tracking-[0.24em] text-white/60">Available now</p>
              <p className="mt-2 text-2xl font-semibold">{availableNowCount}</p>
              <p className="mt-1 text-sm muted">
                {availableNowCount === 1 ? 'Authenticated package' : 'Authenticated packages'}
              </p>
            </div>
            <div className="panel-soft min-w-[11rem] px-4 py-4">
              <p className="text-xs uppercase tracking-[0.24em] text-white/60">Planned</p>
              <p className="mt-2 text-2xl font-semibold">{plannedDownloads.length}</p>
              <p className="mt-1 text-sm muted">Future clients and companion releases</p>
            </div>
          </div>
        </div>
      </header>

      {message && <div className="panel-soft px-4 py-3 text-sm text-[var(--ok)]">{message}</div>}
      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <section className="grid grid-cols-1 gap-7 xl:grid-cols-[minmax(0,1.8fr)_minmax(18rem,1fr)]">
        <article className="panel space-y-6 p-6 sm:p-8">
          {currentRelease ? (
            <>
              <div className="flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between">
                <div className="flex items-start gap-4">
                  <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl border border-[var(--border-strong)] bg-black/25 shadow-[0_18px_40px_rgba(0,0,0,0.22)]">
                    <svg viewBox="0 0 24 24" className="h-6 w-6 text-[var(--orange-soft)]" fill="none" aria-hidden="true">
                      <path d="M12 4v9" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                      <path d="m8.5 10.5 3.5 3.5 3.5-3.5" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
                      <rect x="4" y="16" width="16" height="4" rx="2" stroke="currentColor" strokeWidth="1.9" />
                    </svg>
                  </div>

                  <div className="space-y-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="text-2xl font-semibold">{currentRelease.title}</h2>
                      {catalogLoading ? (
                        <span className="chip">Loading metadata</span>
                      ) : currentRelease.version ? (
                        <span className="chip chip-accent">v{currentRelease.version}</span>
                      ) : (
                        <span className="chip">{availabilityLabel(currentRelease.availability)}</span>
                      )}
                    </div>
                    <p className="max-w-2xl text-sm muted sm:text-base">{currentRelease.summary}</p>
                  </div>
                </div>

                <div className="flex flex-wrap gap-2">
                  <span className="chip">{availabilityLabel(currentRelease.availability)}</span>
                  <span className="chip">
                    {catalogLoading ? 'Loading details' : installModeLabel(currentRelease.install_mode)}
                  </span>
                  {currentRelease.requires_sign_in && <span className="chip">Requires sign-in</span>}
                </div>
              </div>

              <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
                <div className="panel-soft px-4 py-4">
                  <p className="text-xs uppercase tracking-[0.24em] text-white/60">Use case</p>
                  <p className="mt-2 font-medium">{currentRelease.summary}</p>
                </div>
                <div className="panel-soft px-4 py-4">
                  <p className="text-xs uppercase tracking-[0.24em] text-white/60">Package</p>
                  <p className="mt-2 font-medium">
                    {catalogLoading ? 'Loading package metadata...' : currentRelease.package_filename || 'Not currently downloadable'}
                  </p>
                </div>
                <div className="panel-soft px-4 py-4">
                  <p className="text-xs uppercase tracking-[0.24em] text-white/60">Setup</p>
                  <p className="mt-2 font-medium">
                    {currentRelease.setup_path ? 'Continue from the Vault page' : currentRelease.detail}
                  </p>
                </div>
              </div>

              <div className="flex flex-wrap gap-3">
                <button
                  type="button"
                  className="btn-primary px-5 py-3 text-sm"
                  disabled={downloadPending || currentRelease.availability !== 'available'}
                  onClick={() => handleArtifactDownload(currentRelease)}
                >
                  {downloadPending ? 'Downloading...' : currentRelease.availability === 'available' ? 'Download package' : 'Download unavailable'}
                </button>
                {currentRelease.setup_path && (
                  <Link href={currentRelease.setup_path} className="btn-secondary px-5 py-3 text-sm">
                    Open setup
                  </Link>
                )}
              </div>

              <p className="text-sm muted">{currentRelease.detail}</p>

              {catalogWarning && (
                <div className="panel-soft px-4 py-3 text-sm muted">{catalogWarning}</div>
              )}
            </>
          ) : (
            <div className="panel-soft px-4 py-4 text-sm muted">
              No authenticated downloads are currently available on this host.
            </div>
          )}
        </article>

        <div className="space-y-4">
          <aside className="panel-soft space-y-4 px-5 py-5">
            <div>
              <p className="text-sm font-semibold">Install flow</p>
              <p className="mt-1 text-sm muted">
                Download availability is published by the host, while pairing and protected actions stay on the
                owning product surface.
              </p>
            </div>
            <div className="space-y-3">
              {(currentRelease?.install_steps || []).map((step, index) => (
                <div key={step} className="flex items-start gap-3">
                  <span className="chip mt-0.5 min-w-[1.9rem] justify-center">{index + 1}</span>
                  <p className="text-sm muted">{step}</p>
                </div>
              ))}
            </div>
          </aside>

          <aside className="panel-soft space-y-4 px-5 py-5">
            <div>
              <p className="text-sm font-semibold">After download</p>
              <p className="mt-1 text-sm muted">
                Pairing, device revocation, and short-lived protected actions remain on the owning feature page
                so the security flow stays in one place.
              </p>
            </div>
            {currentRelease?.setup_path ? (
              <Link href={currentRelease.setup_path} className="btn-secondary w-full px-4 py-3 text-sm">
                Go to setup
              </Link>
            ) : (
              <button type="button" className="btn-secondary w-full px-4 py-3 text-sm opacity-65" disabled>
                No setup available
              </button>
            )}
          </aside>
        </div>
      </section>

      <section className="panel space-y-5 p-6 sm:p-8">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h2 className="text-2xl font-semibold">Coming Soon</h2>
            <p className="mt-1 text-sm muted sm:text-base">
              This route is intended to grow into the main release surface for first-party Rustyfin downloads.
            </p>
          </div>
          <span className="chip">Release-ready structure</span>
        </div>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          {plannedDownloads.map((item) => (
            <article key={item.id} className="tile space-y-4 px-5 py-5">
              <div className="flex items-start justify-between gap-3">
                <div>
                  <h3 className="text-lg font-semibold">{item.title}</h3>
                  <p className="mt-1 text-sm muted">{item.summary}</p>
                </div>
                <span className="chip">{availabilityLabel(item.availability)}</span>
              </div>
              <p className="text-sm muted">{item.detail}</p>
              <button
                type="button"
                className="btn-secondary px-4 py-2 text-sm opacity-65"
                disabled
              >
                Coming soon
              </button>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}
