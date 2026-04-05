'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { format } from 'date-fns';

import { useAuth } from '@/lib/auth';
import {
  BackupJob,
  BackupPolicy,
  createBackupJob,
  listJobs,
  listPolicies,
  restoreBackup,
} from '@/lib/backupsApi';
import { clientErrorMessage } from '@/lib/errors';

function JobStatusBadge({ status }: { status: string }) {
  if (status === 'success') {
    return (
      <span className="chip border-[var(--ok)] bg-[var(--ok-dim)] text-[var(--ok)]">
        Success
      </span>
    );
  }
  if (status === 'failed') {
    return <span className="chip border-red-500/30 bg-red-900/20 text-red-300">Failed</span>;
  }
  if (status === 'running') {
    return (
      <span className="chip animate-pulse border-blue-500/30 bg-blue-900/20 text-blue-300">
        Running
      </span>
    );
  }
  return <span className="chip">{status}</span>;
}

function formatBytes(bytes: number | null | undefined) {
  if (bytes === undefined || bytes === null || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
}

export default function BackupsPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [jobs, setJobs] = useState<BackupJob[]>([]);
  const [policies, setPolicies] = useState<BackupPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'history' | 'policies'>('history');

  const fetchLists = async () => {
    try {
      setLoading(true);
      const [jobsData, policiesData] = await Promise.all([listJobs(), listPolicies()]);
      setJobs(jobsData);
      setPolicies(policiesData);
    } catch (err) {
      setError(clientErrorMessage(err, 'Failed to load backup data.'));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (authLoading || !me) return;
    void fetchLists();
  }, [authLoading, me]);

  const handleCreateBackup = async () => {
    try {
      await createBackupJob();
      setTimeout(() => {
        void fetchLists();
      }, 1000);
    } catch (err) {
      setError(clientErrorMessage(err, 'Failed to start backup.'));
    }
  };

  const handleRestore = async (jobId: string) => {
    if (
      !confirm(
        'WARNING: Restore will overwrite current database and restart the server. Are you sure?',
      )
    ) {
      return;
    }
    try {
      await restoreBackup(jobId);
      alert('Restore initiated. Server will restart shortly. Please refresh the page in a minute.');
    } catch (err) {
      alert(`Restore failed: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  if (authLoading || !me) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading...</p>
      </div>
    );
  }

  return (
    <div className="animate-rise rf-flat-page">
      <header className="rf-flat-header flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="space-y-2">
          <h1 className="text-2xl font-semibold sm:text-3xl">Backups</h1>
          <p className="max-w-3xl text-sm muted">
            Manage scheduled backups and manual snapshots.
          </p>
        </div>
        <div className="shrink-0">
          <button onClick={handleCreateBackup} className="btn-primary px-4 py-2 text-sm">
            Back Up Now
          </button>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <div className="rf-top-tabbar border-b border-[var(--border-subtle)] pb-0">
        <button
          className="rf-top-tab"
          data-active={activeTab === 'history'}
          onClick={() => setActiveTab('history')}
        >
          History
        </button>
        <button
          className="rf-top-tab"
          data-active={activeTab === 'policies'}
          onClick={() => setActiveTab('policies')}
        >
          Policies
        </button>
      </div>

      {loading ? (
        <div className="rf-flat-empty text-center muted">Loading...</div>
      ) : activeTab === 'history' ? (
        <section className="rf-flat-section">
          {jobs.length === 0 ? (
            <div className="rf-flat-empty text-center muted">No backup jobs found.</div>
          ) : (
            <div className="rf-flat-list">
              {jobs.map((job) => (
                <article
                  key={job.id}
                  className="rf-flat-row flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between"
                >
                  <div className="space-y-1">
                    <div className="flex flex-wrap items-center gap-3">
                      <JobStatusBadge status={job.status} />
                      <span className="font-mono text-sm text-slate-400">
                        {job.id.substring(0, 8)}
                      </span>
                      <span className="text-sm capitalize muted">{job.trigger_type}</span>
                    </div>
                    <div className="text-sm">
                      Started: {format(new Date(job.start_ts * 1000), 'PPpp')}
                    </div>
                    {job.end_ts && (
                      <div className="text-xs muted">Duration: {job.end_ts - job.start_ts}s</div>
                    )}
                    {job.error_message && (
                      <div className="mt-1 text-xs text-red-400">{job.error_message}</div>
                    )}
                  </div>
                  <div className="flex flex-col gap-1 text-left sm:items-end sm:text-right">
                    <div className="text-lg font-light">{formatBytes(job.total_size_bytes)}</div>
                    {job.status === 'success' && (
                      <>
                        <div className="text-xs text-[var(--ok)]">Verified</div>
                        <button
                          onClick={() => handleRestore(job.id)}
                          className="text-xs text-red-400 underline hover:text-red-300"
                        >
                          Restore
                        </button>
                      </>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      ) : (
        <section className="rf-flat-section">
          {policies.length === 0 ? (
            <div className="rf-flat-empty text-center muted">No scheduled policies defined.</div>
          ) : (
            <div className="rf-flat-list">
              {policies.map((policy) => (
                <article key={policy.id} className="rf-flat-row space-y-2">
                  <div className="flex items-start justify-between gap-4">
                    <h3 className="font-semibold">{policy.name}</h3>
                    <span className={`chip ${policy.enabled ? 'text-[var(--ok)]' : 'muted'}`}>
                      {policy.enabled ? 'Enabled' : 'Disabled'}
                    </span>
                  </div>
                  <div className="text-sm text-slate-300">
                    {policy.schedule_cron ? `Schedule: ${policy.schedule_cron}` : 'Manual Only'}
                  </div>
                  <div className="text-sm muted">Retain: {policy.retention_count} snapshots</div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {policy.include_database && <span className="chip text-xs">DB</span>}
                    {policy.include_server_config && <span className="chip text-xs">Config</span>}
                    {policy.include_server_worlds && <span className="chip text-xs">Worlds</span>}
                  </div>
                </article>
              ))}
            </div>
          )}

          <div className="rf-flat-empty text-center text-sm muted">
            To create a new policy, please use the configuration file or CLI for now.
          </div>
        </section>
      )}
    </div>
  );
}
