'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import { getSmartHomeState, SmartDevice, SmartHomeSummary } from '@/lib/smartHomeApi';

function DeviceIcon({ type }: { type: SmartDevice['device_type'] }) {
  if (type === 'camera') {
    return (
      <svg
        className="h-5 w-5 text-slate-300"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      >
        <path
          d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    );
  }
  if (type === 'light') {
    return (
      <svg
        className="h-5 w-5 text-amber-300"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      >
        <path
          d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    );
  }
  if (type === 'door_lock') {
    return (
      <svg
        className="h-5 w-5 text-emerald-400"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      >
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
        <path d="M7 11V7a5 5 0 0110 0v4" />
      </svg>
    );
  }
  if (type === 'alarm') {
    return (
      <svg
        className="h-5 w-5 text-red-400"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      >
        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
      </svg>
    );
  }
  return (
    <svg
      className="h-5 w-5 text-slate-400"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
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
          <h1 className="text-2xl font-semibold sm:text-3xl">Home Control</h1>
          <p className="max-w-3xl text-sm muted">
            Monitor and control connected devices in your home.
          </p>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      {loading ? (
        <div className="rf-flat-empty text-center muted">Connecting to devices...</div>
      ) : !summary || !summary.available ? (
        <div className="rf-flat-empty text-center">
          <p className="text-lg font-semibold">Smart Home Unavailable</p>
          <p className="mx-auto max-w-lg muted">
            No smart home provider is currently configured. Set{' '}
            <code>RUSTFIN_SMART_HOME_URL</code> to enable Home Assistant integration.
          </p>
        </div>
      ) : summary.devices.length === 0 ? (
        <div className="rf-flat-empty text-center">
          <p className="text-lg font-semibold">No Devices Found</p>
          <p className="muted">
            Connected to {summary.provider || 'provider'}, but no devices were returned.
          </p>
        </div>
      ) : (
        <section className="rf-flat-section">
          <div className="rf-flat-list">
            {summary.devices.map((device) => (
              <article key={device.id} className="rf-flat-row space-y-3">
                <div className="flex items-start justify-between gap-4">
                  <div className="flex min-w-0 items-center gap-3">
                    <div className="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-black/20">
                      <DeviceIcon type={device.device_type} />
                    </div>
                    <div className="min-w-0">
                      <h3 className="truncate text-base font-medium">{device.name}</h3>
                      {device.room && <p className="text-xs text-slate-400">{device.room}</p>}
                    </div>
                  </div>
                  <span
                    className={`chip text-xs ${device.status === 'online' ? 'text-[var(--ok)]' : 'muted'}`}
                  >
                    {device.status}
                  </span>
                </div>

                {device.battery_level !== undefined && device.battery_level !== null && (
                  <div className="mt-2 flex items-center gap-2 border-t border-[var(--border-subtle)] pt-3">
                    <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-black/30">
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
        </section>
      )}
    </div>
  );
}
