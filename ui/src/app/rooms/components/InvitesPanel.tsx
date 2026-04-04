'use client';

import { WatchPartyInvite } from '@/lib/watchPartyApi';
import { roleLabel } from '@/lib/watchPartyRoles';

type Props = {
  invites: WatchPartyInvite[];
  onJoin: (roomId: string) => void;
  onDecline: (roomId: string) => void;
  decliningRoomId: string | null;
};

export default function InvitesPanel({ invites, onJoin, onDecline, decliningRoomId }: Props) {
  return (
    <section className="rf-flat-section flex h-full min-h-0 flex-col gap-4">
      <div className="space-y-0">
        <h2 className="text-xl font-semibold">Invites Inbox</h2>
      </div>

      {invites.length === 0 ? (
        <div className="rf-flat-empty text-sm muted">No pending invites.</div>
      ) : (
        <ul className="rf-flat-list min-h-0 flex-1 overflow-y-auto pr-1 pt-2">
          {invites.map((invite) => (
            <li key={invite.room_id} className="rf-flat-row">
              <div
                className="flex w-full items-center justify-between gap-3 rounded-2xl px-3 py-2 text-left transition hover:bg-white/[0.05]"
                role="button"
                tabIndex={0}
                onClick={() => onJoin(invite.room_id)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    onJoin(invite.room_id);
                  }
                }}
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium">{invite.item_title}</p>
                  <p className="text-xs muted">
                    Host: {invite.host_username} • Role: {roleLabel(invite.role, 'video')}
                    {invite.password_required ? ' • Password required' : ''}
                  </p>
                </div>
                <button
                  type="button"
                  className="rf-text-action text-xs"
                  onClick={(event) => {
                    event.stopPropagation();
                    onDecline(invite.room_id);
                  }}
                  disabled={decliningRoomId === invite.room_id}
                >
                  {decliningRoomId === invite.room_id ? 'Declining…' : 'Decline'}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
