'use client';

import { useState } from 'react';
import { useChannels } from '@/lib/channelsContext';
import type { ChannelEvent, ChannelInfo, UserInfo } from '@/lib/channelsApi';

interface Props {
  channel: ChannelInfo;
  voicePresence: Record<string, UserInfo[]>;
  currentUserId: string;
  currentUsername: string;
  wsEvents: ChannelEvent | null;
}

function hashColor(userId: string): string {
  let hash = 0;
  for (let i = 0; i < userId.length; i++) {
    hash = userId.charCodeAt(i) + ((hash << 5) - hash);
  }
  const colors = [
    '#e67e22', '#3498db', '#2ecc71', '#9b59b6', '#e74c3c',
    '#1abc9c', '#f39c12', '#16a085', '#d35400', '#8e44ad',
  ];
  return colors[Math.abs(hash) % colors.length];
}

function ParticipantCard({
  userInfo,
  isSpeaking,
  isSelf,
  sliderLabel,
  sliderDisabled,
  volume,
  onVolumeChange,
}: {
  userInfo: UserInfo;
  isSpeaking: boolean;
  isSelf: boolean;
  sliderLabel: string;
  sliderDisabled: boolean;
  volume: number;
  onVolumeChange: (nextVolume: number) => void;
}) {
  const color = hashColor(userInfo.user_id);
  const initials = userInfo.username.slice(0, 2).toUpperCase();
  const volumePercent = Math.round(Math.min(1, Math.max(0, volume)) * 100);
  return (
    <div className="tile flex flex-col items-center gap-3 p-6 min-w-[140px]">
      <div
        className="rounded-full p-[3px] transition-all duration-150"
        style={
          isSpeaking
            ? {
                background:
                  'linear-gradient(115deg, var(--orange) 0%, var(--purple-strong) 75%)',
                boxShadow: '0 0 0 1px rgba(255, 145, 77, 0.35)',
              }
            : undefined
        }
      >
        <div
          className="w-28 h-28 rounded-full flex items-center justify-center text-4xl font-bold text-white"
          style={{ backgroundColor: color }}
        >
          {initials}
        </div>
      </div>
      <span className="text-sm font-medium">{userInfo.username}</span>
      <div className="w-full space-y-1">
        <div className="flex items-center justify-between text-[11px] muted">
          <span>{isSelf ? 'Input' : 'Output'}</span>
          <span>{volumePercent}%</span>
        </div>
        <input
          type="range"
          min={0}
          max={100}
          value={volumePercent}
          disabled={sliderDisabled}
          onChange={(event) => onVolumeChange(Number(event.target.value) / 100)}
          className="w-full accent-[var(--orange-soft)] disabled:opacity-40 disabled:cursor-not-allowed"
          aria-label={sliderLabel}
        />
      </div>
      {isSelf && <span className="text-[11px] muted">You</span>}
    </div>
  );
}

export default function VoiceChannelView({
  channel,
  voicePresence,
  currentUserId,
  currentUsername,
  wsEvents,
}: Props) {
  const {
    voiceSession,
    voiceSpeaking,
    remoteVolumes,
    localMicGain,
    joinVoice,
    leaveVoice,
    toggleMute,
    toggleDeafen,
    setRemoteVolume,
    setLocalMicGain,
  } = useChannels();
  const [error, setError] = useState<string | null>(null);

  const members = voicePresence[channel.id] ?? [];
  const speakingIds = new Set(voiceSpeaking[channel.id] ?? []);
  const isConnected = voiceSession?.channelId === channel.id;
  const muted = isConnected ? (voiceSession?.muted ?? false) : false;
  const deafened = isConnected ? (voiceSession?.deafened ?? false) : false;

  async function handleConnect() {
    setError(null);
    const err = await joinVoice(channel.id, channel.name);
    if (err) setError(err);
  }

  function handleDisconnect() {
    leaveVoice();
  }

  return (
    <div className="flex flex-col flex-1 h-full overflow-hidden">
      {/* Header — channel name, member count, and controls all inline */}
      <div className="h-14 px-4 border-b border-[var(--border)] flex items-center gap-2 shrink-0 overflow-x-auto whitespace-nowrap">
        <span className="font-semibold truncate">{channel.name}</span>
        <span className="chip text-xs shrink-0">
          <span className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-[var(--border)] bg-black/20 text-[10px] font-semibold mr-1">
            {members.length}
          </span>
          member{members.length !== 1 ? 's' : ''}
        </span>

        <div className="ml-auto flex items-center gap-2 shrink-0">
          {error && <p className="text-xs text-red-400 max-w-[12rem] truncate">{error}</p>}

          {!isConnected ? (
            <button onClick={handleConnect} className="btn-primary px-4 py-1.5 text-sm">
              Connect
            </button>
          ) : (
            <>
              {voiceSession?.localStream ? (
                <button
                  onClick={toggleMute}
                  className={`btn-secondary px-3 py-1.5 text-sm ${muted ? 'opacity-60' : ''}`}
                >
                  {muted ? 'Unmute' : 'Mute'}
                </button>
              ) : (
                <span className="text-xs muted px-2">Listening</span>
              )}
              <button
                onClick={toggleDeafen}
                className={`btn-secondary px-3 py-1.5 text-sm ${deafened ? 'text-[var(--orange-soft)]' : ''}`}
              >
                {deafened ? 'Undeafen' : 'Deafen'}
              </button>
              <button onClick={handleDisconnect} className="btn-secondary px-3 py-1.5 text-sm text-red-400">
                Disconnect
              </button>
            </>
          )}
        </div>
      </div>

      {/* Participant grid */}
      <div className="flex-1 overflow-y-auto p-4">
        {members.length === 0 ? (
          <p className="text-sm muted text-center mt-12">No one is here yet. Join to start!</p>
        ) : (
          <div className="flex flex-wrap gap-3">
            {members.map((u) => (
              <ParticipantCard
                key={u.user_id}
                userInfo={u}
                isSpeaking={speakingIds.has(u.user_id)}
                isSelf={u.user_id === currentUserId}
                sliderLabel={
                  u.user_id === currentUserId
                    ? `Adjust your microphone volume`
                    : `Adjust ${u.username} volume`
                }
                sliderDisabled={u.user_id === currentUserId && !Boolean(voiceSession?.localStream)}
                volume={u.user_id === currentUserId ? localMicGain : (remoteVolumes[u.user_id] ?? 1)}
                onVolumeChange={(nextVolume) => {
                  if (u.user_id === currentUserId) {
                    setLocalMicGain(nextVolume);
                    return;
                  }
                  setRemoteVolume(u.user_id, nextVolume);
                }}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
