'use client';

import { nonAdminRoleLabel, type WatchPartyRoomMode } from '@/lib/watchPartyRoles';

type Props = {
  roomMode: WatchPartyRoomMode;
  password: string;
  allowPlayPause: boolean;
  allowSeek: boolean;
  inviteOnly: boolean;
  defaultJoinRole: 'viewer' | 'controller';
  noShadow?: boolean;
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
  noShadow = false,
  onPasswordChange,
  onAllowPlayPauseChange,
  onAllowSeekChange,
  onInviteOnlyChange,
  onDefaultJoinRoleChange,
}: Props) {
  const memberLabel = nonAdminRoleLabel(roomMode);
  return (
    <section className="panel space-y-4 p-5 sm:p-6" style={noShadow ? { boxShadow: 'none' } : undefined}>
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Room Options</h2>
        <p className="text-sm muted">Configure access controls for members.</p>
      </div>

      <div className="space-y-3">
        <label className="block text-sm">
          <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Password (optional)</span>
          <input
            type="password"
            value={password}
            onChange={(e) => onPasswordChange(e.target.value)}
            className="input px-3 py-2 text-sm"
            placeholder="Leave empty for no password"
            minLength={4}
            maxLength={128}
          />
        </label>

        <div className="space-y-2 rounded-xl border border-white/10 bg-black/15 p-3">
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
            className="select px-3 py-2 text-sm"
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
