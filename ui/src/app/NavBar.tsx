'use client';

import { useState } from 'react';
import Link from 'next/link';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { usePathname } from 'next/navigation';

export default function NavBar() {
  const { me, loading, logout } = useAuth();
  const { voiceSession } = useChannels();
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);

  if (pathname.startsWith('/setup')) {
    return null;
  }

  const navLinks = [
    { href: '/libraries', label: 'Libraries' },
    { href: '/channels', label: 'Channels' },
    { href: '/rooms', label: 'Rooms' },
    ...(!loading && me?.role === 'admin' ? [{ href: '/admin', label: 'Admin' }] : []),
  ];

  return (
    <nav className="app-nav animate-rise rounded-2xl px-4 py-3 md:px-6">

      {/* ── Desktop bar (lg+): nav links left | logo centered | user right ── */}
      <div className="relative hidden items-center lg:flex">

        {/* Left: nav links */}
        <div className="flex items-center gap-1">
          {navLinks.map((link) => (
            <Link key={link.href} href={link.href} className="btn-ghost px-3 py-2 text-sm">
              {link.label}
            </Link>
          ))}
        </div>

        {/* Center: logo (absolute so it doesn't affect flex layout) */}
        <Link
          href="/"
          className="absolute left-1/2 -translate-x-1/2 text-2xl font-semibold accent-logo"
        >
          Rustyfin
        </Link>

        {/* Right: voice indicator + user section */}
        <div className="ml-auto flex items-center gap-2">
          {voiceSession && (
            <Link
              href="/channels"
              className="chip border-green-500/50 text-green-400"
            >
              <span className="h-1.5 w-1.5 rounded-full bg-green-400 animate-pulse" />
              {voiceSession.channelName}
            </Link>
          )}
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
            <Link href="/login" className="btn-secondary px-4 py-2 text-sm">
              Login
            </Link>
          )}
        </div>
      </div>

      {/* ── Mobile top bar (below lg): hamburger | logo centered | username ── */}
      <div className="flex items-center lg:hidden">
        <button
          type="button"
          className="btn-ghost flex items-center px-3 py-2 text-xl leading-none"
          onClick={() => setMenuOpen((prev) => !prev)}
          aria-label="Toggle menu"
          aria-expanded={menuOpen}
        >
          ☰
        </button>

        <Link href="/" className="mx-auto text-2xl font-semibold accent-logo">
          Rustyfin
        </Link>

        {/* Voice indicator pill on mobile */}
        {voiceSession && (
          <Link
            href="/channels"
            className="chip border-green-500/50 text-green-400 mr-2"
          >
            <span className="h-1.5 w-1.5 rounded-full bg-green-400 animate-pulse" />
            🔊
          </Link>
        )}

        {!loading && me && <span className="chip">{me.username}</span>}
      </div>

      {/* ── Mobile dropdown ── */}
      {menuOpen && (
        <div className="mt-2 flex flex-col gap-0.5 border-t border-[var(--border)] pt-2 lg:hidden">
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className="btn-ghost rounded-xl px-3 py-3 text-base"
              onClick={() => setMenuOpen(false)}
            >
              {link.label}
            </Link>
          ))}
          <div className="mt-1 border-t border-[var(--border)] pt-2">
            {loading ? (
              <span className="px-3 py-2 text-sm muted">&hellip;</span>
            ) : me ? (
              <div className="flex items-center justify-end gap-2 px-1">
                <button
                  onClick={() => { logout(); setMenuOpen(false); }}
                  className="btn-secondary px-4 py-2 text-sm"
                >
                  Logout
                </button>
              </div>
            ) : (
              <Link
                href="/login"
                className="btn-secondary block px-4 py-2.5 text-center text-sm"
                onClick={() => setMenuOpen(false)}
              >
                Login
              </Link>
            )}
          </div>
        </div>
      )}
    </nav>
  );
}
