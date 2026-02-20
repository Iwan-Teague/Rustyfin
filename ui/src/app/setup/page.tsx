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

  if (step === 'loading') {
    return (
      <div className="flex items-center justify-center min-h-[50vh]">
        <div className="text-gray-400">Checking setup status...</div>
      </div>
    );
  }

  if (step === 'done') {
    return (
      <div className="space-y-6 text-center py-12">
        <div className="text-5xl">🎉</div>
        <h2 className="text-2xl font-bold">Setup Complete!</h2>
        <p className="text-gray-400">Your Rustyfin server is ready to use.</p>
        <a
          href="/login"
          className="inline-block px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 transition"
        >
          Go to Login
        </a>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Progress bar */}
      <div className="flex gap-2 mb-8">
        {stepOrder.map((s, i) => (
          <div key={s} className="flex-1">
            <div
              className={`h-2 rounded-full ${
                i <= currentIndex ? 'bg-blue-500' : 'bg-gray-700'
              }`}
            />
            <div className={`text-xs mt-1 text-center ${
              i === currentIndex ? 'text-blue-400 font-semibold' : 'text-gray-500'
            }`}>
              {stepNames[s]}
            </div>
          </div>
        ))}
      </div>

      {/* Error display */}
      {error && (
        <div className="p-4 bg-red-900/50 border border-red-700 rounded-lg text-red-200 text-sm">
          {error}
        </div>
      )}

      {/* Step content */}
      {step === 'welcome' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Welcome to Rustyfin</h2>
          <p className="text-gray-400">
            Let&apos;s set up your media server. This wizard will guide you through
            configuring your server, creating an admin account, and setting up your preferences.
          </p>
          <button
            onClick={handleStart}
            disabled={saving}
            className="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 disabled:opacity-50 transition"
          >
            {saving ? 'Starting...' : 'Get Started'}
          </button>
        </div>
      )}

      {step === 'config' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Server Configuration</h2>
          <div className="space-y-4">
            <div>
              <label className="block text-sm text-gray-300 mb-1">Server Name</label>
              <input
                type="text"
                value={serverName}
                onChange={(e) => setServerName(e.target.value)}
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.server_name ? 'border-red-500' : 'border-gray-700'
                }`}
                maxLength={64}
              />
              {fieldErrors.server_name && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.server_name[0]}</p>
              )}
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Default Locale (BCP-47)</label>
              <input
                type="text"
                value={locale}
                onChange={(e) => setLocale(e.target.value)}
                placeholder="en-US"
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.default_ui_locale ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.default_ui_locale && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.default_ui_locale[0]}</p>
              )}
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Region (ISO 3166-1, e.g. US)</label>
              <input
                type="text"
                value={region}
                onChange={(e) => setRegion(e.target.value)}
                placeholder="US"
                maxLength={2}
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.default_region ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.default_region && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.default_region[0]}</p>
              )}
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Time Zone (IANA, optional)</label>
              <input
                type="text"
                value={timeZone}
                onChange={(e) => setTimeZone(e.target.value)}
                placeholder="America/New_York"
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg focus:outline-none focus:border-blue-500"
              />
            </div>
          </div>
          <button
            onClick={handleConfig}
            disabled={saving}
            className="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 disabled:opacity-50 transition"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
        </div>
      )}

      {step === 'admin' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Create Admin Account</h2>
          <p className="text-gray-400 text-sm">
            Create the initial administrator account. You&apos;ll use this to log in and manage your server.
          </p>
          <div className="space-y-4">
            <div>
              <label className="block text-sm text-gray-300 mb-1">Username</label>
              <input
                type="text"
                value={adminUsername}
                onChange={(e) => setAdminUsername(e.target.value)}
                placeholder="admin"
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.username ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.username && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.username[0]}</p>
              )}
              <p className="text-gray-500 text-xs mt-1">3-32 characters: letters, numbers, dots, hyphens, underscores</p>
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Password</label>
              <input
                type="password"
                value={adminPassword}
                onChange={(e) => setAdminPassword(e.target.value)}
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.password ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.password && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.password[0]}</p>
              )}
              <p className="text-gray-500 text-xs mt-1">Minimum 12 characters</p>
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Confirm Password</label>
              <input
                type="password"
                value={adminPasswordConfirm}
                onChange={(e) => setAdminPasswordConfirm(e.target.value)}
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.password_confirm ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.password_confirm && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.password_confirm[0]}</p>
              )}
            </div>
          </div>
          <button
            onClick={handleAdmin}
            disabled={saving}
            className="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 disabled:opacity-50 transition"
          >
            {saving ? 'Creating...' : 'Next'}
          </button>
        </div>
      )}

      {step === 'metadata' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Metadata Preferences</h2>
          <p className="text-gray-400 text-sm">
            Set the default language and region for fetching metadata (titles, descriptions, artwork).
          </p>
          <div className="space-y-4">
            <div>
              <label className="block text-sm text-gray-300 mb-1">Metadata Language</label>
              <input
                type="text"
                value={metaLanguage}
                onChange={(e) => setMetaLanguage(e.target.value)}
                placeholder="en"
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.metadata_language ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.metadata_language && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.metadata_language[0]}</p>
              )}
            </div>
            <div>
              <label className="block text-sm text-gray-300 mb-1">Metadata Region</label>
              <input
                type="text"
                value={metaRegion}
                onChange={(e) => setMetaRegion(e.target.value)}
                placeholder="US"
                maxLength={2}
                className={`w-full px-3 py-2 bg-gray-800 border rounded-lg focus:outline-none focus:border-blue-500 ${
                  fieldErrors.metadata_region ? 'border-red-500' : 'border-gray-700'
                }`}
              />
              {fieldErrors.metadata_region && (
                <p className="text-red-400 text-xs mt-1">{fieldErrors.metadata_region[0]}</p>
              )}
            </div>
          </div>
          <button
            onClick={handleMetadata}
            disabled={saving}
            className="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 disabled:opacity-50 transition"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
        </div>
      )}

      {step === 'network' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Network Settings</h2>
          <p className="text-gray-400 text-sm">
            Configure remote access and network options.
          </p>
          <div className="space-y-4">
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={allowRemote}
                onChange={(e) => setAllowRemote(e.target.checked)}
                className="w-5 h-5 rounded border-gray-600 bg-gray-800"
              />
              <span>Allow remote access to this server</span>
            </label>
            <label className="flex items-center gap-3 cursor-pointer">
              <input
                type="checkbox"
                checked={autoPort}
                onChange={(e) => setAutoPort(e.target.checked)}
                className="w-5 h-5 rounded border-gray-600 bg-gray-800"
              />
              <span>Enable automatic port mapping (UPnP)</span>
            </label>
          </div>
          <button
            onClick={handleNetwork}
            disabled={saving}
            className="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-500 disabled:opacity-50 transition"
          >
            {saving ? 'Saving...' : 'Next'}
          </button>
        </div>
      )}

      {step === 'complete' && (
        <div className="space-y-6">
          <h2 className="text-2xl font-bold">Ready to Go!</h2>
          <p className="text-gray-400">
            Your server is configured and ready. Click &quot;Finish&quot; to complete setup and start
            using Rustyfin.
          </p>
          <div className="p-4 bg-gray-800 rounded-lg space-y-2 text-sm">
            <div><span className="text-gray-400">Server:</span> {serverName}</div>
            <div><span className="text-gray-400">Admin:</span> {adminUsername}</div>
            <div><span className="text-gray-400">Locale:</span> {locale} / {region}</div>
            <div><span className="text-gray-400">Remote Access:</span> {allowRemote ? 'Enabled' : 'Disabled'}</div>
          </div>
          <button
            onClick={handleComplete}
            disabled={saving}
            className="px-6 py-3 bg-green-600 text-white rounded-lg hover:bg-green-500 disabled:opacity-50 transition"
          >
            {saving ? 'Completing...' : 'Finish Setup'}
          </button>
        </div>
      )}
    </div>
  );
}
