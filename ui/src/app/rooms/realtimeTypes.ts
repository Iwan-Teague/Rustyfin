import type {
  WsAudioStateMessage,
  WsCreateStateMessage,
  WsOnlineAudioStatusMessage,
  WsPlayStateMessage,
  WsRoomReconfiguredMessage,
  WsWebStateMessage,
  WsYouTubeStateMessage,
} from '@/lib/watchPartyApi';

export type WsStateMessage = {
  type: 'state';
  room_id: string;
  item_id: string;
  playing: boolean;
  position_ms: number;
  updated_ts_ms: number;
  server_ts_ms: number;
  members: WsPresenceMember[];
};

export type WsPresenceMember = {
  user_id: string;
  username: string;
  role: string;
  connected: boolean;
};

export type WsPresenceMessage = {
  type: 'presence';
  user_id: string;
  connected: boolean;
};

export type WsErrorMessage = {
  type: 'error';
  message: string;
};

export type WsPongMessage = {
  type: 'pong';
};

export type WsRoomEndedMessage = {
  type: 'room_ended';
};

export type WsMessage =
  | WsStateMessage
  | WsAudioStateMessage
  | WsOnlineAudioStatusMessage
  | WsWebStateMessage
  | WsYouTubeStateMessage
  | WsCreateStateMessage
  | WsPlayStateMessage
  | WsRoomReconfiguredMessage
  | WsPresenceMessage
  | WsErrorMessage
  | WsPongMessage
  | WsRoomEndedMessage;

export type RoomMode = 'video' | 'audio' | 'youtube' | 'web' | 'create' | 'play';

export type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
  hls_start_url: string;
  media_info_url: string;
  duration_ms?: number | null;
};

export type PlaybackSession = {
  session_id: string;
  hls_url: string;
};

export type StartPlaybackOptions = {
  autoplayWhenNoState?: boolean;
  silent?: boolean;
  targetHeightOverride?: number | null;
  seekTimeOverrideSecs?: number;
};

export type RuntimeConfig = {
  backend_origin?: string | null;
};
