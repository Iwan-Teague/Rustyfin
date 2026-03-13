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
import { uploadVoiceTranscriptionChunk } from './channelsApi';
import type {
  ChannelEvent,
  ChannelInfo,
  ChannelMessage,
  UserInfo,
  VoiceTranscribeChunkRequest,
  VoiceTranscriptionState,
} from './channelsApi';
import VoiceEngine from '@/app/channels/components/VoiceEngine';

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

export interface ChannelsContextValue {
  wsReady: boolean;
  sendWs: (msg: object) => void;
  channels: ChannelInfo[];
  voicePresence: Record<string, UserInfo[]>;
  voiceActiveSince: Record<string, number>;
  voiceSpeaking: Record<string, string[]>;
  voiceTranscriptions: Record<string, VoiceTranscriptionState>;
  remoteVolumes: Record<string, number>;
  localMicGain: number;
  preferredInputDeviceId: string | null;
  preferredOutputDeviceId: string | null;
  newMessages: ChannelMessage[];
  lastWsEvent: ChannelEvent | null;
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
  sendTranscriptionChunk: (
    channelId: string,
    payload: VoiceTranscribeChunkRequest,
  ) => Promise<void>;
}

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

// ── Provider ──────────────────────────────────────────────────────────────────

export function ChannelsProvider({ children }: { children: React.ReactNode }) {
  const { me, loading: authLoading } = useAuth();

  const [wsReady, setWsReady] = useState(false);
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [voicePresence, setVoicePresence] = useState<Record<string, UserInfo[]>>({});
  const [voiceActiveSince, setVoiceActiveSince] = useState<Record<string, number>>({});
  const [voiceSpeaking, setVoiceSpeaking] = useState<Record<string, string[]>>({});
  const [voiceTranscriptions, setVoiceTranscriptions] = useState<
    Record<string, VoiceTranscriptionState>
  >({});
  const [remoteVolumes, setRemoteVolumes] = useState<Record<string, number>>({});
  const [localMicGain, setLocalMicGainState] = useState(1);
  const [preferredInputDeviceId, setPreferredInputDeviceId] = useState<string | null>(null);
  const [preferredOutputDeviceId, setPreferredOutputDeviceId] = useState<string | null>(null);
  const [newMessages, setNewMessages] = useState<ChannelMessage[]>([]);
  const [lastWsEvent, setLastWsEvent] = useState<ChannelEvent | null>(null);
  const [voiceSession, setVoiceSession] = useState<VoiceSession | null>(null);

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
      setVoiceTranscriptions({});
      setRemoteVolumes({});
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

      const token = localStorage.getItem('token');
      if (!token) return;

      const ws = new WebSocket(getWsUrl());
      wsRef.current = ws;

      ws.onopen = () => {
        const t = localStorage.getItem('token');
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

        if (event.type === 'hello') {
          setChannels(event.channels);
          setVoicePresence(event.voice_presence);
          setVoiceActiveSince(event.voice_active_since_ts ?? {});
          setVoiceTranscriptions(event.voice_transcriptions ?? {});
          setVoiceSpeaking({});
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
            },
          ]);
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

      // Leave any existing session first
      if (pendingVoiceRef.current || voiceSession) {
        const previousChannelId =
          voiceSession?.channelId ?? pendingVoiceRef.current?.channelId ?? null;
        sendWs({ type: 'leave_voice', channel_id: voiceSession?.channelId ?? pendingVoiceRef.current?.channelId });
        voiceSession?.localStream?.getTracks().forEach((t) => t.stop());
        pendingVoiceRef.current?.stream?.getTracks().forEach((t) => t.stop());
        pendingVoiceRef.current = null;
        if (previousChannelId) {
          setVoiceSpeaking((prev) => {
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
      if (wsRef.current?.readyState === WebSocket.OPEN && wsReady) {
        sendWs({ type: 'join_voice', channel_id: channelId });
      }
      return micStatusMessage;
    },
    [voiceSession, sendWs, preferredInputDeviceId, resolvePreferredInputDeviceId, wsReady],
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
      setVoiceSession(null);
      clearPersistedVoiceSession();
    }
    if (pendingVoiceRef.current) {
      sendWs({ type: 'leave_voice', channel_id: pendingVoiceRef.current.channelId });
      pendingVoiceRef.current.stream?.getTracks().forEach((t) => t.stop());
      pendingVoiceRef.current = null;
      clearPersistedVoiceSession();
    }
  }, [voiceSession, sendWs]);

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

  const sendTranscriptionChunk = useCallback(
    async (channelId: string, payload: VoiceTranscribeChunkRequest) => {
      const maxAttempts = 5;
      for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
        try {
          await uploadVoiceTranscriptionChunk(channelId, payload);
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
            'Voice transcription chunk upload failed',
            {
              channelId,
              sessionId: payload.session_id,
              sampleRateHz: payload.sample_rate_hz,
              startedTsMs: payload.started_ts_ms,
              endedTsMs: payload.ended_ts_ms,
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

  // ── Context value ────────────────────────────────────────────────────────────

  const value: ChannelsContextValue = {
    wsReady,
    sendWs,
    channels,
    voicePresence,
    voiceActiveSince,
    voiceSpeaking,
    voiceTranscriptions,
    remoteVolumes,
    localMicGain,
    preferredInputDeviceId,
    preferredOutputDeviceId,
    newMessages,
    lastWsEvent,
    voiceSession,
    joinVoice,
    leaveVoice,
    toggleMute,
    toggleDeafen,
    setRemoteVolume,
    setLocalMicGain,
    setPreferredAudioDevices,
    setVoiceTranscriptionState,
    sendTranscriptionChunk,
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
          wsEvents={lastWsEvent}
          sendWs={sendWs}
          deafened={voiceSession.deafened}
          remoteVolumes={remoteVolumes}
          localMicGain={localMicGain}
          preferredOutputDeviceId={preferredOutputDeviceId}
          onSpeakingChange={handleSpeakingChange}
          transcriptionState={voiceTranscriptions[voiceSession.channelId] ?? null}
          onTranscriptionChunk={sendTranscriptionChunk}
        />
      )}
    </ChannelsContext.Provider>
  );
}
