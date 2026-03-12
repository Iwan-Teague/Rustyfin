'use client';

import { useCallback, useEffect, useState } from 'react';
import { useAuth, type Me } from '@/lib/auth';
import {
  changeMyPassword,
  clearMyActivityHistory,
  defaultUserPreferences,
  deleteMyAvatar,
  getMyActivitySummary,
  getMyPreferences,
  getMyProfile,
  type ActivityRange,
  type ActivitySummaryResponse,
  type MyProfile,
  type UserPreferences,
  updateMyPreferences,
  updateMyProfile,
  uploadMyAvatar,
} from '@/lib/userProfileApi';

type Options = {
  enabled?: boolean;
  onApplyAudioPreferences?: (inputDeviceId: string | null, outputDeviceId: string | null) => void;
  onRequireRelogin?: () => void;
};

function normalizeNullableString(value: string | null | undefined): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function profileToMe(profile: MyProfile): Me {
  return {
    id: profile.id,
    username: profile.username,
    login_username: profile.login_username,
    role: profile.role,
    time_zone: profile.time_zone ?? null,
    avatar_url: profile.avatar_url ?? null,
  };
}

function sameMe(left: Me | null, right: Me | null): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.id === right.id &&
    left.username === right.username &&
    left.login_username === right.login_username &&
    left.role === right.role &&
    (left.time_zone ?? null) === (right.time_zone ?? null) &&
    (left.avatar_url ?? null) === (right.avatar_url ?? null)
  );
}

function emptyActivitySummary(range: ActivityRange): ActivitySummaryResponse {
  return {
    range,
    generated_ts: Date.now(),
    activity_enabled: true,
    totals: {
      total_time_ms: 0,
      rooms_time_ms: 0,
      voice_time_ms: 0,
      media_watch_time_ms: 0,
    },
    most_used_sections: [],
    top_rooms: [],
    top_voice_channels: [],
    top_watched_media: [],
    recent_activity: [],
    session_counts: {
      room_sessions: 0,
      voice_sessions: 0,
      media_sessions: 0,
    },
  };
}

export function useMyAccount(options: Options = {}) {
  const { enabled = true, onApplyAudioPreferences, onRequireRelogin } = options;
  const { me, replaceMe } = useAuth();
  const userId = me?.id ?? null;
  const [profile, setProfile] = useState<MyProfile | null>(null);
  const [preferences, setPreferences] = useState<UserPreferences>(defaultUserPreferences());
  const [activityRange, setActivityRange] = useState<ActivityRange>('7d');
  const [activitySummary, setActivitySummary] = useState<ActivitySummaryResponse>(
    emptyActivitySummary('7d'),
  );
  const [loading, setLoading] = useState(enabled);
  const [activityLoading, setActivityLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!enabled || !userId) return;
    setLoading(true);
    setError(null);
    try {
      const [nextProfile, nextPreferences] = await Promise.all([
        getMyProfile(),
        getMyPreferences(),
      ]);
      setProfile(nextProfile);
      setPreferences(nextPreferences);
      setActivityRange(nextPreferences.activity.default_range);
      onApplyAudioPreferences?.(
        normalizeNullableString(nextPreferences.audio.input_device_id),
        normalizeNullableString(nextPreferences.audio.output_device_id),
      );
      const nextMe = profileToMe(nextProfile);
      if (!sameMe(me, nextMe)) {
        replaceMe(nextMe);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load account');
    } finally {
      setLoading(false);
    }
  }, [enabled, me, onApplyAudioPreferences, replaceMe, userId]);

  const refreshActivity = useCallback(
    async (range = activityRange) => {
      if (!enabled || !userId) return;
      setActivityLoading(true);
      try {
        const summary = await getMyActivitySummary(range);
        setActivitySummary(summary);
        setError(null);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load activity');
      } finally {
        setActivityLoading(false);
      }
    },
    [activityRange, enabled, userId],
  );

  useEffect(() => {
    if (!enabled || !userId) return;
    void load();
  }, [enabled, load, userId]);

  useEffect(() => {
    if (!enabled || !userId) return;
    void refreshActivity(activityRange);
  }, [activityRange, enabled, refreshActivity, userId]);

  useEffect(() => {
    if (!userId) {
      setProfile(null);
      setPreferences(defaultUserPreferences());
      setActivitySummary(emptyActivitySummary('7d'));
    }
  }, [userId]);

  const saveProfile = useCallback(
    async (payload: {
      displayName: string;
      timeZone: string | null;
      avatarFile: File | null;
      removeAvatar: boolean;
    }) => {
      const normalizedDisplayName = payload.displayName.trim().replace(/\s+/g, ' ');
      let nextProfile = await updateMyProfile({
        display_name: normalizedDisplayName,
        time_zone: normalizeNullableString(payload.timeZone),
      });

      if (payload.removeAvatar) {
        nextProfile = await deleteMyAvatar();
      } else if (payload.avatarFile) {
        nextProfile = await uploadMyAvatar(payload.avatarFile);
      }

      setProfile(nextProfile);
      const nextMe = profileToMe(nextProfile);
      if (!sameMe(me, nextMe)) {
        replaceMe(nextMe);
      }
      return nextProfile;
    },
    [me, replaceMe],
  );

  const savePreferences = useCallback(
    async (nextPreferences: UserPreferences) => {
      const updated = await updateMyPreferences(nextPreferences);
      setPreferences(updated);
      onApplyAudioPreferences?.(
        normalizeNullableString(updated.audio.input_device_id),
        normalizeNullableString(updated.audio.output_device_id),
      );
      if (updated.activity.default_range !== activityRange) {
        setActivityRange(updated.activity.default_range);
      }
      return updated;
    },
    [activityRange, onApplyAudioPreferences],
  );

  const savePrivacy = useCallback(
    async (personalActivityEnabled: boolean) => {
      const nextPreferences: UserPreferences = {
        ...preferences,
        privacy: {
          ...preferences.privacy,
          personal_activity_enabled: personalActivityEnabled,
        },
      };
      const updated = await savePreferences(nextPreferences);
      if (!updated.privacy.personal_activity_enabled) {
        setActivitySummary((current) => ({
          ...current,
          activity_enabled: false,
        }));
      } else {
        await refreshActivity(activityRange);
      }
      return updated;
    },
    [activityRange, preferences, refreshActivity, savePreferences],
  );

  const clearActivity = useCallback(async () => {
    await clearMyActivityHistory();
    setActivitySummary(emptyActivitySummary(activityRange));
  }, [activityRange]);

  const submitPasswordChange = useCallback(
    async (payload: {
      current_password: string;
      new_password: string;
      confirm_password: string;
    }) => {
      const result = await changeMyPassword(payload);
      if (result.relogin_required) {
        onRequireRelogin?.();
      }
      return result;
    },
    [onRequireRelogin],
  );

  return {
    profile,
    preferences,
    activityRange,
    setActivityRange,
    activitySummary,
    loading,
    activityLoading,
    error,
    setError,
    load,
    refreshActivity,
    saveProfile,
    savePreferences,
    savePrivacy,
    clearActivity,
    submitPasswordChange,
  };
}
