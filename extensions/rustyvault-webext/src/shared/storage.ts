import type {
  ExtensionSettings,
  GeneratedPasswordDraft,
  LastFilledContext,
  PendingAction,
  PopupDraft,
  RustyVaultSession,
} from './types.js';

export const DEFAULT_SETTINGS: ExtensionSettings = {
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
const SESSION_POPUP_DRAFT_KEY = 'rustyvault_popup_draft_v1';

const DEFAULT_POPUP_DRAFT: PopupDraft = {
  serverBaseUrlInput: '',
  pairingInput: '',
};

type SubmittedCandidate = {
  tabId: number;
  title: string;
  url: string;
  username: string;
  email: string;
  password: string;
  pageKind: string;
  pagePasswordCount: number;
  submittedAt: number;
};

async function sessionArea() {
  return chrome.storage.session ?? chrome.storage.local;
}

export async function getSettings(): Promise<ExtensionSettings> {
  const stored = await chrome.storage.local.get(LOCAL_SETTINGS_KEY);
  return {
    ...DEFAULT_SETTINGS,
    ...(stored[LOCAL_SETTINGS_KEY] || {}),
  };
}

export async function setSettings(
  next: Partial<ExtensionSettings>,
): Promise<ExtensionSettings> {
  const current = await getSettings();
  const merged = {
    ...current,
    ...next,
  };
  await chrome.storage.local.set({ [LOCAL_SETTINGS_KEY]: merged });
  return merged;
}

export async function getSession(): Promise<RustyVaultSession | null> {
  const stored = await chrome.storage.local.get(LOCAL_SESSION_KEY);
  return stored[LOCAL_SESSION_KEY] || null;
}

export async function setSession(session: RustyVaultSession): Promise<void> {
  await chrome.storage.local.set({ [LOCAL_SESSION_KEY]: session });
}

export async function clearSession(): Promise<void> {
  await chrome.storage.local.remove(LOCAL_SESSION_KEY);
}

export async function getGrantedOrigins(): Promise<string[]> {
  const stored = await chrome.storage.local.get(LOCAL_GRANTED_ORIGINS_KEY);
  const values = Array.isArray(stored[LOCAL_GRANTED_ORIGINS_KEY])
    ? stored[LOCAL_GRANTED_ORIGINS_KEY]
    : [];
  return values.filter((value: unknown): value is string => typeof value === 'string');
}

export async function setGrantedOrigins(origins: string[]): Promise<void> {
  const normalized = [...new Set(origins.filter(Boolean))].sort();
  await chrome.storage.local.set({ [LOCAL_GRANTED_ORIGINS_KEY]: normalized });
}

export async function getPendingActions(): Promise<Record<string, PendingAction>> {
  const area = await sessionArea();
  const stored = await area.get(SESSION_PENDING_KEY);
  return stored[SESSION_PENDING_KEY] || {};
}

export async function savePendingAction(tabId: number, action: PendingAction): Promise<void> {
  const area = await sessionArea();
  const current = await getPendingActions();
  current[String(tabId)] = action;
  await area.set({ [SESSION_PENDING_KEY]: current });
}

export async function clearPendingAction(tabId: number): Promise<void> {
  const area = await sessionArea();
  const current = await getPendingActions();
  delete current[String(tabId)];
  await area.set({ [SESSION_PENDING_KEY]: current });
}

export async function getPendingAction(tabId: number): Promise<PendingAction | null> {
  const current = await getPendingActions();
  return current[String(tabId)] || null;
}

export async function getLastFilledMap(): Promise<Record<string, LastFilledContext>> {
  const area = await sessionArea();
  const stored = await area.get(SESSION_LAST_FILLED_KEY);
  return stored[SESSION_LAST_FILLED_KEY] || {};
}

export async function setLastFilled(tabId: number, context: LastFilledContext): Promise<void> {
  const area = await sessionArea();
  const current = await getLastFilledMap();
  current[String(tabId)] = context;
  await area.set({ [SESSION_LAST_FILLED_KEY]: current });
}

export async function getLastFilled(tabId: number): Promise<LastFilledContext | null> {
  const current = await getLastFilledMap();
  return current[String(tabId)] || null;
}

export async function getGeneratedPasswords(): Promise<Record<string, GeneratedPasswordDraft>> {
  const area = await sessionArea();
  const stored = await area.get(SESSION_GENERATED_KEY);
  return stored[SESSION_GENERATED_KEY] || {};
}

export async function setGeneratedPassword(
  tabId: number,
  draft: GeneratedPasswordDraft,
): Promise<void> {
  const area = await sessionArea();
  const current = await getGeneratedPasswords();
  current[String(tabId)] = draft;
  await area.set({ [SESSION_GENERATED_KEY]: current });
}

export async function getGeneratedPassword(
  tabId: number,
): Promise<GeneratedPasswordDraft | null> {
  const current = await getGeneratedPasswords();
  return current[String(tabId)] || null;
}

export async function clearGeneratedPassword(tabId: number): Promise<void> {
  const area = await sessionArea();
  const current = await getGeneratedPasswords();
  delete current[String(tabId)];
  await area.set({ [SESSION_GENERATED_KEY]: current });
}

export async function getSubmittedCandidates(): Promise<Record<string, SubmittedCandidate>> {
  const area = await sessionArea();
  const stored = await area.get(SESSION_SUBMITTED_KEY);
  return stored[SESSION_SUBMITTED_KEY] || {};
}

export async function setSubmittedCandidate(
  tabId: number,
  candidate: SubmittedCandidate,
): Promise<void> {
  const area = await sessionArea();
  const current = await getSubmittedCandidates();
  current[String(tabId)] = candidate;
  await area.set({ [SESSION_SUBMITTED_KEY]: current });
}

export async function getSubmittedCandidate(
  tabId: number,
): Promise<SubmittedCandidate | null> {
  const current = await getSubmittedCandidates();
  return current[String(tabId)] || null;
}

export async function clearSubmittedCandidate(tabId: number): Promise<void> {
  const area = await sessionArea();
  const current = await getSubmittedCandidates();
  delete current[String(tabId)];
  await area.set({ [SESSION_SUBMITTED_KEY]: current });
}

export async function getPopupDraft(): Promise<PopupDraft> {
  const area = await sessionArea();
  const stored = await area.get(SESSION_POPUP_DRAFT_KEY);
  const draft = stored[SESSION_POPUP_DRAFT_KEY] || {};
  return {
    ...DEFAULT_POPUP_DRAFT,
    ...(draft || {}),
  };
}

export async function setPopupDraft(next: Partial<PopupDraft>): Promise<PopupDraft> {
  const area = await sessionArea();
  const current = await getPopupDraft();
  const merged = {
    ...current,
    ...next,
  };
  await area.set({ [SESSION_POPUP_DRAFT_KEY]: merged });
  return merged;
}
