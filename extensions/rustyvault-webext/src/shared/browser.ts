export function extensionRuntime() {
  return chrome;
}

export async function getCurrentTab() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  return tab || null;
}

export function logIfEnabled(enabled: boolean, scope: string, ...parts: unknown[]) {
  if (!enabled) {
    return;
  }
  console.info(`[${scope}]`, ...parts);
}
