import test from 'node:test';
import assert from 'node:assert/strict';

import { parseRustyVaultConnectionInput } from '../../dist/chromium/src/shared/api.js';

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
