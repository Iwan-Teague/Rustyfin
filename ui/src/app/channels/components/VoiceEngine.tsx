'use client';

import { useEffect, useRef } from 'react';
import type { ChannelEvent, UserInfo } from '@/lib/channelsApi';

interface Props {
  localStream: MediaStream | null;
  channelId: string;
  currentUserId: string;
  existingMembers: UserInfo[];
  wsEvents: ChannelEvent | null;
  sendWs: (msg: object) => void;
  deafened: boolean;
  remoteVolumes: Record<string, number>;
  onSpeakingChange: (channelId: string, userId: string, speaking: boolean) => void;
}

const STUN_URL =
  process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302';
const SPEAKING_SAMPLE_INTERVAL_MS = 120;
const SPEAKING_RMS_THRESHOLD = 0.03;
const SPEAKING_HANG_MS = 300;

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

export default function VoiceEngine({
  localStream,
  channelId,
  currentUserId,
  existingMembers,
  wsEvents,
  sendWs,
  deafened,
  remoteVolumes,
  onSpeakingChange,
}: Props) {
  const peersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const audioElementsRef = useRef<Map<string, HTMLAudioElement>>(new Map());
  const speakingMonitorsRef = useRef<Map<string, SpeakingMonitor>>(new Map());
  const audioContextRef = useRef<AudioContext | null>(null);

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

  function addLocalTracks(pc: RTCPeerConnection) {
    if (!localStream) return;
    localStream.getTracks().forEach((track) => pc.addTrack(track, localStream!));
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
      el.style.display = 'none';
      document.body.appendChild(el);
      audioElementsRef.current.set(userId, el);
    }
    el.muted = deafened;
    el.volume = getPeerVolume(userId);
    el.srcObject = stream;
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
      if (audioContextRef.current) {
        void audioContextRef.current.close().catch(() => {});
        audioContextRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!localStream) {
      stopSpeakingMonitor(currentUserId);
      return;
    }
    startSpeakingMonitor(currentUserId, localStream);
    return () => stopSpeakingMonitor(currentUserId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [localStream, currentUserId, channelId]);

  useEffect(() => {
    audioElementsRef.current.forEach((audio, userId) => {
      audio.muted = deafened;
      audio.volume = getPeerVolume(userId);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deafened, remoteVolumes]);

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
