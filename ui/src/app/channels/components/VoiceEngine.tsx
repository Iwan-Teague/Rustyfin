'use client';

import { useEffect, useRef } from 'react';
import type {
  ChannelEvent,
  UserInfo,
  VoiceTranscribeChunkRequest,
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
  onSpeakingChange: (channelId: string, userId: string, speaking: boolean) => void;
  transcriptionState: VoiceTranscriptionState | null;
  onTranscriptionChunk: (channelId: string, payload: VoiceTranscribeChunkRequest) => Promise<void>;
}

const STUN_URL =
  process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302';
const SPEAKING_SAMPLE_INTERVAL_MS = 120;
const SPEAKING_RMS_THRESHOLD = 0.03;
const SPEAKING_HANG_MS = 300;
const TRANSCRIPTION_SAMPLE_RATE = 16_000;
const TRANSCRIPTION_CHUNK_SECONDS = 4;
const TRANSCRIPTION_CHUNK_SAMPLES = TRANSCRIPTION_SAMPLE_RATE * TRANSCRIPTION_CHUNK_SECONDS;

type SpeakingMonitor = {
  source: MediaStreamAudioSourceNode;
  analyser: AnalyserNode;
  buffer: Uint8Array;
  timerId: number;
  speaking: boolean;
  lastLoudAtMs: number;
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
  onSpeakingChange,
  transcriptionState,
  onTranscriptionChunk,
}: Props) {
  const peersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const audioElementsRef = useRef<Map<string, HTMLAudioElement>>(new Map());
  const speakingMonitorsRef = useRef<Map<string, SpeakingMonitor>>(new Map());
  const audioContextRef = useRef<AudioContext | null>(null);
  const localMicContextRef = useRef<AudioContext | null>(null);
  const localMicGainNodeRef = useRef<GainNode | null>(null);
  const localMicSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const localMicDestinationRef = useRef<MediaStreamAudioDestinationNode | null>(null);
  const localMicProcessedStreamRef = useRef<MediaStream | null>(null);
  const localMicInputTrackIdRef = useRef<string | null>(null);
  const transcriptionContextRef = useRef<AudioContext | null>(null);
  const transcriptionSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const transcriptionProcessorRef = useRef<ScriptProcessorNode | null>(null);
  const transcriptionSamplesRef = useRef<number[]>([]);
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

  function floatToInt16(input: Float32Array): Int16Array {
    const out = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const clamped = Math.max(-1, Math.min(1, input[i]));
      out[i] = clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff);
    }
    return out;
  }

  function downsampleTo16k(input: Float32Array, inputRate: number): Int16Array {
    if (!Number.isFinite(inputRate) || inputRate <= 0) {
      return new Int16Array();
    }
    if (inputRate === TRANSCRIPTION_SAMPLE_RATE) {
      return floatToInt16(input);
    }

    const ratio = inputRate / TRANSCRIPTION_SAMPLE_RATE;
    const outputLength = Math.max(1, Math.floor(input.length / ratio));
    const output = new Int16Array(outputLength);
    let outputIndex = 0;
    let sourceIndex = 0;
    while (outputIndex < outputLength) {
      const nextSourceIndex = Math.min(input.length, Math.round((outputIndex + 1) * ratio));
      let accumulator = 0;
      let count = 0;
      for (let i = sourceIndex; i < nextSourceIndex; i++) {
        accumulator += input[i];
        count += 1;
      }
      const sample = count > 0 ? accumulator / count : 0;
      const clamped = Math.max(-1, Math.min(1, sample));
      output[outputIndex] = clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff);
      outputIndex += 1;
      sourceIndex = nextSourceIndex;
    }
    return output;
  }

  function pcmInt16ToBase64(samples: Int16Array): string {
    const bytes = new Uint8Array(samples.buffer, samples.byteOffset, samples.byteLength);
    let binary = '';
    const chunkSize = 0x8000;
    for (let i = 0; i < bytes.length; i += chunkSize) {
      binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
    }
    return btoa(binary);
  }

  function measurePeak(samples: Int16Array): number {
    if (samples.length === 0) return 0;
    let peak = 0;
    for (let i = 0; i < samples.length; i++) {
      const value = Math.abs(samples[i]);
      if (value > peak) peak = value;
    }
    return peak / 32768;
  }

  function normalizeChunk(samples: Int16Array): Int16Array {
    const peak = measurePeak(samples);
    if (peak <= 0.0001 || peak >= 0.14) {
      return samples;
    }
    const gain = Math.min(8, 0.14 / peak);
    const out = new Int16Array(samples.length);
    for (let i = 0; i < samples.length; i++) {
      const value = Math.round(samples[i] * gain);
      if (value > 32767) {
        out[i] = 32767;
      } else if (value < -32768) {
        out[i] = -32768;
      } else {
        out[i] = value;
      }
    }
    return out;
  }

  function teardownTranscriptionCapture() {
    transcriptionProcessorRef.current?.disconnect();
    transcriptionSourceRef.current?.disconnect();
    if (transcriptionContextRef.current && transcriptionContextRef.current !== audioContextRef.current) {
      void transcriptionContextRef.current.close().catch(() => {});
    }
    transcriptionProcessorRef.current = null;
    transcriptionSourceRef.current = null;
    transcriptionContextRef.current = null;
    transcriptionSamplesRef.current = [];
    activeTranscriptionSessionRef.current = null;
  }

  function flushTranscriptionChunk(sessionId: string, force = false) {
    const samples = transcriptionSamplesRef.current;
    if (samples.length === 0) return;
    if (!force && samples.length < TRANSCRIPTION_CHUNK_SAMPLES) return;

    const chunkSize = force ? samples.length : TRANSCRIPTION_CHUNK_SAMPLES;
    const chunk = samples.splice(0, chunkSize);
    if (chunk.length === 0) return;
    const raw = Int16Array.from(chunk);
    if (measurePeak(raw) < 0.003) {
      return;
    }
    const normalized = normalizeChunk(raw);

    const now = Date.now();
    const durationMs = Math.max(1, Math.round((chunk.length / TRANSCRIPTION_SAMPLE_RATE) * 1000));
    const payload: VoiceTranscribeChunkRequest = {
      session_id: sessionId,
      sample_rate_hz: TRANSCRIPTION_SAMPLE_RATE,
      started_ts_ms: now - durationMs,
      ended_ts_ms: now,
      pcm_s16le_base64: pcmInt16ToBase64(normalized),
    };

    void onTranscriptionChunk(channelId, payload);
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
    const audioTracks = outboundStream.getAudioTracks();
    if (audioTracks.length === 0) {
      ensureReceiveAudio(pc);
      return;
    }
    audioTracks.forEach((track) => pc.addTrack(track, outboundStream));
  }

  function getPeerVolume(userId: string): number {
    const value = remoteVolumes[userId];
    if (!Number.isFinite(value)) return 1;
    return Math.min(1, Math.max(0, value));
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
    el.volume = getPeerVolume(userId);
    el.srcObject = stream;
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

  // On mount: initiate connections to existing members
  useEffect(() => {
    let cancelled = false;

    async function initiateConnections() {
      for (const member of existingMembers) {
        if (member.user_id === currentUserId) continue;
        const pc = createPeer(member.user_id);
        addLocalTracks(pc);

        try {
          const offer = await pc.createOffer();
          await pc.setLocalDescription(offer);
          if (!cancelled) {
            sendWs({
              type: 'rtc_offer',
              to_user_id: member.user_id,
              channel_id: channelId,
              sdp: JSON.stringify(pc.localDescription),
            });
          }
        } catch (err) {
          console.error('VoiceEngine: failed to create offer for', member.user_id, err);
        }
      }
    }

    initiateConnections();

    return () => {
      cancelled = true;
      peersRef.current.forEach((pc) => pc.close());
      peersRef.current.clear();
      audioElementsRef.current.forEach((el) => {
        el.srcObject = null;
        el.remove();
      });
      audioElementsRef.current.clear();
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
  }, [localMicGain]);

  useEffect(() => {
    const sessionId =
      transcriptionState?.status === 'running'
        ? (transcriptionState.session_id ?? null)
        : null;
    const transcriptionStream = getOutboundStream() ?? localStream;
    const localTrack = transcriptionStream?.getAudioTracks()[0] ?? null;

    if (!transcriptionStream || !localTrack || !sessionId) {
      const previousSession = activeTranscriptionSessionRef.current;
      if (previousSession) {
        flushTranscriptionChunk(previousSession, true);
      }
      teardownTranscriptionCapture();
      return;
    }

    if (
      transcriptionContextRef.current &&
      activeTranscriptionSessionRef.current === sessionId
    ) {
      return;
    }

    const previousSession = activeTranscriptionSessionRef.current;
    if (previousSession) {
      flushTranscriptionChunk(previousSession, true);
    }
    teardownTranscriptionCapture();

    const context = getAudioContext();
    if (!context) {
      return;
    }

    try {
      const source = context.createMediaStreamSource(transcriptionStream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (event) => {
        if (!localTrack.enabled) {
          return;
        }
        const input = event.inputBuffer.getChannelData(0);
        const downsampled = downsampleTo16k(input, event.inputBuffer.sampleRate);
        if (downsampled.length === 0) {
          return;
        }
        const target = transcriptionSamplesRef.current;
        for (let i = 0; i < downsampled.length; i++) {
          target.push(downsampled[i]);
        }
        while (target.length >= TRANSCRIPTION_CHUNK_SAMPLES) {
          flushTranscriptionChunk(sessionId, false);
        }
      };

      source.connect(processor);
      processor.connect(context.destination);
      void context.resume().catch(() => {});

      transcriptionContextRef.current = context;
      transcriptionSourceRef.current = source;
      transcriptionProcessorRef.current = processor;
      activeTranscriptionSessionRef.current = sessionId;
    } catch (err) {
      console.warn('VoiceEngine: failed to start transcription capture pipeline', err);
      teardownTranscriptionCapture();
      return;
    }

    return () => {
      if (activeTranscriptionSessionRef.current === sessionId) {
        flushTranscriptionChunk(sessionId, true);
      }
      teardownTranscriptionCapture();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localStream, transcriptionState?.status, transcriptionState?.session_id, channelId, localMicGain]);

  useEffect(() => {
    audioElementsRef.current.forEach((audio, userId) => {
      audio.muted = deafened;
      audio.volume = getPeerVolume(userId);
      if (!deafened) {
        const playPromise = audio.play();
        if (playPromise) {
          void playPromise.catch(() => {});
        }
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deafened, remoteVolumes]);

  useEffect(() => {
    const nudgeAudio = () => {
      const audioContext = getAudioContext();
      if (audioContext && audioContext.state !== 'running') {
        void audioContext.resume().catch(() => {});
      }
      if (transcriptionContextRef.current && transcriptionContextRef.current.state !== 'running') {
        void transcriptionContextRef.current.resume().catch(() => {});
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
        }
        // If joined=true: they will send an offer to us; we wait
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
