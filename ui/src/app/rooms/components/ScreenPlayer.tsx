'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type {
  WsScreenAnswerMessage,
  WsScreenIceMessage,
  WsScreenOfferMessage,
  WsScreenStateMessage,
} from '@/lib/watchPartyApi';
import { isAdminRole } from '@/lib/watchPartyRoles';

const STUN_URL = process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302';

type ScreenSignalEvent =
  | WsScreenOfferMessage
  | WsScreenAnswerMessage
  | WsScreenIceMessage
  | null;

type PresenterPhase = 'idle' | 'requesting_capture' | 'starting' | 'live' | 'ended' | 'error';
type QualityProfile = 'auto' | 'text_clarity' | 'motion';
type ObjectFitMode = 'contain' | 'cover';
type PendingCaptureMode = 'start' | 'replace' | null;

type Props = {
  roomId: string;
  currentUserId: string;
  joinedRole: string;
  wsConnected: boolean;
  screenState: WsScreenStateMessage | null;
  screenSignalEvent: ScreenSignalEvent;
  sendWs: (payload: Record<string, unknown>) => boolean;
  setError: (message: string) => void;
};

function createPeerConfig(): RTCConfiguration {
  return {
    iceServers: [{ urls: STUN_URL }],
  };
}

function clampVolume(value: number): number {
  if (!Number.isFinite(value)) return 1;
  if (value < 0) return 0;
  if (value > 1) return 1;
  return value;
}

function normalizeDisplaySurface(raw: string | undefined): 'browser' | 'window' | 'monitor' | 'unknown' {
  if (raw === 'browser' || raw === 'window' || raw === 'monitor') {
    return raw;
  }
  return 'unknown';
}

function isLikelySharedAudioAvailable(): boolean {
  if (typeof navigator === 'undefined') return false;
  const ua = navigator.userAgent.toLowerCase();
  const isChromium =
    ua.includes('chrome') || ua.includes('chromium') || ua.includes('edg') || ua.includes('brave');
  const isMobile = ua.includes('iphone') || ua.includes('ipad') || ua.includes('android');
  return isChromium && !isMobile;
}

function isSilentCaptureError(err: unknown): boolean {
  return err instanceof Error && (err.name === 'NotAllowedError' || err.name === 'AbortError');
}

function applyTrackHints(stream: MediaStream, qualityProfile: QualityProfile): void {
  const videoTrack = stream.getVideoTracks()[0];
  if (!videoTrack) return;
  if (qualityProfile === 'motion') {
    videoTrack.contentHint = 'motion';
  } else if (qualityProfile === 'text_clarity') {
    videoTrack.contentHint = 'detail';
  } else {
    videoTrack.contentHint = '';
  }
}

function isCapturePending(phase: PresenterPhase): boolean {
  return phase === 'requesting_capture' || phase === 'starting';
}

export default function ScreenPlayer({
  roomId,
  currentUserId,
  joinedRole,
  wsConnected,
  screenState,
  screenSignalEvent,
  sendWs,
  setError,
}: Props) {
  const canPresent = isAdminRole(joinedRole);
  const isPresenter = screenState?.presenter_user_id === currentUserId;
  const isViewer = Boolean(screenState?.active && screenState.presenter_user_id !== currentUserId);
  const shareLockedByOther = Boolean(screenState?.presenter_user_id && screenState.presenter_user_id !== currentUserId);
  const lockOwnerLabel = screenState?.presenter_username || screenState?.presenter_user_id || 'another presenter';

  const [presenterPhase, setPresenterPhase] = useState<PresenterPhase>('idle');
  const [qualityProfile, setQualityProfile] = useState<QualityProfile>('auto');
  const [requestAudio, setRequestAudio] = useState(isLikelySharedAudioAvailable());
  const [viewerVolume, setViewerVolume] = useState(1);
  const [objectFitMode, setObjectFitMode] = useState<ObjectFitMode>('contain');
  const [autoplayBlocked, setAutoplayBlocked] = useState(false);
  const [claimPending, setClaimPending] = useState(false);

  const localVideoRef = useRef<HTMLVideoElement | null>(null);
  const remoteVideoRef = useRef<HTMLVideoElement | null>(null);
  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const localStreamRef = useRef<MediaStream | null>(null);
  const remoteStreamRef = useRef<MediaStream | null>(null);
  const viewerPeerRef = useRef<RTCPeerConnection | null>(null);
  const presenterPeersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const viewerSessionIdRef = useRef<string | null>(null);
  const localCleanupRef = useRef(false);
  const stopReasonRef = useRef<'manual' | 'remote' | null>(null);
  const presenterPhaseRef = useRef<PresenterPhase>('idle');
  const presenterSessionIdRef = useRef<string | null>(null);
  const presenterUserIdRef = useRef<string | null>(null);
  const pendingCaptureModeRef = useRef<PendingCaptureMode>(null);

  const showLocalPreview = isPresenter || isCapturePending(presenterPhase);

  const preflightIssues = useMemo(() => {
    const issues: string[] = [];
    if (typeof window === 'undefined' || !window.isSecureContext) {
      issues.push('Screen sharing requires HTTPS or localhost.');
    }
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.getDisplayMedia) {
      issues.push('This browser does not support screen capture.');
    }
    if (!canPresent) {
      issues.push('Only the host or controllers can present a screen in this room.');
    }
    if (!wsConnected) {
      issues.push('Realtime connection is offline.');
    }
    return issues;
  }, [canPresent, wsConnected]);

  const closeViewerPeer = useCallback(() => {
    viewerSessionIdRef.current = null;
    if (viewerPeerRef.current) {
      try {
        viewerPeerRef.current.close();
      } catch {
        // no-op
      }
      viewerPeerRef.current = null;
    }
    remoteStreamRef.current = null;
    const remoteVideo = remoteVideoRef.current;
    if (remoteVideo) {
      remoteVideo.pause();
      remoteVideo.srcObject = null;
    }
    setAutoplayBlocked(false);
  }, []);

  const closePresenterPeers = useCallback(() => {
    presenterPeersRef.current.forEach((peer) => {
      try {
        peer.close();
      } catch {
        // no-op
      }
    });
    presenterPeersRef.current.clear();
  }, []);

  const resetLocalCaptureForReplace = useCallback(() => {
    closePresenterPeers();

    const stream = localStreamRef.current;
    localStreamRef.current = null;
    if (stream) {
      stream.getTracks().forEach((track) => {
        track.onended = null;
        track.stop();
      });
    }

    const localVideo = localVideoRef.current;
    if (localVideo) {
      localVideo.pause();
      localVideo.srcObject = null;
    }
  }, [closePresenterPeers]);

  const releaseLocalShare = useCallback(
    (notifyRoom: boolean) => {
      if (localCleanupRef.current) return;
      localCleanupRef.current = true;

      closePresenterPeers();

      const stream = localStreamRef.current;
      localStreamRef.current = null;
      if (stream) {
        stream.getTracks().forEach((track) => {
          track.onended = null;
          track.stop();
        });
      }

      const localVideo = localVideoRef.current;
      if (localVideo) {
        localVideo.pause();
        localVideo.srcObject = null;
      }

      if (notifyRoom && screenState?.active && screenState.presenter_user_id === currentUserId) {
        sendWs({ type: 'screen_stop' });
      } else if (notifyRoom && !screenState?.active && screenState?.presenter_user_id === currentUserId) {
        sendWs({ type: 'screen_release' });
      }

      setClaimPending(false);
      setPresenterPhase((prev) => (prev === 'error' ? prev : 'ended'));
      window.setTimeout(() => {
        localCleanupRef.current = false;
      }, 0);
    },
    [closePresenterPeers, currentUserId, screenState?.active, screenState?.presenter_user_id, sendWs],
  );

  const attemptRemotePlayback = useCallback(() => {
    const video = remoteVideoRef.current;
    if (!video) return;
    const playPromise = video.play();
    if (!playPromise) return;
    void playPromise.then(
      () => setAutoplayBlocked(false),
      () => setAutoplayBlocked(true),
    );
  }, []);

  useEffect(() => {
    const video = remoteVideoRef.current;
    if (!video) return;
    video.volume = clampVolume(viewerVolume);
    video.muted = viewerVolume <= 0;
  }, [viewerVolume]);

  useEffect(() => {
    presenterPhaseRef.current = presenterPhase;
  }, [presenterPhase]);

  useEffect(() => {
    if (screenState?.active) {
      presenterSessionIdRef.current = screenState.session_id ?? presenterSessionIdRef.current;
      presenterUserIdRef.current = screenState.presenter_user_id ?? null;
      if (screenState.presenter_user_id === currentUserId) {
        pendingCaptureModeRef.current = null;
      }
      return;
    }

    presenterSessionIdRef.current = null;
    presenterUserIdRef.current = null;
    pendingCaptureModeRef.current = null;
  }, [currentUserId, screenState?.active, screenState?.presenter_user_id, screenState?.session_id]);

  useEffect(() => {
    const desiredQuality =
      screenState?.quality_profile === 'text_clarity' || screenState?.quality_profile === 'motion'
        ? screenState.quality_profile
        : 'auto';
    setQualityProfile(desiredQuality);
  }, [screenState?.quality_profile]);

  useEffect(() => {
    const stream = localStreamRef.current;
    if (!stream) return;
    applyTrackHints(stream, qualityProfile);
  }, [qualityProfile]);

  useEffect(() => {
    const video = localVideoRef.current;
    const stream = localStreamRef.current;
    if (!video || !stream) return;
    if (video.srcObject !== stream) {
      video.srcObject = stream;
    }
    video.muted = true;
    void video.play().catch(() => {});
  }, [isPresenter, presenterPhase]);

  useEffect(() => {
    if (screenState?.active && isPresenter) {
      setClaimPending(false);
      setPresenterPhase('live');
      stopReasonRef.current = null;
      return;
    }
    if (!screenState?.active && localStreamRef.current) {
      releaseLocalShare(false);
      stopReasonRef.current = null;
    }
  }, [isPresenter, releaseLocalShare, screenState?.active]);

  const releasePresenterClaim = useCallback(() => {
    if (screenState?.presenter_user_id === currentUserId && !screenState.active) {
      sendWs({ type: 'screen_release' });
    }
  }, [currentUserId, screenState?.active, screenState?.presenter_user_id, sendWs]);

  useEffect(() => {
    if (!screenState?.active || !screenState.session_id || !screenState.presenter_user_id || !isViewer) {
      closeViewerPeer();
      return;
    }
    if (!wsConnected) {
      return;
    }
    if (viewerSessionIdRef.current === screenState.session_id && viewerPeerRef.current) {
      return;
    }

    closeViewerPeer();

    const peer = new RTCPeerConnection(createPeerConfig());
    const remoteStream = new MediaStream();
    viewerPeerRef.current = peer;
    viewerSessionIdRef.current = screenState.session_id;
    remoteStreamRef.current = remoteStream;

    peer.addTransceiver('video', { direction: 'recvonly' });
    if (screenState.audio_enabled) {
      peer.addTransceiver('audio', { direction: 'recvonly' });
    }

    peer.ontrack = (event) => {
      event.streams[0]?.getTracks().forEach((track) => {
        if (!remoteStream.getTracks().find((existing) => existing.id === track.id)) {
          remoteStream.addTrack(track);
        }
      });
      const video = remoteVideoRef.current;
      if (video) {
        video.srcObject = remoteStream;
        attemptRemotePlayback();
      }
    };

    peer.onicecandidate = (event) => {
      if (!event.candidate || !screenState.session_id) return;
      sendWs({
        type: 'screen_ice',
        to_user_id: screenState.presenter_user_id,
        session_id: screenState.session_id,
        candidate: event.candidate.candidate,
      });
    };

    void (async () => {
      try {
        const offer = await peer.createOffer();
        await peer.setLocalDescription(offer);
        if (!offer.sdp) {
          throw new Error('Failed to create a viewer offer.');
        }
        sendWs({
          type: 'screen_offer',
          to_user_id: screenState.presenter_user_id,
          session_id: screenState.session_id,
          sdp: offer.sdp,
        });
      } catch (err) {
        closeViewerPeer();
      }
    })();

    return () => {
      if (viewerSessionIdRef.current === screenState.session_id) {
        closeViewerPeer();
      }
    };
  }, [
    attemptRemotePlayback,
    closeViewerPeer,
    isViewer,
    screenState?.active,
    screenState?.audio_enabled,
    screenState?.presenter_user_id,
    screenState?.session_id,
    sendWs,
    setError,
    wsConnected,
  ]);

  const handleIncomingOffer = useCallback(
    async (message: WsScreenOfferMessage) => {
      const localStream = localStreamRef.current;
      if (!localStream) {
        return;
      }

      const knownSessionId = presenterSessionIdRef.current;
      const knownPresenterUserId = presenterUserIdRef.current;
      const pendingMode = pendingCaptureModeRef.current;
      const phase = presenterPhaseRef.current;
      const isKnownPresenter = knownPresenterUserId === currentUserId;

      let acceptedSessionId: string | null = knownSessionId;
      if (pendingMode === 'replace' && message.session_id === knownSessionId) {
        // Ignore stale offers from the previous session while a replacement capture is starting.
        return;
      }
      if (!isKnownPresenter) {
        if (!isCapturePending(phase)) {
          return;
        }
        acceptedSessionId = message.session_id;
        presenterSessionIdRef.current = message.session_id;
        presenterUserIdRef.current = currentUserId;
        pendingCaptureModeRef.current = null;
      } else if (pendingMode === 'replace') {
        acceptedSessionId = message.session_id;
        presenterSessionIdRef.current = message.session_id;
        pendingCaptureModeRef.current = null;
      } else if (!acceptedSessionId || message.session_id !== acceptedSessionId) {
        return;
      }

      const existing = presenterPeersRef.current.get(message.from_user_id);
      if (existing) {
        try {
          existing.close();
        } catch {
          // no-op
        }
      }

      const peer = new RTCPeerConnection(createPeerConfig());
      presenterPeersRef.current.set(message.from_user_id, peer);
      localStream.getTracks().forEach((track) => {
        peer.addTrack(track, localStream);
      });

      peer.onicecandidate = (event) => {
        if (!event.candidate || !acceptedSessionId) return;
        sendWs({
          type: 'screen_ice',
          to_user_id: message.from_user_id,
          session_id: acceptedSessionId,
          candidate: event.candidate.candidate,
        });
      };

      peer.onconnectionstatechange = () => {
        if (
          peer.connectionState === 'failed' ||
          peer.connectionState === 'closed' ||
          peer.connectionState === 'disconnected'
        ) {
          presenterPeersRef.current.delete(message.from_user_id);
        }
      };

      try {
        await peer.setRemoteDescription({ type: 'offer', sdp: message.sdp });
        const answer = await peer.createAnswer();
        await peer.setLocalDescription(answer);
        if (!answer.sdp) {
          throw new Error('Failed to create a presenter answer.');
        }
        sendWs({
          type: 'screen_answer',
          to_user_id: message.from_user_id,
          session_id: acceptedSessionId,
          sdp: answer.sdp,
        });
      } catch (err) {
        presenterPeersRef.current.delete(message.from_user_id);
        try {
          peer.close();
        } catch {
          // no-op
        }
      }
    },
    [currentUserId, sendWs],
  );

  const handleIncomingAnswer = useCallback(
    async (message: WsScreenAnswerMessage) => {
      const peer = viewerPeerRef.current;
      if (!peer || viewerSessionIdRef.current !== message.session_id) {
        return;
      }
      try {
        await peer.setRemoteDescription({ type: 'answer', sdp: message.sdp });
      } catch {}
    },
    [],
  );

  const handleIncomingIce = useCallback(
    async (message: WsScreenIceMessage) => {
      try {
        if (presenterUserIdRef.current === currentUserId) {
          const peer = presenterPeersRef.current.get(message.from_user_id);
          if (!peer || presenterSessionIdRef.current !== message.session_id) {
            return;
          }
          await peer.addIceCandidate({ candidate: message.candidate });
          return;
        }

        const peer = viewerPeerRef.current;
        if (!peer || viewerSessionIdRef.current !== message.session_id) {
          return;
        }
        await peer.addIceCandidate({ candidate: message.candidate });
      } catch {
        // Browsers may emit ICE after teardown while peers are closing.
      }
    },
    [currentUserId],
  );

  useEffect(() => {
    if (!screenSignalEvent) return;
    if (screenSignalEvent.to_user_id !== currentUserId) return;

    if (screenSignalEvent.type === 'screen_offer') {
      void handleIncomingOffer(screenSignalEvent);
      return;
    }
    if (screenSignalEvent.type === 'screen_answer') {
      void handleIncomingAnswer(screenSignalEvent);
      return;
    }
    void handleIncomingIce(screenSignalEvent);
  }, [currentUserId, handleIncomingAnswer, handleIncomingIce, handleIncomingOffer, screenSignalEvent]);

  useEffect(() => {
    return () => {
      closeViewerPeer();
      closePresenterPeers();
      const stream = localStreamRef.current;
      localStreamRef.current = null;
      if (stream) {
        stream.getTracks().forEach((track) => {
          track.onended = null;
          track.stop();
        });
      }
    };
  }, [closePresenterPeers, closeViewerPeer]);

  const startCaptureSession = useCallback(
    async (replace: boolean) => {
      const replacingLiveShare = replace && Boolean(localStreamRef.current || screenState?.active);
      if (replace) {
        resetLocalCaptureForReplace();
      }

      setPresenterPhase('requesting_capture');
      setError('');

      try {
        const stream = await navigator.mediaDevices.getDisplayMedia({
          video: qualityProfile === 'motion'
            ? { frameRate: { ideal: 30, max: 60 } }
            : { frameRate: { ideal: 15, max: 30 } },
          audio: requestAudio,
        });

        const videoTrack = stream.getVideoTracks()[0];
        if (!videoTrack) {
          stream.getTracks().forEach((track) => track.stop());
          throw new Error('No video track was returned by screen capture.');
        }

        if (localStreamRef.current) {
          resetLocalCaptureForReplace();
        }

        stream.getTracks().forEach((track) => {
          track.onended = () => {
            stopReasonRef.current = 'manual';
            releaseLocalShare(true);
          };
        });

        applyTrackHints(stream, qualityProfile);
        localStreamRef.current = stream;
        closePresenterPeers();

        const localVideo = localVideoRef.current;
        if (localVideo) {
          localVideo.srcObject = stream;
          localVideo.muted = true;
          localVideo.play().catch(() => {});
        }

        const surfaceType = normalizeDisplaySurface(
          (videoTrack.getSettings() as MediaTrackSettings & { displaySurface?: string }).displaySurface,
        );
        const audioEnabled = stream.getAudioTracks().length > 0;
        setPresenterPhase('starting');
        pendingCaptureModeRef.current = replace || Boolean(screenState?.active) ? 'replace' : 'start';
        const sent = sendWs({
          type: replace || screenState?.active ? 'screen_replace' : 'screen_start',
          surface_type: surfaceType,
          audio_enabled: audioEnabled,
          quality_profile: qualityProfile,
        });
        if (!sent) {
          pendingCaptureModeRef.current = null;
          resetLocalCaptureForReplace();
          if (!replace) {
            releasePresenterClaim();
          }
          throw new Error('Failed to send the screen-share start command.');
        }
      } catch (err) {
        pendingCaptureModeRef.current = null;
        if (!replace) {
          releasePresenterClaim();
        }
        if (replacingLiveShare) {
          stopReasonRef.current = 'manual';
          sendWs({ type: 'screen_stop' });
          setPresenterPhase('ended');
          return;
        }
        setPresenterPhase(isSilentCaptureError(err) ? 'idle' : 'error');
      }
    },
    [qualityProfile, releasePresenterClaim, requestAudio, resetLocalCaptureForReplace, screenState?.active, sendWs, setError],
  );

  useEffect(() => {
    if (!claimPending) return;
    if (
      screenState?.presenter_user_id === currentUserId &&
      !screenState.active &&
      screenState.presenter_state === 'requesting_capture'
    ) {
      setClaimPending(false);
      void startCaptureSession(false);
      return;
    }
    if (screenState?.presenter_user_id && screenState.presenter_user_id !== currentUserId) {
      setClaimPending(false);
    }
  }, [
    claimPending,
    currentUserId,
    screenState?.active,
    screenState?.presenter_state,
    screenState?.presenter_user_id,
    startCaptureSession,
  ]);

  const beginCapture = useCallback(
    (replace: boolean) => {
      if (preflightIssues.length > 0) {
        setPresenterPhase('error');
        return;
      }

      if (replace || isPresenter || screenState?.presenter_user_id === currentUserId) {
        setClaimPending(false);
        void startCaptureSession(replace);
        return;
      }

      if (shareLockedByOther) {
        setPresenterPhase('error');
        return;
      }

      setClaimPending(true);
      setError('');
      const sent = sendWs({ type: 'screen_claim' });
      if (!sent) {
        setClaimPending(false);
        setPresenterPhase('error');
      }
    },
    [
      currentUserId,
      isPresenter,
      lockOwnerLabel,
      preflightIssues,
      screenState?.presenter_user_id,
      sendWs,
      setError,
      shareLockedByOther,
      startCaptureSession,
    ],
  );

  const handleStopSharing = useCallback(() => {
    stopReasonRef.current = 'manual';
    releaseLocalShare(true);
  }, [releaseLocalShare]);

  const handleQualityChange = useCallback(
    (nextQuality: QualityProfile) => {
      setQualityProfile(nextQuality);
      const stream = localStreamRef.current;
      if (stream) {
        applyTrackHints(stream, nextQuality);
      }
      if (isPresenter && screenState?.active) {
        sendWs({ type: 'screen_quality', quality_profile: nextQuality });
      }
    },
    [isPresenter, screenState?.active, sendWs],
  );

  const handleToggleFullscreen = useCallback(() => {
    if (!surfaceRef.current) return;
    if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => {});
      return;
    }
    void surfaceRef.current.requestFullscreen().catch(() => {});
  }, []);

  const handleViewerModeChange = useCallback(
    (value: 'fit' | 'fill' | 'fullscreen') => {
      if (value === 'fullscreen') {
        handleToggleFullscreen();
        return;
      }
      setObjectFitMode(value === 'fill' ? 'cover' : 'contain');
    },
    [handleToggleFullscreen],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="space-y-1">
          <h2 className="text-xl font-semibold">Screen Share</h2>
          <p className="text-sm muted">
            Share a browser tab, window, or full display using the native picker.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span className="chip text-xs">
            Presenter: {screenState?.presenter_username || screenState?.presenter_user_id || 'None'}
          </span>
          <span className="chip text-xs">
            Viewers: {screenState?.viewer_count ?? 0}
          </span>
          {screenState?.presenter_user_id && !screenState.active && (
            <span className="chip text-xs">
              Locked by {screenState.presenter_username || screenState.presenter_user_id}
            </span>
          )}
          {screenState?.audio_enabled && <span className="chip text-xs">Shared Audio</span>}
        </div>
      </div>

      <div
        ref={surfaceRef}
        className="relative overflow-hidden rounded-2xl border border-[var(--border)] bg-black/50"
      >
        {showLocalPreview ? (
          <video
            ref={localVideoRef}
            autoPlay
            muted
            playsInline
            className="aspect-video max-h-[70vh] w-full bg-black"
            style={{ objectFit: objectFitMode }}
          />
        ) : (
          <video
            ref={remoteVideoRef}
            autoPlay
            playsInline
            className="aspect-video max-h-[70vh] w-full bg-black"
            style={{ objectFit: objectFitMode }}
          />
        )}

        {!screenState?.active && !showLocalPreview && (
          <div className="absolute inset-0 flex items-center justify-center px-6 text-center text-sm muted">
            {shareLockedByOther
              ? `${lockOwnerLabel} is choosing a screen to share.`
              : canPresent
              ? 'No one is sharing right now. Start a screen share to present to the room.'
              : 'Waiting for a host or controller to start sharing a screen.'}
          </div>
        )}

        {showLocalPreview && isCapturePending(presenterPhase) && (
          <div className="absolute inset-0 flex items-center justify-center px-6 text-center text-sm muted">
            {presenterPhase === 'requesting_capture'
              ? 'Choose a tab, window, or screen to start presenting.'
              : 'Starting screen share…'}
          </div>
        )}

        {screenState?.active && isViewer && !remoteStreamRef.current && (
          <div className="absolute inset-0 flex items-center justify-center px-6 text-center text-sm muted">
            Connecting to the presenter’s screen…
          </div>
        )}
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
        <section className="panel-soft space-y-4 rounded-2xl px-4 py-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="text-sm font-semibold">Presenter Controls</p>
              <p className="text-xs muted">
                {shareLockedByOther ? `Locked by ${lockOwnerLabel}` : `Phase: ${presenterPhase}`}
                {screenState?.surface_type ? ` · Source: ${screenState.surface_type}` : ''}
              </p>
            </div>
            {screenState?.active && isPresenter && (
              <span className="chip text-xs">
                Live session {screenState.session_id?.slice(0, 8) || 'pending'}
              </span>
            )}
          </div>

          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              onClick={() => beginCapture(false)}
              disabled={
                !canPresent ||
                shareLockedByOther ||
                claimPending ||
                presenterPhase === 'requesting_capture' ||
                presenterPhase === 'starting'
              }
            >
              {screenState?.active && isPresenter
                ? 'Sharing…'
                : claimPending
                  ? 'Locking…'
                  : shareLockedByOther
                    ? `Locked by ${lockOwnerLabel}`
                    : 'Share screen'}
            </button>
            <button
              type="button"
              className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
              onClick={() => beginCapture(true)}
              disabled={!canPresent || !isPresenter || !screenState?.active}
            >
              Change shared item
            </button>
            <button
              type="button"
              className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
              onClick={handleStopSharing}
              disabled={!isPresenter || !screenState?.active}
            >
              Stop sharing
            </button>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Quality</span>
              <select
                className="select w-full px-3 py-2"
                value={qualityProfile}
                onChange={(event) => handleQualityChange(event.target.value as QualityProfile)}
              >
                <option value="auto">Auto</option>
                <option value="text_clarity">Text clarity</option>
                <option value="motion">Motion</option>
              </select>
            </label>

            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Shared Audio</span>
              <span className="flex items-center gap-2 rounded-xl border border-white/10 bg-black/10 px-3 py-2">
                <input
                  type="checkbox"
                  checked={requestAudio}
                  onChange={(event) => setRequestAudio(event.target.checked)}
                />
                <span className="text-sm">Request audio when the browser supports it</span>
              </span>
            </label>
          </div>

          {preflightIssues.length > 0 && canPresent && (
            <div className="space-y-1 rounded-xl border border-amber-400/30 bg-amber-500/10 px-3 py-3 text-xs text-amber-100">
              {preflightIssues.map((issue) => (
                <p key={issue}>{issue}</p>
              ))}
            </div>
          )}
        </section>

        <section className="panel-soft space-y-4 rounded-2xl px-4 py-4">
          <p className="text-sm font-semibold">Viewer Controls</p>

          <label className="block text-sm">
            <span className="mb-1 block text-xs uppercase tracking-wide muted">View</span>
            <select
              className="select w-full px-3 py-2"
              value={objectFitMode === 'contain' ? 'fit' : 'fill'}
              onChange={(event) =>
                handleViewerModeChange(event.target.value as 'fit' | 'fill' | 'fullscreen')
              }
            >
              <option value="fit">Fit</option>
              <option value="fill">Fill</option>
              <option value="fullscreen">Fullscreen</option>
            </select>
          </label>

          {screenState?.audio_enabled && (
            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Volume</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={viewerVolume}
                onChange={(event) => setViewerVolume(Number(event.target.value))}
                className="rf-gradient-slider w-full"
              />
            </label>
          )}

          {autoplayBlocked && (
            <div className="space-y-2 rounded-xl border border-white/10 bg-black/10 px-3 py-3 text-sm muted">
              <p>Browser autoplay blocked the incoming shared screen audio.</p>
              <button
                type="button"
                className="btn-primary px-4 py-2 text-sm"
                onClick={attemptRemotePlayback}
              >
                Enable playback
              </button>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
