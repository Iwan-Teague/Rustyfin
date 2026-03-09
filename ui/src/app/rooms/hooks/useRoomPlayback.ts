import { useCallback, useEffect, useRef, useState } from 'react';

import { normalizePlaybackQualitySelection } from '@/app/components/VideoPlayerSurface';
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

type MediaInfo = {
  duration_secs?: number;
  video?: {
    height?: number;
  } | null;
};

type HlsFatalErrorData = {
  fatal?: boolean;
  type?: string;
  details?: string;
};

type VideoAudioState = {
  muted: boolean;
  volume: number;
};

function clampVideoVolume(value: number): number {
  if (!Number.isFinite(value)) return 1;
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

function readVideoAudioState(video: HTMLVideoElement): VideoAudioState {
  return {
    muted: video.muted,
    volume: clampVideoVolume(video.volume),
  };
}

function applyVideoAudioState(video: HTMLVideoElement, state: VideoAudioState): void {
  video.volume = clampVideoVolume(state.volume);
  video.muted = state.muted;
}

function resetVideoSourceForMse(video: HTMLVideoElement): void {
  // Ensure any previous direct/native source is fully detached before hls.js
  // attaches a MediaSource. This avoids stale source state on mode toggles.
  video.pause();
  video.removeAttribute('src');
  video.load();
}

function resolveHlsMediaSource(hls: unknown): MediaSource | null {
  const candidate = hls as {
    mediaSource?: unknown;
    bufferController?: { mediaSource?: unknown };
    coreComponents?: Array<{ mediaSource?: unknown }>;
  } | null;
  if (!candidate) return null;
  if (candidate.mediaSource instanceof MediaSource) return candidate.mediaSource;
  if (candidate.bufferController?.mediaSource instanceof MediaSource) {
    return candidate.bufferController.mediaSource;
  }
  if (Array.isArray(candidate.coreComponents)) {
    for (const component of candidate.coreComponents) {
      if (component?.mediaSource instanceof MediaSource) {
        return component.mediaSource;
      }
    }
  }
  return null;
}

function applyKnownDurationToHlsMediaSource(hls: unknown, durationSeconds: number): void {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return;
  const mediaSource = resolveHlsMediaSource(hls);
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

function readBufferedWindowDuration(video: HTMLVideoElement | null): number {
  if (!video) return 0;
  try {
    const buffered = video.buffered;
    if (buffered && buffered.length > 0) {
      const end = buffered.end(buffered.length - 1);
      if (Number.isFinite(end) && end > 0) {
        return end;
      }
    }
  } catch {
    // Some browsers can throw while buffered ranges are mutating.
  }
  return Number.isFinite(video.currentTime) && video.currentTime > 0 ? video.currentTime : 0;
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

function extractPlaylistWindowDuration(levelData: unknown): number {
  const details = (levelData as { details?: Record<string, unknown> } | null)?.details as
    | {
        fragments?: Array<{
          end?: number;
          start?: number;
          duration?: number;
        }>;
      }
    | undefined;
  const fragments = details?.fragments;
  if (!Array.isArray(fragments) || fragments.length === 0) {
    return 0;
  }
  const last = fragments[fragments.length - 1];
  const fromEnd = typeof last?.end === 'number' && Number.isFinite(last.end) ? last.end : 0;
  const fromStartDuration =
    typeof last?.start === 'number' &&
    Number.isFinite(last.start) &&
    typeof last?.duration === 'number' &&
    Number.isFinite(last.duration)
      ? last.start + last.duration
      : 0;
  return Math.max(fromEnd, fromStartDuration, 0);
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

async function waitForVideoFrameData(
  video: HTMLVideoElement,
  timeoutMs = 4000,
): Promise<void> {
  if (video.readyState >= 2) return;

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      video.removeEventListener('loadeddata', finish);
      video.removeEventListener('canplay', finish);
      resolve();
    };

    video.addEventListener('loadeddata', finish);
    video.addEventListener('canplay', finish);
    window.setTimeout(finish, timeoutMs);
  });
}

async function ensurePreviewFrame(video: HTMLVideoElement): Promise<void> {
  await waitForVideoMetadata(video);
  await waitForVideoFrameData(video);

  try {
    const previewTarget = video.currentTime > 0.001 ? video.currentTime : 0.001;
    if (Math.abs(video.currentTime - previewTarget) > 0.0005) {
      video.currentTime = previewTarget;
    }
  } catch {
    // Some browsers block tiny seeks while the first segment is still warming up.
  }
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
  const [mediaInfo, setMediaInfo] = useState<MediaInfo | null>(null);
  const [startingHls, setStartingHls] = useState(false);
  const [hlsTargetHeight, setHlsTargetHeight] = useState<number | null>(null);
  const [hlsSessionStartOffsetSecs, setHlsSessionStartOffsetSecs] = useState(0);
  const [hlsAvailableWindowDurationSecs, setHlsAvailableWindowDurationSecs] = useState(0);

  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<unknown>(null);
  const sessionIdRef = useRef<string | null>(null);
  const applyingRemoteRef = useRef(false);
  const autoPreloadedItemRef = useRef<string | null>(null);
  const audioStateRef = useRef<VideoAudioState>({ muted: false, volume: 1 });

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
    setMediaInfo(null);
    setHlsSessionStartOffsetSecs(0);
    setHlsAvailableWindowDurationSecs(0);
  }, [destroyHls, stopSession]);

  const applyRemoteStateRef = useRef<(stateMessage: WsStateMessage) => Promise<void>>(async () => {});

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
        audioStateRef.current = readVideoAudioState(video);
        const preservedAudioState = audioStateRef.current;
        const selectedTargetHeight =
          options.targetHeightOverride !== undefined
            ? options.targetHeightOverride
            : hlsTargetHeight;
        const knownDurationSeconds = Math.max(
          descriptor.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
          mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
        );
        const explicitSeekTime =
          options.seekTimeOverrideSecs !== undefined &&
          Number.isFinite(options.seekTimeOverrideSecs) &&
          options.seekTimeOverrideSecs >= 0
            ? options.seekTimeOverrideSecs
            : undefined;
        destroyHls();
        if (sessionIdRef.current) {
          await stopSession(sessionIdRef.current);
          sessionIdRef.current = null;
        }

        const session = await apiJson<PlaybackSession>(descriptor.hls_start_url, {
          method: 'POST',
          body: JSON.stringify({
            file_id: descriptor.file_id,
            start_time_secs: explicitSeekTime,
            target_height: selectedTargetHeight ?? undefined,
          }),
        });

        sessionIdRef.current = session.session_id;
        setHlsSessionStartOffsetSecs(explicitSeekTime ?? 0);
        setHlsAvailableWindowDurationSecs(0);

        const Hls = (await import('hls.js')).default;
        const canNativeHls = video.canPlayType('application/vnd.apple.mpegurl') !== '';

        // Prefer hls.js whenever possible; browser canPlayType() is not reliable for HLS support.
        if (Hls.isSupported()) {
          resetVideoSourceForMse(video);
          const hls = new Hls({
            enableWorker: true,
            lowLatencyMode: false,
            liveDurationInfinity: false,
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
          const reinforceKnownDuration = (data?: unknown) => {
            const playlistWindowDuration = extractPlaylistWindowDuration(data);
            const reportedWindowDuration = Math.max(
              playlistWindowDuration,
              readBufferedWindowDuration(video),
            );
            if (reportedWindowDuration > 0) {
              setHlsAvailableWindowDurationSecs((current) =>
                reportedWindowDuration > current ? reportedWindowDuration : current,
              );
            }
            if (data) {
              forceKnownDurationInLevelDetails(data, knownDurationSeconds);
            }
            applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            window.setTimeout(() => {
              applyKnownDurationToHlsMediaSource(hls, knownDurationSeconds);
            }, 0);
          };
          hls.on(Hls.Events.MANIFEST_PARSED, () => {
            reinforceKnownDuration();
            applyVideoAudioState(video, preservedAudioState);
            if (roomState) {
              void applyRemoteStateRef.current(roomState);
            } else if (options.autoplayWhenNoState ?? true) {
              void video.play().catch(async () => {
                await ensurePreviewFrame(video).catch(() => {});
              });
            } else {
              video.pause();
              void ensurePreviewFrame(video).catch(() => {});
            }
          });
          hls.on(Hls.Events.LEVEL_LOADED, (_event: unknown, data: unknown) => {
            reinforceKnownDuration(data);
          });
          hls.on(Hls.Events.LEVEL_UPDATED, (_event: unknown, data: unknown) => {
            reinforceKnownDuration(data);
          });
          hls.on(Hls.Events.FRAG_BUFFERED, () => {
            reinforceKnownDuration();
          });
          hls.on(Hls.Events.ERROR, (_event: unknown, data: HlsFatalErrorData) => {
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
          applyVideoAudioState(video, preservedAudioState);
          await waitForVideoMetadata(video);
          if (roomState) {
            await applyRemoteStateRef.current(roomState);
          } else if (options.autoplayWhenNoState ?? true) {
            await video.play().catch(async () => {
              await ensurePreviewFrame(video).catch(() => {});
            });
          } else {
            video.pause();
            await ensurePreviewFrame(video).catch(() => {});
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
      setError,
      setInfo,
      hlsTargetHeight,
      mediaInfo,
    ],
  );

  const applyRemoteState = useCallback(async (stateMessage: WsStateMessage) => {
    const video = videoRef.current;
    if (!video) return;
    if (!video.currentSrc && !video.src) return;

    applyingRemoteRef.current = true;

    const targetSeconds = stateMessage.position_ms / 1000;
    const currentWindowDuration = Math.max(
      hlsAvailableWindowDurationSecs,
      readBufferedWindowDuration(video),
    );
    const bufferedWindowEndSecs = hlsSessionStartOffsetSecs + currentWindowDuration;
    const requiresSessionRestart =
      targetSeconds < Math.max(0, hlsSessionStartOffsetSecs - 1) ||
      targetSeconds > bufferedWindowEndSecs + 1;

    if (requiresSessionRestart) {
      await startHls({
        autoplayWhenNoState: stateMessage.playing,
        silent: true,
        targetHeightOverride: hlsTargetHeight,
        seekTimeOverrideSecs: targetSeconds,
      });
      window.setTimeout(() => {
        applyingRemoteRef.current = false;
      }, 60);
      return;
    }

    const sessionRelativeTarget = Math.max(0, targetSeconds - hlsSessionStartOffsetSecs);
    if (Math.abs(video.currentTime - sessionRelativeTarget) > 1.2) {
      video.currentTime = sessionRelativeTarget;
    }

    if (stateMessage.playing && video.paused) {
      applyVideoAudioState(video, audioStateRef.current);
      await video.play().catch(() => {});
    }

    if (!stateMessage.playing && !video.paused) {
      video.pause();
    }

    window.setTimeout(() => {
      applyingRemoteRef.current = false;
    }, 60);
  }, [hlsAvailableWindowDurationSecs, hlsSessionStartOffsetSecs, hlsTargetHeight, startHls]);

  useEffect(() => {
    applyRemoteStateRef.current = applyRemoteState;
  }, [applyRemoteState]);

  useEffect(() => {
    if (!room || !joinedRole || !isVideoRoom) return;
    if (!room.item_id || room.item_id.trim().length === 0) {
      setDescriptor(null);
      setMediaInfo(null);
      return;
    }

    let cancelled = false;

    apiJson<PlaybackDescriptor>(`/items/${room.item_id}/playback`)
      .then((data) => {
        if (cancelled) return;
        setDescriptor(data);
        void apiJson<MediaInfo>(data.media_info_url)
          .then((info) => {
            if (!cancelled) {
              setMediaInfo(info);
            }
          })
          .catch(() => {
            // Media info fetch is best-effort and only used for duration stabilization.
          });
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
    const video = videoRef.current;
    if (!video) return;
    const syncAudioState = () => {
      audioStateRef.current = readVideoAudioState(video);
    };
    syncAudioState();
    video.addEventListener('volumechange', syncAudioState);
    video.addEventListener('loadedmetadata', syncAudioState);
    return () => {
      video.removeEventListener('volumechange', syncAudioState);
      video.removeEventListener('loadedmetadata', syncAudioState);
    };
  }, [room?.item_id]);

  useEffect(() => {
    if (!room || !joinedRole || !isVideoRoom || !descriptor) return;
    if (!room.item_id || room.item_id.trim().length === 0) return;
    if (startingHls) return;
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
      setInfo('HLS preload failed. Retry the stream manually.');
    })();
  }, [
    room,
    joinedRole,
    descriptor,
    isVideoRoom,
    startingHls,
    startHls,
    appendDebug,
    setInfo,
  ]);

  useEffect(() => {
    const hls = hlsRef.current as { __stopKnownDurationEnforcer?: () => void } | null;
    if (!hls) return;

    const nextDuration = Math.max(
      descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms / 1000 : 0,
      mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs : 0,
    );
    if (!Number.isFinite(nextDuration) || nextDuration <= 0) return;

    const previousStop = hls.__stopKnownDurationEnforcer;
    if (typeof previousStop === 'function') {
      try {
        previousStop();
      } catch {
        // no-op
      }
    }

    const stop = installKnownDurationEnforcer(hls, nextDuration);
    hls.__stopKnownDurationEnforcer = stop;
    applyKnownDurationToHlsMediaSource(hls, nextDuration);

    return () => {
      if (hls.__stopKnownDurationEnforcer === stop) {
        try {
          stop();
        } catch {
          // no-op
        }
      }
    };
  }, [descriptor?.duration_ms, mediaInfo?.duration_secs]);

  const sourceVideoHeight =
    mediaInfo?.video?.height && mediaInfo.video.height > 0 ? mediaInfo.video.height : null;

  useEffect(() => {
    const normalized = normalizePlaybackQualitySelection(hlsTargetHeight, sourceVideoHeight);
    if (normalized !== hlsTargetHeight) {
      setHlsTargetHeight(normalized);
    }
  }, [hlsTargetHeight, sourceVideoHeight]);

  useEffect(() => {
    return () => {
      destroyHls();
      if (sessionIdRef.current) {
        void stopSession(sessionIdRef.current);
        sessionIdRef.current = null;
      }
    };
  }, [destroyHls, stopSession]);

  const knownDurationMs = Math.floor(
    Math.max(
      descriptor?.duration_ms && descriptor.duration_ms > 0 ? descriptor.duration_ms : 0,
      mediaInfo?.duration_secs && mediaInfo.duration_secs > 0 ? mediaInfo.duration_secs * 1000 : 0,
    ),
  );

  return {
    descriptor,
    knownDurationMs,
    startingHls,
    hlsTargetHeight,
    hlsSessionStartOffsetSecs,
    hlsAvailableWindowDurationSecs,
    sourceVideoHeight,
    setHlsTargetHeight,
    isVideoRoom,
    videoRef,
    applyingRemoteRef,
    applyRemoteState,
    startHls,
    resetPlaybackState,
  };
}
