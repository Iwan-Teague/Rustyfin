'use client';

import { apiFetch, extractErrorMessage, parseResponseBody } from './api';

export interface CreateAccountBackupArchiveRequest {
  vault_export_json?: unknown;
  vault_preferences_json?: unknown;
}

export async function downloadAccountBackupArchive(
  payload: CreateAccountBackupArchiveRequest,
): Promise<Blob> {
  const response = await apiFetch('/backups/account/archive', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const body = await parseResponseBody(response);
    throw new Error(extractErrorMessage(body, 'Failed to create account backup archive.'));
  }
  return response.blob();
}
