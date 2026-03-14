function $(id) {
  return document.getElementById(id);
}

function setStatus(message, isError = false) {
  const statusEl = $('status');
  statusEl.textContent = message;
  statusEl.style.color = isError ? '#ffd4d8' : '';
}

async function load() {
  try {
    const response = await chrome.runtime.sendMessage({ type: 'get-popup-state' });
    if (!response?.ok) {
      setStatus(response?.error || 'Failed to load extension settings', true);
      return;
    }
    $('server-url').value = response.settings.serverBaseUrl || '';
    $('auto-lock').value = response.settings.autoLockMinutes;
    $('match-mode').value = response.settings.defaultMatchMode;
    $('excluded-domains').value = (response.settings.excludedDomains || []).join('\n');
    $('warn-http').checked = Boolean(response.settings.warnOnHttp);
    $('allow-http-fill').checked = Boolean(response.settings.allowManualHttpFill);
    $('warn-iframe').checked = Boolean(response.settings.warnOnUntrustedIframe);
    $('page-load-autofill').checked = Boolean(response.settings.pageLoadAutofill);
    setStatus('Settings loaded.');
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

$('save-options').addEventListener('click', async () => {
  try {
    const response = await chrome.runtime.sendMessage({
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
      },
    });
    setStatus(response?.ok ? 'Settings saved.' : response?.error || 'Failed to save settings', !response?.ok);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
});

load().catch((error) => {
  setStatus(error instanceof Error ? error.message : String(error), true);
});
