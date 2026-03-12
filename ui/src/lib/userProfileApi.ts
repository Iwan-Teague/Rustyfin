'use client';

import { apiFetch, apiJson, extractErrorMessage, parseResponseBody } from './api';

export type ActivityRange = '7d' | '30d' | 'all';

export type MyProfile = {
  id: string;
  username: string;
  login_username: string;
  role: 'admin' | 'user';
  created_ts: number;
  time_zone?: string | null;
  avatar_url?: string | null;
};

export type UserPreferences = {
  version: number;
  audio: {
    input_device_id?: string | null;
    output_device_id?: string | null;
  };
  activity: {
    default_range: ActivityRange;
  };
  privacy: {
    personal_activity_enabled: boolean;
  };
  notifications: {
    desktop_enabled: boolean;
  };
  accessibility: {
    reduce_motion: boolean;
  };
  appearance: {
    density?: string | null;
  };
};

export type ActivityBucket = {
  key: string;
  label: string;
  total_ms: number;
  session_count: number;
};

export type ActivityRecentEntry = {
  activity_kind: string;
  label: string;
  started_ts: number;
  ended_ts?: number | null;
  total_ms: number;
};

export type ActivitySummaryResponse = {
  range: ActivityRange;
  generated_ts: number;
  activity_enabled: boolean;
  totals: {
    total_time_ms: number;
    rooms_time_ms: number;
    voice_time_ms: number;
    media_watch_time_ms: number;
  };
  most_used_sections: ActivityBucket[];
  top_rooms: ActivityBucket[];
  top_voice_channels: ActivityBucket[];
  top_watched_media: ActivityBucket[];
  recent_activity: ActivityRecentEntry[];
  session_counts: {
    room_sessions: number;
    voice_sessions: number;
    media_sessions: number;
  };
};

export type ChangePasswordResponse = {
  ok: boolean;
  relogin_required: boolean;
};

export type BrowserActivityEvent = {
  client_session_id: string;
  tab_id: string;
  section: string;
  event: 'start' | 'heartbeat' | 'stop';
};

export function defaultUserPreferences(): UserPreferences {
  return {
    version: 1,
    audio: {
      input_device_id: null,
      output_device_id: null,
    },
    activity: {
      default_range: '7d',
    },
    privacy: {
      personal_activity_enabled: true,
    },
    notifications: {
      desktop_enabled: false,
    },
    accessibility: {
      reduce_motion: false,
    },
    appearance: {},
  };
}

function normalizeRange(raw: unknown): ActivityRange {
  return raw === '30d' || raw === 'all' ? raw : '7d';
}

function normalizePreferences(raw: Partial<UserPreferences> | null | undefined): UserPreferences {
  const defaults = defaultUserPreferences();
  return {
    version: typeof raw?.version === 'number' ? raw.version : defaults.version,
    audio: {
      input_device_id:
        typeof raw?.audio?.input_device_id === 'string' || raw?.audio?.input_device_id === null
          ? raw.audio.input_device_id
          : defaults.audio.input_device_id,
      output_device_id:
        typeof raw?.audio?.output_device_id === 'string' || raw?.audio?.output_device_id === null
          ? raw.audio.output_device_id
          : defaults.audio.output_device_id,
    },
    activity: {
      default_range: normalizeRange(raw?.activity?.default_range),
    },
    privacy: {
      personal_activity_enabled:
        typeof raw?.privacy?.personal_activity_enabled === 'boolean'
          ? raw.privacy.personal_activity_enabled
          : defaults.privacy.personal_activity_enabled,
    },
    notifications: {
      desktop_enabled:
        typeof raw?.notifications?.desktop_enabled === 'boolean'
          ? raw.notifications.desktop_enabled
          : defaults.notifications.desktop_enabled,
    },
    accessibility: {
      reduce_motion:
        typeof raw?.accessibility?.reduce_motion === 'boolean'
          ? raw.accessibility.reduce_motion
          : defaults.accessibility.reduce_motion,
    },
    appearance: {
      density:
        typeof raw?.appearance?.density === 'string' || raw?.appearance?.density === null
          ? raw.appearance.density
          : defaults.appearance.density,
    },
  };
}

export async function getMyProfile(): Promise<MyProfile> {
  return apiJson<MyProfile>('/users/me/profile');
}

export async function updateMyProfile(payload: {
  display_name: string;
  time_zone?: string | null;
}): Promise<MyProfile> {
  return apiJson<MyProfile>('/users/me/profile', {
    method: 'PATCH',
    body: JSON.stringify(payload),
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

export async function getMyPreferences(): Promise<UserPreferences> {
  const prefs = await apiJson<Partial<UserPreferences>>('/users/me/preferences');
  return normalizePreferences(prefs);
}

export async function updateMyPreferences(
  prefs: UserPreferences,
): Promise<UserPreferences> {
  const next = normalizePreferences(prefs);
  const updated = await apiJson<Partial<UserPreferences>>('/users/me/preferences', {
    method: 'PATCH',
    body: JSON.stringify(next),
  });
  return normalizePreferences(updated);
}

export async function changeMyPassword(payload: {
  current_password: string;
  new_password: string;
  confirm_password: string;
}): Promise<ChangePasswordResponse> {
  return apiJson<ChangePasswordResponse>('/users/me/password', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function getMyActivitySummary(
  range: ActivityRange,
): Promise<ActivitySummaryResponse> {
  return apiJson<ActivitySummaryResponse>(
    `/users/me/activity?range=${encodeURIComponent(range)}`,
  );
}

export async function clearMyActivityHistory(): Promise<void> {
  await apiJson('/users/me/activity', {
    method: 'DELETE',
  });
}

export async function postBrowserActivity(
  event: BrowserActivityEvent,
  keepalive = false,
): Promise<void> {
  const res = await apiFetch('/users/me/activity/browser', {
    method: 'POST',
    keepalive,
    body: JSON.stringify(event),
  });
  const body = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(body, 'Failed to record activity'));
  }
}
