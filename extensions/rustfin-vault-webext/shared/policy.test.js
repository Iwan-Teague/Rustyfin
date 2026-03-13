import test from 'node:test';
import assert from 'node:assert/strict';

import { describePolicyReason, evaluatePagePolicy, isExcludedDomain } from './policy.js';

test('excluded domains suppress lookup, fill, and save prompts', () => {
  const policy = evaluatePagePolicy(
    {
      url: 'https://accounts.instagram.com/login',
      topLevelUrl: 'https://accounts.instagram.com/login',
      isTopFrame: true,
    },
    {
      excludedDomains: ['instagram.com'],
      allowManualHttpFill: false,
      warnOnUntrustedIframe: true,
    },
  );

  assert.equal(isExcludedDomain('accounts.instagram.com', ['instagram.com']), true);
  assert.equal(policy.isExcluded, true);
  assert.equal(policy.canLookup, false);
  assert.equal(policy.canManualFill, false);
  assert.equal(policy.canSavePrompt, false);
  assert.equal(policy.manualFillBlockedReason, 'excluded_domain');
  assert.match(describePolicyReason(policy.manualFillBlockedReason), /excluded domain/i);
});

test('http pages are blocked by default but can be opted into explicitly', () => {
  const blocked = evaluatePagePolicy(
    {
      url: 'http://router.local/login',
      topLevelUrl: 'http://router.local/login',
      isTopFrame: true,
    },
    {
      excludedDomains: [],
      allowManualHttpFill: false,
      warnOnUntrustedIframe: true,
    },
  );
  assert.equal(blocked.isHttp, true);
  assert.equal(blocked.canLookup, false);
  assert.equal(blocked.canManualFill, false);
  assert.equal(blocked.canSavePrompt, true);
  assert.equal(blocked.manualFillBlockedReason, 'http_blocked');

  const allowed = evaluatePagePolicy(
    {
      url: 'http://router.local/login',
      topLevelUrl: 'http://router.local/login',
      isTopFrame: true,
    },
    {
      excludedDomains: [],
      allowManualHttpFill: true,
      warnOnUntrustedIframe: true,
    },
  );
  assert.equal(allowed.canLookup, true);
  assert.equal(allowed.canManualFill, true);
  assert.equal(allowed.canSavePrompt, true);
});

test('cross-origin iframes are treated as blocked targets', () => {
  const policy = evaluatePagePolicy(
    {
      url: 'https://login.example-cdn.net/embed',
      topLevelUrl: 'https://example.com/app',
      isTopFrame: false,
    },
    {
      excludedDomains: [],
      allowManualHttpFill: false,
      warnOnUntrustedIframe: true,
    },
  );

  assert.equal(policy.crossOriginIframe, true);
  assert.equal(policy.sameOriginIframe, false);
  assert.equal(policy.canLookup, false);
  assert.equal(policy.canManualFill, false);
  assert.equal(policy.canSavePrompt, false);
  assert.equal(policy.manualFillBlockedReason, 'untrusted_iframe');
});

test('same-origin iframes remain eligible for manual fill', () => {
  const policy = evaluatePagePolicy(
    {
      url: 'https://example.com/embedded-login',
      topLevelUrl: 'https://example.com/app',
      isTopFrame: false,
    },
    {
      excludedDomains: [],
      allowManualHttpFill: false,
      warnOnUntrustedIframe: true,
    },
  );

  assert.equal(policy.sameOriginIframe, true);
  assert.equal(policy.crossOriginIframe, false);
  assert.equal(policy.canLookup, true);
  assert.equal(policy.canManualFill, true);
  assert.equal(policy.canSavePrompt, true);
});
