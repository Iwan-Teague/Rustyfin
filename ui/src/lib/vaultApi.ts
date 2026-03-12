'use client';

import { apiFetch, extractErrorMessage, parseResponseBody } from './api';

export const VAULT_ACCESS_HEADER = 'x-rustfin-vault-access';

export type VaultClientKind = 'web_vault' | 'browser_extension';
export type VaultUriMatchMode = 'exact' | 'host' | 'base_domain' | 'never';
export type VaultProtectedActionKind =
  | 'rekey'
  | 'export'
  | 'import_overwrite'
  | 'destroy_vault'
  | 'approve_device'
  | 'revoke_other_sessions';

export type VaultWrappedKeyMetadata = {
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

export type VaultConfigResponse = {
  enabled: boolean;
  schema_version: number;
  supported_kdf_algorithms: string[];
  supported_encryption_algorithms: string[];
  active_wrapped_key?: VaultWrappedKeyMetadata | null;
  item_count: number;
};

export type EncryptedVaultItemSummary = {
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

export type EncryptedVaultItem = EncryptedVaultItemSummary & {
  payload_version: number;
  payload_nonce_hex: string;
  payload_ciphertext_hex: string;
};

export type VaultUriIndexInput = {
  match_hash_hex: string;
  match_type: VaultUriMatchMode;
  rank: number;
};

export type UpsertVaultItemRequest = {
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
  uri_indexes: VaultUriIndexInput[];
};

export type VaultItemListResponse = {
  items: EncryptedVaultItemSummary[];
  next_offset?: number | null;
  total: number;
};

export type VaultLookupResponse = {
  items: EncryptedVaultItemSummary[];
};

export type VaultSyncResponse = {
  cursor: number;
  items: EncryptedVaultItemSummary[];
};

export type VaultDeviceSessionTokens = {
  session_id: string;
  access_token: string;
  refresh_token: string;
  access_expires_ts: number;
  refresh_expires_ts: number;
};

export type VaultPairingCodeResponse = {
  pairing_code: string;
  fingerprint_phrase: string;
  expires_ts: number;
};

export type CreateVaultDeviceSessionResponse = {
  session?: VaultDeviceSessionTokens | null;
  pairing?: VaultPairingCodeResponse | null;
};

export type CreateVaultDeviceSessionRequest = {
  client_kind: VaultClientKind;
  device_name: string;
  device_platform?: string | null;
  protected_action_token?: string | null;
};

export type VaultDeviceSessionResponse = {
  id: string;
  client_kind: VaultClientKind;
  device_name: string;
  device_platform?: string | null;
  created_ts: number;
  last_used_ts: number;
  expires_ts: number;
  revoked_ts?: number | null;
  current: boolean;
};

export type VaultProtectedActionChallengeResponse = {
  action_token: string;
  action_kind: VaultProtectedActionKind;
  expires_ts: number;
};

export type VaultAuditEventResponse = {
  id: string;
  event_kind: string;
  target_item_id?: string | null;
  created_ts: number;
  event_json: Record<string, unknown>;
};

export type VaultAuditListResponse = {
  events: VaultAuditEventResponse[];
};

export type VaultExportResponse = {
  config: VaultConfigResponse;
  items: EncryptedVaultItem[];
};

export type VaultImportBitwardenResponse = {
  imported_count: number;
  cleared_existing: boolean;
};

export type VaultRevokeOtherSessionsResponse = {
  revoked_count: number;
};

type VaultJsonOptions = RequestInit & {
  vaultAccessToken?: string | null;
};

async function vaultJson<T>(path: string, options: VaultJsonOptions = {}): Promise<T> {
  const headers = new Headers(options.headers || {});
  if (options.vaultAccessToken && !headers.has(VAULT_ACCESS_HEADER)) {
    headers.set(VAULT_ACCESS_HEADER, options.vaultAccessToken);
  }
  const res = await apiFetch(path, { ...options, headers });
  const body = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(body, `Vault API error: ${res.status}`));
  }
  return body as T;
}

export async function getVaultConfig(): Promise<VaultConfigResponse> {
  return vaultJson<VaultConfigResponse>('/vault/config');
}

export async function bootstrapVault(payload: {
  wrapped_key: VaultWrappedKeyMetadata;
}): Promise<VaultConfigResponse> {
  return vaultJson<VaultConfigResponse>('/vault/bootstrap', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function rekeyVault(
  vaultAccessToken: string,
  protectedActionToken: string,
  payload: { wrapped_key: VaultWrappedKeyMetadata },
): Promise<VaultConfigResponse> {
  return vaultJson<VaultConfigResponse>('/vault/rekey', {
    method: 'POST',
    vaultAccessToken,
    headers: {
      'x-rustfin-vault-protected-action': protectedActionToken,
    },
    body: JSON.stringify(payload),
  });
}

export async function listVaultItems(
  vaultAccessToken: string,
  query: { limit?: number; offset?: number } = {},
): Promise<VaultItemListResponse> {
  const params = new URLSearchParams();
  if (typeof query.limit === 'number') params.set('limit', String(query.limit));
  if (typeof query.offset === 'number') params.set('offset', String(query.offset));
  const suffix = params.toString() ? `?${params.toString()}` : '';
  return vaultJson<VaultItemListResponse>(`/vault/items${suffix}`, {
    vaultAccessToken,
  });
}

export async function getVaultItem(
  vaultAccessToken: string,
  itemId: string,
): Promise<EncryptedVaultItem> {
  return vaultJson<EncryptedVaultItem>(`/vault/items/${encodeURIComponent(itemId)}`, {
    vaultAccessToken,
  });
}

export async function createVaultItem(
  vaultAccessToken: string,
  payload: UpsertVaultItemRequest,
): Promise<void> {
  await vaultJson('/vault/items', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function replaceVaultItem(
  vaultAccessToken: string,
  itemId: string,
  payload: UpsertVaultItemRequest,
): Promise<void> {
  await vaultJson(`/vault/items/${encodeURIComponent(itemId)}`, {
    method: 'PUT',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function deleteVaultItem(
  vaultAccessToken: string,
  itemId: string,
): Promise<void> {
  await vaultJson(`/vault/items/${encodeURIComponent(itemId)}`, {
    method: 'DELETE',
    vaultAccessToken,
  });
}

export async function lookupVaultItems(
  vaultAccessToken: string,
  matchHashesHex: string[],
): Promise<VaultLookupResponse> {
  return vaultJson<VaultLookupResponse>('/vault/lookup', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ match_hashes_hex: matchHashesHex }),
  });
}

export async function syncVaultItems(
  vaultAccessToken: string,
  cursor = 0,
): Promise<VaultSyncResponse> {
  return vaultJson<VaultSyncResponse>(`/vault/sync?cursor=${encodeURIComponent(String(cursor))}`, {
    vaultAccessToken,
  });
}

export async function createVaultDeviceSession(
  payload: CreateVaultDeviceSessionRequest,
): Promise<CreateVaultDeviceSessionResponse> {
  return vaultJson<CreateVaultDeviceSessionResponse>('/vault/device-sessions/pair', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function consumeVaultPairingCode(payload: {
  pairing_code: string;
  device_name: string;
  device_platform?: string | null;
}): Promise<VaultDeviceSessionTokens> {
  return vaultJson<VaultDeviceSessionTokens>('/vault/device-sessions/pair/consume', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function refreshVaultDeviceSession(
  refreshToken: string,
): Promise<VaultDeviceSessionTokens> {
  return vaultJson<VaultDeviceSessionTokens>('/vault/device-sessions/refresh', {
    method: 'POST',
    body: JSON.stringify({ refresh_token: refreshToken }),
  });
}

export async function listVaultDeviceSessions(
  vaultAccessToken?: string | null,
): Promise<VaultDeviceSessionResponse[]> {
  return vaultJson<VaultDeviceSessionResponse[]>('/vault/device-sessions', {
    vaultAccessToken,
  });
}

export async function revokeVaultDeviceSession(sessionId: string): Promise<void> {
  await vaultJson(`/vault/device-sessions/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE',
  });
}

export async function revokeOtherVaultSessions(
  protectedActionToken: string,
  vaultAccessToken?: string | null,
): Promise<VaultRevokeOtherSessionsResponse> {
  return vaultJson<VaultRevokeOtherSessionsResponse>('/vault/device-sessions/revoke-others', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}

export async function challengeVaultProtectedAction(payload: {
  action_kind: VaultProtectedActionKind;
  current_password: string;
  target_item_id?: string | null;
  vaultAccessToken?: string | null;
}): Promise<VaultProtectedActionChallengeResponse> {
  const { vaultAccessToken, ...body } = payload;
  return vaultJson<VaultProtectedActionChallengeResponse>('/vault/protected-actions/challenge', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(body),
  });
}

export async function completeVaultProtectedAction(
  payload: {
    action_token: string;
    action_kind: VaultProtectedActionKind;
    target_item_id?: string | null;
  },
  vaultAccessToken?: string | null,
): Promise<{ ok: boolean }> {
  return vaultJson<{ ok: boolean }>('/vault/protected-actions/complete', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function listVaultAuditEvents(): Promise<VaultAuditListResponse> {
  return vaultJson<VaultAuditListResponse>('/vault/audit');
}

export async function exportVault(
  vaultAccessToken: string,
  protectedActionToken: string,
): Promise<VaultExportResponse> {
  return vaultJson<VaultExportResponse>('/vault/export', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}

export async function importBitwardenCiphertexts(
  vaultAccessToken: string,
  payload: {
    protected_action_token: string;
    clear_existing: boolean;
    items: UpsertVaultItemRequest[];
  },
): Promise<VaultImportBitwardenResponse> {
  return vaultJson<VaultImportBitwardenResponse>('/vault/import/bitwarden', {
    method: 'POST',
    vaultAccessToken,
    body: JSON.stringify(payload),
  });
}

export async function destroyVault(
  vaultAccessToken: string,
  protectedActionToken: string,
): Promise<{ destroyed: boolean }> {
  return vaultJson<{ destroyed: boolean }>('/vault', {
    method: 'DELETE',
    vaultAccessToken,
    body: JSON.stringify({ protected_action_token: protectedActionToken }),
  });
}
