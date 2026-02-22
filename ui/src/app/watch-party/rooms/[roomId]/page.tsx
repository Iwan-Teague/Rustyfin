'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useRouter } from 'next/navigation';
import { apiFetch, apiJson } from '@/lib/api';
import { useAuth } from '@/lib/auth';
import {
  WatchPartyRoomResponse,
  endWatchPartyRoom,
  getWatchPartyRoom,
  joinWatchPartyRoom,
  leaveWatchPartyRoom,
} from '@/lib/watchPartyApi';

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

type WsMessage = WsStateMessage | WsPresenceMessage | WsErrorMessage | WsPongMessage;

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
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [joinPassword, setJoinPassword] = useState('');
  const [joinedRole, setJoinedRole] = useState<string | null>(null);

  const [loadingRoom, setLoadingRoom] = useState(true);
  const [joining, setJoining] = useState(false);
  const [startingDirect, setStartingDirect] = useState(false);
  const [startingHls, setStartingHls] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [ending, setEnding] = useState(false);

  const [wsConnected, setWsConnected] = useState(false);
  const [error, setError] = useState('');
  const [info, setInfo] = useState('');
  const [mode, setMode] = useState<'direct' | 'hls'>('direct');

  const wsRef = useRef<WebSocket | null>(null);
  const hlsRef = useRef<any>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const sessionIdRef = useRef<string | null>(null);
  const applyingRemoteRef = useRef(false);

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

  const activeMembers = roomState?.members || room?.members.map((member) => ({
    user_id: member.user_id,
    username: member.username,
    role: member.role,
    connected: member.status === 'joined',
  })) || [];

  const sendWs = useCallback((payload: Record<string, unknown>) => {
    const socket = wsRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify(payload));
  }, []);

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
      if (me) {
        const current = data.members.find((member) => member.user_id === me.id);
        if (current?.status === 'joined') {
          setJoinedRole(current.role);
        }
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to load watch party room');
    } finally {
      setLoadingRoom(false);
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
    if (!room || !joinedRole) return;

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
  }, [room, joinedRole]);

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

    const token = localStorage.getItem('token');
    if (!token) return;

    const bindOpenSocket = (socket: WebSocket, candidateIndex: number) => {
      activeSocket = socket;
      wsRef.current = socket;
      setWsConnected(true);

      if (candidateIndex > 0) {
        setInfo('Connected to watch-party websocket via backend fallback.');
      }

      socket.send(JSON.stringify({ type: 'auth', token }));

      socket.onmessage = (event) => {
        try {
          const payload = JSON.parse(event.data) as WsMessage;

          if (payload.type === 'state') {
            setRoomState(payload);
            void applyRemoteState(payload);
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
          } else if (payload.type === 'error') {
            setError(payload.message);
          }
        } catch {
          setError('Invalid websocket message received');
        }
      };

      socket.onerror = () => {
        // onclose handles fallback/disconnect state.
      };

      socket.onclose = () => {
        if (cancelled) return;
        setWsConnected(false);
      };
    };

    const attemptConnect = async () => {
      const candidates: string[] = [wsUrlForOrigin(window.location.origin, roomId)];

      try {
        const runtimeConfig = await fetch('/runtime-config', { cache: 'no-store' });
        if (runtimeConfig.ok) {
          const payload = (await runtimeConfig.json()) as RuntimeConfig;
          if (payload.backend_origin) {
            const directBackendWs = wsUrlForOrigin(payload.backend_origin, roomId);
            if (!candidates.includes(directBackendWs)) {
              candidates.push(directBackendWs);
            }
          }
        }
      } catch {
        // Best-effort fallback lookup.
      }

      for (let index = 0; index < candidates.length; index += 1) {
        if (cancelled) return;

        const candidate = candidates[index];
        const socket = await new Promise<WebSocket | null>((resolve) => {
          let settled = false;
          const ws = new WebSocket(candidate);

          const finish = (result: WebSocket | null) => {
            if (settled) return;
            settled = true;
            resolve(result);
          };

          ws.onopen = () => finish(ws);
          ws.onerror = () => {};
          ws.onclose = () => finish(null);

          window.setTimeout(() => {
            if (settled) return;
            try {
              ws.close();
            } catch {
              // no-op
            }
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
          'Watch-party websocket connection failed. Restart with ./scripts/start.sh --build and retry.',
        );
      }
    };

    void attemptConnect();

    return () => {
      cancelled = true;
      if (activeSocket) {
        activeSocket.close();
      }
      wsRef.current = null;
      setWsConnected(false);
    };
  }, [roomId, joinedRole, applyRemoteState]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionIdRef.current) {
        void stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }
    };
  }, [destroyHls, stopSession]);

  const startDirect = useCallback(async () => {
    if (!descriptor) {
      setError('Playback descriptor is not ready yet');
      return;
    }

    setStartingDirect(true);
    setError('');

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
      } else {
        await video.play().catch(() => {});
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to start direct playback');
    } finally {
      setStartingDirect(false);
    }
  }, [descriptor, destroyHls, stopSession, roomState, applyRemoteState]);

  const startHls = useCallback(async () => {
    if (!descriptor) {
      setError('Playback descriptor is not ready yet');
      return;
    }

    setStartingHls(true);
    setError('');
    setInfo('Preparing transcoded stream…');

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
        } else {
          await video.play().catch(() => {});
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
          } else {
            void video.play().catch(() => {});
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
    } catch (err: any) {
      setError(err?.message || 'Failed to start HLS playback');
    } finally {
      setStartingHls(false);
    }
  }, [descriptor, destroyHls, stopSession, roomState, applyRemoteState]);

  async function handleJoin() {
    setJoining(true);
    setError('');
    setInfo('');
    try {
      const result = await joinWatchPartyRoom(roomId, joinPassword || undefined);
      setJoinedRole(result.role);
      setInfo('Joined watch-party room.');
      await loadRoom();
    } catch (err: any) {
      setError(err?.message || 'Failed to join room');
    } finally {
      setJoining(false);
    }
  }

  async function handleLeave() {
    setLeaving(true);
    setError('');
    try {
      await leaveWatchPartyRoom(roomId);
      router.push('/watch-party');
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
      await loadRoom();
      setInfo('Room ended.');
    } catch (err: any) {
      setError(err?.message || 'Failed to end room');
    } finally {
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
              onClick={() => router.push('/watch-party')}
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
      <header className="panel space-y-3 p-6 sm:p-7">
        <span className="chip chip-accent">Watch Party Room</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Room {room.room_id}</h1>
        <p className="text-sm muted">
          Item: {room.item_id} • Status: {room.status}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <button type="button" className="btn-secondary px-4 py-2 text-sm" onClick={copyLink}>
            Copy room link
          </button>
          <button
            type="button"
            className="btn-secondary px-4 py-2 text-sm"
            onClick={handleLeave}
            disabled={leaving}
          >
            {leaving ? 'Leaving…' : 'Leave room'}
          </button>
          {joinedRole === 'host' && (
            <button
              type="button"
              className="btn-ghost px-4 py-2 text-sm"
              onClick={handleEndRoom}
              disabled={ending}
            >
              {ending ? 'Ending…' : 'End room'}
            </button>
          )}
          <span className="chip">WS: {wsConnected ? 'connected' : 'disconnected'}</span>
        </div>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {info && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{info}</div>}

      {!joinedRole && (
        <section className="panel space-y-4 p-5 sm:p-6">
          <h2 className="text-xl font-semibold">Join Room</h2>
          <p className="text-sm muted">
            You must join this room before opening synchronized playback.
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

      {joinedRole && (
        <>
          <section className="panel space-y-4 p-5 sm:p-6">
            <div className="flex flex-wrap items-center gap-2">
              <span className="chip">Role: {joinedRole}</span>
              <span className="chip">Play/Pause: {canPlayPause ? 'allowed' : 'host-only'}</span>
              <span className="chip">Seek: {canSeek ? 'allowed' : 'host-only'}</span>
            </div>

            <div className="tile overflow-hidden rounded-2xl border border-white/10 bg-black">
              <video
                ref={videoRef}
                controls={controlsEnabled}
                autoPlay
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

          <section className="panel space-y-3 p-5 sm:p-6">
            <h2 className="text-xl font-semibold">Roster</h2>
            <ul className="space-y-2">
              {activeMembers.map((member) => (
                <li key={member.user_id} className="tile rounded-xl px-3 py-2">
                  <div className="flex items-center justify-between gap-2">
                    <div>
                      <p className="text-sm font-medium">{member.username}</p>
                      <p className="text-xs muted">{member.role}</p>
                    </div>
                    <span className="chip">{member.connected ? 'Connected' : 'Offline'}</span>
                  </div>
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </div>
  );
}
