'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { usePathname } from 'next/navigation';
import { createPortal } from 'react-dom';

export default function NavBar() {
  const { me, loading, logout } = useAuth();
  const { voiceSession, toggleMute, toggleDeafen, leaveVoice } = useChannels();
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmLeaveVoiceOpen, setConfirmLeaveVoiceOpen] = useState(false);
  const [portalMounted, setPortalMounted] = useState(false);
  const [showDesktopNav, setShowDesktopNav] = useState(false);
  const desktopShellRef = useRef<HTMLDivElement | null>(null);
  const desktopLeftMeasureRef = useRef<HTMLDivElement | null>(null);
  const desktopRightMeasureRef = useRef<HTMLDivElement | null>(null);
  const desktopLogoMeasureRef = useRef<HTMLAnchorElement | null>(null);

  useEffect(() => {
    setPortalMounted(true);
    return () => setPortalMounted(false);
  }, []);

  useEffect(() => {
    if (!voiceSession && confirmLeaveVoiceOpen) {
      setConfirmLeaveVoiceOpen(false);
    }
  }, [voiceSession, confirmLeaveVoiceOpen]);

  useEffect(() => {
    if (showDesktopNav && menuOpen) {
      setMenuOpen(false);
    }
  }, [menuOpen, showDesktopNav]);

  if (pathname.startsWith('/setup')) {
    return null;
  }

  const navLinks = [
    { href: '/channels', label: 'Channels' },
    { href: '/rooms', label: 'Rooms' },
    { href: '/servers', label: 'Servers' },
    { href: '/calendar', label: 'Calendar' },
    { href: '/libraries', label: 'Libraries' },
    ...(!loading && me?.role === 'admin' ? [{ href: '/admin', label: 'Admin' }] : []),
  ];
  const desktopLeftNavLinks = navLinks.filter((link) =>
    ['/channels', '/rooms', '/servers', '/calendar', '/libraries'].includes(link.href),
  );
  const desktopRightNavLinks = navLinks.filter((link) =>
    ['/admin'].includes(link.href),
  );
  const hasLocalStream = voiceSession?.localStream !== null;
  const muted = voiceSession?.muted ?? false;
  const deafened = voiceSession?.deafened ?? false;
  const baseVoiceActionClass =
    'inline-flex items-center justify-center rounded-full border transition';

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const mediaQuery = window.matchMedia('(min-width: 1024px)');
    const updateLayoutMode = () => {
      if (!mediaQuery.matches) {
        setShowDesktopNav(false);
        return;
      }

      const shellWidth = desktopShellRef.current?.getBoundingClientRect().width ?? 0;
      const logoWidth = desktopLogoMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const leftWidth = desktopLeftMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const rightWidth = desktopRightMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const sideAllowance = (shellWidth - (logoWidth + 96)) / 2;

      setShowDesktopNav(sideAllowance > 0 && leftWidth <= sideAllowance && rightWidth <= sideAllowance);
    };

    updateLayoutMode();

    const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(updateLayoutMode) : null;
    if (observer) {
      if (desktopShellRef.current) observer.observe(desktopShellRef.current);
      if (desktopLogoMeasureRef.current) observer.observe(desktopLogoMeasureRef.current);
      if (desktopLeftMeasureRef.current) observer.observe(desktopLeftMeasureRef.current);
      if (desktopRightMeasureRef.current) observer.observe(desktopRightMeasureRef.current);
    }

    const handleMediaChange = () => updateLayoutMode();
    if (typeof mediaQuery.addEventListener === 'function') {
      mediaQuery.addEventListener('change', handleMediaChange);
    } else {
      mediaQuery.addListener(handleMediaChange);
    }

    window.addEventListener('resize', updateLayoutMode);

    return () => {
      observer?.disconnect();
      if (typeof mediaQuery.removeEventListener === 'function') {
        mediaQuery.removeEventListener('change', handleMediaChange);
      } else {
        mediaQuery.removeListener(handleMediaChange);
      }
      window.removeEventListener('resize', updateLayoutMode);
    };
  }, [loading, me?.role, me?.username, voiceSession?.channelName, hasLocalStream, muted, deafened]);

  if (!loading && !me) {
    return (
      <nav className="app-nav animate-rise rounded-2xl px-4 py-3 md:px-6">
        <div className="relative flex min-h-10 items-center justify-center">
          <Link
            href="/"
            className="rounded-full px-3 text-center text-2xl font-semibold accent-logo transition hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--orange-soft)]/70"
            aria-label="Go to Rustyfin home"
          >
            Rustyfin
          </Link>
        </div>
      </nav>
    );
  }

  return (
    <nav className="app-nav animate-rise rounded-2xl px-4 py-3 md:px-6">
      <div className="relative hidden lg:block" aria-hidden="true">
        <div ref={desktopShellRef} className="pointer-events-none absolute inset-x-0 top-0 -z-10 opacity-0">
          <div className="flex items-center justify-between gap-4">
            <div ref={desktopLeftMeasureRef} className="flex items-center justify-start gap-2">
              {desktopLeftNavLinks.map((link) => (
                <span key={link.href} className="btn-ghost shrink-0 px-3 py-2 text-sm">
                  {link.label}
                </span>
              ))}
            </div>
            <Link
              ref={desktopLogoMeasureRef}
              href="/"
              className="rounded-full px-3 text-center text-2xl font-semibold accent-logo"
              aria-label="Go to Rustyfin home"
            >
              Rustyfin
            </Link>
            <div ref={desktopRightMeasureRef} className="flex items-center justify-end gap-2">
              {voiceSession && (
                <div className="chip h-10 shrink-0 gap-2 border-green-500/50 px-2 py-1.5 text-green-300">
                  <span className="inline-flex min-w-0 items-center gap-2 rounded-full px-1 text-green-300">
                    <span className="h-2 w-2 shrink-0 rounded-full bg-green-400" />
                    <span className="max-w-[12rem] truncate text-xs font-medium">
                      {voiceSession.channelName}
                    </span>
                  </span>
                </div>
              )}
              {loading ? (
                <span className="text-sm muted">&hellip;</span>
              ) : me ? (
                <>
                  <span className="chip h-10 shrink-0 px-4 text-sm">{me.username}</span>
                  {desktopRightNavLinks.map((link) => (
                    <span key={link.href} className="btn-ghost shrink-0 px-3 py-2 text-sm">
                      {link.label}
                    </span>
                  ))}
                  <span className="btn-secondary h-10 shrink-0 px-4 py-2 text-sm">Logout</span>
                </>
              ) : (
                <span className="btn-secondary h-10 shrink-0 px-4 py-2 text-sm">Login</span>
              )}
            </div>
          </div>
        </div>
      </div>

      {showDesktopNav ? (
        <div className="relative hidden lg:block">
          <div className="pointer-events-none absolute inset-x-0 top-1/2 flex -translate-y-1/2 justify-center">
            <Link
              href="/"
              className="pointer-events-auto rounded-full px-3 text-center text-2xl font-semibold accent-logo transition hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--orange-soft)]/70"
              aria-label="Go to Rustyfin home"
            >
              Rustyfin
            </Link>
          </div>

          <div className="flex items-center justify-between gap-4">
            <div className="flex min-w-0 items-center justify-start gap-2">
              {desktopLeftNavLinks.map((link) => (
                <Link key={link.href} href={link.href} className="btn-ghost shrink-0 px-3 py-2 text-sm">
                  {link.label}
                </Link>
              ))}
            </div>

            <div className="flex min-w-0 items-center justify-end gap-2">
              {voiceSession && (
                <div className="chip h-10 shrink-0 border-green-500/50 text-green-300 gap-2 px-2 py-1.5">
                  <Link
                    href="/channels"
                    className="inline-flex min-w-0 items-center gap-2 rounded-full px-1 text-green-300 hover:text-green-200"
                    title={`Open channel: ${voiceSession.channelName}`}
                  >
                    <span className="h-2 w-2 rounded-full bg-green-400 animate-pulse shrink-0" />
                    <span className="max-w-[12rem] truncate text-xs font-medium">
                      {voiceSession.channelName}
                    </span>
                  </Link>
                  <div className="h-4 w-px bg-green-400/35" />
                  <button
                    type="button"
                    onClick={toggleMute}
                    disabled={!hasLocalStream}
                    className={`${baseVoiceActionClass} h-7 w-7 disabled:opacity-40 disabled:cursor-not-allowed ${
                      muted
                        ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                        : 'border-[var(--border)] bg-black/45 text-white/85 hover:text-white'
                    }`}
                    aria-label={muted ? 'Unmute microphone' : 'Mute microphone'}
                    title={
                      hasLocalStream
                        ? muted
                          ? 'Unmute microphone'
                          : 'Mute microphone'
                        : 'No microphone — listening only'
                    }
                  >
                    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                      <rect
                        x="9"
                        y="3.5"
                        width="6"
                        height="10"
                        rx="3"
                        stroke="currentColor"
                        strokeWidth="1.8"
                      />
                      <path d="M6.5 11.5a5.5 5.5 0 0 0 11 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      <path d="M12 17v3.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      <path d="M8.5 20.5h7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      {muted && (
                        <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      )}
                    </svg>
                  </button>
                  <button
                    type="button"
                    onClick={toggleDeafen}
                    className={`${baseVoiceActionClass} h-7 w-7 ${
                      deafened
                        ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                        : 'border-[var(--border)] bg-black/45 text-white/85 hover:text-white'
                    }`}
                    aria-label={deafened ? 'Undeafen' : 'Deafen'}
                    title={deafened ? 'Undeafen' : 'Deafen'}
                  >
                    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                      <path d="M4 12a8 8 0 0 1 16 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      <rect x="2.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                      <rect x="17.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                      <path d="M17.5 18.5a4.5 4.5 0 0 1-4.5 4.5h-1" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      {deafened && (
                        <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                      )}
                    </svg>
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmLeaveVoiceOpen(true)}
                    className={`${baseVoiceActionClass} h-7 w-7 border-[var(--border)] bg-black/55 text-white/90 hover:text-white`}
                    aria-label="Disconnect from voice"
                    title="Disconnect from voice"
                  >
                    <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                      <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                    </svg>
                  </button>
                </div>
              )}
              {loading ? (
                <span className="text-sm muted">&hellip;</span>
              ) : me ? (
                <>
                  <span className="chip h-10 shrink-0 px-4 text-sm">{me.username}</span>
                  {desktopRightNavLinks.map((link) => (
                    <Link key={link.href} href={link.href} className="btn-ghost shrink-0 px-3 py-2 text-sm">
                      {link.label}
                    </Link>
                  ))}
                  <button onClick={logout} className="btn-secondary h-10 shrink-0 px-4 text-sm">
                    Logout
                  </button>
                </>
              ) : (
                <Link href="/login" className="btn-secondary h-10 shrink-0 px-4 text-sm">
                  Login
                </Link>
              )}
            </div>
          </div>
        </div>
      ) : null}

      {!showDesktopNav ? (
        <div className="relative flex items-center justify-between">
          <div className="pointer-events-none absolute inset-x-0 top-1/2 flex -translate-y-1/2 justify-center">
            <Link
              href="/"
              className="pointer-events-auto rounded-full px-3 text-center text-2xl font-semibold accent-logo transition hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--orange-soft)]/70"
              aria-label="Go to Rustyfin home"
            >
              Rustyfin
            </Link>
          </div>

          <button
            type="button"
            className="btn-ghost flex items-center px-3 py-2 text-xl leading-none"
            onClick={() => setMenuOpen((prev) => !prev)}
            aria-label="Toggle menu"
            aria-expanded={menuOpen}
          >
            ☰
          </button>

          <div className="flex items-center justify-end gap-2">
            {voiceSession ? (
              <div className="mr-2 flex items-center gap-1 rounded-full border border-green-500/50 bg-black/35 px-1.5 py-1">
                <Link
                  href="/channels"
                  className="inline-flex min-w-0 items-center gap-1.5 rounded-full px-1 text-[11px] text-green-300"
                  title={`Open channel: ${voiceSession.channelName}`}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-green-400 animate-pulse shrink-0" />
                  <span className="max-w-[5.5rem] truncate">{voiceSession.channelName}</span>
                </Link>
                <button
                  type="button"
                  onClick={toggleMute}
                  disabled={!hasLocalStream}
                  className={`${baseVoiceActionClass} h-6 w-6 disabled:opacity-40 disabled:cursor-not-allowed ${
                    muted
                      ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                      : 'border-[var(--border)] bg-black/45 text-white/85'
                  }`}
                  aria-label={muted ? 'Unmute microphone' : 'Mute microphone'}
                  title={
                    hasLocalStream
                      ? muted
                        ? 'Unmute microphone'
                        : 'Mute microphone'
                      : 'No microphone — listening only'
                  }
                >
                  <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" aria-hidden="true">
                    <rect x="9" y="3.5" width="6" height="10" rx="3" stroke="currentColor" strokeWidth="1.8" />
                    <path d="M6.5 11.5a5.5 5.5 0 0 0 11 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <path d="M12 17v3.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <path d="M8.5 20.5h7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    {muted && (
                      <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    )}
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={toggleDeafen}
                  className={`${baseVoiceActionClass} h-6 w-6 ${
                    deafened
                      ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                      : 'border-[var(--border)] bg-black/45 text-white/85'
                  }`}
                  aria-label={deafened ? 'Undeafen' : 'Deafen'}
                  title={deafened ? 'Undeafen' : 'Deafen'}
                >
                  <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" aria-hidden="true">
                    <path d="M4 12a8 8 0 0 1 16 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <rect x="2.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                    <rect x="17.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                    <path d="M17.5 18.5a4.5 4.5 0 0 1-4.5 4.5h-1" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    {deafened && (
                      <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    )}
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmLeaveVoiceOpen(true)}
                  className={`${baseVoiceActionClass} h-6 w-6 border-[var(--border)] bg-black/55 text-white/90`}
                  aria-label="Disconnect from voice"
                  title="Disconnect from voice"
                >
                  <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" aria-hidden="true">
                    <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                  </svg>
                </button>
              </div>
            ) : !loading && me ? (
              <span className="chip">{me.username}</span>
            ) : (
              <span className="w-10 shrink-0" aria-hidden="true" />
            )}
          </div>
        </div>
      ) : null}

      {!showDesktopNav && menuOpen ? (
        <div className="mt-2 flex flex-col gap-0.5 border-t border-[var(--border)] pt-2">
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
                  onClick={() => {
                    logout();
                    setMenuOpen(false);
                  }}
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
      ) : null}

      {voiceSession &&
        confirmLeaveVoiceOpen &&
        portalMounted &&
        createPortal(
          <div className="fixed inset-0 z-[80] flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]">
            <div className="panel w-full max-w-sm space-y-4 rounded-2xl border border-[var(--border)] p-6">
              <h2 className="text-lg font-semibold">Leave Voice Channel?</h2>
              <p className="text-sm muted">
                Leave <span className="font-medium text-[var(--text-main)]">{voiceSession.channelName}</span> and disconnect from the audio call?
              </p>
              <div className="flex justify-end gap-2">
                <button
                  type="button"
                  onClick={() => setConfirmLeaveVoiceOpen(false)}
                  className="btn-ghost px-4 py-2 text-sm"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setConfirmLeaveVoiceOpen(false);
                    leaveVoice();
                  }}
                  className="btn-primary bg-red-500 px-4 py-2 text-sm hover:bg-red-600"
                >
                  Leave
                </button>
              </div>
            </div>
          </div>,
          document.body,
        )}
    </nav>
  );
}
