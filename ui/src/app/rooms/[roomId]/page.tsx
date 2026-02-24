'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useRouter } from 'next/navigation';
import { apiFetch, apiJson } from '@/lib/api';
import { useAuth } from '@/lib/auth';
import {
  ReconfigureWatchPartyRoomRequest,
  WatchPartyRoomResponse,
  WatchPartyUser,
  WsAudioStateMessage,
  WsRoomReconfiguredMessage,
  WsYouTubeStateMessage,
  endWatchPartyRoom,
  getEligibleLibraries,
  getWatchPartyRoom,
  inviteToRoom,
  joinWatchPartyRoom,
  leaveWatchPartyRoom,
  listWatchPartyUsers,
  reconfigureWatchPartyRoom,
} from '@/lib/watchPartyApi';
import { formatElapsedSeconds } from '@/lib/time';
import { nonAdminRoleLabel, roleLabel } from '@/lib/watchPartyRoles';
import AudioPlayer from '../components/AudioPlayer';
import MediaPicker, { MediaItemNode, MediaLibrary } from '../components/MediaPicker';
import YouTubePlayer from '../components/YouTubePlayer';

type PlaybackDescriptor = {
  item_id: string;
  file_id: string;
  direct_url: string;
  hls_start_url: string;
  media_info_url: string;
};

type PlaybackSession = {
  session_id: string;
  hls_url: string;
};

type StartPlaybackOptions = {
  autoplayWhenNoState?: boolean;
  silent?: boolean;
};

type WsPresenceMember = {
  user_id: string;
  username: string;
  role: string;
  connected: boolean;
};

type WsStateMessage = {
  type: 'state';
  room_id: string;
  item_id: string;
  playing: boolean;
  position_ms: number;
  updated_ts_ms: number;
  server_ts_ms: number;
  members: WsPresenceMember[];
};

type WsPresenceMessage = {
  type: 'presence';
  user_id: string;
  connected: boolean;
};

type WsErrorMessage = {
  type: 'error';
  message: string;
};

type WsPongMessage = {
  type: 'pong';
};

type WsRoomEndedMessage = {
  type: 'room_ended';
};

type RoomMode = 'video' | 'audio' | 'youtube';

type WsMessage =
  | WsStateMessage
  | WsAudioStateMessage
  | WsYouTubeStateMessage
  | WsRoomReconfiguredMessage
  | WsPresenceMessage
  | WsErrorMessage
  | WsPongMessage
  | WsRoomEndedMessage;

type RuntimeConfig = {
  backend_origin?: string | null;
};

function wsUrlForOrigin(origin: string, roomId: string): string {
  const url = new URL(origin);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.pathname = `/api/v1/watch-party/rooms/${roomId}/ws`;
  url.search = '';
  url.hash = '';
  return url.toString();
}

async function waitForVideoMetadata(
  video: HTMLVideoElement,
  timeoutMs = 5000,
): Promise<void> {
  if (video.readyState >= 1) return;

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      video.removeEventListener('loadedmetadata', finish);
      resolve();
    };

    video.addEventListener('loadedmetadata', finish);
    window.setTimeout(finish, timeoutMs);
  });
}

export default function WatchPartyRoomPage() {
  const params = useParams();
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const roomId = params.roomId as string;

  const [room, setRoom] = useState<WatchPartyRoomResponse | null>(null);
  const [roomState, setRoomState] = useState<WsStateMessage | null>(null);
  const [audioState, setAudioState] = useState<WsAudioStateMessage | null>(null);
  const [youtubeState, setYoutubeState] = useState<WsYouTubeStateMessage | null>(null);
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [joinPassword, setJoinPassword] = useState('');
  const [joinedRole, setJoinedRole] = useState<string | null>(null);

  const [loadingRoom, setLoadingRoom] = useState(true);
  const [joining, setJoining] = useState(false);
  const [startingDirect, setStartingDirect] = useState(false);
  const [startingHls, setStartingHls] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [ending, setEnding] = useState(false);

  // In-room invite state
  const [allUsers, setAllUsers] = useState<WatchPartyUser[]>([]);
  const [inviteSelections, setInviteSelections] = useState<Record<string, 'viewer' | 'controller'>>({});
  const [sendingInvites, setSendingInvites] = useState(false);

  const [wsConnected, setWsConnected] = useState(false);
  const [wsEpoch, setWsEpoch] = useState(0);
  const [error, setError] = useState('');
  const [info, setInfo] = useState('');
  const [mode, setMode] = useState<'direct' | 'hls'>('direct');
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [debugLog, setDebugLog] = useState<string[]>([]);
  const [allLibraries, setAllLibraries] = useState<MediaLibrary[]>([]);
  const [eligibleLibraryIds, setEligibleLibraryIds] = useState<string[]>([]);
  const [reconfigureMode, setReconfigureMode] = useState<RoomMode>('video');
  const [reconfigureWatchSource, setReconfigureWatchSource] = useState<'video' | 'youtube'>('video');
  const [reconfigureVideoLibraryId, setReconfigureVideoLibraryId] = useState('');
  const [reconfigureVideoItem, setReconfigureVideoItem] = useState<MediaItemNode | null>(null);
  const [reconfigureAudioLibraryId, setReconfigureAudioLibraryId] = useState('');
  const [reconfigureDirty, setReconfigureDirty] = useState(false);
  const [reconfiguring, setReconfiguring] = useState(false);
  const [roomPanelExpanded, setRoomPanelExpanded] = useState(false);

  const wsRef = useRef<WebSocket | null>(null);
  const hlsRef = useRef<any>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const sessionIdRef = useRef<string | null>(null);
  const applyingRemoteRef = useRef(false);
  const autoPreloadedItemRef = useRef<string | null>(null);
  const infoTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearInfoTimeout = useCallback(() => {
    if (infoTimeoutRef.current) {
      clearTimeout(infoTimeoutRef.current);
      infoTimeoutRef.current = null;
    }
  }, []);

  const setInfoForDuration = useCallback((message: string, durationMs: number) => {
    clearInfoTimeout();
    setInfo(message);
    infoTimeoutRef.current = setTimeout(() => {
      setInfo((current) => (current === message ? '' : current));
      infoTimeoutRef.current = null;
    }, durationMs);
  }, [clearInfoTimeout]);

  const isAudioRoom = room?.room_mode === 'audio';
  const isYoutubeRoom = room?.room_mode === 'youtube';
  const effectiveRoomMode = room?.room_mode ?? 'video';
  const memberRoleDisplay = nonAdminRoleLabel(effectiveRoomMode);
  const joinedRoleDisplay = joinedRole ? roleLabel(joinedRole, effectiveRoomMode) : '';

  const myMember = useMemo(() => {
    if (!room || !me) return null;
    return room.members.find((member) => member.user_id === me.id) || null;
  }, [room, me]);

  const canPlayPause = useMemo(() => {
    if (!room || !joinedRole) return false;
    if (joinedRole === 'host') return true;
    return room.policy.allow_non_host_play_pause;
  }, [room, joinedRole]);

  const canSeek = useMemo(() => {
    if (!room || !joinedRole) return false;
    if (joinedRole === 'host') return true;
    return room.policy.allow_non_host_seek;
  }, [room, joinedRole]);

  const controlsEnabled = canPlayPause || canSeek || joinedRole === 'host';

  const reconfigureVideoLibraries = useMemo(
    () =>
      allLibraries.filter(
        (library) => library.kind !== 'music' && eligibleLibraryIds.includes(library.id),
      ),
    [allLibraries, eligibleLibraryIds],
  );

  const reconfigureMusicLibraries = useMemo(
    () =>
      allLibraries.filter(
        (library) => library.kind === 'music' && eligibleLibraryIds.includes(library.id),
      ),
    [allLibraries, eligibleLibraryIds],
  );

  const roomDurationSeconds = useMemo(() => {
    if (!room) return 0;
    const endTs = room.ended_ts ?? Math.floor(nowMs / 1000);
    return Math.max(0, endTs - room.created_ts);
  }, [room, nowMs]);

  const activeMembers = (roomState?.members ?? audioState?.members ?? youtubeState?.members) ?? room?.members.map((member) => ({
    user_id: member.user_id,
    username: member.username,
    role: member.role,
    connected: member.status === 'joined',
  })) ?? [];

  const invitableUsers = useMemo(() => {
    if (!me) return [];
    const memberIds = new Set(activeMembers.map((member) => member.user_id));
    return allUsers.filter((user) => user.id !== me.id && !memberIds.has(user.id));
  }, [allUsers, activeMembers, me]);

  const appendDebug = useCallback((message: string) => {
    const line = `${new Date().toISOString()} ${message}`;
    setDebugLog((prev) => [...prev.slice(-199), line]);
    if (typeof window !== 'undefined') {
      console.info(`[watch-party:${roomId}] ${message}`);
    }
  }, [roomId]);

  const handleYoutubeDebug = useCallback((message: string) => {
    appendDebug(`youtube ${message}`);
  }, [appendDebug]);

  const sendWs = useCallback((payload: Record<string, unknown>): boolean => {
    const socket = wsRef.current;
    const msgType = typeof payload.type === 'string' ? payload.type : 'unknown';
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      setWsConnected(false);
      setError('Watch-party realtime connection is offline. Reconnect and retry.');
      appendDebug(`ws send rejected type=${msgType} reason=socket_not_open`);
      return false;
    }
    try {
      socket.send(JSON.stringify(payload));
      appendDebug(`ws send type=${msgType}`);
      return true;
    } catch {
      setWsConnected(false);
      setError('Failed to send watch-party command over websocket.');
      appendDebug(`ws send failed type=${msgType} reason=send_exception`);
      return false;
    }
  }, [appendDebug]);

  useEffect(() => () => {
    clearInfoTimeout();
  }, [clearInfoTimeout]);

  const destroyHls = useCallback(() => {
    if (hlsRef.current) {
      try {
        hlsRef.current.destroy();
      } catch {
        // no-op
      }
      hlsRef.current = null;
    }
  }, []);

  const stopSession = useCallback(async (sessionId: string) => {
    await apiFetch(`/playback/sessions/${sessionId}/stop`, { method: 'POST' }).catch(() => {});
  }, []);

  const loadRoom = useCallback(async () => {
    setLoadingRoom(true);
    setError('');
    try {
      const data = await getWatchPartyRoom(roomId);
      setRoom(data);
      appendDebug(
        `room loaded mode=${data.room_mode} status=${data.status} members=${data.members.length} password_required=${data.password_required}`,
      );
      if (me) {
        const current = data.members.find((member) => member.user_id === me.id);
        setJoinedRole(current?.status === 'joined' ? current.role : null);
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to load watch party room');
      appendDebug(`room load failed error=${String(err?.message || err)}`);
    } finally {
      setLoadingRoom(false);
    }
  }, [roomId, me, appendDebug]);

  const refreshRoom = useCallback(async () => {
    try {
      const data = await getWatchPartyRoom(roomId);
      setRoom(data);
      if (me) {
        const current = data.members.find((member) => member.user_id === me.id);
        setJoinedRole(current?.status === 'joined' ? current.role : null);
      }
    } catch {
      // Non-fatal background refresh.
    }
  }, [roomId, me]);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!me) return;
    void loadRoom();
  }, [me, loadRoom]);

  useEffect(() => {
    if (!me) return;
    listWatchPartyUsers().then(setAllUsers).catch(() => {});
  }, [me]);

  useEffect(() => {
    if (!room) return;
    const mode = room.room_mode === 'audio' || room.room_mode === 'youtube' ? room.room_mode : 'video';
    if (!reconfigureDirty) {
      setReconfigureMode(mode);
      if (mode === 'audio') {
        // Keep last watch source when switching away from watch-together mode.
        return;
      }
      setReconfigureWatchSource(mode);
    }
  }, [room?.room_mode, reconfigureDirty]);

  useEffect(() => {
    if (!room || !me || joinedRole !== 'host') return;

    let cancelled = false;

    (async () => {
      try {
        const libraries = await apiJson<MediaLibrary[]>('/libraries');
        if (cancelled) return;
        setAllLibraries(libraries);

        const participantIds = room.members
          .filter((member) => member.user_id !== me.id && member.status !== 'left' && member.status !== 'declined')
          .map((member) => member.user_id);
        const eligible = await getEligibleLibraries(participantIds);
        if (cancelled) return;
        setEligibleLibraryIds(eligible);

        if (!eligible.includes(reconfigureVideoLibraryId)) {
          const defaultVideoLibrary = libraries.find(
            (library) => library.kind !== 'music' && eligible.includes(library.id),
          );
          setReconfigureVideoLibraryId(defaultVideoLibrary?.id ?? '');
          setReconfigureVideoItem(null);
        }

        if (!eligible.includes(reconfigureAudioLibraryId)) {
          const defaultAudioLibrary = libraries.find(
            (library) => library.kind === 'music' && eligible.includes(library.id),
          );
          setReconfigureAudioLibraryId(defaultAudioLibrary?.id ?? '');
        }
      } catch {
        // Non-fatal; panel can still render with current state.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [room, me, joinedRole, reconfigureVideoLibraryId, reconfigureAudioLibraryId]);

  useEffect(() => {
    if (!room || room.status === 'ended') return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [room]);

  useEffect(() => {
    if (!joinedRole) return;
    const id = window.setInterval(() => {
      void refreshRoom();
    }, 5000);
    return () => window.clearInterval(id);
  }, [joinedRole, refreshRoom]);

  useEffect(() => {
    if (!room || !joinedRole || isAudioRoom || isYoutubeRoom) return;

    let cancelled = false;

    apiJson<PlaybackDescriptor>(`/items/${room.item_id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
      })
      .catch((err: any) => {
        if (!cancelled) {
          setError(err?.message || 'Failed to load playback descriptor');
        }
      });

    return () => {
      cancelled = true;
    };
  }, [room, joinedRole, isAudioRoom, isYoutubeRoom]);

  const applyRemoteState = useCallback(async (stateMessage: WsStateMessage) => {
    const video = videoRef.current;
    if (!video) return;
    if (!video.currentSrc && !video.src) return;

    applyingRemoteRef.current = true;

    const targetSeconds = stateMessage.position_ms / 1000;
    if (Math.abs(video.currentTime - targetSeconds) > 1.2) {
      video.currentTime = targetSeconds;
    }

    if (stateMessage.playing && video.paused) {
      await video.play().catch(() => {});
    }

    if (!stateMessage.playing && !video.paused) {
      video.pause();
    }

    window.setTimeout(() => {
      applyingRemoteRef.current = false;
    }, 60);
  }, []);

  useEffect(() => {
    if (!joinedRole) return;

    let cancelled = false;
    let activeSocket: WebSocket | null = null;
    let reconnectTimer: number | null = null;

    const token = localStorage.getItem('token');
    if (!token) {
      appendDebug('ws connect aborted: missing auth token in localStorage');
      return;
    }

    const bindOpenSocket = (socket: WebSocket, candidateIndex: number) => {
      activeSocket = socket;
      wsRef.current = socket;
      setWsConnected(true);
      appendDebug(
        `ws connected candidate_index=${candidateIndex} url=${socket.url || 'unknown_url'}`,
      );

      if (candidateIndex > 0) {
        setInfo('Connected to watch-party websocket via backend fallback.');
      }

      try {
        socket.send(JSON.stringify({ type: 'auth', token }));
        appendDebug('ws auth frame sent');
      } catch {
        appendDebug('ws auth frame send failed');
        setWsConnected(false);
        setError('Failed to authenticate watch-party websocket connection.');
        return;
      }

      socket.onmessage = (event) => {
        try {
          const payload = JSON.parse(event.data) as WsMessage;
          appendDebug(`ws recv type=${payload.type}`);

          if (payload.type === 'state') {
            setRoomState(payload);
            void applyRemoteState(payload);
          } else if (payload.type === 'audio_state') {
            setAudioState(payload);
            // Update member list in roomState-like fashion
            setRoomState((prev) => {
              if (!prev) return prev;
              return { ...prev, members: payload.members };
            });
          } else if (payload.type === 'youtube_state' || payload.type === 'you_tube_state') {
            appendDebug(
              `youtube state video_id=${payload.video_id || 'none'} playing=${payload.playing} position_ms=${payload.position_ms}`,
            );
            setYoutubeState({
              ...payload,
              type: 'youtube_state',
            });
          } else if (payload.type === 'room_reconfigured') {
            appendDebug(
              `room reconfigured mode=${payload.room_mode} item_id=${payload.item_id || 'none'} audio_library_id=${payload.audio_library_id || 'none'} youtube_video_id=${payload.youtube_video_id || 'none'}`,
            );
            setInfo('Room configuration changed. Refreshing room mode…');
            setReconfigureDirty(false);
            setReconfigureVideoItem(null);
            setDescriptor(null);
            setRoomState(null);
            setAudioState(null);
            setYoutubeState(null);
            destroyHls();
            if (sessionIdRef.current) {
              void stopSession(sessionIdRef.current);
              sessionIdRef.current = null;
            }
            void loadRoom();
            setWsEpoch((prev) => prev + 1);
          } else if (payload.type === 'presence') {
            setRoomState((prev) => {
              if (!prev) return prev;
              return {
                ...prev,
                members: prev.members.map((member) =>
                  member.user_id === payload.user_id
                    ? { ...member, connected: payload.connected }
                    : member,
                ),
              };
            });
            setAudioState((prev) => {
              if (!prev) return prev;
              return {
                ...prev,
                members: prev.members.map((member) =>
                  member.user_id === payload.user_id
                    ? { ...member, connected: payload.connected }
                    : member,
                ),
              };
            });
            setYoutubeState((prev) => {
              if (!prev) return prev;
              return {
                ...prev,
                members: prev.members.map((member) =>
                  member.user_id === payload.user_id
                    ? { ...member, connected: payload.connected }
                    : member,
                ),
              };
            });
          } else if (payload.type === 'error') {
            setError(payload.message);
            appendDebug(`ws server_error message=${payload.message}`);
            const normalized = payload.message.toLowerCase();
            if (normalized.includes('not joined')) {
              setJoinedRole(null);
              void refreshRoom();
            }
          } else if (payload.type === 'room_ended') {
            appendDebug('ws room ended notification received');
            router.push('/rooms');
          }
        } catch (err) {
          setError('Invalid websocket message received');
          const snippet = String(event.data).slice(0, 180).replace(/\s+/g, ' ');
          appendDebug(
            `ws message parse failed error=${String((err as any)?.message || err)} payload="${snippet}"`,
          );
        }
      };

      socket.onerror = () => {
        appendDebug('ws error event fired');
      };

      socket.onclose = (event) => {
        if (cancelled) return;
        setWsConnected(false);
        appendDebug(
          `ws closed code=${event.code} reason=${event.reason || 'none'} clean=${event.wasClean}`,
        );
        if (joinedRole) {
          reconnectTimer = window.setTimeout(() => {
            if (cancelled) return;
            appendDebug('ws reconnect scheduled after close');
            setWsEpoch((prev) => prev + 1);
          }, 1200);
        }
      };
    };

    const attemptConnect = async () => {
      const candidates: string[] = [wsUrlForOrigin(window.location.origin, roomId)];
      const pageIsSecure = window.location.protocol === 'https:';
      appendDebug(
        `ws connect start role=${joinedRole} page_origin=${window.location.origin} secure=${window.isSecureContext}`,
      );

      try {
        const runtimeConfig = await fetch('/runtime-config', { cache: 'no-store' });
        if (runtimeConfig.ok) {
          const payload = (await runtimeConfig.json()) as RuntimeConfig;
          if (payload.backend_origin) {
            const directBackendWs = wsUrlForOrigin(payload.backend_origin, roomId);
            const insecureFromSecurePage =
              pageIsSecure && directBackendWs.toLowerCase().startsWith('ws://');
            if (insecureFromSecurePage) {
              appendDebug(
                `ws fallback candidate skipped (mixed content blocked on https page): ${directBackendWs}`,
              );
            }
            if (!insecureFromSecurePage && !candidates.includes(directBackendWs)) {
              candidates.push(directBackendWs);
            }
          }
        }
      } catch (err) {
        appendDebug(
          `runtime-config lookup failed error=${String((err as any)?.message || err)}`,
        );
      }

      for (let index = 0; index < candidates.length; index += 1) {
        if (cancelled) return;

        const candidate = candidates[index];
        appendDebug(`ws attempting candidate[${index}] ${candidate}`);
        const socket = await new Promise<WebSocket | null>((resolve) => {
          let settled = false;
          let ws: WebSocket;
          try {
            ws = new WebSocket(candidate);
          } catch {
            appendDebug(`ws candidate threw during constructor: ${candidate}`);
            resolve(null);
            return;
          }

          const finish = (result: WebSocket | null) => {
            if (settled) return;
            settled = true;
            resolve(result);
          };

          ws.onopen = () => finish(ws);
          ws.onerror = () => {
            if (!settled) {
              appendDebug(`ws candidate error before open: ${candidate}`);
            }
          };
          ws.onclose = (event) => {
            if (!settled) {
              appendDebug(
                `ws candidate closed before open code=${event.code} reason=${event.reason || 'none'}`,
              );
            }
            finish(null);
          };

          window.setTimeout(() => {
            if (settled) return;
            try {
              ws.close();
            } catch {
              // no-op
            }
            appendDebug(`ws candidate timeout after 4000ms: ${candidate}`);
            finish(null);
          }, 4_000);
        });

        if (!socket) {
          continue;
        }

        if (cancelled) {
          socket.close();
          return;
        }

        bindOpenSocket(socket, index);
        return;
      }

      if (!cancelled) {
        setWsConnected(false);
        setError(
          'Watch-party websocket connection failed. Restart with ./scripts/start.sh and retry.',
        );
        appendDebug('ws connection failed for all candidates');
        reconnectTimer = window.setTimeout(() => {
          if (cancelled) return;
          appendDebug('ws reconnect scheduled after candidate failure');
          setWsEpoch((prev) => prev + 1);
        }, 1500);
      }
    };

    void attemptConnect();

    return () => {
      cancelled = true;
      if (reconnectTimer) {
        window.clearTimeout(reconnectTimer);
      }
      if (activeSocket) {
        activeSocket.close();
      }
      wsRef.current = null;
      setWsConnected(false);
    };
  }, [
    roomId,
    joinedRole,
    applyRemoteState,
    appendDebug,
    destroyHls,
    loadRoom,
    stopSession,
    wsEpoch,
    refreshRoom,
    router,
  ]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionIdRef.current) {
        void stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }
    };
  }, [destroyHls, stopSession]);

  const startDirect = useCallback(async (options: StartPlaybackOptions = {}): Promise<boolean> => {
    if (!descriptor) {
      if (!options.silent) {
        setError('Playback descriptor is not ready yet');
      }
      return false;
    }

    setStartingDirect(true);
    if (!options.silent) {
      setError('');
    }

    try {
      destroyHls();
      if (sessionIdRef.current) {
        await stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }

      const video = videoRef.current;
      if (!video) throw new Error('Video element is not ready');

      setMode('direct');
      video.src = descriptor.direct_url;
      video.load();
      await waitForVideoMetadata(video);

      if (roomState) {
        await applyRemoteState(roomState);
      } else if (options.autoplayWhenNoState ?? true) {
        await video.play().catch(() => {});
      } else {
        video.pause();
        try {
          if (video.currentTime !== 0) {
            video.currentTime = 0;
          }
        } catch {
          // Some browsers may block programmatic seek before enough data is ready.
        }
      }
      return true;
    } catch (err: any) {
      if (!options.silent) {
        setError(err?.message || 'Failed to start direct playback');
      }
      return false;
    } finally {
      setStartingDirect(false);
    }
  }, [descriptor, destroyHls, stopSession, roomState, applyRemoteState]);

  const startHls = useCallback(async (options: StartPlaybackOptions = {}): Promise<boolean> => {
    if (!descriptor) {
      if (!options.silent) {
        setError('Playback descriptor is not ready yet');
      }
      return false;
    }

    setStartingHls(true);
    if (!options.silent) {
      setError('');
      setInfo('Preparing transcoded stream…');
    }

    try {
      const video = videoRef.current;
      if (!video) throw new Error('Video element is not ready');

      destroyHls();
      if (sessionIdRef.current) {
        await stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }

      const session = await apiJson<PlaybackSession>(descriptor.hls_start_url, {
        method: 'POST',
        body: JSON.stringify({ file_id: descriptor.file_id }),
      });

      sessionIdRef.current = session.session_id;
      setMode('hls');

      if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = session.hls_url;
        video.load();
        await waitForVideoMetadata(video);
        if (roomState) {
          await applyRemoteState(roomState);
        } else if (options.autoplayWhenNoState ?? true) {
          await video.play().catch(() => {});
        } else {
          video.pause();
          try {
            if (video.currentTime !== 0) {
              video.currentTime = 0;
            }
          } catch {
            // Some browsers may block programmatic seek before enough data is ready.
          }
        }
      } else {
        const Hls = (await import('hls.js')).default;
        if (!Hls.isSupported()) {
          throw new Error('HLS is not supported in this browser');
        }
        const hls = new Hls();
        hlsRef.current = hls;
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          if (roomState) {
            void applyRemoteState(roomState);
          } else if (options.autoplayWhenNoState ?? true) {
            void video.play().catch(() => {});
          } else {
            video.pause();
            try {
              if (video.currentTime !== 0) {
                video.currentTime = 0;
              }
            } catch {
              // Some browsers may block programmatic seek before enough data is ready.
            }
          }
        });
        hls.on(Hls.Events.ERROR, (_event: any, data: any) => {
          if (data?.fatal) {
            setError(`HLS playback error: ${data.details || 'fatal stream error'}`);
          }
        });
        hls.attachMedia(video);
        hls.loadSource(session.hls_url);
      }
      return true;
    } catch (err: any) {
      if (!options.silent) {
        setError(err?.message || 'Failed to start HLS playback');
      }
      return false;
    } finally {
      setStartingHls(false);
    }
  }, [descriptor, destroyHls, stopSession, roomState, applyRemoteState]);

  useEffect(() => {
    // Reset one-time preload guard when room item or mode changes.
    autoPreloadedItemRef.current = null;
  }, [room?.item_id, room?.room_mode]);

  useEffect(() => {
    if (!room || !joinedRole || isAudioRoom || isYoutubeRoom || !descriptor) return;
    if (startingDirect || startingHls) return;
    if (autoPreloadedItemRef.current === room.item_id) return;

    autoPreloadedItemRef.current = room.item_id;
    appendDebug(`auto preload requested item_id=${room.item_id} preferred=direct`);

    void (async () => {
      const directOk = await startDirect({
        autoplayWhenNoState: false,
        silent: true,
      });
      if (directOk) {
        appendDebug(`auto preload succeeded mode=direct item_id=${room.item_id}`);
        return;
      }

      appendDebug(`auto preload direct failed; falling back to hls item_id=${room.item_id}`);
      const hlsOk = await startHls({
        autoplayWhenNoState: false,
        silent: true,
      });

      if (hlsOk) {
        setInfo('Direct play could not be preloaded automatically. Using HLS preload.');
      } else {
        setInfo('Automatic preload failed. Use Direct Play or Transcode (HLS).');
      }
    })();
  }, [
    room,
    joinedRole,
    isAudioRoom,
    isYoutubeRoom,
    descriptor,
    startingDirect,
    startingHls,
    startDirect,
    startHls,
    appendDebug,
  ]);

  async function handleJoin() {
    setJoining(true);
    setError('');
    setInfo('');
    appendDebug('room join requested');
    try {
      const result = await joinWatchPartyRoom(roomId, joinPassword || undefined);
      setJoinedRole(result.role);
      setInfo('Joined watch-party room.');
      appendDebug(`room join succeeded role=${result.role}`);
      await loadRoom();
    } catch (err: any) {
      setError(err?.message || 'Failed to join room');
      appendDebug(`room join failed error=${String(err?.message || err)}`);
    } finally {
      setJoining(false);
    }
  }

  async function copyDebugLog() {
    try {
      await navigator.clipboard.writeText(debugLog.join('\n'));
      setInfo('Diagnostics copied to clipboard.');
      appendDebug('diagnostics copied to clipboard');
    } catch {
      setError('Failed to copy diagnostics');
      appendDebug('diagnostics copy failed');
    }
  }

  async function handleLeave() {
    setLeaving(true);
    setError('');
    try {
      await leaveWatchPartyRoom(roomId);
      router.push('/rooms');
    } catch (err: any) {
      setError(err?.message || 'Failed to leave room');
    } finally {
      setLeaving(false);
    }
  }

  async function handleEndRoom() {
    setEnding(true);
    setError('');
    try {
      await endWatchPartyRoom(roomId);
      router.push('/rooms');
    } catch (err: any) {
      setError(err?.message || 'Failed to end room');
      setEnding(false);
    }
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setInfo('Room link copied to clipboard.');
    } catch {
      setError('Failed to copy room link');
    }
  }

  async function handleSendInvites() {
    const payload = Object.entries(inviteSelections).map(([user_id, role]) => ({ user_id, role }));
    if (payload.length === 0) return;
    setSendingInvites(true);
    setError('');
    try {
      const response = await inviteToRoom(roomId, payload);
      setInviteSelections({});
      await refreshRoom();
      if (response.cooldown_blocked_users.length > 0) {
        const blocked = response.cooldown_blocked_users.join(', ');
        setInfo(
          `Invited ${response.invited} user${response.invited === 1 ? '' : 's'}. Cooldown active for: ${blocked}.`,
        );
      } else {
        setInfo(`Invited ${response.invited} user${response.invited === 1 ? '' : 's'}.`);
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to send invites');
    } finally {
      setSendingInvites(false);
    }
  }

  async function handleReconfigureRoom() {
    if (joinedRole !== 'host') {
      setError('Only the room host can reconfigure mode.');
      return;
    }

    let payload: ReconfigureWatchPartyRoomRequest;
    if (reconfigureMode === 'video') {
      if (!reconfigureVideoItem) {
        setError('Select a movie or episode first.');
        return;
      }
      payload = {
        room_mode: 'video',
        item_id: reconfigureVideoItem.id,
      };
    } else if (reconfigureMode === 'audio') {
      if (!reconfigureAudioLibraryId) {
        setError('Select a music library first.');
        return;
      }
      payload = {
        room_mode: 'audio',
        audio_library_id: reconfigureAudioLibraryId,
      };
    } else {
      payload = {
        room_mode: 'youtube',
      };
    }

    setReconfiguring(true);
    setReconfigureDirty(true);
    setError('');
    setInfo('');
    try {
      await reconfigureWatchPartyRoom(roomId, payload);
      setInfoForDuration('Reconfiguring room for all participants…', 10_000);
    } catch (err: any) {
      setError(err?.message || 'Failed to reconfigure room');
    } finally {
      setReconfiguring(false);
    }
  }

  if (authLoading || loadingRoom) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading room...</p>
      </div>
    );
  }

  if (!me) {
    return null;
  }

  if (!room) {
    const normalized = error.toLowerCase();
    let hint = 'This room could not be opened for this account.';
    if (normalized.includes('invite-only')) {
      hint =
        'This room is invite-only for this account. Ask the host to send an invite. A password alone does not bypass invite-only access.';
    } else if (normalized.includes('library access denied')) {
      hint =
        'This account does not have access to the library containing this media. Ask an admin to grant library access.';
    } else if (normalized.includes('not found')) {
      hint = 'This room link is invalid or the room has already ended.';
    }

    return (
      <div className="space-y-4 animate-rise">
        <section className="panel space-y-3 p-6 sm:p-7">
          <span className="chip chip-accent">Watch Party Room</span>
          <h1 className="text-2xl font-semibold sm:text-3xl">Unable to open room</h1>
          <p className="text-sm muted">{error || 'Failed to load watch party room.'}</p>
          <p className="text-sm muted">{hint}</p>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              className="btn-secondary px-4 py-2 text-sm"
              onClick={() => void loadRoom()}
            >
              Retry
            </button>
            <button
              type="button"
              className="btn-primary px-4 py-2 text-sm"
              onClick={() => router.push('/rooms')}
            >
              Back to Watch Party
            </button>
          </div>
        </section>
      </div>
    );
  }

  return (
    <div className="space-y-6 animate-rise">
      <section className="panel p-3 sm:p-4">
        <div
          role="button"
          tabIndex={0}
          aria-expanded={roomPanelExpanded}
          aria-label="Toggle room details"
          className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-white/10 bg-black/10 px-3 py-2"
          onClick={() => setRoomPanelExpanded((prev) => !prev)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault();
              setRoomPanelExpanded((prev) => !prev);
            }
          }}
        >
          <div className="min-w-0">
            <p className="text-sm font-semibold">
              {roomPanelExpanded ? 'Hide room details' : 'Show room details'}
            </p>
            <p className="text-[11px] muted truncate">
              Room {room.room_id}
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <span className="chip text-xs">Duration: {formatElapsedSeconds(roomDurationSeconds)}</span>
            <span className="chip text-xs">WS: {wsConnected ? 'connected' : 'disconnected'}</span>
            <button
              type="button"
              className="btn-secondary px-3 py-1.5 text-xs"
              onClick={(event) => {
                event.stopPropagation();
                void copyLink();
              }}
            >
              Copy room link
            </button>
            <button
              type="button"
              className="btn-secondary px-3 py-1.5 text-xs"
              onClick={(event) => {
                event.stopPropagation();
                void handleLeave();
              }}
              disabled={leaving}
            >
              {leaving ? 'Leaving…' : 'Leave room'}
            </button>
            {joinedRole === 'host' && (
              <button
                type="button"
                className="btn-secondary px-3 py-1.5 text-xs"
                onClick={(event) => {
                  event.stopPropagation();
                  void handleEndRoom();
                }}
                disabled={ending}
              >
                {ending ? 'Ending…' : 'End room'}
              </button>
            )}
          </div>
        </div>

        {roomPanelExpanded && (
          <div className="mt-4">
            {joinedRole === 'host' ? (
              <div className="space-y-4">
                <div className="space-y-1">
                  <h2 className="text-xl font-semibold">Reconfigure Room</h2>
                  <p className="text-sm muted">
                    Switch between Watch Together and Listen Together without creating a new room.
                  </p>
                </div>

                <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-black/10 p-3">
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className={`px-4 py-2 text-sm rounded-lg ${reconfigureMode !== 'audio' ? 'btn-primary' : 'btn-secondary'}`}
                      onClick={() => {
                        setReconfigureDirty(true);
                        setReconfigureMode(reconfigureWatchSource);
                      }}
                    >
                      Watch Together
                    </button>
                    <button
                      type="button"
                      className={`px-4 py-2 text-sm rounded-lg ${reconfigureMode === 'audio' ? 'btn-primary' : 'btn-secondary'}`}
                      onClick={() => {
                        setReconfigureDirty(true);
                        setReconfigureMode('audio');
                      }}
                    >
                      Listen Together
                    </button>
                  </div>
                  <div className="flex w-full justify-end sm:w-auto">
                    <button
                      type="button"
                      className="btn-primary px-5 py-2.5 text-sm disabled:opacity-50"
                      onClick={handleReconfigureRoom}
                      disabled={
                        reconfiguring ||
                        (reconfigureMode === 'video' && !reconfigureVideoItem) ||
                        (reconfigureMode === 'audio' && !reconfigureAudioLibraryId)
                      }
                    >
                      {reconfiguring ? 'Reconfiguring…' : 'Apply Room Mode'}
                    </button>
                  </div>
                </div>

                {reconfigureMode !== 'audio' ? (
                  <div className="space-y-3">
                    <div className="space-y-2">
                      <p className="text-xs uppercase tracking-wide muted">Watch Source</p>
                      <div role="radiogroup" aria-label="Watch source" className="grid gap-2 sm:grid-cols-2">
                        <label
                          className={`tile cursor-pointer rounded-xl px-3 py-3 transition-colors ${
                            reconfigureMode === 'video' ? 'border-[var(--purple)]' : ''
                          }`}
                        >
                          <input
                            type="radio"
                            className="sr-only"
                            checked={reconfigureMode === 'video'}
                            onChange={() => {
                              setReconfigureDirty(true);
                              setReconfigureWatchSource('video');
                              setReconfigureMode('video');
                            }}
                          />
                          <p className="text-sm font-medium">Local Media</p>
                          <p className="text-xs muted">Use content from shared server libraries.</p>
                        </label>
                        <label
                          className={`tile cursor-pointer rounded-xl px-3 py-3 transition-colors ${
                            reconfigureMode === 'youtube' ? 'border-[var(--purple)]' : ''
                          }`}
                        >
                          <input
                            type="radio"
                            className="sr-only"
                            checked={reconfigureMode === 'youtube'}
                            onChange={() => {
                              setReconfigureDirty(true);
                              setReconfigureWatchSource('youtube');
                              setReconfigureMode('youtube');
                            }}
                          />
                          <p className="text-sm font-medium">YouTube</p>
                          <p className="text-xs muted">Shared search and queue inside the lobby.</p>
                        </label>
                      </div>
                    </div>

                    {reconfigureMode === 'video' ? (
                      reconfigureVideoLibraries.length === 0 ? (
                        <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                          No shared video libraries available for current room participants.
                        </div>
                      ) : (
                        <MediaPicker
                          libraries={allLibraries}
                          eligibleLibraryIds={eligibleLibraryIds}
                          selectedLibraryId={reconfigureVideoLibraryId}
                          selectedItem={reconfigureVideoItem}
                          layout="stacked"
                          onLibraryChange={setReconfigureVideoLibraryId}
                          onSelectItem={setReconfigureVideoItem}
                        />
                      )
                    ) : null}
                  </div>
                ) : (
                  reconfigureMusicLibraries.length === 0 ? (
                    <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                      No shared music libraries available for current room participants.
                    </div>
                  ) : (
                    <div className="space-y-2">
                      <label
                        htmlFor="reconfigure-audio-library"
                        className="block text-xs uppercase tracking-wide muted"
                      >
                        Music Library
                      </label>
                      <select
                        id="reconfigure-audio-library"
                        value={reconfigureAudioLibraryId}
                        onChange={(e) => setReconfigureAudioLibraryId(e.target.value)}
                        className="select px-3 py-2 text-sm"
                      >
                        {reconfigureMusicLibraries.map((library) => (
                          <option key={library.id} value={library.id}>
                            {library.name}
                          </option>
                        ))}
                      </select>
                    </div>
                  )
                )}
              </div>
            ) : (
              <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                Only room admins can reconfigure the room.
              </div>
            )}
          </div>
        )}
      </section>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {info && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{info}</div>}

      {!joinedRole && (
        <section className="panel space-y-4 p-5 sm:p-6">
          <h2 className="text-xl font-semibold">Join Room</h2>
          <p className="text-sm muted">
            You must join this room before {isAudioRoom ? 'listening' : isYoutubeRoom ? 'watching YouTube together' : 'opening synchronized playback'}.
          </p>

          {room.password_required && (
            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Password</span>
              <input
                type="password"
                value={joinPassword}
                onChange={(e) => setJoinPassword(e.target.value)}
                className="input px-3 py-2"
                placeholder="Enter room password"
              />
            </label>
          )}

          <button
            type="button"
            className="btn-primary px-5 py-2.5 text-sm"
            onClick={handleJoin}
            disabled={joining}
          >
            {joining ? 'Joining…' : 'Join room'}
          </button>
        </section>
      )}

      {joinedRole && isAudioRoom && audioState && (
        <AudioPlayer
          audioState={audioState}
          canControl={canPlayPause}
          canSeek={canSeek}
          roomId={roomId}
          sendWs={sendWs}
        />
      )}

      {joinedRole && isAudioRoom && !audioState && (
        <section className="panel p-5 sm:p-6">
          <p className="text-sm muted">Connecting to music party…</p>
        </section>
      )}

      {joinedRole && isYoutubeRoom && (
        <section className="panel space-y-4 p-5 sm:p-6">
          <div className="flex flex-wrap items-center gap-2">
            <span className="chip">Role: {joinedRoleDisplay}</span>
            <span className="chip">Controls: {canPlayPause ? 'allowed' : 'host-only'}</span>
          </div>
          <YouTubePlayer
            roomId={roomId}
            ytState={youtubeState}
            canControl={canPlayPause}
            canQueue={!!joinedRole}
            wsConnected={wsConnected}
            sendWs={sendWs}
            onDebugLog={handleYoutubeDebug}
          />
        </section>
      )}

      {joinedRole && isYoutubeRoom && (
        <section className="panel space-y-3 p-4 sm:p-5">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-base font-semibold">YouTube Diagnostics</h2>
            <button
              type="button"
              className="btn-secondary px-3 py-1.5 text-xs"
              onClick={copyDebugLog}
            >
              Copy logs
            </button>
          </div>
          <p className="text-xs muted">
            These logs capture websocket and YouTube player events for this room.
          </p>
          <div className="max-h-64 overflow-y-auto rounded-xl border border-white/10 bg-black/40 px-3 py-2 text-[11px] leading-5 text-white/80">
            {debugLog.length === 0 ? (
              <p className="muted">No diagnostic events yet.</p>
            ) : (
              <pre className="whitespace-pre-wrap break-words font-mono">
                {debugLog.join('\n')}
              </pre>
            )}
          </div>
        </section>
      )}

      {joinedRole && !isAudioRoom && !isYoutubeRoom && (
        <>
          <section className="panel space-y-4 p-5 sm:p-6">
            <div className="flex flex-wrap items-center gap-2">
              <span className="chip">Role: {joinedRoleDisplay}</span>
              <span className="chip">Play/Pause: {canPlayPause ? 'allowed' : 'host-only'}</span>
              <span className="chip">Seek: {canSeek ? 'allowed' : 'host-only'}</span>
            </div>

            <div className="tile overflow-hidden rounded-2xl border border-white/10 bg-black">
              <video
                ref={videoRef}
                controls={controlsEnabled}
                preload="auto"
                playsInline
                className="w-full max-h-[70vh]"
                onPlay={(event) => {
                  if (applyingRemoteRef.current || !canPlayPause) return;
                  sendWs({
                    type: 'play',
                    position_ms: Math.floor(event.currentTarget.currentTime * 1000),
                  });
                }}
                onPause={(event) => {
                  if (applyingRemoteRef.current || !canPlayPause) return;
                  sendWs({
                    type: 'pause',
                    position_ms: Math.floor(event.currentTarget.currentTime * 1000),
                  });
                }}
                onSeeked={(event) => {
                  if (applyingRemoteRef.current || !canSeek) return;
                  sendWs({
                    type: 'seek',
                    position_ms: Math.floor(event.currentTarget.currentTime * 1000),
                  });
                }}
                onError={() => {
                  if (mode !== 'direct') return;
                  setInfo('Direct playback failed. Switching to HLS…');
                  void startHls();
                }}
              />
            </div>

            <div className="panel-soft flex flex-wrap items-center gap-2 rounded-xl px-3 py-3">
              <button
                type="button"
                className={`px-4 py-2 text-sm ${mode === 'direct' ? 'btn-primary' : 'btn-secondary'}`}
                onClick={() => void startDirect()}
                disabled={startingDirect || startingHls || !descriptor}
              >
                {startingDirect ? 'Starting…' : 'Direct Play'}
              </button>
              <button
                type="button"
                className={`px-4 py-2 text-sm ${mode === 'hls' ? 'btn-primary' : 'btn-secondary'}`}
                onClick={() => void startHls()}
                disabled={startingDirect || startingHls || !descriptor}
              >
                {startingHls ? 'Starting…' : 'Transcode (HLS)'}
              </button>
              {!controlsEnabled && (
                <span className="text-xs muted">Playback controls are host-only in this room.</span>
              )}
            </div>
          </section>
        </>
      )}

      {joinedRole && (
        <div className="grid gap-5 xl:grid-cols-2">
          <section className="panel space-y-3 p-5 sm:p-6">
            <h2 className="text-xl font-semibold">Roster</h2>
            <ul className="space-y-2">
              {activeMembers.map((member) => (
                <li key={member.user_id} className="tile rounded-xl px-3 py-2">
                  <div className="flex items-center justify-between gap-2">
                    <div>
                      <p className="text-sm font-medium">{member.username}</p>
                      <p className="text-xs muted">{roleLabel(member.role, effectiveRoomMode)}</p>
                    </div>
                    <span className="chip">{member.connected ? 'Connected' : 'Offline'}</span>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel space-y-4 p-5 sm:p-6">
            <h2 className="text-xl font-semibold">Invite to Room</h2>
            <p className="text-xs muted">
              Re-invites are allowed. Sending repeated invites to the same user has a 5-second cooldown.
            </p>
            {invitableUsers.length === 0 ? (
              <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                All eligible users are already in this room.
              </div>
            ) : (
              <>
                <ul className="space-y-2">
                  {invitableUsers.map((user) => {
                    const checked = user.id in inviteSelections;
                    const role = inviteSelections[user.id] ?? 'viewer';
                    return (
                      <li key={user.id} className="tile rounded-xl px-3 py-2">
                        <div className="flex items-center gap-3">
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={() => {
                              setInviteSelections((prev) => {
                                const next = { ...prev };
                                if (next[user.id] !== undefined) {
                                  delete next[user.id];
                                } else {
                                  next[user.id] = 'viewer';
                                }
                                return next;
                              });
                            }}
                            className="h-4 w-4 shrink-0"
                          />
                          <span className="flex-1 text-sm font-medium">{user.username}</span>
                          <select
                            className="select px-2 py-1.5 text-sm"
                            value={role}
                            onChange={(e) =>
                              setInviteSelections((prev) => ({
                                ...prev,
                                [user.id]: e.target.value as 'viewer' | 'controller',
                              }))
                            }
                          >
                            <option value="viewer">{memberRoleDisplay}</option>
                            <option value="controller">Admin</option>
                          </select>
                        </div>
                      </li>
                    );
                  })}
                </ul>
                <button
                  type="button"
                  className="btn-primary px-5 py-2.5 text-sm disabled:opacity-50"
                  onClick={handleSendInvites}
                  disabled={sendingInvites || Object.keys(inviteSelections).length === 0}
                >
                  {sendingInvites ? 'Sending…' : 'Send Invites'}
                </button>
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
