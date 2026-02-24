const API_BASE = '/api/v1';

function resolveApiPath(path: string): string {
  if (/^https?:\/\//.test(path)) {
    return path;
  }
  if (path.startsWith(API_BASE)) {
    return path;
  }
  return `${API_BASE}${path}`;
}

export async function apiFetch(path: string, options: RequestInit = {}) {
  const token = typeof window !== 'undefined' ? localStorage.getItem('token') : null;
  const headers = new Headers(options.headers || {});
  if (token && !headers.has('Authorization')) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  const isFormData =
    typeof FormData !== 'undefined' &&
    options.body !== undefined &&
    options.body instanceof FormData;
  if (!isFormData && options.body !== undefined && options.body !== null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const res = await fetch(resolveApiPath(path), { ...options, headers });

  if (res.status === 401) {
    if (typeof window !== 'undefined') {
      localStorage.removeItem('token');
      window.location.href = '/login';
    }
    throw new Error('Unauthorized');
  }

  return res;
}

export async function apiJson<T = unknown>(path: string, options: RequestInit = {}): Promise<T> {
  const res = await apiFetch(path, options);
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body?.error?.message || `API error: ${res.status}`);
  }
  return res.json();
}
