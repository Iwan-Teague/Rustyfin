'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Image from 'next/image';
import { apiFetch } from '@/lib/api';
import {
  cancelVoiceTranscription,
  deleteVoiceTranscriptionSession,
  getVoiceTranscriptionStatus,
  listVoiceTranscriptionSessions,
  startVoiceTranscription,
  stopVoiceTranscription,
} from '@/lib/channelsApi';
import { findDataDeleteTarget, playTelegramDeleteAnimation } from '@/lib/deleteAnimation';
import { useChannels } from '@/lib/channelsContext';
import type {
  ChannelEvent,
  ChannelInfo,
  UserInfo,
  VoiceTranscriptionSessionSummary,
  VoiceTranscriptionState,
  VoiceTranscriptionStatus,
} from '@/lib/channelsApi';

const DELETE_AFTER_CONFIRM_DELAY_MS = 500;

interface Props {
  channel: ChannelInfo;
  voicePresence: Record<string, UserInfo[]>;
  currentUserId: string;
  currentUsername: string;
  wsEvents: ChannelEvent | null;
  onToggleSidebar: () => void;
  sidebarVisible: boolean;
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

function sanitizeFileStem(value: string): string {
  const cleaned = value
    .trim()
    .replace(/[<>:"/\\|?*\u0000-\u001F]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^[-.]+|[-.]+$/g, '');
  return cleaned || 'voice-channel';
}

function formatDateForFile(timestampSeconds?: number | null): string {
  const date = typeof timestampSeconds === 'number'
    ? new Date(timestampSeconds * 1000)
    : new Date();
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function ParticipantCard({
  userInfo,
  isSpeaking,
  isSelf,
  sliderLabel,
  sliderDisabled,
  volume,
  sliderMaxPercent,
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
  sliderMaxPercent: number;
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
  const safeMaxPercent = Math.max(100, sliderMaxPercent);
  const maxVolume = safeMaxPercent / 100;
  const volumePercent = Math.round(Math.min(maxVolume, Math.max(0, volume)) * 100);
  const sliderFillPercent = Math.max(
    0,
    Math.min(100, (volumePercent / safeMaxPercent) * 100),
  );
  return (
    <div className="flex min-w-[140px] flex-col items-center gap-3 rounded-2xl border border-[var(--border-subtle)] px-5 py-6">
      <div>
        <div
          className="rounded-full p-[3px] transition-all duration-150"
          style={
            isSpeaking
              ? {
                  background:
                    'linear-gradient(115deg, var(--orange) 0%, var(--purple-strong) 75%)',
                }
              : undefined
          }
        >
          {userInfo.avatar_url ? (
            <Image
              src={userInfo.avatar_url}
              alt={userInfo.username}
              width={112}
              height={112}
              unoptimized
              className="h-28 w-28 rounded-full border border-[var(--border)] bg-black/20 object-cover"
              loading="lazy"
            />
          ) : (
            <div
              className="w-28 h-28 rounded-full flex items-center justify-center text-4xl font-bold text-white"
              style={{ backgroundColor: color }}
            >
              {initials}
            </div>
          )}
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
          max={safeMaxPercent}
          value={volumePercent}
          disabled={sliderDisabled}
          onChange={(event) => onVolumeChange(Number(event.target.value) / 100)}
          className="rf-gradient-slider w-full disabled:opacity-40 disabled:cursor-not-allowed"
          style={{ ['--rf-slider-value' as string]: `${sliderFillPercent}%` }}
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
  onToggleSidebar,
  sidebarVisible,
}: Props) {
  const {
    voiceSession,
    connectedVoiceChannelId,
    hasLocalVoiceSession,
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
  const [desktopTranscriptOpen, setDesktopTranscriptOpen] = useState(true);
  const [mobileTranscriptOpen, setMobileTranscriptOpen] = useState(false);

  const members = voicePresence[channel.id] ?? [];
  const speakingIds = new Set(voiceSpeaking[channel.id] ?? []);
  const isConnectedHere = voiceSession?.channelId === channel.id;
  const isConnectedElsewhere = connectedVoiceChannelId === channel.id && !isConnectedHere;
  const isConnected = isConnectedHere || isConnectedElsewhere;
  const transcriptionState = voiceTranscriptions[channel.id] ?? null;
  const muted = isConnectedHere ? (voiceSession?.muted ?? false) : false;
  const deafened = isConnectedHere ? (voiceSession?.deafened ?? false) : false;
  const downloadableTranscriptSessions = useMemo(
    () =>
      transcriptSessions.filter(
        (session: VoiceTranscriptionSessionSummary) => session.output_available,
      ),
    [transcriptSessions],
  );

  async function handleConnect() {
    setError(null);
    if (isConnectedElsewhere) {
      setError('You are already connected to this channel in another tab.');
      return;
    }
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
    if (wsEvents?.type !== 'error') return;
    setError(wsEvents.message);
  }, [wsEvents]);

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

  async function handleDownloadTranscription(session: VoiceTranscriptionSessionSummary) {
    if (!session.session_id) {
      return;
    }
    setTranscriptionBusy(true);
    setTranscriptionError(null);
    try {
      const path = `/channels/${channel.id}/transcription/sessions/${session.session_id}/download`;
      const response = await apiFetch(path, { method: 'GET' });
      if (!response.ok) {
        throw new Error('failed to download transcript');
      }
      const blob = await response.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = downloadUrl;
      link.download = `${sanitizeFileStem(channel.name)}-${formatDateForFile(session.started_ts)}.md`;
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
      await new Promise<void>((resolve) => {
        window.setTimeout(resolve, DELETE_AFTER_CONFIRM_DELAY_MS);
      });
      const target = findDataDeleteTarget('data-transcript-session-id', sessionId);
      await playTelegramDeleteAnimation(target);
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
        <button
          type="button"
          className="rf-inline-icon-btn h-9 w-9 text-lg leading-none"
          onClick={onToggleSidebar}
          aria-label={sidebarVisible ? 'Hide channels' : 'Show channels'}
        >
          ☰
        </button>
        <span className="font-semibold truncate">{channel.name}</span>
        <span className="shrink-0 pl-2 text-xs text-white/55">
          {members.length} member{members.length !== 1 ? 's' : ''}
        </span>

        <div className="ml-auto flex items-center gap-5 shrink-0">
          {error && <p className="text-xs text-red-400 max-w-[12rem] truncate">{error}</p>}

          {isConnectedHere && (
            <>
              {!voiceSession?.localStream && (
                <span className="text-xs muted px-2">Listening</span>
              )}
            </>
          )}
          {isConnectedElsewhere && (
            <span className="text-xs muted px-2">Connected in another tab</span>
          )}
          <button
            type="button"
            onClick={() => {
              if (typeof window !== 'undefined' && window.innerWidth >= 768) {
                setDesktopTranscriptOpen((current) => !current);
              } else {
                setMobileTranscriptOpen((current) => !current);
              }
            }}
            className="rf-text-action text-sm"
          >
            Transcripts
          </button>
          <button
            onClick={isConnectedHere ? handleDisconnect : () => void handleConnect()}
            disabled={isConnectedElsewhere}
            className={`text-sm ${
              isConnectedHere
                ? 'rf-text-action rf-text-action-danger'
                : isConnectedElsewhere
                  ? 'rf-text-action rf-text-action-muted disabled:opacity-60'
                  : 'rf-text-action'
            }`}
          >
            {isConnectedHere
              ? 'Disconnect'
              : isConnectedElsewhere
                ? 'Connected in another tab'
                : 'Connect'}
          </button>
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
                    sliderMaxPercent={isSelf ? 100 : 200}
                    muted={muted}
                    deafened={deafened}
                    showSelfControls={isSelf && isConnectedHere && hasLocalVoiceSession}
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
        <aside
          className="ai-side-panel-shell ai-side-panel-shell-right hidden md:flex md:min-h-0 md:flex-col md:overflow-hidden"
          data-open={desktopTranscriptOpen ? 'true' : 'false'}
          data-side="right"
          style={{ width: desktopTranscriptOpen ? '16rem' : '0px' }}
        >
          <div className="ai-side-panel-inner flex h-full min-h-0 flex-col border-l border-[var(--border)] px-3 py-4">
            <div className="mb-3 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-sm font-semibold">Transcripts</h3>
                <span className="text-[11px] text-white/55">{downloadableTranscriptSessions.length}</span>
              </div>
              {isConnectedHere ? (
                transcriptionState?.status === 'running' ? (
                  <div className="flex flex-wrap gap-x-4 gap-y-1">
                    <button
                      type="button"
                      onClick={handleStopTranscription}
                      disabled={transcriptionBusy}
                      className="rf-text-action text-sm"
                    >
                      Stop & Save
                    </button>
                    <button
                      type="button"
                      onClick={handleCancelTranscription}
                      disabled={transcriptionBusy}
                      className="rf-text-action rf-text-action-danger text-sm"
                    >
                      Discard Transcript
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={handleStartTranscription}
                    disabled={transcriptionBusy}
                    className="rf-text-action text-sm"
                    aria-busy={transcriptionStarting}
                  >
                    {transcriptionStarting ? 'Starting…' : 'Start Transcript'}
                  </button>
                )
              ) : null}
            </div>
            {loadingTranscriptSessions ? (
              <p className="text-xs muted">Loading transcripts…</p>
            ) : downloadableTranscriptSessions.length === 0 ? (
              <p className="text-xs muted">
                {transcriptionState?.status === 'running' || transcriptionState?.status === 'finalizing'
                  ? 'Transcript is still running. It will appear here after Stop & Save.'
                  : 'No transcripts saved for this voice channel yet.'}
              </p>
            ) : (
              <ul className="rf-flat-list overflow-y-auto">
                {downloadableTranscriptSessions.map((session) => (
                  <li
                    key={session.session_id}
                    data-transcript-session-id={session.session_id}
                    className="rf-flat-row space-y-2"
                  >
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
                    <div className="flex flex-wrap gap-x-4 gap-y-1">
                      <button
                        type="button"
                        className="rf-text-action text-xs"
                        onClick={() => void handleDownloadTranscription(session)}
                        disabled={transcriptionBusy}
                      >
                        Download
                      </button>
                      <button
                        type="button"
                        className="rf-text-action rf-text-action-danger text-xs"
                        onClick={() => void handleDeleteTranscription(session.session_id)}
                        disabled={transcriptionBusy}
                      >
                        Delete
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>
      </div>

      {mobileTranscriptOpen ? (
        <div
          className="fixed inset-0 z-50 flex justify-end bg-black/55 md:hidden"
          onClick={() => setMobileTranscriptOpen(false)}
        >
          <aside
            className="flex h-full w-[18rem] max-w-[86vw] flex-col border-l border-[var(--border)] bg-[var(--surface)] px-3 py-4"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="mb-3 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-sm font-semibold">Transcripts</h3>
                <button
                  type="button"
                  className="rf-inline-icon-btn h-8 w-8 text-lg"
                  onClick={() => setMobileTranscriptOpen(false)}
                  aria-label="Close transcripts"
                >
                  ×
                </button>
              </div>
              {isConnectedHere ? (
                transcriptionState?.status === 'running' ? (
                  <div className="flex flex-wrap gap-x-4 gap-y-1">
                    <button
                      type="button"
                      onClick={handleStopTranscription}
                      disabled={transcriptionBusy}
                      className="rf-text-action text-sm"
                    >
                      Stop & Save
                    </button>
                    <button
                      type="button"
                      onClick={handleCancelTranscription}
                      disabled={transcriptionBusy}
                      className="rf-text-action rf-text-action-danger text-sm"
                    >
                      Discard Transcript
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={handleStartTranscription}
                    disabled={transcriptionBusy}
                    className="rf-text-action text-sm"
                    aria-busy={transcriptionStarting}
                  >
                    {transcriptionStarting ? 'Starting…' : 'Start Transcript'}
                  </button>
                )
              ) : null}
            </div>
            {loadingTranscriptSessions ? (
              <p className="text-xs muted">Loading transcripts…</p>
            ) : downloadableTranscriptSessions.length === 0 ? (
              <p className="text-xs muted">
                {transcriptionState?.status === 'running' || transcriptionState?.status === 'finalizing'
                  ? 'Transcript is still running. It will appear here after Stop & Save.'
                  : 'No transcripts saved for this voice channel yet.'}
              </p>
            ) : (
              <ul className="rf-flat-list overflow-y-auto">
                {downloadableTranscriptSessions.map((session) => (
                  <li
                    key={session.session_id}
                    data-transcript-session-id={session.session_id}
                    className="rf-flat-row space-y-2"
                  >
                    <div className="min-w-0">
                      <p className="truncate text-xs font-medium">
                        {new Date(session.started_ts * 1000).toLocaleString()}
                      </p>
                      <p className="truncate text-[11px] muted">
                        by {session.started_by_username}
                      </p>
                    </div>
                    <p className="text-[11px] muted">
                      {session.entry_count} line{session.entry_count === 1 ? '' : 's'}
                    </p>
                    <div className="flex flex-wrap gap-x-4 gap-y-1">
                      <button
                        type="button"
                        className="rf-text-action text-xs"
                        onClick={() => void handleDownloadTranscription(session)}
                        disabled={transcriptionBusy}
                      >
                        Download
                      </button>
                      <button
                        type="button"
                        className="rf-text-action rf-text-action-danger text-xs"
                        onClick={() => void handleDeleteTranscription(session.session_id)}
                        disabled={transcriptionBusy}
                      >
                        Delete
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </aside>
        </div>
      ) : null}
    </div>
  );
}
