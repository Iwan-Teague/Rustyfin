'use client';

import { useEffect, useState, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import {
  getPublicSystemInfo,
  claimSession,
  setOwnerToken,
  clearOwnerToken,
  putSetupConfig,
  createAdmin,
  createLibraries,
  listSetupHostDirectories,
  putSetupMetadata,
  putSetupNetwork,
  completeSetup,
  type LibrarySpec,
  type SetupHostDirectoryListEntry,
  type SetupError,
} from '@/lib/setupApi';
import { parseResponseBody } from '@/lib/api';
import Link from 'next/link';
import { useAuth } from '@/lib/auth';

type Step = 'loading' | 'welcome' | 'config' | 'admin' | 'libraries' | 'metadata' | 'network' | 'complete' | 'done';

interface FieldErrors {
  [key: string]: string[];
}

interface SetupLibraryDraft {
  id: string;
  name: string;
  kind: 'movies' | 'tv_shows' | 'music';
  path: string;
  is_read_only: boolean;
}

export default function SetupWizard() {
  const router = useRouter();
  const [step, setStep] = useState<Step>('loading');
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [saving, setSaving] = useState(false);
  const { refreshMe } = useAuth();

  // Config state
  const [serverName, setServerName] = useState('Rustyfin');
  const [locale, setLocale] = useState('en');
  const [region, setRegion] = useState('US');
  const [timeZone, setTimeZone] = useState('');

  // Admin state
  const [adminUsername, setAdminUsername] = useState('');
  const [adminPassword, setAdminPassword] = useState('');
  const [adminPasswordConfirm, setAdminPasswordConfirm] = useState('');

  // Metadata state
  const [metaLanguage, setMetaLanguage] = useState('en');
  const [metaRegion, setMetaRegion] = useState('US');

  // Network state
  const [allowRemote, setAllowRemote] = useState(false);
  const [autoPort, setAutoPort] = useState(false);

  // Optional libraries state
  const [libraryDrafts, setLibraryDrafts] = useState<SetupLibraryDraft[]>([
    {
      id: 'lib-1',
      name: '',
      kind: 'movies',
      path: '',
      is_read_only: false,
    },
  ]);

  // Setup host directory browser state
  const [hostDirBrowserOpen, setHostDirBrowserOpen] = useState(false);
  const [hostDirBrowserLoading, setHostDirBrowserLoading] = useState(false);
  const [hostDirBrowserError, setHostDirBrowserError] = useState<string | null>(null);
  const [hostDirBrowserCurrentPath, setHostDirBrowserCurrentPath] = useState('');
  const [hostDirBrowserParentPath, setHostDirBrowserParentPath] = useState<string | null>(null);
  const [hostDirBrowserRoots, setHostDirBrowserRoots] = useState<string[]>([]);
  const [hostDirBrowserDirectories, setHostDirBrowserDirectories] = useState<
    SetupHostDirectoryListEntry[]
  >([]);
  const [hostDirBrowserTargetLibraryId, setHostDirBrowserTargetLibraryId] = useState<
    string | null
  >(null);

  useEffect(() => {
    let cancelled = false;
    getPublicSystemInfo()
      .then((info) => {
        if (cancelled) return;
        if (info.setup_completed) {
          router.replace('/');
        } else {
          setStep('welcome');
        }
      })
      .catch(() => {
        if (!cancelled) setStep('welcome');
      });
    return () => {
      cancelled = true;
    };
  }, [router]);

  const handleError = useCallback((err: unknown) => {
    setSaving(false);
    const setupErr = err as SetupError;
    if (setupErr?.code === 'validation_failed' && setupErr?.details?.fields) {
      setFieldErrors(setupErr.details.fields as FieldErrors);
      setError('Please fix the highlighted fields.');
    } else {
      setFieldErrors({});
      setError(setupErr?.message || 'An unexpected error occurred.');
    }
  }, []);

  const clearErrors = () => {
    setError(null);
    setFieldErrors({});
  };

  // Step 1: Welcome — claim session
  const handleStart = async () => {
    clearErrors();
    setSaving(true);
    try {
      const result = await claimSession('WebUI', false, false);
      setOwnerToken(result.owner_token);
      setStep('config');
    } catch (err: unknown) {
      const setupErr = err as SetupError;
      if (setupErr?.code === 'setup_claimed') {
        // Try force takeover
        try {
          const result = await claimSession('WebUI', true, true);
          setOwnerToken(result.owner_token);
          setStep('config');
        } catch (innerErr) {
          handleError(innerErr);
        }
      } else {
        handleError(err);
      }
    }
    setSaving(false);
  };

  // Step 2: Config
  const handleConfig = async () => {
    clearErrors();
    setSaving(true);
    try {
      await putSetupConfig({
        server_name: serverName,
        default_ui_locale: locale,
        default_region: region.toUpperCase(),
        default_time_zone: timeZone || null,
      });
      setStep('admin');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  // Step 3: Admin
  const handleAdmin = async () => {
    clearErrors();
    if (adminPassword !== adminPasswordConfirm) {
      setFieldErrors({ password_confirm: ['Passwords do not match'] });
      setError('Passwords do not match.');
      return;
    }
    setSaving(true);
    try {
      const idempotencyKey =
        typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
          ? crypto.randomUUID()
          : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
      await createAdmin(adminUsername, adminPassword, idempotencyKey);
      setStep('libraries');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  const updateLibraryDraft = <K extends keyof SetupLibraryDraft>(
    id: string,
    key: K,
    value: SetupLibraryDraft[K],
  ) => {
    setLibraryDrafts((prev) =>
      prev.map((draft) => (draft.id === id ? { ...draft, [key]: value } : draft)),
    );
  };

  const addLibraryDraft = () => {
    setLibraryDrafts((prev) => [
      ...prev,
      {
        id: `lib-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
        name: '',
        kind: 'movies',
        path: '',
        is_read_only: false,
      },
    ]);
  };

  const removeLibraryDraft = (id: string) => {
    setLibraryDrafts((prev) => (prev.length <= 1 ? prev : prev.filter((draft) => draft.id !== id)));
  };

  const fetchHostDirectories = async (path?: string) => {
    const data = await listSetupHostDirectories(path);
    setHostDirBrowserCurrentPath(data.current_path);
    setHostDirBrowserParentPath(data.parent_path);
    setHostDirBrowserRoots(data.roots);
    setHostDirBrowserDirectories(data.directories);
  };

  const openHostDirectoryBrowser = (libraryId: string, initialPath?: string) => {
    setHostDirBrowserOpen(true);
    setHostDirBrowserTargetLibraryId(libraryId);
    setHostDirBrowserError(null);
    setHostDirBrowserLoading(true);
    void fetchHostDirectories(initialPath)
      .catch((err: unknown) => {
        const setupErr = err as SetupError;
        setHostDirBrowserError(
          setupErr?.message || 'Failed to browse backend directories.',
        );
      })
      .finally(() => {
        setHostDirBrowserLoading(false);
      });
  };

  const closeHostDirectoryBrowser = () => {
    setHostDirBrowserOpen(false);
    setHostDirBrowserLoading(false);
    setHostDirBrowserError(null);
    setHostDirBrowserTargetLibraryId(null);
  };

  const navigateHostDirectory = (path?: string | null) => {
    const target = path?.trim();
    if (!target) return;
    setHostDirBrowserError(null);
    setHostDirBrowserLoading(true);
    void fetchHostDirectories(target)
      .catch((err: unknown) => {
        const setupErr = err as SetupError;
        setHostDirBrowserError(
          setupErr?.message || 'Failed to browse backend directories.',
        );
      })
      .finally(() => {
        setHostDirBrowserLoading(false);
      });
  };

  const confirmHostDirectorySelection = () => {
    if (!hostDirBrowserTargetLibraryId || !hostDirBrowserCurrentPath.trim()) {
      setHostDirBrowserError('No directory selected.');
      return;
    }
    updateLibraryDraft(hostDirBrowserTargetLibraryId, 'path', hostDirBrowserCurrentPath);
    closeHostDirectoryBrowser();
  };

  const handleLibraries = async () => {
    clearErrors();
    const normalized = libraryDrafts.map((draft) => ({
      ...draft,
      name: draft.name.trim(),
      path: draft.path.trim(),
    }));

    const populated = normalized.filter((draft) => draft.name || draft.path);
    if (populated.length === 0) {
      setStep('metadata');
      return;
    }

    const invalidPartial = populated.find((draft) => !draft.name || !draft.path);
    if (invalidPartial) {
      setError('Each library row must include both a name and a path, or be left blank.');
      return;
    }

    const payload: LibrarySpec[] = populated.map((draft) => ({
      name: draft.name,
      kind: draft.kind,
      paths: [draft.path],
      is_read_only: draft.is_read_only,
    }));

    setSaving(true);
    try {
      await createLibraries(payload);
      setStep('metadata');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  // Step 5: Metadata
  const handleMetadata = async () => {
    clearErrors();
    setSaving(true);
    try {
      await putSetupMetadata({
        metadata_language: metaLanguage,
        metadata_region: metaRegion.toUpperCase(),
      });
      setStep('network');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  // Step 6: Network
  const handleNetwork = async () => {
    clearErrors();
    setSaving(true);
    try {
      await putSetupNetwork({
        allow_remote_access: allowRemote,
        enable_automatic_port_mapping: autoPort,
        trusted_proxies: [],
      });
      setStep('complete');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  // Step 7: Complete
  const handleComplete = async () => {
    clearErrors();
    setSaving(true);
    try {
      await completeSetup();
      clearOwnerToken();
      // Auto-login as the admin created during setup
      const res = await fetch('/api/v1/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: adminUsername, password: adminPassword }),
      });
      if (res.ok) {
        const body = await parseResponseBody(res);
        if (body && typeof body === 'object' && typeof (body as { token?: unknown }).token === 'string') {
          localStorage.setItem('token', (body as { token: string }).token);
        }
        await refreshMe();
      }
      router.replace('/');
      setStep('done');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  const stepNames: Record<string, string> = {
    welcome: 'Welcome',
    config: 'Server Config',
    admin: 'Create Admin',
    libraries: 'Libraries (Optional)',
    metadata: 'Metadata',
    network: 'Networking',
    complete: 'Finish',
  };

  const stepOrder: Step[] = ['welcome', 'config', 'admin', 'libraries', 'metadata', 'network', 'complete'];
  const currentIndex = stepOrder.indexOf(step);
  const progressPercent = currentIndex >= 0 ? ((currentIndex + 1) / stepOrder.length) * 100 : 0;
  const inputClass = (hasError: boolean) => `input px-3 py-2 ${hasError ? 'border-[var(--danger)]' : ''}`;

  if (step === 'loading') {
    return (
      <div className="panel-soft flex min-h-[40vh] items-center justify-center">
        <div className="text-sm muted">Checking setup status...</div>
      </div>
    );
  }

  if (step === 'done') {
    return (
      <section className="panel space-y-6 py-10 text-center">
        <div className="text-4xl">Setup Complete</div>
        <p className="text-sm muted sm:text-base">Your Rustyfin server is ready to use.</p>
        <Link
          href="/"
          className="btn-primary inline-flex px-6 py-2.5 text-sm"
        >
          Go to Home
        </Link>
      </section>
    );
  }

  return (
    <div className="space-y-6 animate-rise">
      <div className="panel-soft space-y-3 p-4 sm:p-5">
        <div className="flex items-center justify-between gap-3">
          <span className="chip chip-accent">Setup Progress</span>
          <span className="text-xs muted">
            {Math.max(currentIndex + 1, 1)}/{stepOrder.length}
          </span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
            style={{ width: `${Math.max(progressPercent, 8)}%` }}
          />
        </div>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
          {stepOrder.map((s, i) => (
            <div
              key={s}
              className={`chip justify-center text-center ${i === currentIndex ? 'chip-accent' : ''}`}
            >
              {stepNames[s]}
            </div>
          ))}
        </div>
      </div>

      {error && (
        <div className="notice-error rounded-xl px-4 py-2 text-sm">
          {error}
        </div>
      )}

      {step === 'welcome' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Welcome to Rustyfin</h2>
          <p className="muted">
            Let&apos;s set up your media server. This wizard will guide you through
            configuring your server, creating an admin account, and setting up your preferences.
          </p>
          <form onSubmit={(e) => { e.preventDefault(); handleStart(); }}>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Starting...' : 'Get Started'}
            </button>
          </form>
        </section>
      )}

      {step === 'config' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Server Configuration</h2>
          <form onSubmit={(e) => { e.preventDefault(); handleConfig(); }} className="space-y-4">
            <div>
              <label className="mb-1 block text-sm font-medium muted">Server Name</label>
              <input
                type="text"
                value={serverName}
                onChange={(e) => setServerName(e.target.value)}
                className={inputClass(Boolean(fieldErrors.server_name))}
                maxLength={64}
              />
              {fieldErrors.server_name && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.server_name[0]}</p>
              )}
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Default Locale (BCP-47)</label>
              <input
                type="text"
                value={locale}
                onChange={(e) => setLocale(e.target.value)}
                placeholder="en-US"
                className={inputClass(Boolean(fieldErrors.default_ui_locale))}
              />
              {fieldErrors.default_ui_locale && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.default_ui_locale[0]}</p>
              )}
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Region (ISO 3166-1, e.g. US)</label>
              <input
                type="text"
                value={region}
                onChange={(e) => setRegion(e.target.value)}
                placeholder="US"
                maxLength={2}
                className={inputClass(Boolean(fieldErrors.default_region))}
              />
              {fieldErrors.default_region && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.default_region[0]}</p>
              )}
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Time Zone (IANA, optional)</label>
              <input
                type="text"
                value={timeZone}
                onChange={(e) => setTimeZone(e.target.value)}
                placeholder="America/New_York"
                className={inputClass(false)}
              />
            </div>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Saving...' : 'Next'}
            </button>
          </form>
        </section>
      )}

      {step === 'admin' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Create Admin Account</h2>
          <p className="text-sm muted">
            Create the initial administrator account. You&apos;ll use this to log in and manage your server.
          </p>
          <form onSubmit={(e) => { e.preventDefault(); handleAdmin(); }} className="space-y-4">
            <div>
              <label className="mb-1 block text-sm font-medium muted">Username</label>
              <input
                type="text"
                value={adminUsername}
                onChange={(e) => setAdminUsername(e.target.value)}
                placeholder="admin"
                className={inputClass(Boolean(fieldErrors.username))}
              />
              {fieldErrors.username && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.username[0]}</p>
              )}
              <p className="mt-1 text-xs muted">3-32 characters: letters, numbers, dots, hyphens, underscores</p>
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Password</label>
              <input
                type="password"
                minLength={6}
                value={adminPassword}
                onChange={(e) => setAdminPassword(e.target.value)}
                className={inputClass(Boolean(fieldErrors.password))}
              />
              {fieldErrors.password && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.password[0]}</p>
              )}
              <p className="mt-1 text-xs muted">Minimum 6 characters</p>
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Confirm Password</label>
              <input
                type="password"
                minLength={6}
                value={adminPasswordConfirm}
                onChange={(e) => setAdminPasswordConfirm(e.target.value)}
                className={inputClass(Boolean(fieldErrors.password_confirm))}
              />
              {fieldErrors.password_confirm && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.password_confirm[0]}</p>
              )}
            </div>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Creating...' : 'Next'}
            </button>
          </form>
        </section>
      )}

      {step === 'libraries' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Libraries (Optional)</h2>
          <p className="text-sm muted">
            Add media libraries now, or skip and configure them later in Admin. Browse uses the backend host filesystem.
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void handleLibraries();
            }}
            className="space-y-4"
          >
            <div className="space-y-3">
              {libraryDrafts.map((draft, idx) => (
                <div key={draft.id} className="tile space-y-3 p-3">
                  <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.1fr_0.8fr_2fr_auto]">
                    <input
                      type="text"
                      value={draft.name}
                      onChange={(e) => updateLibraryDraft(draft.id, 'name', e.target.value)}
                      placeholder={`Library ${idx + 1} name`}
                      className="input px-3 py-2 text-sm"
                    />
                    <select
                      value={draft.kind}
                      onChange={(e) =>
                        updateLibraryDraft(
                          draft.id,
                          'kind',
                          e.target.value as 'movies' | 'tv_shows' | 'music',
                        )
                      }
                      className="select px-3 py-2 text-sm"
                    >
                      <option value="movies">Movies</option>
                      <option value="tv_shows">TV Shows</option>
                      <option value="music">Music</option>
                    </select>
                    <input
                      type="text"
                      value={draft.path}
                      onChange={(e) => updateLibraryDraft(draft.id, 'path', e.target.value)}
                      placeholder="/media/movies"
                      className="input px-3 py-2 text-sm"
                    />
                    <button
                      type="button"
                      onClick={() => openHostDirectoryBrowser(draft.id, draft.path)}
                      className="btn-secondary px-4 py-2 text-sm"
                    >
                      Browse Host
                    </button>
                  </div>
                  <div className="flex items-center justify-between gap-2">
                    <label className="panel-soft inline-flex items-center gap-2 px-3 py-2 text-sm">
                      <input
                        type="checkbox"
                        checked={draft.is_read_only}
                        onChange={(e) =>
                          updateLibraryDraft(draft.id, 'is_read_only', e.target.checked)
                        }
                        className="h-4 w-4 [accent-color:var(--purple)]"
                      />
                      <span>Read only</span>
                    </label>
                    {libraryDrafts.length > 1 && (
                      <button
                        type="button"
                        onClick={() => removeLibraryDraft(draft.id)}
                        className="btn-ghost px-3 py-1.5 text-xs text-[var(--danger)]"
                      >
                        Remove
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={addLibraryDraft}
                className="btn-secondary px-4 py-2 text-sm"
              >
                Add Library
              </button>
              <button
                type="button"
                onClick={() => setStep('metadata')}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Skip for now
              </button>
              <button
                type="submit"
                disabled={saving}
                className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
              >
                {saving ? 'Saving...' : 'Next'}
              </button>
            </div>
          </form>
        </section>
      )}

      {step === 'metadata' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Metadata Preferences</h2>
          <p className="text-sm muted">
            Set the default language and region for fetching metadata (titles, descriptions, artwork).
          </p>
          <form onSubmit={(e) => { e.preventDefault(); handleMetadata(); }} className="space-y-4">
            <div>
              <label className="mb-1 block text-sm font-medium muted">Metadata Language</label>
              <input
                type="text"
                value={metaLanguage}
                onChange={(e) => setMetaLanguage(e.target.value)}
                placeholder="en"
                className={inputClass(Boolean(fieldErrors.metadata_language))}
              />
              {fieldErrors.metadata_language && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.metadata_language[0]}</p>
              )}
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Metadata Region</label>
              <input
                type="text"
                value={metaRegion}
                onChange={(e) => setMetaRegion(e.target.value)}
                placeholder="US"
                maxLength={2}
                className={inputClass(Boolean(fieldErrors.metadata_region))}
              />
              {fieldErrors.metadata_region && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.metadata_region[0]}</p>
              )}
            </div>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Saving...' : 'Next'}
            </button>
          </form>
        </section>
      )}

      {step === 'network' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Network Settings</h2>
          <p className="text-sm muted">
            Configure remote access and network options.
          </p>
          <form onSubmit={(e) => { e.preventDefault(); handleNetwork(); }} className="space-y-4">
            <label className="panel-soft flex cursor-pointer items-center gap-3 px-4 py-3">
              <input
                type="checkbox"
                checked={allowRemote}
                onChange={(e) => setAllowRemote(e.target.checked)}
                className="h-4 w-4 rounded border-white/30 bg-black/20 [accent-color:var(--purple)]"
              />
              <span className="text-sm">Allow remote access to this server</span>
            </label>
            <label className="panel-soft flex cursor-pointer items-center gap-3 px-4 py-3">
              <input
                type="checkbox"
                checked={autoPort}
                onChange={(e) => setAutoPort(e.target.checked)}
                className="h-4 w-4 rounded border-white/30 bg-black/20 [accent-color:var(--purple)]"
              />
              <span className="text-sm">Enable automatic port mapping (UPnP)</span>
            </label>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Saving...' : 'Next'}
            </button>
          </form>
        </section>
      )}

      {step === 'complete' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Ready to Go</h2>
          <p className="muted">
            Your server is configured and ready. Click &quot;Finish&quot; to complete setup and start
            using Rustyfin.
          </p>
          <div className="panel-soft space-y-2 rounded-xl p-4 text-sm">
            <div><span className="muted">Server:</span> {serverName}</div>
            <div><span className="muted">Admin:</span> {adminUsername}</div>
            <div><span className="muted">Locale:</span> {locale} / {region}</div>
            <div>
              <span className="muted">Libraries:</span>{' '}
              {
                libraryDrafts.filter((draft) => draft.name.trim() && draft.path.trim()).length
              } configured
            </div>
            <div><span className="muted">Remote Access:</span> {allowRemote ? 'Enabled' : 'Disabled'}</div>
          </div>
          <form onSubmit={(e) => { e.preventDefault(); handleComplete(); }}>
            <button
              type="submit"
              disabled={saving}
              className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
            >
              {saving ? 'Completing...' : 'Finish Setup'}
            </button>
          </form>
        </section>
      )}

      {hostDirBrowserOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 backdrop-blur-[2px] p-4">
          <div className="panel w-full max-w-3xl max-h-[82vh] rounded-2xl border border-[var(--border)] p-4 sm:p-5 flex flex-col gap-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <h3 className="text-lg font-semibold">Browse Backend Directories</h3>
                <p className="text-xs muted">
                  Choose a folder on the server host (Debian 12) for this library path.
                </p>
              </div>
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-3 py-1.5 text-sm"
              >
                Close
              </button>
            </div>

            {hostDirBrowserRoots.length > 0 && (
              <div className="flex flex-wrap gap-2">
                {hostDirBrowserRoots.map((rootPath) => (
                  <button
                    key={rootPath}
                    type="button"
                    onClick={() => navigateHostDirectory(rootPath)}
                    className={`btn-ghost px-2.5 py-1 text-xs ${
                      hostDirBrowserCurrentPath.startsWith(rootPath)
                        ? 'border-[var(--orange-soft)] text-[var(--orange-soft)]'
                        : ''
                    }`}
                  >
                    {rootPath}
                  </button>
                ))}
              </div>
            )}

            <div className="panel-soft rounded-xl border border-[var(--border)] px-3 py-2 flex items-center gap-2">
              <button
                type="button"
                onClick={() => navigateHostDirectory(hostDirBrowserParentPath)}
                disabled={!hostDirBrowserParentPath || hostDirBrowserLoading}
                className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
              >
                Up
              </button>
              <div className="min-w-0">
                <p className="text-[11px] uppercase tracking-[0.12em] muted">Current Path</p>
                <p className="text-sm font-mono truncate" title={hostDirBrowserCurrentPath}>
                  {hostDirBrowserCurrentPath || '—'}
                </p>
              </div>
            </div>

            {hostDirBrowserError && <p className="text-sm text-red-300">{hostDirBrowserError}</p>}

            <div className="panel-soft min-h-[260px] overflow-auto rounded-xl border border-[var(--border)] p-2">
              {hostDirBrowserLoading ? (
                <p className="px-2 py-2 text-sm muted">Loading directories…</p>
              ) : hostDirBrowserDirectories.length === 0 ? (
                <p className="px-2 py-2 text-sm muted">No child directories found.</p>
              ) : (
                <div className="space-y-1">
                  {hostDirBrowserDirectories.map((entry) => (
                    <button
                      key={entry.path}
                      type="button"
                      onClick={() => navigateHostDirectory(entry.path)}
                      className="w-full rounded-lg border border-[var(--border)] bg-[var(--panel)]/65 px-3 py-2 text-left text-sm hover:border-[var(--orange-soft)]/55 hover:bg-[var(--panel)]"
                      title={entry.path}
                    >
                      <div className="font-medium">{entry.name}</div>
                      <div className="truncate text-xs muted">{entry.path}</div>
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={confirmHostDirectorySelection}
                disabled={hostDirBrowserLoading || !hostDirBrowserCurrentPath}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                Use This Folder
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
