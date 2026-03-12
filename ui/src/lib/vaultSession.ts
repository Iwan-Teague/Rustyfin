'use client';

import {
  createVaultDeviceSession,
  refreshVaultDeviceSession,
  type VaultDeviceSessionTokens,
} from './vaultApi';

const VAULT_SESSION_STORAGE_KEY = 'rustfin_vault_session_v1';
const ACCESS_SKEW_SECONDS = 45;

export type StoredVaultSession = VaultDeviceSessionTokens;

function isBrowser() {
  return typeof window !== 'undefined';
}

function deviceName(): string {
  if (!isBrowser()) return 'Rustyfin Web Vault';
  const platform = navigator.platform?.trim() || 'Browser';
  return `Rustyfin Web Vault (${platform})`;
}

function devicePlatform(): string {
  if (!isBrowser()) return 'browser';
  const parts = ['browser', navigator.platform?.trim() || '', navigator.userAgent || ''].filter(
    Boolean,
  );
  return parts.join(':').slice(0, 80);
}

export function readVaultSession(): StoredVaultSession | null {
  if (!isBrowser()) return null;
  try {
    const raw = sessionStorage.getItem(VAULT_SESSION_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<StoredVaultSession>;
    if (
      typeof parsed.session_id !== 'string' ||
      typeof parsed.access_token !== 'string' ||
      typeof parsed.refresh_token !== 'string' ||
      typeof parsed.access_expires_ts !== 'number' ||
      typeof parsed.refresh_expires_ts !== 'number'
    ) {
      return null;
    }
    return {
      session_id: parsed.session_id,
      access_token: parsed.access_token,
      refresh_token: parsed.refresh_token,
      access_expires_ts: parsed.access_expires_ts,
      refresh_expires_ts: parsed.refresh_expires_ts,
    };
  } catch {
    return null;
  }
}

export function writeVaultSession(session: StoredVaultSession | null) {
  if (!isBrowser()) return;
  if (!session) {
    sessionStorage.removeItem(VAULT_SESSION_STORAGE_KEY);
    return;
  }
  sessionStorage.setItem(VAULT_SESSION_STORAGE_KEY, JSON.stringify(session));
}

export function clearVaultSession() {
  writeVaultSession(null);
}

function accessTokenStillValid(session: StoredVaultSession) {
  const now = Math.floor(Date.now() / 1000);
  return session.access_expires_ts > now + ACCESS_SKEW_SECONDS;
}

function refreshTokenStillValid(session: StoredVaultSession) {
  const now = Math.floor(Date.now() / 1000);
  return session.refresh_expires_ts > now + ACCESS_SKEW_SECONDS;
}

export async function createWebVaultSession(): Promise<StoredVaultSession> {
  const response = await createVaultDeviceSession({
    client_kind: 'web_vault',
    device_name: deviceName(),
    device_platform: devicePlatform(),
  });
  if (!response.session) {
    throw new Error('Vault device session was not issued');
  }
  writeVaultSession(response.session);
  return response.session;
}

export async function refreshStoredVaultSession(
  current?: StoredVaultSession | null,
): Promise<StoredVaultSession> {
  const session = current ?? readVaultSession();
  if (!session || !refreshTokenStillValid(session)) {
    throw new Error('Vault session refresh token is unavailable');
  }
  const refreshed = await refreshVaultDeviceSession(session.refresh_token);
  writeVaultSession(refreshed);
  return refreshed;
}

export async function ensureWebVaultSession(): Promise<StoredVaultSession> {
  const existing = readVaultSession();
  if (!existing) {
    return createWebVaultSession();
  }
  if (accessTokenStillValid(existing)) {
    return existing;
  }
  if (refreshTokenStillValid(existing)) {
    try {
      return await refreshStoredVaultSession(existing);
    } catch {
      clearVaultSession();
    }
  }
  return createWebVaultSession();
}
