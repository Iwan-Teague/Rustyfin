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
