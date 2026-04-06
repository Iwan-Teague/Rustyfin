'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';

export default function DictionaryPage() {
  const router = useRouter();
  const { me, loading } = useAuth();

  useEffect(() => {
    if (!loading && !me) {
      router.replace('/login');
    }
  }, [loading, me, router]);

  if (loading) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading dictionary...</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Redirecting to login...</p>
      </div>
    );
  }

  return (
    <div className="animate-rise rf-flat-page">
      <section className="rf-flat-section space-y-5">
        <header className="space-y-2">
          <p className="text-xs uppercase tracking-[0.18em] text-white/55">Personal</p>
          <h1 className="text-2xl font-semibold sm:text-3xl">Dictionary</h1>
          <p className="max-w-3xl text-sm muted">
            A dedicated space for quick definitions, saved terms, and future Rustyfin glossary tools.
          </p>
        </header>

        <div className="space-y-3 text-sm text-white/88">
          <p>Dictionary is available in the sidebar now and ready for the next set of word and glossary features.</p>
          <p className="muted">This page is intentionally minimal for now so the navigation entry has a real home.</p>
        </div>
      </section>
    </div>
  );
}
