'use client';

import { WatchPartyUser } from '@/lib/watchPartyApi';

export type SelectedInvite = {
  role: 'viewer' | 'controller';
};

type Props = {
  users: WatchPartyUser[];
  currentUserId: string;
  selected: Record<string, SelectedInvite>;
  onToggle: (userId: string) => void;
  onRoleChange: (userId: string, role: 'viewer' | 'controller') => void;
};

export default function UserInvitePicker({
  users,
  currentUserId,
  selected,
  onToggle,
  onRoleChange,
}: Props) {
  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Invite Users</h2>
        <p className="text-sm muted">Choose who to invite and what role they should have in-room.</p>
      </div>

      <ul className="space-y-2">
        {users
          .filter((user) => user.id !== currentUserId)
          .map((user) => {
            const isSelected = Boolean(selected[user.id]);
            const role = selected[user.id]?.role || 'viewer';
            return (
              <li key={user.id} className="tile rounded-xl px-3 py-2">
                <div className="grid items-center gap-2 md:grid-cols-[auto_1fr_auto]">
                  <label className="inline-flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => onToggle(user.id)}
                      aria-label={`Invite ${user.username}`}
                    />
                    <span>{user.username}</span>
                  </label>

                  <span className="text-xs muted">{user.id}</span>

                  <label className="text-xs muted">
                    <span className="sr-only">Role for {user.username}</span>
                    <select
                      className="select px-2 py-1.5 text-sm"
                      aria-label={`Role for ${user.username}`}
                      value={role}
                      disabled={!isSelected}
                      onChange={(e) => onRoleChange(user.id, e.target.value as 'viewer' | 'controller')}
                    >
                      <option value="viewer">Viewer</option>
                      <option value="controller">Controller</option>
                    </select>
                  </label>
                </div>
              </li>
            );
          })}
      </ul>
    </section>
  );
}
