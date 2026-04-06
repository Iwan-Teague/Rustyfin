import { clearSession, getSession, getSettings, setSession } from './storage.js';
export function sanitizeServerBaseUrl(value) {
    const trimmed = (value || '').trim();
    if (!trimmed)
        return '';
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
    const raw = await response.text();
    let body = null;
    if (raw.trim()) {
        try {
            body = JSON.parse(raw);
        }
        catch {
            body = raw;
        }
    }
    if (!response.ok) {
        const message = body?.error?.message ||
            body?.message ||
            (typeof body === 'string' ? body : '') ||
            `Request failed (${response.status})`;
        throw new Error(message);
    }
    return body;
}
export async function refreshRustyVaultSession() {
    const current = await getSession();
    if (!current?.refresh_token) {
        throw new Error('Extension session is not paired');
    }
    const refreshed = await apiRequest('/vault/device-sessions/refresh', {
        method: 'POST',
        body: JSON.stringify({ refresh_token: current.refresh_token }),
    });
    await setSession(refreshed);
    return refreshed;
}
export async function withRustyVaultSession(path, options = {}) {
    let session = await getSession();
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
    }
    catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (message.toLowerCase().includes('unauthorized')) {
            try {
                session = await refreshRustyVaultSession();
                return await apiRequest(path, {
                    ...options,
                    vaultAccessToken: session.access_token,
                });
            }
            catch (refreshError) {
                await clearSession();
                throw refreshError;
            }
        }
        throw error;
    }
}
export function defaultRustyVaultPreferences() {
    return {
        auto_lock_minutes: 15,
        clipboard_clear_seconds: 30,
        inline_save_prompt_enabled: true,
        inline_autofill_enabled: true,
        default_match_mode: 'base_domain',
        warn_on_http: true,
        warn_on_untrusted_iframe: true,
        excluded_domains: [],
        allow_manual_http_fill: false,
        password_generator_default_preset: 'balanced',
        password_generator_default_length: 22,
        password_generator_include_uppercase: true,
        password_generator_include_lowercase: true,
        password_generator_include_numbers: true,
        password_generator_include_symbols: true,
        password_generator_exclude_ambiguous: true,
    };
}
export async function getVaultPreferences() {
    const prefs = await withRustyVaultSession('/vault/preferences');
    return {
        ...defaultRustyVaultPreferences(),
        ...(prefs || {}),
    };
}
