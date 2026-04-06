function $(id) {
    return document.getElementById(id);
}
async function callBackground(message) {
    return chrome.runtime.sendMessage(message);
}
function setStatus(message, isError = false) {
    const status = $('status');
    status.textContent = message;
    status.style.color = isError ? 'rgba(248, 248, 255, 0.92)' : '';
}
async function load() {
    const response = await callBackground({ type: 'get-popup-state' });
    if (!response.ok || !response.state) {
        setStatus(response.ok ? 'Failed to load extension state' : response.error, true);
        return;
    }
    const settings = response.state.settings;
    $('server-url').value = settings.serverBaseUrl || '';
    $('auto-lock').value = String(settings.autoLockMinutes);
    $('match-mode').value = settings.defaultMatchMode;
    $('excluded-domains').value = settings.excludedDomains.join('\n');
    $('warn-http').checked = Boolean(settings.warnOnHttp);
    $('allow-http-fill').checked = Boolean(settings.allowManualHttpFill);
    $('warn-iframe').checked = Boolean(settings.warnOnUntrustedIframe);
    $('page-load-autofill').checked = Boolean(settings.pageLoadAutofill);
    $('inline-autofill').checked = Boolean(settings.inlineAutofillEnabled);
    $('inline-save').checked = Boolean(settings.inlineSavePromptEnabled);
    $('debug-logging').checked = Boolean(settings.debugLogging);
    setStatus('Settings loaded.');
}
$('save-options').addEventListener('click', async () => {
    const response = await callBackground({
        type: 'save-settings',
        settings: {
            serverBaseUrl: $('server-url').value,
            autoLockMinutes: Number.parseInt($('auto-lock').value || '15', 10) || 15,
            defaultMatchMode: $('match-mode').value,
            excludedDomains: $('excluded-domains')
                .value.split('\n')
                .map((value) => value.trim().toLowerCase())
                .filter(Boolean),
            warnOnHttp: $('warn-http').checked,
            allowManualHttpFill: $('allow-http-fill').checked,
            warnOnUntrustedIframe: $('warn-iframe').checked,
            pageLoadAutofill: $('page-load-autofill').checked,
            inlineAutofillEnabled: $('inline-autofill').checked,
            inlineSavePromptEnabled: $('inline-save').checked,
            debugLogging: $('debug-logging').checked,
        },
    });
    setStatus(response.ok ? 'Settings saved.' : response.error, !response.ok);
});
load().catch((error) => {
    setStatus(error instanceof Error ? error.message : String(error), true);
});
export {};
