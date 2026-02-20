'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';

export default function LoginPage() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const router = useRouter();
  const { refreshMe } = useAuth();

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');

    try {
      localStorage.removeItem('token');
      const res = await fetch('/api/v1/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });

      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setError(body?.error?.message || 'Login failed');
        return;
      }

      const data = await res.json();
      localStorage.setItem('token', data.token);
      await refreshMe();
      router.push('/libraries');
    } catch {
      setError('Network error');
    }
  }

  return (
    <div className="mx-auto mt-8 max-w-md animate-rise sm:mt-14">
      <div className="panel space-y-6 px-6 py-7 sm:px-8">
        <div className="space-y-2">
          <span className="chip chip-accent">Secure Access</span>
          <h1 className="text-3xl font-semibold">Welcome back</h1>
          <p className="text-sm muted">Sign in to stream media and manage your server.</p>
        </div>

        {error && <p className="notice-error rounded-xl px-4 py-2 text-sm">{error}</p>}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="mb-1.5 block text-sm font-medium muted">Username</label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="input px-4 py-2.5"
              required
            />
          </div>
          <div>
            <label className="mb-1.5 block text-sm font-medium muted">Password</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="input px-4 py-2.5"
              required
            />
          </div>
          <button type="submit" className="btn-primary w-full py-2.5 text-sm">
            Sign In
          </button>
        </form>

        <p className="text-xs muted">
          Tip: if this is your first run, default credentials are <span className="font-semibold">admin/admin</span>.
        </p>
      </div>
    </div>
  );
}
