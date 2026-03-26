'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { getNetworkTopology, NetworkTopologySnapshot } from '@/lib/networkApi';
import { clientErrorMessage } from '@/lib/errors';

function StatusBadge({ status }: { status: string }) {
  if (status === 'online') {
    return <span className="chip text-[var(--ok)] bg-[var(--ok-dim)] border-[var(--ok)]">Online</span>;
  }
  if (status === 'offline') {
    return <span className="chip muted">Offline</span>;
  }
  if (status === 'loopback') {
    return <span className="chip text-blue-300 bg-blue-900/20 border-blue-500/30">Loopback</span>;
  }
  return <span className="chip">{status}</span>;
}

export default function NetworkPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [topology, setTopology] = useState<NetworkTopologySnapshot | null>(null);
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
    return <div className="panel-soft animate-rise px-5 py-4"><p className="text-sm muted">Loading...</p></div>;
  }

  if (!me) {
    return <div className="panel-soft animate-rise px-5 py-4"><p className="text-sm muted">Redirecting...</p></div>;
  }

  return (
    <div className="space-y-8 animate-rise">
      <header className="panel overflow-hidden p-6 sm:p-8">
        <div className="space-y-4">
          <span className="chip border-[var(--border-strong)] bg-black/20 text-white/90">
            Network Topology
          </span>
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold sm:text-4xl">Network</h1>
            <p className="max-w-3xl text-sm muted sm:text-base">
              Host network interfaces and topology status.
            </p>
          </div>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      {loading ? (
        <div className="panel-soft px-5 py-8 text-center muted">Scanning network...</div>
      ) : !topology ? (
        <div className="panel-soft px-5 py-8 text-center muted">No topology data available.</div>
      ) : !topology.available ? (
         <div className="panel-soft px-5 py-8 text-center space-y-2">
            <p className="font-semibold text-lg">Topology Unavailable</p>
            <p className="muted">{topology.reason || 'Host network data cannot be accessed.'}</p>
         </div>
      ) : (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1fr_300px]">
          <div className="space-y-6">
            <h2 className="text-xl font-semibold">Interfaces</h2>
            <div className="grid grid-cols-1 gap-4">
              {topology.nodes.map((node) => (
                <article key={node.name} className="tile p-5 space-y-4">
                   <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className={`w-3 h-3 rounded-full ${node.status === 'online' ? 'bg-[var(--ok)] shadow-[0_0_8px_var(--ok)]' : 'bg-slate-600'}`} />
                        <h3 className="text-lg font-medium font-mono">{node.name}</h3>
                      </div>
                      <StatusBadge status={node.status} />
                   </div>
                   
                   {node.addresses.length > 0 ? (
                     <div className="space-y-2">
                        {node.addresses.map((addr, idx) => (
                          <div key={idx} className="flex items-center gap-3 text-sm font-mono text-slate-300 bg-black/20 p-2 rounded">
                             <span className="text-xs uppercase text-slate-500 w-10">{addr.family === 'inet' ? 'IPv4' : 'IPv6'}</span>
                             <span>{addr.address}</span>
                             {addr.scope && <span className="text-xs text-slate-500 ml-auto">{addr.scope}</span>}
                          </div>
                        ))}
                     </div>
                   ) : (
                     <p className="text-sm muted italic">No IP addresses assigned</p>
                   )}
                </article>
              ))}
            </div>
          </div>

          <div className="space-y-6">
             <section className="panel-soft p-5 space-y-4">
                <h3 className="font-semibold text-sm uppercase tracking-wider text-slate-400">Host Status</h3>
                
                <div className="space-y-1">
                   <p className="text-xs text-slate-500 uppercase">Host Label</p>
                   <p className="font-mono text-sm">{topology.host_label || 'Unknown'}</p>
                </div>

                <div className="space-y-1">
                   <p className="text-xs text-slate-500 uppercase">Public Host</p>
                   <p className="font-mono text-sm">{topology.public_host || 'Not configured'}</p>
                </div>

                <div className="space-y-1">
                   <p className="text-xs text-slate-500 uppercase">Remote Access</p>
                   <p className="text-sm">
                      {topology.remote_access_enabled ? (
                        <span className="text-[var(--ok)]">Enabled</span>
                      ) : (
                        <span className="text-slate-400">Disabled</span>
                      )}
                   </p>
                </div>
             </section>

             <section className="panel-soft p-5 space-y-4">
                <h3 className="font-semibold text-sm uppercase tracking-wider text-slate-400">Summary</h3>
                 <div className="grid grid-cols-2 gap-4">
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
                <section className="panel-soft p-5 space-y-4">
                    <h3 className="font-semibold text-sm uppercase tracking-wider text-slate-400">Trusted Proxies</h3>
                    <div className="space-y-1">
                        {topology.trusted_proxies.map(ip => (
                            <p key={ip} className="font-mono text-xs text-slate-300">{ip}</p>
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
