'use client';

import { useEffect, useState, useCallback } from 'react';
import {
  getPublicSystemInfo,
  claimSession,
  setOwnerToken,
  clearOwnerToken,
  putSetupConfig,
  createAdmin,
  putSetupMetadata,
  putSetupNetwork,
  completeSetup,
  type SetupError,
} from '@/lib/setupApi';

type Step = 'loading' | 'welcome' | 'config' | 'admin' | 'libraries' | 'metadata' | 'network' | 'complete' | 'done';

interface FieldErrors {
  [key: string]: string[];
}

export default function SetupWizard() {
  const [step, setStep] = useState<Step>('loading');
  const [error, setError] = useState<string | null>(null);
  const [fieldErrors, setFieldErrors] = useState<FieldErrors>({});
  const [saving, setSaving] = useState(false);

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

  useEffect(() => {
    getPublicSystemInfo()
      .then((info) => {
        if (info.setup_completed) {
          window.location.href = '/';
        } else {
          setStep('welcome');
        }
      })
      .catch(() => setStep('welcome'));
  }, []);

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
      const idempotencyKey = crypto.randomUUID();
      await createAdmin(adminUsername, adminPassword, idempotencyKey);
      setStep('metadata');
    } catch (err) {
      handleError(err);
    }
    setSaving(false);
  };

  // Step 4: Metadata (skipping libraries for simplicity — optional step)
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

  // Step 5: Network
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

  // Step 6: Complete
  const handleComplete = async () => {
    clearErrors();
    setSaving(true);
    try {
      await completeSetup();
      clearOwnerToken();
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
    libraries: 'Libraries',
    metadata: 'Metadata',
    network: 'Networking',
    complete: 'Finish',
  };

  const stepOrder: Step[] = ['welcome', 'config', 'admin', 'metadata', 'network', 'complete'];
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
        <a
          href="/login"
          className="btn-primary inline-flex px-6 py-2.5 text-sm"
        >
          Go to Login
        </a>
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
          <button
            onClick={handleStart}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Starting...' : 'Get Started'}
          </button>
        </section>
      )}

      {step === 'config' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Server Configuration</h2>
          <div className="space-y-4">
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
          </div>
          <button
            onClick={handleConfig}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
        </section>
      )}

      {step === 'admin' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Create Admin Account</h2>
          <p className="text-sm muted">
            Create the initial administrator account. You&apos;ll use this to log in and manage your server.
          </p>
          <div className="space-y-4">
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
                value={adminPassword}
                onChange={(e) => setAdminPassword(e.target.value)}
                className={inputClass(Boolean(fieldErrors.password))}
              />
              {fieldErrors.password && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.password[0]}</p>
              )}
              <p className="mt-1 text-xs muted">Minimum 12 characters</p>
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium muted">Confirm Password</label>
              <input
                type="password"
                value={adminPasswordConfirm}
                onChange={(e) => setAdminPasswordConfirm(e.target.value)}
                className={inputClass(Boolean(fieldErrors.password_confirm))}
              />
              {fieldErrors.password_confirm && (
                <p className="mt-1 text-xs text-[var(--danger)]">{fieldErrors.password_confirm[0]}</p>
              )}
            </div>
          </div>
          <button
            onClick={handleAdmin}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Creating...' : 'Next'}
          </button>
        </section>
      )}

      {step === 'metadata' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Metadata Preferences</h2>
          <p className="text-sm muted">
            Set the default language and region for fetching metadata (titles, descriptions, artwork).
          </p>
          <div className="space-y-4">
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
          </div>
          <button
            onClick={handleMetadata}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
        </section>
      )}

      {step === 'network' && (
        <section className="panel space-y-6 p-6 sm:p-7">
          <h2 className="text-2xl font-semibold sm:text-3xl">Network Settings</h2>
          <p className="text-sm muted">
            Configure remote access and network options.
          </p>
          <div className="space-y-4">
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
          </div>
          <button
            onClick={handleNetwork}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
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
            <div><span className="muted">Remote Access:</span> {allowRemote ? 'Enabled' : 'Disabled'}</div>
          </div>
          <button
            onClick={handleComplete}
            disabled={saving}
            className="btn-primary px-6 py-2.5 text-sm disabled:opacity-50"
          >
            {saving ? 'Completing...' : 'Finish Setup'}
          </button>
        </section>
      )}
    </div>
  );
}
