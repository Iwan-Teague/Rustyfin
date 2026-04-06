import { ArgonType, hash as argon2BrowserHash, probeArgon2BrowserFallback } from './argon2-browser.js';

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

let nativeArgon2SupportPromise = null;
let portableArgon2SupportPromise = null;

function toArrayBuffer(bytes) {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
}

export function toHex(bytes) {
  return [...bytes].map((value) => value.toString(16).padStart(2, '0')).join('');
}

export function fromHex(value) {
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

function aadBytes(userId, itemId, blobKind, version) {
  return encoder.encode(`rustyvault:${userId}:${itemId}:${blobKind}:v${version}`);
}

async function importArgon2PasswordKey(password) {
  return crypto.subtle.importKey('raw-secret', encoder.encode(password), 'Argon2id', false, [
    'deriveBits',
  ]);
}

async function browserSupportsNativeArgon2Id() {
  if (!nativeArgon2SupportPromise) {
    nativeArgon2SupportPromise = (async () => {
      try {
        const probe = await importArgon2PasswordKey('probe');
        await crypto.subtle.deriveBits(
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

async function deriveMasterMaterial(password, salt) {
  let bits;
  if (await browserSupportsNativeArgon2Id()) {
    const passwordKey = await importArgon2PasswordKey(password);
    bits = await crypto.subtle.deriveBits(
      {
        name: 'Argon2id',
        nonce: toArrayBuffer(salt),
        parallelism: 4,
        memory: 65536,
        passes: 3,
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
      time: 3,
      mem: 65536,
      parallelism: 4,
      hashLen: 32,
      type: ArgonType.Argon2id,
    }).then((result) => result.hash);
  }
  return crypto.subtle.importKey('raw', bits, 'HKDF', false, ['deriveKey']);
}

async function deriveWrapKey(masterMaterial) {
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

async function deriveIndexKey(masterMaterial) {
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

export async function unlockRustyVault(masterPassword, userId, wrappedKey) {
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

export async function decryptRustyVaultSummary(unlocked, item) {
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

export async function decryptRustyVaultItem(unlocked, item) {
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

export function normalizeWebsiteUrl(raw) {
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

function sanitizeHost(host) {
  return (host || '').trim().toLowerCase().replace(/\.+$/, '');
}

function isIpv4Host(host) {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host);
}

function isIpv6Host(host) {
  return host.includes(':');
}

function toBaseDomain(host) {
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

function uriTokensForUrl(url, matchMode) {
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

async function blindedIndexHex(indexKey, token) {
  const signature = await crypto.subtle.sign('HMAC', indexKey, encoder.encode(token));
  return toHex(new Uint8Array(signature));
}

export async function buildLookupHashesForUrl(indexKey, rawUrl, matchMode) {
  const normalized = normalizeWebsiteUrl(rawUrl);
  if (!normalized) {
    return [];
  }
  const hashes = [];
  for (const token of uriTokensForUrl(normalized, matchMode)) {
    hashes.push(await blindedIndexHex(indexKey, token.token));
  }
  return hashes;
}

export async function buildUriIndexes(indexKey, urls, matchMode) {
  const outputs = [];
  const seen = new Set();
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

async function encryptBlob(unlocked, itemId, blobKind, version, value) {
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

export async function encryptRustyVaultLoginItem(unlocked, item, matchMode) {
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
    revision: item.revision || 1,
    uri_indexes: await buildUriIndexes(unlocked.index_key, item.website_urls || [], matchMode),
  };
}
