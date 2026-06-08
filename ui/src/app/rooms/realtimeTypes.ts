import type {
  WsAudioStateMessage,
  WsCreateStateMessage,
  WsOnlineAudioStatusMessage,
  WsPlayStateMessage,
  WsScreenAnswerMessage,
  WsScreenIceMessage,
  WsScreenOfferMessage,
  WsScreenStateMessage,
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
  | WsScreenStateMessage
  | WsScreenOfferMessage
  | WsScreenAnswerMessage
  | WsScreenIceMessage
  | WsYouTubeStateMessage
  | WsCreateStateMessage
  | WsPlayStateMessage
  | WsRoomReconfiguredMessage
  | WsPresenceMessage
  | WsErrorMessage
  | WsPongMessage
  | WsRoomEndedMessage;

export type RoomMode = 'video' | 'audio' | 'youtube' | 'web' | 'screen' | 'create' | 'play';

export type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
  hls_start_url: string;
  media_info_url: string;
  duration_ms?: number | null;
  // Server-authoritative default play method. "direct" means the browser can natively
  // play `direct_url` (range server handles seeking); anything else (including a missing
  // field on older servers) means use the HLS transcode path.
  play_method?: 'direct' | 'transcode' | string | null;
  direct_play?: boolean | null;
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
  syncRoomStateOnReady?: boolean;
};

export type RuntimeConfig = {
  backend_origin?: string | null;
};
