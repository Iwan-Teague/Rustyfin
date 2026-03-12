'use client';

import { useEffect, useMemo, useRef, type ReactNode } from 'react';
import { usePathname } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { readBrowserToken } from '@/lib/browserAuth';
import { postBrowserActivity } from '@/lib/userProfileApi';

const TAB_ID_KEY = 'rustfin_activity_tab_id_v1';
const HEARTBEAT_INTERVAL_MS = 30_000;

type ActiveSession = {
  clientSessionId: string;
  tabId: string;
  section: string;
};

function generateId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `activity-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function getOrCreateTabId(): string {
  if (typeof window === 'undefined') {
    return generateId();
  }
  try {
    const cached = window.sessionStorage.getItem(TAB_ID_KEY);
    if (cached) {
      return cached;
    }
    const next = generateId();
    window.sessionStorage.setItem(TAB_ID_KEY, next);
    return next;
  } catch {
    return generateId();
  }
}

function mapPathnameToSection(pathname: string): string | null {
  if (!pathname || pathname === '/') return 'home';
  if (pathname.startsWith('/channels')) return 'channels';
  if (pathname.startsWith('/rooms')) return 'rooms';
  if (pathname.startsWith('/servers')) return 'servers';
  if (pathname.startsWith('/calendar')) return 'calendar';
  if (pathname.startsWith('/libraries') || pathname.startsWith('/player/')) return 'libraries';
  if (pathname.startsWith('/admin')) return 'admin';
  if (pathname.startsWith('/account')) return 'account';
  if (pathname.startsWith('/login') || pathname.startsWith('/setup')) return null;
  return 'home';
}

export default function ActivityPresenceProvider({
  children,
}: {
  children: ReactNode;
}) {
  const { me } = useAuth();
  const pathname = usePathname();
  const section = useMemo(() => mapPathnameToSection(pathname), [pathname]);
  const activeSessionRef = useRef<ActiveSession | null>(null);

  useEffect(() => {
    if (typeof document === 'undefined') {
      return;
    }

    async function stopCurrent(keepalive = false) {
      const active = activeSessionRef.current;
      if (!active || !readBrowserToken()) {
        activeSessionRef.current = null;
        return;
      }
      activeSessionRef.current = null;
      await postBrowserActivity(
        {
          client_session_id: active.clientSessionId,
          tab_id: active.tabId,
          section: active.section,
          event: 'stop',
        },
        keepalive,
      ).catch(() => {});
    }

    async function startCurrent(nextSection: string) {
      if (!me || !readBrowserToken()) return;
      const nextSession: ActiveSession = {
        clientSessionId: generateId(),
        tabId: getOrCreateTabId(),
        section: nextSection,
      };
      activeSessionRef.current = nextSession;
      await postBrowserActivity({
        client_session_id: nextSession.clientSessionId,
        tab_id: nextSession.tabId,
        section: nextSession.section,
        event: 'start',
      }).catch(() => {});
    }

    const visible = document.visibilityState === 'visible';
    const active = activeSessionRef.current;

    if (!me || !section || !visible) {
      void stopCurrent();
      return;
    }

    if (!active || active.section !== section) {
      void (async () => {
        await stopCurrent();
        await startCurrent(section);
      })();
    }

    return () => {
      if (activeSessionRef.current?.section === section && !me) {
        void stopCurrent();
      }
    };
  }, [me, section]);

  useEffect(() => {
    if (!me) return;
    const interval = window.setInterval(() => {
      const active = activeSessionRef.current;
      if (!active || !readBrowserToken() || document.visibilityState !== 'visible') {
        return;
      }
      void postBrowserActivity({
        client_session_id: active.clientSessionId,
        tab_id: active.tabId,
        section: active.section,
        event: 'heartbeat',
      }).catch(() => {});
    }, HEARTBEAT_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [me]);

  useEffect(() => {
    async function stopCurrent(keepalive = false) {
      const active = activeSessionRef.current;
      if (!active || !readBrowserToken()) {
        activeSessionRef.current = null;
        return;
      }
      activeSessionRef.current = null;
      await postBrowserActivity(
        {
          client_session_id: active.clientSessionId,
          tab_id: active.tabId,
          section: active.section,
          event: 'stop',
        },
        keepalive,
      ).catch(() => {});
    }

    async function startIfNeeded() {
      if (!me || !section || document.visibilityState !== 'visible' || activeSessionRef.current) {
        return;
      }
      const nextSession: ActiveSession = {
        clientSessionId: generateId(),
        tabId: getOrCreateTabId(),
        section,
      };
      activeSessionRef.current = nextSession;
      await postBrowserActivity({
        client_session_id: nextSession.clientSessionId,
        tab_id: nextSession.tabId,
        section: nextSession.section,
        event: 'start',
      }).catch(() => {});
    }

    const handleVisibility = () => {
      if (document.visibilityState === 'visible') {
        void startIfNeeded();
      } else {
        void stopCurrent();
      }
    };

    const handlePageHide = () => {
      void stopCurrent(true);
    };

    document.addEventListener('visibilitychange', handleVisibility);
    window.addEventListener('pagehide', handlePageHide);
    return () => {
      document.removeEventListener('visibilitychange', handleVisibility);
      window.removeEventListener('pagehide', handlePageHide);
    };
  }, [me, section]);

  return <>{children}</>;
}
