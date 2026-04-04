'use client';

import { useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { createPortal } from 'react-dom';
import NotificationsPopover from '@/app/components/NotificationsPopover';
import {
  PRIMARY_NAV_GROUPS,
  navigationGroupForPath,
  type NavigationGroupItem,
} from '@/app/navigationGroups';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';

type RootNavItem = {
  href: string;
  label: string;
  items: NavigationGroupItem[];
  icon: ReactNode;
};

const ROOT_NAV_ITEMS: RootNavItem[] = [
  {
    href: '/personal',
    label: 'Personal',
    items: PRIMARY_NAV_GROUPS[0]?.items ?? [],
    icon: (
      <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
        <path d="M12 4.5a3.25 3.25 0 1 1 0 6.5 3.25 3.25 0 0 1 0-6.5Z" stroke="currentColor" strokeWidth="1.8" />
        <path d="M6.5 19a5.5 5.5 0 0 1 11 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    href: '/social',
    label: 'Social',
    items: PRIMARY_NAV_GROUPS[1]?.items ?? [],
    icon: (
      <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
        <path d="M5 7.5h14v8H9l-4 3v-11Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      </svg>
    ),
  },
  {
    href: '/ai',
    label: 'AI',
    items: [],
    icon: (
      <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
        <path d="M8 6.5 12 4l4 2.5v5L12 14l-4-2.5v-5Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
        <path d="M8.5 15.5 12 18l3.5-2.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    href: '/server',
    label: 'Server',
    items: PRIMARY_NAV_GROUPS[2]?.items ?? [],
    icon: (
      <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
        <rect x="4" y="5" width="16" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
        <rect x="4" y="14" width="16" height="5" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
        <path d="M8 7.5h.01M8 16.5h.01" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    ),
  },
];

function BarsIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h16" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
      <path d="M12 5a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7Z" stroke="currentColor" strokeWidth="1.8" />
      <path d="M5.5 19a6.5 6.5 0 0 1 13 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function AdminIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
      <path d="m12 4 6 3v5c0 4-2.5 6.5-6 8-3.5-1.5-6-4-6-8V7l6-3Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      <path d="M9.5 12.5 11 14l3.5-4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function VoiceIcon() {
  return (
    <svg viewBox="0 0 24 24" className="h-5 w-5" fill="none" aria-hidden="true">
      <path d="M4 12a8 8 0 0 1 16 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      <rect x="2.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
      <rect x="17.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
    </svg>
  );
}

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
  const activeGroup = navigationGroupForPath(pathname);
  const [menuOpen, setMenuOpen] = useState(false);
  const [portalMounted, setPortalMounted] = useState(false);
  const [confirmLeaveVoiceOpen, setConfirmLeaveVoiceOpen] = useState(false);
  const [railHovered, setRailHovered] = useState(false);
  const [expandedGroupHrefs, setExpandedGroupHrefs] = useState<string[]>([]);

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
    setMenuOpen(false);
  }, [pathname]);

  useEffect(() => {
    if (activeGroup?.href) {
      setExpandedGroupHrefs((prev) =>
        prev.includes(activeGroup.href) ? prev : [...prev, activeGroup.href],
      );
    }
  }, [activeGroup?.href]);

  if (pathname.startsWith('/setup')) {
    return null;
  }

  const activeVoiceChannelName = voiceSession?.channelName ?? connectedVoiceChannelName ?? 'Voice';
  const showVoiceWidget = Boolean(connectedVoiceChannelId && activeVoiceChannelName);
  const hasLocalStream = hasLocalVoiceSession && voiceSession?.localStream !== null;
  const muted = hasLocalVoiceSession ? (voiceSession?.muted ?? false) : false;
  const deafened = hasLocalVoiceSession ? (voiceSession?.deafened ?? false) : false;
  const baseVoiceActionClass =
    'inline-flex h-9 w-9 items-center justify-center rounded-full border transition disabled:cursor-not-allowed disabled:opacity-40';
  const railExpanded = railHovered;

  const navLinks = useMemo(
    () => [
      ...ROOT_NAV_ITEMS.map(({ href, label }) => ({ href, label })),
      ...(!loading && me?.role === 'admin' ? [{ href: '/admin', label: 'Admin' }] : []),
    ],
    [loading, me?.role],
  );

  const isActivePath = (href: string) => {
    if (pathname === href || pathname.startsWith(`${href}/`)) {
      return true;
    }
    if (activeGroup && href === activeGroup.href) {
      return true;
    }
    return false;
  };

  const railLinkClass = (href: string) =>
    `rf-nav-link rf-nav-root btn-ghost flex min-h-[3.35rem] items-center rounded-[1.1rem] text-base font-semibold transition ${
      isActivePath(href) ? 'text-[var(--text-main)]' : ''
    } ${
      railExpanded ? 'w-full justify-start gap-3 px-3.5' : 'w-full justify-center px-0'
    }`;

  const railUtilityClass = (href: string) =>
    `rf-nav-link btn-ghost flex min-h-[3.25rem] items-center rounded-[1.1rem] text-sm font-medium transition ${
      isActivePath(href) ? 'text-[var(--text-main)]' : ''
    } ${
      railExpanded ? 'w-full justify-start gap-3 px-3.5' : 'w-full justify-center px-0'
    }`;

  const openGroupHrefs = railExpanded ? new Set(expandedGroupHrefs) : new Set<string>();

  const desktopRail = (
    <aside
      className={`app-nav app-nav-rail animate-rise hidden md:flex md:flex-col ${
        railExpanded ? 'w-[15.75rem]' : 'w-[5.75rem]'
      }`}
      data-expanded={railExpanded ? 'true' : 'false'}
      onMouseEnter={() => setRailHovered(true)}
      onMouseLeave={() => setRailHovered(false)}
    >
      <div className="flex items-center justify-center">
        <Link
          href="/"
          className={`accent-logo flex h-11 items-center rounded-2xl px-2 text-center font-semibold transition hover:opacity-90 ${
            railExpanded ? 'w-full justify-start text-2xl' : 'w-11 justify-center text-xl'
          }`}
          aria-label="Go to Rustyfin home"
        >
          <span className="shrink-0">{railExpanded ? 'Rustyfin' : 'R'}</span>
        </Link>
      </div>

      <div className="mt-5 flex min-h-0 flex-1 flex-col gap-2">
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
          {ROOT_NAV_ITEMS.map((item) => {
            const showChildren = Boolean(item.items.length && openGroupHrefs.has(item.href));
            return (
              <div key={item.href} className="flex flex-col gap-1">
                {item.items.length ? (
                  <button
                    type="button"
                    className={railLinkClass(item.href)}
                    aria-current={isActivePath(item.href) ? 'page' : undefined}
                    aria-expanded={showChildren}
                    onClick={() => {
                      setExpandedGroupHrefs((prev) =>
                        prev.includes(item.href)
                          ? prev.filter((href) => href !== item.href)
                          : [...prev, item.href],
                      );
                    }}
                  >
                    <span className="shrink-0">{item.icon}</span>
                    {railExpanded ? <span className="truncate">{item.label}</span> : null}
                  </button>
                ) : (
                  <Link
                    href={item.href}
                    className={railLinkClass(item.href)}
                    aria-current={isActivePath(item.href) ? 'page' : undefined}
                  >
                    <span className="shrink-0">{item.icon}</span>
                    {railExpanded ? <span className="truncate">{item.label}</span> : null}
                  </Link>
                )}
                {item.items.length ? (
                  <div
                    className="rf-nav-children ml-3"
                    data-open={showChildren ? 'true' : 'false'}
                    aria-hidden={!showChildren}
                  >
                    <div className="rf-nav-children-inner flex flex-col gap-1 pt-0.5">
                      {item.items.map((subItem) => (
                        <Link
                          key={subItem.href}
                          href={subItem.href}
                          className={`rf-nav-link rf-nav-subpage btn-ghost min-h-10 rounded-[0.95rem] px-3 py-2 text-left text-[0.95rem] font-medium ${
                            isActivePath(subItem.href) ? 'text-[var(--text-main)]' : 'text-white/70'
                          }`}
                          aria-current={isActivePath(subItem.href) ? 'page' : undefined}
                          tabIndex={showChildren ? 0 : -1}
                        >
                          {subItem.label}
                        </Link>
                      ))}
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>

        {showVoiceWidget ? (
          <div className="mt-3 rounded-2xl border border-green-500/35 bg-black/30 p-2">
            <Link
              href="/channels"
              className={`flex items-center text-green-300 ${railExpanded ? 'gap-3' : 'justify-center'}`}
              title={`Open channel: ${activeVoiceChannelName}`}
            >
              <span className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-green-500/12">
                <span className="h-2.5 w-2.5 rounded-full bg-green-400" />
              </span>
              {railExpanded ? (
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium">{activeVoiceChannelName}</span>
                  <span className="block text-xs text-green-200/80">
                    {hasLocalVoiceSession ? 'Connected' : 'In another tab'}
                  </span>
                </span>
              ) : null}
            </Link>

            {railExpanded && hasLocalVoiceSession ? (
              <div className="mt-2 flex items-center gap-2">
                <button
                  type="button"
                  onClick={toggleMute}
                  disabled={!hasLocalStream}
                  className={`${baseVoiceActionClass} ${
                    muted
                      ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                      : 'border-[var(--border)] bg-black/45 text-white/85 hover:text-white'
                  }`}
                  aria-label={muted ? 'Unmute microphone' : 'Mute microphone'}
                >
                  <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                    <rect x="9" y="3.5" width="6" height="10" rx="3" stroke="currentColor" strokeWidth="1.8" />
                    <path d="M6.5 11.5a5.5 5.5 0 0 0 11 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <path d="M12 17v3.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <path d="M8.5 20.5h7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    {muted ? (
                      <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    ) : null}
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={toggleDeafen}
                  className={`${baseVoiceActionClass} ${
                    deafened
                      ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                      : 'border-[var(--border)] bg-black/45 text-white/85 hover:text-white'
                  }`}
                  aria-label={deafened ? 'Undeafen' : 'Deafen'}
                >
                  <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                    <path d="M4 12a8 8 0 0 1 16 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    <rect x="2.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                    <rect x="17.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                    <path d="M17.5 18.5a4.5 4.5 0 0 1-4.5 4.5h-1" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    {deafened ? (
                      <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                    ) : null}
                  </svg>
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmLeaveVoiceOpen(true)}
                  className={`${baseVoiceActionClass} border-[var(--border)] bg-black/55 text-white/90 hover:text-white`}
                  aria-label="Disconnect from voice"
                >
                  <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                    <path d="M7 7l10 10M17 7 7 17" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
                  </svg>
                </button>
              </div>
            ) : null}

            {!railExpanded ? (
              <div className="mt-2 flex justify-center text-green-300/90">
                <span className="inline-flex h-9 w-9 items-center justify-center rounded-full bg-green-500/10">
                  <VoiceIcon />
                </span>
              </div>
            ) : null}
          </div>
        ) : null}

        <div className="mt-auto flex flex-col gap-1.5">
          {!loading && me?.role === 'admin' ? (
            <Link
              href="/admin"
              className={railUtilityClass('/admin')}
              aria-current={isActivePath('/admin') ? 'page' : undefined}
            >
              <span className="shrink-0">
                <AdminIcon />
              </span>
              {railExpanded ? <span className="truncate">Admin</span> : null}
            </Link>
          ) : null}

          {!loading && me ? (
            <Link
              href="/account"
              className={railUtilityClass('/account')}
              aria-current={isActivePath('/account') ? 'page' : undefined}
            >
              <span className="shrink-0">
                <UserIcon />
              </span>
              {railExpanded ? <span className="truncate">{me.username}</span> : null}
            </Link>
          ) : null}

          {!loading && me ? (
            <NotificationsPopover
              isAdmin={me.role === 'admin'}
              className={railExpanded ? 'w-full' : ''}
            />
          ) : null}

          {!loading && !me ? (
            <Link href="/login" className={railUtilityClass('/login')}>
              <span className="shrink-0">
                <UserIcon />
              </span>
              {railExpanded ? <span className="truncate">Login</span> : null}
            </Link>
          ) : null}
        </div>
      </div>
    </aside>
  );

  const mobileTopBar = (
    <nav className="app-nav animate-rise rounded-2xl px-4 py-3 md:hidden">
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
          <BarsIcon />
        </button>

        <div className="flex min-w-0 items-center justify-end gap-1.5">
          {!loading && me ? (
            <>
              {me.role === 'admin' ? (
                <Link
                  href="/admin"
                  className={`rf-nav-link btn-ghost h-11 shrink-0 px-3 text-sm ${
                    isActivePath('/admin') ? 'text-[var(--text-main)]' : ''
                  }`}
                  aria-current={isActivePath('/admin') ? 'page' : undefined}
                >
                  Admin
                </Link>
              ) : null}
              <Link
                href="/account"
                className={`rf-nav-link btn-ghost h-11 max-w-[9.5rem] shrink-0 px-3 text-sm ${
                  isActivePath('/account') ? 'text-[var(--text-main)]' : ''
                }`}
                aria-current={isActivePath('/account') ? 'page' : undefined}
              >
                <span className="truncate">{me.username}</span>
              </Link>
              <NotificationsPopover isAdmin={me.role === 'admin'} />
            </>
          ) : null}
        </div>
      </div>

      {menuOpen ? (
        <div
          id="mobile-nav-menu"
          className="rf-mobile-nav-menu-enter mt-2 flex flex-col gap-0.5 border-t border-[var(--border)] pt-2"
        >
          {navLinks.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className={`rf-nav-link btn-ghost rounded-xl px-3 py-3 text-base ${
                isActivePath(link.href) ? 'text-[var(--text-main)]' : ''
              }`}
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
    </nav>
  );

  return (
    <>
      {desktopRail}
      {mobileTopBar}

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
    </>
  );
}
