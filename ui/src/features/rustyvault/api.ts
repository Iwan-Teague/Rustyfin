'use client';

import { apiFetch, extractErrorMessage, parseResponseBody } from '@/lib/api';

export const RUSTYVAULT_ACCESS_HEADER = 'x-rustyvault-access';

export type RustyVaultClientKind = 'rustyvault_web' | 'browser_extension';
export type RustyVaultUriMatchMode = 'exact' | 'host' | 'base_domain' | 'never';
export type RustyVaultProtectedActionKind =
  | 'rekey'
  | 'export'
  | 'import_overwrite'
  | 'destroy_rustyvault'
  | 'approve_device'
  | 'revoke_other_sessions';

export type RustyVaultWrappedKeyMetadata = {
  key_version: number;
  kdf_algorithm: string;
  kdf_memory_kib: number;
  kdf_iterations: number;
  kdf_parallelism: number;
  kdf_salt_hex: string;
  hkdf_algorithm: string;
  wrap_algorithm: string;
  wrap_nonce_hex: string;
  wrapped_vault_key_hex: string;
  created_ts: number;
};

export type RustyVaultConfigResponse = {
  enabled: boolean;
  schema_version: number;
  supported_kdf_algorithms: string[];
  supported_encryption_algorithms: string[];
  active_wrapped_key?: RustyVaultWrappedKeyMetadata | null;
  item_count: number;
};

export type EncryptedRustyVaultItemSummary = {
  id: string;
  item_type: string;
  key_version: number;
  summary_version: number;
  summary_nonce_hex: string;
  summary_ciphertext_hex: string;
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
  deleted_ts?: number | null;
};

export type EncryptedRustyVaultItem = EncryptedRustyVaultItemSummary & {
  payload_version: number;
  payload_nonce_hex: string;
  payload_ciphertext_hex: string;
};

export type RustyVaultUriIndexInput = {
  match_hash_hex: string;
  match_type: RustyVaultUriMatchMode;
  rank: number;
};

export type UpsertRustyVaultItemRequest = {
  id: string;
  item_type: string;
  key_version: number;
  summary_version: number;
  summary_nonce_hex: string;
  summary_ciphertext_hex: string;
  payload_version: number;
  payload_nonce_hex: string;
  payload_ciphertext_hex: string;
  favorite: boolean;
  revision: number;
  uri_indexes: RustyVaultUriIndexInput[];
};

export type RustyVaultItemListResponse = {
  items: EncryptedRustyVaultItemSummary[];
  next_offset?: number | null;
  total: number;
};

export type RustyVaultLookupResponse = {
  items: EncryptedRustyVaultItemSummary[];
};

export type RustyVaultDeviceSessionTokens = {
  session_id: string;
  access_token: string;
  refresh_token: string;
  access_expires_ts: number;
  refresh_expires_ts: number;
};

export type RustyVaultPairingCodeResponse = {
  pairing_code: string;
  fingerprint_phrase: string;
  expires_ts: number;
};

export type CreateRustyVaultDeviceSessionResponse = {
  session?: RustyVaultDeviceSessionTokens | null;
  pairing?: RustyVaultPairingCodeResponse | null;
};

export type CreateRustyVaultDeviceSessionRequest = {
  client_kind: RustyVaultClientKind;
  device_name: string;
  device_platform?: string | null;
  protected_action_token?: string | null;
};

export type RustyVaultDeviceSessionResponse = {
  id: string;
  client_kind: RustyVaultClientKind;
  device_name: string;
  device_platform?: string | null;
  created_ts: number;
  last_used_ts: number;
  expires_ts: number;
  revoked_ts?: number | null;
  current: boolean;
};

export type RustyVaultProtectedActionChallengeResponse = {
  action_token: string;
  action_kind: RustyVaultProtectedActionKind;
  expires_ts: number;
};

export type RustyVaultAuditEventResponse = {
  id: string;
  event_kind: string;
  target_item_id?: string | null;
  created_ts: number;
  event_json: Record<string, unknown>;
};

export type RustyVaultAuditListResponse = {
  events: RustyVaultAuditEventResponse[];
};

export type RustyVaultExportResponse = {
  config: RustyVaultConfigResponse;
  items: EncryptedRustyVaultItem[];
};

export type RustyVaultImportBitwardenResponse = {
  imported_count: number;
  cleared_existing: boolean;
};

export type RustyVaultRevokeOtherSessionsResponse = {
  revoked_count: number;
};

type RustyVaultJsonOptions = RequestInit & {
  vaultAccessToken?: string | null;
};

async function rustyVaultJson<T>(path: string, options: RustyVaultJsonOptions = {}): Promise<T> {
  const headers = new Headers(options.headers || {});
  if (options.vaultAccessToken && !headers.has(RUSTYVAULT_ACCESS_HEADER)) {
    headers.set(RUSTYVAULT_ACCESS_HEADER, options.vaultAccessToken);
  }
  const res = await apiFetch(path, { ...options, headers });
  const body = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(body, `Vault API error: ${res.status}`));
  }
  return body as T;
}

export async function getRustyVaultConfig(
  vaultAccessToken?: string | null,
): Promise<RustyVaultConfigResponse> {
  return rustyVaultJson<RustyVaultConfigResponse>('/vault/config', {
    vaultAccessToken,
  });
}

export async function bootstrapRustyVault(payload: {
  wrapped_key: RustyVaultWrappedKeyMetadata;
}, vaultAccessToken?: string | null): Promise<RustyVaultConfigResponse> {
  return rustyVaultJson<RustyVaultConfigResponse>('/vault/bootstrap', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function rekeyRustyVault(
  vaultAccessToken: string,
  protectedActionToken: string,
  payload: { wrapped_key: RustyVaultWrappedKeyMetadata },
): Promise<RustyVaultConfigResponse> {
  return rustyVaultJson<RustyVaultConfigResponse>('/vault/rekey', {
    method: 'POST',
    vaultAccessToken,
    headers: {
      'x-rustyvault-protected-action': protectedActionToken,
    },
    body: JSON.stringify(payload),
  });
}

export async function listRustyVaultItems(
  vaultAccessToken: string,
  query: { limit?: number; offset?: number } = {},
): Promise<RustyVaultItemListResponse> {
  const params = new URLSearchParams();
  if (typeof query.limit === 'number') params.set('limit', String(query.limit));
  if (typeof query.offset === 'number') params.set('offset', String(query.offset));
  const suffix = params.toString() ? `?${params.toString()}` : '';
  return rustyVaultJson<RustyVaultItemListResponse>(`/vault/items${suffix}`, {
    vaultAccessToken,
  });
}

export async function getRustyVaultItem(
  vaultAccessToken: string,
  itemId: string,
): Promise<EncryptedRustyVaultItem> {
  return rustyVaultJson<EncryptedRustyVaultItem>(`/vault/items/${encodeURIComponent(itemId)}`, {
    vaultAccessToken,
  });
}

export async function createRustyVaultItem(
  vaultAccessToken: string,
  payload: UpsertRustyVaultItemRequest,
): Promise<void> {
  await rustyVaultJson('/vault/items', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function replaceRustyVaultItem(
  vaultAccessToken: string,
  itemId: string,
  payload: UpsertRustyVaultItemRequest,
): Promise<void> {
  await rustyVaultJson(`/vault/items/${encodeURIComponent(itemId)}`, {
    method: 'PUT',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function deleteRustyVaultItem(
  vaultAccessToken: string,
  itemId: string,
): Promise<void> {
  await rustyVaultJson(`/vault/items/${encodeURIComponent(itemId)}`, {
    method: 'DELETE',
    vaultAccessToken,
  });
}

export async function lookupRustyVaultItems(
  vaultAccessToken: string,
  matchHashesHex: string[],
): Promise<RustyVaultLookupResponse> {
  return rustyVaultJson<RustyVaultLookupResponse>('/vault/lookup', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ match_hashes_hex: matchHashesHex }),
  });
}

export async function createRustyVaultDeviceSession(
  payload: CreateRustyVaultDeviceSessionRequest,
  vaultAccessToken?: string | null,
): Promise<CreateRustyVaultDeviceSessionResponse> {
  return rustyVaultJson<CreateRustyVaultDeviceSessionResponse>('/vault/device-sessions/pair', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function refreshRustyVaultDeviceSession(
  refreshToken: string,
): Promise<RustyVaultDeviceSessionTokens> {
  return rustyVaultJson<RustyVaultDeviceSessionTokens>('/vault/device-sessions/refresh', {
    method: 'POST',
    body: JSON.stringify({ refresh_token: refreshToken }),
  });
}

export async function listRustyVaultDeviceSessions(
  vaultAccessToken?: string | null,
): Promise<RustyVaultDeviceSessionResponse[]> {
  return rustyVaultJson<RustyVaultDeviceSessionResponse[]>('/vault/device-sessions', {
    vaultAccessToken,
  });
}

export async function revokeRustyVaultDeviceSession(
  sessionId: string,
  vaultAccessToken?: string | null,
): Promise<void> {
  await rustyVaultJson(`/vault/device-sessions/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE',
    vaultAccessToken,
  });
}

export async function revokeOtherRustyVaultSessions(
  protectedActionToken: string,
  vaultAccessToken?: string | null,
): Promise<RustyVaultRevokeOtherSessionsResponse> {
  return rustyVaultJson<RustyVaultRevokeOtherSessionsResponse>('/vault/device-sessions/revoke-others', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}

export async function challengeRustyVaultProtectedAction(payload: {
  action_kind: RustyVaultProtectedActionKind;
  current_password: string;
  target_item_id?: string | null;
  vaultAccessToken?: string | null;
}): Promise<RustyVaultProtectedActionChallengeResponse> {
  const { vaultAccessToken, ...body } = payload;
  return rustyVaultJson<RustyVaultProtectedActionChallengeResponse>('/vault/protected-actions/challenge', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(body),
  });
}

export async function listRustyVaultAuditEvents(
  vaultAccessToken?: string | null,
): Promise<RustyVaultAuditListResponse> {
  return rustyVaultJson<RustyVaultAuditListResponse>('/vault/audit', {
    vaultAccessToken,
  });
}

export async function exportRustyVault(
  vaultAccessToken: string,
  protectedActionToken: string,
): Promise<RustyVaultExportResponse> {
  return rustyVaultJson<RustyVaultExportResponse>('/vault/export', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}

export async function importRustyVaultBitwardenCiphertexts(
  vaultAccessToken: string,
  payload: {
    protected_action_token: string;
    clear_existing: boolean;
    items: UpsertRustyVaultItemRequest[];
  },
): Promise<RustyVaultImportBitwardenResponse> {
  return rustyVaultJson<RustyVaultImportBitwardenResponse>('/vault/import/bitwarden', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function destroyRustyVault(
  vaultAccessToken: string,
  protectedActionToken: string,
): Promise<{ destroyed: boolean }> {
  return rustyVaultJson<{ destroyed: boolean }>('/vault', {
    method: 'DELETE',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}
