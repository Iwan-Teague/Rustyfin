import { apiFetch, apiJson, extractErrorMessage, parseResponseBody } from './api';

export type MyProfile = {
  id: string;
  username: string;
  login_username: string;
  role: 'admin' | 'user';
  avatar_url?: string | null;
};

export async function getMyProfile(): Promise<MyProfile> {
  return apiJson<MyProfile>('/users/me/profile');
}

export async function updateMyProfile(displayName: string): Promise<MyProfile> {
  return apiJson<MyProfile>('/users/me/profile', {
    method: 'PATCH',
    body: JSON.stringify({ display_name: displayName }),
  });
}

export async function uploadMyAvatar(file: File): Promise<MyProfile> {
  const body = new FormData();
  body.append('file', file);
  const res = await apiFetch('/users/me/avatar', {
    method: 'POST',
    body,
  });
  const payload = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(payload, 'Failed to upload avatar'));
  }
  return payload as MyProfile;
}

export async function deleteMyAvatar(): Promise<MyProfile> {
  return apiJson<MyProfile>('/users/me/avatar', {
    method: 'DELETE',
  });
}

export async function getMyPreferences(): Promise<Record<string, unknown>> {
  return apiJson<Record<string, unknown>>('/users/me/preferences');
}

export async function updateMyPreferences(
  prefs: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  return apiJson<Record<string, unknown>>('/users/me/preferences', {
    method: 'PATCH',
    body: JSON.stringify(prefs),
  });
}
