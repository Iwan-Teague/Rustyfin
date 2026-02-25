'use client';

import { useEffect, useState } from 'react';
import { apiFetch } from '@/lib/api';
import {
  cancelVoiceTranscription,
  getVoiceTranscriptionStatus,
  startVoiceTranscription,
  stopVoiceTranscription,
} from '@/lib/channelsApi';
import { useChannels } from '@/lib/channelsContext';
import type {
  ChannelEvent,
  ChannelInfo,
  UserInfo,
  VoiceTranscriptionState,
  VoiceTranscriptionStatus,
} from '@/lib/channelsApi';

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
    voiceTranscriptions,
    remoteVolumes,
    localMicGain,
    joinVoice,
    leaveVoice,
    toggleMute,
    toggleDeafen,
    setRemoteVolume,
    setLocalMicGain,
    setVoiceTranscriptionState,
  } = useChannels();
  const [error, setError] = useState<string | null>(null);
  const [transcriptionBusy, setTranscriptionBusy] = useState(false);
  const [transcriptionError, setTranscriptionError] = useState<string | null>(null);

  const members = voicePresence[channel.id] ?? [];
  const speakingIds = new Set(voiceSpeaking[channel.id] ?? []);
  const isConnected = voiceSession?.channelId === channel.id;
  const transcriptionState = voiceTranscriptions[channel.id] ?? null;
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

  function mapStatusToState(status: VoiceTranscriptionStatus): VoiceTranscriptionState {
    return {
      status: status.status,
      session_id: status.session_id ?? null,
      started_by_username: status.started_by_username ?? null,
      started_ts: status.started_ts ?? null,
      ended_ts: status.ended_ts ?? null,
      output_available: status.output_available,
      message: status.message ?? null,
    };
  }

  useEffect(() => {
    let cancelled = false;
    void getVoiceTranscriptionStatus(channel.id)
      .then((status) => {
        if (cancelled) return;
        setVoiceTranscriptionState(channel.id, mapStatusToState(status));
      })
      .catch(() => {
        if (!cancelled) {
          setTranscriptionError('Unable to load transcription state for this channel.');
        }
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel.id]);

  async function handleStartTranscription() {
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const status = await startVoiceTranscription(channel.id);
      setVoiceTranscriptionState(channel.id, mapStatusToState(status));
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to start transcription');
    } finally {
      setTranscriptionBusy(false);
    }
  }

  async function handleStopTranscription() {
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const status = await stopVoiceTranscription(channel.id);
      setVoiceTranscriptionState(channel.id, mapStatusToState(status));
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to stop transcription');
    } finally {
      setTranscriptionBusy(false);
    }
  }

  async function handleCancelTranscription() {
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const status = await cancelVoiceTranscription(channel.id);
      setVoiceTranscriptionState(channel.id, mapStatusToState(status));
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to cancel transcription');
    } finally {
      setTranscriptionBusy(false);
    }
  }

  async function handleDownloadTranscription() {
    if (!transcriptionState?.session_id || !transcriptionState.output_available) {
      return;
    }
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const path = `/channels/${channel.id}/transcription/sessions/${transcriptionState.session_id}/download`;
      const response = await apiFetch(path, { method: 'GET' });
      if (!response.ok) {
        throw new Error('failed to download transcript');
      }
      const blob = await response.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = downloadUrl;
      link.download = `voice-transcript-${transcriptionState.session_id}.md`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(downloadUrl);
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to download transcript');
    } finally {
      setTranscriptionBusy(false);
    }
  }

  const transcriptionStatusLabel = (() => {
    const status = transcriptionState?.status ?? 'idle';
    switch (status) {
      case 'running':
        return 'Running';
      case 'finalizing':
        return 'Finalizing';
      case 'completed':
        return 'Completed';
      case 'cancelled':
        return 'Cancelled';
      case 'failed':
        return 'Failed';
      default:
        return 'Idle';
    }
  })();

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
          {isConnected && (
            <>
              <span className="chip text-xs" title="Channel transcript status">
                Transcript: {transcriptionStatusLabel}
              </span>
              {transcriptionState?.status === 'running' ? (
                <>
                  <button
                    onClick={handleStopTranscription}
                    disabled={transcriptionBusy}
                    className="btn-secondary px-3 py-1.5 text-sm"
                  >
                    Stop &amp; Save
                  </button>
                  <button
                    onClick={handleCancelTranscription}
                    disabled={transcriptionBusy}
                    className="btn-secondary px-3 py-1.5 text-sm text-red-300"
                  >
                    Cancel
                  </button>
                </>
              ) : (
                <button
                  onClick={handleStartTranscription}
                  disabled={transcriptionBusy}
                  className="btn-secondary px-3 py-1.5 text-sm"
                >
                  Start Transcript
                </button>
              )}
              {transcriptionState?.output_available && transcriptionState?.session_id && (
                <button
                  onClick={handleDownloadTranscription}
                  disabled={transcriptionBusy}
                  className="btn-secondary px-3 py-1.5 text-sm"
                >
                  Download
                </button>
              )}
            </>
          )}
        </div>
      </div>
      {transcriptionError && (
        <div className="px-4 py-2 border-b border-[var(--border)] text-xs text-red-300">
          {transcriptionError}
        </div>
      )}

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
