'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';

export default function NetworkPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  if (authLoading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading network page...</p>
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
      <header className="panel space-y-3 p-6 sm:p-8">
        <h1 className="text-3xl font-semibold sm:text-4xl">Network</h1>
        <p className="text-sm muted sm:text-base">
          This page is ready for future network-related features.
        </p>
      </header>

      <section className="panel-soft px-5 py-6">
        <p className="text-sm muted">Nothing is here yet.</p>
      </section>
    </div>
  );
}
