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
function rustyvaultServerReachabilityMessage(serverBaseUrl) {
    const normalizedBaseUrl = sanitizeServerBaseUrl(serverBaseUrl);
    if (normalizedBaseUrl.startsWith('https://')) {
        return 'Could not reach that Rustyfin server address. If Rustyfin is using a self-signed or otherwise untrusted HTTPS certificate, trust that certificate in the browser first.';
    }
    return 'Could not reach that Rustyfin server address';
}
export async function verifyRustyfinServerBaseUrl(serverBaseUrl) {
    const normalizedBaseUrl = sanitizeServerBaseUrl(serverBaseUrl);
    let response;
    try {
        response = await fetch(`${normalizedBaseUrl}/runtime-config`, {
            method: 'GET',
            cache: 'no-store',
            credentials: 'omit',
            headers: {
                Accept: 'application/json',
            },
        });
    }
    catch {
        throw new Error(rustyvaultServerReachabilityMessage(normalizedBaseUrl));
    }
    if (!response.ok) {
        throw new Error(`Rustyfin server check failed (${response.status})`);
    }
    let body = null;
    try {
        body = await response.json();
    }
    catch {
        throw new Error('That address responded, but it did not look like a Rustyfin server');
    }
    if (!body || typeof body !== 'object' || !Array.isArray(body.ice_servers)) {
        throw new Error('That address responded, but it did not look like a Rustyfin server');
    }
    return { normalizedBaseUrl };
}
export function parseRustyVaultConnectionInput(input) {
    const trimmed = (input || '').trim();
    if (!trimmed) {
        throw new Error('Enter a pairing code from Rustyfin /vault');
    }
    if (/^RFVLT-[A-Z0-9-]+$/i.test(trimmed)) {
        return {
            serverBaseUrl: null,
            pairingCode: trimmed.toUpperCase(),
        };
    }
    let parsed;
    try {
        parsed = new URL(trimmed);
    }
    catch {
        throw new Error('Enter a valid pairing code or RustyVault connection code');
    }
    if (parsed.protocol !== 'rustyvault:' || parsed.hostname !== 'pair') {
        throw new Error('Enter a valid RustyVault connection code');
    }
    const server = parsed.searchParams.get('server');
    const code = parsed.searchParams.get('code');
    if (!server || !code) {
        throw new Error('RustyVault connection code is missing the server URL or pairing code');
    }
    return {
        serverBaseUrl: sanitizeServerBaseUrl(server),
        pairingCode: code.trim().toUpperCase(),
    };
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
    let response;
    try {
        response = await fetch(joinPath(settings.serverBaseUrl, path), {
            ...options,
            headers,
        });
    }
    catch {
        throw new Error(rustyvaultServerReachabilityMessage(settings.serverBaseUrl));
    }
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
