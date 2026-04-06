import type { BackgroundRequest, BackgroundResponse } from '../shared/messages.js';

function $(id: string) {
  return document.getElementById(id) as HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement | HTMLParagraphElement | null;
}

async function callBackground(message: BackgroundRequest): Promise<BackgroundResponse> {
  return chrome.runtime.sendMessage(message);
}

function setStatus(message: string, isError = false) {
  const status = $('status') as HTMLParagraphElement;
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
  ($('server-url') as HTMLInputElement).value = settings.serverBaseUrl || '';
  ($('auto-lock') as HTMLInputElement).value = String(settings.autoLockMinutes);
  ($('match-mode') as HTMLSelectElement).value = settings.defaultMatchMode;
  ($('excluded-domains') as HTMLTextAreaElement).value = settings.excludedDomains.join('\n');
  ($('warn-http') as HTMLInputElement).checked = Boolean(settings.warnOnHttp);
  ($('allow-http-fill') as HTMLInputElement).checked = Boolean(settings.allowManualHttpFill);
  ($('warn-iframe') as HTMLInputElement).checked = Boolean(settings.warnOnUntrustedIframe);
  ($('page-load-autofill') as HTMLInputElement).checked = Boolean(settings.pageLoadAutofill);
  ($('inline-autofill') as HTMLInputElement).checked = Boolean(settings.inlineAutofillEnabled);
  ($('inline-save') as HTMLInputElement).checked = Boolean(settings.inlineSavePromptEnabled);
  ($('debug-logging') as HTMLInputElement).checked = Boolean(settings.debugLogging);
  setStatus('Settings loaded.');
}

($('save-options') as HTMLButtonElement).addEventListener('click', async () => {
  const response = await callBackground({
    type: 'save-settings',
    settings: {
      serverBaseUrl: ($('server-url') as HTMLInputElement).value,
      autoLockMinutes: Number.parseInt(($('auto-lock') as HTMLInputElement).value || '15', 10) || 15,
      defaultMatchMode: ($('match-mode') as HTMLSelectElement).value,
      excludedDomains: ($('excluded-domains') as HTMLTextAreaElement)
        .value.split('\n')
        .map((value) => value.trim().toLowerCase())
        .filter(Boolean),
      warnOnHttp: ($('warn-http') as HTMLInputElement).checked,
      allowManualHttpFill: ($('allow-http-fill') as HTMLInputElement).checked,
      warnOnUntrustedIframe: ($('warn-iframe') as HTMLInputElement).checked,
      pageLoadAutofill: ($('page-load-autofill') as HTMLInputElement).checked,
      inlineAutofillEnabled: ($('inline-autofill') as HTMLInputElement).checked,
      inlineSavePromptEnabled: ($('inline-save') as HTMLInputElement).checked,
      debugLogging: ($('debug-logging') as HTMLInputElement).checked,
    },
  });
  setStatus(response.ok ? 'Settings saved.' : response.error, !response.ok);
});

load().catch((error) => {
  setStatus(error instanceof Error ? error.message : String(error), true);
});
