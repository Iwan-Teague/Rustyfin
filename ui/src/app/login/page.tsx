'use client';

import { useState } from 'react';
import { useAuth } from '@/lib/auth';
import { extractErrorMessage, parseResponseBody } from '@/lib/api';
import { toClientError } from '@/lib/errors';
import { clearBrowserToken, writeBrowserToken } from '@/lib/browserAuth';

export default function LoginPage() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const { replaceMe } = useAuth();

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError('');

    try {
      clearBrowserToken();
      const res = await fetch('/api/v1/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      });
      const body = await parseResponseBody(res);

      if (!res.ok) {
        setError(extractErrorMessage(body, 'Login failed'));
        return;
      }

      if (!body || typeof body !== 'object' || typeof (body as { token?: unknown }).token !== 'string') {
        setError('Login failed');
        return;
      }

      const payload = body as {
        token: string;
        user_id?: unknown;
        username?: unknown;
        role?: unknown;
      };

      writeBrowserToken(payload.token);

      if (
        typeof payload.user_id === 'string' &&
        typeof payload.username === 'string' &&
        (payload.role === 'admin' || payload.role === 'user')
      ) {
        replaceMe({
          id: payload.user_id,
          username: payload.username,
          login_username: payload.username,
          role: payload.role,
        });
      }

      window.location.assign('/');
    } catch (err: unknown) {
      setError(toClientError(err).message || 'Network error');
    }
  }

  return (
    <div className="mx-auto mt-8 max-w-md animate-rise sm:mt-14">
      <div className="panel space-y-6 px-6 py-7 sm:px-8">
        <div className="space-y-2">
          <h1 className="text-3xl font-semibold">Welcome back</h1>
          <p className="text-sm muted">Sign in to stream media and manage your server.</p>
        </div>

        {error && (
          <p className="notice-error rounded-xl px-4 py-2 text-sm" role="alert">
            {error}
          </p>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="login-username" className="mb-1.5 block text-sm font-medium muted">Username</label>
            <input
              id="login-username"
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              className="input px-4 py-2.5"
              required
            />
          </div>
          <div>
            <label htmlFor="login-password" className="mb-1.5 block text-sm font-medium muted">Password</label>
            <input
              id="login-password"
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
      </div>
    </div>
  );
}
