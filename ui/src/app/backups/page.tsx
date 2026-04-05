'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import { downloadAccountBackupArchive } from '@/lib/backupsApi';
import { clientErrorMessage } from '@/lib/errors';
import {
  challengeRustyVaultProtectedAction,
  exportRustyVault,
  getRustyVaultConfig,
} from '@/features/rustyvault/api';
import { getMyRustyVaultPreferences } from '@/features/rustyvault/preferences';
import {
  ensureRustyVaultWebSession,
  refreshStoredRustyVaultSession,
} from '@/features/rustyvault/session';

type BackupTab = 'accounts' | 'gallery';

async function withRustyVaultAccess<T>(
  callback: (accessToken: string) => Promise<T>,
): Promise<T> {
  const current = await ensureRustyVaultWebSession();
  try {
    return await callback(current.access_token);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (message.includes('401') || message.toLowerCase().includes('unauthorized')) {
      const refreshed = await refreshStoredRustyVaultSession(current);
      return callback(refreshed.access_token);
    }
    throw error;
  }
}

function triggerDownload(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

export default function BackupsPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [activeTab, setActiveTab] = useState<BackupTab>('accounts');
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [vaultPassword, setVaultPassword] = useState('');

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  async function handleDownloadAccountBackup() {
    if (!me) return;

    try {
      setDownloading(true);
      setError(null);
      setStatus('Building account archive...');

      let vaultExportJson: unknown;
      let vaultPreferencesJson: unknown;
      let includedVault = false;

      if (vaultPassword.trim()) {
        setStatus('Including RustyVault snapshot...');
        await withRustyVaultAccess(async (accessToken) => {
          const config = await getRustyVaultConfig(accessToken);
          if (!config.enabled) {
            return;
          }
          const prefs = await getMyRustyVaultPreferences(accessToken);
          const challenge = await challengeRustyVaultProtectedAction({
            action_kind: 'export',
            current_password: vaultPassword,
            vaultAccessToken: accessToken,
          });
          const exportPayload = await exportRustyVault(accessToken, challenge.action_token);
          vaultExportJson = exportPayload;
          vaultPreferencesJson = prefs;
          includedVault = true;
        });
      }

      setStatus('Compressing archive...');
      const archive = await downloadAccountBackupArchive({
        vault_export_json: vaultExportJson,
        vault_preferences_json: vaultPreferencesJson,
      });
      triggerDownload(
        archive,
        `rustyfin-account-backup-${me.username}-${new Date().toISOString().slice(0, 10)}.zip`,
      );
      setStatus(
        includedVault
          ? 'Account backup downloaded with RustyVault snapshot.'
          : 'Account backup downloaded.',
      );
    } catch (error) {
      setStatus(null);
      setError(clientErrorMessage(error, 'Failed to create account backup.'));
    } finally {
      setDownloading(false);
    }
  }

  if (authLoading || !me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading...</p>
      </div>
    );
  }

  return (
    <div className="animate-rise rf-flat-page">
      <header className="rf-flat-header flex flex-col gap-4">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold sm:text-3xl">Backups</h1>
          <p className="max-w-3xl text-sm muted">
            Export your own account state as a compressed archive, with a separate gallery section
            reserved for media-focused backup flows.
          </p>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {status && <div className="text-sm muted">{status}</div>}

      <div className="rf-top-tabbar border-b border-[var(--border-subtle)] pb-0">
        <button
          className="rf-top-tab"
          data-active={activeTab === 'accounts'}
          onClick={() => setActiveTab('accounts')}
        >
          Accounts
        </button>
        <button
          className="rf-top-tab"
          data-active={activeTab === 'gallery'}
          onClick={() => setActiveTab('gallery')}
        >
          Gallery
        </button>
      </div>

      {activeTab === 'accounts' ? (
        <section className="rf-flat-section">
          <div className="rf-flat-list">
            <article className="rf-flat-row space-y-5">
              <div className="space-y-2">
                <h2 className="text-xl font-semibold">Account Archive</h2>
                <p className="max-w-3xl text-sm muted">
                  Download a user-scoped backup that can be unzipped and loaded back into place
                  later. The archive includes account profile data, preferences, AI chat history,
                  watch history, continue-watching state, and playback progress.
                </p>
              </div>

              <div className="grid gap-3 text-sm text-slate-200 sm:grid-cols-2">
                <div className="rf-flat-row space-y-1">
                  <h3 className="font-medium">Included</h3>
                  <p className="muted">Profile and account preferences</p>
                  <p className="muted">AI conversations and turn history</p>
                  <p className="muted">Activity history, playback state, and continue watching</p>
                </div>
                <div className="rf-flat-row space-y-1">
                  <h3 className="font-medium">Optional</h3>
                  <p className="muted">RustyVault export snapshot</p>
                  <p className="muted">
                    Enter your Rustyfin password below if you want the protected RustyVault export
                    payload bundled into the archive as well.
                  </p>
                </div>
              </div>

              <label className="flex max-w-xl flex-col gap-2 text-sm">
                <span className="muted">Rustyfin password for RustyVault snapshot (optional)</span>
                <input
                  type="password"
                  value={vaultPassword}
                  onChange={(event) => setVaultPassword(event.target.value)}
                  placeholder="Leave blank to export account data without vault snapshot"
                  className="rounded-full border border-white/10 bg-black/10 px-4 py-3 text-white outline-none transition focus:border-white/25"
                />
              </label>

              <div className="flex flex-wrap items-center gap-4">
                <button
                  onClick={handleDownloadAccountBackup}
                  className="btn-primary px-4 py-2 text-sm"
                  disabled={downloading}
                >
                  {downloading ? 'Preparing Backup...' : 'Download Account Backup'}
                </button>
                <p className="text-xs muted">
                  The archive downloads as a `.zip` containing sectioned JSON snapshots.
                </p>
              </div>
            </article>
          </div>
        </section>
      ) : (
        <section className="rf-flat-section">
          <div className="rf-flat-empty text-left">
            <h2 className="text-xl font-semibold">Gallery</h2>
            <p className="mt-2 max-w-2xl text-sm muted">
              Gallery backups are split out here so personal media and gallery-oriented exports can
              evolve separately from account-state archives.
            </p>
          </div>
        </section>
      )}
    </div>
  );
}
