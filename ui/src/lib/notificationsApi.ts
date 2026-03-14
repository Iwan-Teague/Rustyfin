import { apiJson } from './api';

type LibraryScanJob = {
  id: string;
  payload?: Record<string, unknown> | null;
  created_ts: number;
  updated_ts: number;
};

type LibraryRecord = {
  id: string;
  name: string;
  kind: string;
  created_ts: number;
};

type UserRecord = {
  id: string;
  username: string;
  role: 'admin' | 'user';
  created_ts: number;
};

export type AdminNotificationKind = 'scan_complete' | 'library_created' | 'user_created';

export type AdminDiagnosticNotification = {
  id: string;
  kind: AdminNotificationKind;
  title: string;
  detail: string;
  timestamp_ts: number;
};

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
}

function asString(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0 ? value : null;
}

function scanLibraryLabel(payload: Record<string, unknown> | null): string {
  if (!payload) return 'Library scan completed.';
  return (
    asString(payload.library_name) ??
    asString(payload.library_title) ??
    asString(payload.library_id) ??
    'Library scan completed.'
  );
}

export async function listAdminDiagnosticNotifications(
  limit = 24,
): Promise<AdminDiagnosticNotification[]> {
  const safeLimit = Math.max(1, Math.min(100, Math.trunc(limit)));
  const scanParams = new URLSearchParams();
  scanParams.set('kind', 'library_scan');
  scanParams.set('status', 'complete');
  scanParams.set('limit', String(Math.max(12, safeLimit)));

  const [scanJobs, libraries, users] = await Promise.all([
    apiJson<LibraryScanJob[]>(`/jobs?${scanParams.toString()}`),
    apiJson<LibraryRecord[]>('/libraries'),
    apiJson<UserRecord[]>('/users'),
  ]);

  const scanNotifications: AdminDiagnosticNotification[] = scanJobs.map((job) => {
    const payload = asRecord(job.payload);
    return {
      id: `scan-${job.id}`,
      kind: 'scan_complete',
      title: 'Library scan completed',
      detail: scanLibraryLabel(payload),
      timestamp_ts: Math.max(job.updated_ts ?? 0, job.created_ts ?? 0),
    };
  });

  const libraryNotifications: AdminDiagnosticNotification[] = libraries.map((library) => ({
    id: `library-${library.id}`,
    kind: 'library_created',
    title: 'Library created',
    detail: `${library.name} (${library.kind})`,
    timestamp_ts: library.created_ts,
  }));

  const userNotifications: AdminDiagnosticNotification[] = users.map((user) => ({
    id: `user-${user.id}`,
    kind: 'user_created',
    title: 'User account created',
    detail: `${user.username} (${user.role})`,
    timestamp_ts: user.created_ts,
  }));

  return [...scanNotifications, ...libraryNotifications, ...userNotifications]
    .sort((a, b) => b.timestamp_ts - a.timestamp_ts)
    .slice(0, safeLimit);
}
