'use client';

import type {
  EncryptedVaultItem,
  EncryptedVaultItemSummary,
  UpsertVaultItemRequest,
  VaultUriIndexInput,
  VaultUriMatchMode,
  VaultWrappedKeyMetadata,
} from './vaultApi';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const ARGON2_PARAMS = {
  memory_kib: 65_536,
  iterations: 3,
  parallelism: 4,
} as const;

const EMPTY_SALT = new Uint8Array(0);
const WRAP_INFO = encoder.encode('rustfin-vault-wrap-v1');
const INDEX_INFO = encoder.encode('rustfin-vault-index-v1');
const SUMMARY_VERSION = 1;
const PAYLOAD_VERSION = 1;
const MULTIPART_SUFFIXES = new Set([
  'co.uk',
  'org.uk',
  'gov.uk',
  'ac.uk',
  'com.au',
  'net.au',
  'org.au',
  'co.nz',
  'com.br',
  'com.mx',
  'co.jp',
  'co.kr',
  'co.za',
]);

export type VaultLoginItem = {
  id: string;
  title: string;
  username: string;
  login_email: string;
  password: string;
  notes: string;
  website_urls: string[];
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
};

export type VaultSummaryPlaintext = {
  title: string;
  subtitle: string;
  primary_uri: string;
  username: string;
  login_email: string;
  favorite: boolean;
};

export type VaultUnlockedContext = {
  user_id: string;
  key_version: number;
  wrapped_key: VaultWrappedKeyMetadata;
  vault_key: CryptoKey;
  wrap_key: CryptoKey;
  index_key: CryptoKey;
};

function aadBytes(
  userId: string,
  itemId: string,
  blobKind: 'summary' | 'payload',
  version: number,
) {
  return encoder.encode(`rustfin-vault:${userId}:${itemId}:${blobKind}:v${version}`);
}

function randomBytes(length: number) {
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return bytes;
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

export function toHex(bytes: Uint8Array): string {
  return [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('');
}

export function fromHex(value: string): Uint8Array {
  const normalized = value.trim();
  if (normalized.length % 2 !== 0) {
    throw new Error('Invalid hex length');
  }
  const bytes = new Uint8Array(normalized.length / 2);
  for (let idx = 0; idx < normalized.length; idx += 2) {
    bytes[idx / 2] = Number.parseInt(normalized.slice(idx, idx + 2), 16);
  }
  return bytes;
}

async function importArgon2PasswordKey(password: string): Promise<CryptoKey> {
  return (crypto.subtle.importKey as unknown as (
    format: string,
    keyData: BufferSource,
    algorithm: AlgorithmIdentifier,
    extractable: boolean,
    keyUsages: KeyUsage[],
  ) => Promise<CryptoKey>)('raw-secret', encoder.encode(password), 'Argon2id', false, [
    'deriveBits',
  ]);
}

async function deriveMasterMaterial(password: string, salt: Uint8Array): Promise<CryptoKey> {
  const passwordKey = await importArgon2PasswordKey(password);
  const bits = await crypto.subtle.deriveBits(
    {
      name: 'Argon2id',
      nonce: toArrayBuffer(salt),
      parallelism: ARGON2_PARAMS.parallelism,
      memory: ARGON2_PARAMS.memory_kib,
      passes: ARGON2_PARAMS.iterations,
    } as AlgorithmIdentifier,
    passwordKey,
    256,
  );
  return crypto.subtle.importKey('raw', bits, 'HKDF', false, ['deriveKey']);
}

async function deriveWrapKey(masterMaterial: CryptoKey): Promise<CryptoKey> {
  return crypto.subtle.deriveKey(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: EMPTY_SALT,
      info: WRAP_INFO,
    },
    masterMaterial,
    { name: 'AES-GCM', length: 256 },
    true,
    ['wrapKey', 'unwrapKey'],
  );
}

async function deriveIndexKey(masterMaterial: CryptoKey): Promise<CryptoKey> {
  return crypto.subtle.deriveKey(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: EMPTY_SALT,
      info: INDEX_INFO,
    },
    masterMaterial,
    { name: 'HMAC', hash: 'SHA-256', length: 256 },
    false,
    ['sign'],
  );
}

function summaryFromItem(item: VaultLoginItem): VaultSummaryPlaintext {
  return {
    title: item.title.trim(),
    subtitle: item.username.trim() || item.login_email.trim() || 'Saved login',
    primary_uri: item.website_urls[0] ?? '',
    username: item.username.trim(),
    login_email: item.login_email.trim(),
    favorite: item.favorite,
  };
}

async function encryptBlob<T>(
  vaultKey: CryptoKey,
  userId: string,
  itemId: string,
  blobKind: 'summary' | 'payload',
  version: number,
  value: T,
): Promise<{ nonce_hex: string; ciphertext_hex: string }> {
  const nonce = randomBytes(12);
  const ciphertext = await crypto.subtle.encrypt(
    {
      name: 'AES-GCM',
      iv: nonce,
      additionalData: aadBytes(userId, itemId, blobKind, version),
    },
    vaultKey,
    encoder.encode(JSON.stringify(value)),
  );
  return {
    nonce_hex: toHex(nonce),
    ciphertext_hex: toHex(new Uint8Array(ciphertext)),
  };
}

async function decryptBlob<T>(
  vaultKey: CryptoKey,
  userId: string,
  itemId: string,
  blobKind: 'summary' | 'payload',
  version: number,
  nonceHex: string,
  ciphertextHex: string,
): Promise<T> {
  const plaintext = await crypto.subtle.decrypt(
    {
      name: 'AES-GCM',
      iv: toArrayBuffer(fromHex(nonceHex)),
      additionalData: aadBytes(userId, itemId, blobKind, version),
    },
    vaultKey,
    toArrayBuffer(fromHex(ciphertextHex)),
  );
  return JSON.parse(decoder.decode(plaintext)) as T;
}

export async function supportsVaultCrypto(): Promise<boolean> {
  try {
    const probe = await importArgon2PasswordKey('probe');
    await crypto.subtle.deriveBits(
      {
        name: 'Argon2id',
        nonce: new Uint8Array(16).buffer,
        parallelism: 1,
        memory: 8_192,
        passes: 1,
      } as AlgorithmIdentifier,
      probe,
      256,
    );
    return true;
  } catch {
    return false;
  }
}

export async function bootstrapVaultKeys(
  masterPassword: string,
  userId: string,
): Promise<VaultUnlockedContext> {
  const salt = randomBytes(16);
  const masterMaterial = await deriveMasterMaterial(masterPassword, salt);
  const wrapKey = await deriveWrapKey(masterMaterial);
  const indexKey = await deriveIndexKey(masterMaterial);
  const vaultKey = await crypto.subtle.generateKey(
    { name: 'AES-GCM', length: 256 },
    true,
    ['encrypt', 'decrypt', 'wrapKey'],
  );
  const wrapNonce = randomBytes(12);
  const wrappedKeyBuffer = await crypto.subtle.wrapKey(
    'raw',
    vaultKey,
    wrapKey,
    { name: 'AES-GCM', iv: wrapNonce },
  );

  return {
    user_id: userId,
    key_version: 1,
    wrapped_key: {
      key_version: 1,
      kdf_algorithm: 'argon2id',
      kdf_memory_kib: ARGON2_PARAMS.memory_kib,
      kdf_iterations: ARGON2_PARAMS.iterations,
      kdf_parallelism: ARGON2_PARAMS.parallelism,
      kdf_salt_hex: toHex(salt),
      hkdf_algorithm: 'hkdf-sha-256',
      wrap_algorithm: 'aes-256-gcm',
      wrap_nonce_hex: toHex(wrapNonce),
      wrapped_vault_key_hex: toHex(new Uint8Array(wrappedKeyBuffer)),
      created_ts: Math.floor(Date.now() / 1000),
    },
    vault_key: vaultKey,
    wrap_key: wrapKey,
    index_key: indexKey,
  };
}

export async function unlockVault(
  masterPassword: string,
  userId: string,
  wrappedKey: VaultWrappedKeyMetadata,
): Promise<VaultUnlockedContext> {
  if (
    wrappedKey.kdf_algorithm !== 'argon2id' ||
    wrappedKey.hkdf_algorithm !== 'hkdf-sha-256' ||
    wrappedKey.wrap_algorithm !== 'aes-256-gcm'
  ) {
    throw new Error('Vault key metadata uses an unsupported algorithm');
  }
  const masterMaterial = await deriveMasterMaterial(masterPassword, fromHex(wrappedKey.kdf_salt_hex));
  const wrapKey = await deriveWrapKey(masterMaterial);
  const indexKey = await deriveIndexKey(masterMaterial);
  const vaultKey = await crypto.subtle.unwrapKey(
    'raw',
    toArrayBuffer(fromHex(wrappedKey.wrapped_vault_key_hex)),
    wrapKey,
    {
      name: 'AES-GCM',
      iv: toArrayBuffer(fromHex(wrappedKey.wrap_nonce_hex)),
    },
    { name: 'AES-GCM', length: 256 },
    true,
    ['encrypt', 'decrypt', 'wrapKey'],
  );
  return {
    user_id: userId,
    key_version: wrappedKey.key_version,
    wrapped_key: wrappedKey,
    vault_key: vaultKey,
    wrap_key: wrapKey,
    index_key: indexKey,
  };
}

export async function rewrapVaultKey(
  unlocked: VaultUnlockedContext,
  newMasterPassword: string,
  nextKeyVersion: number,
): Promise<VaultUnlockedContext> {
  const salt = randomBytes(16);
  const masterMaterial = await deriveMasterMaterial(newMasterPassword, salt);
  const wrapKey = await deriveWrapKey(masterMaterial);
  const indexKey = await deriveIndexKey(masterMaterial);
  const wrapNonce = randomBytes(12);
  const wrappedKeyBuffer = await crypto.subtle.wrapKey(
    'raw',
    unlocked.vault_key,
    wrapKey,
    { name: 'AES-GCM', iv: wrapNonce },
  );
  return {
    user_id: unlocked.user_id,
    key_version: nextKeyVersion,
    wrapped_key: {
      key_version: nextKeyVersion,
      kdf_algorithm: 'argon2id',
      kdf_memory_kib: ARGON2_PARAMS.memory_kib,
      kdf_iterations: ARGON2_PARAMS.iterations,
      kdf_parallelism: ARGON2_PARAMS.parallelism,
      kdf_salt_hex: toHex(salt),
      hkdf_algorithm: 'hkdf-sha-256',
      wrap_algorithm: 'aes-256-gcm',
      wrap_nonce_hex: toHex(wrapNonce),
      wrapped_vault_key_hex: toHex(new Uint8Array(wrappedKeyBuffer)),
      created_ts: Math.floor(Date.now() / 1000),
    },
    vault_key: unlocked.vault_key,
    wrap_key: wrapKey,
    index_key: indexKey,
  };
}

function sanitizeHost(host: string): string {
  return host.trim().toLowerCase().replace(/\.+$/, '');
}

function isIpv4Host(host: string) {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host);
}

function isIpv6Host(host: string) {
  return host.includes(':');
}

function toBaseDomain(host: string): string | null {
  const clean = sanitizeHost(host);
  if (!clean || clean === 'localhost' || isIpv4Host(clean) || isIpv6Host(clean)) {
    return null;
  }
  const labels = clean.split('.').filter(Boolean);
  if (labels.length <= 2) {
    return clean;
  }
  const tail = labels.slice(-2).join('.');
  const multiTail = labels.slice(-3).join('.');
  if (MULTIPART_SUFFIXES.has(tail)) {
    return multiTail;
  }
  return tail;
}

export function normalizeWebsiteUrl(raw: string): URL | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const candidate = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
  try {
    const url = new URL(candidate);
    if (!['https:', 'http:'].includes(url.protocol)) {
      return null;
    }
    return url;
  } catch {
    return null;
  }
}

function uriTokensForUrl(url: URL, matchMode: VaultUriMatchMode): Array<{ token: string; match_type: VaultUriMatchMode; rank: number }> {
  if (matchMode === 'never') {
    return [];
  }
  const exact = `${url.protocol}//${sanitizeHost(url.host)}${url.pathname || '/'}`;
  const host = sanitizeHost(url.host);
  const baseDomain = toBaseDomain(url.hostname);
  const tokens: Array<{ token: string; match_type: VaultUriMatchMode; rank: number }> = [
    { token: `exact:${exact}`, match_type: 'exact', rank: 0 },
  ];
  if (matchMode === 'host' || matchMode === 'base_domain') {
    tokens.push({ token: `host:${host}`, match_type: 'host', rank: 1 });
  }
  if (matchMode === 'base_domain' && baseDomain) {
    tokens.push({ token: `base_domain:${baseDomain}`, match_type: 'base_domain', rank: 2 });
  }
  return tokens;
}

async function blindedIndexHex(indexKey: CryptoKey, token: string): Promise<string> {
  const signature = await crypto.subtle.sign('HMAC', indexKey, encoder.encode(token));
  return toHex(new Uint8Array(signature));
}

export async function buildUriIndexes(
  indexKey: CryptoKey,
  urls: string[],
  matchMode: VaultUriMatchMode,
): Promise<VaultUriIndexInput[]> {
  const seen = new Set<string>();
  const outputs: VaultUriIndexInput[] = [];
  for (const raw of urls) {
    const normalized = normalizeWebsiteUrl(raw);
    if (!normalized) continue;
    for (const token of uriTokensForUrl(normalized, matchMode)) {
      if (seen.has(token.token)) continue;
      seen.add(token.token);
      outputs.push({
        match_hash_hex: await blindedIndexHex(indexKey, token.token),
        match_type: token.match_type,
        rank: token.rank,
      });
    }
  }
  outputs.sort((left, right) => left.rank - right.rank);
  return outputs;
}

export async function buildLookupHashesForUrl(
  indexKey: CryptoKey,
  rawUrl: string,
  matchMode: VaultUriMatchMode,
): Promise<string[]> {
  const normalized = normalizeWebsiteUrl(rawUrl);
  if (!normalized) {
    return [];
  }
  const hashes: string[] = [];
  for (const token of uriTokensForUrl(normalized, matchMode)) {
    hashes.push(await blindedIndexHex(indexKey, token.token));
  }
  return hashes;
}

export async function encryptVaultLoginItem(
  unlocked: VaultUnlockedContext,
  item: VaultLoginItem,
  matchMode: VaultUriMatchMode,
): Promise<UpsertVaultItemRequest> {
  const summary = summaryFromItem(item);
  const encryptedSummary = await encryptBlob(
    unlocked.vault_key,
    unlocked.user_id,
    item.id,
    'summary',
    SUMMARY_VERSION,
    summary,
  );
  const encryptedPayload = await encryptBlob(
    unlocked.vault_key,
    unlocked.user_id,
    item.id,
    'payload',
    PAYLOAD_VERSION,
    item,
  );
  const uriIndexes = await buildUriIndexes(unlocked.index_key, item.website_urls, matchMode);

  return {
    id: item.id,
    item_type: 'login',
    key_version: unlocked.key_version,
    summary_version: SUMMARY_VERSION,
    summary_nonce_hex: encryptedSummary.nonce_hex,
    summary_ciphertext_hex: encryptedSummary.ciphertext_hex,
    payload_version: PAYLOAD_VERSION,
    payload_nonce_hex: encryptedPayload.nonce_hex,
    payload_ciphertext_hex: encryptedPayload.ciphertext_hex,
    favorite: item.favorite,
    revision: item.revision,
    uri_indexes: uriIndexes,
  };
}

export async function decryptVaultSummary(
  unlocked: VaultUnlockedContext,
  item: EncryptedVaultItemSummary,
): Promise<VaultSummaryPlaintext> {
  return decryptBlob<VaultSummaryPlaintext>(
    unlocked.vault_key,
    unlocked.user_id,
    item.id,
    'summary',
    item.summary_version,
    item.summary_nonce_hex,
    item.summary_ciphertext_hex,
  );
}

export async function decryptVaultItem(
  unlocked: VaultUnlockedContext,
  item: EncryptedVaultItem,
): Promise<VaultLoginItem> {
  return decryptBlob<VaultLoginItem>(
    unlocked.vault_key,
    unlocked.user_id,
    item.id,
    'payload',
    item.payload_version,
    item.payload_nonce_hex,
    item.payload_ciphertext_hex,
  );
}
