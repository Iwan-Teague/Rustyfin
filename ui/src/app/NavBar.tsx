'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import Link from 'next/link';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { usePathname } from 'next/navigation';
import { createPortal } from 'react-dom';
import NotificationsPopover from '@/app/components/NotificationsPopover';

export default function NavBar() {
  const { me, loading } = useAuth();
  const {
    voiceSession,
    connectedVoiceChannelId,
    connectedVoiceChannelName,
    hasLocalVoiceSession,
    toggleMute,
    toggleDeafen,
    leaveVoice,
  } = useChannels();
  const pathname = usePathname();
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmLeaveVoiceOpen, setConfirmLeaveVoiceOpen] = useState(false);
  const [portalMounted, setPortalMounted] = useState(false);
  const [showCompactDesktopNav, setShowCompactDesktopNav] = useState(false);
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
    if (showCompactDesktopNav && menuOpen) {
      setMenuOpen(false);
    }
  }, [menuOpen, showCompactDesktopNav]);

  const navLinks = useMemo(
    () => [
      { href: '/libraries', label: 'Libraries' },
      { href: '/rooms', label: 'Rooms' },
      { href: '/channels', label: 'Channels' },
      { href: '/vault', label: 'Vault' },
      { href: '/servers', label: 'Servers' },
      { href: '/calendar', label: 'Calendar' },
      { href: '/network', label: 'Network' },
      { href: '/downloads', label: 'Downloads' },
      { href: '/ai', label: 'AI' },
      { href: '/backups', label: 'Backups' },
      ...(!loading && me?.role === 'admin' ? [{ href: '/admin', label: 'Admin' }] : []),
    ],
    [loading, me?.role],
  );
  const desktopLeftNavLinks = useMemo(
    () =>
      navLinks.filter((link) =>
        [
          '/libraries',
          '/rooms',
          '/channels',
          '/vault',
          '/servers',
          '/calendar',
          '/network',
          '/downloads',
          '/ai',
          '/backups',
        ].includes(link.href),
      ),
    [navLinks],
  );
  const desktopRightNavLinks = useMemo(
    () => navLinks.filter((link) => ['/admin'].includes(link.href)),
    [navLinks],
  );
  const activeVoiceChannelName = voiceSession?.channelName ?? connectedVoiceChannelName ?? 'Voice';
  const showVoiceWidget = Boolean(connectedVoiceChannelId && activeVoiceChannelName);
  const hasLocalStream = hasLocalVoiceSession && voiceSession?.localStream !== null;
  const muted = hasLocalVoiceSession ? (voiceSession?.muted ?? false) : false;
  const deafened = hasLocalVoiceSession ? (voiceSession?.deafened ?? false) : false;
  const baseVoiceActionClass =
    'inline-flex items-center justify-center rounded-full border transition disabled:cursor-not-allowed disabled:opacity-40';
  const isActivePath = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
  const navLinkClass = (href: string, base: string) =>
    `rf-nav-link ${base} ${
      isActivePath(href)
        ? 'text-[var(--text-main)]'
        : ''
    }`;

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const compactMediaQuery = window.matchMedia('(min-width: 768px)');
    const updateLayoutMode = () => {
      if (!compactMediaQuery.matches) {
        setShowCompactDesktopNav(false);
        return;
      }

      const shellWidth = desktopShellRef.current?.getBoundingClientRect().width ?? 0;
      const logoWidth = desktopLogoMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const leftWidth = desktopLeftMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const rightWidth = desktopRightMeasureRef.current?.getBoundingClientRect().width ?? 0;
      const compactFits =
        compactMediaQuery.matches && shellWidth >= leftWidth + logoWidth + rightWidth + 88;

      setShowCompactDesktopNav(compactFits);
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
    if (typeof compactMediaQuery.addEventListener === 'function') {
      compactMediaQuery.addEventListener('change', handleMediaChange);
    } else {
      compactMediaQuery.addListener(handleMediaChange);
    }

    window.addEventListener('resize', updateLayoutMode);

    return () => {
      observer?.disconnect();
      if (typeof compactMediaQuery.removeEventListener === 'function') {
        compactMediaQuery.removeEventListener('change', handleMediaChange);
      } else {
        compactMediaQuery.removeListener(handleMediaChange);
      }
      window.removeEventListener('resize', updateLayoutMode);
    };
  }, [
    loading,
    me?.role,
    me?.username,
    showVoiceWidget,
    activeVoiceChannelName,
    hasLocalVoiceSession,
    hasLocalStream,
    muted,
    deafened,
  ]);

  if (pathname.startsWith('/setup')) {
    return null;
  }

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
      <div className="relative hidden md:block" aria-hidden="true">
        <div ref={desktopShellRef} className="pointer-events-none absolute inset-x-0 top-0 -z-10 opacity-0">
          <div className="flex items-center justify-between gap-4">
            <div ref={desktopLeftMeasureRef} className="flex items-center justify-start gap-2">
              {desktopLeftNavLinks.map((link) => (
                <span key={link.href} className="btn-ghost h-11 shrink-0 px-3 text-sm">
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
              {showVoiceWidget && (
                <div className="chip h-10 shrink-0 gap-2 border-green-500/50 px-2 py-1.5 text-green-300">
                  <span className="inline-flex min-w-0 items-center gap-2 rounded-full px-1 text-green-300">
                    <span className="h-2 w-2 shrink-0 rounded-full bg-green-400" />
                    <span className="max-w-[12rem] truncate text-xs font-medium">
                      {activeVoiceChannelName}
                    </span>
                  </span>
                </div>
              )}
              {loading ? (
                <span className="text-sm muted">&hellip;</span>
              ) : me ? (
                <>
                  <span className="btn-ghost h-11 w-11 shrink-0 p-0" />
                  <Link
                    href="/account"
                    className={navLinkClass('/account', 'btn-ghost h-11 shrink-0 px-4 text-sm')}
                  >
                    {me.username}
                  </Link>
                  {desktopRightNavLinks.map((link) => (
                    <span key={link.href} className="btn-ghost h-11 shrink-0 px-3 text-sm">
                      {link.label}
                    </span>
                  ))}
                </>
              ) : (
                <span className="btn-secondary h-11 shrink-0 px-4 text-sm">Login</span>
              )}
            </div>
          </div>
        </div>
      </div>

      {showCompactDesktopNav ? (
        <div className="hidden md:block">
          <div className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-2">
              <Link
                href="/"
                className="shrink-0 rounded-full px-3 text-center text-2xl font-semibold accent-logo transition hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--orange-soft)]/70"
                aria-label="Go to Rustyfin home"
              >
                Rustyfin
              </Link>
              <div className="flex min-w-0 items-center gap-2">
                {desktopLeftNavLinks.map((link) => (
                  <Link
                    key={`compact-${link.href}`}
                    href={link.href}
                    className={navLinkClass(link.href, 'btn-ghost h-11 shrink-0 px-3 text-sm')}
                    aria-current={isActivePath(link.href) ? 'page' : undefined}
                  >
                    {link.label}
                  </Link>
                ))}
              </div>
            </div>

            <div className="flex min-w-0 items-center justify-end gap-2">
              {showVoiceWidget && (
                <div className="chip h-10 shrink-0 border-green-500/50 text-green-300 gap-2 px-2 py-1.5">
                  <Link
                    href="/channels"
                    className="inline-flex min-w-0 items-center gap-2 rounded-full px-1 text-green-300 hover:text-green-200"
                    title={`Open channel: ${activeVoiceChannelName}`}
                  >
                    <span className="h-2 w-2 rounded-full bg-green-400 animate-pulse shrink-0" />
                    <span className="max-w-[12rem] truncate text-xs font-medium">
                      {activeVoiceChannelName}
                    </span>
                  </Link>
                  {hasLocalVoiceSession ? (
                    <>
                      <div className="h-4 w-px bg-green-400/35" />
                      <button
                        type="button"
                        onClick={toggleMute}
                        disabled={!hasLocalStream}
                        className={`${baseVoiceActionClass} h-9 w-9 ${
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
                        className={`${baseVoiceActionClass} h-9 w-9 ${
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
                        className={`${baseVoiceActionClass} h-9 w-9 border-[var(--border)] bg-black/55 text-white/90 hover:text-white`}
                        aria-label="Disconnect from voice"
                        title="Disconnect from voice"
                      >
                        <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                          <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                        </svg>
                      </button>
                    </>
                  ) : (
                    <>
                      <div className="h-4 w-px bg-green-400/35" />
                      <span className="px-1 text-[11px] text-green-200/85">In another tab</span>
                    </>
                  )}
                </div>
              )}
              {loading ? (
                <span className="text-sm muted">&hellip;</span>
              ) : me ? (
                <>
                  <NotificationsPopover isAdmin={me.role === 'admin'} />
                  <Link
                    href="/account"
                    className={navLinkClass('/account', 'btn-ghost h-11 shrink-0 px-4 text-sm')}
                    aria-current={isActivePath('/account') ? 'page' : undefined}
                  >
                    {me.username}
                  </Link>
                  {desktopRightNavLinks.map((link) => (
                    <Link
                      key={`compact-right-${link.href}`}
                      href={link.href}
                      className={navLinkClass(link.href, 'btn-ghost h-11 shrink-0 px-3 text-sm')}
                      aria-current={isActivePath(link.href) ? 'page' : undefined}
                    >
                      {link.label}
                    </Link>
                  ))}
                </>
              ) : (
                <Link href="/login" className="btn-secondary h-11 shrink-0 px-4 text-sm">
                  Login
                </Link>
              )}
            </div>
          </div>
        </div>
      ) : null}

      {!showCompactDesktopNav ? (
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
            className="btn-ghost h-11 w-11 p-0"
            onClick={() => setMenuOpen((prev) => !prev)}
            aria-label="Toggle menu"
            aria-expanded={menuOpen}
            aria-controls="mobile-nav-menu"
          >
            <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
              <path d="M4 7h16M4 12h16M4 17h16" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
            </svg>
          </button>

          <div className="flex min-w-0 items-center justify-end gap-1.5">
            {!loading && me ? (
              <>
                <NotificationsPopover isAdmin={me.role === 'admin'} />
                <Link
                  href="/account"
                  className={navLinkClass('/account', 'btn-ghost h-11 max-w-[9.5rem] shrink-0 px-3 text-sm')}
                  aria-current={isActivePath('/account') ? 'page' : undefined}
                >
                  <span className="truncate">{me.username}</span>
                </Link>
              </>
            ) : null}

            {showVoiceWidget ? (
              <div className="flex max-w-[calc(100vw-11rem)] items-center gap-1 rounded-full border border-green-500/50 bg-black/35 px-1.5 py-1">
                <Link
                  href="/channels"
                  className="inline-flex min-w-0 items-center gap-1.5 rounded-full px-1 text-[11px] text-green-300"
                  title={`Open channel: ${activeVoiceChannelName}`}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-green-400 animate-pulse shrink-0" />
                  <span className="max-w-[3.75rem] truncate">{activeVoiceChannelName}</span>
                </Link>
                {hasLocalVoiceSession ? (
                  <>
                    <button
                      type="button"
                      onClick={toggleMute}
                      disabled={!hasLocalStream}
                      className={`${baseVoiceActionClass} h-8 w-8 ${
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
                      className={`${baseVoiceActionClass} h-8 w-8 ${
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
                      className={`${baseVoiceActionClass} h-8 w-8 border-[var(--border)] bg-black/55 text-white/90`}
                      aria-label="Disconnect from voice"
                      title="Disconnect from voice"
                    >
                      <svg viewBox="0 0 24 24" className="h-3 w-3" fill="none" aria-hidden="true">
                        <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                      </svg>
                    </button>
                  </>
                ) : (
                  <span className="px-1 text-[11px] text-green-200/85">In another tab</span>
                )}
              </div>
            ) : loading || !me ? (
              <span className="w-10 shrink-0" aria-hidden="true" />
            ) : null}

          </div>
        </div>
      ) : null}

      {!showCompactDesktopNav && menuOpen ? (
        <div
          id="mobile-nav-menu"
          className="rf-mobile-nav-menu-enter mt-2 flex flex-col gap-0.5 border-t border-[var(--border)] pt-2"
        >
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className={navLinkClass(link.href, 'btn-ghost rounded-xl px-3 py-3 text-base')}
              aria-current={isActivePath(link.href) ? 'page' : undefined}
              onClick={() => setMenuOpen(false)}
            >
              {link.label}
            </Link>
          ))}
          {!loading && !me ? (
            <div className="mt-1 border-t border-[var(--border)] pt-2">
              <Link
                href="/login"
                className="btn-secondary block h-11 px-4 text-center text-sm leading-[2.75rem]"
                onClick={() => setMenuOpen(false)}
              >
                Login
              </Link>
            </div>
          ) : null}
        </div>
      ) : null}

      {voiceSession &&
        confirmLeaveVoiceOpen &&
        portalMounted &&
        createPortal(
          <div className="rf-overlay-enter fixed inset-0 z-[80] flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]">
            <div className="rf-modal-enter panel w-full max-w-sm space-y-4 rounded-2xl border border-[var(--border)] p-6">
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
                    className="btn-danger px-4 py-2 text-sm"
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
