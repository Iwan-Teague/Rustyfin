import { useCallback, useEffect, useRef, useState } from 'react';

import { apiFetch, apiJson } from '@/lib/api';
import { clientErrorMessage } from '@/lib/errors';
import type { WatchPartyRoomResponse } from '@/lib/watchPartyApi';
import type { PlaybackDescriptor, PlaybackSession, StartPlaybackOptions, WsStateMessage } from '../realtimeTypes';

type UseRoomPlaybackArgs = {
  room: WatchPartyRoomResponse | null;
  joinedRole: string | null;
  roomState: WsStateMessage | null;
  appendDebug: (message: string) => void;
  setError: (message: string) => void;
  setInfo: (message: string) => void;
};

function normalizeVideoVolume(video: HTMLVideoElement): void {
  // Preserve user mute/unmute choice; only normalize invalid volume values.
  if (!Number.isFinite(video.volume)) {
    video.volume = 1;
    return;
  }
  if (video.volume < 0) {
    video.volume = 0;
  } else if (video.volume > 1) {
    video.volume = 1;
  }
}

function resetVideoSourceForMse(video: HTMLVideoElement): void {
  // Ensure any previous direct/native source is fully detached before hls.js
  // attaches a MediaSource. This avoids stale source state on mode toggles.
  video.pause();
  video.removeAttribute('src');
  video.load();
}

function applyKnownDurationToHlsMediaSource(hls: unknown, durationSeconds: number): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const mediaSource = (hls as { mediaSource?: MediaSource } | null)?.mediaSource;
  if (!mediaSource || typeof mediaSource.duration !== 'number') return;
  try {
    if (mediaSource.readyState === 'open') {
      const current = mediaSource.duration;
      if (!Number.isFinite(current) || current < durationSeconds - 0.5) {
        mediaSource.duration = durationSeconds;
      }
    }
  } catch {
    // Some browsers may reject duration writes while updating source buffers.
  }
}

function forceKnownDurationInLevelDetails(levelData: unknown, durationSeconds: number): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const details = (levelData as { details?: Record<string, unknown> } | null)?.details;
  if (!details) return;
  const currentTotal =
    typeof details.totalduration === 'number' && Number.isFinite(details.totalduration)
      ? details.totalduration
      : 0;
  if (currentTotal < durationSeconds) {
    details.totalduration = durationSeconds;
    details.edge = durationSeconds;
  }
}

function installKnownDurationEnforcer(hls: unknown, durationSeconds: number): () => void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return () => {};
  }
  const tick = () => applyKnownDurationToHlsMediaSource(hls, durationSeconds);
  tick();
  const timer = window.setInterval(tick, 500);
  return () => window.clearInterval(timer);
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

export function useRoomPlayback({
  room,
  joinedRole,
  roomState,
  appendDebug,
  setError,
  setInfo,
}: UseRoomPlaybackArgs) {
  const [descriptor, setDescriptor] = useState<PlaybackDescriptor | null>(null);
  const [mode, setMode] = useState<'direct' | 'hls'>('direct');
  const [startingDirect, setStartingDirect] = useState(false);
  const [startingHls, setStartingHls] = useState(false);
  const [hlsTargetHeight, setHlsTargetHeight] = useState<number | null>(null);

  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<unknown>(null);
  const sessionIdRef = useRef<string | null>(null);
  const applyingRemoteRef = useRef(false);
  const autoPreloadedItemRef = useRef<string | null>(null);

  const isAudioRoom = room?.room_mode === 'audio';
  const isWebRoom = room?.room_mode === 'web';
  const isYoutubeRoom = room?.room_mode === 'youtube';
  const isCreateRoom = room?.room_mode === 'create';
  const isPlayRoom = room?.room_mode === 'play';
  const isVideoRoom =
    room?.room_mode === 'video' ||
    (!isAudioRoom && !isWebRoom && !isYoutubeRoom && !isCreateRoom && !isPlayRoom);

  const destroyHls = useCallback(() => {
    const hls = hlsRef.current as { destroy?: () => void } | null;
    const stopDurationEnforcer = (hls as { __stopKnownDurationEnforcer?: () => void } | null)
      ?.__stopKnownDurationEnforcer;
    if (typeof stopDurationEnforcer === 'function') {
      try {
        stopDurationEnforcer();
      } catch {
        // no-op
      }
    }
    if (hls && typeof hls.destroy === 'function') {
      try {
        hls.destroy();
      } catch {
        // no-op
      }
    }
    hlsRef.current = null;
  }, []);

  const stopSession = useCallback(async (sessionId: string) => {
    await apiFetch(`/playback/sessions/${sessionId}/stop`, { method: 'POST' }).catch(() => {});
  }, []);

  const resetPlaybackState = useCallback(async () => {
    destroyHls();
    if (sessionIdRef.current) {
      await stopSession(sessionIdRef.current);
      sessionIdRef.current = null;
    }
    setDescriptor(null);
    setMode('direct');
  }, [destroyHls, stopSession]);

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
      normalizeVideoVolume(video);
      await video.play().catch(() => {});
    }

    if (!stateMessage.playing && !video.paused) {
      video.pause();
    }

    window.setTimeout(() => {
      applyingRemoteRef.current = false;
    }, 60);
  }, []);

  const startDirect = useCallback(
    async (options: StartPlaybackOptions = {}): Promise<boolean> => {
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
        normalizeVideoVolume(video);

        setMode('direct');
        video.src = descriptor.direct_url;
        video.load();
        await waitForVideoMetadata(video);

        if (roomState) {
          await applyRemoteState(roomState);
        } else if (options.autoplayWhenNoState ?? true) {
          normalizeVideoVolume(video);
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
      } catch (err: unknown) {
        if (!options.silent) {
          setError(clientErrorMessage(err, 'Failed to start direct playback'));
        }
        return false;
      } finally {
        setStartingDirect(false);
      }
    },
    [descriptor, destroyHls, stopSession, roomState, applyRemoteState, setError],
  );

  const startHls = useCallback(
    async (options: StartPlaybackOptions = {}): Promise<boolean> => {
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
        normalizeVideoVolume(video);
        const selectedTargetHeight =
          options.targetHeightOverride !== undefined
            ? options.targetHeightOverride
            : hlsTargetHeight;
        const knownDurationSeconds =
          descriptor.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0;
        destroyHls();
        if (sessionIdRef.current) {
          await stopSession(sessionIdRef.current);
          sessionIdRef.current = null;
        }

        const session = await apiJson<PlaybackSession>(descriptor.hls_start_url, {
          method: 'POST',
          body: JSON.stringify({
            file_id: descriptor.file_id,
            target_height: selectedTargetHeight ?? undefined,
          }),
        });

        sessionIdRef.current = session.session_id;
        setMode('hls');

        const Hls = (await import('hls.js')).default;
        const canNativeHls = video.canPlayType('application/vnd.apple.mpegurl') !== '';

        // Prefer hls.js whenever possible; browser canPlayType() is not reliable for HLS support.
        if (Hls.isSupported()) {
          resetVideoSourceForMse(video);
          const hls = new Hls({
            enableWorker: true,
            lowLatencyMode: false,
            // Preload a deeper forward buffer for smoother watch-party playback.
            maxBufferLength: 600,
            maxMaxBufferLength: 1200,
            maxBufferSize: 256 * 1000 * 1000,
            backBufferLength: 180,
            startFragPrefetch: true,
            manifestLoadingTimeOut: 45000,
            levelLoadingTimeOut: 45000,
            fragLoadingTimeOut: 45000,
            manifestLoadingMaxRetry: 10,
            levelLoadingMaxRetry: 10,
            fragLoadingMaxRetry: 8,
            manifestLoadingRetryDelay: 1000,
            levelLoadingRetryDelay: 1000,
            fragLoadingRetryDelay: 1000,
            manifestLoadingMaxRetryTimeout: 10000,
            levelLoadingMaxRetryTimeout: 10000,
            fragLoadingMaxRetryTimeout: 8000,
          });
          const stopDurationEnforcer = installKnownDurationEnforcer(hls, knownDurationSeconds);
          (hls as { __stopKnownDurationEnforcer?: () => void }).__stopKnownDurationEnforcer =
            stopDurationEnforcer;
          hlsRef.current = hls;
          let networkRecoveries = 0;
          hls.on(Hls.Events.MANIFEST_PARSED, () => {
            applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            window.setTimeout(() => {
              applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            }, 0);
            if (roomState) {
              void applyRemoteState(roomState);
            } else if (options.autoplayWhenNoState ?? true) {
              normalizeVideoVolume(video);
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
          hls.on(Hls.Events.LEVEL_LOADED, (_event: unknown, data: unknown) => {
            forceKnownDurationInLevelDetails(data, knownDurationSeconds);
            applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            window.setTimeout(() => {
              applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            }, 0);
          });
          hls.on(Hls.Events.ERROR, (_event: unknown, data: any) => {
            if (!data?.fatal) return;
            const errorType = data?.type;
            if (errorType === Hls.ErrorTypes.NETWORK_ERROR && networkRecoveries < 10) {
              networkRecoveries += 1;
              setInfo('Transcode stream reconnecting…');
              try {
                hls.startLoad();
                return;
              } catch {
                // fall through to hard error
              }
            } else if (errorType === Hls.ErrorTypes.MEDIA_ERROR) {
              try {
                hls.recoverMediaError();
                return;
              } catch {
                // fall through to hard error
              }
            }
            setError(`HLS playback error: ${data.details || 'fatal stream error'}`);
          });
          hls.attachMedia(video);
          hls.loadSource(session.hls_url);
        } else if (canNativeHls) {
          video.src = session.hls_url;
          video.load();
          await waitForVideoMetadata(video);
          if (roomState) {
            await applyRemoteState(roomState);
          } else if (options.autoplayWhenNoState ?? true) {
            normalizeVideoVolume(video);
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
          throw new Error('HLS is not supported in this browser');
        }
        return true;
      } catch (err: unknown) {
        if (!options.silent) {
          setError(clientErrorMessage(err, 'Failed to start HLS playback'));
        }
        return false;
      } finally {
        setStartingHls(false);
      }
    },
    [
      descriptor,
      destroyHls,
      stopSession,
      roomState,
      applyRemoteState,
      setError,
      setInfo,
      hlsTargetHeight,
    ],
  );

  useEffect(() => {
    if (!room || !joinedRole || !isVideoRoom) return;
    if (!room.item_id || room.item_id.trim().length === 0) {
      setDescriptor(null);
      return;
    }

    let cancelled = false;

    apiJson<PlaybackDescriptor>(`/items/${room.item_id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load playback descriptor'));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [room, joinedRole, isVideoRoom, setError]);

  useEffect(() => {
    autoPreloadedItemRef.current = null;
  }, [room?.item_id, room?.room_mode]);

  useEffect(() => {
    if (!room || !joinedRole || !isVideoRoom || !descriptor) return;
    if (!room.item_id || room.item_id.trim().length === 0) return;
    if (startingDirect || startingHls) return;
    if (autoPreloadedItemRef.current === room.item_id) return;

    autoPreloadedItemRef.current = room.item_id;
    appendDebug(`auto preload requested item_id=${room.item_id} preferred=hls`);

    void (async () => {
      const hlsOk = await startHls({
        autoplayWhenNoState: false,
        silent: true,
      });
      if (hlsOk) {
        appendDebug(`auto preload succeeded mode=hls item_id=${room.item_id}`);
        return;
      }

      appendDebug(`auto preload hls failed; keeping room idle item_id=${room.item_id}`);
      setInfo('HLS preload failed. Retry Transcode (HLS) or use Direct Play manually.');
    })();
  }, [
    room,
    joinedRole,
    descriptor,
    isVideoRoom,
    startingDirect,
    startingHls,
    startHls,
    appendDebug,
    setInfo,
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

  return {
    descriptor,
    mode,
    startingDirect,
    startingHls,
    hlsTargetHeight,
    setHlsTargetHeight,
    isVideoRoom,
    videoRef,
    applyingRemoteRef,
    applyRemoteState,
    startDirect,
    startHls,
    resetPlaybackState,
  };
}
