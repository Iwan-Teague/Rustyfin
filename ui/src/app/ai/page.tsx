'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';

export default function AiPage() {
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
        <p className="text-sm muted">Loading AI page...</p>
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
        <h1 className="text-3xl font-semibold sm:text-4xl">AI</h1>
        <p className="text-sm muted sm:text-base">
          This page is ready for future AI assistants and automations.
        </p>
      </header>

      <section className="panel-soft px-5 py-6">
        <p className="text-sm text-slate-200">
          No AI tools are enabled yet. This placeholder reserves space for future media insights,
          smart workflows, and assistant-driven actions.
        </p>
      </section>
    </div>
  );
}
