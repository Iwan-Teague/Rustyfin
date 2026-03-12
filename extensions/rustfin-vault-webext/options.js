function $(id) {
  return document.getElementById(id);
}

async function load() {
  const response = await chrome.runtime.sendMessage({ type: 'get-popup-state' });
  if (!response.ok) {
    $('status').textContent = response.error || 'Failed to load extension settings';
    return;
  }
  $('server-url').value = response.settings.serverBaseUrl || '';
  $('auto-lock').value = response.settings.autoLockMinutes;
  $('match-mode').value = response.settings.defaultMatchMode;
  $('excluded-domains').value = (response.settings.excludedDomains || []).join('\n');
  $('warn-http').checked = Boolean(response.settings.warnOnHttp);
  $('warn-iframe').checked = Boolean(response.settings.warnOnUntrustedIframe);
  $('page-load-autofill').checked = Boolean(response.settings.pageLoadAutofill);
  $('status').textContent = 'Settings loaded.';
}

$('save-options').addEventListener('click', async () => {
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
      warnOnUntrustedIframe: $('warn-iframe').checked,
      pageLoadAutofill: $('page-load-autofill').checked,
    },
  });
  $('status').textContent = response.ok ? 'Settings saved.' : response.error;
});

load().catch((error) => {
  $('status').textContent = error instanceof Error ? error.message : String(error);
});
