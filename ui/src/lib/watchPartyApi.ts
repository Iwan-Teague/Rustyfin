import { apiJson } from './api';

export type WatchPartyUser = {
  id: string;
  username: string;
};

export type WatchPartyEligibleLibrariesResponse = {
  library_ids: string[];
};

export type WatchPartyPolicy = {
  allow_non_host_play_pause: boolean;
  allow_non_host_seek: boolean;
  default_join_role: 'viewer' | 'controller';
  invite_only: boolean;
};

export type WatchPartyInviteInput = {
  user_id: string;
  role: 'viewer' | 'controller';
};

export type CreateWatchPartyRoomRequest = {
  item_id: string;
  invites: WatchPartyInviteInput[];
  password?: string;
  policy: WatchPartyPolicy;
};

export type CreateWatchPartyRoomResponse = {
  room_id: string;
  join_path: string;
};

export type WatchPartyRoomMember = {
  user_id: string;
  username: string;
  role: string;
  status: string;
};

export type WatchPartyRoomResponse = {
  room_id: string;
  item_id: string;
  host_user_id: string;
  status: 'lobby' | 'ended' | string;
  password_required: boolean;
  policy: WatchPartyPolicy;
  members: WatchPartyRoomMember[];
};

export type JoinWatchPartyRoomResponse = {
  ok: boolean;
  role: 'host' | 'controller' | 'viewer' | string;
};

export type WatchPartyInvite = {
  room_id: string;
  item_id: string;
  item_title: string;
  host_user_id: string;
  host_username: string;
  created_ts: number;
  password_required: boolean;
  role: string;
  status: string;
};

export async function listWatchPartyUsers(): Promise<WatchPartyUser[]> {
  return apiJson<WatchPartyUser[]>('/watch-party/users');
}

export async function getEligibleLibraries(userIds: string[]): Promise<string[]> {
  const result = await apiJson<WatchPartyEligibleLibrariesResponse>(
    '/watch-party/eligible-libraries',
    {
      method: 'POST',
      body: JSON.stringify({ user_ids: userIds }),
    },
  );
  return result.library_ids;
}

export async function createWatchPartyRoom(
  payload: CreateWatchPartyRoomRequest,
): Promise<CreateWatchPartyRoomResponse> {
  return apiJson<CreateWatchPartyRoomResponse>('/watch-party/rooms', {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function getWatchPartyRoom(roomId: string): Promise<WatchPartyRoomResponse> {
  return apiJson<WatchPartyRoomResponse>(`/watch-party/rooms/${roomId}`);
}

export async function joinWatchPartyRoom(
  roomId: string,
  password?: string,
): Promise<JoinWatchPartyRoomResponse> {
  return apiJson<JoinWatchPartyRoomResponse>(`/watch-party/rooms/${roomId}/join`, {
    method: 'POST',
    body: JSON.stringify({ password }),
  });
}

export async function leaveWatchPartyRoom(roomId: string): Promise<void> {
  await apiJson(`/watch-party/rooms/${roomId}/leave`, {
    method: 'POST',
  });
}

export async function endWatchPartyRoom(roomId: string): Promise<void> {
  await apiJson(`/watch-party/rooms/${roomId}/end`, {
    method: 'POST',
  });
}

export async function listWatchPartyInvites(): Promise<WatchPartyInvite[]> {
  return apiJson<WatchPartyInvite[]>('/watch-party/invites');
}

export async function declineWatchPartyInvite(roomId: string): Promise<void> {
  await apiJson(`/watch-party/invites/${roomId}/decline`, {
    method: 'POST',
  });
}
