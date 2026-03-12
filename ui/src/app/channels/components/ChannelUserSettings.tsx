'use client';

import Link from 'next/link';
import { useEffect, useMemo, useState } from 'react';
import type { Me } from '@/lib/auth';
import { useMyAccount } from '@/app/account/hooks/useMyAccount';

type AudioDeviceOption = {
  id: string;
  label: string;
};

const SYNTHETIC_INPUT_PREFIX = 'synthetic-audioinput-';
const SYNTHETIC_OUTPUT_PREFIX = 'synthetic-audiooutput-';

function isSyntheticDeviceId(value: string | null | undefined): boolean {
  if (!value) return false;
  return value.startsWith(SYNTHETIC_INPUT_PREFIX) || value.startsWith(SYNTHETIC_OUTPUT_PREFIX);
}

interface Props {
  me: Me;
  preferredInputDeviceId: string | null;
  preferredOutputDeviceId: string | null;
  setPreferredAudioDevices: (inputDeviceId: string | null, outputDeviceId: string | null) => void;
}

export default function ChannelUserSettings({
  me,
  preferredInputDeviceId,
  preferredOutputDeviceId,
  setPreferredAudioDevices,
}: Props) {
  const [open, setOpen] = useState(false);
  const account = useMyAccount({
    enabled: open,
    onApplyAudioPreferences: setPreferredAudioDevices,
  });
  const [displayName, setDisplayName] = useState(me.username);
  const [timeZone, setTimeZone] = useState(me.time_zone ?? '');
  const [selectedInputDeviceId, setSelectedInputDeviceId] = useState<string | null>(
    preferredInputDeviceId,
  );
  const [selectedOutputDeviceId, setSelectedOutputDeviceId] = useState<string | null>(
    preferredOutputDeviceId,
  );
  const [inputDevices, setInputDevices] = useState<AudioDeviceOption[]>([]);
  const [outputDevices, setOutputDevices] = useState<AudioDeviceOption[]>([]);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [removeAvatar, setRemoveAvatar] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [supportsOutputDeviceSelection, setSupportsOutputDeviceSelection] = useState(true);

  const avatarPreviewUrl = useMemo(() => {
    if (avatarFile) {
      return URL.createObjectURL(avatarFile);
    }
    if (removeAvatar) {
      return null;
    }
    return account.profile?.avatar_url ?? me.avatar_url ?? null;
  }, [account.profile?.avatar_url, avatarFile, me.avatar_url, removeAvatar]);

  useEffect(() => {
    return () => {
      if (avatarFile && avatarPreviewUrl) {
        URL.revokeObjectURL(avatarPreviewUrl);
      }
    };
  }, [avatarFile, avatarPreviewUrl]);

  useEffect(() => {
    if (!account.profile) return;
    setDisplayName(account.profile.username);
    setTimeZone(account.profile.time_zone ?? '');
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

  async function openSettings() {
    setOpen(true);
    setError(null);
    setSuccess(null);
    setAvatarFile(null);
    setRemoveAvatar(false);
    const sinkCapable =
      typeof window !== 'undefined' &&
      typeof (
        HTMLMediaElement.prototype as HTMLMediaElement & {
          setSinkId?: (deviceId: string) => Promise<void>;
        }
      ).setSinkId === 'function';
    setSupportsOutputDeviceSelection(sinkCapable);
    await loadDevices();
  }

  async function saveSettings() {
    setSaving(true);
    setError(null);
    setSuccess(null);
    try {
      await account.saveProfile({
        displayName,
        timeZone,
        avatarFile,
        removeAvatar,
      });

      await account.savePreferences({
        ...account.preferences,
        audio: {
          input_device_id: selectedInputDeviceId,
          output_device_id: supportsOutputDeviceSelection ? selectedOutputDeviceId : null,
        },
      });

      setAvatarFile(null);
      setRemoveAvatar(false);
      setSuccess('Settings saved.');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <div className="h-16 border-t border-[var(--border)] px-4 flex items-center justify-between shrink-0 bg-[var(--surface)]">
        <div className="flex items-center gap-3 min-w-0">
          {me.avatar_url ? (
            <img
              src={me.avatar_url}
              alt={me.username}
              className="h-10 w-10 rounded-full object-cover border border-[var(--border)] bg-black/20"
            />
          ) : (
            <div className="h-10 w-10 rounded-full bg-gradient-to-br from-[var(--orange)] to-[var(--purple-strong)] text-white font-semibold flex items-center justify-center">
              {me.username.slice(0, 2).toUpperCase()}
            </div>
          )}
          <div className="min-w-0">
            <p className="text-sm font-medium truncate">{me.username}</p>
            <p className="text-[11px] muted truncate">{me.role}</p>
          </div>
        </div>
        <button
          type="button"
          className="btn-ghost px-3 py-2"
          aria-label="Open user settings"
          onClick={() => {
            void openSettings();
          }}
        >
          ⚙
        </button>
      </div>

      {open && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 p-4">
          <div className="panel w-full max-w-2xl rounded-2xl border border-[var(--border)] p-5 md:p-6 space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">User Settings</h2>
              <div className="flex items-center gap-2">
                <Link
                  href="/account"
                  className="btn-ghost px-3 py-2 text-sm"
                  onClick={() => setOpen(false)}
                >
                  Open account page
                </Link>
                <button type="button" className="btn-ghost px-2 py-1 text-sm" onClick={() => setOpen(false)}>
                  Close
                </button>
              </div>
            </div>

            {(account.loading || !account.profile) ? (
              <p className="muted text-sm">Loading settings...</p>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <section className="panel-soft rounded-xl border border-[var(--border)] p-4 space-y-3">
                  <h3 className="text-sm font-semibold">Profile</h3>
                  <div className="flex items-center gap-3">
                    {avatarPreviewUrl ? (
                      <img
                        src={avatarPreviewUrl}
                        alt={displayName}
                        className="h-16 w-16 rounded-full object-cover border border-[var(--border)] bg-black/20"
                      />
                    ) : (
                      <div className="h-16 w-16 rounded-full bg-gradient-to-br from-[var(--orange)] to-[var(--purple-strong)] text-white text-xl font-semibold flex items-center justify-center">
                        {displayName.slice(0, 2).toUpperCase()}
                      </div>
                    )}
                    <div className="flex-1 space-y-2">
                      <input
                        type="file"
                        accept="image/png,image/jpeg,image/webp,image/gif"
                        className="w-full rounded-lg border border-[var(--border)] bg-black/20 px-3 py-2 text-xs text-white/85 file:mr-3 file:rounded-md file:border file:border-[var(--border)] file:bg-black/35 file:px-3 file:py-1 file:text-xs file:font-medium file:text-white hover:file:bg-black/45"
                        onChange={(event) => {
                          const file = event.target.files?.[0] ?? null;
                          setAvatarFile(file);
                          if (file) setRemoveAvatar(false);
                        }}
                      />
                      <button
                        type="button"
                        className="btn-ghost px-2 py-1 text-xs"
                        onClick={() => {
                          setAvatarFile(null);
                          setRemoveAvatar(true);
                        }}
                      >
                        Remove avatar
                      </button>
                    </div>
                  </div>
                  <label className="text-xs muted">Display Name</label>
                  <input
                    className="panel w-full rounded-lg px-3 py-2 text-sm"
                    value={displayName}
                    onChange={(event) => setDisplayName(event.target.value)}
                    maxLength={40}
                  />
                  <label className="text-xs muted">Time Zone</label>
                  <input
                    className="panel w-full rounded-lg px-3 py-2 text-sm"
                    value={timeZone}
                    onChange={(event) => setTimeZone(event.target.value)}
                    placeholder="Europe/Dublin"
                  />
                </section>

                <section className="panel-soft rounded-xl border border-[var(--border)] p-4 space-y-3">
                  <h3 className="text-sm font-semibold">Audio Devices</h3>
                  <label className="text-xs muted">Input Device</label>
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
                    <p className="text-xs muted">
                      Browser privacy mode is hiding microphone IDs.
                    </p>
                  )}

                  <label className="text-xs muted">Output Device</label>
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
                    <p className="text-xs muted">
                      This browser does not support speaker selection in-app.
                    </p>
                  )}

                  <button
                    type="button"
                    className="btn-ghost px-3 py-2 text-xs"
                    onClick={() => {
                      void loadDevices();
                    }}
                  >
                    Refresh device list
                  </button>
                </section>
              </div>
            )}

            {(error || account.error) && <p className="text-sm text-red-300">{error ?? account.error}</p>}
            {success && <p className="text-sm text-emerald-300">{success}</p>}

            <div className="flex justify-end gap-2">
              <button type="button" className="btn-ghost px-4 py-2 text-sm" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                disabled={account.loading || saving}
                onClick={() => {
                  void saveSettings();
                }}
              >
                {saving ? 'Saving...' : 'Save Settings'}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
