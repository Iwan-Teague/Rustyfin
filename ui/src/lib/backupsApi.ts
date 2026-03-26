'use client';

import { apiJson } from './api';

export interface BackupPolicy {
  id: string;
  name: string;
  schedule_cron?: string | null;
  retention_count: number;
  target_type: string;
  target_path?: string | null;
  include_database: boolean;
  include_server_config: boolean;
  include_server_worlds: boolean;
  enabled: boolean;
  last_run_ts?: number | null;
  created_ts: number;
  updated_ts: number;
}

export interface BackupJob {
  id: string;
  policy_id?: string | null;
  status: string;
  trigger_type: string;
  start_ts: number;
  end_ts?: number | null;
  log_text?: string | null;
  error_message?: string | null;
  total_size_bytes?: number | null;
}

export async function listPolicies(): Promise<BackupPolicy[]> {
  return apiJson<BackupPolicy[]>('/system/backups/policies');
}

export async function createPolicy(policy: Partial<BackupPolicy>): Promise<BackupPolicy> {
  return apiJson<BackupPolicy>('/system/backups/policies', {
    method: 'POST',
    body: JSON.stringify(policy),
  });
}

export async function listJobs(): Promise<BackupJob[]> {
  return apiJson<BackupJob[]>('/system/backups/jobs');
}

export async function createBackupJob(): Promise<string> {
  return apiJson<string>('/system/backups/jobs', {
    method: 'POST',
    body: JSON.stringify({}),
  });
}

export async function restoreBackup(jobId: string): Promise<void> {
  await apiJson(`/system/backups/jobs/${jobId}/restore`, {
    method: 'POST',
    body: JSON.stringify({}),
  });
}
