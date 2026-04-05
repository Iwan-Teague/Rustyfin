'use client';

import { nonAdminRoleLabel, type WatchPartyRoomMode } from '@/lib/watchPartyRoles';

type Props = {
  roomMode: WatchPartyRoomMode;
  password: string;
  allowPlayPause: boolean;
  allowSeek: boolean;
  inviteOnly: boolean;
  defaultJoinRole: 'viewer' | 'controller';
  embedded?: boolean;
  noShadow?: boolean;
  fillHeight?: boolean;
  onPasswordChange: (value: string) => void;
  onAllowPlayPauseChange: (value: boolean) => void;
  onAllowSeekChange: (value: boolean) => void;
  onInviteOnlyChange: (value: boolean) => void;
  onDefaultJoinRoleChange: (value: 'viewer' | 'controller') => void;
};

export default function RoomOptions({
  roomMode,
  password,
  allowPlayPause,
  allowSeek,
  inviteOnly,
  defaultJoinRole,
  embedded = false,
  noShadow = false,
  fillHeight = false,
  onPasswordChange,
  onAllowPlayPauseChange,
  onAllowSeekChange,
  onInviteOnlyChange,
  onDefaultJoinRoleChange,
}: Props) {
  const memberLabel = nonAdminRoleLabel(roomMode);
  const containerClassName = fillHeight
    ? embedded
      ? 'flex h-full min-h-0 flex-col gap-4'
      : 'rf-flat-section flex h-full min-h-0 flex-col gap-4'
    : embedded
      ? 'space-y-4'
      : 'rf-flat-section space-y-4';
  return (
    <section className={containerClassName} style={noShadow ? { boxShadow: 'none' } : undefined}>
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Room Options</h2>
        <p className="text-sm muted">Configure access controls for members.</p>
      </div>

      <div className={fillHeight ? 'min-h-0 flex-1 space-y-3 overflow-y-auto pr-1' : 'space-y-3'}>
        <label className="block text-sm">
          <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Password (optional)</span>
          <input
            type="password"
            value={password}
            onChange={(e) => onPasswordChange(e.target.value)}
            className="rf-flat-input px-3 py-2 text-sm"
            placeholder="Leave empty for no password"
            minLength={4}
            maxLength={128}
          />
        </label>

        <div className="space-y-2 rounded-xl border border-[var(--border-subtle)] px-3 py-3">
          <label className="flex items-center gap-3 rounded-md px-2 py-2 text-sm">
            <input
              type="checkbox"
              checked={allowPlayPause}
              onChange={(e) => onAllowPlayPauseChange(e.target.checked)}
            />
            Allow non-host play/pause
          </label>

          <label className="flex items-center gap-3 rounded-md px-2 py-2 text-sm">
            <input
              type="checkbox"
              checked={allowSeek}
              onChange={(e) => onAllowSeekChange(e.target.checked)}
            />
            Allow non-host seek
          </label>

          <label className="flex items-center gap-3 rounded-md px-2 py-2 text-sm">
            <input
              type="checkbox"
              checked={inviteOnly}
              onChange={(e) => onInviteOnlyChange(e.target.checked)}
            />
            Invite-only room
          </label>
        </div>

        <label className="block text-sm">
          <span className="mb-1 block text-xs uppercase tracking-wide muted">Default access for link joins</span>
          <select
            className="rf-flat-input px-3 py-2 text-sm"
            aria-label="Default access for link joins"
            value={defaultJoinRole}
            onChange={(e) => onDefaultJoinRoleChange(e.target.value as 'viewer' | 'controller')}
          >
            <option value="viewer">{memberLabel}</option>
            <option value="controller">Admin</option>
          </select>
        </label>
      </div>
    </section>
  );
}
