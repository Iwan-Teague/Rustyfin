'use client';

import { useState } from 'react';
import { WatchPartyUser } from '@/lib/watchPartyApi';
import { nonAdminRoleLabel, type WatchPartyRoomMode } from '@/lib/watchPartyRoles';

export type SelectedInvite = {
  role: 'viewer' | 'controller';
};

type Props = {
  users: WatchPartyUser[];
  currentUserId: string;
  roomMode: WatchPartyRoomMode;
  selected: Record<string, SelectedInvite>;
  onToggle: (userId: string, initialRole?: 'viewer' | 'controller') => void;
  onRoleChange: (userId: string, role: 'viewer' | 'controller') => void;
};

export default function UserInvitePicker({
  users,
  currentUserId,
  roomMode,
  selected,
  onToggle,
  onRoleChange,
}: Props) {
  // Tracks chosen role for users not yet checked — preserved when they get selected
  const [pendingRoles, setPendingRoles] = useState<Record<string, 'viewer' | 'controller'>>({});
  const memberLabel = nonAdminRoleLabel(roomMode);

  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Invite Users</h2>
        <p className="text-sm muted">Set each user's access level, then check the box to invite them.</p>
      </div>

      <ul className="space-y-2">
        {users
          .filter((user) => user.id !== currentUserId)
          .map((user) => {
            const isSelected = Boolean(selected[user.id]);
            const role = isSelected ? selected[user.id].role : (pendingRoles[user.id] ?? 'viewer');
            return (
              <li key={user.id} className="tile rounded-xl px-3 py-2">
                <div className="flex items-center gap-3">
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => onToggle(user.id, pendingRoles[user.id])}
                    aria-label={`Invite ${user.username}`}
                    className="h-4 w-4 shrink-0"
                  />
                  <span className="flex-1 text-sm font-medium">{user.username}</span>
                  <select
                    className="select px-2 py-1.5 text-sm"
                    aria-label={`Role for ${user.username}`}
                    value={role}
                    onChange={(e) => {
                      const newRole = e.target.value as 'viewer' | 'controller';
                      if (isSelected) {
                        onRoleChange(user.id, newRole);
                      } else {
                        setPendingRoles((prev) => ({ ...prev, [user.id]: newRole }));
                      }
                    }}
                  >
                    <option value="viewer">{memberLabel}</option>
                    <option value="controller">Admin</option>
                  </select>
                </div>
              </li>
            );
          })}
      </ul>
    </section>
  );
}
