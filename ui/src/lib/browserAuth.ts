export const AUTH_TOKEN_STORAGE_KEY = 'token';
export const AUTH_TOKEN_COOKIE_KEY = 'rustfin_token';

function buildCookieAttributes(maxAgeSeconds: number): string {
  return `Path=/; Max-Age=${maxAgeSeconds}; SameSite=Lax`;
}

export function readBrowserToken(): string | null {
  if (typeof window === 'undefined') {
    return null;
  }
  return localStorage.getItem(AUTH_TOKEN_STORAGE_KEY);
}

export function writeBrowserToken(token: string) {
  if (typeof window === 'undefined') {
    return;
  }
  localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, token);
  document.cookie = `${AUTH_TOKEN_COOKIE_KEY}=${encodeURIComponent(token)}; ${buildCookieAttributes(60 * 60 * 24 * 30)}`;
}

export function clearBrowserToken() {
  if (typeof window === 'undefined') {
    return;
  }
  localStorage.removeItem(AUTH_TOKEN_STORAGE_KEY);
  document.cookie = `${AUTH_TOKEN_COOKIE_KEY}=; Path=/; Max-Age=0; SameSite=Lax; Expires=Thu, 01 Jan 1970 00:00:00 GMT`;
}
