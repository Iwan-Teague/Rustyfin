'use client';

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useAuth } from './auth';
import { readBrowserToken } from './browserAuth';
import {
  markChannelRead,
  uploadVoiceTranscriptionRecording,
  uploadVoiceTranscriptionText,
} from './channelsApi';
import type {
  ChannelEvent,
  ChannelInfo,
  ChannelMessage,
  UserInfo,
  VoiceTranscriptionRecordingUpload,
  VoiceTranscriptionTextUpload,
  VoiceTranscriptionState,
} from './channelsApi';
import VoiceEngine from '@/app/channels/components/VoiceEngine';
import type { PeerConnectionUiState } from '@/app/channels/components/VoiceEngine';

// ── Types ─────────────────────────────────────────────────────────────────────

interface VoiceSession {
  channelId: string;
  channelName: string;
  localStream: MediaStream | null; // null = listen-only (no mic)
  existingMembers: UserInfo[];
  muted: boolean;
  deafened: boolean;
}

interface PersistedVoiceSession {
  channelId: string;
  channelName: string;
  wantMic: boolean;
}

interface PersistedAudioDevicePrefs {
  inputDeviceId: string | null;
  outputDeviceId: string | null;
}

type RuntimeIceServer = {
  urls: string[];
  username?: string;
  credential?: string;
};

// Re-exported so UI consumers (e.g. the voice member list) can type the
// per-peer connection indicator without reaching into VoiceEngine internals.
export type { PeerConnectionUiState };

// channelId → (remote userId → connection UI state). Lets the UI show a
// "reconnecting…/couldn't connect" badge instead of silent dead audio.
type VoicePeerConnectionStates = Record<
  string,
  Record<string, PeerConnectionUiState>
>;

export interface ChannelsContextValue {
  wsReady: boolean;
  sendWs: (msg: object) => void;
  channels: ChannelInfo[];
  voicePresence: Record<string, UserInfo[]>;
  connectedVoiceChannelId: string | null;
  connectedVoiceChannelName: string | null;
  hasLocalVoiceSession: boolean;
  voiceActiveSince: Record<string, number>;
  voiceSpeaking: Record<string, string[]>;
  voicePeerConnectionStates: VoicePeerConnectionStates;
  voiceTranscriptions: Record<string, VoiceTranscriptionState>;
  remoteVolumes: Record<string, number>;
  localMicGain: number;
  preferredInputDeviceId: string | null;
  preferredOutputDeviceId: string | null;
  newMessages: ChannelMessage[];
  lastWsEvent: ChannelEvent | null;
  unreadByChannel: Record<string, number>;
  // Report the highest sort_seq the UI has seen for a channel (e.g. messages loaded over
  // REST in the text view) so the read cursor can be advanced to it.
  recordChannelSeq: (channelId: string, sortSeq: number) => void;
  // Mark a text channel read locally (clear its unread badge) and persist the read cursor
  // to the server at the highest sort_seq seen for that channel.
  markTextChannelRead: (channelId: string) => void;
  voiceSession: VoiceSession | null;
  joinVoice: (channelId: string, channelName: string) => Promise<string | null>;
  leaveVoice: () => void;
  toggleMute: () => void;
  toggleDeafen: () => void;
  setRemoteVolume: (userId: string, volume: number) => void;
  setLocalMicGain: (volume: number) => void;
  setPreferredAudioDevices: (inputDeviceId: string | null, outputDeviceId: string | null) => void;
  setVoiceTranscriptionState: (
    channelId: string,
    next: VoiceTranscriptionState | null,
  ) => void;
  uploadTranscriptionRecording: (
    channelId: string,
    payload: VoiceTranscriptionRecordingUpload,
  ) => Promise<void>;
  uploadTranscriptionText: (
    channelId: string,
    payload: VoiceTranscriptionTextUpload,
  ) => Promise<void>;
}

type VoiceEngineEventEnvelope = {
  seq: number;
  event: ChannelEvent;
};

// ── Context ───────────────────────────────────────────────────────────────────

const ChannelsContext = createContext<ChannelsContextValue | null>(null);

export function useChannels() {
  const ctx = useContext(ChannelsContext);
  if (!ctx) throw new Error('useChannels must be used within ChannelsProvider');
  return ctx;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function getWsUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${window.location.host}/api/v1/channels/ws`;
}

const VOICE_SESSION_KEY = 'channels_voice_session_v1';
const AUDIO_DEVICE_PREFS_KEY = 'channels_audio_device_prefs_v1';
const SYNTHETIC_INPUT_PREFIX = 'synthetic-audioinput-';

function loadPersistedVoiceSession(): PersistedVoiceSession | null {
  try {
    const raw = sessionStorage.getItem(VOICE_SESSION_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as PersistedVoiceSession;
    if (!parsed?.channelId || !parsed?.channelName) return null;
    return {
      channelId: parsed.channelId,
      channelName: parsed.channelName,
      wantMic: Boolean(parsed.wantMic),
    };
  } catch {
    return null;
  }
}

function savePersistedVoiceSession(session: PersistedVoiceSession) {
  try {
    sessionStorage.setItem(VOICE_SESSION_KEY, JSON.stringify(session));
  } catch {
    // ignore storage errors
  }
}

function clearPersistedVoiceSession() {
  try {
    sessionStorage.removeItem(VOICE_SESSION_KEY);
  } catch {
    // ignore storage errors
  }
}

function loadAudioDevicePrefs(): PersistedAudioDevicePrefs {
  try {
    const raw = localStorage.getItem(AUDIO_DEVICE_PREFS_KEY);
    if (!raw) {
      return { inputDeviceId: null, outputDeviceId: null };
    }
    const parsed = JSON.parse(raw) as PersistedAudioDevicePrefs;
    return {
      inputDeviceId: parsed?.inputDeviceId ?? null,
      outputDeviceId: parsed?.outputDeviceId ?? null,
    };
  } catch {
    return { inputDeviceId: null, outputDeviceId: null };
  }
}

function saveAudioDevicePrefs(inputDeviceId: string | null, outputDeviceId: string | null) {
  try {
    const payload: PersistedAudioDevicePrefs = {
      inputDeviceId: inputDeviceId || null,
      outputDeviceId: outputDeviceId || null,
    };
    localStorage.setItem(AUDIO_DEVICE_PREFS_KEY, JSON.stringify(payload));
  } catch {
    // ignore storage errors
  }
}

function findUserVoiceChannelId(
  presence: Record<string, UserInfo[]>,
  userId: string,
): string | null {
  for (const [channelId, members] of Object.entries(presence)) {
    if (members.some((member) => member.user_id === userId)) {
      return channelId;
    }
  }
  return null;
}

// ── Provider ──────────────────────────────────────────────────────────────────

export function ChannelsProvider({ children }: { children: React.ReactNode }) {
  const { me, loading: authLoading } = useAuth();

  const [wsReady, setWsReady] = useState(false);
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [voicePresence, setVoicePresence] = useState<Record<string, UserInfo[]>>({});
  const [voiceActiveSince, setVoiceActiveSince] = useState<Record<string, number>>({});
  const [voiceSpeaking, setVoiceSpeaking] = useState<Record<string, string[]>>({});
  const [voicePeerConnectionStates, setVoicePeerConnectionStates] =
    useState<VoicePeerConnectionStates>({});
  const [voiceTranscriptions, setVoiceTranscriptions] = useState<
    Record<string, VoiceTranscriptionState>
  >({});
  const [remoteVolumes, setRemoteVolumes] = useState<Record<string, number>>({});
  const [localMicGain, setLocalMicGainState] = useState(1);
  const [preferredInputDeviceId, setPreferredInputDeviceId] = useState<string | null>(null);
  const [preferredOutputDeviceId, setPreferredOutputDeviceId] = useState<string | null>(null);
  const [newMessages, setNewMessages] = useState<ChannelMessage[]>([]);
  const [lastWsEvent, setLastWsEvent] = useState<ChannelEvent | null>(null);
  const [unreadByChannel, setUnreadByChannel] = useState<Record<string, number>>({});
  const [voiceEngineEvents, setVoiceEngineEvents] = useState<VoiceEngineEventEnvelope[]>([]);
  const [voiceSession, setVoiceSession] = useState<VoiceSession | null>(null);
  const [voiceIceServers, setVoiceIceServers] = useState<RuntimeIceServer[]>([
    { urls: ['stun:stun.l.google.com:19302'] },
  ]);
  const hasLocalVoiceSession = voiceSession !== null;
  const connectedVoiceChannelId =
    voiceSession?.channelId ??
    (me?.id ? findUserVoiceChannelId(voicePresence, me.id) : null);
  const connectedVoiceChannelName =
    voiceSession?.channelName ??
    (connectedVoiceChannelId
      ? channels.find((channel) => channel.id === connectedVoiceChannelId)?.name ?? null
      : null);

  const wsRef = useRef<WebSocket | null>(null);
  // Holds mic stream + channel info while waiting for the server's voice_joined reply
  const pendingVoiceRef = useRef<{
    channelId: string;
    channelName: string;
    stream: MediaStream | null;
  } | null>(null);

  // Refs for reconnect and pagehide access (avoid stale closures)
  const voiceSessionRef = useRef<VoiceSession | null>(null);
  const voicePresenceRef = useRef<Record<string, UserInfo[]>>({});
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptsRef = useRef(0);
  const voiceEngineEventSeqRef = useRef(0);
  // Highest sort_seq the UI has observed per channel (from new_message events and
  // REST-loaded history). Drives the value sent to the mark-read endpoint.
  const seenSeqByChannelRef = useRef<Record<string, number>>({});
  // The currently-open text channel; new messages here don't bump unread and keep the
  // read cursor advanced. Kept in a ref so the WS onmessage closure isn't stale.
  const activeTextChannelRef = useRef<string | null>(null);

  const clearVoiceEngineEvents = useCallback(() => {
    voiceEngineEventSeqRef.current = 0;
    setVoiceEngineEvents([]);
  }, []);

  const enqueueVoiceEngineEvent = useCallback((event: ChannelEvent) => {
    if (
      event.type !== 'voice_presence' &&
      event.type !== 'rtc_offer' &&
      event.type !== 'rtc_answer' &&
      event.type !== 'rtc_ice'
    ) {
      return;
    }

    const seq = voiceEngineEventSeqRef.current + 1;
    voiceEngineEventSeqRef.current = seq;
    setVoiceEngineEvents((prev) => {
      const next = [...prev, { seq, event }];
      return next.length > 128 ? next.slice(next.length - 128) : next;
    });
  }, []);

  const resolvePreferredInputDeviceId = useCallback(
    async (selectedId: string | null): Promise<string | null> => {
      if (!selectedId) return null;
      const trimmed = selectedId.trim();
      if (!trimmed) return null;
      if (!trimmed.startsWith(SYNTHETIC_INPUT_PREFIX)) {
        return trimmed;
      }
      const indexPart = trimmed.slice(SYNTHETIC_INPUT_PREFIX.length);
      const oneBasedIndex = Number.parseInt(indexPart, 10);
      if (!Number.isFinite(oneBasedIndex) || oneBasedIndex < 1) {
        return null;
      }
      if (!navigator.mediaDevices?.enumerateDevices) {
        return null;
      }
      try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        const inputs = devices.filter((device) => device.kind === 'audioinput');
        const target = inputs[oneBasedIndex - 1];
        const realId = target?.deviceId?.trim();
        return realId || null;
      } catch {
        return null;
      }
    },
    [],
  );

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const prefs = loadAudioDevicePrefs();
    setPreferredInputDeviceId(prefs.inputDeviceId);
    setPreferredOutputDeviceId(prefs.outputDeviceId);
  }, []);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const response = await fetch('/runtime-config', { cache: 'no-store' });
        if (!response.ok) {
          return;
        }
        const payload = (await response.json()) as {
          ice_servers?: RuntimeIceServer[];
        };
        if (cancelled) {
          return;
        }
        const nextIceServers = Array.isArray(payload.ice_servers)
          ? payload.ice_servers
              .map((server) => ({
                urls: Array.isArray(server?.urls)
                  ? server.urls.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
                  : [],
                username:
                  typeof server?.username === 'string' && server.username.trim().length > 0
                    ? server.username.trim()
                    : undefined,
                credential:
                  typeof server?.credential === 'string' && server.credential.trim().length > 0
                    ? server.credential.trim()
                    : undefined,
              }))
              .filter((server) => server.urls.length > 0)
          : [];
        if (nextIceServers.length > 0) {
          setVoiceIceServers(nextIceServers);
        }
      } catch {
        // Keep default STUN-only fallback if runtime-config lookup fails.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Keep voiceSessionRef in sync so WS callbacks always see the latest session
  useEffect(() => {
    voiceSessionRef.current = voiceSession;
  }, [voiceSession]);

  useEffect(() => {
    voicePresenceRef.current = voicePresence;
  }, [voicePresence]);

  // ── sendWs ──────────────────────────────────────────────────────────────────

  const sendWs = useCallback((msg: object) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  }, []);

  // ── Unread tracking ───────────────────────────────────────────────────────────

  const recordChannelSeq = useCallback((channelId: string, sortSeq: number) => {
    if (!channelId || !Number.isFinite(sortSeq)) return;
    const prev = seenSeqByChannelRef.current[channelId] ?? 0;
    if (sortSeq > prev) {
      seenSeqByChannelRef.current[channelId] = sortSeq;
    }
  }, []);

  const markTextChannelRead = useCallback((channelId: string) => {
    if (!channelId) return;
    activeTextChannelRef.current = channelId;
    // Clear the badge immediately for snappy UX.
    setUnreadByChannel((prev) => {
      if (!(channelId in prev)) return prev;
      const next = { ...prev };
      delete next[channelId];
      return next;
    });
    // Persist the read cursor at the newest sort_seq we've seen for this channel.
    const seq = seenSeqByChannelRef.current[channelId] ?? 0;
    void markChannelRead(channelId, seq).catch(() => {
      // Non-fatal: the badge is already cleared locally and the next hello reconciles.
    });
  }, []);

  // ── WebSocket lifecycle with auto-reconnect ──────────────────────────────────

  useEffect(() => {
    if (authLoading) {
      return;
    }

    if (!me) {
      // Logged out — cancel any pending reconnect and close the socket
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      wsRef.current?.close();
      wsRef.current = null;
      setVoiceSession(null);
      setVoicePresence({});
      setVoiceActiveSince({});
      setVoiceSpeaking({});
      setVoicePeerConnectionStates({});
      setVoiceTranscriptions({});
      setRemoteVolumes({});
      setUnreadByChannel({});
      seenSeqByChannelRef.current = {};
      activeTextChannelRef.current = null;
      clearVoiceEngineEvents();
      clearPersistedVoiceSession();
      pendingVoiceRef.current = null;
      return;
    }

    const currentUserId = me.id;
    let intentionalClose = false;

    function connect() {
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }

      const token = readBrowserToken();
      if (!token) return;

      const ws = new WebSocket(getWsUrl());
      wsRef.current = ws;

      ws.onopen = () => {
        const t = readBrowserToken();
        if (t) ws.send(JSON.stringify({ type: 'auth', token: t }));
        reconnectAttemptsRef.current = 0;
      };

      ws.onmessage = (e) => {
        let event: ChannelEvent;
        try {
          event = JSON.parse(e.data) as ChannelEvent;
        } catch {
          return;
        }

        setLastWsEvent(event);
        enqueueVoiceEngineEvent(event);

        if (event.type === 'hello') {
          clearVoiceEngineEvents();
          setChannels(event.channels);
          // Seed unread badges from the server's per-user counts. The active text channel
          // (if any) is treated as already-read so reopening/reconnecting never shows a
          // stale badge for the channel currently on screen.
          const activeChannelId = activeTextChannelRef.current;
          setUnreadByChannel(() => {
            const seeded: Record<string, number> = {};
            for (const channel of event.channels) {
              if (channel.kind !== 'text') continue;
              if (channel.id === activeChannelId) continue;
              const count = channel.unread_count ?? 0;
              if (count > 0) seeded[channel.id] = count;
            }
            return seeded;
          });
          if (activeChannelId) {
            // Re-assert the read cursor for the channel we're viewing.
            markTextChannelRead(activeChannelId);
          }
          setVoicePresence(event.voice_presence);
          setVoiceActiveSince(event.voice_active_since_ts ?? {});
          setVoiceTranscriptions(event.voice_transcriptions ?? {});
          setVoiceSpeaking({});
          setVoicePeerConnectionStates({});
          setWsReady(true);
          // Auto-rejoin voice channel if there was an active session before the reconnect
          const prevSession = voiceSessionRef.current;
          if (prevSession) {
            setVoiceSession(null);
            const stream = prevSession.localStream;
            const isStreamActive = stream?.getTracks().some((t) => t.readyState === 'live') ?? false;
            pendingVoiceRef.current = {
              channelId: prevSession.channelId,
              channelName: prevSession.channelName,
              stream: isStreamActive ? stream : null,
            };
            ws.send(JSON.stringify({ type: 'join_voice', channel_id: prevSession.channelId }));
          } else if (pendingVoiceRef.current) {
            ws.send(
              JSON.stringify({
                type: 'join_voice',
                channel_id: pendingVoiceRef.current.channelId,
              }),
            );
          } else if (!pendingVoiceRef.current) {
            const alreadyConnectedChannelId = findUserVoiceChannelId(
              event.voice_presence ?? {},
              currentUserId,
            );
            if (alreadyConnectedChannelId) {
              clearPersistedVoiceSession();
              return;
            }

            const persisted = loadPersistedVoiceSession();
            if (!persisted) return;

            const target = event.channels.find(
              (c) => c.id === persisted.channelId && c.kind === 'voice',
            );
            if (!target) {
              clearPersistedVoiceSession();
              return;
            }

            void (async () => {
              let stream: MediaStream | null = null;
              if (persisted.wantMic && navigator.mediaDevices?.getUserMedia) {
                try {
                  stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                } catch {
                  stream = null;
                }
              }

              pendingVoiceRef.current = {
                channelId: target.id,
                channelName: target.name,
                stream,
              };
              ws.send(JSON.stringify({ type: 'join_voice', channel_id: target.id }));
            })();
          }
        } else if (event.type === 'voice_presence') {
          const pending = pendingVoiceRef.current;
          setVoicePresence((prev) => {
            const current = prev[event.channel_id] ?? [];
            if (event.joined) {
              if (current.find((u) => u.user_id === event.user_id)) return prev;
              return {
                ...prev,
                [event.channel_id]: [
                  ...current,
                  {
                    user_id: event.user_id,
                    username: event.username,
                    avatar_url: event.avatar_url ?? null,
                  },
                ],
              };
            } else {
              const updated = current.filter((u) => u.user_id !== event.user_id);
              const next = { ...prev };
              if (updated.length === 0) delete next[event.channel_id];
              else next[event.channel_id] = updated;
              return next;
            }
          });
          if (
            event.joined &&
            event.user_id === currentUserId &&
            pending &&
            pending.channelId === event.channel_id
          ) {
            const existingMembers = (voicePresenceRef.current[event.channel_id] ?? []).filter(
              (member) => member.user_id !== currentUserId,
            );
            pendingVoiceRef.current = null;
            savePersistedVoiceSession({
              channelId: event.channel_id,
              channelName: pending.channelName,
              wantMic: pending.stream !== null,
            });
            setVoiceSession({
              channelId: event.channel_id,
              channelName: pending.channelName,
              localStream: pending.stream,
              existingMembers,
              muted: false,
              deafened: false,
            });
          } else if (
            !event.joined &&
            event.user_id === currentUserId &&
            voiceSessionRef.current?.channelId === event.channel_id
          ) {
            clearPersistedVoiceSession();
            setVoiceSession(null);
          }
          if (!event.joined) {
            setVoiceSpeaking((prev) => {
              const channelSpeaking = prev[event.channel_id];
              if (!channelSpeaking || !channelSpeaking.includes(event.user_id)) {
                return prev;
              }
              const nextChannelSpeaking = channelSpeaking.filter(
                (userId) => userId !== event.user_id,
              );
              const next = { ...prev };
              if (nextChannelSpeaking.length === 0) {
                delete next[event.channel_id];
              } else {
                next[event.channel_id] = nextChannelSpeaking;
              }
              return next;
            });
          }
          setVoiceActiveSince((prev) => {
            const next = { ...prev };
            if (event.active_since_ts == null) {
              delete next[event.channel_id];
            } else {
              next[event.channel_id] = event.active_since_ts;
            }
            return next;
          });
        } else if (event.type === 'voice_joined') {
          const pending = pendingVoiceRef.current;
          if (pending && pending.channelId === event.channel_id) {
            pendingVoiceRef.current = null;
            savePersistedVoiceSession({
              channelId: event.channel_id,
              channelName: pending.channelName,
              wantMic: pending.stream !== null,
            });
            setVoiceSession({
              channelId: event.channel_id,
              channelName: pending.channelName,
              localStream: pending.stream,
              existingMembers: event.existing_members,
              muted: false,
              deafened: false,
            });
          }
        } else if (event.type === 'new_message') {
          const msg = event.msg;
          setNewMessages((prev) => [
            ...prev.slice(-200),
            {
              id: msg.id,
              channel_id: msg.channel_id,
              user_id: msg.user_id,
              username: msg.username,
              avatar_url: msg.avatar_url ?? null,
              content: msg.content,
              attachments: msg.attachments || [],
              created_ts: msg.created_ts,
              sort_seq: msg.sort_seq,
            },
          ]);
          // Track the newest sort_seq seen so mark-read can advance the cursor to it.
          recordChannelSeq(msg.channel_id, msg.sort_seq);
          const isActive = activeTextChannelRef.current === msg.channel_id;
          const isOwnMessage = msg.user_id === currentUserId;
          if (isActive) {
            // Viewing this channel: keep the server cursor current, no badge.
            markTextChannelRead(msg.channel_id);
          } else if (!isOwnMessage) {
            // Background channel, someone else posted: bump the unread badge.
            setUnreadByChannel((prev) => ({
              ...prev,
              [msg.channel_id]: (prev[msg.channel_id] ?? 0) + 1,
            }));
          }
        } else if (event.type === 'channel_created') {
          setChannels((prev) => {
            if (prev.find((c) => c.id === event.channel.id)) return prev;
            return [...prev, event.channel].sort(
              (a, b) => a.position - b.position || a.name.localeCompare(b.name),
            );
          });
        } else if (event.type === 'channel_updated') {
          setChannels((prev) =>
            prev.map((c) => (c.id === event.channel.id ? event.channel : c)),
          );
        } else if (event.type === 'channel_deleted') {
          if (voiceSessionRef.current?.channelId === event.channel_id) {
            clearPersistedVoiceSession();
          }
          setChannels((prev) => prev.filter((c) => c.id !== event.channel_id));
          setVoiceTranscriptions((prev) => {
            if (!(event.channel_id in prev)) return prev;
            const next = { ...prev };
            delete next[event.channel_id];
            return next;
          });
          setUnreadByChannel((prev) => {
            if (!(event.channel_id in prev)) return prev;
            const next = { ...prev };
            delete next[event.channel_id];
            return next;
          });
          delete seenSeqByChannelRef.current[event.channel_id];
          if (activeTextChannelRef.current === event.channel_id) {
            activeTextChannelRef.current = null;
          }
        } else if (event.type === 'message_deleted') {
          setNewMessages((prev) => prev.filter((m) => m.id !== event.message_id));
        } else if (event.type === 'voice_transcription_state') {
          setVoiceTranscriptions((prev) => ({
            ...prev,
            [event.channel_id]: event.state,
          }));
        }
      };

      ws.onclose = () => {
        setWsReady(false);
        wsRef.current = null;
        if (intentionalClose) return;
        // Reconnect with exponential backoff (1s, 2s, 4s … max 30s)
        const attempts = reconnectAttemptsRef.current;
        const delay = Math.min(1000 * 2 ** attempts, 30_000);
        reconnectAttemptsRef.current = Math.min(attempts + 1, 8);
        reconnectTimerRef.current = setTimeout(connect, delay);
      };
    }

    reconnectAttemptsRef.current = 0;
    connect();

    return () => {
      intentionalClose = true;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      wsRef.current?.close();
      wsRef.current = null;
    };
  }, [me?.id, authLoading]);

  // ── Clean disconnect when user actually leaves the site ───────────────────────

  useEffect(() => {
    function handlePageHide() {
      // Cancel reconnect so we don't try to reconnect after the tab is closing
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      const session = voiceSessionRef.current;
      if (session) {
        const ws = wsRef.current;
        if (ws?.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'leave_voice', channel_id: session.channelId }));
        }
        session.localStream?.getTracks().forEach((t) => t.stop());
      }
      wsRef.current?.close();
    }

    window.addEventListener('pagehide', handlePageHide);
    return () => window.removeEventListener('pagehide', handlePageHide);
  }, []);

  // ── Voice actions ────────────────────────────────────────────────────────────

  // Returns null on success, or an error string on failure.
  const joinVoice = useCallback(
    async (channelId: string, channelName: string): Promise<string | null> => {
      // New join intent supersedes any persisted session.
      clearPersistedVoiceSession();
      clearVoiceEngineEvents();

      const connectedChannelId = me?.id
        ? findUserVoiceChannelId(voicePresenceRef.current, me.id)
        : null;
      if (!voiceSession && !pendingVoiceRef.current && connectedChannelId === channelId) {
        return 'You are already connected to this voice channel in another tab.';
      }

      // Leave any existing session first
      if (pendingVoiceRef.current || voiceSession) {
        const previousChannelId =
          voiceSession?.channelId ?? pendingVoiceRef.current?.channelId ?? null;
        sendWs({ type: 'leave_voice', channel_id: voiceSession?.channelId ?? pendingVoiceRef.current?.channelId });
        voiceSession?.localStream?.getTracks().forEach((t) => t.stop());
        pendingVoiceRef.current?.stream?.getTracks().forEach((t) => t.stop());
        pendingVoiceRef.current = null;
        clearVoiceEngineEvents();
        if (previousChannelId) {
          setVoiceSpeaking((prev) => {
            if (!(previousChannelId in prev)) return prev;
            const next = { ...prev };
            delete next[previousChannelId];
            return next;
          });
          setVoicePeerConnectionStates((prev) => {
            if (!(previousChannelId in prev)) return prev;
            const next = { ...prev };
            delete next[previousChannelId];
            return next;
          });
        }
        setVoiceSession(null);
      }

      // Try to get the microphone; fall back to listen-only if unavailable or denied.
      let stream: MediaStream | null = null;
      let micStatusMessage: string | null = null;
      if (!window.isSecureContext) {
        micStatusMessage =
          'Microphone requires a secure origin. Open Rustyfin over HTTPS on your LAN address to talk in voice channels.';
      } else if (!navigator.mediaDevices?.getUserMedia) {
        micStatusMessage =
          'Microphone API is unavailable in this browser context. Joined as listener.';
      } else {
        try {
          const resolvedInputId = await resolvePreferredInputDeviceId(preferredInputDeviceId);
          const preferredConstraint = resolvedInputId
            ? { deviceId: { exact: resolvedInputId } }
            : true;
          stream = await navigator.mediaDevices.getUserMedia({ audio: preferredConstraint });
        } catch (error) {
          const name = error instanceof DOMException ? error.name : '';
          const shouldRetryWithDefaultDevice =
            name === 'OverconstrainedError' || name === 'NotFoundError';
          if (shouldRetryWithDefaultDevice) {
            try {
              stream = await navigator.mediaDevices.getUserMedia({ audio: true });
            } catch (retryErr) {
              const retryName = retryErr instanceof DOMException ? retryErr.name : '';
              if (retryName === 'NotAllowedError' || retryName === 'PermissionDeniedError') {
                micStatusMessage =
                  'Microphone permission was denied. Joined as listener until mic access is allowed.';
              } else if (retryName === 'NotFoundError') {
                micStatusMessage = 'No microphone was found on this device. Joined as listener.';
              } else {
                micStatusMessage = 'Unable to access microphone. Joined as listener.';
              }
              stream = null;
            }
          } else if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
            micStatusMessage =
              'Microphone permission was denied. Joined as listener until mic access is allowed.';
            stream = null;
          } else {
            micStatusMessage = 'Unable to access microphone. Joined as listener.';
            stream = null;
          }
        }
      }

      pendingVoiceRef.current = { channelId, channelName, stream };
      clearVoiceEngineEvents();
      if (wsRef.current?.readyState === WebSocket.OPEN && wsReady) {
        sendWs({ type: 'join_voice', channel_id: channelId });
      }
      return micStatusMessage;
    },
    [
      me?.id,
      voiceSession,
      sendWs,
      preferredInputDeviceId,
      resolvePreferredInputDeviceId,
      wsReady,
    ],
  );

  const leaveVoice = useCallback(() => {
    if (voiceSession) {
      sendWs({ type: 'leave_voice', channel_id: voiceSession.channelId });
      voiceSession.localStream?.getTracks().forEach((t) => t.stop());
      setVoiceSpeaking((prev) => {
        if (!(voiceSession.channelId in prev)) return prev;
        const next = { ...prev };
        delete next[voiceSession.channelId];
        return next;
      });
      setVoicePeerConnectionStates((prev) => {
        if (!(voiceSession.channelId in prev)) return prev;
        const next = { ...prev };
        delete next[voiceSession.channelId];
        return next;
      });
      setVoiceSession(null);
      clearVoiceEngineEvents();
      clearPersistedVoiceSession();
    }
    if (pendingVoiceRef.current) {
      sendWs({ type: 'leave_voice', channel_id: pendingVoiceRef.current.channelId });
      pendingVoiceRef.current.stream?.getTracks().forEach((t) => t.stop());
      pendingVoiceRef.current = null;
      clearVoiceEngineEvents();
      clearPersistedVoiceSession();
    }
  }, [voiceSession, sendWs, clearVoiceEngineEvents]);

  const toggleMute = useCallback(() => {
    setVoiceSession((prev) => {
      if (!prev || !prev.localStream) return prev;
      const track = prev.localStream.getAudioTracks()[0];
      // If currently muted (enabled=false) → unmute (enabled=true), and vice versa
      if (track) track.enabled = prev.muted;
      return { ...prev, muted: !prev.muted };
    });
  }, []);

  const toggleDeafen = useCallback(() => {
    setVoiceSession((prev) => {
      if (!prev) return prev;
      return { ...prev, deafened: !prev.deafened };
    });
  }, []);

  const setRemoteVolume = useCallback((userId: string, volume: number) => {
    const clamped = Number.isFinite(volume) ? Math.min(2, Math.max(0, volume)) : 1;
    const rounded = Math.round(clamped * 100) / 100;
    setRemoteVolumes((prev) => {
      if (prev[userId] === rounded) return prev;
      return {
        ...prev,
        [userId]: rounded,
      };
    });
  }, []);

  const setLocalMicGain = useCallback((volume: number) => {
    const clamped = Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 1;
    const rounded = Math.round(clamped * 100) / 100;
    setLocalMicGainState((prev) => (prev === rounded ? prev : rounded));
  }, []);

  const setPreferredAudioDevices = useCallback((inputDeviceId: string | null, outputDeviceId: string | null) => {
    const normalizedInput = inputDeviceId && inputDeviceId.trim() ? inputDeviceId.trim() : null;
    const normalizedOutput = outputDeviceId && outputDeviceId.trim() ? outputDeviceId.trim() : null;
    setPreferredInputDeviceId(normalizedInput);
    setPreferredOutputDeviceId(normalizedOutput);
    saveAudioDevicePrefs(normalizedInput, normalizedOutput);
  }, []);

  const setVoiceTranscriptionState = useCallback(
    (channelId: string, next: VoiceTranscriptionState | null) => {
      setVoiceTranscriptions((prev) => {
        if (!next) {
          if (!(channelId in prev)) return prev;
          const copy = { ...prev };
          delete copy[channelId];
          return copy;
        }
        return {
          ...prev,
          [channelId]: next,
        };
      });
    },
    [],
  );

  const uploadTranscriptionRecordingForSession = useCallback(
    async (channelId: string, payload: VoiceTranscriptionRecordingUpload) => {
      const maxAttempts = 5;
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        try {
          await uploadVoiceTranscriptionRecording(channelId, payload);
          return;
        } catch (err) {
          const message =
            err instanceof Error ? err.message.toLowerCase() : '';
          const retryableCapacityError =
            message.includes('too many requests') ||
            message.includes('capacity') ||
            message.includes('retry');
          const shouldRetry =
            attempt < maxAttempts && (retryableCapacityError || attempt <= 2);
          if (shouldRetry) {
            const backoffMs = 200 * attempt;
            await new Promise((resolve) => setTimeout(resolve, backoffMs));
            continue;
          }
          console.warn(
            'Voice transcription recording upload failed',
            {
              channelId,
              sessionId: payload.sessionId,
              startedTsMs: payload.captureStartedTsMs,
              endedTsMs: payload.captureEndedTsMs,
              sizeBytes: payload.blob.size,
            },
            err,
          );
        }
      }
    },
    [],
  );

  const uploadTranscriptionTextForSession = useCallback(
    async (channelId: string, payload: VoiceTranscriptionTextUpload) => {
      const maxAttempts = 5;
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        try {
          await uploadVoiceTranscriptionText(channelId, payload);
          return;
        } catch (err) {
          const message =
            err instanceof Error ? err.message.toLowerCase() : '';
          const retryableCapacityError =
            message.includes('too many requests') ||
            message.includes('capacity') ||
            message.includes('retry');
          const shouldRetry =
            attempt < maxAttempts && (retryableCapacityError || attempt <= 2);
          if (shouldRetry) {
            const backoffMs = 200 * attempt;
            await new Promise((resolve) => setTimeout(resolve, backoffMs));
            continue;
          }
          console.warn(
            'Voice transcription text upload failed',
            {
              channelId,
              sessionId: payload.sessionId,
              startedTsMs: payload.startedTsMs,
              endedTsMs: payload.endedTsMs,
              textLength: payload.text.length,
            },
            err,
          );
        }
      }
    },
    [],
  );

  const handleSpeakingChange = useCallback(
    (channelId: string, userId: string, speaking: boolean) => {
      setVoiceSpeaking((prev) => {
        const channelSpeaking = prev[channelId] ?? [];
        const alreadySpeaking = channelSpeaking.includes(userId);
        if (speaking && alreadySpeaking) return prev;
        if (!speaking && !alreadySpeaking) return prev;

        const next = { ...prev };
        if (speaking) {
          next[channelId] = [...channelSpeaking, userId];
        } else {
          const filtered = channelSpeaking.filter((id) => id !== userId);
          if (filtered.length === 0) {
            delete next[channelId];
          } else {
            next[channelId] = filtered;
          }
        }
        return next;
      });
    },
    [],
  );

  // Per-peer WebRTC connection state from VoiceEngine. A null state means the
  // peer is gone (drop the entry). Mirrors the nested-record shape of
  // voiceSpeaking so the UI can show a reconnecting/failed indicator.
  const handlePeerConnectionStateChange = useCallback(
    (channelId: string, userId: string, state: PeerConnectionUiState | null) => {
      setVoicePeerConnectionStates((prev) => {
        const channelStates = prev[channelId];
        if (state === null) {
          if (!channelStates || !(userId in channelStates)) return prev;
          const nextChannel = { ...channelStates };
          delete nextChannel[userId];
          const next = { ...prev };
          if (Object.keys(nextChannel).length === 0) {
            delete next[channelId];
          } else {
            next[channelId] = nextChannel;
          }
          return next;
        }
        if (channelStates?.[userId] === state) return prev;
        return {
          ...prev,
          [channelId]: {
            ...(channelStates ?? {}),
            [userId]: state,
          },
        };
      });
    },
    [],
  );

  // ── Context value ────────────────────────────────────────────────────────────

  const value: ChannelsContextValue = {
    wsReady,
    sendWs,
    channels,
    voicePresence,
    connectedVoiceChannelId,
    connectedVoiceChannelName,
    hasLocalVoiceSession,
    voiceActiveSince,
    voiceSpeaking,
    voicePeerConnectionStates,
    voiceTranscriptions,
    remoteVolumes,
    localMicGain,
    preferredInputDeviceId,
    preferredOutputDeviceId,
    newMessages,
    lastWsEvent,
    unreadByChannel,
    recordChannelSeq,
    markTextChannelRead,
    voiceSession,
    joinVoice,
    leaveVoice,
    toggleMute,
    toggleDeafen,
    setRemoteVolume,
    setLocalMicGain,
    setPreferredAudioDevices,
    setVoiceTranscriptionState,
    uploadTranscriptionRecording: uploadTranscriptionRecordingForSession,
    uploadTranscriptionText: uploadTranscriptionTextForSession,
  };

  return (
    <ChannelsContext.Provider value={value}>
      {children}
      {/* VoiceEngine is headless — keeps peer connections alive across navigation */}
      {voiceSession && me && (
        <VoiceEngine
          key={voiceSession.channelId}
          localStream={voiceSession.localStream}
          channelId={voiceSession.channelId}
          currentUserId={me.id}
          existingMembers={voiceSession.existingMembers}
          wsEvents={voiceEngineEvents}
          sendWs={sendWs}
          iceServers={voiceIceServers}
          deafened={voiceSession.deafened}
          remoteVolumes={remoteVolumes}
          localMicGain={localMicGain}
          preferredOutputDeviceId={preferredOutputDeviceId}
          onSpeakingChange={handleSpeakingChange}
          onPeerConnectionStateChange={handlePeerConnectionStateChange}
          transcriptionState={voiceTranscriptions[voiceSession.channelId] ?? null}
          onTranscriptionRecordingUpload={uploadTranscriptionRecordingForSession}
          onTranscriptionTextUpload={uploadTranscriptionTextForSession}
        />
      )}
    </ChannelsContext.Provider>
  );
}
