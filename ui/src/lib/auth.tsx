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
  avatar_url?: string | null;
};

type AuthState = {
  me: Me | null;
  loading: boolean;
  refreshMe: () => Promise<void>;
  logout: () => void;
};

const AuthContext = createContext<AuthState | null>(null);
const AUTH_ME_CACHE_KEY = 'rustfin_auth_me_v1';

function isRole(value: unknown): value is Me['role'] {
  return value === 'admin' || value === 'user';
}

function readCachedMe(): Me | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = localStorage.getItem(AUTH_ME_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Me>;
    if (
      typeof parsed?.id !== 'string' ||
      typeof parsed?.username !== 'string' ||
      !isRole(parsed?.role)
    ) {
      return null;
    }
    return {
      id: parsed.id,
      username: parsed.username,
      role: parsed.role,
      login_username:
        typeof parsed.login_username === 'string' ? parsed.login_username : undefined,
      avatar_url:
        typeof parsed.avatar_url === 'string' || parsed.avatar_url === null
          ? parsed.avatar_url
          : undefined,
    };
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

  const refreshMe = useCallback(async () => {
    const token = readBrowserToken();
    if (!token) {
      setMe(null);
      writeCachedMe(null);
      return;
    }
    try {
      const data = await apiJson<Me>('/users/me');
      setMe(data);
      writeCachedMe(data);
    } catch {
      clearBrowserToken();
      setMe(null);
      writeCachedMe(null);
    }
  }, []);

  const logout = useCallback(() => {
    clearBrowserToken();
    setMe(null);
    writeCachedMe(null);
    router.push('/login');
  }, [router]);

  useEffect(() => {
    const token = readBrowserToken();
    if (!token) {
      setMe(null);
      writeCachedMe(null);
      setLoading(false);
      return;
    }

    const cached = readCachedMe();
    if (cached) {
      setMe(cached);
      setLoading(false);
    }

    refreshMe().finally(() => setLoading(false));
  }, [refreshMe]);

  const value = useMemo(
    () => ({ me, loading, refreshMe, logout }),
    [me, loading, refreshMe, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
