'use client';

import { useEffect, useRef } from 'react';
import type {
  ChannelEvent,
  UserInfo,
  VoiceTranscriptionRecordingUpload,
  VoiceTranscriptionTextUpload,
  VoiceTranscriptionState,
} from '@/lib/channelsApi';

interface Props {
  localStream: MediaStream | null;
  channelId: string;
  currentUserId: string;
  existingMembers: UserInfo[];
  wsEvents: Array<{
    seq: number;
    event: ChannelEvent;
  }>;
  sendWs: (msg: object) => void;
  iceServers: RTCIceServer[];
  deafened: boolean;
  remoteVolumes: Record<string, number>;
  localMicGain: number;
  preferredOutputDeviceId: string | null;
  onSpeakingChange: (channelId: string, userId: string, speaking: boolean) => void;
  onPeerConnectionStateChange: (
    channelId: string,
    userId: string,
    state: PeerConnectionUiState | null,
  ) => void;
  transcriptionState: VoiceTranscriptionState | null;
  onTranscriptionRecordingUpload: (
    channelId: string,
    payload: VoiceTranscriptionRecordingUpload,
  ) => Promise<void>;
  onTranscriptionTextUpload: (
    channelId: string,
    payload: VoiceTranscriptionTextUpload,
  ) => Promise<void>;
}

type ChannelSpeechRecognitionResult = {
  isFinal: boolean;
  0: {
    transcript: string;
  };
  length: number;
};

type ChannelSpeechRecognitionEvent = {
  results: ArrayLike<ChannelSpeechRecognitionResult>;
};

type ChannelSpeechRecognitionErrorEvent = {
  error: string;
};

type ChannelSpeechRecognition = {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  maxAlternatives: number;
  onresult: ((event: ChannelSpeechRecognitionEvent) => void) | null;
  onerror: ((event: ChannelSpeechRecognitionErrorEvent) => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
};

type ChannelSpeechRecognitionConstructor = new () => ChannelSpeechRecognition;

// Per-peer connection state surfaced to the UI so it can show a
// "reconnecting…/couldn't connect" indicator instead of silent dead audio.
export type PeerConnectionUiState =
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'failed';

const SPEAKING_SAMPLE_INTERVAL_MS = 120;
const SPEAKING_RMS_THRESHOLD = 0.03;
const SPEAKING_HANG_MS = 300;
const SYNTHETIC_OUTPUT_PREFIX = 'synthetic-audiooutput-';

// ── ICE / connection recovery tuning (AUD-1) ────────────────────────────────
// Grace period after a peer goes `disconnected` before we treat it as needing
// recovery. Transient network blips often self-heal back to `connected`.
const PEER_DISCONNECT_GRACE_MS = 4000;
// Debounce so a burst of `failed`/`disconnected` transitions only triggers one
// recovery attempt.
const PEER_RECOVERY_DEBOUNCE_MS = 800;
// Cap recovery attempts per peer within a window so a permanently broken link
// (e.g. no reachable TURN) can't spin forever.
const PEER_MAX_RECOVERY_ATTEMPTS = 5;
// After this quiet period (no further failures) the recovery attempt counter
// resets, so a peer that later breaks again gets a fresh budget.
const PEER_RECOVERY_ATTEMPT_RESET_MS = 30_000;

function collectBrowserTranscript(event: ChannelSpeechRecognitionEvent): string {
  const transcripts: string[] = [];
  for (let index = 0; index < event.results.length; index += 1) {
    const result = event.results[index];
    const transcript = result?.[0]?.transcript?.trim();
    if (transcript) {
      transcripts.push(transcript);
    }
  }
  return transcripts.join(' ').trim();
}

type SpeakingMonitor = {
  source: MediaStreamAudioSourceNode;
  analyser: AnalyserNode;
  buffer: Uint8Array;
  timerId: number;
  speaking: boolean;
  lastLoudAtMs: number;
};

type RemoteAudioPipeline = {
  streamId: string;
  source: MediaStreamAudioSourceNode;
  gain: GainNode;
  destination: MediaStreamAudioDestinationNode;
};

function createPeerConfig(iceServers: RTCIceServer[]): RTCConfiguration {
  return {
    iceServers,
  };
}

function isAudioTransceiver(transceiver: RTCRtpTransceiver): boolean {
  return transceiver.receiver.track.kind === 'audio';
}

export default function VoiceEngine({
  localStream,
  channelId,
  currentUserId,
  existingMembers,
  wsEvents,
  sendWs,
  iceServers,
  deafened,
  remoteVolumes,
  localMicGain,
  preferredOutputDeviceId,
  onSpeakingChange,
  onPeerConnectionStateChange,
  transcriptionState,
  onTranscriptionRecordingUpload,
  onTranscriptionTextUpload,
}: Props) {
  const peersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const audioElementsRef = useRef<Map<string, HTMLAudioElement>>(new Map());
  const remoteAudioPipelinesRef = useRef<Map<string, RemoteAudioPipeline>>(new Map());
  const speakingMonitorsRef = useRef<Map<string, SpeakingMonitor>>(new Map());
  const pendingIceCandidatesRef = useRef<Map<string, RTCIceCandidateInit[]>>(new Map());
  // ── Recovery bookkeeping (AUD-1) ──────────────────────────────────────────
  // Per-peer recovery state, keyed by remote userId. Survives pc rebuilds
  // because it is indexed by userId, not by the RTCPeerConnection instance.
  const peerRecoveryRef = useRef<
    Map<
      string,
      {
        graceTimerId: number | null;
        debounceTimerId: number | null;
        attempts: number;
        lastAttemptAtMs: number;
        uiState: PeerConnectionUiState | null;
      }
    >
  >(new Map());
  // Latest iceServers and state callback, mirrored into refs so the long-lived
  // pc.onconnectionstatechange closures and the late-iceServers refresh always
  // read current values instead of the values captured at peer construction.
  const iceServersRef = useRef<RTCIceServer[]>(iceServers);
  const onPeerConnectionStateChangeRef = useRef(onPeerConnectionStateChange);
  const processedEventSeqRef = useRef(0);
  const audioContextRef = useRef<AudioContext | null>(null);
  const localMicContextRef = useRef<AudioContext | null>(null);
  const localMicGainNodeRef = useRef<GainNode | null>(null);
  const localMicSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const localMicDestinationRef = useRef<MediaStreamAudioDestinationNode | null>(null);
  const localMicProcessedStreamRef = useRef<MediaStream | null>(null);
  const localMicInputTrackIdRef = useRef<string | null>(null);
  const transcriptionRecorderRef = useRef<MediaRecorder | null>(null);
  const transcriptionChunksRef = useRef<Blob[]>([]);
  const transcriptionCaptureStartedAtRef = useRef<number>(0);
  const activeTranscriptionSessionRef = useRef<string | null>(null);
  const transcriptionStopRequestedRef = useRef(false);
  const recognitionRef = useRef<ChannelSpeechRecognition | null>(null);
  const browserSpeechShouldContinueRef = useRef(false);
  const browserSpeechStartedAtRef = useRef<number>(0);
  const browserSpeechAccumulatedRef = useRef('');
  const browserSpeechCurrentRef = useRef('');
  const browserSpeechUploadedRef = useRef(false);

  function getAudioContext(): AudioContext | null {
    if (audioContextRef.current) {
      return audioContextRef.current;
    }
    const audioContextCtor =
      window.AudioContext ||
      (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!audioContextCtor) {
      return null;
    }
    audioContextRef.current = new audioContextCtor();
    return audioContextRef.current;
  }

  function preferredRecorderMimeType(): string {
    if (typeof MediaRecorder === 'undefined' || typeof MediaRecorder.isTypeSupported !== 'function') {
      return '';
    }
    const candidates = [
      'audio/webm;codecs=opus',
      'audio/webm',
      'audio/mp4',
      'audio/ogg;codecs=opus',
    ];
    for (const mimeType of candidates) {
      if (MediaRecorder.isTypeSupported(mimeType)) {
        return mimeType;
      }
    }
    return '';
  }

  function teardownTranscriptionCapture() {
    if (recognitionRef.current) {
      browserSpeechShouldContinueRef.current = false;
      recognitionRef.current.stop();
      return;
    }
    const recorder = transcriptionRecorderRef.current;
    if (recorder) {
      if (recorder.state !== 'inactive' && !transcriptionStopRequestedRef.current) {
        transcriptionStopRequestedRef.current = true;
        recorder.stop();
      }
      return;
    }
    transcriptionRecorderRef.current = null;
    transcriptionChunksRef.current = [];
    transcriptionCaptureStartedAtRef.current = 0;
    activeTranscriptionSessionRef.current = null;
    transcriptionStopRequestedRef.current = false;
    browserSpeechShouldContinueRef.current = false;
    browserSpeechStartedAtRef.current = 0;
    browserSpeechAccumulatedRef.current = '';
    browserSpeechCurrentRef.current = '';
    browserSpeechUploadedRef.current = false;
  }

  function stopSpeakingMonitor(userId: string) {
    const monitor = speakingMonitorsRef.current.get(userId);
    if (!monitor) return;
    window.clearInterval(monitor.timerId);
    monitor.source.disconnect();
    monitor.analyser.disconnect();
    speakingMonitorsRef.current.delete(userId);
    onSpeakingChange(channelId, userId, false);
  }

  function startSpeakingMonitor(userId: string, stream: MediaStream) {
    if (stream.getAudioTracks().length === 0) {
      onSpeakingChange(channelId, userId, false);
      return;
    }
    stopSpeakingMonitor(userId);

    const audioContext = getAudioContext();
    if (!audioContext) {
      return;
    }

    const source = audioContext.createMediaStreamSource(stream);
    const analyser = audioContext.createAnalyser();
    analyser.fftSize = 512;
    analyser.smoothingTimeConstant = 0.25;
    source.connect(analyser);
    const buffer = new Uint8Array(analyser.fftSize);

    const monitor: SpeakingMonitor = {
      source,
      analyser,
      buffer,
      timerId: 0,
      speaking: false,
      lastLoudAtMs: 0,
    };

    const tick = () => {
      analyser.getByteTimeDomainData(buffer);
      let sumSquares = 0;
      for (let i = 0; i < buffer.length; i++) {
        const normalized = (buffer[i] - 128) / 128;
        sumSquares += normalized * normalized;
      }
      const rms = Math.sqrt(sumSquares / buffer.length);
      const nowMs = performance.now();
      if (rms >= SPEAKING_RMS_THRESHOLD) {
        monitor.lastLoudAtMs = nowMs;
      }
      const nextSpeaking =
        rms >= SPEAKING_RMS_THRESHOLD || nowMs - monitor.lastLoudAtMs < SPEAKING_HANG_MS;
      if (nextSpeaking !== monitor.speaking) {
        monitor.speaking = nextSpeaking;
        onSpeakingChange(channelId, userId, nextSpeaking);
      }
    };

    monitor.timerId = window.setInterval(tick, SPEAKING_SAMPLE_INTERVAL_MS);
    speakingMonitorsRef.current.set(userId, monitor);
    void audioContext.resume().catch(() => {});
  }

  function ensureReceiveAudio(pc: RTCPeerConnection) {
    const hasAudioTransceiver = pc.getTransceivers().some(isAudioTransceiver);
    if (!hasAudioTransceiver) {
      pc.addTransceiver('audio', { direction: 'recvonly' });
    }
  }

  function shouldInitiatePeer(remoteUserId: string): boolean {
    return currentUserId.localeCompare(remoteUserId) < 0;
  }

  function teardownLocalMicPipeline() {
    localMicSourceRef.current?.disconnect();
    localMicGainNodeRef.current?.disconnect();
    localMicDestinationRef.current?.disconnect();
    if (localMicContextRef.current) {
      void localMicContextRef.current.close().catch(() => {});
    }
    localMicSourceRef.current = null;
    localMicGainNodeRef.current = null;
    localMicDestinationRef.current = null;
    localMicContextRef.current = null;
    localMicProcessedStreamRef.current = null;
    localMicInputTrackIdRef.current = null;
  }

  function getOutboundStream(): MediaStream | null {
    if (!localStream) return null;
    const inputTrack = localStream.getAudioTracks()[0];
    if (!inputTrack) return null;

    // Default to raw mic stream for reliability. This avoids browsers that
    // suspend WebAudio graphs before first user interaction, which can result
    // in silent outbound audio.
    if (Math.abs(localMicGain - 1) < 0.001) {
      teardownLocalMicPipeline();
      return localStream;
    }

    const hasValidPipeline =
      localMicProcessedStreamRef.current &&
      localMicInputTrackIdRef.current === inputTrack.id &&
      localMicContextRef.current &&
      localMicContextRef.current.state !== 'closed' &&
      localMicGainNodeRef.current;

    if (hasValidPipeline) {
      return localMicProcessedStreamRef.current;
    }

    teardownLocalMicPipeline();

    try {
      const audioContextCtor =
        window.AudioContext ||
        (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!audioContextCtor) {
        return localStream;
      }
      const context = new audioContextCtor();
      const source = context.createMediaStreamSource(localStream);
      const gainNode = context.createGain();
      const destination = context.createMediaStreamDestination();
      gainNode.gain.value = localMicGain;
      source.connect(gainNode);
      gainNode.connect(destination);
      localMicContextRef.current = context;
      localMicSourceRef.current = source;
      localMicGainNodeRef.current = gainNode;
      localMicDestinationRef.current = destination;
      localMicProcessedStreamRef.current = destination.stream;
      localMicInputTrackIdRef.current = inputTrack.id;
      void context.resume().catch(() => {});
      return destination.stream;
    } catch (err) {
      console.warn('VoiceEngine: mic gain pipeline unavailable, falling back to raw stream', err);
      teardownLocalMicPipeline();
      return localStream;
    }
  }

  function addLocalTracks(pc: RTCPeerConnection) {
    const outboundStream = getOutboundStream();
    if (!outboundStream) {
      ensureReceiveAudio(pc);
      return;
    }
    const existingAudioSender = pc
      .getSenders()
      .find((sender) => sender.track?.kind === 'audio');
    if (existingAudioSender) {
      ensureReceiveAudio(pc);
      return;
    }
    const audioTracks = outboundStream.getAudioTracks();
    if (audioTracks.length === 0) {
      ensureReceiveAudio(pc);
      return;
    }
    audioTracks.forEach((track) => pc.addTrack(track, outboundStream));
  }

  function syncOutboundAudioTrack() {
    const outboundStream = getOutboundStream();
    const outboundTrack = outboundStream?.getAudioTracks()[0] ?? null;

    peersRef.current.forEach((pc) => {
      const sender = pc.getSenders().find((candidate) => candidate.track?.kind === 'audio');
      if (!sender) {
        if (outboundTrack && outboundStream) {
          pc.addTrack(outboundTrack, outboundStream);
        } else {
          ensureReceiveAudio(pc);
        }
        return;
      }

      if ((sender.track?.id ?? null) === (outboundTrack?.id ?? null)) {
        return;
      }
      void sender.replaceTrack(outboundTrack).catch((err) => {
        console.warn('VoiceEngine: failed to replace outbound audio track', err);
      });
    });
  }

  function getPeerVolume(userId: string): number {
    const value = remoteVolumes[userId];
    if (!Number.isFinite(value)) return 1;
    return Math.min(2, Math.max(0, value));
  }

  function teardownRemoteAudioPipeline(userId: string) {
    const pipeline = remoteAudioPipelinesRef.current.get(userId);
    if (!pipeline) return;
    try {
      pipeline.source.disconnect();
    } catch {
      // no-op
    }
    try {
      pipeline.gain.disconnect();
    } catch {
      // no-op
    }
    try {
      pipeline.destination.disconnect();
    } catch {
      // no-op
    }
    remoteAudioPipelinesRef.current.delete(userId);
  }

  function applyPreferredOutputDevice(el: HTMLAudioElement) {
    const sinkCapable = el as HTMLAudioElement & {
      setSinkId?: (deviceId: string) => Promise<void>;
      sinkId?: string;
    };
    if (typeof sinkCapable.setSinkId !== 'function') {
      return;
    }
    const nextSinkId = preferredOutputDeviceId && preferredOutputDeviceId.trim()
      ? preferredOutputDeviceId.trim()
      : '';
    if (nextSinkId.startsWith(SYNTHETIC_OUTPUT_PREFIX)) {
      return;
    }
    const currentSinkId = sinkCapable.sinkId ?? '';
    if (currentSinkId === nextSinkId) {
      return;
    }
    void sinkCapable.setSinkId(nextSinkId).catch((err) => {
      console.warn('VoiceEngine: failed setting output audio device', err);
    });
  }

  function attachAudio(userId: string, stream: MediaStream) {
    let el = audioElementsRef.current.get(userId);
    if (!el) {
      el = document.createElement('audio');
      el.autoplay = true;
      el.setAttribute('playsinline', 'true');
      el.preload = 'auto';
      // Keep element effectively invisible while avoiding display:none autoplay quirks.
      el.style.position = 'fixed';
      el.style.width = '1px';
      el.style.height = '1px';
      el.style.opacity = '0';
      el.style.pointerEvents = 'none';
      el.style.left = '-9999px';
      el.style.top = '-9999px';
      document.body.appendChild(el);
      audioElementsRef.current.set(userId, el);
    }
    el.muted = deafened;
    applyPreferredOutputDevice(el);

    const context = getAudioContext();
    if (context) {
      let pipeline = remoteAudioPipelinesRef.current.get(userId);
      if (!pipeline || pipeline.streamId !== stream.id) {
        teardownRemoteAudioPipeline(userId);
        const source = context.createMediaStreamSource(stream);
        const gain = context.createGain();
        const destination = context.createMediaStreamDestination();
        gain.gain.value = getPeerVolume(userId);
        source.connect(gain);
        gain.connect(destination);
        pipeline = {
          streamId: stream.id,
          source,
          gain,
          destination,
        };
        remoteAudioPipelinesRef.current.set(userId, pipeline);
      } else {
        pipeline.gain.gain.value = getPeerVolume(userId);
      }
      el.volume = 1;
      el.srcObject = pipeline.destination.stream;
      void context.resume().catch(() => {});
    } else {
      el.volume = Math.min(1, getPeerVolume(userId));
      el.srcObject = stream;
    }

    const tryPlay = () => {
      const playPromise = el?.play();
      if (playPromise) {
        void playPromise.catch((err) => {
          console.warn('VoiceEngine: remote audio autoplay blocked until user interaction', err);
        });
      }
    };
    el.onloadedmetadata = tryPlay;
    tryPlay();
    startSpeakingMonitor(userId, stream);
  }

  // ── Recovery state machine helpers (AUD-1 / AUD-3) ─────────────────────────

  function getPeerRecovery(userId: string) {
    let entry = peerRecoveryRef.current.get(userId);
    if (!entry) {
      entry = {
        graceTimerId: null,
        debounceTimerId: null,
        attempts: 0,
        lastAttemptAtMs: 0,
        uiState: null,
      };
      peerRecoveryRef.current.set(userId, entry);
    }
    return entry;
  }

  function clearPeerRecoveryTimers(userId: string) {
    const entry = peerRecoveryRef.current.get(userId);
    if (!entry) return;
    if (entry.graceTimerId !== null) {
      window.clearTimeout(entry.graceTimerId);
      entry.graceTimerId = null;
    }
    if (entry.debounceTimerId !== null) {
      window.clearTimeout(entry.debounceTimerId);
      entry.debounceTimerId = null;
    }
  }

  // Surface a per-peer connection state to the UI (deduped). Passing null clears
  // it (peer fully gone), matching the onSpeakingChange(false) convention.
  function setPeerUiState(userId: string, state: PeerConnectionUiState | null) {
    const entry = getPeerRecovery(userId);
    if (entry.uiState === state) return;
    entry.uiState = state;
    onPeerConnectionStateChangeRef.current(channelId, userId, state);
  }

  function closePeer(userId: string, options?: { preserveRecovery?: boolean }) {
    const pc = peersRef.current.get(userId);
    if (pc) {
      // Detach handlers so the close() we trigger here can't re-enter the
      // recovery state machine via onconnectionstatechange('closed').
      pc.onicecandidate = null;
      pc.ontrack = null;
      pc.onconnectionstatechange = null;
      pc.oniceconnectionstatechange = null;
      pc.close();
      peersRef.current.delete(userId);
    }
    const el = audioElementsRef.current.get(userId);
    if (el) {
      el.srcObject = null;
      el.remove();
      audioElementsRef.current.delete(userId);
    }
    teardownRemoteAudioPipeline(userId);
    stopSpeakingMonitor(userId);
    pendingIceCandidatesRef.current.delete(userId);
    if (!options?.preserveRecovery) {
      clearPeerRecoveryTimers(userId);
      // Emit a final null state before dropping the bookkeeping entry.
      setPeerUiState(userId, null);
      peerRecoveryRef.current.delete(userId);
    }
  }

  function queueRemoteIceCandidate(userId: string, candidate: RTCIceCandidateInit) {
    const existing = pendingIceCandidatesRef.current.get(userId) ?? [];
    existing.push(candidate);
    pendingIceCandidatesRef.current.set(userId, existing);
  }

  async function flushPendingIceCandidates(userId: string, pc: RTCPeerConnection) {
    const queued = pendingIceCandidatesRef.current.get(userId);
    if (!queued?.length || !pc.remoteDescription) {
      return;
    }
    pendingIceCandidatesRef.current.delete(userId);
    for (const candidate of queued) {
      try {
        await pc.addIceCandidate(new RTCIceCandidate(candidate));
      } catch (err) {
        console.error('VoiceEngine: failed to flush ICE candidate from', userId, err);
      }
    }
  }

  // Re-send an offer on an existing pc (used after restartIce on the initiator
  // side). Re-attaches local tracks and re-flushes pending ICE the same way the
  // initial connect path does.
  async function sendOfferForRecovery(userId: string, pc: RTCPeerConnection) {
    addLocalTracks(pc);
    try {
      const offer = await pc.createOffer({ iceRestart: true });
      await pc.setLocalDescription(offer);
      sendWs({
        type: 'rtc_offer',
        to_user_id: userId,
        channel_id: channelId,
        sdp: JSON.stringify(pc.localDescription),
      });
      await flushPendingIceCandidates(userId, pc);
    } catch (err) {
      console.error('VoiceEngine: recovery offer failed for', userId, err);
      // Fall back to a full rebuild on the next scheduled attempt.
      closePeer(userId, { preserveRecovery: true });
    }
  }

  // Core recovery action. Decides restartIce vs full rebuild and respects the
  // single-offerer rule so both sides don't generate glare.
  function recoverPeer(userId: string) {
    if (userId === currentUserId) return;
    // Peer was torn down (user left) while the timer was pending.
    if (!peerRecoveryRef.current.has(userId)) return;

    const entry = getPeerRecovery(userId);
    entry.debounceTimerId = null;

    // Reset the attempt budget if the link was quiet long enough since the last
    // attempt, so a peer that breaks again much later gets a fresh allowance.
    const nowMs = performance.now();
    if (
      entry.attempts > 0 &&
      nowMs - entry.lastAttemptAtMs > PEER_RECOVERY_ATTEMPT_RESET_MS
    ) {
      entry.attempts = 0;
    }

    if (entry.attempts >= PEER_MAX_RECOVERY_ATTEMPTS) {
      // Give up actively retrying; leave UI in `failed` so it can show
      // "couldn't connect". A later natural recovery (onconnectionstatechange
      // → connected) or a fresh offer from the remote will clear it.
      setPeerUiState(userId, 'failed');
      return;
    }

    entry.attempts += 1;
    entry.lastAttemptAtMs = nowMs;
    setPeerUiState(userId, 'reconnecting');

    const pc = peersRef.current.get(userId);
    const initiator = shouldInitiatePeer(userId);

    // Prefer an in-place ICE restart when the platform supports it and the pc is
    // still around — it's cheaper and keeps the same RTCPeerConnection/track.
    const canRestartIce =
      !!pc &&
      pc.signalingState !== 'closed' &&
      typeof pc.restartIce === 'function';

    if (canRestartIce && pc) {
      try {
        pc.restartIce();
        if (initiator) {
          // Drive renegotiation explicitly with an iceRestart offer; the
          // answerer's restartIce primes it to accept the new credentials.
          void sendOfferForRecovery(userId, pc);
        }
        return;
      } catch (err) {
        console.warn('VoiceEngine: restartIce failed, rebuilding peer', userId, err);
        // fall through to rebuild
      }
    }

    // Rebuild path: tear down (preserving recovery bookkeeping/UI state) and
    // re-create. Only the deterministic initiator re-offers; the answerer
    // rebuilds a fresh pc that is ready to accept the incoming re-offer.
    closePeer(userId, { preserveRecovery: true });
    const fresh = createPeer(userId);
    // Re-attach the local audio track (or recvonly transceiver) just like the
    // initial connect path, so the answerer is ready before the re-offer lands.
    addLocalTracks(fresh);
    if (initiator) {
      void (async () => {
        try {
          const offer = await fresh.createOffer();
          await fresh.setLocalDescription(offer);
          sendWs({
            type: 'rtc_offer',
            to_user_id: userId,
            channel_id: channelId,
            sdp: JSON.stringify(fresh.localDescription),
          });
        } catch (err) {
          console.error('VoiceEngine: rebuild offer failed for', userId, err);
        }
      })();
    }
  }

  // Debounced trigger so a burst of failure transitions collapses into one
  // recovery attempt.
  function schedulePeerRecovery(userId: string) {
    if (userId === currentUserId) return;
    if (!peersRef.current.has(userId) && !peerRecoveryRef.current.has(userId)) {
      return;
    }
    const entry = getPeerRecovery(userId);
    setPeerUiState(userId, 'reconnecting');
    if (entry.debounceTimerId !== null) {
      return;
    }
    entry.debounceTimerId = window.setTimeout(() => {
      recoverPeer(userId);
    }, PEER_RECOVERY_DEBOUNCE_MS);
  }

  // State-machine dispatcher wired to onconnectionstatechange /
  // oniceconnectionstatechange. Maps transport states to UI state + recovery.
  function handlePeerConnectionStateChange(userId: string, pc: RTCPeerConnection) {
    // Prefer the aggregate connectionState; fall back to iceConnectionState on
    // platforms that fire the ice event but not the aggregate one.
    const connState = pc.connectionState;
    const iceState = pc.iceConnectionState;
    const entry = getPeerRecovery(userId);

    const isConnected = connState === 'connected' || iceState === 'connected' || iceState === 'completed';
    const isFailed = connState === 'failed' || iceState === 'failed';
    const isDisconnected = connState === 'disconnected' || iceState === 'disconnected';

    if (isConnected) {
      // Healthy again — cancel any pending recovery and reset the budget.
      clearPeerRecoveryTimers(userId);
      entry.attempts = 0;
      setPeerUiState(userId, 'connected');
      return;
    }

    if (isFailed) {
      // Hard failure: recover immediately (debounced).
      if (entry.graceTimerId !== null) {
        window.clearTimeout(entry.graceTimerId);
        entry.graceTimerId = null;
      }
      schedulePeerRecovery(userId);
      return;
    }

    if (isDisconnected) {
      // Soft failure: start a grace timer; many `disconnected` blips heal on
      // their own back to `connected` (which clears this timer above).
      setPeerUiState(userId, 'reconnecting');
      if (entry.graceTimerId === null) {
        entry.graceTimerId = window.setTimeout(() => {
          entry.graceTimerId = null;
          const current = peersRef.current.get(userId);
          const stillBad =
            !current ||
            current.connectionState === 'disconnected' ||
            current.connectionState === 'failed' ||
            current.iceConnectionState === 'disconnected' ||
            current.iceConnectionState === 'failed';
          if (stillBad) {
            schedulePeerRecovery(userId);
          }
        }, PEER_DISCONNECT_GRACE_MS);
      }
      return;
    }

    if (connState === 'connecting' || iceState === 'checking') {
      // Only show "connecting" if we aren't already mid-reconnect, so we don't
      // flap the indicator back from "reconnecting".
      if (entry.uiState !== 'reconnecting' && entry.uiState !== 'connected') {
        setPeerUiState(userId, 'connecting');
      }
    }
  }

  function createPeer(userId: string): RTCPeerConnection {
    const existing = peersRef.current.get(userId);
    if (existing) {
      // Detach handlers first so the old connection's `closed` transition and
      // any late ICE events can't re-enter the recovery state machine or race
      // the replacement peer.
      existing.onicecandidate = null;
      existing.ontrack = null;
      existing.onconnectionstatechange = null;
      existing.oniceconnectionstatechange = null;
      existing.close();
    }

    const pc = new RTCPeerConnection(createPeerConfig(iceServersRef.current));

    pc.onicecandidate = (e) => {
      if (e.candidate) {
        sendWs({
          type: 'rtc_ice',
          to_user_id: userId,
          channel_id: channelId,
          candidate: JSON.stringify(e.candidate),
        });
      }
    };

    pc.ontrack = (e) => {
      const stream = e.streams[0] ?? new MediaStream([e.track]);
      attachAudio(userId, stream);
    };

    pc.onconnectionstatechange = () => {
      handlePeerConnectionStateChange(userId, pc);
    };
    pc.oniceconnectionstatechange = () => {
      handlePeerConnectionStateChange(userId, pc);
    };

    // Seed an initial "connecting" indicator at construction.
    setPeerUiState(userId, 'connecting');

    peersRef.current.set(userId, pc);
    return pc;
  }

  async function initiatePeerConnection(userId: string) {
    if (userId === currentUserId) return;
    if (!shouldInitiatePeer(userId)) return;
    if (peersRef.current.has(userId)) return;

    const pc = createPeer(userId);
    addLocalTracks(pc);

    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      sendWs({
        type: 'rtc_offer',
        to_user_id: userId,
        channel_id: channelId,
        sdp: JSON.stringify(pc.localDescription),
      });
    } catch (err) {
      console.error('VoiceEngine: failed to create offer for', userId, err);
      closePeer(userId);
    }
  }

  useEffect(() => {
    for (const member of existingMembers) {
      if (member.user_id === currentUserId) continue;
      if (peersRef.current.has(member.user_id)) continue;
      void initiatePeerConnection(member.user_id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [existingMembers, currentUserId, channelId, iceServers]);

  // Keep the state-change callback fresh for the long-lived pc handlers.
  useEffect(() => {
    onPeerConnectionStateChangeRef.current = onPeerConnectionStateChange;
  }, [onPeerConnectionStateChange]);

  // ── AUD-7: apply late iceServers to already-built peers ────────────────────
  // /runtime-config resolves asynchronously; peers created before it lands hold
  // the STUN-only fallback and would never learn about TURN. When iceServers
  // actually changes, push the new configuration onto every live peer (or
  // rebuild if setConfiguration is unavailable) so TURN can be used. Skip on the
  // first run and when the value is unchanged to avoid thrashing healthy peers.
  const previousIceServersRef = useRef<RTCIceServer[]>(iceServers);
  useEffect(() => {
    iceServersRef.current = iceServers;

    const previous = previousIceServersRef.current;
    const unchanged =
      previous === iceServers ||
      JSON.stringify(previous) === JSON.stringify(iceServers);
    if (unchanged) {
      return;
    }
    previousIceServersRef.current = iceServers;

    const nextConfig = createPeerConfig(iceServers);
    peersRef.current.forEach((pc, userId) => {
      if (pc.signalingState === 'closed') return;
      const configurable = pc as RTCPeerConnection & {
        setConfiguration?: (config: RTCConfiguration) => void;
      };
      if (typeof configurable.setConfiguration === 'function') {
        try {
          configurable.setConfiguration(nextConfig);
          // setConfiguration alone won't re-gather against the new (TURN)
          // servers; trigger an ICE restart so the new servers are exercised.
          // Only the deterministic initiator drives the renegotiation.
          if (typeof pc.restartIce === 'function') {
            pc.restartIce();
            if (shouldInitiatePeer(userId)) {
              void sendOfferForRecovery(userId, pc);
            }
          }
          return;
        } catch (err) {
          console.warn('VoiceEngine: setConfiguration failed, rebuilding peer', userId, err);
        }
      }
      // Fallback: rebuild the peer with the new config. Preserve recovery
      // bookkeeping; only the initiator re-offers.
      closePeer(userId, { preserveRecovery: true });
      const fresh = createPeer(userId);
      addLocalTracks(fresh);
      if (shouldInitiatePeer(userId)) {
        void (async () => {
          try {
            const offer = await fresh.createOffer();
            await fresh.setLocalDescription(offer);
            sendWs({
              type: 'rtc_offer',
              to_user_id: userId,
              channel_id: channelId,
              sdp: JSON.stringify(fresh.localDescription),
            });
          } catch (err) {
            console.error('VoiceEngine: rebuild-after-ice-change offer failed for', userId, err);
          }
        })();
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [iceServers]);

  useEffect(() => {
    processedEventSeqRef.current = 0;
    pendingIceCandidatesRef.current.clear();
    return () => {
      peersRef.current.forEach((pc) => pc.close());
      peersRef.current.clear();
      audioElementsRef.current.forEach((el) => {
        el.srcObject = null;
        el.remove();
      });
      audioElementsRef.current.clear();
      for (const userId of Array.from(remoteAudioPipelinesRef.current.keys())) {
        teardownRemoteAudioPipeline(userId);
      }
      for (const userId of Array.from(speakingMonitorsRef.current.keys())) {
        stopSpeakingMonitor(userId);
      }
      for (const userId of Array.from(peerRecoveryRef.current.keys())) {
        clearPeerRecoveryTimers(userId);
      }
      peerRecoveryRef.current.clear();
      teardownLocalMicPipeline();
      teardownTranscriptionCapture();
      if (audioContextRef.current) {
        void audioContextRef.current.close().catch(() => {});
        audioContextRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!localStream) {
      teardownLocalMicPipeline();
      stopSpeakingMonitor(currentUserId);
      return;
    }
    getOutboundStream();
    startSpeakingMonitor(currentUserId, localStream);
    return () => {
      stopSpeakingMonitor(currentUserId);
      teardownLocalMicPipeline();
      teardownTranscriptionCapture();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localStream, currentUserId, channelId]);

  useEffect(() => {
    if (localMicGainNodeRef.current) {
      localMicGainNodeRef.current.gain.value = localMicGain;
    }
    syncOutboundAudioTrack();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localMicGain]);

  useEffect(() => {
    syncOutboundAudioTrack();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localStream]);

  useEffect(() => {
    const sessionId =
      transcriptionState?.status === 'running'
        ? (transcriptionState.session_id ?? null)
        : null;
    const transcriptionStream = localStream;
    const localTrack = transcriptionStream?.getAudioTracks()[0] ?? null;

    if (!transcriptionStream || !localTrack || !sessionId) {
      teardownTranscriptionCapture();
      return;
    }

    if (transcriptionRecorderRef.current && activeTranscriptionSessionRef.current === sessionId) {
      return;
    }

    teardownTranscriptionCapture();

    try {
      const SpeechRecognitionCtor =
        typeof window !== 'undefined'
          ? (
              window as Window & {
                SpeechRecognition?: ChannelSpeechRecognitionConstructor;
                webkitSpeechRecognition?: ChannelSpeechRecognitionConstructor;
              }
            ).SpeechRecognition ??
            (
              window as Window & {
                SpeechRecognition?: ChannelSpeechRecognitionConstructor;
                webkitSpeechRecognition?: ChannelSpeechRecognitionConstructor;
              }
            ).webkitSpeechRecognition
          : undefined;
      if (SpeechRecognitionCtor && !recognitionRef.current) {
        const startRecognition = () => {
          const recognition = new SpeechRecognitionCtor();
          recognitionRef.current = recognition;
          recognition.lang = 'en-US';
          recognition.continuous = true;
          recognition.interimResults = true;
          recognition.maxAlternatives = 1;
          recognition.onresult = (event) => {
            const transcript = collectBrowserTranscript(event);
            if (transcript) {
              browserSpeechCurrentRef.current = transcript;
            }
          };
          recognition.onerror = (event) => {
            if (event.error === 'no-speech' && browserSpeechShouldContinueRef.current) {
              return;
            }
            if (!browserSpeechShouldContinueRef.current && event.error === 'aborted') {
              return;
            }
            browserSpeechShouldContinueRef.current = false;
            recognitionRef.current = null;
            browserSpeechCurrentRef.current = '';
          };
          recognition.onend = () => {
            recognitionRef.current = null;
            const currentTranscript = browserSpeechCurrentRef.current.trim();
            if (browserSpeechShouldContinueRef.current) {
              if (currentTranscript) {
                browserSpeechAccumulatedRef.current = [
                  browserSpeechAccumulatedRef.current,
                  currentTranscript,
                ]
                  .filter(Boolean)
                  .join(' ')
                  .trim();
                browserSpeechCurrentRef.current = '';
              }
              startRecognition();
              return;
            }
            void (async () => {
              const finalTranscript = [
                browserSpeechAccumulatedRef.current,
                currentTranscript,
              ]
                .filter(Boolean)
                .join(' ')
                .trim();
              if (finalTranscript) {
                try {
                  const startedAt =
                    browserSpeechStartedAtRef.current || transcriptionCaptureStartedAtRef.current || Date.now();
                  const endedAt = Date.now();
                  await onTranscriptionTextUpload(channelId, {
                    sessionId,
                    startedTsMs: startedAt,
                    endedTsMs: Math.max(endedAt, startedAt + 1),
                    text: finalTranscript,
                  });
                  browserSpeechUploadedRef.current = true;
                } catch (error) {
                  console.warn('VoiceEngine: browser speech transcript upload failed', error);
                }
              }
              browserSpeechStartedAtRef.current = 0;
              browserSpeechAccumulatedRef.current = '';
              browserSpeechCurrentRef.current = '';
              const recorder = transcriptionRecorderRef.current;
              if (recorder && recorder.state !== 'inactive' && !transcriptionStopRequestedRef.current) {
                transcriptionStopRequestedRef.current = true;
                recorder.stop();
                return;
              }
              transcriptionRecorderRef.current = null;
              transcriptionChunksRef.current = [];
              transcriptionCaptureStartedAtRef.current = 0;
              activeTranscriptionSessionRef.current = null;
              transcriptionStopRequestedRef.current = false;
              browserSpeechUploadedRef.current = false;
            })();
          };
          recognition.start();
        };
        browserSpeechShouldContinueRef.current = true;
        browserSpeechStartedAtRef.current = Date.now();
        browserSpeechAccumulatedRef.current = '';
        browserSpeechCurrentRef.current = '';
        browserSpeechUploadedRef.current = false;
        startRecognition();
      }
      if (typeof MediaRecorder === 'undefined') {
        return;
      }
      const mimeType = preferredRecorderMimeType();
      const recorder = mimeType
        ? new MediaRecorder(transcriptionStream, { mimeType })
        : new MediaRecorder(transcriptionStream);
      const sessionIdForUpload = sessionId;
      const captureStartedAt = Date.now();
      transcriptionChunksRef.current = [];
      transcriptionCaptureStartedAtRef.current = captureStartedAt;
      transcriptionStopRequestedRef.current = false;
      recorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          transcriptionChunksRef.current.push(event.data);
        }
      };
      recorder.onerror = (event) => {
        console.warn('VoiceEngine: transcript recorder error', event);
      };
      recorder.onstop = () => {
        const chunks = [...transcriptionChunksRef.current];
        transcriptionRecorderRef.current = null;
        transcriptionChunksRef.current = [];
        transcriptionCaptureStartedAtRef.current = 0;
        activeTranscriptionSessionRef.current = null;
        transcriptionStopRequestedRef.current = false;
        if (browserSpeechUploadedRef.current) {
          browserSpeechUploadedRef.current = false;
          return;
        }
        if (!sessionIdForUpload || chunks.length === 0) {
          return;
        }
        const endedAt = Date.now();
        const blob = new Blob(chunks, {
          type: recorder.mimeType || mimeType || 'audio/webm',
        });
        const uploadPayload: VoiceTranscriptionRecordingUpload = {
          sessionId: sessionIdForUpload,
          captureStartedTsMs: captureStartedAt,
          captureEndedTsMs: Math.max(endedAt, captureStartedAt + 1),
          blob,
          fileName: `voice-transcript-${sessionIdForUpload}.webm`,
        };
        void onTranscriptionRecordingUpload(channelId, uploadPayload);
      };
      recorder.start(1000);
      transcriptionRecorderRef.current = recorder;
      activeTranscriptionSessionRef.current = sessionId;
    } catch (err) {
      console.warn('VoiceEngine: failed to start transcription recording pipeline', err);
      teardownTranscriptionCapture();
      return;
    }

    return () => {
      teardownTranscriptionCapture();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localStream, transcriptionState?.status, transcriptionState?.session_id, channelId]);

  useEffect(() => {
    audioElementsRef.current.forEach((audio, userId) => {
      audio.muted = deafened;
      const pipeline = remoteAudioPipelinesRef.current.get(userId);
      if (pipeline) {
        pipeline.gain.gain.value = getPeerVolume(userId);
        audio.volume = 1;
      } else {
        audio.volume = Math.min(1, getPeerVolume(userId));
      }
      applyPreferredOutputDevice(audio);
      if (!deafened) {
        const playPromise = audio.play();
        if (playPromise) {
          void playPromise.catch(() => {});
        }
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deafened, remoteVolumes, preferredOutputDeviceId]);

  useEffect(() => {
    const nudgeAudio = () => {
      const audioContext = getAudioContext();
      if (audioContext && audioContext.state !== 'running') {
        void audioContext.resume().catch(() => {});
      }
      audioElementsRef.current.forEach((audio) => {
        if (audio.muted) return;
        const playPromise = audio.play();
        if (playPromise) {
          void playPromise.catch(() => {});
        }
      });
    };

    window.addEventListener('pointerdown', nudgeAudio, { passive: true });
    window.addEventListener('keydown', nudgeAudio);
    return () => {
      window.removeEventListener('pointerdown', nudgeAudio);
      window.removeEventListener('keydown', nudgeAudio);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Handle incoming WS events
  useEffect(() => {
    async function handle() {
      const pendingEvents = wsEvents.filter(({ seq }) => seq > processedEventSeqRef.current);
      if (pendingEvents.length === 0) {
        return;
      }

      for (const { seq, event: e } of pendingEvents) {
        processedEventSeqRef.current = seq;

        if (e.type === 'voice_presence') {
          if (e.channel_id !== channelId) continue;
          if (e.user_id === currentUserId) continue;

          if (!e.joined) {
            closePeer(e.user_id);
          } else {
            void initiatePeerConnection(e.user_id);
          }
        } else if (e.type === 'rtc_offer') {
          if (e.channel_id !== channelId) continue;

          const pc = createPeer(e.from_user_id);
          addLocalTracks(pc);

          try {
            const remoteDesc = JSON.parse(e.sdp) as RTCSessionDescriptionInit;
            await pc.setRemoteDescription(new RTCSessionDescription(remoteDesc));
            await flushPendingIceCandidates(e.from_user_id, pc);
            const answer = await pc.createAnswer();
            await pc.setLocalDescription(answer);
            sendWs({
              type: 'rtc_answer',
              to_user_id: e.from_user_id,
              channel_id: channelId,
              sdp: JSON.stringify(pc.localDescription),
            });
          } catch (err) {
            console.error('VoiceEngine: failed to handle offer from', e.from_user_id, err);
          }
        } else if (e.type === 'rtc_answer') {
          if (e.channel_id !== channelId) continue;

          const pc = peersRef.current.get(e.from_user_id);
          if (!pc) continue;

          try {
            const remoteDesc = JSON.parse(e.sdp) as RTCSessionDescriptionInit;
            await pc.setRemoteDescription(new RTCSessionDescription(remoteDesc));
            await flushPendingIceCandidates(e.from_user_id, pc);
          } catch (err) {
            console.error('VoiceEngine: failed to set answer from', e.from_user_id, err);
          }
        } else if (e.type === 'rtc_ice') {
          if (e.channel_id !== channelId) continue;

          const pc = peersRef.current.get(e.from_user_id);
          const candidate = JSON.parse(e.candidate) as RTCIceCandidateInit;
          if (!pc || !pc.remoteDescription) {
            queueRemoteIceCandidate(e.from_user_id, candidate);
            continue;
          }

          try {
            await pc.addIceCandidate(new RTCIceCandidate(candidate));
          } catch (err) {
            console.error('VoiceEngine: failed to add ICE candidate from', e.from_user_id, err);
          }
        }
      }
    }

    void handle();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsEvents]);

  // headless — renders nothing visible
  return null;
}
