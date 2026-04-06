export const DEFAULT_SETTINGS = {
    serverBaseUrl: '',
    autoLockMinutes: 15,
    excludedDomains: [],
    defaultMatchMode: 'base_domain',
    warnOnHttp: true,
    warnOnUntrustedIframe: true,
    allowManualHttpFill: false,
    pageLoadAutofill: false,
    inlineAutofillEnabled: true,
    inlineSavePromptEnabled: true,
    debugLogging: false,
};
const LOCAL_SETTINGS_KEY = 'rustyvault_settings_v2';
const LOCAL_SESSION_KEY = 'rustyvault_session_v2';
const LOCAL_GRANTED_ORIGINS_KEY = 'rustyvault_granted_origins_v1';
const SESSION_PENDING_KEY = 'rustyvault_pending_actions_v1';
const SESSION_LAST_FILLED_KEY = 'rustyvault_last_filled_v1';
const SESSION_GENERATED_KEY = 'rustyvault_generated_passwords_v1';
const SESSION_SUBMITTED_KEY = 'rustyvault_submitted_candidates_v1';
async function sessionArea() {
    return chrome.storage.session ?? chrome.storage.local;
}
export async function getSettings() {
    const stored = await chrome.storage.local.get(LOCAL_SETTINGS_KEY);
    return {
        ...DEFAULT_SETTINGS,
        ...(stored[LOCAL_SETTINGS_KEY] || {}),
    };
}
export async function setSettings(next) {
    const current = await getSettings();
    const merged = {
        ...current,
        ...next,
    };
    await chrome.storage.local.set({ [LOCAL_SETTINGS_KEY]: merged });
    return merged;
}
export async function getSession() {
    const stored = await chrome.storage.local.get(LOCAL_SESSION_KEY);
    return stored[LOCAL_SESSION_KEY] || null;
}
export async function setSession(session) {
    await chrome.storage.local.set({ [LOCAL_SESSION_KEY]: session });
}
export async function clearSession() {
    await chrome.storage.local.remove(LOCAL_SESSION_KEY);
}
export async function getGrantedOrigins() {
    const stored = await chrome.storage.local.get(LOCAL_GRANTED_ORIGINS_KEY);
    const values = Array.isArray(stored[LOCAL_GRANTED_ORIGINS_KEY])
        ? stored[LOCAL_GRANTED_ORIGINS_KEY]
        : [];
    return values.filter((value) => typeof value === 'string');
}
export async function setGrantedOrigins(origins) {
    const normalized = [...new Set(origins.filter(Boolean))].sort();
    await chrome.storage.local.set({ [LOCAL_GRANTED_ORIGINS_KEY]: normalized });
}
export async function getPendingActions() {
    const area = await sessionArea();
    const stored = await area.get(SESSION_PENDING_KEY);
    return stored[SESSION_PENDING_KEY] || {};
}
export async function savePendingAction(tabId, action) {
    const area = await sessionArea();
    const current = await getPendingActions();
    current[String(tabId)] = action;
    await area.set({ [SESSION_PENDING_KEY]: current });
}
export async function clearPendingAction(tabId) {
    const area = await sessionArea();
    const current = await getPendingActions();
    delete current[String(tabId)];
    await area.set({ [SESSION_PENDING_KEY]: current });
}
export async function getPendingAction(tabId) {
    const current = await getPendingActions();
    return current[String(tabId)] || null;
}
export async function getLastFilledMap() {
    const area = await sessionArea();
    const stored = await area.get(SESSION_LAST_FILLED_KEY);
    return stored[SESSION_LAST_FILLED_KEY] || {};
}
export async function setLastFilled(tabId, context) {
    const area = await sessionArea();
    const current = await getLastFilledMap();
    current[String(tabId)] = context;
    await area.set({ [SESSION_LAST_FILLED_KEY]: current });
}
export async function getLastFilled(tabId) {
    const current = await getLastFilledMap();
    return current[String(tabId)] || null;
}
export async function getGeneratedPasswords() {
    const area = await sessionArea();
    const stored = await area.get(SESSION_GENERATED_KEY);
    return stored[SESSION_GENERATED_KEY] || {};
}
export async function setGeneratedPassword(tabId, draft) {
    const area = await sessionArea();
    const current = await getGeneratedPasswords();
    current[String(tabId)] = draft;
    await area.set({ [SESSION_GENERATED_KEY]: current });
}
export async function getGeneratedPassword(tabId) {
    const current = await getGeneratedPasswords();
    return current[String(tabId)] || null;
}
export async function clearGeneratedPassword(tabId) {
    const area = await sessionArea();
    const current = await getGeneratedPasswords();
    delete current[String(tabId)];
    await area.set({ [SESSION_GENERATED_KEY]: current });
}
export async function getSubmittedCandidates() {
    const area = await sessionArea();
    const stored = await area.get(SESSION_SUBMITTED_KEY);
    return stored[SESSION_SUBMITTED_KEY] || {};
}
export async function setSubmittedCandidate(tabId, candidate) {
    const area = await sessionArea();
    const current = await getSubmittedCandidates();
    current[String(tabId)] = candidate;
    await area.set({ [SESSION_SUBMITTED_KEY]: current });
}
export async function getSubmittedCandidate(tabId) {
    const current = await getSubmittedCandidates();
    return current[String(tabId)] || null;
}
export async function clearSubmittedCandidate(tabId) {
    const area = await sessionArea();
    const current = await getSubmittedCandidates();
    delete current[String(tabId)];
    await area.set({ [SESSION_SUBMITTED_KEY]: current });
}
