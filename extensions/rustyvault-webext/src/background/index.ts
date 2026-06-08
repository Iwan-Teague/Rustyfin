import type { BackgroundRequest, BackgroundResponse } from '../shared/messages.js';
import type {
  DecryptedLoginItem,
  MatchedVaultItem,
  PageContextPayload,
  PagePolicy,
  PendingItemDraft,
  RustyVaultPreferences,
} from '../shared/types.js';
import { getCurrentTab, logIfEnabled } from '../shared/browser.js';
import {
  apiRequest,
  defaultRustyVaultPreferences,
  getVaultPreferences,
  parseRustyVaultConnectionInput,
  refreshRustyVaultSession,
  sanitizeServerBaseUrl,
  verifyRustyfinServerBaseUrl,
  withRustyVaultSession,
} from '../shared/api.js';
import {
  buildLookupHashesForUrl,
  buildUriIndexes,
  decryptRustyVaultItem,
  decryptRustyVaultSummary,
  encryptRustyVaultLoginItem,
  generatePasswordFromPreferences,
  unlockRustyVault,
} from '../shared/crypto.js';
import { describePolicyReason, evaluatePagePolicy } from '../shared/policy.js';
import { classifyPendingAction } from '../shared/save-classifier.js';
import {
  clearGeneratedPassword,
  clearPendingAction,
  clearSession,
  clearSubmittedCandidate,
  DEFAULT_SETTINGS,
  getGeneratedPassword,
  getGrantedOrigins,
  getLastFilled,
  getPendingAction,
  getPopupDraft,
  getSession,
  getSettings,
  getSubmittedCandidate,
  savePendingAction,
  setGeneratedPassword,
  setGrantedOrigins,
  setLastFilled,
  setPopupDraft,
  setSession,
  setSettings,
  setSubmittedCandidate,
} from '../shared/storage.js';

const AUTO_LOCK_ALARM = 'rustyvault-auto-lock';
const REGISTERED_SCRIPT_ID = 'rustyvault-site-script';
const PENDING_EXPIRY_MS = 45_000;

type RememberedFrame = {
  frameId: number;
  url: string;
  isTopFrame: boolean;
};

let unlocked: any = null;
let matchesByTab = new Map<number, MatchedVaultItem[]>();
let pagePolicyByTab = new Map<number, PagePolicy>();
let lastFrameByTab = new Map<number, RememberedFrame>();
let vaultPreferences: RustyVaultPreferences = defaultRustyVaultPreferences();

function nowTs() {
  return Math.floor(Date.now() / 1000);
}

function nowMs() {
  return Date.now();
}

function responseOk(extra: Record<string, unknown> = {}): BackgroundResponse {
  return { ok: true, ...extra };
}

function responseError(error: unknown): BackgroundResponse {
  return {
    ok: false,
    error: error instanceof Error ? error.message : String(error),
  };
}

function requireTabId(explicitTabId: number | undefined, sender: any, action: string) {
  const tabId = explicitTabId ?? sender?.tab?.id;
  if (typeof tabId !== 'number') {
    throw new Error(`RustyVault could not determine which tab to ${action}`);
  }
  return tabId;
}

function rememberFrame(tabId: number, sender: any) {
  if (typeof sender?.frameId === 'number' && sender.frameId >= 0) {
    lastFrameByTab.set(tabId, {
      frameId: sender.frameId,
      url: typeof sender.url === 'string' ? sender.url : '',
      isTopFrame: sender.frameId === 0,
    });
  }
}

function preferredFrameId(tabId: number, sender: any) {
  if (typeof sender?.frameId === 'number' && sender.frameId >= 0) {
    return sender.frameId;
  }
  return lastFrameByTab.get(tabId)?.frameId;
}

// Resolve the frame that actually owns a fill/save request so the page policy
// can be evaluated against *that* frame instead of the top document. The fill
// path delivers credentials here, so a cross-origin subframe must be caught.
function resolveTargetFrame(tabId: number, sender: any): RememberedFrame | null {
  if (typeof sender?.frameId === 'number' && sender.frameId >= 0) {
    return {
      frameId: sender.frameId,
      url: typeof sender.url === 'string' ? sender.url : '',
      isTopFrame: sender.frameId === 0,
    };
  }
  return lastFrameByTab.get(tabId) ?? null;
}

async function sendContentMessage(
  tabId: number,
  message: Record<string, unknown>,
  frameId?: number,
) {
  if (typeof frameId === 'number') {
    try {
      return await chrome.tabs.sendMessage(tabId, message, { frameId });
    } catch {
      // Fall back to the top frame if the original target is gone.
    }
  }
  return chrome.tabs.sendMessage(tabId, message);
}

function decodeJwtPayload(token: string) {
  const parts = (token || '').split('.');
  if (parts.length < 2) {
    throw new Error('Invalid vault access token');
  }
  const padded = parts[1].replace(/-/g, '+').replace(/_/g, '/');
  const json = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, '='));
  return JSON.parse(json);
}

function buildImportedItem(draft: PendingItemDraft): DecryptedLoginItem {
  let fallbackTitle = 'Imported login';
  try {
    const parsedUrl = new URL(draft.url);
    if (parsedUrl.hostname) {
      fallbackTitle = parsedUrl.hostname;
    }
  } catch {
    // Keep the fallback for malformed captured URLs.
  }
  return {
    id: crypto.randomUUID(),
    title: (draft.title || '').trim() || fallbackTitle,
    username: (draft.username || '').trim(),
    login_email: (draft.email || '').trim(),
    password: draft.password || '',
    notes: '',
    website_urls: [draft.url].filter(Boolean),
    favorite: false,
    revision: 1,
    created_ts: nowTs(),
    updated_ts: nowTs(),
  };
}

function mergePreferencesIntoSettings(prefs: RustyVaultPreferences) {
  return {
    autoLockMinutes: prefs.auto_lock_minutes,
    excludedDomains: prefs.excluded_domains,
    defaultMatchMode: prefs.default_match_mode,
    warnOnHttp: prefs.warn_on_http,
    warnOnUntrustedIframe: prefs.warn_on_untrusted_iframe,
    allowManualHttpFill: prefs.allow_manual_http_fill,
    inlineAutofillEnabled: prefs.inline_autofill_enabled,
    inlineSavePromptEnabled: prefs.inline_save_prompt_enabled,
  };
}

async function scheduleAutoLock() {
  const settings = await getSettings();
  await chrome.alarms.clear(AUTO_LOCK_ALARM);
  if (!unlocked || settings.autoLockMinutes <= 0) {
    return;
  }
  await chrome.alarms.create(AUTO_LOCK_ALARM, {
    delayInMinutes: settings.autoLockMinutes,
  });
}

function lockRustyVaultState() {
  unlocked = null;
  matchesByTab = new Map();
  chrome.action.setBadgeText({ text: '' });
}

function originPatternForUrl(rawUrl: string) {
  const url = new URL(rawUrl);
  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error('This page cannot be granted site access');
  }
  return `${url.origin}/*`;
}

async function hasSitePermission(rawUrl: string) {
  const origin = originPatternForUrl(rawUrl);
  return chrome.permissions.contains({ origins: [origin] });
}

async function syncRegisteredContentScripts() {
  const permissions = await chrome.permissions.getAll();
  const grantedOrigins = (permissions.origins || []).filter((origin: string) =>
    origin.startsWith('http://') || origin.startsWith('https://'),
  );
  await setGrantedOrigins(grantedOrigins);
  await chrome.scripting.unregisterContentScripts({ ids: [REGISTERED_SCRIPT_ID] }).catch(() => null);
  if (grantedOrigins.length === 0) {
    return;
  }
  await chrome.scripting.registerContentScripts([
    {
      id: REGISTERED_SCRIPT_ID,
      matches: grantedOrigins,
      js: ['src/content/index.js'],
      allFrames: true,
      runAt: 'document_idle',
      persistAcrossSessions: true,
    },
  ]);
}

async function ensureSitePermission(rawUrl: string, tabId?: number) {
  const origin = originPatternForUrl(rawUrl);
  const alreadyGranted = await chrome.permissions.contains({ origins: [origin] });
  if (!alreadyGranted) {
    throw new Error('RustyVault needs site access for that page first');
  }
  await syncRegisteredContentScripts();
  if (typeof tabId === 'number') {
    await chrome.scripting.executeScript({
      target: { tabId, allFrames: true },
      files: ['src/content/index.js'],
    }).catch(() => null);
  }
  return true;
}

function originPatternForBaseUrl(baseUrl: string) {
  const url = new URL(baseUrl);
  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error('Server URL must use http or https');
  }
  return `${url.origin}/*`;
}

async function ensureServerPermission(baseUrl: string) {
  const origin = originPatternForBaseUrl(baseUrl);
  const alreadyGranted = await chrome.permissions.contains({ origins: [origin] });
  if (!alreadyGranted) {
    throw new Error('RustyVault needs access to that server address first');
  }
  return origin;
}

function policyInputForTab(tab: any, override: Record<string, unknown> = {}) {
  return {
    url: (override.url as string) || (override.frameUrl as string) || tab?.url || '',
    topLevelUrl:
      (override.topLevelUrl as string) ||
      tab?.url ||
      (override.url as string) ||
      (override.frameUrl as string) ||
      '',
    isTopFrame: override.isTopFrame !== false,
  };
}

async function resolvePagePolicy(tabId?: number, override: Record<string, unknown> = {}) {
  let tab: any = null;
  if (typeof tabId === 'number') {
    tab = await chrome.tabs.get(tabId).catch(() => null);
  }
  const settings = await getSettings();
  const policy = evaluatePagePolicy(policyInputForTab(tab, override), settings);
  if (typeof tabId === 'number') {
    pagePolicyByTab.set(tabId, policy);
  }
  return policy;
}

async function updateBadge(tabId: number, count: number, hasPending: boolean) {
  const text = hasPending ? 'SAVE' : count > 0 ? String(Math.min(count, 9)) : '';
  await chrome.action.setBadgeBackgroundColor({
    color: hasPending ? '#ff7588' : '#ff914d',
    tabId,
  });
  await chrome.action.setBadgeText({ text, tabId });
}

async function loadMatchesForUrl(
  tabId: number,
  url: string,
  override: Record<string, unknown> = {},
) {
  const policy = await resolvePagePolicy(tabId, { url, ...override });
  const pendingAction = await getPendingAction(tabId);
  if (!unlocked || !policy.url || !policy.canLookup) {
    matchesByTab.delete(tabId);
    await updateBadge(tabId, 0, Boolean(pendingAction));
    return [];
  }
  const settings = await getSettings();
  const hashes = await buildLookupHashesForUrl(unlocked.index_key, policy.url, settings.defaultMatchMode);
  const lookup = await withRustyVaultSession<{ items: any[] }>('/vault/lookup', {
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
  await updateBadge(tabId, decrypted.length, Boolean(pendingAction));
  return decrypted;
}

async function getPopupState() {
  const settings = await getSettings();
  const session = await getSession();
  const popupDraft = await getPopupDraft();
  const tab = await getCurrentTab();
  const pagePolicy = tab?.id
    ? await resolvePagePolicy(tab.id)
    : evaluatePagePolicy({}, settings);
  let sitePermissionGranted = false;
  let currentOrigin: string | null = null;
  if (tab?.url) {
    try {
      currentOrigin = new URL(tab.url).origin;
      sitePermissionGranted = await hasSitePermission(tab.url);
    } catch {
      sitePermissionGranted = false;
    }
  }
  return {
    settings,
    paired: Boolean(session),
    unlocked: Boolean(unlocked),
    currentTab: tab ? { id: tab.id!, url: tab.url, title: tab.title } : null,
    pagePolicy,
    matches: tab?.id ? matchesByTab.get(tab.id) || [] : [],
    pendingAction: tab?.id ? await getPendingAction(tab.id) : null,
    sitePermissionGranted,
    currentOrigin,
    popupDraft,
  };
}

async function maybeFinalizeSubmittedCandidate(tabId: number, payload: PageContextPayload) {
  const candidate = await getSubmittedCandidate(tabId);
  if (!candidate) {
    return;
  }
  if (nowMs() - candidate.submittedAt > PENDING_EXPIRY_MS) {
    await clearSubmittedCandidate(tabId);
    return;
  }
  const successLikely = payload.url !== candidate.url || !payload.hasPasswordField;
  if (!successLikely) {
    return;
  }
  const generated = await getGeneratedPassword(tabId);
  const matches = matchesByTab.get(tabId) || [];
  const draft: PendingItemDraft = {
    title: candidate.title,
    username: candidate.username,
    email: candidate.email,
    password: generated?.password || candidate.password,
    url: candidate.url,
    pageKind: candidate.pageKind as PendingItemDraft['pageKind'],
  };
  const action = classifyPendingAction({
    tabId,
    draft,
    matches,
    lastFilled: await getLastFilled(tabId),
    pageKind: candidate.pageKind as PendingItemDraft['pageKind'],
  });
  await clearSubmittedCandidate(tabId);
  if (!action) {
    return;
  }
  await savePendingAction(tabId, action);
  await updateBadge(tabId, matches.length, true);
  const settings = await getSettings();
  if (settings.inlineSavePromptEnabled) {
    await sendContentMessage(
      tabId,
      {
        type: 'show-save-prompt',
        payload: {
          kind: action.kind,
          message: action.message,
        },
      },
      lastFrameByTab.get(tabId)?.frameId,
    ).catch(() => null);
  }
}

async function fetchAndMergeItem(
  itemId: string,
  draft: PendingItemDraft,
  kind: 'update_existing' | 'add_uri',
) {
  const encrypted = await withRustyVaultSession<any>(`/vault/items/${encodeURIComponent(itemId)}`);
  const current = (await decryptRustyVaultItem(unlocked, encrypted)) as DecryptedLoginItem;
  const urls = [...new Set([...(current.website_urls || []), draft.url].filter(Boolean))];
  return {
    ...current,
    title: draft.title || current.title,
    username: draft.username || current.username,
    login_email: draft.email || current.login_email,
    password: kind === 'update_existing' ? draft.password : current.password,
    website_urls: urls,
    revision: current.revision + 1,
    updated_ts: nowTs(),
  } satisfies DecryptedLoginItem;
}

async function savePendingActionToServer(
  tabId: number,
  overrides: Partial<PendingItemDraft> = {},
  frameId?: number,
) {
  if (!unlocked) {
    throw new Error('Unlock the vault before saving a login');
  }
  const pending = await getPendingAction(tabId);
  if (!pending) {
    throw new Error('No pending save is available');
  }
  const settings = await getSettings();
  const draft = {
    ...pending.draft,
    ...overrides,
  };
  let item: DecryptedLoginItem;
  let path = '/vault/items';
  let method = 'POST';
  if (pending.kind === 'save_new') {
    item = buildImportedItem(draft);
  } else {
    item = await fetchAndMergeItem(pending.itemId!, draft, pending.kind);
    path = `/vault/items/${encodeURIComponent(item.id)}`;
    method = 'PUT';
  }
  const encrypted = await encryptRustyVaultLoginItem(unlocked, item, settings.defaultMatchMode);
  await withRustyVaultSession(path, {
    method,
    body: JSON.stringify(encrypted),
  });
  await clearPendingAction(tabId);
  await clearGeneratedPassword(tabId);
  await sendContentMessage(tabId, { type: 'dismiss-save-prompt' }, frameId ?? lastFrameByTab.get(tabId)?.frameId).catch(() => null);
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  if (tab?.url) {
    await loadMatchesForUrl(tabId, tab.url, { topLevelUrl: tab.url, isTopFrame: true }).catch(() => null);
  } else {
    await updateBadge(tabId, matchesByTab.get(tabId)?.length || 0, false);
  }
}

chrome.runtime.onInstalled.addListener(async () => {
  await setSettings(DEFAULT_SETTINGS);
  await syncRegisteredContentScripts();
});

chrome.permissions.onRemoved.addListener(async () => {
  await syncRegisteredContentScripts();
});

chrome.alarms.onAlarm.addListener((alarm: any) => {
  if (alarm.name === AUTO_LOCK_ALARM) {
    lockRustyVaultState();
  }
});

chrome.tabs.onActivated.addListener(async ({ tabId }: { tabId: number }) => {
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  if (tab?.url) {
    await loadMatchesForUrl(tabId, tab.url, { topLevelUrl: tab.url, isTopFrame: true }).catch(() => null);
  }
});

chrome.tabs.onUpdated.addListener(async (tabId: number, changeInfo: any, tab: any) => {
  if (changeInfo.status === 'complete' && tab.url) {
    await loadMatchesForUrl(tabId, tab.url, { topLevelUrl: tab.url, isTopFrame: true }).catch(() => null);
  }
});

chrome.tabs.onRemoved.addListener((tabId: number) => {
  matchesByTab.delete(tabId);
  pagePolicyByTab.delete(tabId);
  lastFrameByTab.delete(tabId);
});

chrome.runtime.onMessage.addListener(
  (message: BackgroundRequest, sender: any, sendResponse: (response: BackgroundResponse) => void) => {
    (async () => {
      const settings = await getSettings();
      try {
        switch (message.type) {
          case 'save-popup-draft': {
            const popupDraft = await setPopupDraft(message.draft || {});
            sendResponse(responseOk({ message: 'Popup draft saved.', state: { ...(await getPopupState()), popupDraft } }));
            return;
          }
          case 'set-server-url': {
            const normalizedBaseUrl = sanitizeServerBaseUrl(message.serverBaseUrl);
            await ensureServerPermission(normalizedBaseUrl);
            await verifyRustyfinServerBaseUrl(normalizedBaseUrl);
            const nextSettings = await setSettings({ serverBaseUrl: normalizedBaseUrl });
            await setPopupDraft({ serverBaseUrlInput: normalizedBaseUrl });
            sendResponse(
              responseOk({
                settings: nextSettings,
                message: 'Rustyfin server verified and saved.',
              }),
            );
            return;
          }
          case 'save-settings': {
            const nextSettings = await setSettings(message.settings as any);
            sendResponse(responseOk({ settings: nextSettings }));
            return;
          }
          case 'pair-device': {
            const parsed = parseRustyVaultConnectionInput(message.pairingInput);
            const candidateServerBaseUrl =
              parsed.serverBaseUrl || message.serverBaseUrl || settings.serverBaseUrl;
            if (!candidateServerBaseUrl) {
              throw new Error('Set the Rustyfin server URL in the extension first');
            }
            const normalizedBaseUrl = sanitizeServerBaseUrl(candidateServerBaseUrl);
            await ensureServerPermission(normalizedBaseUrl);
            await verifyRustyfinServerBaseUrl(normalizedBaseUrl);
            if (normalizedBaseUrl !== settings.serverBaseUrl) {
              await setSettings({ serverBaseUrl: normalizedBaseUrl });
            }
            await setPopupDraft({
              serverBaseUrlInput: normalizedBaseUrl,
              pairingInput: parsed.pairingCode,
            });
            const tokens = await apiRequest<any>('/vault/device-sessions/pair/consume', {
              method: 'POST',
              body: JSON.stringify({
                pairing_code: parsed.pairingCode,
                device_name: message.deviceName || 'Rustyfin Browser Extension',
                device_platform: 'webext',
              }),
            });
            await setSession(tokens);
            await setPopupDraft({
              serverBaseUrlInput: normalizedBaseUrl,
              pairingInput: '',
            });
            sendResponse(
              responseOk({
                message: 'Extension paired. Unlock it with the vault master password.',
              }),
            );
            return;
          }
          case 'unlock-vault': {
            const session = await refreshRustyVaultSession().catch(() => getSession());
            if (!session?.access_token) {
              throw new Error('The extension is not paired to Rustyfin');
            }
            const config = await withRustyVaultSession<any>('/vault/config');
            if (!config?.active_wrapped_key) {
              throw new Error('No Rustyfin vault is configured for this account');
            }
            const claims = decodeJwtPayload(session.access_token);
            unlocked = await unlockRustyVault(message.masterPassword, claims.sub, config.active_wrapped_key);
            vaultPreferences = await getVaultPreferences().catch(() => defaultRustyVaultPreferences());
            await setSettings(mergePreferencesIntoSettings(vaultPreferences));
            await scheduleAutoLock();
            const tab = await getCurrentTab();
            if (tab?.id && tab.url) {
              await loadMatchesForUrl(tab.id, tab.url, { topLevelUrl: tab.url, isTopFrame: true });
            }
            sendResponse(responseOk());
            return;
          }
          case 'lock-vault': {
            lockRustyVaultState();
            sendResponse(responseOk());
            return;
          }
          case 'get-popup-state': {
            sendResponse(responseOk({ state: await getPopupState() }));
            return;
          }
          case 'ensure-site-permission': {
            const granted = await ensureSitePermission(message.url, message.tabId);
            sendResponse(responseOk({ granted }));
            return;
          }
          case 'page-context': {
            if (sender.tab?.id) {
              if (
                message.payload.hasPasswordField ||
                message.payload.pageKind === 'login' ||
                message.payload.pageKind === 'signup' ||
                message.payload.pageKind === 'change_password' ||
                message.payload.pageKind === 'username_step'
              ) {
                rememberFrame(sender.tab.id, sender);
              }
              // Prefer the browser-supplied frame identity over page-reported
              // values so a cross-origin iframe cannot poison this tab's match
              // list by claiming to be the top frame at an attacker-chosen URL.
              const frameUrl = (typeof sender.url === 'string' && sender.url) || message.payload.url;
              const frameIsTop =
                typeof sender.frameId === 'number' ? sender.frameId === 0 : message.payload.isTopFrame;
              await loadMatchesForUrl(sender.tab.id, frameUrl, {
                topLevelUrl: sender.tab.url || message.payload.topLevelUrl,
                isTopFrame: frameIsTop,
              });
              await maybeFinalizeSubmittedCandidate(sender.tab.id, message.payload);
            }
            sendResponse(responseOk());
            return;
          }
          case 'credential-capture': {
            if (sender.tab?.id) {
              rememberFrame(sender.tab.id, sender);
              const policy = await resolvePagePolicy(sender.tab.id, {
                url: sender.url || message.payload.url,
                topLevelUrl: sender.tab.url || message.payload.url,
                isTopFrame: (sender.frameId ?? 0) === 0,
              });
              if (policy.canSavePrompt) {
                await setSubmittedCandidate(sender.tab.id, {
                  ...message.payload,
                  submittedAt: nowMs(),
                  tabId: sender.tab.id,
                });
              }
            }
            sendResponse(responseOk());
            return;
          }
          case 'fill-item': {
            if (!unlocked) {
              throw new Error('Unlock the vault and select an item first');
            }
            const tabId = requireTabId(message.tabId, sender, 'fill credentials on');
            rememberFrame(tabId, sender);
            // Credentials are delivered to this exact frame, so the page policy
            // must be evaluated against the target frame (not the top document).
            // Otherwise a login form inside a cross-origin iframe would receive
            // plaintext credentials the `untrusted_iframe` policy forbids.
            const targetFrame = resolveTargetFrame(tabId, sender);
            const tab = await chrome.tabs.get(tabId).catch(() => null);
            // Fail closed: a subframe target can only be filled if we can compare
            // its real URL against the top document. Missing either side means we
            // cannot prove same-origin, so refuse rather than risk a cross-origin
            // leak the `untrusted_iframe` policy is meant to block.
            if (targetFrame && !targetFrame.isTopFrame && (!targetFrame.url || !tab?.url)) {
              throw new Error(describePolicyReason('untrusted_iframe'));
            }
            const policy = await resolvePagePolicy(tabId, {
              url: targetFrame?.url || tab?.url,
              topLevelUrl: tab?.url || targetFrame?.url,
              isTopFrame: targetFrame ? targetFrame.isTopFrame : true,
            });
            if (!policy.canManualFill) {
              throw new Error(
                describePolicyReason(policy.manualFillBlockedReason) ||
                  'Manual fill is blocked on this page',
              );
            }
            const encrypted = await withRustyVaultSession<any>(
              `/vault/items/${encodeURIComponent(message.itemId)}`,
            );
            const payload = await decryptRustyVaultItem(unlocked, encrypted);
            await sendContentMessage(
              tabId,
              {
                type: 'fill-credentials',
                payload: {
                  username: payload.username,
                  email: payload.login_email,
                  password: payload.password,
                },
              },
              targetFrame ? targetFrame.frameId : preferredFrameId(tabId, sender),
            );
            await setLastFilled(tabId, {
              tabId,
              itemId: message.itemId,
              url: policy.url || '',
              username: payload.username || '',
              email: payload.login_email || '',
              filledAt: nowMs(),
            });
            await scheduleAutoLock();
            sendResponse(responseOk());
            return;
          }
          case 'save-pending-item': {
            const tabId = requireTabId(message.tabId, sender, 'save this login for');
            rememberFrame(tabId, sender);
            await savePendingActionToServer(
              tabId,
              message.draft || {},
              preferredFrameId(tabId, sender),
            );
            sendResponse(responseOk());
            return;
          }
          case 'dismiss-pending-item': {
            const tabId = requireTabId(message.tabId, sender, 'dismiss this save prompt for');
            await clearPendingAction(tabId);
            await sendContentMessage(
              tabId,
              { type: 'dismiss-save-prompt' },
              preferredFrameId(tabId, sender),
            ).catch(() => null);
            await updateBadge(tabId, matchesByTab.get(tabId)?.length || 0, false);
            sendResponse(responseOk());
            return;
          }
          case 'get-inline-state': {
            const tabId = requireTabId(message.tabId, sender, 'show inline state for');
            rememberFrame(tabId, sender);
            const sitePermissionGranted = await hasSitePermission(message.url).catch(() => false);
            // Evaluate against the requesting frame, not a blanket top-level URL,
            // so a cross-origin iframe overlay is told `untrusted_iframe` and is
            // never offered the vault match list or a fill affordance.
            const frameUrl = (typeof sender?.url === 'string' && sender.url) || message.url;
            const frameIsTop = typeof sender?.frameId === 'number' ? sender.frameId === 0 : true;
            const policy = await resolvePagePolicy(tabId, {
              url: frameUrl,
              topLevelUrl: sender?.tab?.url || message.url,
              isTopFrame: frameIsTop,
            });
            sendResponse(
              responseOk({
                unlocked: Boolean(unlocked),
                // Never leak the vault match list to an untrusted cross-origin
                // frame, even as titles/usernames in an overlay it controls.
                matches: policy.crossOriginIframe ? [] : matchesByTab.get(tabId) || [],
                pagePolicy: policy,
                pendingAction: await getPendingAction(tabId),
                sitePermissionGranted,
                settings,
              }),
            );
            return;
          }
          case 'generate-password': {
            if (!unlocked) {
              throw new Error('Unlock the vault before generating a password');
            }
            const tabId = requireTabId(message.tabId, sender, 'generate a password for');
            const password = generatePasswordFromPreferences(vaultPreferences);
            await setGeneratedPassword(tabId, {
              tabId,
              password,
              url: message.url,
              pageKind: message.pageKind as any,
              createdAt: nowMs(),
            });
            sendResponse(responseOk({ password }));
            return;
          }
          case 'notify-inline-dismissed': {
            sendResponse(responseOk());
            return;
          }
          default: {
            throw new Error(`Unsupported extension message: ${(message as any).type}`);
          }
        }
      } catch (error) {
        logIfEnabled(settings.debugLogging, 'bg', 'request failed', message.type, error);
        sendResponse(responseError(error));
      }
    })();
    return true;
  },
);
