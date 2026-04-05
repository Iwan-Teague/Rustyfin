import { apiFetch, apiJson, extractErrorMessage, parseResponseBody } from './api';

export type Channel = {
  id: string;
  name: string;
  kind: 'text' | 'voice';
  position: number;
  is_private: boolean;
  created_by: string;
  created_ts: number;
};

export type ChannelMessage = {
  id: string;
  channel_id: string;
  user_id: string;
  username: string;
  avatar_url?: string | null;
  content: string;
  attachments: ChannelMessageAttachment[];
  created_ts: number;
};

export type ChannelMessageAttachment = {
  id: string;
  filename: string;
  content_type: string;
  size_bytes: number;
  download_path: string;
};

export type CreateChannelRequest = {
  name: string;
  kind: 'text' | 'voice';
  is_private: boolean;
};

// ── WebSocket event types ────────────────────────────────────────────────────

export type ChannelInfo = {
  id: string;
  name: string;
  kind: 'text' | 'voice';
  position: number;
  is_private: boolean;
};

export type UserInfo = {
  user_id: string;
  username: string;
  avatar_url?: string | null;
};

export type MessageInfo = {
  id: string;
  channel_id: string;
  user_id: string;
  username: string;
  avatar_url?: string | null;
  content: string;
  attachments: ChannelMessageAttachment[];
  created_ts: number;
};

export type ChannelEvent =
  | {
      type: 'hello';
      channels: ChannelInfo[];
      voice_presence: Record<string, UserInfo[]>;
      voice_active_since_ts: Record<string, number>;
      voice_transcriptions?: Record<string, VoiceTranscriptionState>;
    }
  | {
      type: 'voice_presence';
      channel_id: string;
      user_id: string;
      username: string;
      avatar_url?: string | null;
      joined: boolean;
      active_since_ts?: number | null;
    }
  | { type: 'voice_joined'; channel_id: string; existing_members: UserInfo[] }
  | {
      type: 'voice_transcription_state';
      channel_id: string;
      state: VoiceTranscriptionState;
    }
  | { type: 'rtc_offer'; from_user_id: string; channel_id: string; sdp: string }
  | { type: 'rtc_answer'; from_user_id: string; channel_id: string; sdp: string }
  | { type: 'rtc_ice'; from_user_id: string; channel_id: string; candidate: string }
  | { type: 'new_message'; msg: MessageInfo }
  | { type: 'channel_created'; channel: ChannelInfo }
  | { type: 'channel_updated'; channel: ChannelInfo }
  | { type: 'channel_deleted'; channel_id: string }
  | { type: 'message_deleted'; message_id: string; channel_id: string }
  | { type: 'pong' }
  | { type: 'error'; message: string };

export type VoiceTranscriptionState = {
  status: string;
  session_id?: string | null;
  started_by_username?: string | null;
  started_ts?: number | null;
  ended_ts?: number | null;
  output_available: boolean;
  message?: string | null;
};

export type VoiceTranscriptionStatus = {
  channel_id: string;
  status: string;
  session_id?: string | null;
  started_by_username?: string | null;
  started_ts?: number | null;
  ended_ts?: number | null;
  output_available: boolean;
  output_download_path?: string | null;
  message?: string | null;
  entry_count: number;
};

export type VoiceTranscriptionSessionSummary = {
  session_id: string;
  status: string;
  started_by_username: string;
  started_ts: number;
  ended_ts?: number | null;
  output_available: boolean;
  output_download_path?: string | null;
  message?: string | null;
  entry_count: number;
};

export type VoiceTranscriptionSessionsResponse = {
  channel_id: string;
  sessions: VoiceTranscriptionSessionSummary[];
};

export type VoiceTranscribeChunkRequest = {
  session_id: string;
  sample_rate_hz: number;
  started_ts_ms: number;
  ended_ts_ms: number;
  pcm_s16le_base64: string;
  language?: string;
};

export type VoiceTranscribeChunkResponse = {
  accepted: boolean;
  persisted_segments: number;
};

export type VoiceTranscriptionRecordingUpload = {
  sessionId: string;
  captureStartedTsMs: number;
  captureEndedTsMs: number;
  blob: Blob;
  fileName?: string;
};

export type VoiceTranscriptionRecordingUploadResponse = {
  accepted: boolean;
  persisted_segments: number;
};

export type VoiceTranscriptionTextUpload = {
  sessionId: string;
  startedTsMs: number;
  endedTsMs: number;
  text: string;
};

export type VoiceTranscriptionTextUploadResponse = {
  accepted: boolean;
  persisted_segments: number;
};

// ── REST API ─────────────────────────────────────────────────────────────────

export async function listChannels(): Promise<Channel[]> {
  return apiJson<Channel[]>('/channels');
}

export async function createChannel(req: CreateChannelRequest): Promise<Channel> {
  return apiJson<Channel>('/channels', {
    method: 'POST',
    body: JSON.stringify(req),
  });
}

export async function renameChannel(id: string, name: string): Promise<Channel> {
  return apiJson<Channel>(`/channels/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ name }),
  });
}

export async function deleteChannel(id: string): Promise<void> {
  await apiJson<void>(`/channels/${id}`, { method: 'DELETE' });
}

export async function getMessages(
  channelId: string,
  before?: number,
  beforeId?: string,
  limit = 50,
): Promise<ChannelMessage[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  if (before !== undefined) params.set('before', String(before));
  if (beforeId && beforeId.trim()) params.set('before_id', beforeId.trim());
  const messages = await apiJson<ChannelMessage[]>(`/channels/${channelId}/messages?${params}`);
  return messages.map((message) => ({
    ...message,
    attachments: message.attachments || [],
  }));
}

export async function postMessage(channelId: string, content: string): Promise<ChannelMessage> {
  const message = await apiJson<ChannelMessage>(`/channels/${channelId}/messages`, {
    method: 'POST',
    body: JSON.stringify({ content }),
  });
  return { ...message, attachments: message.attachments || [] };
}

export async function uploadMessageAttachment(
  channelId: string,
  file: File,
  content?: string,
): Promise<ChannelMessage> {
  const body = new FormData();
  body.append('file', file);
  if (content && content.trim()) {
    body.append('content', content.trim());
  }

  const res = await apiFetch(`/channels/${channelId}/attachments`, {
    method: 'POST',
    body,
  });
  const payload = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(payload, 'Failed to upload attachment'));
  }
  if (!payload || typeof payload !== 'object') {
    throw new Error('Attachment upload response was empty');
  }
  const message = payload as ChannelMessage;
  return { ...message, attachments: message.attachments || [] };
}

export async function deleteMessage(channelId: string, messageId: string): Promise<void> {
  await apiJson<void>(`/channels/${channelId}/messages/${messageId}`, { method: 'DELETE' });
}

export async function getVoiceTranscriptionStatus(
  channelId: string,
): Promise<VoiceTranscriptionStatus> {
  return apiJson<VoiceTranscriptionStatus>(`/channels/${channelId}/transcription/status`);
}

export async function listVoiceTranscriptionSessions(
  channelId: string,
): Promise<VoiceTranscriptionSessionsResponse> {
  return apiJson<VoiceTranscriptionSessionsResponse>(`/channels/${channelId}/transcription/sessions`);
}

export async function deleteVoiceTranscriptionSession(
  channelId: string,
  sessionId: string,
): Promise<void> {
  await apiJson<void>(`/channels/${channelId}/transcription/sessions/${sessionId}`, {
    method: 'DELETE',
  });
}

export async function startVoiceTranscription(
  channelId: string,
): Promise<VoiceTranscriptionStatus> {
  return apiJson<VoiceTranscriptionStatus>(`/channels/${channelId}/transcription/start`, {
    method: 'POST',
  });
}

export async function stopVoiceTranscription(channelId: string): Promise<VoiceTranscriptionStatus> {
  return apiJson<VoiceTranscriptionStatus>(`/channels/${channelId}/transcription/stop`, {
    method: 'POST',
  });
}

export async function cancelVoiceTranscription(
  channelId: string,
): Promise<VoiceTranscriptionStatus> {
  return apiJson<VoiceTranscriptionStatus>(`/channels/${channelId}/transcription/cancel`, {
    method: 'POST',
  });
}

export async function uploadVoiceTranscriptionChunk(
  channelId: string,
  payload: VoiceTranscribeChunkRequest,
): Promise<VoiceTranscribeChunkResponse> {
  return apiJson<VoiceTranscribeChunkResponse>(`/channels/${channelId}/transcription/chunk`, {
    method: 'POST',
    body: JSON.stringify(payload),
  });
}

export async function uploadVoiceTranscriptionRecording(
  channelId: string,
  payload: VoiceTranscriptionRecordingUpload,
): Promise<VoiceTranscriptionRecordingUploadResponse> {
  const body = new FormData();
  body.append(
    'file',
    payload.blob,
    payload.fileName ?? `voice-transcript-${payload.sessionId}.webm`,
  );
  body.append('session_id', payload.sessionId);
  body.append('capture_started_ts_ms', String(payload.captureStartedTsMs));
  body.append('capture_ended_ts_ms', String(payload.captureEndedTsMs));

  const res = await apiFetch(`/channels/${channelId}/transcription/recording`, {
    method: 'POST',
    body,
  });
  const parsed = await parseResponseBody(res);
  if (!res.ok) {
    throw new Error(extractErrorMessage(parsed, 'Failed to upload transcript recording'));
  }
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Transcript upload response was empty');
  }
  return parsed as VoiceTranscriptionRecordingUploadResponse;
}

export async function uploadVoiceTranscriptionText(
  channelId: string,
  payload: VoiceTranscriptionTextUpload,
): Promise<VoiceTranscriptionTextUploadResponse> {
  return apiJson<VoiceTranscriptionTextUploadResponse>(`/channels/${channelId}/transcription/text`, {
    method: 'POST',
    body: JSON.stringify({
      session_id: payload.sessionId,
      started_ts_ms: payload.startedTsMs,
      ended_ts_ms: payload.endedTsMs,
      text: payload.text,
    }),
  });
}
