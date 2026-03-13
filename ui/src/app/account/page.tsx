'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { useAuth } from '@/lib/auth';
import { useChannels } from '@/lib/channelsContext';
import { useMyAccount } from './hooks/useMyAccount';

type AudioDeviceOption = {
  id: string;
  label: string;
};

const SYNTHETIC_INPUT_PREFIX = 'synthetic-audioinput-';
const SYNTHETIC_OUTPUT_PREFIX = 'synthetic-audiooutput-';
const FALLBACK_TIME_ZONES = [
  'Europe/Dublin',
  'Europe/London',
  'UTC',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
];

function formatDuration(totalMs: number): string {
  const totalMinutes = Math.max(0, Math.round(totalMs / 60_000));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

function formatJoinedDate(createdTs?: number | null): string {
  if (!createdTs) return 'Unknown';
  return new Date(createdTs * 1000).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });
}

function isSyntheticDeviceId(value: string | null | undefined): boolean {
  if (!value) return false;
  return value.startsWith(SYNTHETIC_INPUT_PREFIX) || value.startsWith(SYNTHETIC_OUTPUT_PREFIX);
}

function getSupportedTimeZones(): string[] {
  const supportedValuesOf = (
    Intl as typeof Intl & {
      supportedValuesOf?: (key: string) => string[];
    }
  ).supportedValuesOf;
  if (typeof supportedValuesOf === 'function') {
    const zones = supportedValuesOf('timeZone');
    if (Array.isArray(zones) && zones.length > 0) {
      return zones;
    }
  }
  return FALLBACK_TIME_ZONES;
}

function formatDaysSince(createdTs?: number | null): string {
  if (!createdTs) return 'Unknown';
  const createdAtMs = createdTs * 1000;
  const diffMs = Math.max(0, Date.now() - createdAtMs);
  const days = Math.floor(diffMs / 86_400_000);
  if (days === 0) return 'Joined today';
  if (days === 1) return '1 day ago';
  return `${days} days ago`;
}

export default function AccountPage() {
  const router = useRouter();
  const { me, loading: authLoading, logout } = useAuth();
  const {
    preferredInputDeviceId,
    preferredOutputDeviceId,
    setPreferredAudioDevices,
  } = useChannels();
  const account = useMyAccount({
    enabled: !authLoading && Boolean(me),
    onApplyAudioPreferences: setPreferredAudioDevices,
    onRequireRelogin: logout,
  });
  const [displayName, setDisplayName] = useState('');
  const [timeZone, setTimeZone] = useState('');
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [removeAvatar, setRemoveAvatar] = useState(false);
  const [selectedInputDeviceId, setSelectedInputDeviceId] = useState<string | null>(null);
  const [selectedOutputDeviceId, setSelectedOutputDeviceId] = useState<string | null>(null);
  const [inputDevices, setInputDevices] = useState<AudioDeviceOption[]>([]);
  const [outputDevices, setOutputDevices] = useState<AudioDeviceOption[]>([]);
  const [supportsOutputDeviceSelection, setSupportsOutputDeviceSelection] = useState(true);
  const [profileSaving, setProfileSaving] = useState(false);
  const [preferencesSaving, setPreferencesSaving] = useState(false);
  const [privacySaving, setPrivacySaving] = useState(false);
  const [passwordSaving, setPasswordSaving] = useState(false);
  const [clearingActivity, setClearingActivity] = useState(false);
  const [passwordForm, setPasswordForm] = useState({
    current_password: '',
    new_password: '',
    confirm_password: '',
  });
  const [formError, setFormError] = useState<string | null>(null);
  const [formSuccess, setFormSuccess] = useState<string | null>(null);
  const timeZones = useMemo(() => getSupportedTimeZones(), []);
  const joinedDateLabel = useMemo(
    () => formatJoinedDate(account.profile?.created_ts),
    [account.profile?.created_ts],
  );
  const joinedDaysLabel = useMemo(
    () => formatDaysSince(account.profile?.created_ts),
    [account.profile?.created_ts],
  );
  const activitySlices = useMemo(() => {
    const totals = account.activitySummary.totals;
    return [
      {
        key: 'rooms',
        label: 'Rooms',
        value: Math.max(0, totals.rooms_time_ms),
        color: 'var(--accent-orange)',
      },
      {
        key: 'voice',
        label: 'Voice',
        value: Math.max(0, totals.voice_time_ms),
        color: 'var(--accent-pink)',
      },
      {
        key: 'media',
        label: 'Media',
        value: Math.max(0, totals.media_watch_time_ms),
        color: 'var(--accent-purple)',
      },
    ];
  }, [account.activitySummary.totals]);
  const activityPie = useMemo(() => {
    const total = activitySlices.reduce((sum, slice) => sum + slice.value, 0);
    if (total <= 0) {
      return {
        total,
        background:
          'conic-gradient(rgba(255,255,255,0.08) 0deg 360deg)',
        percentages: activitySlices.map((slice) => ({
          ...slice,
          percent: 0,
        })),
      };
    }
    let currentDeg = 0;
    const stops: string[] = [];
    const percentages = activitySlices.map((slice, index) => {
      const percent = (slice.value / total) * 100;
      const rawDeg = (slice.value / total) * 360;
      const endDeg = index === activitySlices.length - 1 ? 360 : currentDeg + rawDeg;
      stops.push(`${slice.color} ${currentDeg}deg ${endDeg}deg`);
      currentDeg = endDeg;
      return {
        ...slice,
        percent,
      };
    });
    return {
      total,
      background: `conic-gradient(${stops.join(', ')})`,
      percentages,
    };
  }, [activitySlices]);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!account.profile) return;
    setDisplayName(account.profile.username);
    setTimeZone(account.profile.time_zone ?? '');
    setAvatarFile(null);
    setRemoveAvatar(false);
  }, [account.profile]);

  useEffect(() => {
    setSelectedInputDeviceId(account.preferences.audio.input_device_id ?? preferredInputDeviceId ?? null);
    setSelectedOutputDeviceId(
      account.preferences.audio.output_device_id ?? preferredOutputDeviceId ?? null,
    );
  }, [
    account.preferences.audio.input_device_id,
    account.preferences.audio.output_device_id,
    preferredInputDeviceId,
    preferredOutputDeviceId,
  ]);

  useEffect(() => {
    const sinkCapable =
      typeof window !== 'undefined' &&
      typeof (
        HTMLMediaElement.prototype as HTMLMediaElement & {
          setSinkId?: (deviceId: string) => Promise<void>;
        }
      ).setSinkId === 'function';
    setSupportsOutputDeviceSelection(sinkCapable);
  }, []);

  useEffect(() => {
    async function loadDevices() {
      if (!navigator.mediaDevices?.enumerateDevices) {
        setInputDevices([]);
        setOutputDevices([]);
        return;
      }
      try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        let inputIndex = 0;
        let outputIndex = 0;
        const nextInputs: AudioDeviceOption[] = [];
        const nextOutputs: AudioDeviceOption[] = [];
        for (const device of devices) {
          if (device.kind === 'audioinput') {
            inputIndex += 1;
            nextInputs.push({
              id: device.deviceId?.trim() || `${SYNTHETIC_INPUT_PREFIX}${inputIndex}`,
              label: device.label || `Microphone ${inputIndex}`,
            });
          } else if (device.kind === 'audiooutput') {
            outputIndex += 1;
            nextOutputs.push({
              id: device.deviceId?.trim() || `${SYNTHETIC_OUTPUT_PREFIX}${outputIndex}`,
              label: device.label || `Speaker ${outputIndex}`,
            });
          }
        }
        setInputDevices(nextInputs);
        setOutputDevices(nextOutputs);
      } catch {
        setInputDevices([]);
        setOutputDevices([]);
      }
    }

    void loadDevices();
  }, []);

  const avatarPreviewUrl = useMemo(() => {
    if (avatarFile) {
      return URL.createObjectURL(avatarFile);
    }
    if (removeAvatar) {
      return null;
    }
    return account.profile?.avatar_url ?? me?.avatar_url ?? null;
  }, [account.profile?.avatar_url, avatarFile, me?.avatar_url, removeAvatar]);

  useEffect(() => {
    return () => {
      if (avatarFile && avatarPreviewUrl) {
        URL.revokeObjectURL(avatarPreviewUrl);
      }
    };
  }, [avatarFile, avatarPreviewUrl]);

  if (authLoading || !me) {
    return (
      <div className="flex items-center justify-center h-full py-20 animate-rise">
        <span className="muted">Loading account...</span>
      </div>
    );
  }

  const accountError = formError ?? account.error;

  async function handleSaveProfile() {
    setProfileSaving(true);
    setFormError(null);
    setFormSuccess(null);
    try {
      await account.saveProfile({
        displayName,
        timeZone,
        avatarFile,
        removeAvatar,
      });
      setAvatarFile(null);
      setRemoveAvatar(false);
      setFormSuccess('Profile saved.');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to save profile');
    } finally {
      setProfileSaving(false);
    }
  }

  async function handleSavePreferences() {
    setPreferencesSaving(true);
    setFormError(null);
    setFormSuccess(null);
    try {
      await account.savePreferences({
        ...account.preferences,
        audio: {
          input_device_id: selectedInputDeviceId,
          output_device_id: supportsOutputDeviceSelection ? selectedOutputDeviceId : null,
        },
      });
      setFormSuccess('Preferences saved.');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to save preferences');
    } finally {
      setPreferencesSaving(false);
    }
  }

  async function handleSavePrivacy(nextValue: boolean) {
    setPrivacySaving(true);
    setFormError(null);
    setFormSuccess(null);
    try {
      await account.savePrivacy(nextValue);
      setFormSuccess('Privacy settings saved.');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to update privacy settings');
    } finally {
      setPrivacySaving(false);
    }
  }

  async function handleChangePassword() {
    setPasswordSaving(true);
    setFormError(null);
    setFormSuccess(null);
    try {
      await account.submitPasswordChange(passwordForm);
      setFormSuccess('Password updated. Sign in again to continue.');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to change password');
    } finally {
      setPasswordSaving(false);
    }
  }

  async function handleClearActivity() {
    setClearingActivity(true);
    setFormError(null);
    setFormSuccess(null);
    try {
      await account.clearActivity();
      setFormSuccess('Activity history cleared.');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to clear activity history');
    } finally {
      setClearingActivity(false);
    }
  }

  return (
    <div className="space-y-6 animate-rise">
      <section className="panel rounded-3xl border border-[var(--border)] p-5 md:p-6">
        <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-4 min-w-0">
            {avatarPreviewUrl ? (
              <img
                src={avatarPreviewUrl}
                alt={displayName || me.username}
                className="h-20 w-20 rounded-full object-cover border border-[var(--border)] bg-black/20"
              />
            ) : (
              <div className="h-20 w-20 rounded-full bg-gradient-to-br from-[var(--orange)] to-[var(--purple-strong)] text-white text-2xl font-semibold flex items-center justify-center">
                {(displayName || me.username).slice(0, 2).toUpperCase()}
              </div>
            )}
            <div className="min-w-0">
              <h1 className="text-2xl font-semibold truncate">{account.profile?.username ?? me.username}</h1>
              <p className="text-sm muted truncate">Login: {account.profile?.login_username ?? me.login_username ?? me.username}</p>
              <p className="text-sm muted">Role: {account.profile?.role ?? me.role}</p>
            </div>
          </div>
          <div className="rounded-2xl border border-[var(--border)] bg-black/15 px-4 py-3 text-sm text-white/80 space-y-1 min-w-[220px]">
            <p className="text-xs muted">Date joined</p>
            <p className="font-medium text-white/90">{joinedDateLabel}</p>
            <p className="text-xs muted">{joinedDaysLabel}</p>
          </div>
        </div>
      </section>

      {(account.loading || account.activityLoading) && (
        <p className="text-sm muted">{account.loading ? 'Loading account...' : 'Refreshing activity...'}</p>
      )}
      {accountError && <p className="text-sm text-red-300">{accountError}</p>}
      {formSuccess && <p className="text-sm text-emerald-300">{formSuccess}</p>}

      <div className="grid grid-cols-1 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,0.8fr)] gap-6">
        <div className="space-y-6">
          <section className="panel-soft rounded-2xl border border-[var(--border)] p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Profile</h2>
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                disabled={profileSaving}
                onClick={() => {
                  void handleSaveProfile();
                }}
              >
                {profileSaving ? 'Saving...' : 'Save profile'}
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <label className="text-xs muted">Display name</label>
                <input
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                  maxLength={40}
                />
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">Time zone</label>
                <input
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={timeZone}
                  onChange={(event) => setTimeZone(event.target.value)}
                  list="rustyfin-time-zones"
                  placeholder="Europe/Dublin"
                />
                <datalist id="rustyfin-time-zones">
                  {timeZones.map((zone) => (
                    <option key={zone} value={zone} />
                  ))}
                </datalist>
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">Login username</label>
                <input
                  className="panel w-full rounded-lg px-3 py-2 text-sm opacity-80"
                  value={account.profile?.login_username ?? me.login_username ?? me.username}
                  readOnly
                />
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">Role</label>
                <input
                  className="panel w-full rounded-lg px-3 py-2 text-sm opacity-80"
                  value={account.profile?.role ?? me.role}
                  readOnly
                />
              </div>
            </div>

            <div className="space-y-3">
              <label className="text-xs muted">Avatar</label>
              <input
                type="file"
                accept="image/png,image/jpeg,image/webp,image/gif"
                className="w-full rounded-lg border border-[var(--border)] bg-black/20 px-3 py-2 text-xs text-white/85 file:mr-3 file:rounded-md file:border file:border-[var(--border)] file:bg-black/35 file:px-3 file:py-1 file:text-xs file:font-medium file:text-white hover:file:bg-black/45"
                onChange={(event) => {
                  const file = event.target.files?.[0] ?? null;
                  setAvatarFile(file);
                  if (file) {
                    setRemoveAvatar(false);
                  }
                }}
              />
              <button
                type="button"
                className="btn-ghost px-3 py-2 text-xs"
                onClick={() => {
                  setAvatarFile(null);
                  setRemoveAvatar(true);
                }}
              >
                Remove avatar
              </button>
            </div>
          </section>

          <section className="panel-soft rounded-2xl border border-[var(--border)] p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Preferences</h2>
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                disabled={preferencesSaving}
                onClick={() => {
                  void handleSavePreferences();
                }}
              >
                {preferencesSaving ? 'Saving...' : 'Save preferences'}
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <label className="text-xs muted">Input device</label>
                <select
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={selectedInputDeviceId ?? ''}
                  onChange={(event) => setSelectedInputDeviceId(event.target.value || null)}
                >
                  <option value="">Default input</option>
                  {inputDevices.map((device) => (
                    <option key={device.id} value={device.id}>
                      {device.label}
                    </option>
                  ))}
                </select>
                {isSyntheticDeviceId(selectedInputDeviceId) && (
                  <p className="text-xs muted">Browser privacy mode is hiding microphone IDs.</p>
                )}
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">Output device</label>
                <select
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={selectedOutputDeviceId ?? ''}
                  onChange={(event) => setSelectedOutputDeviceId(event.target.value || null)}
                  disabled={!supportsOutputDeviceSelection}
                >
                  <option value="">Default output</option>
                  {outputDevices.map((device) => (
                    <option key={device.id} value={device.id}>
                      {device.label}
                    </option>
                  ))}
                </select>
                {!supportsOutputDeviceSelection && (
                  <p className="text-xs muted">This browser does not support in-app output selection.</p>
                )}
              </div>
            </div>
          </section>

          <section className="panel-soft rounded-2xl border border-[var(--border)] p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Security</h2>
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                disabled={passwordSaving}
                onClick={() => {
                  void handleChangePassword();
                }}
              >
                {passwordSaving ? 'Saving...' : 'Change password'}
              </button>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="space-y-2">
                <label className="text-xs muted">Current password</label>
                <input
                  type="password"
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={passwordForm.current_password}
                  onChange={(event) =>
                    setPasswordForm((current) => ({
                      ...current,
                      current_password: event.target.value,
                    }))
                  }
                />
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">New password</label>
                <input
                  type="password"
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={passwordForm.new_password}
                  onChange={(event) =>
                    setPasswordForm((current) => ({
                      ...current,
                      new_password: event.target.value,
                    }))
                  }
                />
              </div>
              <div className="space-y-2">
                <label className="text-xs muted">Confirm password</label>
                <input
                  type="password"
                  className="panel w-full rounded-lg px-3 py-2 text-sm"
                  value={passwordForm.confirm_password}
                  onChange={(event) =>
                    setPasswordForm((current) => ({
                      ...current,
                      confirm_password: event.target.value,
                    }))
                  }
                />
              </div>
            </div>
          </section>
        </div>

        <div className="space-y-6">
          <section className="panel-soft rounded-2xl border border-[var(--border)] p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Data and Privacy</h2>
              <button
                type="button"
                className="btn-ghost px-4 py-2 text-sm"
                disabled={clearingActivity}
                onClick={() => {
                  void handleClearActivity();
                }}
              >
                {clearingActivity ? 'Clearing...' : 'Clear activity history'}
              </button>
            </div>
            <p className="text-sm muted">
              Rustyfin stores personal activity summaries for section presence, watch rooms, voice channels, and media watch time so your account page can show simple usage insights.
            </p>
            <label className="flex items-start gap-3 rounded-xl border border-[var(--border)] bg-black/15 px-4 py-3 text-sm">
              <input
                type="checkbox"
                className="mt-1"
                checked={account.preferences.privacy.personal_activity_enabled}
                disabled={privacySaving}
                onChange={(event) => {
                  void handleSavePrivacy(event.target.checked);
                }}
              />
              <span>
                Store personal activity insights for this account.
              </span>
            </label>
          </section>

          <section className="panel-soft rounded-2xl border border-[var(--border)] p-5 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Activity</h2>
              <select
                className="panel rounded-lg px-3 py-2 text-sm"
                value={account.activityRange}
                onChange={(event) => account.setActivityRange(event.target.value as '7d' | '30d' | 'all')}
              >
                <option value="7d">Last 7 days</option>
                <option value="30d">Last 30 days</option>
                <option value="all">All time</option>
              </select>
            </div>

            {!account.activitySummary.activity_enabled && (
              <p className="text-sm muted">Activity persistence is currently disabled for this account.</p>
            )}

            <div className="grid grid-cols-2 gap-3">
              <div className="rounded-2xl border border-[var(--border)] bg-black/15 p-4">
                <p className="text-xs muted">Time on Rustyfin</p>
                <p className="mt-2 text-xl font-semibold">{formatDuration(account.activitySummary.totals.total_time_ms)}</p>
              </div>
              <div className="rounded-2xl border border-[var(--border)] bg-black/15 p-4">
                <p className="text-xs muted">Rooms time</p>
                <p className="mt-2 text-xl font-semibold">{formatDuration(account.activitySummary.totals.rooms_time_ms)}</p>
              </div>
              <div className="rounded-2xl border border-[var(--border)] bg-black/15 p-4">
                <p className="text-xs muted">Voice time</p>
                <p className="mt-2 text-xl font-semibold">{formatDuration(account.activitySummary.totals.voice_time_ms)}</p>
              </div>
              <div className="rounded-2xl border border-[var(--border)] bg-black/15 p-4">
                <p className="text-xs muted">Media watch time</p>
                <p className="mt-2 text-xl font-semibold">{formatDuration(account.activitySummary.totals.media_watch_time_ms)}</p>
              </div>
            </div>

            <section className="rounded-2xl border border-[var(--border)] bg-black/15 p-4 space-y-4">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">Activity mix</h3>
                <span className="text-xs muted">
                  {activityPie.total > 0 ? formatDuration(activityPie.total) : 'No recorded activity yet'}
                </span>
              </div>
              <div className="grid grid-cols-1 md:grid-cols-[220px_minmax(0,1fr)] gap-6 items-center">
                <div className="flex justify-center">
                  <div
                    className="h-48 w-48 rounded-full border border-[var(--border)] shadow-[0_18px_42px_rgba(0,0,0,0.28)]"
                    style={{ background: activityPie.background }}
                    aria-label="Activity pie chart"
                  />
                </div>
                <div className="grid grid-cols-1 gap-3">
                  {activityPie.percentages.map((slice) => (
                    <div
                      key={slice.key}
                      className="rounded-xl border border-[var(--border)] bg-black/10 px-3 py-3"
                    >
                      <div className="flex items-center gap-2">
                        <span
                          className="h-3 w-3 rounded-full shrink-0"
                          style={{ backgroundColor: slice.color }}
                        />
                        <p className="text-sm font-medium">{slice.label}</p>
                      </div>
                      <div className="mt-2 flex items-baseline justify-between gap-3">
                        <span className="text-sm text-white/85">{formatDuration(slice.value)}</span>
                        <span className="text-xs muted">{slice.percent.toFixed(0)}%</span>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </section>
          </section>
        </div>
      </div>
    </div>
  );
}
