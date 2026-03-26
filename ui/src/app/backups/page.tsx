'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { BackupJob, BackupPolicy, listJobs, listPolicies, createBackupJob, restoreBackup } from '@/lib/backupsApi';
import { clientErrorMessage } from '@/lib/errors';
import { format } from 'date-fns';

function JobStatusBadge({ status }: { status: string }) {
  if (status === 'success') {
    return <span className="chip text-[var(--ok)] bg-[var(--ok-dim)] border-[var(--ok)]">Success</span>;
  }
  if (status === 'failed') {
    return <span className="chip text-red-300 bg-red-900/20 border-red-500/30">Failed</span>;
  }
  if (status === 'running') {
    return <span className="chip text-blue-300 bg-blue-900/20 border-blue-500/30 animate-pulse">Running</span>;
  }
  return <span className="chip">{status}</span>;
}

function formatBytes(bytes: number | null | undefined) {
  if (bytes === undefined || bytes === null || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
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
    fetchLists();
  }, [authLoading, me]);

  const handleCreateBackup = async () => {
    try {
      await createBackupJob();
      // Refresh list after a moment or redirect
      setTimeout(fetchLists, 1000);
    } catch (err) {
      setError(clientErrorMessage(err, 'Failed to start backup.'));
    }
  };

  const handleRestore = async (jobId: string) => {
    if (!confirm('WARNING: Restore will overwrite current database and restart the server. Are you sure?')) {
      return;
    }
    try {
      await restoreBackup(jobId);
      alert('Restore initiated. Server will restart shortly. Please refresh the page in a minute.');
    } catch (err) {
      alert('Restore failed: ' + (err instanceof Error ? err.message : String(err)));
    }
  };

  if (authLoading || !me) {
    return <div className="panel-soft animate-rise px-5 py-4"><p className="text-sm muted">Loading...</p></div>;
  }

  return (
    <div className="space-y-8 animate-rise">
      <header className="panel overflow-hidden p-6 sm:p-8 flex items-center justify-between">
        <div className="space-y-4">
          <span className="chip border-[var(--border-strong)] bg-black/20 text-white/90">
            System Backups
          </span>
          <div className="space-y-2">
            <h1 className="text-3xl font-semibold sm:text-4xl">Backups</h1>
            <p className="max-w-3xl text-sm muted sm:text-base">
              Manage scheduled backups and manual snapshots.
            </p>
          </div>
        </div>
        <button onClick={handleCreateBackup} className="btn-primary px-4 py-2">
          Back Up Now
        </button>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <div className="flex gap-4 border-b border-[var(--border-subtle)] pb-2">
        <button
          className={`px-4 py-2 text-sm font-medium ${activeTab === 'history' ? 'text-white border-b-2 border-[var(--orange)]' : 'text-slate-400 hover:text-white'}`}
          onClick={() => setActiveTab('history')}
        >
          History
        </button>
        <button
          className={`px-4 py-2 text-sm font-medium ${activeTab === 'policies' ? 'text-white border-b-2 border-[var(--orange)]' : 'text-slate-400 hover:text-white'}`}
          onClick={() => setActiveTab('policies')}
        >
          Policies
        </button>
      </div>

      {loading ? (
        <div className="panel-soft px-5 py-8 text-center muted">Loading...</div>
      ) : activeTab === 'history' ? (
        <div className="space-y-4">
          {jobs.length === 0 ? (
            <div className="panel-soft px-5 py-8 text-center muted">No backup jobs found.</div>
          ) : (
            jobs.map((job) => (
              <article key={job.id} className="tile p-5 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
                <div className="space-y-1">
                  <div className="flex items-center gap-3">
                    <JobStatusBadge status={job.status} />
                    <span className="font-mono text-sm text-slate-400">{job.id.substring(0, 8)}</span>
                    <span className="text-sm muted capitalize">{job.trigger_type}</span>
                  </div>
                  <div className="text-sm">
                    Started: {format(new Date(job.start_ts * 1000), 'PPpp')}
                  </div>
                  {job.end_ts && (
                    <div className="text-xs muted">
                      Duration: {job.end_ts - job.start_ts}s
                    </div>
                  )}
                  {job.error_message && (
                    <div className="text-xs text-red-400 mt-1">{job.error_message}</div>
                  )}
                </div>
                <div className="text-right flex flex-col items-end gap-1">
                  <div className="text-lg font-light">{formatBytes(job.total_size_bytes)}</div>
                  {job.status === 'success' && (
                    <>
                      <div className="text-xs text-[var(--ok)]">Verified</div>
                      <button 
                        onClick={() => handleRestore(job.id)}
                        className="text-xs text-red-400 hover:text-red-300 underline"
                      >
                        Restore
                      </button>
                    </>
                  )}
                </div>
              </article>
            ))
          )}
        </div>
      ) : (
        <div className="space-y-4">
          {policies.length === 0 ? (
            <div className="panel-soft px-5 py-8 text-center muted">No scheduled policies defined.</div>
          ) : (
            policies.map((policy) => (
              <article key={policy.id} className="tile p-5 space-y-2">
                <div className="flex justify-between items-start">
                  <h3 className="font-semibold">{policy.name}</h3>
                  <span className={`chip ${policy.enabled ? 'text-[var(--ok)]' : 'muted'}`}>
                    {policy.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                </div>
                <div className="text-sm text-slate-300">
                  {policy.schedule_cron ? `Schedule: ${policy.schedule_cron}` : 'Manual Only'}
                </div>
                <div className="text-sm muted">
                  Retain: {policy.retention_count} snapshots
                </div>
                <div className="flex gap-2 mt-2">
                   {policy.include_database && <span className="chip text-xs">DB</span>}
                   {policy.include_server_config && <span className="chip text-xs">Config</span>}
                   {policy.include_server_worlds && <span className="chip text-xs">Worlds</span>}
                </div>
              </article>
            ))
          )}
          
          <div className="panel-soft px-5 py-4 text-center muted text-sm">
             To create a new policy, please use the configuration file or CLI for now.
          </div>
        </div>
      )}
    </div>
  );
}
