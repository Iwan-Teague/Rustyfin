import { buildLookupHashesForUrl, decryptRustyVaultItem, decryptRustyVaultSummary, encryptRustyVaultLoginItem, unlockRustyVault } from './shared/crypto.js';
import { getRustyVaultSession, getSettings, refreshRustyVaultSession, sanitizeServerBaseUrl, setRustyVaultSession, setSettings, withRustyVaultSession, apiRequest } from './shared/api.js';
import { describePolicyReason, evaluatePagePolicy } from './shared/policy.js';

const RUSTYVAULT_AUTO_LOCK_ALARM = 'rustyvault-auto-lock';
let unlocked = null;
let matchesByTab = new Map();
let pendingByTab = new Map();
let pagePolicyByTab = new Map();

function nowTs() {
  return Math.floor(Date.now() / 1000);
}

async function scheduleAutoLock() {
  const settings = await getSettings();
  await chrome.alarms.clear(RUSTYVAULT_AUTO_LOCK_ALARM);
  if (!unlocked || settings.autoLockMinutes <= 0) {
    return;
  }
  await chrome.alarms.create(RUSTYVAULT_AUTO_LOCK_ALARM, {
    delayInMinutes: settings.autoLockMinutes,
  });
}

function lockRustyVaultState() {
  unlocked = null;
  matchesByTab = new Map();
  pendingByTab = new Map();
  chrome.action.setBadgeText({ text: '' });
}

function policyInputForTab(tab, override = {}) {
  return {
    url: override.url || override.frameUrl || tab?.url || '',
    topLevelUrl: override.topLevelUrl || tab?.url || override.url || override.frameUrl || '',
    isTopFrame: override.isTopFrame !== false,
  };
}

async function resolvePagePolicy(tabId, override = null) {
  let tab = null;
  if (typeof tabId === 'number') {
    tab = await chrome.tabs.get(tabId).catch(() => null);
  }
  if (!override && typeof tabId === 'number' && pagePolicyByTab.has(tabId)) {
    const current = pagePolicyByTab.get(tabId);
    if (!tab?.url || current?.topLevelUrl === tab.url) {
      return current;
    }
  }
  const settings = await getSettings();
  const policy = evaluatePagePolicy(policyInputForTab(tab, override || {}), settings);
  if (typeof tabId === 'number') {
    pagePolicyByTab.set(tabId, policy);
  }
  return policy;
}

async function updateBadge(tabId, count, pending) {
  const text = pending ? 'SAVE' : count > 0 ? String(Math.min(count, 9)) : '';
  await chrome.action.setBadgeBackgroundColor({
    color: pending ? '#ff7588' : '#ff914d',
    tabId,
  });
  await chrome.action.setBadgeText({ tabId, text });
}

async function fetchCurrentTab() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  return tab || null;
}

function buildImportedItem(payload) {
  let fallbackTitle = 'Imported login';
  try {
    const parsedUrl = new URL(payload.url);
    if (parsedUrl.hostname) {
      fallbackTitle = parsedUrl.hostname;
    }
  } catch {
    // Keep default fallback for malformed URLs captured from page scripts.
  }
  return {
    id: crypto.randomUUID(),
    title: (payload.title || '').trim() || fallbackTitle,
    username: (payload.username || '').trim(),
    login_email: (payload.email || '').trim(),
    password: payload.password || '',
    notes: '',
    website_urls: [payload.url].filter(Boolean),
    favorite: false,
    revision: 1,
    created_ts: nowTs(),
    updated_ts: nowTs(),
  };
}

function decodeJwtPayload(token) {
  const parts = (token || '').split('.');
  if (parts.length < 2) {
    throw new Error('Invalid vault access token');
  }
  const padded = parts[1].replace(/-/g, '+').replace(/_/g, '/');
  const json = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, '='));
  return JSON.parse(json);
}

async function loadMatchesForUrl(tabId, url, override = {}) {
  const policy = await resolvePagePolicy(tabId, {
    url,
    ...override,
  });
  if (!policy.canSavePrompt) {
    pendingByTab.delete(tabId);
  }
  if (!unlocked || !policy.url) {
    matchesByTab.delete(tabId);
    await updateBadge(tabId, 0, pendingByTab.has(tabId));
    return [];
  }
  if (!policy.canLookup) {
    matchesByTab.delete(tabId);
    await updateBadge(tabId, 0, pendingByTab.has(tabId));
    return [];
  }
  const settings = await getSettings();
  const hashes = await buildLookupHashesForUrl(unlocked.index_key, policy.url, settings.defaultMatchMode);
  const lookup = await withRustyVaultSession('/vault/lookup', {
    method: 'POST',
    body: JSON.stringify({ match_hashes_hex: hashes }),
  });
  const decrypted = await Promise.all(
    (lookup.items || []).map(async (item) => ({
      encrypted: item,
      summary: await decryptRustyVaultSummary(unlocked, item),
    })),
  );
  matchesByTab.set(tabId, decrypted);
  await updateBadge(tabId, decrypted.length, pendingByTab.has(tabId));
  return decrypted;
}

chrome.runtime.onInstalled.addListener(async () => {
  await setSettings({});
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === RUSTYVAULT_AUTO_LOCK_ALARM) {
    lockRustyVaultState();
  }
});

chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  try {
    const tab = await chrome.tabs.get(tabId);
    if (tab?.url) {
      await loadMatchesForUrl(tabId, tab.url, { topLevelUrl: tab.url, isTopFrame: true });
    }
  } catch {
    await updateBadge(tabId, 0, pendingByTab.has(tabId));
  }
});

chrome.tabs.onUpdated.addListener(async (tabId, changeInfo, tab) => {
  if (changeInfo.status === 'complete' && tab.url) {
    try {
      await loadMatchesForUrl(tabId, tab.url, { topLevelUrl: tab.url, isTopFrame: true });
    } catch {
      await updateBadge(tabId, 0, pendingByTab.has(tabId));
    }
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  matchesByTab.delete(tabId);
  pendingByTab.delete(tabId);
  pagePolicyByTab.delete(tabId);
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  (async () => {
    switch (message.type) {
      case 'set-server-url': {
        const serverBaseUrl = sanitizeServerBaseUrl(message.serverBaseUrl);
        const settings = await setSettings({ serverBaseUrl });
        sendResponse({ ok: true, settings });
        break;
      }
      case 'save-settings': {
        const settings = await setSettings(message.settings || {});
        const tab = await fetchCurrentTab();
        if (tab?.id && tab.url) {
          await loadMatchesForUrl(tab.id, tab.url, { topLevelUrl: tab.url, isTopFrame: true }).catch(() => null);
        }
        sendResponse({ ok: true, settings });
        break;
      }
      case 'pair-device': {
        const tokens = await apiRequest('/vault/device-sessions/pair/consume', {
          method: 'POST',
          body: JSON.stringify({
            pairing_code: message.pairingCode,
            device_name: message.deviceName || 'Rustyfin Browser Extension',
            device_platform: 'webext',
          }),
        });
        await setRustyVaultSession(tokens);
        sendResponse({ ok: true, tokens });
        break;
      }
      case 'unlock-vault': {
        const session = await refreshRustyVaultSession().catch(() => getRustyVaultSession());
        if (!session?.access_token) {
          throw new Error('The extension is not paired to Rustyfin');
        }
        const config = await withRustyVaultSession('/vault/config');
        if (!config?.active_wrapped_key) {
          throw new Error('No Rustyfin vault is configured for this account');
        }
        const claims = decodeJwtPayload(session.access_token);
        unlocked = await unlockRustyVault(message.masterPassword, claims.sub, config.active_wrapped_key);
        // The extension does not persist unlocked keys. Service-worker restarts lock it automatically.
        await scheduleAutoLock();
        const tab = await fetchCurrentTab();
        if (tab?.id && tab.url) {
          await loadMatchesForUrl(tab.id, tab.url, { topLevelUrl: tab.url, isTopFrame: true });
        }
        sendResponse({ ok: true, unlocked: true });
        break;
      }
      case 'lock-vault': {
        lockRustyVaultState();
        sendResponse({ ok: true });
        break;
      }
      case 'get-popup-state': {
        const settings = await getSettings();
        const session = await getRustyVaultSession();
        const tab = await fetchCurrentTab();
        const pagePolicy = tab?.id
          ? await resolvePagePolicy(tab.id)
          : evaluatePagePolicy({}, settings);
        sendResponse({
          ok: true,
          settings,
          paired: Boolean(session),
          unlocked: Boolean(unlocked),
          currentTab: tab ? { id: tab.id, url: tab.url, title: tab.title } : null,
          matches: tab?.id ? matchesByTab.get(tab.id) || [] : [],
          pendingSave: tab?.id ? pendingByTab.get(tab.id) || null : null,
          pagePolicy,
        });
        break;
      }
      case 'page-context': {
        if (sender.tab?.id && message.url) {
          await loadMatchesForUrl(sender.tab.id, message.url, {
            topLevelUrl: sender.tab.url || message.url,
            isTopFrame: (sender.frameId ?? 0) === 0,
          });
        }
        sendResponse({ ok: true });
        break;
      }
      case 'credential-capture': {
        if (sender.tab?.id && message.payload?.password) {
          const policy = await resolvePagePolicy(sender.tab.id, {
            url: sender.url || message.payload.url,
            topLevelUrl: sender.tab.url || message.payload.url,
            isTopFrame: (sender.frameId ?? 0) === 0,
          });
          if (!policy.canSavePrompt) {
            pendingByTab.delete(sender.tab.id);
            await updateBadge(sender.tab.id, (matchesByTab.get(sender.tab.id) || []).length, false);
            sendResponse({ ok: true, suppressed: true });
            break;
          }
          pendingByTab.set(sender.tab.id, message.payload);
          const matches = matchesByTab.get(sender.tab.id) || [];
          await updateBadge(sender.tab.id, matches.length, true);
        }
        sendResponse({ ok: true });
        break;
      }
      case 'fill-item': {
        const tabId = sender.tab?.id || message.tabId;
        if (!unlocked || !message.itemId || !tabId) {
          throw new Error('Unlock the vault and select an item first');
        }
        const policy = await resolvePagePolicy(tabId);
        if (!policy.canManualFill) {
          throw new Error(describePolicyReason(policy.manualFillBlockedReason) || 'Manual fill is blocked on this page');
        }
        const encrypted = await withRustyVaultSession(`/vault/items/${encodeURIComponent(message.itemId)}`);
        const payload = await decryptRustyVaultItem(unlocked, encrypted);
        await chrome.tabs.sendMessage(tabId, {
          type: 'fill-credentials',
          payload: {
            username: payload.username,
            email: payload.login_email,
            password: payload.password,
          },
        });
        await scheduleAutoLock();
        sendResponse({ ok: true });
        break;
      }
      case 'save-pending-item': {
        if (!unlocked) {
          throw new Error('Unlock the vault before saving a login');
        }
        const tabId = sender.tab?.id || message.tabId;
        const pending = tabId ? pendingByTab.get(tabId) : null;
        if (!pending) {
          throw new Error('No pending login capture is available');
        }
        const settings = await getSettings();
        const item = buildImportedItem(pending);
        item.title = message.title || item.title;
        item.username = message.username ?? item.username;
        item.login_email = message.email ?? item.login_email;
        item.password = message.password ?? item.password;
        const encrypted = await encryptRustyVaultLoginItem(unlocked, item, settings.defaultMatchMode);
        await withRustyVaultSession('/vault/items', {
          method: 'POST',
          body: JSON.stringify(encrypted),
        });
        pendingByTab.delete(tabId);
        await loadMatchesForUrl(tabId, pending.url, { topLevelUrl: pending.url, isTopFrame: true });
        sendResponse({ ok: true });
        break;
      }
      default:
        sendResponse({ ok: false, error: 'Unknown message type' });
        break;
    }
  })().catch((error) => {
    sendResponse({
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  });
  return true;
});
