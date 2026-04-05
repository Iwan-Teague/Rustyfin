'use client';

import { useState } from 'react';
import { WatchPartyUser } from '@/lib/watchPartyApi';
import { nonAdminRoleLabel, type WatchPartyRoomMode } from '@/lib/watchPartyRoles';

export type SelectedInvite = {
  role: 'viewer' | 'controller';
};

const INVITE_NAME_MAX_CHARS = 14;

function truncateInviteName(name: string): string {
  if (name.length <= INVITE_NAME_MAX_CHARS) return name;
  return `${name.slice(0, INVITE_NAME_MAX_CHARS)}…`;
}

type Props = {
  users: WatchPartyUser[];
  currentUserId: string;
  roomMode: WatchPartyRoomMode;
  selected: Record<string, SelectedInvite>;
  embedded?: boolean;
  noShadow?: boolean;
  fillHeight?: boolean;
  onToggle: (userId: string, initialRole?: 'viewer' | 'controller') => void;
  onRoleChange: (userId: string, role: 'viewer' | 'controller') => void;
};

export default function UserInvitePicker({
  users,
  currentUserId,
  roomMode,
  selected,
  embedded = false,
  noShadow = false,
  fillHeight = false,
  onToggle,
  onRoleChange,
}: Props) {
  // Tracks chosen role for users not yet checked — preserved when they get selected
  const [pendingRoles, setPendingRoles] = useState<Record<string, 'viewer' | 'controller'>>({});
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
        <h2 className="text-xl font-semibold">Invite Users</h2>
        <p className="text-sm muted">Set each user&apos;s access level, then check the box to invite them.</p>
      </div>

      <ul
        className={
          fillHeight
            ? 'rf-flat-list min-h-0 flex-1 overflow-y-auto pr-1'
            : 'rf-flat-list'
        }
      >
        {users
          .filter((user) => user.id !== currentUserId)
          .map((user) => {
            const isSelected = Boolean(selected[user.id]);
            const role = isSelected ? selected[user.id].role : (pendingRoles[user.id] ?? 'viewer');
            return (
              <li
                key={user.id}
                className="rf-flat-row"
                style={{ boxShadow: 'none' }}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="flex min-w-0 items-center gap-3">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => onToggle(user.id, pendingRoles[user.id])}
                      aria-label={`Invite ${user.username}`}
                      className="h-4 w-4 shrink-0"
                    />
                    <span className="w-[14ch] truncate text-sm font-medium" title={user.username}>
                      {truncateInviteName(user.username)}
                    </span>
                  </div>
                  <div className="w-[7.75rem] shrink-0">
                    <select
                      className="rf-flat-input w-full px-2 py-1.5 text-sm"
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
                </div>
              </li>
            );
          })}
      </ul>
    </section>
  );
}
