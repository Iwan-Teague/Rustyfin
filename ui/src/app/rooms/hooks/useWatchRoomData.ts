import { useCallback, useState } from 'react';

import type { Me } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import { WatchPartyRoomResponse, getWatchPartyRoom } from '@/lib/watchPartyApi';

type Args = {
  roomId: string;
  me: Me | null;
  setError: (message: string) => void;
  appendDebug: (message: string) => void;
};

export function useWatchRoomData({ roomId, me, setError, appendDebug }: Args) {
  const [room, setRoom] = useState<WatchPartyRoomResponse | null>(null);
  const [loadingRoom, setLoadingRoom] = useState(true);
  const [joinedRole, setJoinedRole] = useState<string | null>(null);

  const syncJoinedRole = useCallback(
    (roomData: WatchPartyRoomResponse) => {
      if (!me) return;
      const current = roomData.members.find((member) => member.user_id === me.id);
      setJoinedRole(current?.status === 'joined' ? current.role : null);
    },
    [me],
  );

  const loadRoom = useCallback(async () => {
    setLoadingRoom(true);
    setError('');
    try {
      const data = await getWatchPartyRoom(roomId);
      setRoom(data);
      appendDebug(
        `room loaded mode=${data.room_mode} status=${data.status} members=${data.members.length} password_required=${data.password_required}`,
      );
      syncJoinedRole(data);
    } catch (err: unknown) {
      const message = clientErrorMessage(err, 'Failed to load watch party room');
      setError(message);
      appendDebug(`room load failed error=${message}`);
    } finally {
      setLoadingRoom(false);
    }
  }, [appendDebug, roomId, setError, syncJoinedRole]);

  const refreshRoom = useCallback(async () => {
    try {
      const data = await getWatchPartyRoom(roomId);
      setRoom(data);
      syncJoinedRole(data);
    } catch {
      // Non-fatal background refresh.
    }
  }, [roomId, syncJoinedRole]);

  return {
    room,
    setRoom,
    loadingRoom,
    joinedRole,
    setJoinedRole,
    loadRoom,
    refreshRoom,
  };
}
