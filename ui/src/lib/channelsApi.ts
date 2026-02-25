import { apiFetch, apiJson } from './api';

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
};

export type MessageInfo = {
  id: string;
  channel_id: string;
  user_id: string;
  username: string;
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
  limit = 50,
): Promise<ChannelMessage[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  if (before !== undefined) params.set('before', String(before));
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
  if (!res.ok) {
    const payload = await res.json().catch(() => ({}));
    throw new Error(payload?.error?.message || 'Failed to upload attachment');
  }
  const message = (await res.json()) as ChannelMessage;
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
