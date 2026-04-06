import test from 'node:test';
import assert from 'node:assert/strict';

import { classifyPendingAction } from '../../dist/chromium/src/shared/save-classifier.js';

const match = {
  encrypted: { id: 'item-1' },
  summary: {
    title: 'Example Login',
    username: 'iwan',
    login_email: 'iwan@example.com',
    primary_uri: 'https://example.com/login',
  },
};

test('classifies identity match as update', () => {
  const action = classifyPendingAction({
    tabId: 1,
    draft: {
      title: 'Example Login',
      username: 'iwan',
      email: '',
      password: 'new-password',
      url: 'https://example.com/login',
      pageKind: 'login',
    },
    matches: [match],
    lastFilled: null,
    pageKind: 'login',
  });
  assert.equal(action?.kind, 'update_existing');
  assert.equal(action?.itemId, 'item-1');
});

test('classifies new site after fill as add-uri', () => {
  const action = classifyPendingAction({
    tabId: 1,
    draft: {
      title: 'Example Login',
      username: 'iwan',
      email: '',
      password: 'same-password',
      url: 'https://accounts.example.com/login',
      pageKind: 'login',
    },
    matches: [],
    lastFilled: {
      tabId: 1,
      itemId: 'item-1',
      url: 'https://example.com/login',
      username: 'iwan',
      email: '',
      filledAt: Date.now(),
    },
    pageKind: 'login',
  });
  assert.equal(action?.kind, 'add_uri');
});

test('classifies unmatched credential as save-new', () => {
  const action = classifyPendingAction({
    tabId: 1,
    draft: {
      title: 'New Site',
      username: 'new-user',
      email: '',
      password: 'new-password',
      url: 'https://new.example.com',
      pageKind: 'signup',
    },
    matches: [],
    lastFilled: null,
    pageKind: 'signup',
  });
  assert.equal(action?.kind, 'save_new');
});
