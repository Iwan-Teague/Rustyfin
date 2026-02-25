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
import VoiceBar from '@/app/channels/components/VoiceBar';

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
  newMessages: ChannelMessage[];
  lastWsEvent: ChannelEvent | null;
  voiceSession: VoiceSession | null;
  joinVoice: (channelId: string, channelName: string) => Promise<string | null>;
  leaveVoice: () => void;
  toggleMute: () => void;
  toggleDeafen: () => void;
  setRemoteVolume: (userId: string, volume: number) => void;
  setLocalMicGain: (volume: number) => void;
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
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttemptsRef = useRef(0);

  // Keep voiceSessionRef in sync so WS callbacks always see the latest session
  useEffect(() => {
    voiceSessionRef.current = voiceSession;
  }, [voiceSession]);

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
          setVoicePresence((prev) => {
            const current = prev[event.channel_id] ?? [];
            if (event.joined) {
              if (current.find((u) => u.user_id === event.user_id)) return prev;
              return {
                ...prev,
                [event.channel_id]: [
                  ...current,
                  { user_id: event.user_id, username: event.username },
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
  }, [me, authLoading]);

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
          stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        } catch (error) {
          const name = error instanceof DOMException ? error.name : '';
          if (name === 'NotAllowedError' || name === 'PermissionDeniedError') {
            micStatusMessage =
              'Microphone permission was denied. Joined as listener until mic access is allowed.';
          } else if (name === 'NotFoundError') {
            micStatusMessage = 'No microphone was found on this device. Joined as listener.';
          } else {
            micStatusMessage = 'Unable to access microphone. Joined as listener.';
          }
          stream = null;
        }
      }

      pendingVoiceRef.current = { channelId, channelName, stream };
      sendWs({ type: 'join_voice', channel_id: channelId });
      return micStatusMessage;
    },
    [voiceSession, sendWs],
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
    const clamped = Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 1;
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
      try {
        await uploadVoiceTranscriptionChunk(channelId, payload);
      } catch (err) {
        console.warn('Voice transcription chunk upload failed', err);
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
    newMessages,
    lastWsEvent,
    voiceSession,
    joinVoice,
    leaveVoice,
    toggleMute,
    toggleDeafen,
    setRemoteVolume,
    setLocalMicGain,
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
          onSpeakingChange={handleSpeakingChange}
          transcriptionState={voiceTranscriptions[voiceSession.channelId] ?? null}
          onTranscriptionChunk={sendTranscriptionChunk}
        />
      )}
      {/* Floating bar shown on every page while in a voice channel */}
      {voiceSession && (
        <VoiceBar
          channelName={voiceSession.channelName}
          muted={voiceSession.muted}
          deafened={voiceSession.deafened}
          hasLocalStream={voiceSession.localStream !== null}
          onToggleMute={toggleMute}
          onToggleDeafen={toggleDeafen}
          onLeave={leaveVoice}
        />
      )}
    </ChannelsContext.Provider>
  );
}
