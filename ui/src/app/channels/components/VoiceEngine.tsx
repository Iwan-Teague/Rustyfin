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
}

const STUN_URL =
  process.env.NEXT_PUBLIC_STUN_URL ?? 'stun:stun.l.google.com:19302';

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
}: Props) {
  const peersRef = useRef<Map<string, RTCPeerConnection>>(new Map());
  const audioElementsRef = useRef<Map<string, HTMLAudioElement>>(new Map());

  function addLocalTracks(pc: RTCPeerConnection) {
    if (!localStream) return;
    localStream.getTracks().forEach((track) => pc.addTrack(track, localStream!));
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
    el.srcObject = stream;
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
