'use client';

import { useEffect, useState } from 'react';
import { getPublicSystemInfo } from '@/lib/setupApi';

export default function Home() {
  const quickActions = [
    {
      title: 'Libraries',
      href: '/libraries',
      description: 'Browse your media collections and open items quickly.',
      badge: 'Catalog',
    },
    {
      title: 'Admin',
      href: '/admin',
      description: 'Create libraries, run scans, and monitor processing jobs.',
      badge: 'Manage',
    },
    {
      title: 'Login',
      href: '/login',
      description: 'Authenticate to stream, sync progress, and keep sessions secure.',
      badge: 'Access',
    },
  ];

  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getPublicSystemInfo()
      .then((info) => {
        if (!info.setup_completed) {
          window.location.href = '/setup';
        } else {
          setLoading(false);
        }
      })
      .catch(() => {
        setLoading(false);
      });
  }, []);

  if (loading) {
    return (
      <div className="panel-soft flex min-h-[45vh] items-center justify-center animate-rise">
        <div className="text-sm muted">Loading...</div>
      </div>
    );
  }

  return (
    <div className="space-y-8 animate-rise">
      <section className="panel p-6 sm:p-8 lg:p-10">
        <div className="flex flex-col gap-8 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-2xl space-y-4">
            <span className="chip chip-accent">High Fidelity Streaming</span>
            <h1 className="text-4xl font-semibold leading-tight sm:text-5xl">
              Your media server,
              <br />
              refined for every screen.
            </h1>
            <p className="max-w-xl text-base leading-relaxed muted sm:text-lg">
              Rustyfin gives home servers a cleaner control center: organized libraries, fast playback,
              and modern visuals tuned for long sessions.
            </p>
            <div className="flex flex-wrap items-center gap-3 pt-2">
              <a href="/libraries" className="btn-primary px-5 py-2.5 text-sm sm:text-base">
                Browse Libraries
              </a>
              <a href="/admin" className="btn-secondary px-5 py-2.5 text-sm sm:text-base">
                Open Admin
              </a>
            </div>
          </div>
          <div className="panel-soft max-w-sm space-y-3 px-5 py-4">
            <p className="text-xs uppercase tracking-[0.22em] muted">Playback Modes</p>
            <div className="space-y-2 text-sm">
              <p className="flex items-center justify-between">
                <span className="muted">Direct Play</span>
                <span className="font-semibold text-[var(--orange-soft)]">Fast Path</span>
              </p>
              <p className="flex items-center justify-between">
                <span className="muted">HLS Transcode</span>
                <span className="font-semibold text-[var(--purple)]">Adaptive</span>
              </p>
              <p className="flex items-center justify-between">
                <span className="muted">Live Progress</span>
                <span className="font-semibold">Synced</span>
              </p>
            </div>
          </div>
        </div>
      </section>

      <section className="space-y-4">
        <h2 className="text-xl font-semibold sm:text-2xl">Quick Actions</h2>
        <div className="grid grid-cols-1 gap-5 md:grid-cols-3">
          {quickActions.map((action) => (
            <a key={action.title} href={action.href} className="tile tile-hover block p-5">
              <span className="chip">{action.badge}</span>
              <h3 className="mt-4 text-xl font-semibold">{action.title}</h3>
              <p className="mt-2 text-sm leading-relaxed muted">{action.description}</p>
            </a>
          ))}
        </div>
      </section>
    </div>
  );
}
