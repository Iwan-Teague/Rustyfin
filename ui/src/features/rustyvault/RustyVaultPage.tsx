'use client';

import Link from 'next/link';
import { useCallback, useDeferredValue, useEffect, useState, startTransition } from 'react';
import { useRouter } from 'next/navigation';

import RfSwitch from '@/app/components/RfSwitch';
import SurfaceTabsBar from '@/app/components/SurfaceTabsBar';
import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import {
  generatePassword,
  presetOptions,
  type PasswordGeneratorOptions,
  type PasswordGeneratorPreset,
} from '@/features/rustyvault/passwordGenerator';
import {
  bootstrapRustyVaultKeys,
  buildLookupHashesForUrl,
  decryptRustyVaultItem,
  decryptRustyVaultSummary,
  encryptRustyVaultItem,
  getRustyVaultCryptoReadiness,
  normalizeWebsiteUrl,
  rewrapRustyVaultKey,
  type RustyVaultCryptoReadiness,
  type RustyVaultItem,
  type RustyVaultItemType,
  type RustyVaultSummaryPlaintext,
  type RustyVaultUnlockedContext,
} from '@/features/rustyvault/crypto';
import {
  bootstrapRustyVault,
  challengeRustyVaultProtectedAction,
  createRustyVaultDeviceSession,
  createRustyVaultItem,
  deleteRustyVaultItem,
  destroyRustyVault,
  exportRustyVault,
  getRustyVaultConfig,
  getRustyVaultItem,
  importRustyVaultBitwardenCiphertexts,
  listRustyVaultDeviceSessions,
  listRustyVaultItems,
  lookupRustyVaultItems,
  replaceRustyVaultItem,
  rekeyRustyVault,
  revokeOtherRustyVaultSessions,
  revokeRustyVaultDeviceSession,
  type EncryptedRustyVaultItemSummary,
  type RustyVaultConfigResponse,
  type RustyVaultDeviceSessionResponse,
  type RustyVaultPairingCodeResponse,
  type RustyVaultUriMatchMode,
} from '@/features/rustyvault/api';
import {
  clearRustyVaultSession,
  ensureRustyVaultWebSession,
  refreshStoredRustyVaultSession,
  readRustyVaultSession,
  type StoredRustyVaultSession,
} from '@/features/rustyvault/session';
import {
  defaultRustyVaultPreferences,
  getMyRustyVaultPreferences,
  updateMyRustyVaultPreferences,
  type RustyVaultPreferences,
} from '@/features/rustyvault/preferences';

type DecryptedSummaryRow = {
  encrypted: EncryptedRustyVaultItemSummary;
  summary: RustyVaultSummaryPlaintext;
};

type EditorState = {
  id: string;
  item_type: RustyVaultItemType;
  title: string;
  username: string;
  login_email: string;
  password: string;
  website_urls: string;
  cardholder_name: string;
  card_number: string;
  expiry_month: string;
  expiry_year: string;
  security_code: string;
  issuer_name: string;
  full_name: string;
  passport_number: string;
  nationality: string;
  issuing_country: string;
  birth_date: string;
  expiry_date: string;
  notes: string;
  favorite: boolean;
  revision: number;
  created_ts: number;
};

type VaultWorkspaceTab = 'credentials' | 'settings' | 'generator' | 'extension';

const ITEM_TYPE_OPTIONS: Array<{
  value: RustyVaultItemType;
  label: string;
  description: string;
}> = [
  {
    value: 'login',
    label: 'Login',
    description: 'Usernames, emails, passwords, and website URLs.',
  },
  {
    value: 'credit_card',
    label: 'Credit card',
    description: 'Cardholder, number, expiry, and security code.',
  },
  {
    value: 'passport',
    label: 'Passport',
    description: 'Document details, expiry dates, and nationality.',
  },
  {
    value: 'secure_note',
    label: 'Secure note',
    description: 'Free-form encrypted notes for anything else important.',
  },
];

function nowTs() {
  return Math.floor(Date.now() / 1000);
}

function parseVaultItemType(value: string): RustyVaultItemType {
  return ITEM_TYPE_OPTIONS.some((option) => option.value === value)
    ? (value as RustyVaultItemType)
    : 'login';
}

function defaultEditorState(itemType: RustyVaultItemType = 'login'): EditorState {
  return {
    id: '',
    item_type: itemType,
    title: '',
    username: '',
    login_email: '',
    password: '',
    website_urls: '',
    cardholder_name: '',
    card_number: '',
    expiry_month: '',
    expiry_year: '',
    security_code: '',
    issuer_name: '',
    full_name: '',
    passport_number: '',
    nationality: '',
    issuing_country: '',
    birth_date: '',
    expiry_date: '',
    notes: '',
    favorite: false,
    revision: 1,
    created_ts: nowTs(),
  };
}

function buildItemFromEditor(editor: EditorState): RustyVaultItem {
  const common = {
    id: editor.id || crypto.randomUUID(),
    title: editor.title.trim(),
    notes: editor.notes,
    favorite: editor.favorite,
    revision: editor.revision,
    created_ts: editor.created_ts,
    updated_ts: nowTs(),
  };

  switch (editor.item_type) {
    case 'credit_card':
      return {
        ...common,
        item_type: 'credit_card',
        cardholder_name: editor.cardholder_name.trim(),
        card_number: editor.card_number.trim(),
        expiry_month: editor.expiry_month.trim(),
        expiry_year: editor.expiry_year.trim(),
        security_code: editor.security_code.trim(),
        issuer_name: editor.issuer_name.trim(),
      };
    case 'passport':
      return {
        ...common,
        item_type: 'passport',
        full_name: editor.full_name.trim(),
        passport_number: editor.passport_number.trim(),
        nationality: editor.nationality.trim(),
        issuing_country: editor.issuing_country.trim(),
        birth_date: editor.birth_date.trim(),
        expiry_date: editor.expiry_date.trim(),
      };
    case 'secure_note':
      return {
        ...common,
        item_type: 'secure_note',
      };
    case 'login':
    default:
      return {
        ...common,
        item_type: 'login',
        username: editor.username.trim(),
        login_email: editor.login_email.trim(),
        password: editor.password,
        website_urls: editor.website_urls
          .split('\n')
          .map((value) => value.trim())
          .filter(Boolean),
      };
  }
}

function buildEditorFromItem(item: RustyVaultItem): EditorState {
  const base = defaultEditorState(item.item_type);
  switch (item.item_type) {
    case 'credit_card':
      return {
        ...base,
        id: item.id,
        title: item.title,
        cardholder_name: item.cardholder_name,
        card_number: item.card_number,
        expiry_month: item.expiry_month,
        expiry_year: item.expiry_year,
        security_code: item.security_code,
        issuer_name: item.issuer_name,
        notes: item.notes,
        favorite: item.favorite,
        revision: item.revision,
        created_ts: item.created_ts,
      };
    case 'passport':
      return {
        ...base,
        id: item.id,
        title: item.title,
        full_name: item.full_name,
        passport_number: item.passport_number,
        nationality: item.nationality,
        issuing_country: item.issuing_country,
        birth_date: item.birth_date,
        expiry_date: item.expiry_date,
        notes: item.notes,
        favorite: item.favorite,
        revision: item.revision,
        created_ts: item.created_ts,
      };
    case 'secure_note':
      return {
        ...base,
        id: item.id,
        title: item.title,
        notes: item.notes,
        favorite: item.favorite,
        revision: item.revision,
        created_ts: item.created_ts,
      };
    case 'login':
    default:
      return {
        ...base,
        id: item.id,
        title: item.title,
        username: item.username,
        login_email: item.login_email,
        password: item.password,
        website_urls: item.website_urls.join('\n'),
        notes: item.notes,
        favorite: item.favorite,
        revision: item.revision,
        created_ts: item.created_ts,
      };
  }
}

function formatTimestamp(value?: number | null) {
  if (!value) return 'Never';
  return new Date(value * 1000).toLocaleString();
}

function maskedSecret(value: string) {
  if (!value) return 'Nothing saved';
  return '•'.repeat(Math.max(8, Math.min(20, value.length)));
}

function normalizeMode(value: string): RustyVaultUriMatchMode {
  return value === 'exact' || value === 'host' || value === 'never' ? value : 'base_domain';
}

function isRustyVaultUnavailableError(message: string | null) {
  if (!message) return false;
  const normalized = message.toLowerCase();
  return (
    normalized.includes('rustyvault is unavailable on this host') ||
    normalized.includes('rustyvault is disabled on this host') ||
    normalized.includes('rustyvault is disabled in this build') ||
    normalized.includes('run database migrations to enable it')
  );
}

function parseBitwardenImport(text: string): RustyVaultItem[] {
  const parsed = JSON.parse(text) as {
    items?: Array<{
      type?: number;
      name?: string;
      notes?: string;
      favorite?: boolean;
      login?: {
        username?: string;
        password?: string;
        uris?: Array<{ uri?: string | null }>;
      };
    }>;
  };
  const items = Array.isArray(parsed.items) ? parsed.items : [];
  const imported: RustyVaultItem[] = [];
  for (const entry of items) {
    if (entry.type !== 1 || !entry.login?.password) {
      continue;
    }
    const urls = (entry.login.uris ?? [])
      .map((uri) => uri.uri?.trim() || '')
      .filter(Boolean);
    const createdTs = nowTs();
    imported.push({
      item_type: 'login',
      id: crypto.randomUUID(),
      title: entry.name?.trim() || normalizeWebsiteUrl(urls[0] ?? '')?.hostname || 'Imported login',
      username: entry.login.username?.trim() || '',
      login_email: entry.login.username?.trim() || '',
      password: entry.login.password,
      notes: entry.notes?.trim() || '',
      website_urls: urls,
      favorite: Boolean(entry.favorite),
      revision: 1,
      created_ts: createdTs,
      updated_ts: createdTs,
    });
  }
  return imported;
}

function downloadJson(filename: string, value: unknown) {
  const blob = new Blob([JSON.stringify(value, null, 2)], {
    type: 'application/json;charset=utf-8',
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

async function writeClipboardWithTimeout(value: string, clearAfterSeconds: number) {
  await navigator.clipboard.writeText(value);
  if (clearAfterSeconds <= 0) {
    return;
  }
  window.setTimeout(async () => {
    try {
      const current = await navigator.clipboard.readText();
      if (current === value) {
        await navigator.clipboard.writeText('');
      }
    } catch {
      // ignore clipboard cleanup failures
    }
  }, clearAfterSeconds * 1000);
}

function itemTypeLabel(itemType: RustyVaultItemType) {
  switch (itemType) {
    case 'credit_card':
      return 'Credit card';
    case 'passport':
      return 'Passport';
    case 'secure_note':
      return 'Secure note';
    case 'login':
    default:
      return 'Login';
  }
}

function itemTypeDescription(itemType: RustyVaultItemType) {
  return ITEM_TYPE_OPTIONS.find((option) => option.value === itemType)?.description ?? '';
}

function itemPrimaryCopyValue(item: RustyVaultItem) {
  switch (item.item_type) {
    case 'credit_card':
      return item.card_number.trim();
    case 'passport':
      return item.passport_number.trim();
    case 'secure_note':
      return item.notes.trim();
    case 'login':
    default:
      return item.password;
  }
}

function itemPrimaryCopyLabel(item: RustyVaultItem) {
  switch (item.item_type) {
    case 'credit_card':
      return 'Copy card number';
    case 'passport':
      return 'Copy passport number';
    case 'secure_note':
      return 'Copy note';
    case 'login':
    default:
      return 'Copy password';
  }
}

function itemPrimaryPreview(item: RustyVaultItem, reveal: boolean) {
  const value = itemPrimaryCopyValue(item);
  if (!value) {
    return item.item_type === 'secure_note' ? 'No note body saved' : 'Nothing saved';
  }
  if (reveal) {
    return value;
  }
  if (item.item_type === 'secure_note') {
    return value.length > 120 ? `${value.slice(0, 120)}…` : value;
  }
  return maskedSecret(value.replace(/\s+/g, ''));
}

function toastClassName(tone: 'success' | 'error' | 'warning') {
  switch (tone) {
    case 'error':
      return 'border border-[var(--danger)]/35 bg-[var(--danger)]/12 text-white';
    case 'warning':
      return 'border border-amber-500/35 bg-amber-500/12 text-amber-50';
    case 'success':
    default:
      return 'border border-emerald-500/35 bg-emerald-500/12 text-emerald-50';
  }
}

export default function RustyVaultPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [cryptoReadiness, setCryptoReadiness] = useState<RustyVaultCryptoReadiness | null>(null);
  const [config, setConfig] = useState<RustyVaultConfigResponse | null>(null);
  const [prefs, setPrefs] = useState<RustyVaultPreferences>(defaultRustyVaultPreferences());
  const [rustyVaultSession, setVaultSession] = useState<StoredRustyVaultSession | null>(
    readRustyVaultSession(),
  );
  const [unlocked, setUnlocked] = useState<RustyVaultUnlockedContext | null>(null);
  const [masterPassword, setMasterPassword] = useState('');
  const [confirmMasterPassword, setConfirmMasterPassword] = useState('');
  const [rows, setRows] = useState<DecryptedSummaryRow[]>([]);
  const [selectedItem, setSelectedItem] = useState<RustyVaultItem | null>(null);
  const [editor, setEditor] = useState<EditorState>(defaultEditorState());
  const [newItemType, setNewItemType] = useState<RustyVaultItemType>('login');
  const [showSensitive, setShowSensitive] = useState(false);
  const [editingExisting, setEditingExisting] = useState(false);
  const [search, setSearch] = useState('');
  const deferredSearch = useDeferredValue(search);
  const [generatorPreset, setGeneratorPreset] =
    useState<PasswordGeneratorPreset>('balanced');
  const [generatorOptions, setGeneratorOptions] = useState<PasswordGeneratorOptions>(
    presetOptions('balanced'),
  );
  const [generatedPassword, setGeneratedPassword] = useState('');
  const [securityPassword, setSecurityPassword] = useState('');
  const [currentRustyVaultPassword, setCurrentVaultPassword] = useState('');
  const [newMasterPassword, setNewMasterPassword] = useState('');
  const [newMasterPasswordConfirm, setNewMasterPasswordConfirm] = useState('');
  const [extensionPairing, setExtensionPairing] =
    useState<RustyVaultPairingCodeResponse | null>(null);
  const [deviceSessions, setDeviceSessions] = useState<RustyVaultDeviceSessionResponse[]>([]);
  const [importFile, setImportFile] = useState<File | null>(null);
  const [importClearExisting, setImportClearExisting] = useState(true);
  const [lookupUrl, setLookupUrl] = useState('');
  const [lookupResultIds, setLookupResultIds] = useState<string[]>([]);
  const [excludedDomainsInput, setExcludedDomainsInput] = useState('');
  const [loadingState, setLoadingState] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeWorkspaceTab, setActiveWorkspaceTab] =
    useState<VaultWorkspaceTab>('credentials');
  const [vaultView, setVaultView] = useState<'index' | 'prompt' | 'workspace'>('index');

  const filteredRows = [...rows]
    .filter((row) => {
      const needle = deferredSearch.trim().toLowerCase();
      if (!needle) return true;
      return [
        row.summary.title,
        row.summary.subtitle,
        row.summary.primary_uri,
        row.summary.username,
        row.summary.login_email,
        itemTypeLabel(row.summary.item_type),
      ]
        .join(' ')
        .toLowerCase()
        .includes(needle);
    })
    .sort((left, right) => {
      if (left.encrypted.favorite !== right.encrypted.favorite) {
        return left.encrypted.favorite ? -1 : 1;
      }
      return (right.encrypted.updated_ts ?? 0) - (left.encrypted.updated_ts ?? 0);
    });

  const currentMatchMode = normalizeMode(prefs.default_match_mode);
  const cryptoReady = cryptoReadiness?.ready === true;
  const canSubmitVaultPrompt = config?.enabled
    ? masterPassword.trim().length > 0
    : masterPassword.length > 0 &&
      confirmMasterPassword.length > 0 &&
      masterPassword === confirmMasterPassword;

  async function withRustyVaultAccess<T>(
    callback: (accessToken: string) => Promise<T>,
  ): Promise<T> {
    const current = await ensureRustyVaultWebSession();
    setVaultSession(current);
    try {
      return await callback(current.access_token);
    } catch (err) {
      const messageText = err instanceof Error ? err.message : String(err);
      if (messageText.includes('401') || messageText.toLowerCase().includes('unauthorized')) {
        const refreshed = await refreshStoredRustyVaultSession(current);
        setVaultSession(refreshed);
        return callback(refreshed.access_token);
      }
      throw err;
    }
  }

  async function reloadRustyVaultChrome() {
    const session = await ensureRustyVaultWebSession();
    setVaultSession(session);
    const [nextConfig, nextPrefs, nextDevices] = await Promise.all([
      getRustyVaultConfig(session.access_token),
      getMyRustyVaultPreferences(session.access_token),
      listRustyVaultDeviceSessions(session.access_token).catch(() => []),
    ]);
    setConfig(nextConfig);
    setPrefs(nextPrefs);
    setExcludedDomainsInput(nextPrefs.excluded_domains.join('\n'));
    setDeviceSessions(nextDevices);
  }

  async function loadItems(unlockedContext: RustyVaultUnlockedContext) {
    const list = await withRustyVaultAccess((accessToken) =>
      listRustyVaultItems(accessToken, { limit: 100 }),
    );
    const decrypted = await Promise.all(
      list.items.map(async (encrypted) => ({
        encrypted,
        summary: await decryptRustyVaultSummary(unlockedContext, encrypted),
      })),
    );
    startTransition(() => {
      setRows(decrypted);
    });
  }

  async function loadItem(itemId: string, unlockedContext = unlocked) {
    if (!unlockedContext) return;
    const encrypted = await withRustyVaultAccess((accessToken) =>
      getRustyVaultItem(accessToken, itemId),
    );
    const decrypted = await decryptRustyVaultItem(unlockedContext, encrypted);
    setSelectedItem(decrypted);
    setEditor(buildEditorFromItem(decrypted));
    setEditingExisting(true);
    setShowSensitive(false);
  }

  async function bootstrapFreshVault() {
    if (!me) return;
    if (!cryptoReady) {
      throw new Error(
        cryptoReadiness?.message ?? 'This browser is not ready for vault cryptography yet',
      );
    }
    if (!masterPassword || masterPassword !== confirmMasterPassword) {
      throw new Error('Enter and confirm the same new vault master password');
    }
    const unlockedContext = await bootstrapRustyVaultKeys(masterPassword, me.id);
    const persistedConfig = await withRustyVaultAccess((accessToken) =>
      bootstrapRustyVault({ wrapped_key: unlockedContext.wrapped_key }, accessToken),
    );
    if (!persistedConfig.enabled || !persistedConfig.active_wrapped_key) {
      throw new Error('Vault creation did not persist on the server');
    }
    setConfig(persistedConfig);
    setUnlocked(unlockedContext);
    setVaultView('workspace');
    setActiveWorkspaceTab('credentials');
    setRows([]);
    setSelectedItem(null);
    setEditingExisting(false);
    setEditor(defaultEditorState('login'));
    setMasterPassword('');
    setConfirmMasterPassword('');
    setShowSensitive(false);
    setMessage('Vault created and unlocked on this device.');
  }

  async function unlockExistingVault() {
    if (!me || !config?.active_wrapped_key) {
      throw new Error('Vault is not ready to unlock');
    }
    if (!cryptoReady) {
      throw new Error(
        cryptoReadiness?.message ?? 'This browser is not ready for vault cryptography yet',
      );
    }
    const unlockedContext = await import('@/features/rustyvault/crypto').then(
      ({ unlockRustyVault }) =>
        unlockRustyVault(masterPassword, me.id, config.active_wrapped_key!),
    );
    setUnlocked(unlockedContext);
    setVaultView('workspace');
    setActiveWorkspaceTab('credentials');
    setSelectedItem(null);
    setEditingExisting(false);
    setEditor(defaultEditorState('login'));
    setShowSensitive(false);
    setMasterPassword('');
    setConfirmMasterPassword('');
    await loadItems(unlockedContext);
    setMessage('Vault unlocked.');
  }

  async function refreshLookup() {
    if (!unlocked || !lookupUrl.trim()) {
      setLookupResultIds([]);
      return;
    }
    const hashes = await buildLookupHashesForUrl(unlocked.index_key, lookupUrl, currentMatchMode);
    const response = await withRustyVaultAccess((accessToken) =>
      lookupRustyVaultItems(accessToken, hashes),
    );
    setLookupResultIds(response.items.map((item) => item.id));
  }

  const resetAutoLock = useCallback(() => {
    if (!unlocked) return;
    if (prefs.auto_lock_minutes <= 0) return;
    const rustyVaultWindow = window as Window & { __rustyVaultAutoLock?: number };
    window.clearTimeout(rustyVaultWindow.__rustyVaultAutoLock);
    rustyVaultWindow.__rustyVaultAutoLock = window.setTimeout(() => {
      setUnlocked(null);
      setVaultView('index');
      setRows([]);
      setSelectedItem(null);
      setEditingExisting(false);
      setEditor(defaultEditorState());
      setShowSensitive(false);
      setMessage('Vault locked after inactivity.');
    }, prefs.auto_lock_minutes * 60 * 1000);
  }, [prefs.auto_lock_minutes, unlocked]);

  useEffect(() => {
    getRustyVaultCryptoReadiness().then(setCryptoReadiness);
  }, []);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (authLoading || !me) return;
    let cancelled = false;
    setLoadingState(true);
    reloadRustyVaultChrome()
      .catch((err) => {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to initialize the web vault session'));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoadingState(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [authLoading, me]);

  useEffect(() => {
    if (!unlocked) return;
    if (prefs.auto_lock_minutes <= 0) return;
    const rustyVaultWindow = window as Window & { __rustyVaultAutoLock?: number };
    const onActivity = () => {
      resetAutoLock();
    };
    resetAutoLock();
    window.addEventListener('pointerdown', onActivity);
    window.addEventListener('keydown', onActivity);
    window.addEventListener('visibilitychange', onActivity);
    return () => {
      window.clearTimeout(rustyVaultWindow.__rustyVaultAutoLock);
      window.removeEventListener('pointerdown', onActivity);
      window.removeEventListener('keydown', onActivity);
      window.removeEventListener('visibilitychange', onActivity);
    };
  }, [prefs.auto_lock_minutes, resetAutoLock, unlocked]);

  useEffect(() => {
    setGeneratorOptions(presetOptions(generatorPreset));
  }, [generatorPreset]);

  useEffect(() => {
    if (unlocked) {
      setVaultView('workspace');
    } else if (vaultView === 'workspace') {
      setVaultView('index');
    }
  }, [unlocked, vaultView]);

  useEffect(() => {
    if (!message) return;
    const timeout = window.setTimeout(() => setMessage(null), 4200);
    return () => window.clearTimeout(timeout);
  }, [message]);

  useEffect(() => {
    if (!error || isRustyVaultUnavailableError(error)) return;
    const timeout = window.setTimeout(() => setError(null), 6500);
    return () => window.clearTimeout(timeout);
  }, [error]);

  async function runAction<T>(label: string, callback: () => Promise<T>) {
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      const value = await callback();
      setMessage(label);
      return value;
    } catch (err) {
      setError(clientErrorMessage(err, 'Vault action failed'));
      throw err;
    } finally {
      setSaving(false);
    }
  }

  function updateEditorField<K extends keyof EditorState>(key: K, value: EditorState[K]) {
    setEditor((current) => ({ ...current, [key]: value } as EditorState));
  }

  function startNewDraft(itemType: RustyVaultItemType) {
    setSelectedItem(null);
    setEditingExisting(false);
    setNewItemType(itemType);
    setEditor({
      ...defaultEditorState(itemType),
      password: itemType === 'login' ? generatedPassword : '',
    });
    setShowSensitive(itemType === 'login' && Boolean(generatedPassword));
    setActiveWorkspaceTab('credentials');
  }

  async function saveEditorItem() {
    if (!unlocked) {
      throw new Error('Unlock the vault before saving');
    }
    const item = buildItemFromEditor(editor);
    if (!item.title) {
      throw new Error('Give this item a title');
    }
    switch (item.item_type) {
      case 'login':
        if (!item.password) {
          throw new Error('Enter a password or use the generator');
        }
        break;
      case 'credit_card':
        if (!item.card_number) {
          throw new Error('Enter the card number');
        }
        break;
      case 'passport':
        if (!item.passport_number) {
          throw new Error('Enter the passport number');
        }
        break;
      case 'secure_note':
        if (!item.notes.trim()) {
          throw new Error('Write something in the secure note');
        }
        break;
      default:
        break;
    }
    const encrypted = await encryptRustyVaultItem(unlocked, item, currentMatchMode);
    await withRustyVaultAccess((accessToken) =>
      editingExisting
        ? replaceRustyVaultItem(accessToken, item.id, encrypted)
        : createRustyVaultItem(accessToken, encrypted),
    );
    await loadItems(unlocked);
    setSelectedItem(item);
    setEditor(buildEditorFromItem(item));
    setEditingExisting(true);
    resetAutoLock();
  }

  async function deleteSelectedItem() {
    if (!selectedItem) {
      throw new Error('Select a vault item first');
    }
    if (!unlocked) {
      throw new Error('Unlock the vault first');
    }
    const target = findDataDeleteTarget('data-vault-item-id', selectedItem.id);
    await playTelegramDeleteAnimation(target);
    await withRustyVaultAccess((accessToken) =>
      deleteRustyVaultItem(accessToken, selectedItem.id),
    );
    setSelectedItem(null);
    setEditor(defaultEditorState(newItemType));
    setEditingExisting(false);
    await loadItems(unlocked);
  }

  async function copySelectedValue() {
    if (!selectedItem) {
      throw new Error('Select a vault item first');
    }
    const value = itemPrimaryCopyValue(selectedItem);
    if (!value) {
      throw new Error('No copyable value is available for this item');
    }
    await writeClipboardWithTimeout(value, prefs.clipboard_clear_seconds);
    setMessage(`${itemPrimaryCopyLabel(selectedItem)} copied.`);
  }

  async function savePreferences() {
    const nextPrefs: RustyVaultPreferences = {
      ...prefs,
      default_match_mode: currentMatchMode,
      excluded_domains: excludedDomainsInput
        .split('\n')
        .map((value) => value.trim().toLowerCase())
        .filter(Boolean),
    };
    const updated = await withRustyVaultAccess((accessToken) =>
      updateMyRustyVaultPreferences(nextPrefs, accessToken),
    );
    setPrefs(updated);
    setExcludedDomainsInput(updated.excluded_domains.join('\n'));
  }

  async function pairExtension() {
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password to pair a new device');
    }
    const response = await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'approve_device',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      const created = await createRustyVaultDeviceSession(
        {
          client_kind: 'browser_extension',
          device_name: 'Rustyfin Browser Extension',
          device_platform: 'webext',
          protected_action_token: challenge.action_token,
        },
        accessToken,
      );
      if (!created.pairing) {
        throw new Error('No pairing code was returned');
      }
      return created.pairing;
    });
    setExtensionPairing(response);
    await reloadRustyVaultChrome();
  }

  async function rekeyMasterPassword() {
    if (!unlocked || !me || !config?.active_wrapped_key) {
      throw new Error('Unlock the vault before rotating the master password');
    }
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password first');
    }
    if (!currentRustyVaultPassword) {
      throw new Error('Enter the current vault master password first');
    }
    if (!newMasterPassword || newMasterPassword !== newMasterPasswordConfirm) {
      throw new Error('Enter and confirm the new vault master password');
    }
    await import('@/features/rustyvault/crypto').then(({ unlockRustyVault }) =>
      unlockRustyVault(currentRustyVaultPassword, me.id, config.active_wrapped_key!),
    );

    const allEncryptedItems = await Promise.all(
      rows.map((row) =>
        withRustyVaultAccess((accessToken) => getRustyVaultItem(accessToken, row.encrypted.id)),
      ),
    );
    const decryptedItems = await Promise.all(
      allEncryptedItems.map((item) => decryptRustyVaultItem(unlocked, item)),
    );
    const nextUnlocked = await rewrapRustyVaultKey(
      unlocked,
      newMasterPassword,
      (config.active_wrapped_key?.key_version ?? 0) + 1,
    );
    const reencryptedItems = await Promise.all(
      decryptedItems.map((item) =>
        encryptRustyVaultItem(
          nextUnlocked,
          {
            ...item,
            revision: item.revision + 1,
            updated_ts: nowTs(),
          },
          currentMatchMode,
        ),
      ),
    );

    await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'rekey',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      await rekeyRustyVault(accessToken, challenge.action_token, {
        wrapped_key: nextUnlocked.wrapped_key,
      });
      await Promise.all(
        reencryptedItems.map((item) => replaceRustyVaultItem(accessToken, item.id, item)),
      );
    });

    setUnlocked(nextUnlocked);
    setConfig((current) =>
      current
        ? {
            ...current,
            active_wrapped_key: nextUnlocked.wrapped_key,
          }
        : current,
    );
    setMasterPassword(newMasterPassword);
    setCurrentVaultPassword('');
    setNewMasterPassword('');
    setNewMasterPasswordConfirm('');
    await loadItems(nextUnlocked);
  }

  async function exportCurrentVault() {
    if (!unlocked || !me) {
      throw new Error('Unlock the vault before exporting');
    }
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password first');
    }
    const response = await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'export',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      return exportRustyVault(accessToken, challenge.action_token);
    });
    const decrypted = await Promise.all(
      response.items.map((item) => decryptRustyVaultItem(unlocked, item)),
    );
    downloadJson(`rustyfin-vault-export-${new Date().toISOString().slice(0, 10)}.json`, {
      exported_at: new Date().toISOString(),
      rustyvault_schema_version: response.config.schema_version,
      items: decrypted,
    });
  }

  async function importBitwardenJson() {
    if (!importFile || !unlocked) {
      throw new Error('Choose a Bitwarden export file first');
    }
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password first');
    }
    const text = await importFile.text();
    const importedItems = parseBitwardenImport(text);
    const ciphertexts = await Promise.all(
      importedItems.map((item) => encryptRustyVaultItem(unlocked, item, currentMatchMode)),
    );
    await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'import_overwrite',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      await importRustyVaultBitwardenCiphertexts(accessToken, {
        protected_action_token: challenge.action_token,
        clear_existing: importClearExisting,
        items: ciphertexts,
      });
    });
    await loadItems(unlocked);
    await reloadRustyVaultChrome();
  }

  async function revokeOtherSessions() {
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password first');
    }
    await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'revoke_other_sessions',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      await revokeOtherRustyVaultSessions(challenge.action_token, accessToken);
    });
    await reloadRustyVaultChrome();
  }

  async function destroyCurrentVault() {
    if (!securityPassword.trim()) {
      throw new Error('Enter your Rustyfin account password first');
    }
    await withRustyVaultAccess(async (accessToken) => {
      const challenge = await challengeRustyVaultProtectedAction({
        action_kind: 'destroy_rustyvault',
        current_password: securityPassword,
        vaultAccessToken: accessToken,
      });
      await destroyRustyVault(accessToken, challenge.action_token);
    });
    clearRustyVaultSession();
    setVaultSession(null);
    setUnlocked(null);
    setVaultView('index');
    setActiveWorkspaceTab('credentials');
    setRows([]);
    setSelectedItem(null);
    setEditor(defaultEditorState());
    setEditingExisting(false);
    const freshSession = await ensureRustyVaultWebSession();
    setVaultSession(freshSession);
    setConfig(await getRustyVaultConfig(freshSession.access_token));
    setDeviceSessions([]);
  }

  const workspaceBadges = [
    unlocked ? 'Unlocked' : config?.enabled ? 'Locked' : 'Not set up',
    `${config?.item_count ?? 0} items`,
    rustyVaultSession ? 'Web session active' : 'No web session',
  ];

  const workspaceContentClassName =
    activeWorkspaceTab === 'credentials'
      ? 'grid grid-cols-1 gap-7 xl:grid-cols-[0.92fr_1.08fr]'
      : activeWorkspaceTab === 'settings'
        ? 'grid grid-cols-1 gap-7 xl:grid-cols-[0.95fr_1.05fr]'
        : activeWorkspaceTab === 'generator'
          ? 'mx-auto max-w-5xl'
          : 'grid grid-cols-1 gap-7 xl:grid-cols-[0.9fr_1.1fr]';
  const vaultFieldClassName = 'rf-flat-input px-4 py-3';
  const vaultSectionClassName = 'space-y-5 border-t border-white/8 pt-5';
  const vaultSubsectionClassName = 'space-y-3 border-t border-white/8 pt-4';
  const currentVaultLabel = config?.enabled ? 'Personal Vault' : 'Set up Vault';
  const currentVaultDescription = config?.enabled
    ? 'Client-side encrypted credentials, cards, passports, and secure notes for this Rustyfin account.'
    : 'Create an encrypted vault before saving credentials or personal records.';

  const toastItems = [
    error && !isRustyVaultUnavailableError(error)
      ? { key: 'error', tone: 'error' as const, text: error }
      : null,
    cryptoReadiness?.mode === 'portable-fallback'
      ? { key: 'fallback', tone: 'warning' as const, text: cryptoReadiness.message }
      : null,
    message ? { key: 'message', tone: 'success' as const, text: message } : null,
  ].filter(
    (
      toast,
    ): toast is {
      key: string;
      tone: 'success' | 'error' | 'warning';
      text: string;
    } => Boolean(toast),
  );

  const toastStack =
    toastItems.length > 0 ? (
      <div className="pointer-events-none fixed bottom-5 left-4 right-4 z-50 flex flex-col gap-3 sm:left-auto sm:right-6 sm:w-[25rem]">
        {toastItems.map((toast) => (
          <div
            key={toast.key}
            className={`pointer-events-auto rounded-2xl px-4 py-3 text-sm shadow-[0_18px_48px_rgba(0,0,0,0.24)] backdrop-blur ${toastClassName(
              toast.tone,
            )}`}
          >
            {toast.text}
          </div>
        ))}
      </div>
    ) : null;

  function renderCredentialFields() {
    switch (editor.item_type) {
      case 'credit_card':
        return (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Card label</span>
              <input
                value={editor.title}
                onChange={(event) => updateEditorField('title', event.target.value)}
                className={vaultFieldClassName}
                placeholder="Main debit card, travel card, business Amex"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Cardholder name</span>
              <input
                value={editor.cardholder_name}
                onChange={(event) => updateEditorField('cardholder_name', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Issuer</span>
              <input
                value={editor.issuer_name}
                onChange={(event) => updateEditorField('issuer_name', event.target.value)}
                className={vaultFieldClassName}
                placeholder="Visa, Mastercard, Amex"
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Card number</span>
              <input
                value={editor.card_number}
                onChange={(event) => updateEditorField('card_number', event.target.value)}
                className={vaultFieldClassName}
                placeholder="4242 4242 4242 4242"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Expiry month</span>
              <input
                value={editor.expiry_month}
                onChange={(event) => updateEditorField('expiry_month', event.target.value)}
                className={vaultFieldClassName}
                placeholder="06"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Expiry year</span>
              <input
                value={editor.expiry_year}
                onChange={(event) => updateEditorField('expiry_year', event.target.value)}
                className={vaultFieldClassName}
                placeholder="2029"
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Security code</span>
              <input
                type={showSensitive ? 'text' : 'password'}
                value={editor.security_code}
                onChange={(event) => updateEditorField('security_code', event.target.value)}
                className={vaultFieldClassName}
                placeholder="CVV / CVC"
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Notes</span>
              <textarea
                value={editor.notes}
                onChange={(event) => updateEditorField('notes', event.target.value)}
                rows={4}
                className="rf-flat-input min-h-[6rem] px-4 py-3"
                placeholder="Billing details, issuer phone numbers, or travel notes."
              />
            </label>
          </div>
        );
      case 'passport':
        return (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Passport label</span>
              <input
                value={editor.title}
                onChange={(event) => updateEditorField('title', event.target.value)}
                className={vaultFieldClassName}
                placeholder="Primary passport, travel document, child passport"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Full name</span>
              <input
                value={editor.full_name}
                onChange={(event) => updateEditorField('full_name', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Nationality</span>
              <input
                value={editor.nationality}
                onChange={(event) => updateEditorField('nationality', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Passport number</span>
              <input
                value={editor.passport_number}
                onChange={(event) => updateEditorField('passport_number', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Issuing country</span>
              <input
                value={editor.issuing_country}
                onChange={(event) => updateEditorField('issuing_country', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Birth date</span>
              <input
                type="date"
                value={editor.birth_date}
                onChange={(event) => updateEditorField('birth_date', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Expiry date</span>
              <input
                type="date"
                value={editor.expiry_date}
                onChange={(event) => updateEditorField('expiry_date', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Notes</span>
              <textarea
                value={editor.notes}
                onChange={(event) => updateEditorField('notes', event.target.value)}
                rows={4}
                className="rf-flat-input min-h-[6rem] px-4 py-3"
                placeholder="Embassy contacts, renewal reminders, or travel notes."
              />
            </label>
          </div>
        );
      case 'secure_note':
        return (
          <div className="grid grid-cols-1 gap-4">
            <label className="space-y-2">
              <span className="text-sm font-medium">Title</span>
              <input
                value={editor.title}
                onChange={(event) => updateEditorField('title', event.target.value)}
                className={vaultFieldClassName}
                placeholder="Server recovery codes, household notes, insurance details"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Secure note</span>
              <textarea
                value={editor.notes}
                onChange={(event) => updateEditorField('notes', event.target.value)}
                rows={12}
                className="rf-flat-input min-h-[14rem] px-4 py-3"
                placeholder="Store anything else that matters here."
              />
            </label>
          </div>
        );
      case 'login':
      default:
        return (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Title</span>
              <input
                value={editor.title}
                onChange={(event) => updateEditorField('title', event.target.value)}
                className={vaultFieldClassName}
                placeholder="Instagram, GitHub, bank portal"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Username</span>
              <input
                value={editor.username}
                onChange={(event) => updateEditorField('username', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium">Login email</span>
              <input
                value={editor.login_email}
                onChange={(event) => updateEditorField('login_email', event.target.value)}
                className={vaultFieldClassName}
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Password</span>
              <div className="flex flex-col gap-2 sm:flex-row">
                <input
                  type={showSensitive ? 'text' : 'password'}
                  value={editor.password}
                  onChange={(event) => updateEditorField('password', event.target.value)}
                  className={`${vaultFieldClassName} flex-1`}
                />
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="rf-text-action rf-text-action-muted text-sm"
                    onClick={() => setShowSensitive((current) => !current)}
                  >
                    {showSensitive ? 'Hide' : 'Reveal'}
                  </button>
                  <button
                    type="button"
                    className="rf-text-action text-sm"
                    onClick={() =>
                      updateEditorField(
                        'password',
                        generatedPassword || generatePassword(generatorOptions, generatorPreset),
                      )
                    }
                  >
                    Use generated
                  </button>
                </div>
              </div>
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Website URLs</span>
              <textarea
                value={editor.website_urls}
                onChange={(event) => updateEditorField('website_urls', event.target.value)}
                rows={4}
                className="rf-flat-input min-h-[6rem] px-4 py-3"
                placeholder={'https://instagram.com\nhttps://www.instagram.com'}
              />
            </label>
            <label className="space-y-2 md:col-span-2">
              <span className="text-sm font-medium">Notes</span>
              <textarea
                value={editor.notes}
                onChange={(event) => updateEditorField('notes', event.target.value)}
                rows={4}
                className="rf-flat-input min-h-[6rem] px-4 py-3"
                placeholder="Recovery hints, support numbers, or login caveats."
              />
            </label>
          </div>
        );
    }
  }

  if (authLoading || loadingState || cryptoReadiness === null) {
    return (
      <div className="rf-flat-empty animate-rise px-5 py-4">
        <p className="text-sm muted">Loading Vault…</p>
      </div>
    );
  }

  if (!me) {
    return (
      <div className="rf-flat-empty animate-rise px-5 py-4">
        <p className="text-sm muted">Redirecting to login…</p>
      </div>
    );
  }

  if (isRustyVaultUnavailableError(error)) {
    return (
      <div className="rf-flat-empty animate-rise px-5 py-4">
        <p className="text-sm muted">{error}</p>
      </div>
    );
  }

  if (vaultView !== 'workspace') {
    return (
      <div className="rf-flat-page rf-flat-scope animate-rise">
        <header className="rf-flat-header pb-3">
          <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between lg:gap-12 xl:gap-16">
            <div className="space-y-2">
              <h1 className="text-3xl font-semibold sm:text-4xl">Vault</h1>
              <p className="max-w-3xl text-sm muted sm:text-base">
                Client-side encrypted credentials, cards, passports, secure notes, password generation, and browser-extension pairing inside the existing Rustyfin security model.
              </p>
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-4 text-sm sm:grid-cols-4 lg:min-w-[30rem]">
              <div className="space-y-1">
                <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">Items</p>
                <p className="text-lg font-semibold">{config?.item_count ?? 0}</p>
              </div>
              <div className="space-y-1">
                <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">KDF</p>
                <p className="text-sm font-medium">
                  {cryptoReadiness.mode === 'portable-fallback'
                    ? 'Argon2id fallback'
                    : 'Argon2id 64 MiB'}
                </p>
              </div>
              <div className="space-y-1">
                <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">
                  Auto-lock
                </p>
                <p className="text-lg font-semibold">{prefs.auto_lock_minutes}m</p>
              </div>
              <div className="space-y-1">
                <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">Session</p>
                <p className="text-sm font-medium">
                  {rustyVaultSession
                    ? formatTimestamp(rustyVaultSession.access_expires_ts)
                    : 'Unavailable'}
                </p>
              </div>
            </div>
          </div>
        </header>

        {cryptoReadiness.reason !== 'ok' && (
          <div className="notice-error rounded-xl px-4 py-3 text-sm">
            {cryptoReadiness.message}
          </div>
        )}

        <section className="rf-flat-section pt-3 sm:pt-4">
          {vaultView === 'index' ? (
            <div className="space-y-4 border-t border-white/8 pt-5">
              <button
                type="button"
                className="w-full border-l border-white/10 px-4 py-5 text-left transition hover:bg-white/[0.02]"
                onClick={() => {
                  setError(null);
                  setMessage(null);
                  setMasterPassword('');
                  setConfirmMasterPassword('');
                  setVaultView('prompt');
                }}
              >
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div className="space-y-1">
                    <h2 className="text-2xl font-semibold">{currentVaultLabel}</h2>
                    <p className="text-sm muted">{currentVaultDescription}</p>
                  </div>
                  <div className="rf-inline-meta justify-start sm:justify-end">
                    <span>{config?.enabled ? 'Locked' : 'Not set up'}</span>
                    <span>{config?.item_count ?? 0} items</span>
                    <span>{rustyVaultSession ? 'Web session active' : 'No web session'}</span>
                  </div>
                </div>
              </button>
            </div>
          ) : (
            <div className="space-y-5 border-t border-white/8 pt-5">
              <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div className="space-y-1">
                  <h2 className="text-2xl font-semibold">
                    {config?.enabled ? 'Unlock Personal Vault' : 'Create Personal Vault'}
                  </h2>
                  <p className="text-sm muted">
                    {config?.enabled
                      ? 'Enter the vault password to open all saved credentials and personal records.'
                      : 'Create a vault password to enable encrypted storage for credentials, cards, documents, and secure notes.'}
                  </p>
                </div>
                <button
                  type="button"
                  className="rf-text-action rf-text-action-muted text-sm"
                  onClick={() => {
                    setMasterPassword('');
                    setConfirmMasterPassword('');
                    setVaultView('index');
                  }}
                >
                  Back to vaults
                </button>
              </div>

              <div className="space-y-4 border-l border-white/10 pl-4">
                <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                  <label className="space-y-2">
                    <span className="text-sm font-medium">Vault password</span>
                    <input
                      type="password"
                      value={masterPassword}
                      onChange={(event) => setMasterPassword(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder={
                        config?.enabled ? 'Enter vault password' : 'Create vault password'
                      }
                    />
                  </label>
                  {!config?.enabled ? (
                    <label className="space-y-2">
                      <span className="text-sm font-medium">Confirm vault password</span>
                      <input
                        type="password"
                        value={confirmMasterPassword}
                        onChange={(event) => setConfirmMasterPassword(event.target.value)}
                        className={vaultFieldClassName}
                        placeholder="Confirm vault password"
                      />
                    </label>
                  ) : null}
                </div>

                <div className="flex flex-wrap gap-x-5 gap-y-2">
                  <button
                    type="button"
                    className="rf-text-action text-sm disabled:opacity-50"
                    disabled={saving || !me || !canSubmitVaultPrompt || !cryptoReady}
                    onClick={() =>
                      runAction(
                        config?.enabled ? 'Vault unlocked.' : 'Vault created.',
                        config?.enabled ? unlockExistingVault : bootstrapFreshVault,
                      )
                    }
                  >
                    {config?.enabled ? 'Unlock vault' : 'Create vault'}
                  </button>
                  <button
                    type="button"
                    className="rf-text-action rf-text-action-muted text-sm"
                    onClick={() => {
                      setMasterPassword('');
                      setConfirmMasterPassword('');
                    }}
                  >
                    Clear
                  </button>
                </div>
              </div>
            </div>
          )}
        </section>
        {toastStack}
      </div>
    );
  }

  return (
    <div className="rf-flat-page rf-flat-scope animate-rise">
      <header className="rf-flat-header pb-3">
        <div className="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between lg:gap-12 xl:gap-16">
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold sm:text-4xl">Vault</h1>
            <p className="max-w-3xl text-sm muted sm:text-base">
              Your encrypted credentials open first, while settings, generator, and extension setup stay neatly separated.
            </p>
          </div>
          <div className="grid grid-cols-2 gap-x-4 gap-y-4 text-sm sm:grid-cols-4 lg:min-w-[30rem]">
            <div className="space-y-1">
              <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">Items</p>
              <p className="text-lg font-semibold">{config?.item_count ?? 0}</p>
            </div>
            <div className="space-y-1">
              <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">KDF</p>
              <p className="text-sm font-medium">
                {cryptoReadiness.mode === 'portable-fallback'
                  ? 'Argon2id fallback'
                  : 'Argon2id 64 MiB'}
              </p>
            </div>
            <div className="space-y-1">
              <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">Auto-lock</p>
              <p className="text-lg font-semibold">{prefs.auto_lock_minutes}m</p>
            </div>
            <div className="space-y-1">
              <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">Session</p>
              <p className="text-sm font-medium">
                {rustyVaultSession
                  ? formatTimestamp(rustyVaultSession.access_expires_ts)
                  : 'Unavailable'}
              </p>
            </div>
          </div>
        </div>
      </header>

      <section className="rf-flat-section pt-2 sm:pt-3">
        <div className="flex justify-end">
          <button
            type="button"
            className="rf-text-action rf-text-action-muted text-sm"
            onClick={() => {
              setUnlocked(null);
              setVaultView('index');
              setActiveWorkspaceTab('credentials');
              setRows([]);
              setSelectedItem(null);
              setEditingExisting(false);
              setEditor(defaultEditorState());
              setShowSensitive(false);
              setMasterPassword('');
              setConfirmMasterPassword('');
            }}
          >
            Lock vault
          </button>
        </div>

        <SurfaceTabsBar
          variant="vault"
          className=""
          activeKey={activeWorkspaceTab}
          onSelect={(value) => setActiveWorkspaceTab(value as VaultWorkspaceTab)}
          options={[
            { key: 'credentials', label: 'Credentials' },
            { key: 'settings', label: 'Settings' },
            { key: 'generator', label: 'Password Generator' },
            { key: 'extension', label: 'Extension' },
          ]}
          badges={workspaceBadges}
          badgesClassName="-translate-y-[2px]"
        />

        <div key={activeWorkspaceTab} className="vault-workspace-panel pt-5 sm:pt-6">
          {activeWorkspaceTab === 'credentials' && (
            <div className={workspaceContentClassName}>
              <div className="space-y-7">
                <div className={vaultSectionClassName}>
                  <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
                    <div>
                      <h2 className="text-xl font-semibold">Saved credentials</h2>
                      <p className="mt-1 max-w-2xl text-sm muted">
                        Unlock once, then browse everything locally from ciphertext summaries. Search stays in the browser.
                      </p>
                    </div>
                    <div className="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center">
                      <input
                        value={search}
                        onChange={(event) => setSearch(event.target.value)}
                        className="rf-flat-input min-w-[16rem] px-4 py-2 text-sm"
                        placeholder="Search title, site, person, or document"
                      />
                      <select
                        value={newItemType}
                        onChange={(event) =>
                          setNewItemType(parseVaultItemType(event.target.value))
                        }
                        className="rf-flat-input min-w-[12rem] px-4 py-2 text-sm"
                      >
                        {ITEM_TYPE_OPTIONS.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        className="rf-text-action text-sm"
                        onClick={() => startNewDraft(newItemType)}
                      >
                        Add new
                      </button>
                    </div>
                  </div>

                  {!unlocked ? (
                    <div className="rf-flat-empty px-4 py-3 text-sm muted">
                      Unlock the vault to browse and edit entries.
                    </div>
                  ) : filteredRows.length === 0 ? (
                    <div className="rf-flat-empty px-4 py-4 text-sm muted">
                      No matching items yet. Start with a login, card, passport, or secure note.
                    </div>
                  ) : (
                    <div className="space-y-1">
                      {filteredRows.map((row) => (
                        <button
                          key={row.encrypted.id}
                          type="button"
                          data-vault-item-id={row.encrypted.id}
                          onClick={() => {
                            setError(null);
                            void loadItem(row.encrypted.id).catch((err) => {
                              setError(clientErrorMessage(err, 'Failed to load the vault item'));
                            });
                          }}
                          className={`w-full rounded-2xl px-4 py-4 text-left transition ${
                            selectedItem?.id === row.encrypted.id
                              ? 'border border-[var(--orange-soft)]/45 bg-white/[0.035]'
                              : 'border border-transparent hover:border-white/10 hover:bg-white/[0.02]'
                          }`}
                        >
                          <div className="flex items-start justify-between gap-4">
                            <div className="min-w-0">
                              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                                <p className="truncate font-semibold">{row.summary.title}</p>
                                <span className="text-[11px] uppercase tracking-[0.18em] text-white/40">
                                  {itemTypeLabel(row.summary.item_type)}
                                </span>
                              </div>
                              <p className="mt-1 truncate text-sm muted">
                                {row.summary.primary_uri || row.summary.subtitle}
                              </p>
                              <p className="mt-1 truncate text-xs text-white/45">
                                {row.summary.username || row.summary.login_email || 'Saved entry'}
                              </p>
                            </div>
                            <div className="shrink-0 text-right text-xs text-white/45">
                              <p>{formatTimestamp(row.encrypted.updated_ts)}</p>
                              {lookupResultIds.includes(row.encrypted.id) ? (
                                <p className="mt-1 text-[var(--orange-soft)]">Matched</p>
                              ) : row.encrypted.favorite ? (
                                <p className="mt-1 text-white/70">Favorite</p>
                              ) : null}
                            </div>
                          </div>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              </div>

              <div className="space-y-7">
                <div className={vaultSectionClassName}>
                  <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                    <div>
                      <p className="text-[11px] uppercase tracking-[0.22em] text-white/45">
                        {editingExisting ? 'Editing saved item' : 'New draft'}
                      </p>
                      <h2 className="text-xl font-semibold">
                        {editingExisting
                          ? editor.title || itemTypeLabel(editor.item_type)
                          : `New ${itemTypeLabel(editor.item_type)}`}
                      </h2>
                      <p className="mt-1 max-w-2xl text-sm muted">
                        {itemTypeDescription(editor.item_type)}
                      </p>
                    </div>
                    {!editingExisting && (
                      <select
                        value={editor.item_type}
                        onChange={(event) =>
                          startNewDraft(parseVaultItemType(event.target.value))
                        }
                        className="rf-flat-input min-w-[12rem] px-4 py-2 text-sm"
                      >
                        {ITEM_TYPE_OPTIONS.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    )}
                  </div>

                  {renderCredentialFields()}

                  <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
                    <RfSwitch
                      label="Favorite item"
                      checked={editor.favorite}
                      onChange={(checked) => updateEditorField('favorite', checked)}
                    />
                    {(editor.item_type === 'login' ||
                      editor.item_type === 'credit_card' ||
                      editor.item_type === 'passport') && (
                      <button
                        type="button"
                        className="rf-text-action rf-text-action-muted text-sm"
                        onClick={() => setShowSensitive((current) => !current)}
                      >
                        {showSensitive ? 'Hide sensitive fields' : 'Reveal sensitive fields'}
                      </button>
                    )}
                    <button
                      type="button"
                      className="rf-text-action text-sm disabled:opacity-50"
                      disabled={saving || !unlocked}
                      onClick={() =>
                        runAction(
                          editingExisting ? 'Vault item saved.' : 'Vault item created.',
                          saveEditorItem,
                        )
                      }
                    >
                      {editingExisting ? 'Save changes' : 'Save item'}
                    </button>
                    {selectedItem && itemPrimaryCopyValue(selectedItem) && (
                      <button
                        type="button"
                        className="rf-text-action text-sm disabled:opacity-50"
                        onClick={() => runAction(itemPrimaryCopyLabel(selectedItem), copySelectedValue)}
                      >
                        {itemPrimaryCopyLabel(selectedItem)}
                      </button>
                    )}
                    {selectedItem && (
                      <button
                        type="button"
                        className="rf-text-action rf-text-action-danger text-sm disabled:opacity-50"
                        onClick={() => runAction('Vault item deleted.', deleteSelectedItem)}
                      >
                        Delete
                      </button>
                    )}
                  </div>

                  {selectedItem && (
                    <div className="space-y-2 border-l border-white/10 pl-4 text-sm">
                      <div className="flex items-center justify-between gap-3">
                        <span className="font-medium">
                          {itemPrimaryCopyLabel(selectedItem).replace('Copy ', '')}
                        </span>
                        <span className="text-xs text-white/55">
                          {selectedItem.favorite ? 'Favorite' : itemTypeLabel(selectedItem.item_type)}
                        </span>
                      </div>
                      <p
                        className={`text-sm text-white/85 ${
                          selectedItem.item_type === 'secure_note'
                            ? 'whitespace-pre-wrap'
                            : 'font-mono tracking-[0.18em]'
                        }`}
                      >
                        {itemPrimaryPreview(selectedItem, showSensitive)}
                      </p>
                      <p className="muted">
                        Created {formatTimestamp(selectedItem.created_ts)} • Updated{' '}
                        {formatTimestamp(selectedItem.updated_ts)}
                      </p>
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}

          {activeWorkspaceTab === 'settings' && (
            <div className={workspaceContentClassName}>
              <div className="space-y-7">
                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Settings</h2>
                    <p className="mt-1 text-sm muted">
                      Protected actions stay here so the credentials view remains focused on stored data.
                    </p>
                  </div>

                  <label className="space-y-2">
                    <span className="text-sm font-medium">
                      Rustyfin account password for protected actions
                    </span>
                    <input
                      type="password"
                      value={securityPassword}
                      onChange={(event) => setSecurityPassword(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder="Used only for short-lived protected action challenges"
                    />
                  </label>
                </div>

                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Vault preferences</h2>
                    <p className="mt-1 text-sm muted">
                      Keep the daily vault behavior predictable without adding clutter to the main credentials view.
                    </p>
                  </div>

                  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                    <label className="space-y-2">
                      <span className="text-sm">Auto-lock minutes</span>
                      <input
                        type="number"
                        min={1}
                        max={240}
                        value={prefs.auto_lock_minutes}
                        onChange={(event) =>
                          setPrefs((current) => ({
                            ...current,
                            auto_lock_minutes:
                              Number.parseInt(event.target.value || '15', 10) || 15,
                          }))
                        }
                        className={vaultFieldClassName}
                      />
                    </label>
                    <label className="space-y-2">
                      <span className="text-sm">Clipboard clear seconds</span>
                      <input
                        type="number"
                        min={0}
                        max={120}
                        value={prefs.clipboard_clear_seconds}
                        onChange={(event) =>
                          setPrefs((current) => ({
                            ...current,
                            clipboard_clear_seconds:
                              Number.parseInt(event.target.value || '30', 10) || 0,
                          }))
                        }
                        className={vaultFieldClassName}
                      />
                    </label>
                    <label className="space-y-2">
                      <span className="text-sm">Default match mode</span>
                      <select
                        value={currentMatchMode}
                        onChange={(event) =>
                          setPrefs((current) => ({
                            ...current,
                            default_match_mode: normalizeMode(event.target.value),
                          }))
                        }
                        className={vaultFieldClassName}
                      >
                        <option value="exact">Exact</option>
                        <option value="host">Host</option>
                        <option value="base_domain">Base domain</option>
                        <option value="never">Never</option>
                      </select>
                    </label>
                    <label className="space-y-2">
                      <span className="text-sm">Excluded domains</span>
                      <textarea
                        value={excludedDomainsInput}
                        onChange={(event) => setExcludedDomainsInput(event.target.value)}
                        rows={4}
                        className="rf-flat-input min-h-[6rem] px-4 py-3"
                        placeholder={'example.com\nbank.example'}
                      />
                    </label>
                  </div>

                  <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                    {([
                      ['inline_save_prompt_enabled', 'Automatic save prompts'],
                      ['inline_autofill_enabled', 'Inline autofill affordances'],
                      ['warn_on_http', 'Warn before HTTP fill'],
                      ['warn_on_untrusted_iframe', 'Warn on untrusted iframe fill'],
                      ['allow_manual_http_fill', 'Allow manual HTTP fill'],
                    ] as const).map(([key, label]) => (
                      <RfSwitch
                        key={key}
                        label={label}
                        checked={prefs[key]}
                        onChange={(checked) =>
                          setPrefs((current) => ({
                            ...current,
                            [key]: checked,
                          }))
                        }
                      />
                    ))}
                  </div>

                  <button
                    type="button"
                    className="rf-text-action text-sm"
                    onClick={() => runAction('Vault preferences saved.', savePreferences)}
                  >
                    Save preferences
                  </button>
                </div>
              </div>

              <div className="space-y-7">
                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Change vault master password</h2>
                    <p className="mt-1 text-sm muted">
                      Re-enter the current vault master password here. The Rustyfin account password above is still required for the protected action challenge.
                    </p>
                  </div>
                  <div className="grid grid-cols-1 gap-3">
                    <input
                      type="password"
                      value={currentRustyVaultPassword}
                      onChange={(event) => setCurrentVaultPassword(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder="Current vault master password"
                    />
                    <input
                      type="password"
                      value={newMasterPassword}
                      onChange={(event) => setNewMasterPassword(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder="New vault master password"
                    />
                    <input
                      type="password"
                      value={newMasterPasswordConfirm}
                      onChange={(event) => setNewMasterPasswordConfirm(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder="Confirm new vault master password"
                    />
                  </div>
                  <button
                    type="button"
                    className="rf-text-action text-sm"
                    disabled={!unlocked}
                    onClick={() =>
                      runAction('Vault master password changed.', rekeyMasterPassword)
                    }
                  >
                    Rotate master password
                  </button>
                </div>

                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Import and export</h2>
                    <p className="mt-1 text-sm muted">
                      Export decrypted JSON locally or import Bitwarden logins into this vault.
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-x-5 gap-y-2">
                    <button
                      type="button"
                      className="rf-text-action text-sm disabled:opacity-50"
                      disabled={!unlocked}
                      onClick={() => runAction('Vault export downloaded.', exportCurrentVault)}
                    >
                      Export decrypted JSON
                    </button>
                  </div>
                  <RfSwitch
                    label="Clear current items before importing"
                    checked={importClearExisting}
                    onChange={setImportClearExisting}
                  />
                  <input
                    type="file"
                    accept="application/json,.json"
                    onChange={(event) => setImportFile(event.target.files?.[0] ?? null)}
                    className="block w-full text-sm"
                  />
                  <button
                    type="button"
                    className="rf-text-action text-sm disabled:opacity-50"
                    disabled={!unlocked || !importFile}
                    onClick={() =>
                      runAction('Bitwarden import completed.', importBitwardenJson)
                    }
                  >
                    Import Bitwarden JSON locally
                  </button>
                </div>

                <div className="space-y-3 border-t border-[var(--danger)]/35 pt-4">
                  <p className="font-medium text-[var(--danger)]">Delete vault</p>
                  <p className="text-sm muted">
                    Destroying the vault deletes wrapped keys, item ciphertext, audit history, and vault device sessions. The main Rustyfin account stays intact.
                  </p>
                  <button
                    type="button"
                    className="rf-text-action rf-text-action-danger text-sm"
                    onClick={() => runAction('Vault destroyed.', destroyCurrentVault)}
                  >
                    Destroy vault
                  </button>
                </div>
              </div>
            </div>
          )}

          {activeWorkspaceTab === 'generator' && (
            <div className={workspaceContentClassName}>
              <div className={vaultSectionClassName}>
                <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                  <div>
                    <h2 className="text-xl font-semibold">Password Generator</h2>
                    <p className="mt-1 max-w-2xl text-sm muted">
                      Strong randomness comes from the browser crypto RNG, not `Math.random()`. Memorable mode now builds human-friendly passphrases first, then layers in sensible substitutions that still respect your toggles.
                    </p>
                  </div>
                  <div className="flex gap-2">
                    {(['memorable', 'balanced', 'maximum'] as PasswordGeneratorPreset[]).map(
                      (preset) => (
                        <button
                          key={preset}
                          type="button"
                          className={`rf-text-action text-sm ${
                            generatorPreset === preset ? '' : 'rf-text-action-muted'
                          }`}
                          onClick={() => setGeneratorPreset(preset)}
                        >
                          {preset}
                        </button>
                      ),
                    )}
                  </div>
                </div>

                <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                  <label className="space-y-2">
                    <span className="text-sm font-medium">Length</span>
                    <input
                      type="number"
                      min={12}
                      max={64}
                      value={generatorOptions.length}
                      onChange={(event) =>
                        setGeneratorOptions((current) => ({
                          ...current,
                          length:
                            Number.parseInt(event.target.value || '0', 10) || current.length,
                        }))
                      }
                      className={vaultFieldClassName}
                    />
                  </label>
                  {([
                    ['include_uppercase', 'Upper'],
                    ['include_lowercase', 'Lower'],
                    ['include_numbers', 'Numbers'],
                    ['include_symbols', 'Symbols'],
                  ] as const).map(([key, label]) => (
                    <RfSwitch
                      key={key}
                      label={label}
                      checked={generatorOptions[key]}
                      onChange={(checked) =>
                        setGeneratorOptions((current) => ({
                          ...current,
                          [key]: checked,
                        }))
                      }
                    />
                  ))}
                </div>

                <RfSwitch
                  label="Exclude ambiguous characters"
                  checked={generatorOptions.exclude_ambiguous}
                  onChange={(checked) =>
                    setGeneratorOptions((current) => ({
                      ...current,
                      exclude_ambiguous: checked,
                    }))
                  }
                />

                {generatorPreset === 'memorable' && (
                  <p className="text-sm muted">
                    Memorable mode prefers real words first, then swaps in replacements that still make sense, like `@` for `a` or `3` for `e`, while honoring the toggles you leave on.
                  </p>
                )}

                <div className="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center">
                  <input
                    readOnly
                    value={generatedPassword}
                    className="rf-flat-input w-full px-4 py-3 font-mono text-sm"
                    placeholder="Generate a password to stage it here"
                  />
                  <button
                    type="button"
                    className="rf-text-action text-sm"
                    onClick={() =>
                      runAction('Generated a new password.', async () =>
                        setGeneratedPassword(
                          generatePassword(generatorOptions, generatorPreset),
                        ),
                      )
                    }
                  >
                    Generate
                  </button>
                  <button
                    type="button"
                    className="rf-text-action text-sm disabled:opacity-50"
                    disabled={!generatedPassword}
                    onClick={() =>
                      runAction('Generated password copied.', async () =>
                        writeClipboardWithTimeout(
                          generatedPassword,
                          prefs.clipboard_clear_seconds,
                        ),
                      )
                    }
                  >
                    Copy
                  </button>
                  <button
                    type="button"
                    className="rf-text-action text-sm disabled:opacity-50"
                    disabled={!generatedPassword}
                    onClick={() => {
                      startNewDraft('login');
                      updateEditorField('password', generatedPassword);
                      setShowSensitive(true);
                      setMessage('Generated password staged in a new login draft.');
                    }}
                  >
                    Use in vault
                  </button>
                </div>
              </div>
            </div>
          )}

          {activeWorkspaceTab === 'extension' && (
            <div className={workspaceContentClassName}>
              <div className="space-y-7">
                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Extension Setup</h2>
                    <p className="mt-1 text-sm muted">
                      Pairing and device session management live here so the main vault workspace stays focused on saved credentials.
                    </p>
                  </div>
                  <label className="space-y-2">
                    <span className="text-sm font-medium">
                      Rustyfin account password for extension actions
                    </span>
                    <input
                      type="password"
                      value={securityPassword}
                      onChange={(event) => setSecurityPassword(event.target.value)}
                      className={vaultFieldClassName}
                      placeholder="Needed for pairing and session revocation"
                    />
                  </label>
                  <div className={vaultSubsectionClassName}>
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <p className="font-medium">Browser extension package</p>
                        <p className="text-sm muted">
                          Download the current RustyVault browser extension package from Downloads, then pair it from this page.
                        </p>
                      </div>
                      <span className="text-xs text-white/55">Host-managed download</span>
                    </div>
                    <div className="flex flex-wrap gap-x-5 gap-y-2">
                      <Link href="/downloads" className="rf-text-action text-sm">
                        Open Downloads
                      </Link>
                      <button
                        type="button"
                        className="rf-text-action text-sm disabled:opacity-50"
                        disabled={!rustyVaultSession}
                        onClick={() =>
                          runAction('Extension pairing code issued.', pairExtension)
                        }
                      >
                        Pair browser extension
                      </button>
                      <button
                        type="button"
                        className="rf-text-action text-sm"
                        onClick={() =>
                          runAction('Other vault sessions revoked.', revokeOtherSessions)
                        }
                      >
                        Revoke other sessions
                      </button>
                    </div>
                    <div className="space-y-1 text-sm muted">
                      <p>1. Download the zip package from Downloads and extract it on your machine.</p>
                      <p>2. In Chrome or Edge developer extensions, choose Load unpacked and select the extracted folder.</p>
                      <p>3. Open the extension popup, set your Rustyfin server URL, then use the pairing code below.</p>
                    </div>
                  </div>
                </div>

                <div className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Lookup Test</h2>
                    <p className="mt-1 text-sm muted">
                      Preview the blinded-site matching flow the extension uses before it offers save or manual fill.
                    </p>
                  </div>
                  <input
                    value={lookupUrl}
                    onChange={(event) => setLookupUrl(event.target.value)}
                    className={vaultFieldClassName}
                    placeholder="https://accounts.example.com/login"
                  />
                  <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
                    <button
                      type="button"
                      className="rf-text-action text-sm disabled:opacity-50"
                      disabled={!unlocked}
                      onClick={() => {
                        setError(null);
                        void refreshLookup().catch((err) => {
                          setError(clientErrorMessage(err, 'Lookup failed'));
                        });
                      }}
                    >
                      Check matches
                    </button>
                    <span className="text-xs text-white/55">
                      Match mode: {currentMatchMode}
                    </span>
                  </div>
                  {lookupResultIds.length > 0 ? (
                    <div className="space-y-2 text-sm">
                      {lookupResultIds.map((itemId) => (
                        <div key={itemId} className="border-l border-white/10 pl-4">
                          {rows.find((row) => row.encrypted.id === itemId)?.summary.title || itemId}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="rf-flat-empty px-4 py-3 text-sm muted">
                      No matches yet, or the vault is still locked.
                    </div>
                  )}
                </div>
              </div>

              <div className="space-y-7">
                <div id="vault-devices" className={vaultSectionClassName}>
                  <div>
                    <h2 className="text-xl font-semibold">Vault Devices</h2>
                    <p className="mt-1 text-sm muted">
                      Dedicated vault sessions stay separate from the main Rustyfin login and can be revoked per device.
                    </p>
                  </div>
                  <div className="space-y-3">
                    {deviceSessions.length === 0 ? (
                      <div className="rf-flat-empty px-4 py-3 text-sm muted">
                        No paired vault devices yet.
                      </div>
                    ) : (
                      deviceSessions.map((session) => (
                        <div key={session.id} className="space-y-2 border-l border-white/10 pl-4">
                          <div className="flex items-center justify-between gap-3">
                            <div>
                              <p className="font-medium">{session.device_name}</p>
                              <p className="text-sm muted">
                                {session.client_kind === 'rustyvault_web'
                                  ? 'Web vault'
                                  : 'Browser extension'}{' '}
                                • {session.device_platform || 'Unknown platform'}
                              </p>
                            </div>
                            <span className="text-xs text-white/55">
                              {session.current
                                ? 'Current'
                                : session.revoked_ts
                                  ? 'Revoked'
                                  : 'Active'}
                            </span>
                          </div>
                          <p className="text-xs muted">
                            Created {formatTimestamp(session.created_ts)} • Last used{' '}
                            {formatTimestamp(session.last_used_ts)}
                          </p>
                          {!session.current && !session.revoked_ts && (
                            <button
                              type="button"
                              className="rf-text-action rf-text-action-danger text-sm"
                              onClick={() =>
                                runAction('Vault device revoked.', async () => {
                                  await withRustyVaultAccess((accessToken) =>
                                    revokeRustyVaultDeviceSession(session.id, accessToken),
                                  );
                                  await reloadRustyVaultChrome();
                                })
                              }
                            >
                              Revoke
                            </button>
                          )}
                        </div>
                      ))
                    )}
                  </div>
                  {extensionPairing && (
                    <div className="space-y-2 border-l border-white/10 pl-4">
                      <p className="text-sm font-semibold">Pairing code</p>
                      <p className="font-mono text-lg tracking-[0.2em] text-white/90">
                        {extensionPairing.pairing_code}
                      </p>
                      <p className="text-sm muted">
                        Fingerprint phrase:{' '}
                        <span className="text-white/90">
                          {extensionPairing.fingerprint_phrase}
                        </span>
                      </p>
                      <p className="text-xs muted">
                        Expires {formatTimestamp(extensionPairing.expires_ts)}
                      </p>
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </div>
      </section>
      {toastStack}
    </div>
  );
}
