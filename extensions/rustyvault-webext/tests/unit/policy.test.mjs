import test from 'node:test';
import assert from 'node:assert/strict';

import { describePolicyReason, evaluatePagePolicy, isExcludedDomain } from '../../dist/chromium/src/shared/policy.js';

test('excluded domains match exact host and subdomains', () => {
  assert.equal(isExcludedDomain('accounts.example.com', ['example.com']), true);
  assert.equal(isExcludedDomain('example.net', ['example.com']), false);
});

test('page policy blocks manual fill on http by default', () => {
  const policy = evaluatePagePolicy(
    {
      url: 'http://example.com/login',
      topLevelUrl: 'http://example.com/login',
      isTopFrame: true,
    },
    {
      allowManualHttpFill: false,
      excludedDomains: [],
    },
  );
  assert.equal(policy.canManualFill, false);
  assert.equal(describePolicyReason(policy.manualFillBlockedReason), 'Manual fill is blocked on HTTP pages unless you explicitly allow it.');
});

test('page policy blocks cross-origin iframe fill', () => {
  const policy = evaluatePagePolicy(
    {
      url: 'https://iframe.example.net/login',
      topLevelUrl: 'https://example.com/login',
      isTopFrame: false,
    },
    {
      allowManualHttpFill: false,
      excludedDomains: [],
      warnOnUntrustedIframe: true,
    },
  );
  assert.equal(policy.canManualFill, false);
  assert.equal(policy.crossOriginIframe, true);
});
