import { useCallback, useEffect, useRef, useState } from 'react';

import type {
  WsAudioStateMessage,
  WsCreateStateMessage,
  WsOnlineAudioStatusMessage,
  WsPlayStateMessage,
  WsRoomReconfiguredMessage,
  WsWebStateMessage,
  WsYouTubeStateMessage,
} from '@/lib/watchPartyApi';
import { clientErrorMessage } from '@/lib/errors';
import type { RuntimeConfig, WsMessage, WsPresenceMember, WsPresenceMessage, WsStateMessage } from '../realtimeTypes';

type UseRoomRealtimeArgs = {
  roomId: string;
  joinedRole: string | null;
  appendDebug: (message: string) => void;
  setError: (message: string) => void;
  setInfo: (message: string) => void;
  refreshRoom: () => Promise<void>;
  loadRoom: () => Promise<void>;
  setJoinedRole: (role: string | null) => void;
  onRoomReconfigured: (payload: WsRoomReconfiguredMessage) => void;
  onRoomEnded: () => void;
};

function wsUrlForOrigin(origin: string, roomId: string): string {
  const url = new URL(origin);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.pathname = `/api/v1/watch-party/rooms/${roomId}/ws`;
  url.search = '';
  url.hash = '';
  return url.toString();
}

function applyPresenceToState<T extends { members: WsPresenceMember[] }>(
  prev: T | null,
  presence: WsPresenceMessage,
): T | null {
  if (!prev) return prev;
  let changed = false;
  const members = prev.members.map((member) => {
    if (member.user_id !== presence.user_id || member.connected === presence.connected) {
      return member;
    }
    changed = true;
    return { ...member, connected: presence.connected };
  });
  if (!changed) return prev;
  return { ...prev, members };
}

export function useRoomRealtime({
  roomId,
  joinedRole,
  appendDebug,
  setError,
  setInfo,
  refreshRoom,
  loadRoom,
  setJoinedRole,
  onRoomReconfigured,
  onRoomEnded,
}: UseRoomRealtimeArgs) {
  const wsRef = useRef<WebSocket | null>(null);
  const [wsConnected, setWsConnected] = useState(false);
  const [wsEpoch, setWsEpoch] = useState(0);

  const [roomState, setRoomState] = useState<WsStateMessage | null>(null);
  const [audioState, setAudioState] = useState<WsAudioStateMessage | null>(null);
  const [onlineAudioStatusEvents, setOnlineAudioStatusEvents] = useState<
    WsOnlineAudioStatusMessage[]
  >([]);
  const [webState, setWebState] = useState<WsWebStateMessage | null>(null);
  const [youtubeState, setYoutubeState] = useState<WsYouTubeStateMessage | null>(null);
  const [createState, setCreateState] = useState<WsCreateStateMessage | null>(null);
  const [playState, setPlayState] = useState<WsPlayStateMessage | null>(null);

  const resetRealtimeState = useCallback(() => {
    setRoomState(null);
    setAudioState(null);
    setOnlineAudioStatusEvents([]);
    setWebState(null);
    setYoutubeState(null);
    setCreateState(null);
    setPlayState(null);
  }, []);

  const sendWs = useCallback(
    (payload: Record<string, unknown>): boolean => {
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
    },
    [appendDebug, setError],
  );

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
            return;
          }

          if (payload.type === 'audio_state') {
            setAudioState(payload);
            setRoomState((prev) => (prev ? { ...prev, members: payload.members } : prev));
            return;
          }

          if (payload.type === 'online_audio_status') {
            appendDebug(
              `online audio status stage=${payload.stage} status=${payload.status} message=${payload.message}`,
            );
            setOnlineAudioStatusEvents((prev) => {
              const keyFor = (eventItem: WsOnlineAudioStatusMessage) =>
                `${eventItem.video_id ?? 'none'}:${eventItem.track_id ?? 'none'}:${eventItem.stage}`;
              const key = keyFor(payload);
              const existingIndex = prev.findIndex((eventItem) => keyFor(eventItem) === key);
              if (existingIndex >= 0) {
                const next = [...prev];
                next[existingIndex] = payload;
                return next;
              }
              const next = [...prev, payload];
              if (next.length > 40) {
                return next.slice(next.length - 40);
              }
              return next;
            });
            return;
          }

          if (payload.type === 'web_state') {
            appendDebug(`web state url=${payload.url || 'none'} updated_ts_ms=${payload.updated_ts_ms}`);
            setWebState(payload);
            setRoomState((prev) => (prev ? { ...prev, members: payload.members } : prev));
            return;
          }

          if (payload.type === 'create_state') {
            appendDebug(
              `create state tool=${payload.active_tool} doc=${payload.document_name} updated_ts_ms=${payload.updated_ts_ms}`,
            );
            setCreateState(payload);
            setRoomState((prev) => (prev ? { ...prev, members: payload.members } : prev));
            return;
          }

          if (payload.type === 'play_state') {
            appendDebug(
              `play state game=${payload.active_game} status=${payload.chess.status} turn=${payload.chess.turn}`,
            );
            setPlayState(payload);
            setRoomState((prev) => (prev ? { ...prev, members: payload.members } : prev));
            return;
          }

          if (payload.type === 'youtube_state' || payload.type === 'you_tube_state') {
            appendDebug(
              `youtube state video_id=${payload.video_id || 'none'} playing=${payload.playing} position_ms=${payload.position_ms}`,
            );
            setYoutubeState({
              ...payload,
              type: 'youtube_state',
            });
            return;
          }

          if (payload.type === 'room_reconfigured') {
            appendDebug(
              `room reconfigured mode=${payload.room_mode} audio_source=${payload.audio_source || 'library'} item_id=${payload.item_id || 'none'} audio_library_id=${payload.audio_library_id || 'none'} youtube_video_id=${payload.youtube_video_id || 'none'} web_url=${payload.web_url || 'none'} create_tool=${payload.create_tool || 'none'} create_document_name=${payload.create_document_name || 'none'}`,
            );
            onRoomReconfigured(payload);
            resetRealtimeState();
            void loadRoom();
            setWsEpoch((prev) => prev + 1);
            return;
          }

          if (payload.type === 'presence') {
            setRoomState((prev) => applyPresenceToState(prev, payload));
            setAudioState((prev) => applyPresenceToState(prev, payload));
            setYoutubeState((prev) => applyPresenceToState(prev, payload));
            setWebState((prev) => applyPresenceToState(prev, payload));
            setCreateState((prev) => applyPresenceToState(prev, payload));
            setPlayState((prev) => applyPresenceToState(prev, payload));
            return;
          }

          if (payload.type === 'error') {
            setError(payload.message);
            appendDebug(`ws server_error message=${payload.message}`);
            const normalized = payload.message.toLowerCase();
            if (normalized.includes('not joined')) {
              setJoinedRole(null);
              void refreshRoom();
            }
            return;
          }

          if (payload.type === 'room_ended') {
            appendDebug('ws room ended notification received');
            onRoomEnded();
          }
        } catch (err: unknown) {
          setError('Invalid websocket message received');
          const snippet = String(event.data).slice(0, 180).replace(/\s+/g, ' ');
          const message = clientErrorMessage(err, 'unknown_parse_error');
          appendDebug(`ws message parse failed error=${message} payload="${snippet}"`);
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
      } catch (err: unknown) {
        const message = clientErrorMessage(err, 'runtime-config lookup failed');
        appendDebug(`runtime-config lookup failed error=${message}`);
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
    appendDebug,
    setError,
    setInfo,
    refreshRoom,
    loadRoom,
    setJoinedRole,
    onRoomReconfigured,
    onRoomEnded,
    resetRealtimeState,
    wsEpoch,
  ]);

  return {
    roomState,
    setRoomState,
    audioState,
    setAudioState,
    onlineAudioStatusEvents,
    setOnlineAudioStatusEvents,
    webState,
    setWebState,
    youtubeState,
    setYoutubeState,
    createState,
    setCreateState,
    playState,
    setPlayState,
    wsConnected,
    sendWs,
    resetRealtimeState,
  };
}
