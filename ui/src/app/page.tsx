'use client';

import { useEffect, useState } from 'react';
import { getPublicSystemInfo } from '@/lib/setupApi';

export default function Home() {
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
      <div className="flex items-center justify-center min-h-[50vh]">
        <div className="text-gray-400">Loading...</div>
      </div>
    );
  }

  return (
    <div className="space-y-8">
      <h1 className="text-3xl font-bold">Welcome to Rustfin</h1>
      <p className="text-gray-400">Your local-first media server.</p>
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        <a
          href="/libraries"
          className="block p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-blue-500 transition"
        >
          <h2 className="text-xl font-semibold mb-2">Libraries</h2>
          <p className="text-gray-400 text-sm">Browse your media libraries</p>
        </a>
        <a
          href="/admin"
          className="block p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-blue-500 transition"
        >
          <h2 className="text-xl font-semibold mb-2">Admin</h2>
          <p className="text-gray-400 text-sm">Manage users, libraries, and settings</p>
        </a>
        <a
          href="/login"
          className="block p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-blue-500 transition"
        >
          <h2 className="text-xl font-semibold mb-2">Login</h2>
          <p className="text-gray-400 text-sm">Sign in to your account</p>
        </a>
      </div>
    </div>
  );
}
