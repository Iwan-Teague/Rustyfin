export const DEFAULT_SETTINGS = {
  serverBaseUrl: '',
  autoLockMinutes: 15,
  excludedDomains: [],
  defaultMatchMode: 'base_domain',
  warnOnHttp: true,
  warnOnUntrustedIframe: true,
  allowManualHttpFill: false,
  pageLoadAutofill: false,
};

const SESSION_KEY = 'rustyvault_session_v1';
const SETTINGS_KEY = 'rustyvault_settings_v1';

export async function getSettings() {
  const stored = await chrome.storage.local.get(SETTINGS_KEY);
  return {
    ...DEFAULT_SETTINGS,
    ...(stored[SETTINGS_KEY] || {}),
  };
}

export async function setSettings(next) {
  const current = await getSettings();
  const merged = { ...current, ...next };
  await chrome.storage.local.set({ [SETTINGS_KEY]: merged });
  return merged;
}

export async function getRustyVaultSession() {
  const stored = await chrome.storage.local.get(SESSION_KEY);
  return stored[SESSION_KEY] || null;
}

export async function setRustyVaultSession(session) {
  await chrome.storage.local.set({ [SESSION_KEY]: session });
}

export async function clearRustyVaultSession() {
  await chrome.storage.local.remove(SESSION_KEY);
}

export function sanitizeServerBaseUrl(value) {
  const trimmed = (value || '').trim();
  if (!trimmed) return '';
  const url = new URL(trimmed);
  url.hash = '';
  url.search = '';
  url.pathname = url.pathname.replace(/\/+$/, '');
  return url.toString().replace(/\/+$/, '');
}

function joinPath(baseUrl, path) {
  return `${baseUrl.replace(/\/+$/, '')}/api/v1${path}`;
}

export async function apiRequest(path, options = {}) {
  const settings = await getSettings();
  if (!settings.serverBaseUrl) {
    throw new Error('Set the Rustyfin server URL in the extension first');
  }
  const headers = new Headers(options.headers || {});
  if (options.vaultAccessToken) {
    headers.set('x-rustyvault-access', options.vaultAccessToken);
  }
  if (options.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }
  const response = await fetch(joinPath(settings.serverBaseUrl, path), {
    ...options,
    headers,
  });
  let body = null;
  const raw = await response.text();
  if (raw.trim()) {
    try {
      body = JSON.parse(raw);
    } catch {
      body = raw;
    }
  }
  if (!response.ok) {
    const errorMessage =
      body?.error?.message ||
      body?.message ||
      (typeof body === 'string' ? body : '') ||
      `Request failed (${response.status})`;
    throw new Error(errorMessage);
  }
  return body;
}

export async function refreshRustyVaultSession() {
  const current = await getRustyVaultSession();
  if (!current?.refresh_token) {
    throw new Error('Extension session is not paired');
  }
  const refreshed = await apiRequest('/vault/device-sessions/refresh', {
    method: 'POST',
    body: JSON.stringify({ refresh_token: current.refresh_token }),
  });
  await setRustyVaultSession(refreshed);
  return refreshed;
}

export async function withRustyVaultSession(path, options = {}) {
  let session = await getRustyVaultSession();
  if (!session?.access_token) {
    throw new Error('Extension is not paired');
  }
  const now = Math.floor(Date.now() / 1000);
  if (session.access_expires_ts <= now + 45) {
    session = await refreshRustyVaultSession();
  }
  try {
    return await apiRequest(path, {
      ...options,
      vaultAccessToken: session.access_token,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (message.toLowerCase().includes('unauthorized')) {
      session = await refreshRustyVaultSession();
      return apiRequest(path, {
        ...options,
        vaultAccessToken: session.access_token,
      });
    }
    throw error;
  }
}
