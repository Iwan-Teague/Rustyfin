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

export type CreateWatchPartyRoomRequest =
  | {
      item_id: string;
      audio_library_id?: never;
      room_mode?: never;
      invites: WatchPartyInviteInput[];
      password?: string;
      policy: WatchPartyPolicy;
    }
  | {
      audio_library_id: string;
      item_id?: never;
      room_mode?: never;
      invites: WatchPartyInviteInput[];
      password?: string;
      policy: WatchPartyPolicy;
    }
  | {
      room_mode: 'youtube';
      item_id?: never;
      audio_library_id?: never;
      web_url?: never;
      invites: WatchPartyInviteInput[];
      password?: string;
      policy: WatchPartyPolicy;
    }
  | {
      room_mode: 'web';
      web_url?: string;
      item_id?: never;
      audio_library_id?: never;
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
  created_ts: number;
  ended_ts?: number | null;
  password_required: boolean;
  policy: WatchPartyPolicy;
  members: WatchPartyRoomMember[];
  room_mode: string;
  audio_library_id?: string;
  youtube_video_id?: string | null;
  web_url?: string | null;
};

export type JoinWatchPartyRoomResponse = {
  ok: boolean;
  role: 'host' | 'controller' | 'viewer' | string;
};

export type ReconfigureWatchPartyRoomRequest =
  | {
      room_mode: 'video';
      item_id: string;
      audio_library_id?: never;
      youtube_video_id?: never;
    }
  | {
      room_mode: 'audio';
      audio_library_id: string;
      item_id?: never;
      youtube_video_id?: never;
    }
  | {
      room_mode: 'youtube';
      youtube_video_id?: string;
      item_id?: never;
      audio_library_id?: never;
      web_url?: never;
    }
  | {
      room_mode: 'web';
      web_url?: string;
      item_id?: never;
      audio_library_id?: never;
      youtube_video_id?: never;
    };

export type ReconfigureWatchPartyRoomResponse = {
  ok: boolean;
  room_mode: 'video' | 'audio' | 'youtube' | 'web' | string;
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

export type InviteMembersResponse = {
  ok: boolean;
  invited: number;
  cooldown_blocked_users: string[];
};

export type AudioTrack = {
  id: string;
  title: string;
  artist: string;
  album: string;
  album_art_url?: string;
  duration_ms?: number;
};

export type YouTubeSearchResult = {
  video_id: string;
  title: string;
  channel: string;
  thumbnail_url: string;
  view_count?: number | null;
};

export type QueueEntry = AudioTrack & { track_id: string };

export type WsPresenceMember = {
  user_id: string;
  username: string;
  role: string;
  connected: boolean;
};

export type WsAudioStateMessage = {
  type: 'audio_state';
  room_id: string;
  track_id: string;
  title: string;
  artist: string;
  album: string;
  album_art_url?: string;
  duration_ms?: number;
  position_ms: number;
  playing: boolean;
  updated_ts_ms: number;
  server_ts_ms: number;
  queue: QueueEntry[];
  queue_index: number;
  members: WsPresenceMember[];
};

export type WsYouTubeStateMessage = {
  type: 'youtube_state' | 'you_tube_state';
  room_id: string;
  video_id: string;
  playing: boolean;
  position_ms: number;
  updated_ts_ms: number;
  server_ts_ms: number;
  queue: string[];
  search_query: string;
  search_results: YouTubeSearchResult[];
  members: WsPresenceMember[];
};

export type WsWebStateMessage = {
  type: 'web_state';
  room_id: string;
  url: string;
  updated_ts_ms: number;
  server_ts_ms: number;
  members: WsPresenceMember[];
};

export type WsRoomReconfiguredMessage = {
  type: 'room_reconfigured';
  room_mode: string;
  item_id: string;
  audio_library_id?: string | null;
  youtube_video_id?: string | null;
  web_url?: string | null;
};

export type PublicRoom = {
  room_id: string;
  host_username: string;
  title: string;
  room_mode: string;
  password_required: boolean;
  member_count: number;
  created_ts: number;
};

export async function listPublicRooms(): Promise<PublicRoom[]> {
  return apiJson<PublicRoom[]>('/watch-party/rooms');
}

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

export async function reconfigureWatchPartyRoom(
  roomId: string,
  payload: ReconfigureWatchPartyRoomRequest,
): Promise<ReconfigureWatchPartyRoomResponse> {
  return apiJson<ReconfigureWatchPartyRoomResponse>(`/watch-party/rooms/${roomId}/reconfigure`, {
    method: 'POST',
    body: JSON.stringify(payload),
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

export async function inviteToRoom(
  roomId: string,
  invites: { user_id: string; role: 'viewer' | 'controller' }[],
): Promise<InviteMembersResponse> {
  return apiJson<InviteMembersResponse>(`/watch-party/rooms/${roomId}/invite`, {
    method: 'POST',
    body: JSON.stringify({ invites }),
  });
}

export async function listAudioTracks(roomId: string, q?: string): Promise<AudioTrack[]> {
  const query = q ? `?q=${encodeURIComponent(q)}` : '';
  return apiJson<AudioTrack[]>(`/watch-party/rooms/${roomId}/audio/tracks${query}`);
}

export async function searchYouTubeVideos(
  roomId: string,
  q: string,
  limit = 10,
): Promise<YouTubeSearchResult[]> {
  const params = new URLSearchParams();
  params.set('q', q);
  params.set('limit', String(limit));
  return apiJson<YouTubeSearchResult[]>(
    `/watch-party/rooms/${roomId}/youtube/search?${params.toString()}`,
  );
}

export async function lookupYouTubeVideos(
  roomId: string,
  videoIds: string[],
): Promise<YouTubeSearchResult[]> {
  return apiJson<YouTubeSearchResult[]>(`/watch-party/rooms/${roomId}/youtube/lookup`, {
    method: 'POST',
    body: JSON.stringify({ video_ids: videoIds }),
  });
}
