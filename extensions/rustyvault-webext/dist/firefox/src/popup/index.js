import { describePolicyReason } from '../shared/policy.js';
function $(id) {
    return document.getElementById(id);
}
async function callBackground(message) {
    return chrome.runtime.sendMessage(message);
}
let transientStatusText = '';
function setVisible(id, visible) {
    $(id)?.classList.toggle('hidden', !visible);
}
function fillButtonLabel(pagePolicy) {
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
function pendingLabel(kind) {
    switch (kind) {
        case 'update_existing':
            return 'Update saved login';
        case 'add_uri':
            return 'Add site to saved login';
        default:
            return 'Save captured login';
    }
}
function setStatusText(text) {
    transientStatusText = text;
    $('status').textContent = text;
}
async function persistPopupDraft(draft) {
    await callBackground({
        type: 'save-popup-draft',
        draft,
    });
}
async function render() {
    const response = await callBackground({ type: 'get-popup-state' });
    if (!response.ok) {
        setStatusText(response.error);
        return;
    }
    const state = response.state;
    $('server-url').value =
        state.popupDraft.serverBaseUrlInput || state.settings.serverBaseUrl || '';
    $('pairing-code').value = state.popupDraft.pairingInput || '';
    setVisible('pairing-panel', !state.paired);
    setVisible('unlock-panel', state.paired);
    setVisible('matches-panel', state.unlocked);
    setVisible('pending-panel', Boolean(state.pendingAction));
    const status = $('status');
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
        (sitePanel?.querySelector('#site-message')).textContent =
            'Grant this site so RustyVault can show suggestions and save prompts here.';
    }
    else {
        setVisible('site-panel', false);
    }
    const pagePolicyChips = $('page-policy-chips');
    const pagePolicyMessage = $('page-policy-message');
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
    const matchesRoot = $('matches');
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
                tabId: state.currentTab.id,
            });
            status.textContent = result.ok ? 'Credentials filled on the page.' : result.error;
            await render();
        });
        wrapper.append(title, uri, principal, fill);
        matchesRoot.appendChild(wrapper);
    }
    if (state.pendingAction) {
        $('pending-heading').textContent = pendingLabel(state.pendingAction.kind);
        $('pending-message').textContent = state.pendingAction.message;
        $('pending-title').value = state.pendingAction.draft.title;
        $('pending-username').value = state.pendingAction.draft.username;
        $('pending-email').value = state.pendingAction.draft.email;
        $('pending-password').value = state.pendingAction.draft.password;
        $('save-pending').textContent =
            state.pendingAction.kind === 'update_existing'
                ? 'Update'
                : state.pendingAction.kind === 'add_uri'
                    ? 'Add site'
                    : 'Save';
    }
}
$('save-server').addEventListener('click', async () => {
    const serverBaseUrl = $('server-url').value;
    const result = await callBackground({
        type: 'set-server-url',
        serverBaseUrl,
    });
    setStatusText(result.ok ? result.message || 'Rustyfin server URL saved.' : result.error);
    await render();
});
$('pair-device').addEventListener('click', async () => {
    const pairingInput = $('pairing-code').value;
    const result = await callBackground({
        type: 'pair-device',
        pairingInput,
        deviceName: 'Rustyfin Browser Extension',
    });
    setStatusText(result.ok
        ? result.message || 'Extension paired. Unlock it with the vault master password.'
        : result.error);
    await render();
});
$('unlock-vault').addEventListener('click', async () => {
    const result = await callBackground({
        type: 'unlock-vault',
        masterPassword: $('master-password').value,
    });
    setStatusText(result.ok ? 'Vault unlocked in extension memory.' : result.error);
    if (result.ok) {
        $('master-password').value = '';
    }
    await render();
});
$('lock-vault').addEventListener('click', async () => {
    await callBackground({ type: 'lock-vault' });
    setStatusText('Vault locked.');
    await render();
});
$('grant-site').addEventListener('click', async () => {
    const state = await callBackground({ type: 'get-popup-state' });
    if (!state.ok || !state.state?.currentTab?.url) {
        return;
    }
    const result = await callBackground({
        type: 'ensure-site-permission',
        url: state.state.currentTab.url,
        tabId: state.state.currentTab.id,
    });
    setStatusText(result.ok && result.granted
        ? 'Site access granted.'
        : result.ok
            ? 'Site access was not granted.'
            : result.error);
    await render();
});
$('save-pending').addEventListener('click', async () => {
    const state = await callBackground({ type: 'get-popup-state' });
    if (!state.ok || !state.state?.currentTab) {
        return;
    }
    const result = await callBackground({
        type: 'save-pending-item',
        tabId: state.state.currentTab.id,
        draft: {
            title: $('pending-title').value,
            username: $('pending-username').value,
            email: $('pending-email').value,
            password: $('pending-password').value,
        },
    });
    setStatusText(result.ok ? 'Vault item updated.' : result.error);
    await render();
});
$('dismiss-pending').addEventListener('click', async () => {
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
$('server-url').addEventListener('input', async (event) => {
    await persistPopupDraft({
        serverBaseUrlInput: event.currentTarget.value,
    });
});
$('pairing-code').addEventListener('input', async (event) => {
    await persistPopupDraft({
        pairingInput: event.currentTarget.value,
    });
});
render().catch((error) => {
    setStatusText(error instanceof Error ? error.message : String(error));
});
