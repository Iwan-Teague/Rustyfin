'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { getNetworkTopology, NetworkTopologySnapshot } from '@/lib/networkApi';
import { clientErrorMessage } from '@/lib/errors';

function StatusBadge({ status }: { status: string }) {
  if (status === 'online') {
    return <span className="text-sm text-[var(--ok)]">Online</span>;
  }
  if (status === 'offline') {
    return <span className="text-sm muted">Offline</span>;
  }
  if (status === 'loopback') {
    return <span className="text-sm text-blue-300">Loopback</span>;
  }
  return <span className="text-sm text-white/72">{status}</span>;
}

export default function NetworkPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [topology, setTopology] = useState<NetworkTopologySnapshot | null>(null);
  const [activeView, setActiveView] = useState<'overview' | 'third-party'>('overview');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    let cancelled = false;
    if (authLoading || !me) return;

    setLoading(true);
    getNetworkTopology()
      .then((data) => {
        if (!cancelled) setTopology(data);
      })
      .catch((err) => {
        if (!cancelled) setError(clientErrorMessage(err, 'Failed to load network topology.'));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [authLoading, me]);

  if (authLoading) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Redirecting...</p>
      </div>
    );
  }

  return (
    <div className="animate-rise rf-flat-page">
      <header className="rf-flat-header">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold sm:text-3xl">Network</h1>
          <p className="max-w-3xl text-sm muted">
            Host network interfaces and topology status.
          </p>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <div className="rf-top-tabbar border-b border-[var(--border-subtle)] pb-0">
        <button
          className="rf-top-tab"
          data-active={activeView === 'overview'}
          onClick={() => setActiveView('overview')}
        >
          Overview
        </button>
        <button
          className="rf-top-tab"
          data-active={activeView === 'third-party'}
          onClick={() => setActiveView('third-party')}
        >
          Third Party
        </button>
      </div>

      {loading ? (
        <div className="rf-flat-empty text-center muted">Scanning network...</div>
      ) : activeView === 'third-party' ? (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_320px]">
          <section className="rf-flat-section space-y-5">
            <div className="space-y-2">
              <h2 className="text-xl font-semibold">WireGuard via Rustyfin</h2>
              <p className="max-w-3xl text-sm muted">
                This placeholder area is reserved for third-party network setup flows managed from
                Rustyfin. The first target here is WireGuard so users can bring remote network
                access online without leaving the product.
              </p>
            </div>

            <div className="rf-flat-list">
              <div className="rf-flat-row space-y-2">
                <h3 className="font-medium">Planned Setup Flow</h3>
                <p className="text-sm muted">Install or detect a host WireGuard runtime</p>
                <p className="text-sm muted">Generate server and peer configuration</p>
                <p className="text-sm muted">Download client config files and QR codes</p>
                <p className="text-sm muted">Show connection status and peer health inside Rustyfin</p>
              </div>

              <div className="rf-flat-row space-y-2">
                <h3 className="font-medium">Placeholder Inputs</h3>
                <div className="grid gap-4 md:grid-cols-2">
                  <label className="space-y-2 text-sm">
                    <span className="muted">Tunnel Network</span>
                    <input
                      disabled
                      value="10.8.0.0/24"
                      readOnly
                      className="w-full rounded-full border border-white/10 bg-black/10 px-4 py-3 text-white/70 outline-none"
                    />
                  </label>
                  <label className="space-y-2 text-sm">
                    <span className="muted">Listen Port</span>
                    <input
                      disabled
                      value="51820"
                      readOnly
                      className="w-full rounded-full border border-white/10 bg-black/10 px-4 py-3 text-white/70 outline-none"
                    />
                  </label>
                </div>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-4">
              <button className="btn-primary px-4 py-2 text-sm opacity-60" disabled>
                Set Up WireGuard
              </button>
              <p className="text-xs muted">
                Placeholder only. This section is here so the full WireGuard flow can land in the
                existing product structure later.
              </p>
            </div>
          </section>

          <aside className="space-y-6 lg:pt-11">
            <section className="rf-flat-section">
              <h3 className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-400">
                Planned Integrations
              </h3>
              <div className="rf-flat-list">
                <div className="rf-flat-row space-y-1">
                  <p className="font-medium text-white">WireGuard</p>
                  <p className="text-sm muted">Next network integration target</p>
                </div>
                <div className="rf-flat-row space-y-1">
                  <p className="font-medium text-white/70">More soon</p>
                  <p className="text-sm muted">
                    Additional third-party network services can slot into this view later.
                  </p>
                </div>
              </div>
            </section>

            <section className="rf-flat-section">
              <h3 className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-400">
                Host Context
              </h3>
              <div className="rf-flat-list">
                <div className="rf-flat-row space-y-1">
                  <p className="text-xs uppercase text-slate-500">Public Host</p>
                  <p className="font-mono text-sm">
                    {topology?.public_host || 'Not configured'}
                  </p>
                </div>
                <div className="rf-flat-row space-y-1">
                  <p className="text-xs uppercase text-slate-500">Remote Access</p>
                  <p className="text-sm">
                    {topology?.remote_access_enabled ? (
                      <span className="text-[var(--ok)]">Enabled</span>
                    ) : (
                      <span className="text-slate-400">Disabled</span>
                    )}
                  </p>
                </div>
              </div>
            </section>
          </aside>
        </div>
      ) : !topology ? (
        <div className="rf-flat-empty text-center muted">No topology data available.</div>
      ) : !topology.available ? (
        <div className="rf-flat-empty text-center">
          <p className="text-lg font-semibold">Topology Unavailable</p>
          <p className="muted">{topology.reason || 'Host network data cannot be accessed.'}</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_300px]">
          <section className="rf-flat-section">
            <h2 className="text-xl font-semibold">Interfaces</h2>
            <div className="rf-flat-list">
              {topology.nodes.map((node) => (
                <article key={node.name} className="rf-flat-row space-y-4">
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex min-w-0 items-center gap-3">
                      <div
                        className={`h-2.5 w-2.5 rounded-full ${node.status === 'online' ? 'bg-[var(--ok)]' : 'bg-slate-600'}`}
                      />
                      <h3 className="truncate font-mono text-base font-medium">{node.name}</h3>
                    </div>
                    <StatusBadge status={node.status} />
                  </div>

                  {node.addresses.length > 0 ? (
                    <div className="space-y-2">
                      {node.addresses.map((addr, idx) => (
                        <div
                          key={idx}
                          className="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--border-subtle)] px-3 py-2 text-sm font-mono text-slate-300"
                        >
                          <span className="w-10 text-xs uppercase text-slate-500">
                            {addr.family === 'inet' ? 'IPv4' : 'IPv6'}
                          </span>
                          <span>{addr.address}</span>
                          {addr.scope && (
                            <span className="ml-auto text-xs text-slate-500">{addr.scope}</span>
                          )}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-sm italic muted">No IP addresses assigned</p>
                  )}
                </article>
              ))}
            </div>
          </section>

          <div className="space-y-6 lg:pt-11">
            <section className="rf-flat-section">
              <h3 className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-400">
                Host Status
              </h3>
              <div className="rf-flat-list">
                <div className="rf-flat-row space-y-1">
                  <p className="text-xs uppercase text-slate-500">Host Label</p>
                  <p className="font-mono text-sm">{topology.host_label || 'Unknown'}</p>
                </div>

                <div className="rf-flat-row space-y-1">
                  <p className="text-xs uppercase text-slate-500">Public Host</p>
                  <p className="font-mono text-sm">{topology.public_host || 'Not configured'}</p>
                </div>

                <div className="rf-flat-row space-y-1">
                  <p className="text-xs uppercase text-slate-500">Remote Access</p>
                  <p className="text-sm">
                    {topology.remote_access_enabled ? (
                      <span className="text-[var(--ok)]">Enabled</span>
                    ) : (
                      <span className="text-slate-400">Disabled</span>
                    )}
                  </p>
                </div>
              </div>
            </section>

            <section className="rf-flat-section">
              <h3 className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-400">
                Summary
              </h3>
              <div className="grid grid-cols-2 gap-4 border-t border-[var(--border-subtle)] pt-4">
                <div>
                  <p className="text-2xl font-light">{topology.online_node_count}</p>
                  <p className="text-xs text-slate-500">Online</p>
                </div>
                <div>
                  <p className="text-2xl font-light">{topology.offline_node_count}</p>
                  <p className="text-xs text-slate-500">Offline</p>
                </div>
              </div>
            </section>

            {topology.trusted_proxies && topology.trusted_proxies.length > 0 && (
              <section className="rf-flat-section">
                <h3 className="text-sm font-semibold uppercase tracking-[0.22em] text-slate-400">
                  Trusted Proxies
                </h3>
                <div className="rf-flat-list">
                  {topology.trusted_proxies.map((ip) => (
                    <div key={ip} className="rf-flat-row">
                      <p className="font-mono text-xs text-slate-300">{ip}</p>
                    </div>
                  ))}
                </div>
              </section>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
