'use client';

import {
  createRustyVaultDeviceSession,
  refreshRustyVaultDeviceSession,
  type RustyVaultDeviceSessionTokens,
} from './api';

const RUSTYVAULT_SESSION_STORAGE_KEY = 'rustyvault_session_v1';
const ACCESS_SKEW_SECONDS = 45;

export type StoredRustyVaultSession = RustyVaultDeviceSessionTokens;

function isBrowser() {
  return typeof window !== 'undefined';
}

function deviceName(): string {
  if (!isBrowser()) return 'RustyVault Web Vault';
  const platform = navigator.platform?.trim() || 'Browser';
  return `RustyVault Web Vault (${platform})`;
}

function devicePlatform(): string {
  if (!isBrowser()) return 'browser';
  const parts = ['browser', navigator.platform?.trim() || '', navigator.userAgent || ''].filter(
    Boolean,
  );
  return parts.join(':').slice(0, 80);
}

export function readRustyVaultSession(): StoredRustyVaultSession | null {
  if (!isBrowser()) return null;
  try {
    const raw = sessionStorage.getItem(RUSTYVAULT_SESSION_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<StoredRustyVaultSession>;
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

export function writeRustyVaultSession(session: StoredRustyVaultSession | null) {
  if (!isBrowser()) return;
  if (!session) {
    sessionStorage.removeItem(RUSTYVAULT_SESSION_STORAGE_KEY);
    return;
  }
  sessionStorage.setItem(RUSTYVAULT_SESSION_STORAGE_KEY, JSON.stringify(session));
}

export function clearRustyVaultSession() {
  writeRustyVaultSession(null);
}

function accessTokenStillValid(session: StoredRustyVaultSession) {
  const now = Math.floor(Date.now() / 1000);
  return session.access_expires_ts > now + ACCESS_SKEW_SECONDS;
}

function refreshTokenStillValid(session: StoredRustyVaultSession) {
  const now = Math.floor(Date.now() / 1000);
  return session.refresh_expires_ts > now + ACCESS_SKEW_SECONDS;
}

export async function createRustyVaultWebSession(): Promise<StoredRustyVaultSession> {
  const response = await createRustyVaultDeviceSession({
    client_kind: 'rustyvault_web',
    device_name: deviceName(),
    device_platform: devicePlatform(),
  });
  if (!response.session) {
    throw new Error('Vault device session was not issued');
  }
  writeRustyVaultSession(response.session);
  return response.session;
}

export async function refreshStoredRustyVaultSession(
  current?: StoredRustyVaultSession | null,
): Promise<StoredRustyVaultSession> {
  const session = current ?? readRustyVaultSession();
  if (!session || !refreshTokenStillValid(session)) {
    throw new Error('Vault session refresh token is unavailable');
  }
  const refreshed = await refreshRustyVaultDeviceSession(session.refresh_token);
  writeRustyVaultSession(refreshed);
  return refreshed;
}

export async function ensureRustyVaultWebSession(): Promise<StoredRustyVaultSession> {
  const existing = readRustyVaultSession();
  if (!existing) {
    return createRustyVaultWebSession();
  }
  if (accessTokenStillValid(existing)) {
    return existing;
  }
  if (refreshTokenStillValid(existing)) {
    try {
      return await refreshStoredRustyVaultSession(existing);
    } catch {
      clearRustyVaultSession();
    }
  }
  return createRustyVaultWebSession();
}
