import test from 'node:test';
import assert from 'node:assert/strict';

import {
  parseRustyVaultConnectionInput,
  verifyRustyfinServerBaseUrl,
} from '../../dist/chromium/src/shared/api.js';

test('parses a plain pairing code without a server override', () => {
  const parsed = parseRustyVaultConnectionInput('rfvlt-aaaaaa-bbbbbb-cccccc-dddddd');
  assert.equal(parsed.serverBaseUrl, null);
  assert.equal(parsed.pairingCode, 'RFVLT-AAAAAA-BBBBBB-CCCCCC-DDDDDD');
});

test('parses a full RustyVault connection code with a server URL', () => {
  const parsed = parseRustyVaultConnectionInput(
    'rustyvault://pair?server=https%3A%2F%2F192.168.0.36%3A3008&code=RFVLT-AAAAAA-BBBBBB-CCCCCC-DDDDDD',
  );
  assert.equal(parsed.serverBaseUrl, 'https://192.168.0.36:3008');
  assert.equal(parsed.pairingCode, 'RFVLT-AAAAAA-BBBBBB-CCCCCC-DDDDDD');
});

test('rejects malformed connection input', () => {
  assert.throws(
    () => parseRustyVaultConnectionInput('not-a-pairing-code'),
    /valid pairing code or RustyVault connection code/i,
  );
});

test('verifies a reachable Rustyfin server URL through runtime-config', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    assert.equal(String(input), 'https://192.168.0.36:3008/runtime-config');
    return new Response(JSON.stringify({ backend_origin: null, ice_servers: [] }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  };

  try {
    const verified = await verifyRustyfinServerBaseUrl('https://192.168.0.36:3008/');
    assert.equal(verified.normalizedBaseUrl, 'https://192.168.0.36:3008');
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('rejects a non-Rustyfin server response during verification', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response('<html>not rustyfin</html>', {
      status: 200,
      headers: { 'content-type': 'text/html' },
    });

  try {
    await assert.rejects(
      () => verifyRustyfinServerBaseUrl('https://example.com'),
      /did not look like a Rustyfin server/i,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
