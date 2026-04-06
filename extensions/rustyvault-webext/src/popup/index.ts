import { describePolicyReason } from '../shared/policy.js';
import {
  parseRustyVaultConnectionInput,
  sanitizeServerBaseUrl,
} from '../shared/api.js';
import type { BackgroundRequest, BackgroundResponse } from '../shared/messages.js';

function $(id: string) {
  return document.getElementById(id) as HTMLInputElement | HTMLButtonElement | HTMLDivElement | HTMLParagraphElement | null;
}

async function callBackground(message: BackgroundRequest): Promise<BackgroundResponse> {
  return chrome.runtime.sendMessage(message);
}

let transientStatusText = '';

function setVisible(id: string, visible: boolean) {
  $(id)?.classList.toggle('hidden', !visible);
}

function fillButtonLabel(pagePolicy: any) {
  switch (pagePolicy?.manualFillBlockedReason) {
    case 'excluded_domain':
      return 'Excluded domain';
    case 'http_blocked':
      return 'HTTP blocked';
    case 'untrusted_iframe':
      return 'Iframe blocked';
    default:
      return 'Fill on page';
  }
}

function pendingLabel(kind: string) {
  switch (kind) {
    case 'update_existing':
      return 'Update saved login';
    case 'add_uri':
      return 'Add site to saved login';
    default:
      return 'Save captured login';
  }
}

function setStatusText(text: string) {
  transientStatusText = text;
  ($('status') as HTMLParagraphElement).textContent = text;
}

async function persistPopupDraft(draft: { serverBaseUrlInput?: string; pairingInput?: string }) {
  await callBackground({
    type: 'save-popup-draft',
    draft,
  });
}

function originPatternForServer(baseUrl: string) {
  const url = new URL(baseUrl);
  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error('Server URL must use http or https');
  }
  return `${url.origin}/*`;
}

async function ensureServerPermissionFromPopup(serverBaseUrl: string) {
  const normalizedBaseUrl = sanitizeServerBaseUrl(serverBaseUrl);
  const origin = originPatternForServer(normalizedBaseUrl);
  const alreadyGranted = await chrome.permissions.contains({ origins: [origin] });
  const granted =
    alreadyGranted || (await chrome.permissions.request({ origins: [origin] }));
  if (!granted) {
    throw new Error('RustyVault needs access to that server address');
  }
  return normalizedBaseUrl;
}

async function ensureCurrentSitePermissionFromPopup(url: string) {
  const origin = originPatternForServer(url);
  const alreadyGranted = await chrome.permissions.contains({ origins: [origin] });
  return alreadyGranted || chrome.permissions.request({ origins: [origin] });
}

async function render() {
  const response = await callBackground({ type: 'get-popup-state' });
  if (!response.ok) {
    setStatusText(response.error);
    return;
  }
  const state = response.state!;
  ($('server-url') as HTMLInputElement).value =
    state.popupDraft.serverBaseUrlInput || state.settings.serverBaseUrl || '';
  ($('pairing-code') as HTMLInputElement).value = state.popupDraft.pairingInput || '';
  setVisible('pairing-panel', !state.paired);
  setVisible('unlock-panel', state.paired);
  setVisible('matches-panel', state.unlocked);
  setVisible('pending-panel', Boolean(state.pendingAction));

  const status = $('status') as HTMLParagraphElement;
  status.textContent =
    transientStatusText ||
    (state.unlocked
      ? `Unlocked for ${state.currentTab?.url || 'current tab'}`
      : state.paired
        ? 'Paired but locked'
        : 'Not paired');

  const sitePanel = $('site-panel');
  if (!state.sitePermissionGranted && state.currentTab?.url?.startsWith('http')) {
    setVisible('site-panel', true);
    (sitePanel?.querySelector('#site-message') as HTMLParagraphElement | null)!.textContent =
      'Grant this site so RustyVault can show suggestions and save prompts here.';
  } else {
    setVisible('site-panel', false);
  }

  const pagePolicyChips = $('page-policy-chips') as HTMLDivElement;
  const pagePolicyMessage = $('page-policy-message') as HTMLParagraphElement;
  pagePolicyChips.innerHTML = '';
  for (const chipText of state.pagePolicy?.chips || []) {
    const chip = document.createElement('span');
    chip.className = 'chip';
    chip.textContent = chipText;
    pagePolicyChips.appendChild(chip);
  }
  pagePolicyMessage.textContent =
    describePolicyReason(state.pagePolicy?.manualFillBlockedReason) ||
    describePolicyReason(state.pagePolicy?.savePromptBlockedReason);
  setVisible('page-policy-panel', Boolean((state.pagePolicy?.chips || []).length || pagePolicyMessage.textContent));

  const matchesRoot = $('matches') as HTMLDivElement;
  matchesRoot.innerHTML = '';
  for (const match of state.matches || []) {
    const wrapper = document.createElement('div');
    wrapper.className = 'match';
    const title = document.createElement('strong');
    title.textContent = match.summary.title;
    const uri = document.createElement('div');
    uri.className = 'muted';
    uri.textContent = match.summary.primary_uri || match.summary.subtitle || '';
    const principal = document.createElement('div');
    principal.className = 'muted';
    principal.textContent = match.summary.username || match.summary.login_email || 'No username';
    const fill = document.createElement('button');
    fill.className = 'btn btn-primary';
    fill.textContent = fillButtonLabel(state.pagePolicy);
    fill.disabled = state.pagePolicy?.canManualFill === false;
    fill.addEventListener('click', async () => {
      const result = await callBackground({
        type: 'fill-item',
        itemId: match.encrypted.id,
        tabId: state.currentTab!.id,
      });
      status.textContent = result.ok ? 'Credentials filled on the page.' : result.error;
      await render();
    });
    wrapper.append(title, uri, principal, fill);
    matchesRoot.appendChild(wrapper);
  }

  if (state.pendingAction) {
    ($('pending-heading') as HTMLHeadingElement).textContent = pendingLabel(state.pendingAction.kind);
    ($('pending-message') as HTMLParagraphElement).textContent = state.pendingAction.message;
    ($('pending-title') as HTMLInputElement).value = state.pendingAction.draft.title;
    ($('pending-username') as HTMLInputElement).value = state.pendingAction.draft.username;
    ($('pending-email') as HTMLInputElement).value = state.pendingAction.draft.email;
    ($('pending-password') as HTMLInputElement).value = state.pendingAction.draft.password;
    ($('save-pending') as HTMLButtonElement).textContent =
      state.pendingAction.kind === 'update_existing'
        ? 'Update'
        : state.pendingAction.kind === 'add_uri'
          ? 'Add site'
          : 'Save';
  }
}

($('save-server') as HTMLButtonElement).addEventListener('click', async () => {
  try {
    const serverBaseUrl = ($('server-url') as HTMLInputElement).value;
    const normalizedBaseUrl = await ensureServerPermissionFromPopup(serverBaseUrl);
    const result = await callBackground({
      type: 'set-server-url',
      serverBaseUrl: normalizedBaseUrl,
    });
    setStatusText(result.ok ? result.message || 'Rustyfin server URL saved.' : result.error);
    await render();
  } catch (error) {
    setStatusText(error instanceof Error ? error.message : String(error));
  }
});

($('pair-device') as HTMLButtonElement).addEventListener('click', async () => {
  try {
    const pairingInput = ($('pairing-code') as HTMLInputElement).value;
    const parsed = parseRustyVaultConnectionInput(pairingInput);
    const candidateServerBaseUrl =
      parsed.serverBaseUrl || ($('server-url') as HTMLInputElement).value;
    if (!candidateServerBaseUrl) {
      throw new Error('Set the Rustyfin server URL in the extension first');
    }
    const normalizedBaseUrl = await ensureServerPermissionFromPopup(candidateServerBaseUrl);
    const result = await callBackground({
      type: 'pair-device',
      pairingInput,
      deviceName: 'Rustyfin Browser Extension',
    });
    if (result.ok) {
      ($('server-url') as HTMLInputElement).value = normalizedBaseUrl;
    }
    setStatusText(
      result.ok
        ? result.message || 'Extension paired. Unlock it with the vault master password.'
        : result.error,
    );
    await render();
  } catch (error) {
    setStatusText(error instanceof Error ? error.message : String(error));
  }
});

($('unlock-vault') as HTMLButtonElement).addEventListener('click', async () => {
  const result = await callBackground({
    type: 'unlock-vault',
    masterPassword: ($('master-password') as HTMLInputElement).value,
  });
  setStatusText(result.ok ? 'Vault unlocked in extension memory.' : result.error);
  if (result.ok) {
    ($('master-password') as HTMLInputElement).value = '';
  }
  await render();
});

($('lock-vault') as HTMLButtonElement).addEventListener('click', async () => {
  await callBackground({ type: 'lock-vault' });
  setStatusText('Vault locked.');
  await render();
});

($('grant-site') as HTMLButtonElement).addEventListener('click', async () => {
  const state = await callBackground({ type: 'get-popup-state' });
  if (!state.ok || !state.state?.currentTab?.url) {
    return;
  }
  const grantedByPopup = await ensureCurrentSitePermissionFromPopup(state.state.currentTab.url).catch(
    (error) => {
      setStatusText(error instanceof Error ? error.message : String(error));
      return false;
    },
  );
  if (!grantedByPopup) {
    if (!transientStatusText) {
      setStatusText('Site access was not granted.');
    }
    return;
  }
  const result = await callBackground({
    type: 'ensure-site-permission',
    url: state.state.currentTab.url,
    tabId: state.state.currentTab.id,
  });
  setStatusText(
    result.ok && result.granted
      ? 'Site access granted.'
      : result.ok
        ? 'Site access was not granted.'
        : result.error,
  );
  await render();
});

($('save-pending') as HTMLButtonElement).addEventListener('click', async () => {
  const state = await callBackground({ type: 'get-popup-state' });
  if (!state.ok || !state.state?.currentTab) {
    return;
  }
  const result = await callBackground({
    type: 'save-pending-item',
    tabId: state.state.currentTab.id,
    draft: {
      title: ($('pending-title') as HTMLInputElement).value,
      username: ($('pending-username') as HTMLInputElement).value,
      email: ($('pending-email') as HTMLInputElement).value,
      password: ($('pending-password') as HTMLInputElement).value,
    },
  });
  setStatusText(result.ok ? 'Vault item updated.' : result.error);
  await render();
});

($('dismiss-pending') as HTMLButtonElement).addEventListener('click', async () => {
  const state = await callBackground({ type: 'get-popup-state' });
  if (!state.ok || !state.state?.currentTab) {
    return;
  }
  await callBackground({
    type: 'dismiss-pending-item',
    tabId: state.state.currentTab.id,
  });
  setStatusText('Pending save dismissed.');
  await render();
});

($('server-url') as HTMLInputElement).addEventListener('input', async (event) => {
  await persistPopupDraft({
    serverBaseUrlInput: (event.currentTarget as HTMLInputElement).value,
  });
});

($('pairing-code') as HTMLInputElement).addEventListener('input', async (event) => {
  await persistPopupDraft({
    pairingInput: (event.currentTarget as HTMLInputElement).value,
  });
});

render().catch((error) => {
  setStatusText(error instanceof Error ? error.message : String(error));
});
