'use client';

import { useAuth } from '@/lib/auth';

export default function NavBar() {
  const { me, loading, logout } = useAuth();

  return (
    <nav className="app-nav animate-rise rounded-2xl px-4 py-3 sm:px-6">
      <div className="flex items-center gap-3 sm:gap-5">
        <a href="/" className="text-2xl font-semibold accent-logo">Rustyfin</a>
        <span className="chip chip-accent hidden md:inline-flex">Home Server Streaming</span>

        <a href="/libraries" className="btn-ghost px-3 py-2 text-sm sm:text-base">Libraries</a>

        {!loading && me?.role === 'admin' && (
          <a href="/admin" className="btn-ghost px-3 py-2 text-sm sm:text-base">Admin</a>
        )}

        <div className="ml-auto flex items-center gap-2">
          {loading ? (
            <span className="text-sm muted">&hellip;</span>
          ) : me ? (
            <>
              <span className="chip">{me.username}</span>
              <button onClick={logout} className="btn-secondary px-4 py-2 text-sm">
                Logout
              </button>
            </>
          ) : (
            <a href="/login" className="btn-secondary px-4 py-2 text-sm">Login</a>
          )}
        </div>
      </div>
    </nav>
  );
}
