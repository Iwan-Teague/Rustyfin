import { ArgonType, hash as argon2BrowserHash, probeArgon2BrowserFallback } from './argon2-browser.js';
import type {
  DecryptedLoginItem,
  EncryptedRustyVaultItem,
  EncryptedRustyVaultSummary,
  RustyVaultPreferences,
} from './types.js';

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const EMPTY_SALT = new Uint8Array(0);
const WRAP_INFO = encoder.encode('rustyvault-wrap-v1');
const INDEX_INFO = encoder.encode('rustyvault-index-v1');
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
const AMBIGUOUS = new Set(['0', 'O', 'o', '1', 'l', 'I']);
const UPPERCASE = 'ABCDEFGHJKLMNPQRSTUVWXYZ';
const LOWERCASE = 'abcdefghijkmnopqrstuvwxyz';
const NUMBERS = '23456789';
const SYMBOLS = '!@#$%^&*()-_=+[]{}:,.?';
const MEMORABLE_WORDS = [
  'amber',
  'anchor',
  'cabin',
  'cedar',
  'cipher',
  'ember',
  'garden',
  'juniper',
  'market',
  'meadow',
  'mint',
  'paper',
  'pepper',
  'raven',
  'river',
  'sunrise',
  'thunder',
  'velvet',
  'whisper',
  'zephyr',
];
const MEMORABLE_NUMBER_SWAP = [
  ['a', '4'],
  ['e', '3'],
  ['i', '1'],
  ['s', '5'],
  ['t', '7'],
] as const;
const MEMORABLE_SYMBOL_SWAP = [
  ['a', '@'],
  ['s', '$'],
  ['x', '*'],
] as const;

let nativeArgon2SupportPromise: Promise<boolean> | null = null;
let portableArgon2SupportPromise: Promise<boolean> | null = null;

const DEFAULT_ARGON2_PARAMS = {
  memory_kib: 65536,
  iterations: 3,
  parallelism: 4,
};

function toArrayBuffer(bytes: Uint8Array) {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

export function toHex(bytes: Uint8Array) {
  return [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('');
}

export function fromHex(value: string) {
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

function aadBytes(userId: string, itemId: string, blobKind: string, version: number) {
  return encoder.encode(`rustyvault:${userId}:${itemId}:${blobKind}:v${version}`);
}

async function importArgon2PasswordKey(password: string) {
  return (crypto.subtle as any).importKey('raw-secret', encoder.encode(password), 'Argon2id', false, [
    'deriveBits',
  ]);
}

async function browserSupportsNativeArgon2Id() {
  if (!nativeArgon2SupportPromise) {
    nativeArgon2SupportPromise = (async () => {
      try {
        const probe = await importArgon2PasswordKey('probe');
        await (crypto.subtle as any).deriveBits(
          {
            name: 'Argon2id',
            nonce: new Uint8Array(16).buffer,
            parallelism: 1,
            memory: 8192,
            passes: 1,
          },
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

async function browserSupportsPortableArgon2() {
  if (!portableArgon2SupportPromise) {
    portableArgon2SupportPromise = probeArgon2BrowserFallback()
      .then(() => true)
      .catch(() => false);
  }
  return portableArgon2SupportPromise;
}

function normalizeArgon2Params(value: any) {
  return {
    memory_kib:
      typeof value?.memory_kib === 'number' && value.memory_kib > 0
        ? Math.floor(value.memory_kib)
        : DEFAULT_ARGON2_PARAMS.memory_kib,
    iterations:
      typeof value?.iterations === 'number' && value.iterations > 0
        ? Math.floor(value.iterations)
        : DEFAULT_ARGON2_PARAMS.iterations,
    parallelism:
      typeof value?.parallelism === 'number' && value.parallelism > 0
        ? Math.floor(value.parallelism)
        : DEFAULT_ARGON2_PARAMS.parallelism,
  };
}

async function deriveMasterMaterial(password: string, salt: Uint8Array, params = DEFAULT_ARGON2_PARAMS) {
  const normalizedParams = normalizeArgon2Params(params);
  let bits: ArrayBuffer;
  if (await browserSupportsNativeArgon2Id()) {
    const passwordKey = await importArgon2PasswordKey(password);
    bits = await (crypto.subtle as any).deriveBits(
      {
        name: 'Argon2id',
        nonce: toArrayBuffer(salt),
        parallelism: normalizedParams.parallelism,
        memory: normalizedParams.memory_kib,
        passes: normalizedParams.iterations,
      },
      passwordKey,
      256,
    );
  } else {
    if (!(await browserSupportsPortableArgon2())) {
      throw new Error('RustyVault Argon2id is unavailable in this browser extension context');
    }
    bits = await argon2BrowserHash({
      pass: encoder.encode(password),
      salt,
      time: normalizedParams.iterations,
      mem: normalizedParams.memory_kib,
      parallelism: normalizedParams.parallelism,
      hashLen: 32,
      type: ArgonType.Argon2id,
    }).then((result) => result.hash.buffer);
  }
  return crypto.subtle.importKey('raw', bits, 'HKDF', false, ['deriveKey']);
}

async function deriveWrapKey(masterMaterial: CryptoKey) {
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

async function deriveIndexKey(masterMaterial: CryptoKey) {
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

export async function unlockRustyVault(masterPassword: string, userId: string, wrappedKey: any) {
  const masterMaterial = await deriveMasterMaterial(masterPassword, fromHex(wrappedKey.kdf_salt_hex), {
    memory_kib: wrappedKey.kdf_memory_kib,
    iterations: wrappedKey.kdf_iterations,
    parallelism: wrappedKey.kdf_parallelism,
  });
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

export async function decryptRustyVaultSummary(unlocked: any, item: EncryptedRustyVaultSummary) {
  const plaintext = await crypto.subtle.decrypt(
    {
      name: 'AES-GCM',
      iv: toArrayBuffer(fromHex(item.summary_nonce_hex)),
      additionalData: aadBytes(unlocked.user_id, item.id, 'summary', item.summary_version),
    },
    unlocked.data_key,
    toArrayBuffer(fromHex(item.summary_ciphertext_hex)),
  );
  return JSON.parse(decoder.decode(plaintext));
}

export async function decryptRustyVaultItem(unlocked: any, item: EncryptedRustyVaultItem) {
  const plaintext = await crypto.subtle.decrypt(
    {
      name: 'AES-GCM',
      iv: toArrayBuffer(fromHex(item.payload_nonce_hex)),
      additionalData: aadBytes(unlocked.user_id, item.id, 'payload', item.payload_version),
    },
    unlocked.data_key,
    toArrayBuffer(fromHex(item.payload_ciphertext_hex)),
  );
  return JSON.parse(decoder.decode(plaintext));
}

export function normalizeWebsiteUrl(raw: string) {
  const trimmed = (raw || '').trim();
  if (!trimmed) return null;
  const candidate = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
  try {
    const url = new URL(candidate);
    if (!['http:', 'https:'].includes(url.protocol)) {
      return null;
    }
    return url;
  } catch {
    return null;
  }
}

function sanitizeHost(host: string) {
  return (host || '').trim().toLowerCase().replace(/\.+$/, '');
}

function isIpv4Host(host: string) {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host);
}

function isIpv6Host(host: string) {
  return host.includes(':');
}

function toBaseDomain(host: string) {
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

function uriTokensForUrl(url: URL, matchMode: string) {
  if (matchMode === 'never') return [];
  const exact = `${url.protocol}//${sanitizeHost(url.host)}${url.pathname || '/'}`;
  const host = sanitizeHost(url.host);
  const baseDomain = toBaseDomain(url.hostname);
  const tokens = [{ token: `exact:${exact}`, match_type: 'exact', rank: 0 }];
  if (matchMode === 'host' || matchMode === 'base_domain') {
    tokens.push({ token: `host:${host}`, match_type: 'host', rank: 1 });
  }
  if (matchMode === 'base_domain' && baseDomain) {
    tokens.push({ token: `base_domain:${baseDomain}`, match_type: 'base_domain', rank: 2 });
  }
  return tokens;
}

async function blindedIndexHex(indexKey: CryptoKey, token: string) {
  const signature = await crypto.subtle.sign('HMAC', indexKey, encoder.encode(token));
  return toHex(new Uint8Array(signature));
}

export async function buildLookupHashesForUrl(indexKey: CryptoKey, rawUrl: string, matchMode: string) {
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

export async function buildUriIndexes(indexKey: CryptoKey, urls: string[], matchMode: string) {
  const outputs: Array<{ match_hash_hex: string; match_type: 'exact' | 'host' | 'base_domain'; rank: number }> = [];
  const seen = new Set();
  for (const raw of urls) {
    const normalized = normalizeWebsiteUrl(raw);
    if (!normalized) continue;
    for (const token of uriTokensForUrl(normalized, matchMode)) {
      if (seen.has(token.token)) continue;
      seen.add(token.token);
      outputs.push({
        match_hash_hex: await blindedIndexHex(indexKey, token.token),
        match_type: token.match_type as 'exact' | 'host' | 'base_domain',
        rank: token.rank,
      });
    }
  }
  outputs.sort((left, right) => left.rank - right.rank);
  return outputs;
}

async function encryptBlob(unlocked: any, itemId: string, blobKind: string, version: number, value: unknown) {
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    {
      name: 'AES-GCM',
      iv: nonce,
      additionalData: aadBytes(unlocked.user_id, itemId, blobKind, version),
    },
    unlocked.data_key,
    encoder.encode(JSON.stringify(value)),
  );
  return {
    nonce_hex: toHex(nonce),
    ciphertext_hex: toHex(new Uint8Array(ciphertext)),
  };
}

export async function encryptRustyVaultLoginItem(unlocked: any, item: DecryptedLoginItem, matchMode: string) {
  const summary = {
    title: (item.title || '').trim(),
    subtitle: (item.username || '').trim() || (item.login_email || '').trim() || 'Saved login',
    primary_uri: item.website_urls?.[0] || '',
    username: (item.username || '').trim(),
    login_email: (item.login_email || '').trim(),
    favorite: Boolean(item.favorite),
  };
  const encryptedSummary = await encryptBlob(unlocked, item.id, 'summary', 1, summary);
  const encryptedPayload = await encryptBlob(unlocked, item.id, 'payload', 1, item);
  const uriIndexes = await buildUriIndexes(unlocked.index_key, item.website_urls || [], matchMode);
  return {
    id: item.id,
    item_type: 'login',
    key_version: unlocked.key_version,
    summary_version: 1,
    summary_nonce_hex: encryptedSummary.nonce_hex,
    summary_ciphertext_hex: encryptedSummary.ciphertext_hex,
    payload_version: 1,
    payload_nonce_hex: encryptedPayload.nonce_hex,
    payload_ciphertext_hex: encryptedPayload.ciphertext_hex,
    favorite: Boolean(item.favorite),
    revision: item.revision,
    uri_indexes: uriIndexes,
  };
}

function getRandomInt(maxExclusive: number) {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return buf[0] % maxExclusive;
}

function randomChoice<T>(items: readonly T[]) {
  return items[getRandomInt(items.length)];
}

function filterAmbiguous(value: string, excludeAmbiguous: boolean) {
  if (!excludeAmbiguous) return value;
  return [...value].filter((char) => !AMBIGUOUS.has(char)).join('');
}

function nextAllowedChar(prefs: RustyVaultPreferences, kindPreference?: 'number' | 'symbol') {
  const pools: string[] = [];
  if (kindPreference !== 'symbol' && prefs.password_generator_include_numbers) {
    pools.push(filterAmbiguous(NUMBERS, prefs.password_generator_exclude_ambiguous));
  }
  if (kindPreference !== 'number' && prefs.password_generator_include_symbols) {
    pools.push(filterAmbiguous(SYMBOLS, prefs.password_generator_exclude_ambiguous));
  }
  if (prefs.password_generator_include_uppercase) {
    pools.push(filterAmbiguous(UPPERCASE, prefs.password_generator_exclude_ambiguous));
  }
  if (prefs.password_generator_include_lowercase) {
    pools.push(filterAmbiguous(LOWERCASE, prefs.password_generator_exclude_ambiguous));
  }
  const combined = pools.join('');
  if (!combined) {
    throw new Error('Select at least one password character group');
  }
  return combined[getRandomInt(combined.length)];
}

function applyWordCase(word: string, prefs: RustyVaultPreferences) {
  if (prefs.password_generator_include_uppercase && prefs.password_generator_include_lowercase) {
    return word.charAt(0).toUpperCase() + word.slice(1);
  }
  if (prefs.password_generator_include_uppercase) {
    return word.toUpperCase();
  }
  return word.toLowerCase();
}

function replaceFirstMatch(
  source: string,
  replacements: readonly (readonly [string, string])[],
  prefs: RustyVaultPreferences,
  kind: 'number' | 'symbol',
) {
  for (const [needle, replacement] of replacements) {
    if (kind === 'number' && !prefs.password_generator_include_numbers) {
      continue;
    }
    if (kind === 'symbol' && !prefs.password_generator_include_symbols) {
      continue;
    }
    const idx = source.toLowerCase().indexOf(needle);
    if (idx >= 0) {
      return source.slice(0, idx) + replacement + source.slice(idx + 1);
    }
  }
  return source;
}

function buildMemorablePassword(prefs: RustyVaultPreferences) {
  const targetLength = Math.max(12, prefs.password_generator_default_length);
  const words: string[] = [];
  let rawLength = 0;
  while (rawLength < Math.max(8, targetLength - 3)) {
    const word = randomChoice(MEMORABLE_WORDS);
    words.push(word);
    rawLength += word.length;
    if (words.length >= 4) {
      break;
    }
  }
  let candidate = words.map((word) => applyWordCase(word, prefs)).join('');
  candidate = replaceFirstMatch(candidate, MEMORABLE_NUMBER_SWAP, prefs, 'number');
  candidate = replaceFirstMatch(candidate, MEMORABLE_SYMBOL_SWAP, prefs, 'symbol');
  while (candidate.length < targetLength) {
    candidate += nextAllowedChar(prefs);
  }
  return candidate.slice(0, targetLength);
}

export function generatePasswordFromPreferences(prefs: RustyVaultPreferences) {
  if (prefs.password_generator_default_preset === 'memorable') {
    return buildMemorablePassword(prefs);
  }
  const pools: string[] = [];
  if (prefs.password_generator_include_uppercase) {
    pools.push(filterAmbiguous(UPPERCASE, prefs.password_generator_exclude_ambiguous));
  }
  if (prefs.password_generator_include_lowercase) {
    pools.push(filterAmbiguous(LOWERCASE, prefs.password_generator_exclude_ambiguous));
  }
  if (prefs.password_generator_include_numbers) {
    pools.push(filterAmbiguous(NUMBERS, prefs.password_generator_exclude_ambiguous));
  }
  if (prefs.password_generator_include_symbols) {
    pools.push(filterAmbiguous(SYMBOLS, prefs.password_generator_exclude_ambiguous));
  }
  if (pools.length === 0) {
    throw new Error('Select at least one password character group');
  }
  const length = prefs.password_generator_default_preset === 'maximum'
    ? Math.max(prefs.password_generator_default_length, 30)
    : prefs.password_generator_default_length;
  const chars: string[] = [];
  const combined = pools.join('');
  for (const pool of pools) {
    chars.push(pool[getRandomInt(pool.length)]);
  }
  while (chars.length < length) {
    chars.push(combined[getRandomInt(combined.length)]);
  }
  for (let idx = chars.length - 1; idx > 0; idx -= 1) {
    const swapIdx = getRandomInt(idx + 1);
    [chars[idx], chars[swapIdx]] = [chars[swapIdx], chars[idx]];
  }
  return chars.join('');
}
