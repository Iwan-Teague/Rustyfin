export function extensionRuntime() {
    return chrome;
}
export async function getCurrentTab() {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab || null;
}
export function logIfEnabled(enabled, scope, ...parts) {
    if (!enabled) {
        return;
    }
    console.info(`[${scope}]`, ...parts);
}
