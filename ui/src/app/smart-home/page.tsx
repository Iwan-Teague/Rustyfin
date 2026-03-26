'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { getSmartHomeState, SmartDevice, SmartHomeSummary } from '@/lib/smartHomeApi';
import { clientErrorMessage } from '@/lib/errors';

function DeviceIcon({ type }: { type: SmartDevice['device_type'] }) {
  if (type === 'camera') {
    return (
      <svg className="w-5 h-5 text-slate-300" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }
  if (type === 'light') {
    return (
      <svg className="w-5 h-5 text-amber-300" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }
  if (type === 'door_lock') {
    return (
      <svg className="w-5 h-5 text-emerald-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
        <path d="M7 11V7a5 5 0 0110 0v4" />
      </svg>
    );
  }
  if (type === 'alarm') {
    return (
      <svg className="w-5 h-5 text-red-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
      </svg>
    );
  }
  return (
    <svg className="w-5 h-5 text-slate-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
       <circle cx="12" cy="12" r="10" />
       <path d="M12 6v6l4 2" />
    </svg>
  );
}

export default function SmartHomePage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();
  
  const [summary, setSummary] = useState<SmartHomeSummary | null>(null);
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
    getSmartHomeState()
      .then((data) => {
        if (!cancelled) setSummary(data);
      })
      .catch((err) => {
        if (!cancelled) setError(clientErrorMessage(err, 'Failed to load smart home state.'));
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
            Smart Home
          </span>
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold sm:text-4xl">Home Control</h1>
            <p className="max-w-3xl text-sm muted sm:text-base">
              Monitor and control connected devices in your home.
            </p>
          </div>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      {loading ? (
        <div className="panel-soft px-5 py-8 text-center muted">Connecting to devices...</div>
      ) : !summary || !summary.available ? (
        <div className="panel-soft px-5 py-8 text-center space-y-4">
           <p className="font-semibold text-lg">Smart Home Unavailable</p>
           <p className="muted max-w-lg mx-auto">
             No smart home provider is currently configured. 
             Set <code>RUSTFIN_SMART_HOME_URL</code> to enable Home Assistant integration.
           </p>
        </div>
      ) : summary.devices.length === 0 ? (
        <div className="panel-soft px-5 py-8 text-center space-y-4">
           <p className="font-semibold text-lg">No Devices Found</p>
           <p className="muted">Connected to {summary.provider || 'provider'}, but no devices were returned.</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {summary.devices.map((device) => (
             <article key={device.id} className="tile p-5 space-y-3">
                <div className="flex items-start justify-between">
                   <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-full bg-black/20 flex items-center justify-center border border-[var(--border-subtle)]">
                         <DeviceIcon type={device.device_type} />
                      </div>
                      <div>
                         <h3 className="font-medium text-base">{device.name}</h3>
                         {device.room && <p className="text-xs text-slate-400">{device.room}</p>}
                      </div>
                   </div>
                   <span className={`chip text-xs ${device.status === 'online' ? 'text-[var(--ok)]' : 'muted'}`}>
                      {device.status}
                   </span>
                </div>
                
                {device.battery_level !== undefined && device.battery_level !== null && (
                   <div className="flex items-center gap-2 pt-2 border-t border-[var(--border-subtle)] mt-2">
                      <div className="h-1.5 flex-1 bg-black/30 rounded-full overflow-hidden">
                         <div 
                           className={`h-full rounded-full ${device.battery_level > 20 ? 'bg-[var(--ok)]' : 'bg-red-500'}`} 
                           style={{ width: `${device.battery_level}%` }} 
                         />
                      </div>
                      <span className="text-xs muted">{device.battery_level}%</span>
                   </div>
                )}
             </article>
          ))}
        </div>
      )}
    </div>
  );
}
