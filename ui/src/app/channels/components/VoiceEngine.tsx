'use client';

import { useEffect, useRef } from 'react';
import type {
  ChannelEvent,
  UserInfo,
  VoiceTranscriptionRecordingUpload,
  VoiceTranscriptionState,
} from '@/lib/channelsApi';

interface Props {
  localStream: MediaStream | null;
  channelId: string;
  currentUserId: string;
  existingMembers: UserInfo[];
  wsEvents: ChannelEvent | null;
  sendWs: (msg: object) => void;
  deafened: boolean;
  remoteVolumes: Record<string, number>;
  localMicGain: number;
  preferredOutputDeviceId: string | null;
  onSpeakingChange: (channelId: string, userId: string, speaking: boolean) => void;
  transcriptionState: VoiceTranscriptionState | null;
  onTranscriptionRecordingUpload: (
    channelId: string,
    payload: VoiceTranscriptionRecordingUpload,
  ) => Promise<void>;
}

const STUN_URL =
  process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302';
const SPEAKING_SAMPLE_INTERVAL_MS = 120;
const SPEAKING_RMS_THRESHOLD = 0.03;
const SPEAKING_HANG_MS = 300;
const SYNTHETIC_OUTPUT_PREFIX = 'synthetic-audiooutput-';

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

function createPeerConfig(): RTCConfiguration {
  return {
    iceServers: [{ urls: STUN_URL }],
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
  deafened,
  remoteVolumes,
  localMicGain,
  preferredOutputDeviceId,
  onSpeakingChange,
  transcriptionState,
  onTranscriptionRecordingUpload,
}: Props) {
  const peersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const audioElementsRef = useRef<Map<string, HTMLAudioElement>>(new Map());
  const remoteAudioPipelinesRef = useRef<Map<string, RemoteAudioPipeline>>(new Map());
  const speakingMonitorsRef = useRef<Map<string, SpeakingMonitor>>(new Map());
  const audioContextRef = useRef<AudioContext | null>(null);
  const localMicContextRef = useRef<AudioContext | null>(null);
  const localMicGainNodeRef = useRef<GainNode | null>(null);
  const localMicSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const localMicDestinationRef = useRef<MediaStreamAudioDestinationNode | null>(null);
  const localMicProcessedStreamRef = useRef<MediaStream | null>(null);
  const localMicInputTrackIdRef = useRef<string | null>(null);
  const transcriptionCaptureContextRef = useRef<AudioContext | null>(null);
  const transcriptionCaptureSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const transcriptionCaptureProcessorRef = useRef<ScriptProcessorNode | null>(null);
  const transcriptionCaptureSilenceRef = useRef<GainNode | null>(null);
  const transcriptionCaptureChunksRef = useRef<Float32Array[]>([]);
  const transcriptionCaptureSampleRateRef = useRef<number>(48_000);
  const transcriptionCaptureStartedAtRef = useRef<number>(0);
  const activeTranscriptionSessionRef = useRef<string | null>(null);

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

  function encodeWavFromFloat32(chunks: Float32Array[], sampleRate: number): Blob | null {
    const totalSamples = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    if (totalSamples <= 0 || !Number.isFinite(sampleRate) || sampleRate <= 0) {
      return null;
    }

    const pcmBytes = totalSamples * 2;
    const buffer = new ArrayBuffer(44 + pcmBytes);
    const view = new DataView(buffer);

    const writeAscii = (offset: number, value: string) => {
      for (let i = 0; i < value.length; i += 1) {
        view.setUint8(offset + i, value.charCodeAt(i));
      }
    };

    writeAscii(0, 'RIFF');
    view.setUint32(4, 36 + pcmBytes, true);
    writeAscii(8, 'WAVE');
    writeAscii(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, Math.round(sampleRate), true);
    view.setUint32(28, Math.round(sampleRate) * 2, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    writeAscii(36, 'data');
    view.setUint32(40, pcmBytes, true);

    let offset = 44;
    for (const chunk of chunks) {
      for (let i = 0; i < chunk.length; i += 1) {
        const sample = Math.max(-1, Math.min(1, chunk[i]));
        const pcm = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
        view.setInt16(offset, Math.round(pcm), true);
        offset += 2;
      }
    }

    return new Blob([buffer], { type: 'audio/wav' });
  }

  function teardownTranscriptionCapture() {
    try {
      transcriptionCaptureSourceRef.current?.disconnect();
    } catch {
      // no-op
    }
    try {
      transcriptionCaptureProcessorRef.current?.disconnect();
    } catch {
      // no-op
    }
    try {
      transcriptionCaptureSilenceRef.current?.disconnect();
    } catch {
      // no-op
    }

    if (transcriptionCaptureContextRef.current) {
      void transcriptionCaptureContextRef.current.close().catch(() => {});
    }

    const sessionId = activeTranscriptionSessionRef.current;
    const startedAt = transcriptionCaptureStartedAtRef.current;
    const sampleRate = transcriptionCaptureSampleRateRef.current;
    const chunks = transcriptionCaptureChunksRef.current.slice();

    transcriptionCaptureSourceRef.current = null;
    transcriptionCaptureProcessorRef.current = null;
    transcriptionCaptureSilenceRef.current = null;
    transcriptionCaptureContextRef.current = null;
    transcriptionCaptureChunksRef.current = [];
    transcriptionCaptureSampleRateRef.current = 48_000;
    transcriptionCaptureStartedAtRef.current = 0;
    activeTranscriptionSessionRef.current = null;

    const blob = encodeWavFromFloat32(chunks, sampleRate);
    if (!blob || !sessionId) {
      return;
    }

    const endedAt = Date.now();
    const uploadPayload: VoiceTranscriptionRecordingUpload = {
      sessionId,
      captureStartedTsMs: startedAt,
      captureEndedTsMs: Math.max(endedAt, startedAt + 1),
      blob,
      fileName: `voice-transcript-${sessionId}.wav`,
    };
    void onTranscriptionRecordingUpload(channelId, uploadPayload);
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

  function closePeer(userId: string) {
    const pc = peersRef.current.get(userId);
    if (pc) {
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
  }

  function createPeer(userId: string): RTCPeerConnection {
    const existing = peersRef.current.get(userId);
    if (existing) {
      existing.close();
    }

    const pc = new RTCPeerConnection(createPeerConfig());

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
  }, [existingMembers, currentUserId, channelId]);

  useEffect(() => {
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

    if (transcriptionCaptureContextRef.current && activeTranscriptionSessionRef.current === sessionId) {
      return;
    }

    teardownTranscriptionCapture();

    try {
      const audioContextCtor =
        window.AudioContext ||
        (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!audioContextCtor) {
        return;
      }
      const context = new audioContextCtor();
      const source = context.createMediaStreamSource(transcriptionStream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const silence = context.createGain();
      silence.gain.value = 0;

      transcriptionCaptureChunksRef.current = [];
      transcriptionCaptureSampleRateRef.current = context.sampleRate;
      transcriptionCaptureStartedAtRef.current = Date.now();
      processor.onaudioprocess = (event) => {
        const channelData = event.inputBuffer.getChannelData(0);
        if (!channelData || channelData.length === 0) {
          return;
        }
        transcriptionCaptureChunksRef.current.push(new Float32Array(channelData));
      };

      source.connect(processor);
      processor.connect(silence);
      silence.connect(context.destination);

      transcriptionCaptureContextRef.current = context;
      transcriptionCaptureSourceRef.current = source;
      transcriptionCaptureProcessorRef.current = processor;
      transcriptionCaptureSilenceRef.current = silence;
      activeTranscriptionSessionRef.current = sessionId;
      void context.resume().catch(() => {});
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
    if (!wsEvents) return;

    const e = wsEvents;

    async function handle() {
      if (e.type === 'voice_presence') {
        if (e.channel_id !== channelId) return;
        if (e.user_id === currentUserId) return;

        if (!e.joined) {
          closePeer(e.user_id);
        } else {
          void initiatePeerConnection(e.user_id);
        }
      } else if (e.type === 'rtc_offer') {
        if (e.channel_id !== channelId) return;

        const pc = createPeer(e.from_user_id);
        addLocalTracks(pc);

        try {
          const remoteDesc = JSON.parse(e.sdp) as RTCSessionDescriptionInit;
          await pc.setRemoteDescription(new RTCSessionDescription(remoteDesc));
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
        if (e.channel_id !== channelId) return;

        const pc = peersRef.current.get(e.from_user_id);
        if (!pc) return;

        try {
          const remoteDesc = JSON.parse(e.sdp) as RTCSessionDescriptionInit;
          await pc.setRemoteDescription(new RTCSessionDescription(remoteDesc));
        } catch (err) {
          console.error('VoiceEngine: failed to set answer from', e.from_user_id, err);
        }
      } else if (e.type === 'rtc_ice') {
        if (e.channel_id !== channelId) return;

        const pc = peersRef.current.get(e.from_user_id);
        if (!pc) return;

        try {
          const candidate = JSON.parse(e.candidate) as RTCIceCandidateInit;
          await pc.addIceCandidate(new RTCIceCandidate(candidate));
        } catch (err) {
          console.error('VoiceEngine: failed to add ICE candidate from', e.from_user_id, err);
        }
      }
    }

    handle();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsEvents]);

  // headless — renders nothing visible
  return null;
}
