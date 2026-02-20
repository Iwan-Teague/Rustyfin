/**
 * Setup-specific API client.
 * Does NOT auto-redirect to /login on 401 (setup doesn't use JWT).
 * Injects X-Setup-Owner-Token from sessionStorage.
 */

const API_BASE = '/api/v1';

export interface SetupError {
  code: string;
  message: string;
  details: Record<string, unknown>;
}

export interface ErrorEnvelope {
  error: SetupError;
}

/** Fetch public system info (unauthenticated). */
export async function getPublicSystemInfo(): Promise<{
  server_name: string;
  version: string;
  setup_completed: boolean;
  setup_state: string;
}> {
  const res = await fetch(`${API_BASE}/system/info/public`);
  if (!res.ok) throw new Error(`Failed to fetch system info: ${res.status}`);
  return res.json();
}

/** Get the stored owner token from sessionStorage. */
function getOwnerToken(): string | null {
  if (typeof window === 'undefined') return null;
  return sessionStorage.getItem('setup_owner_token');
}

/** Store owner token in sessionStorage. */
export function setOwnerToken(token: string): void {
  if (typeof window !== 'undefined') {
    sessionStorage.setItem('setup_owner_token', token);
  }
}

/** Clear owner token. */
export function clearOwnerToken(): void {
  if (typeof window !== 'undefined') {
    sessionStorage.removeItem('setup_owner_token');
  }
}

/** Setup-specific fetch that injects X-Setup-Owner-Token. */
async function setupFetch(
  path: string,
  options: RequestInit = {},
  extraHeaders: Record<string, string> = {}
): Promise<Response> {
  const token = getOwnerToken();
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...(token ? { 'X-Setup-Owner-Token': token } : {}),
    ...extraHeaders,
    ...((options.headers as Record<string, string>) || {}),
  };

  return fetch(`${API_BASE}${path}`, { ...options, headers });
}

/** Setup-specific JSON fetch. Throws with structured error on failure. */
async function setupJson<T = unknown>(
  path: string,
  options: RequestInit = {},
  extraHeaders: Record<string, string> = {}
): Promise<T> {
  const res = await setupFetch(path, options, extraHeaders);
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: { code: 'unknown', message: `HTTP ${res.status}`, details: {} } }));
    const err = body as ErrorEnvelope;
    throw err.error || { code: 'unknown', message: `HTTP ${res.status}`, details: {} };
  }
  return res.json();
}

// --- Session ---

export async function claimSession(clientName: string, force = false, confirmTakeover = false) {
  return setupJson<{
    owner_token: string;
    expires_at: string;
    claimed_by: string;
    setup_state: string;
  }>('/setup/session/claim', {
    method: 'POST',
    body: JSON.stringify({ client_name: clientName, force, confirm_takeover: confirmTakeover }),
  });
}

export async function releaseSession() {
  return setupJson<{ released: boolean }>('/setup/session/release', { method: 'POST' });
}

// --- Config ---

export async function getSetupConfig() {
  return setupJson<{
    server_name: string;
    default_ui_locale: string;
    default_region: string;
    default_time_zone?: string | null;
  }>('/setup/config');
}

export async function putSetupConfig(config: {
  server_name: string;
  default_ui_locale: string;
  default_region: string;
  default_time_zone?: string | null;
}) {
  return setupJson<{ ok: boolean; setup_state: string }>('/setup/config', {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

// --- Admin ---

export async function createAdmin(
  username: string,
  password: string,
  idempotencyKey: string
) {
  return setupJson<{ user_id: string; setup_state: string }>(
    '/setup/admin',
    {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    },
    { 'Idempotency-Key': idempotencyKey }
  );
}

// --- Paths ---

export async function validatePath(path: string) {
  return setupJson<{
    path: string;
    exists: boolean;
    readable: boolean;
    writable: boolean;
    hint: string | null;
  }>('/setup/paths/validate', {
    method: 'POST',
    body: JSON.stringify({ path }),
  });
}

// --- Libraries ---

export interface LibrarySpec {
  name: string;
  kind: string;
  paths: string[];
  is_read_only: boolean;
}

export async function createLibraries(libraries: LibrarySpec[]) {
  return setupJson<{
    created: number;
    libraries: { id: string; name: string }[];
    setup_state: string;
  }>('/setup/libraries', {
    method: 'POST',
    body: JSON.stringify({ libraries }),
  });
}

// --- Metadata ---

export async function getSetupMetadata() {
  return setupJson<{
    metadata_language: string;
    metadata_region: string;
  }>('/setup/metadata');
}

export async function putSetupMetadata(metadata: {
  metadata_language: string;
  metadata_region: string;
}) {
  return setupJson<{ ok: boolean; setup_state: string }>('/setup/metadata', {
    method: 'PUT',
    body: JSON.stringify(metadata),
  });
}

// --- Network ---

export async function getSetupNetwork() {
  return setupJson<{
    allow_remote_access: boolean;
    enable_automatic_port_mapping: boolean;
    trusted_proxies: string[];
  }>('/setup/network');
}

export async function putSetupNetwork(network: {
  allow_remote_access: boolean;
  enable_automatic_port_mapping: boolean;
  trusted_proxies: string[];
}) {
  return setupJson<{ ok: boolean; setup_state: string }>('/setup/network', {
    method: 'PUT',
    body: JSON.stringify(network),
  });
}

// --- Complete ---

export async function completeSetup() {
  return setupJson<{ setup_completed: boolean; setup_state: string }>(
    '/setup/complete',
    {
      method: 'POST',
      body: JSON.stringify({ confirm: true }),
    }
  );
}
