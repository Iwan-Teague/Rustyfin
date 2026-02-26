'use client';

import { useCallback, useEffect, useState } from 'react';
import { apiFetch } from '@/lib/api';
import {
  cancelVoiceTranscription,
  deleteVoiceTranscriptionSession,
  getVoiceTranscriptionStatus,
  listVoiceTranscriptionSessions,
  startVoiceTranscription,
  stopVoiceTranscription,
} from '@/lib/channelsApi';
import { useChannels } from '@/lib/channelsContext';
import type {
  ChannelEvent,
  ChannelInfo,
  UserInfo,
  VoiceTranscriptionSessionSummary,
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
  muted,
  deafened,
  showSelfControls,
  canToggleMute,
  onToggleMute,
  onToggleDeafen,
  onVolumeChange,
}: {
  userInfo: UserInfo;
  isSpeaking: boolean;
  isSelf: boolean;
  sliderLabel: string;
  sliderDisabled: boolean;
  volume: number;
  muted: boolean;
  deafened: boolean;
  showSelfControls: boolean;
  canToggleMute: boolean;
  onToggleMute: () => void;
  onToggleDeafen: () => void;
  onVolumeChange: (nextVolume: number) => void;
}) {
  const color = hashColor(userInfo.user_id);
  const initials = userInfo.username.slice(0, 2).toUpperCase();
  const volumePercent = Math.round(Math.min(1, Math.max(0, volume)) * 100);
  return (
    <div className="tile flex flex-col items-center gap-3 p-6 min-w-[140px]">
      <div>
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
      {isSelf && (
        <div className="flex items-center gap-2">
          <span className="text-[11px] muted">You</span>
          {showSelfControls && (
            <div className="flex items-center gap-1">
              <button
                type="button"
                onClick={onToggleMute}
                disabled={!canToggleMute}
                className={`inline-flex h-7 w-7 items-center justify-center rounded-full border transition disabled:opacity-40 disabled:cursor-not-allowed ${
                  muted
                    ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                    : 'border-[var(--border)] bg-black/45 text-white/80 hover:text-white'
                }`}
                aria-label={muted ? 'Unmute microphone' : 'Mute microphone'}
                title={muted ? 'Unmute microphone' : 'Mute microphone'}
              >
                <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                  <rect
                    x="9"
                    y="3.5"
                    width="6"
                    height="10"
                    rx="3"
                    stroke="currentColor"
                    strokeWidth="1.8"
                  />
                  <path d="M6.5 11.5a5.5 5.5 0 0 0 11 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  <path d="M12 17v3.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  <path d="M8.5 20.5h7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  {muted && (
                    <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  )}
                </svg>
              </button>
              <button
                type="button"
                onClick={onToggleDeafen}
                className={`inline-flex h-7 w-7 items-center justify-center rounded-full border transition ${
                  deafened
                    ? 'border-[var(--orange-soft)] bg-black/65 text-[var(--orange-soft)]'
                    : 'border-[var(--border)] bg-black/45 text-white/80 hover:text-white'
                }`}
                aria-label={deafened ? 'Undeafen' : 'Deafen'}
                title={deafened ? 'Undeafen' : 'Deafen'}
              >
                <svg viewBox="0 0 24 24" className="h-3.5 w-3.5" fill="none" aria-hidden="true">
                  <path d="M4 12a8 8 0 0 1 16 0" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  <rect x="2.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                  <rect x="17.5" y="12" width="4" height="7" rx="1.5" stroke="currentColor" strokeWidth="1.8" />
                  <path d="M17.5 18.5a4.5 4.5 0 0 1-4.5 4.5h-1" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  {deafened && (
                    <path d="M4.5 4.5l15 15" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
                  )}
                </svg>
              </button>
            </div>
          )}
        </div>
      )}
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
  const [transcriptionStarting, setTranscriptionStarting] = useState(false);
  const [transcriptionError, setTranscriptionError] = useState<string | null>(null);
  const [transcriptSessions, setTranscriptSessions] = useState<VoiceTranscriptionSessionSummary[]>([]);
  const [loadingTranscriptSessions, setLoadingTranscriptSessions] = useState(false);

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

  const loadTranscriptSessions = useCallback(
    async (showError = false) => {
      setLoadingTranscriptSessions(true);
      try {
        const response = await listVoiceTranscriptionSessions(channel.id);
        setTranscriptSessions(response.sessions);
      } catch (err) {
        if (showError) {
          setTranscriptionError(
            err instanceof Error ? err.message : 'Unable to load transcript history',
          );
        }
      } finally {
        setLoadingTranscriptSessions(false);
      }
    },
    [channel.id],
  );

  useEffect(() => {
    let cancelled = false;
    setLoadingTranscriptSessions(true);
    void Promise.all([
      getVoiceTranscriptionStatus(channel.id),
      listVoiceTranscriptionSessions(channel.id),
    ])
      .then(([status, sessions]) => {
        if (cancelled) return;
        setVoiceTranscriptionState(channel.id, mapStatusToState(status));
        setTranscriptSessions(sessions.sessions);
      })
      .catch(() => {
        if (cancelled) return;
        setTranscriptionError('Unable to load transcription state for this channel.');
      })
      .finally(() => {
        if (!cancelled) setLoadingTranscriptSessions(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel.id]);

  useEffect(() => {
    if (wsEvents?.type !== 'voice_transcription_state') return;
    if (wsEvents.channel_id !== channel.id) return;
    void loadTranscriptSessions(false);
  }, [wsEvents, channel.id, loadTranscriptSessions]);

  useEffect(() => {
    if (transcriptionState?.status === 'running') {
      setTranscriptionStarting(false);
    }
  }, [transcriptionState?.status]);

  async function handleStartTranscription() {
    setTranscriptionBusy(true);
    setTranscriptionStarting(true);
    setTranscriptionError(null);
    try {
      const status = await startVoiceTranscription(channel.id);
      setVoiceTranscriptionState(channel.id, mapStatusToState(status));
      await loadTranscriptSessions(false);
      if (status.status !== 'running') {
        setTranscriptionStarting(false);
      }
    } catch (err) {
      setTranscriptionStarting(false);
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
      await loadTranscriptSessions(false);
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
      await loadTranscriptSessions(false);
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to cancel transcription');
    } finally {
      setTranscriptionBusy(false);
    }
  }

  async function handleDownloadTranscription(sessionId: string) {
    if (!sessionId) {
      return;
    }
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const path = `/channels/${channel.id}/transcription/sessions/${sessionId}/download`;
      const response = await apiFetch(path, { method: 'GET' });
      if (!response.ok) {
        throw new Error('failed to download transcript');
      }
      const blob = await response.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = downloadUrl;
      link.download = `voice-transcript-${sessionId}.md`;
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

  async function handleDeleteTranscription(sessionId: string) {
    if (!sessionId) {
      return;
    }
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      await deleteVoiceTranscriptionSession(channel.id, sessionId);
      const status = await getVoiceTranscriptionStatus(channel.id);
      setVoiceTranscriptionState(channel.id, mapStatusToState(status));
      await loadTranscriptSessions(false);
    } catch (err) {
      setTranscriptionError(err instanceof Error ? err.message : 'Failed to delete transcript');
    } finally {
      setTranscriptionBusy(false);
    }
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
              {!voiceSession?.localStream && (
                <span className="text-xs muted px-2">Listening</span>
              )}
              <button onClick={handleDisconnect} className="btn-secondary px-3 py-1.5 text-sm text-red-400">
                Disconnect
              </button>
            </>
          )}
          {isConnected && (
            <>
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
                  aria-busy={transcriptionStarting}
                >
                  {transcriptionStarting ? (
                    <span className="inline-flex items-center gap-2">
                      <span className="h-3.5 w-3.5 rounded-full border-2 border-white/30 border-t-[var(--orange-soft)] animate-spin" />
                      Starting…
                    </span>
                  ) : (
                    'Start Transcript'
                  )}
                </button>
              )}
            </>
          )}
        </div>
      </div>
      {transcriptionStarting && (
        <div className="px-4 py-2 border-b border-[var(--border)] text-xs muted">
          Preparing transcription model and capture pipeline…
        </div>
      )}
      {transcriptionError && (
        <div className="px-4 py-2 border-b border-[var(--border)] text-xs text-red-300">
          {transcriptionError}
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        {/* Participant grid */}
        <div className="flex-1 overflow-y-auto p-4">
          {members.length === 0 ? (
            <p className="text-sm muted text-center mt-12">No one is here yet. Join to start!</p>
          ) : (
            <div className="flex flex-wrap gap-3">
              {members.map((u) => {
                const isSelf = u.user_id === currentUserId;
                return (
                  <ParticipantCard
                    key={u.user_id}
                    userInfo={u}
                    isSpeaking={speakingIds.has(u.user_id)}
                    isSelf={isSelf}
                    sliderLabel={
                      isSelf
                        ? 'Adjust your microphone volume'
                        : `Adjust ${u.username} volume`
                    }
                    sliderDisabled={isSelf && !Boolean(voiceSession?.localStream)}
                    volume={isSelf ? localMicGain : (remoteVolumes[u.user_id] ?? 1)}
                    muted={muted}
                    deafened={deafened}
                    showSelfControls={isSelf && isConnected}
                    canToggleMute={Boolean(voiceSession?.localStream)}
                    onToggleMute={toggleMute}
                    onToggleDeafen={toggleDeafen}
                    onVolumeChange={(nextVolume) => {
                      if (isSelf) {
                        setLocalMicGain(nextVolume);
                        return;
                      }
                      setRemoteVolume(u.user_id, nextVolume);
                    }}
                  />
                );
              })}
            </div>
          )}
        </div>

        {/* Transcript history */}
        <aside className="w-60 min-w-[200px] border-l border-[var(--border)] bg-[var(--surface)]/30 p-3 overflow-y-auto">
          <div className="mb-3 flex items-center justify-between gap-2">
            <h3 className="text-sm font-semibold">Transcripts</h3>
            <span className="chip text-[11px]">{transcriptSessions.length}</span>
          </div>
          {loadingTranscriptSessions ? (
            <p className="text-xs muted">Loading transcripts…</p>
          ) : transcriptSessions.length === 0 ? (
            <p className="text-xs muted">No transcripts saved for this voice channel yet.</p>
          ) : (
            <ul className="space-y-2">
              {transcriptSessions.map((session) => (
                <li key={session.session_id} className="tile rounded-xl p-2.5 space-y-2">
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <p className="truncate text-xs font-medium">
                        {new Date(session.started_ts * 1000).toLocaleString()}
                      </p>
                      <p className="truncate text-[11px] muted">
                        by {session.started_by_username}
                      </p>
                    </div>
                  </div>
                  <p className="text-[11px] muted">
                    {session.entry_count} line{session.entry_count === 1 ? '' : 's'}
                  </p>
                  {session.output_available ? (
                    <div className="grid grid-cols-2 gap-2">
                      <button
                        type="button"
                        className="btn-secondary w-full px-2 py-1 text-xs"
                        onClick={() => void handleDownloadTranscription(session.session_id)}
                        disabled={transcriptionBusy}
                      >
                        Download
                      </button>
                      <button
                        type="button"
                        className="btn-secondary w-full px-2 py-1 text-xs text-red-300"
                        onClick={() => void handleDeleteTranscription(session.session_id)}
                        disabled={transcriptionBusy}
                      >
                        Delete
                      </button>
                    </div>
                  ) : (
                    <>
                      <p className="text-[11px] muted">No downloadable output yet</p>
                      <button
                        type="button"
                        className="btn-secondary w-full px-2 py-1 text-xs text-red-300"
                        onClick={() => void handleDeleteTranscription(session.session_id)}
                        disabled={transcriptionBusy}
                      >
                        Delete
                      </button>
                    </>
                  )}
                </li>
              ))}
            </ul>
          )}
        </aside>
      </div>
    </div>
  );
}
