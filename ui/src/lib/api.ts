const API_BASE = '/api/v1';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

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

export async function parseResponseBody(res: Response): Promise<unknown> {
  if (res.status === 204 || res.status === 205 || res.status === 304) {
    return undefined;
  }

  const raw = await res.text();
  const trimmed = raw.trim();
  if (!trimmed) {
    return undefined;
  }

  const contentType = res.headers.get('content-type') || '';
  const looksJson =
    contentType.includes('application/json') ||
    contentType.includes('+json') ||
    trimmed.startsWith('{') ||
    trimmed.startsWith('[');

  if (!looksJson) {
    return trimmed;
  }

  try {
    return JSON.parse(trimmed);
  } catch {
    return trimmed;
  }
}

export function extractErrorMessage(body: unknown, fallback: string): string {
  if (isRecord(body)) {
    const errorValue = body.error;
    if (isRecord(errorValue)) {
      const nestedMessage = errorValue.message;
      if (typeof nestedMessage === 'string' && nestedMessage.trim().length > 0) {
        return nestedMessage;
      }
    }

    const directMessage = body.message;
    if (typeof directMessage === 'string' && directMessage.trim().length > 0) {
      return directMessage;
    }
  }

  if (typeof body === 'string' && body.trim().length > 0) {
    return body;
  }

  return fallback;
}

export async function apiJson<T = unknown>(path: string, options: RequestInit = {}): Promise<T> {
  const res = await apiFetch(path, options);
  const body = await parseResponseBody(res);

  if (!res.ok) {
    throw new Error(extractErrorMessage(body, `API error: ${res.status}`));
  }

  return body as T;
}
