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
    <section className="panel flex h-full min-h-0 flex-col gap-4 p-5 sm:p-6">
      <div className="space-y-2">
        <h2 className="text-xl font-semibold">Invites Inbox</h2>
        <p className="text-sm muted">Pending watch-party invitations for this account.</p>
      </div>

      {invites.length === 0 ? (
        <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">No pending invites.</div>
      ) : (
        <ul className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
          {invites.map((invite) => (
            <li key={invite.room_id} className="tile rounded-xl px-3 py-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <p className="text-sm font-medium">{invite.item_title}</p>
                  <p className="text-xs muted">
                    Host: {invite.host_username} • Role: {roleLabel(invite.role, 'video')}
                    {invite.password_required ? ' • Password required' : ''}
                  </p>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    className="btn-secondary px-3 py-1.5 text-xs"
                    onClick={() => onJoin(invite.room_id)}
                  >
                    Join
                  </button>
                  <button
                    type="button"
                    className="btn-ghost px-3 py-1.5 text-xs"
                    onClick={() => onDecline(invite.room_id)}
                    disabled={decliningRoomId === invite.room_id}
                  >
                    {decliningRoomId === invite.room_id ? 'Declining…' : 'Decline'}
                  </button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
