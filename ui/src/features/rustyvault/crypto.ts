'use client';

import { deriveArgon2IdHashBytes, probeArgon2BrowserFallback } from './argon2Browser';

import type {
  EncryptedRustyVaultItem,
  EncryptedRustyVaultItemSummary,
  UpsertRustyVaultItemRequest,
  RustyVaultUriIndexInput,
  RustyVaultUriMatchMode,
  RustyVaultWrappedKeyMetadata,
} from './api';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const ARGON2_PARAMS = {
  memory_kib: 65_536,
  iterations: 3,
  parallelism: 4,
} as const;

const EMPTY_SALT = new Uint8Array(0);
const WRAP_INFO = encoder.encode('rustyvault-wrap-v1');
const INDEX_INFO = encoder.encode('rustyvault-index-v1');
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

export type RustyVaultLoginItem = {
  item_type: 'login';
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

export type RustyVaultCreditCardItem = {
  item_type: 'credit_card';
  id: string;
  title: string;
  cardholder_name: string;
  card_number: string;
  expiry_month: string;
  expiry_year: string;
  security_code: string;
  issuer_name: string;
  notes: string;
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
};

export type RustyVaultPassportItem = {
  item_type: 'passport';
  id: string;
  title: string;
  full_name: string;
  passport_number: string;
  nationality: string;
  issuing_country: string;
  birth_date: string;
  expiry_date: string;
  notes: string;
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
};

export type RustyVaultSecureNoteItem = {
  item_type: 'secure_note';
  id: string;
  title: string;
  notes: string;
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
};

export type RustyVaultItemType =
  | RustyVaultLoginItem['item_type']
  | RustyVaultCreditCardItem['item_type']
  | RustyVaultPassportItem['item_type']
  | RustyVaultSecureNoteItem['item_type'];

export type RustyVaultItem =
  | RustyVaultLoginItem
  | RustyVaultCreditCardItem
  | RustyVaultPassportItem
  | RustyVaultSecureNoteItem;

export type RustyVaultSummaryPlaintext = {
  item_type: RustyVaultItemType;
  title: string;
  subtitle: string;
  primary_uri: string;
  username: string;
  login_email: string;
  favorite: boolean;
};

export type RustyVaultUnlockedContext = {
  user_id: string;
  key_version: number;
  wrapped_key: RustyVaultWrappedKeyMetadata;
  data_key: CryptoKey;
  wrap_key: CryptoKey;
  index_key: CryptoKey;
};

export type RustyVaultCryptoReadiness = {
  ready: boolean;
  mode: 'native' | 'portable-fallback' | 'unavailable';
  reason:
    | 'ok'
    | 'insecure-context'
    | 'missing-webcrypto'
    | 'missing-subtle'
    | 'argon2-unavailable';
  message: string;
};

let nativeArgon2SupportPromise: Promise<boolean> | null = null;
let portableArgon2SupportPromise: Promise<boolean> | null = null;
const CRYPTO_PROBE_TIMEOUT_MS = 8_000;

async function resolveWithin<T>(promise: Promise<T>, fallback: T, timeoutMs = CRYPTO_PROBE_TIMEOUT_MS) {
  return Promise.race<T>([
    promise,
    new Promise<T>((resolve) => {
      const timeoutId = window.setTimeout(() => resolve(fallback), timeoutMs);
      promise.finally(() => window.clearTimeout(timeoutId)).catch(() => {
        window.clearTimeout(timeoutId);
      });
    }),
  ]);
}

function aadBytes(
  userId: string,
  itemId: string,
  blobKind: 'summary' | 'payload',
  version: number,
) {
  return encoder.encode(`rustyvault:${userId}:${itemId}:${blobKind}:v${version}`);
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

async function browserSupportsNativeArgon2Id(): Promise<boolean> {
  if (!nativeArgon2SupportPromise) {
    nativeArgon2SupportPromise = (async () => {
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
    })();
  }
  return nativeArgon2SupportPromise;
}

async function browserSupportsPortableArgon2(): Promise<boolean> {
  if (!portableArgon2SupportPromise) {
    portableArgon2SupportPromise = probeArgon2BrowserFallback()
      .then(() => true)
      .catch(() => false);
  }
  return portableArgon2SupportPromise;
}

async function deriveMasterMaterial(password: string, salt: Uint8Array): Promise<CryptoKey> {
  let bits: ArrayBuffer | Uint8Array;
  if (await resolveWithin(browserSupportsNativeArgon2Id(), false)) {
    const passwordKey = await importArgon2PasswordKey(password);
    bits = await crypto.subtle.deriveBits(
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
  } else {
    bits = await deriveArgon2IdHashBytes({
      pass: encoder.encode(password),
      salt,
      time: ARGON2_PARAMS.iterations,
      mem: ARGON2_PARAMS.memory_kib,
      parallelism: ARGON2_PARAMS.parallelism,
      hashLen: 32,
    });
  }
  const importBytes = bits instanceof Uint8Array ? toArrayBuffer(bits) : bits;
  return crypto.subtle.importKey('raw', importBytes, 'HKDF', false, ['deriveKey']);
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

function maskVisibleTail(value: string, digits = 4) {
  const trimmed = value.trim();
  if (!trimmed) return '';
  const tail = trimmed.slice(-digits);
  return `•••• ${tail}`;
}

function summaryFromItem(item: RustyVaultItem): RustyVaultSummaryPlaintext {
  switch (item.item_type) {
    case 'credit_card':
      return {
        item_type: item.item_type,
        title: item.title.trim(),
        subtitle: item.cardholder_name.trim() || 'Saved card',
        primary_uri: item.issuer_name.trim() || maskVisibleTail(item.card_number),
        username: maskVisibleTail(item.card_number),
        login_email:
          item.expiry_month.trim() && item.expiry_year.trim()
            ? `Expires ${item.expiry_month.trim()}/${item.expiry_year.trim()}`
            : '',
        favorite: item.favorite,
      };
    case 'passport':
      return {
        item_type: item.item_type,
        title: item.title.trim(),
        subtitle: item.full_name.trim() || 'Saved passport',
        primary_uri: item.issuing_country.trim() || item.nationality.trim(),
        username: maskVisibleTail(item.passport_number),
        login_email: item.expiry_date.trim() ? `Expires ${item.expiry_date.trim()}` : '',
        favorite: item.favorite,
      };
    case 'secure_note':
      return {
        item_type: item.item_type,
        title: item.title.trim(),
        subtitle: 'Secure note',
        primary_uri: '',
        username: '',
        login_email: '',
        favorite: item.favorite,
      };
    case 'login':
    default:
      return {
        item_type: item.item_type,
        title: item.title.trim(),
        subtitle: item.username.trim() || item.login_email.trim() || 'Saved login',
        primary_uri: item.website_urls[0] ?? '',
        username: item.username.trim(),
        login_email: item.login_email.trim(),
        favorite: item.favorite,
      };
  }
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

export async function getRustyVaultCryptoReadiness(): Promise<RustyVaultCryptoReadiness> {
  if (typeof globalThis.crypto === 'undefined') {
    return {
      ready: false,
      mode: 'unavailable',
      reason: 'missing-webcrypto',
      message: 'RustyVault requires browser WebCrypto support.',
    };
  }
  if (!globalThis.crypto.subtle) {
    return {
      ready: false,
      mode: 'unavailable',
      reason: 'missing-subtle',
      message: 'RustyVault requires SubtleCrypto support in this browser.',
    };
  }
  if (typeof window !== 'undefined' && !window.isSecureContext) {
    return {
      ready: false,
      mode: 'unavailable',
      reason: 'insecure-context',
      message:
        'RustyVault requires a trusted HTTPS browser context. Plain HTTP and certificate-warning bypasses are not supported.',
    };
  }
  if (await browserSupportsNativeArgon2Id()) {
    return {
      ready: true,
      mode: 'native',
      reason: 'ok',
      message: 'RustyVault is using native browser Argon2id support.',
    };
  }
  if (await resolveWithin(browserSupportsPortableArgon2(), false)) {
    return {
      ready: true,
      mode: 'portable-fallback',
      reason: 'ok',
      message: 'RustyVault is using a portable Argon2id fallback for this browser.',
    };
  }
  return {
    ready: false,
    mode: 'unavailable',
    reason: 'argon2-unavailable',
    message:
      'This browser does not provide the Argon2id support RustyVault needs, and the portable fallback could not be initialized.',
  };
}

export async function supportsRustyVaultCrypto(): Promise<boolean> {
  return (await getRustyVaultCryptoReadiness()).ready;
}

export async function bootstrapRustyVaultKeys(
  masterPassword: string,
  userId: string,
): Promise<RustyVaultUnlockedContext> {
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
    data_key: vaultKey,
    wrap_key: wrapKey,
    index_key: indexKey,
  };
}

export async function unlockRustyVault(
  masterPassword: string,
  userId: string,
  wrappedKey: RustyVaultWrappedKeyMetadata,
): Promise<RustyVaultUnlockedContext> {
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
    data_key: vaultKey,
    wrap_key: wrapKey,
    index_key: indexKey,
  };
}

export async function rewrapRustyVaultKey(
  unlocked: RustyVaultUnlockedContext,
  newMasterPassword: string,
  nextKeyVersion: number,
): Promise<RustyVaultUnlockedContext> {
  const salt = randomBytes(16);
  const masterMaterial = await deriveMasterMaterial(newMasterPassword, salt);
  const wrapKey = await deriveWrapKey(masterMaterial);
  const indexKey = await deriveIndexKey(masterMaterial);
  const wrapNonce = randomBytes(12);
  const wrappedKeyBuffer = await crypto.subtle.wrapKey(
    'raw',
    unlocked.data_key,
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
    data_key: unlocked.data_key,
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

function uriTokensForUrl(url: URL, matchMode: RustyVaultUriMatchMode): Array<{ token: string; match_type: RustyVaultUriMatchMode; rank: number }> {
  if (matchMode === 'never') {
    return [];
  }
  const exact = `${url.protocol}//${sanitizeHost(url.host)}${url.pathname || '/'}`;
  const host = sanitizeHost(url.host);
  const baseDomain = toBaseDomain(url.hostname);
  const tokens: Array<{ token: string; match_type: RustyVaultUriMatchMode; rank: number }> = [
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
  matchMode: RustyVaultUriMatchMode,
): Promise<RustyVaultUriIndexInput[]> {
  const seen = new Set<string>();
  const outputs: RustyVaultUriIndexInput[] = [];
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
  matchMode: RustyVaultUriMatchMode,
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

export async function encryptRustyVaultItem(
  unlocked: RustyVaultUnlockedContext,
  item: RustyVaultItem,
  matchMode: RustyVaultUriMatchMode,
): Promise<UpsertRustyVaultItemRequest> {
  const summary = summaryFromItem(item);
  const encryptedSummary = await encryptBlob(
    unlocked.data_key,
    unlocked.user_id,
    item.id,
    'summary',
    SUMMARY_VERSION,
    summary,
  );
  const encryptedPayload = await encryptBlob(
    unlocked.data_key,
    unlocked.user_id,
    item.id,
    'payload',
    PAYLOAD_VERSION,
    item,
  );
  const uriIndexes =
    item.item_type === 'login'
      ? await buildUriIndexes(unlocked.index_key, item.website_urls, matchMode)
      : [];

  return {
    id: item.id,
    item_type: item.item_type,
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

export async function encryptRustyVaultLoginItem(
  unlocked: RustyVaultUnlockedContext,
  item: RustyVaultLoginItem,
  matchMode: RustyVaultUriMatchMode,
): Promise<UpsertRustyVaultItemRequest> {
  return encryptRustyVaultItem(unlocked, item, matchMode);
}

export async function decryptRustyVaultSummary(
  unlocked: RustyVaultUnlockedContext,
  item: EncryptedRustyVaultItemSummary,
): Promise<RustyVaultSummaryPlaintext> {
  const summary = await decryptBlob<RustyVaultSummaryPlaintext>(
    unlocked.data_key,
    unlocked.user_id,
    item.id,
    'summary',
    item.summary_version,
    item.summary_nonce_hex,
    item.summary_ciphertext_hex,
  );
  return {
    ...summary,
    item_type: (summary.item_type as RustyVaultItemType | undefined) ?? (item.item_type as RustyVaultItemType),
  };
}

export async function decryptRustyVaultItem(
  unlocked: RustyVaultUnlockedContext,
  item: EncryptedRustyVaultItem,
): Promise<RustyVaultItem> {
  const payload = await decryptBlob<RustyVaultItem>(
    unlocked.data_key,
    unlocked.user_id,
    item.id,
    'payload',
    item.payload_version,
    item.payload_nonce_hex,
    item.payload_ciphertext_hex,
  );
  return {
    ...payload,
    item_type: (payload.item_type as RustyVaultItemType | undefined) ?? (item.item_type as RustyVaultItemType),
  } as RustyVaultItem;
}
