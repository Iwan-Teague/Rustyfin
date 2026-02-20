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

export type Me = { id: string; username: string; role: 'admin' | 'user' };

type AuthState = {
  me: Me | null;
  loading: boolean;
  refreshMe: () => Promise<void>;
  logout: () => void;
};

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);
  const router = useRouter();

  const refreshMe = useCallback(async () => {
    const token = localStorage.getItem('token');
    if (!token) {
      setMe(null);
      return;
    }
    try {
      const data = await apiJson<Me>('/users/me');
      setMe(data);
    } catch {
      localStorage.removeItem('token');
      setMe(null);
    }
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem('token');
    setMe(null);
    router.push('/login');
  }, [router]);

  useEffect(() => {
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
