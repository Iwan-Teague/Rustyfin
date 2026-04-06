export type MatchMode = 'exact' | 'host' | 'base_domain' | 'never';
export type PageKind = 'login' | 'signup' | 'change_password' | 'username_step' | 'unknown';
export type PendingActionKind = 'save_new' | 'update_existing' | 'add_uri';

export type ExtensionSettings = {
  serverBaseUrl: string;
  autoLockMinutes: number;
  excludedDomains: string[];
  defaultMatchMode: MatchMode;
  warnOnHttp: boolean;
  warnOnUntrustedIframe: boolean;
  allowManualHttpFill: boolean;
  pageLoadAutofill: boolean;
  inlineAutofillEnabled: boolean;
  inlineSavePromptEnabled: boolean;
  debugLogging: boolean;
};

export type RustyVaultSession = {
  access_token: string;
  refresh_token: string;
  access_expires_ts: number;
  refresh_expires_ts?: number;
  session_id?: string;
};

export type RustyVaultPreferences = {
  auto_lock_minutes: number;
  clipboard_clear_seconds: number;
  inline_save_prompt_enabled: boolean;
  inline_autofill_enabled: boolean;
  default_match_mode: MatchMode;
  warn_on_http: boolean;
  warn_on_untrusted_iframe: boolean;
  excluded_domains: string[];
  allow_manual_http_fill: boolean;
  password_generator_default_preset: 'memorable' | 'balanced' | 'maximum';
  password_generator_default_length: number;
  password_generator_include_uppercase: boolean;
  password_generator_include_lowercase: boolean;
  password_generator_include_numbers: boolean;
  password_generator_include_symbols: boolean;
  password_generator_exclude_ambiguous: boolean;
};

export type EncryptedRustyVaultSummary = {
  id: string;
  item_type: string;
  summary_nonce_hex: string;
  summary_ciphertext_hex: string;
  summary_version: number;
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
  deleted_ts?: number | null;
};

export type EncryptedRustyVaultItem = EncryptedRustyVaultSummary & {
  payload_nonce_hex: string;
  payload_ciphertext_hex: string;
  payload_version: number;
  key_version: number;
};

export type DecryptedRustyVaultSummary = {
  title: string;
  subtitle?: string;
  primary_uri?: string;
  username?: string;
  login_email?: string;
  favorite?: boolean;
};

export type DecryptedLoginItem = {
  id: string;
  title: string;
  username: string;
  login_email: string;
  password: string;
  notes: string;
  website_urls: string[];
  favorite: boolean;
  revision: number;
  created_ts: number;
  updated_ts: number;
};

export type MatchedVaultItem = {
  encrypted: EncryptedRustyVaultSummary;
  summary: DecryptedRustyVaultSummary;
};

export type PagePolicy = {
  url: string | null;
  topLevelUrl: string | null;
  hostname: string;
  topLevelHostname: string;
  isTopFrame: boolean;
  isHttp: boolean;
  isExcluded: boolean;
  sameOriginIframe: boolean;
  crossOriginIframe: boolean;
  canLookup: boolean;
  canManualFill: boolean;
  canSavePrompt: boolean;
  lookupBlockedReason: string | null;
  manualFillBlockedReason: string | null;
  savePromptBlockedReason: string | null;
  chips: string[];
};

export type PageContextPayload = {
  url: string;
  topLevelUrl: string;
  isTopFrame: boolean;
  hasPasswordField: boolean;
  pageKind: PageKind;
  frameId?: number;
};

export type CredentialCapturePayload = {
  title: string;
  url: string;
  username: string;
  email: string;
  password: string;
  pageKind: PageKind;
  pagePasswordCount: number;
};

export type PendingItemDraft = {
  title: string;
  username: string;
  email: string;
  password: string;
  url: string;
  pageKind: PageKind;
};

export type PendingAction = {
  kind: PendingActionKind;
  tabId: number;
  itemId?: string;
  message: string;
  draft: PendingItemDraft;
  createdAt: number;
};

export type GeneratedPasswordDraft = {
  tabId: number;
  password: string;
  url: string;
  pageKind: PageKind;
  createdAt: number;
};

export type LastFilledContext = {
  tabId: number;
  itemId: string;
  url: string;
  username: string;
  email: string;
  filledAt: number;
};

export type PopupState = {
  settings: ExtensionSettings;
  paired: boolean;
  unlocked: boolean;
  currentTab: { id: number; url?: string; title?: string } | null;
  pagePolicy: PagePolicy | null;
  matches: MatchedVaultItem[];
  pendingAction: PendingAction | null;
  sitePermissionGranted: boolean;
  currentOrigin: string | null;
  popupDraft: PopupDraft;
};

export type PopupDraft = {
  serverBaseUrlInput: string;
  pairingInput: string;
};
