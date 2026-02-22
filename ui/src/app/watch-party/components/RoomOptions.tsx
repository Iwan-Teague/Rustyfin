'use client';

type Props = {
  password: string;
  allowPlayPause: boolean;
  allowSeek: boolean;
  inviteOnly: boolean;
  defaultJoinRole: 'viewer' | 'controller';
  onPasswordChange: (value: string) => void;
  onAllowPlayPauseChange: (value: boolean) => void;
  onAllowSeekChange: (value: boolean) => void;
  onInviteOnlyChange: (value: boolean) => void;
  onDefaultJoinRoleChange: (value: 'viewer' | 'controller') => void;
};

export default function RoomOptions({
  password,
  allowPlayPause,
  allowSeek,
  inviteOnly,
  defaultJoinRole,
  onPasswordChange,
  onAllowPlayPauseChange,
  onAllowSeekChange,
  onInviteOnlyChange,
  onDefaultJoinRoleChange,
}: Props) {
  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Room Options</h2>
        <p className="text-sm muted">Configure access controls for non-host participants.</p>
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

        <label className="inline-flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={allowPlayPause}
            onChange={(e) => onAllowPlayPauseChange(e.target.checked)}
          />
          Allow non-host play/pause
        </label>

        <label className="inline-flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={allowSeek}
            onChange={(e) => onAllowSeekChange(e.target.checked)}
          />
          Allow non-host seek
        </label>

        <label className="inline-flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={inviteOnly}
            onChange={(e) => onInviteOnlyChange(e.target.checked)}
          />
          Invite-only room
        </label>

        <label className="block text-sm">
          <span className="mb-1 block text-xs uppercase tracking-wide muted">Default role for link joins</span>
          <select
            className="select px-3 py-2 text-sm"
            aria-label="Default role for link joins"
            value={defaultJoinRole}
            onChange={(e) => onDefaultJoinRoleChange(e.target.value as 'viewer' | 'controller')}
          >
            <option value="viewer">Viewer</option>
            <option value="controller">Controller</option>
          </select>
        </label>
      </div>
    </section>
  );
}
