'use client';

import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useCallback,
} from 'react';
import { useRouter } from 'next/navigation';
import { apiJson } from './api';
import { clearBrowserToken, readBrowserToken } from './browserAuth';

export type Me = {
  id: string;
  username: string;
  role: 'admin' | 'user';
  login_username?: string;
  time_zone?: string | null;
  avatar_url?: string | null;
};

type AuthState = {
  me: Me | null;
  loading: boolean;
  refreshMe: () => Promise<void>;
  replaceMe: (next: Me | null) => void;
  updateMe: (next: Partial<Me>) => void;
  logout: () => void;
};

const AuthContext = createContext<AuthState | null>(null);
const AUTH_ME_CACHE_KEY = 'rustfin_auth_me_v1';
const AUTH_ME_CHANNEL = 'rustfin-me';

function isRole(value: unknown): value is Me['role'] {
  return value === 'admin' || value === 'user';
}

function normalizeMe(raw: unknown): Me | null {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return null;
  }
  const parsed = raw as Partial<Me>;
  if (
    typeof parsed.id !== 'string' ||
    typeof parsed.username !== 'string' ||
    !isRole(parsed.role)
  ) {
    return null;
  }
  return {
    id: parsed.id,
    username: parsed.username,
    role: parsed.role,
    login_username:
      typeof parsed.login_username === 'string' ? parsed.login_username : undefined,
    time_zone:
      typeof parsed.time_zone === 'string' || parsed.time_zone === null
        ? parsed.time_zone
        : undefined,
    avatar_url:
      typeof parsed.avatar_url === 'string' || parsed.avatar_url === null
        ? parsed.avatar_url
        : undefined,
  };
}

function readCachedMe(): Me | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = localStorage.getItem(AUTH_ME_CACHE_KEY);
    if (!raw) return null;
    return normalizeMe(JSON.parse(raw));
  } catch {
    return null;
  }
}

function writeCachedMe(me: Me | null) {
  if (typeof window === 'undefined') return;
  try {
    if (!me) {
      localStorage.removeItem(AUTH_ME_CACHE_KEY);
      return;
    }
    localStorage.setItem(AUTH_ME_CACHE_KEY, JSON.stringify(me));
  } catch {
    // ignore storage errors
  }
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);
  const router = useRouter();
  const channelRef = React.useRef<BroadcastChannel | null>(null);
  const instanceIdRef = React.useRef(
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : `me-${Date.now()}`,
  );

  const syncMe = useCallback((next: Me | null, broadcast: boolean) => {
    setMe(next);
    writeCachedMe(next);
    if (!broadcast || typeof window === 'undefined') {
      return;
    }
    try {
      channelRef.current?.postMessage({
        type: 'replace-me',
        source: instanceIdRef.current,
        me: next,
      });
    } catch {
      // ignore BroadcastChannel errors
    }
  }, []);

  const replaceMe = useCallback(
    (next: Me | null) => {
      syncMe(next, true);
    },
    [syncMe],
  );

  const updateMe = useCallback(
    (next: Partial<Me>) => {
      setMe((current) => {
        if (!current) return current;
        const updated = { ...current, ...next };
        writeCachedMe(updated);
        try {
          channelRef.current?.postMessage({
            type: 'replace-me',
            source: instanceIdRef.current,
            me: updated,
          });
        } catch {
          // ignore BroadcastChannel errors
        }
        return updated;
      });
    },
    [],
  );

  const refreshMe = useCallback(async () => {
    const token = readBrowserToken();
    if (!token) {
      syncMe(null, false);
      return;
    }
    try {
      const data = normalizeMe(await apiJson<unknown>('/users/me'));
      if (!data) {
        throw new Error('Invalid authenticated user payload');
      }
      syncMe(data, true);
    } catch {
      clearBrowserToken();
      syncMe(null, true);
    }
  }, [syncMe]);

  const logout = useCallback(() => {
    clearBrowserToken();
    syncMe(null, true);
    router.push('/login');
  }, [router, syncMe]);

  useEffect(() => {
    const token = readBrowserToken();
    if (!token) {
      syncMe(null, false);
      setLoading(false);
      return;
    }

    const cached = readCachedMe();
    if (cached) {
      setMe(cached);
      setLoading(false);
    }

    refreshMe().finally(() => setLoading(false));
  }, [refreshMe, syncMe]);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    if (typeof BroadcastChannel === 'function') {
      channelRef.current = new BroadcastChannel(AUTH_ME_CHANNEL);
      channelRef.current.onmessage = (event) => {
        const payload = event.data as { type?: string; source?: string; me?: Me | null } | null;
        if (!payload || payload.type !== 'replace-me' || payload.source === instanceIdRef.current) {
          return;
        }
        setMe(payload.me ?? null);
        writeCachedMe(payload.me ?? null);
      };
    }

    const handleStorage = (event: StorageEvent) => {
      if (event.key !== AUTH_ME_CACHE_KEY) return;
      setMe(readCachedMe());
    };

    window.addEventListener('storage', handleStorage);
    return () => {
      window.removeEventListener('storage', handleStorage);
      channelRef.current?.close();
      channelRef.current = null;
    };
  }, []);

  const value = useMemo(
    () => ({ me, loading, refreshMe, replaceMe, updateMe, logout }),
    [me, loading, refreshMe, replaceMe, updateMe, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
