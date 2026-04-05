'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useRouter } from 'next/navigation';
import { createPortal } from 'react-dom';
import ConfirmModal from '@/app/components/ConfirmModal';
import VideoPlayerSurface, { filterPlaybackQualityOptions } from '@/app/components/VideoPlayerSurface';
import { useAuth } from '@/lib/auth';
import { apiFetch, apiJson } from '@/lib/api';
import {
  WatchPartyUser,
  WsRoomReconfiguredMessage,
  endWatchPartyRoom,
  inviteToRoom,
  joinWatchPartyRoom,
  leaveWatchPartyRoom,
  listWatchPartyUsers,
} from '@/lib/watchPartyApi';
import { formatElapsedSeconds } from '@/lib/time';
import { clientErrorMessage } from '@/lib/errors';
import { nonAdminRoleLabel, roleLabel } from '@/lib/watchPartyRoles';
import AudioPlayer from '../components/AudioPlayer';
import CreateToolTabsBar from '../components/CreateToolTabsBar';
import CreateTogetherEditor from '../components/CreateTogetherEditor';
import MediaPicker from '../components/MediaPicker';
import PlayTogetherChess from '../components/PlayTogetherChess';
import ScreenPlayer from '../components/ScreenPlayer';
import WatchSourceTabsBar from '../components/WatchSourceTabsBar';
import WebPlayer from '../components/WebPlayer';
import YouTubePlayer from '../components/YouTubePlayer';
import { useRoomPlayback } from '../hooks/useRoomPlayback';
import { useRoomReconfigure } from '../hooks/useRoomReconfigure';
import { useRoomRealtime } from '../hooks/useRoomRealtime';
import { useWatchRoomData } from '../hooks/useWatchRoomData';

const INVITE_NAME_MAX_CHARS = 14;

type RoomPresenceSnapshot = {
  user_id: string;
  username: string;
  role: string;
  connected: boolean;
};

type RoomItemSummary = {
  id: string;
  title: string;
  poster_url?: string | null;
  thumb_url?: string | null;
  backdrop_url?: string | null;
};

function formatPlaybackClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${minutes}:${String(seconds).padStart(2, '0')}`;
}

function truncateInviteName(name: string): string {
  if (name.length <= INVITE_NAME_MAX_CHARS) return name;
  return `${name.slice(0, INVITE_NAME_MAX_CHARS)}…`;
}

function normalizeRoomPresenceMember(
  member: Partial<RoomPresenceSnapshot> | null | undefined,
): RoomPresenceSnapshot {
  const normalizedUserId =
    typeof member?.user_id === 'string' && member.user_id.trim().length > 0
      ? member.user_id
      : 'unknown-user';
  const normalizedUsername =
    typeof member?.username === 'string' && member.username.trim().length > 0
      ? member.username
      : 'Unknown user';
  const normalizedRole =
    typeof member?.role === 'string' && member.role.trim().length > 0 ? member.role : 'viewer';
  return {
    user_id: normalizedUserId,
    username: normalizedUsername,
    role: normalizedRole,
    connected: member?.connected === true,
  };
}

function fallbackVideoDownloadName(itemId: string, targetHeight: number | null): string {
  if (targetHeight && targetHeight > 0) {
    return `rustyfin-room-${itemId}-${targetHeight}p.mp4`;
  }
  return `rustyfin-room-${itemId}.bin`;
}

function extractDownloadFilename(header: string | null, fallback: string): string {
  if (!header) return fallback;
  const utf8Match = header.match(/filename\\*=UTF-8''([^;]+)/i);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch {
      return utf8Match[1];
    }
  }
  const basicMatch = header.match(/filename=\"?([^\";]+)\"?/i);
  if (basicMatch?.[1]) {
    return basicMatch[1];
  }
  return fallback;
}

export default function WatchPartyRoomPage() {
  const params = useParams();
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const roomId = params.roomId as string;
  const roomVideoShellRef = useRef<HTMLDivElement>(null);

  const appendDebug = useCallback((message: string) => {
    if (typeof window !== 'undefined') {
      console.info(`[watch-party:${roomId}] ${message}`);
    }
  }, [roomId]);

  const [joinPassword, setJoinPassword] = useState('');
  const [joining, setJoining] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [ending, setEnding] = useState(false);
  const [pendingEndRoomConfirm, setPendingEndRoomConfirm] = useState(false);
  const [roomItem, setRoomItem] = useState<RoomItemSummary | null>(null);
  const [mediaDrawerOpen, setMediaDrawerOpen] = useState(false);

  // In-room invite state
  const [allUsers, setAllUsers] = useState<WatchPartyUser[]>([]);
  const [inviteSelections, setInviteSelections] = useState<Record<string, 'viewer' | 'controller'>>({});
  const [sendingInvites, setSendingInvites] = useState(false);

  const [error, setError] = useState('');
  const [info, setInfo] = useState('');
  const [downloadingVideo, setDownloadingVideo] = useState(false);
  const {
    room,
    loadingRoom,
    joinedRole,
    setJoinedRole,
    loadRoom,
    refreshRoom,
  } = useWatchRoomData({
    roomId,
    me,
    setError,
    appendDebug,
  });
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [portalMounted, setPortalMounted] = useState(false);

  useEffect(() => {
    document.documentElement.dataset.rfPage = 'rooms';
    document.body.dataset.rfPage = 'rooms';
    return () => {
      delete document.documentElement.dataset.rfPage;
      delete document.body.dataset.rfPage;
    };
  }, []);

  const isAudioRoom = room?.room_mode === 'audio';
  const isWebRoom = room?.room_mode === 'web';
  const isScreenRoom = room?.room_mode === 'screen';
  const isYoutubeRoom = room?.room_mode === 'youtube';
  const isCreateRoom = room?.room_mode === 'create';
  const isPlayRoom = room?.room_mode === 'play';
  const isVideoRoom =
    room?.room_mode === 'video' ||
    (!isAudioRoom &&
      !isWebRoom &&
      !isScreenRoom &&
      !isYoutubeRoom &&
      !isCreateRoom &&
      !isPlayRoom);
  const isWatchRoom = isVideoRoom || isYoutubeRoom || isWebRoom || isScreenRoom;
  const activeWatchSource: 'video' | 'youtube' | 'web' | 'screen' = isYoutubeRoom
    ? 'youtube'
    : isWebRoom
      ? 'web'
      : isScreenRoom
        ? 'screen'
      : 'video';
  const watchWindowShiftClass = isWatchRoom ? 'mt-[55px]' : '';
  const watchTabsCounterShiftClass = isWatchRoom ? 'top-[-17px]' : 'top-0';
  const createWindowShiftClass = isCreateRoom ? 'mt-[54px]' : '';
  const createTabsCounterShiftClass = isCreateRoom ? 'top-[-17px]' : 'top-0';
  const createLowerPanelsShiftClass = isCreateRoom ? 'mt-[37px]' : '';
  const currentReconfigureCategory = isAudioRoom
    ? 'audio'
    : isPlayRoom
      ? 'play'
      : isCreateRoom
        ? 'create'
        : 'watch';

  const infoTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resetPlaybackRef = useRef<() => Promise<void>>(async () => {});

  const clearInfoTimeout = useCallback(() => {
    if (infoTimeoutRef.current) {
      clearTimeout(infoTimeoutRef.current);
      infoTimeoutRef.current = null;
    }
  }, []);

  const setInfoForDuration = useCallback((message: string, durationMs: number) => {
    clearInfoTimeout();
    setInfo(message);
    infoTimeoutRef.current = setTimeout(() => {
      setInfo((current) => (current === message ? '' : current));
      infoTimeoutRef.current = null;
    }, durationMs);
  }, [clearInfoTimeout]);

  const reconfigure = useRoomReconfigure({
    roomId,
    room,
    me,
    joinedRole,
    activeWatchSource,
    setError,
    setInfo,
    setInfoForDuration,
  });
  const selectedReconfigureCategory =
    reconfigure.reconfigureMode === 'audio'
      ? 'audio'
      : reconfigure.reconfigureMode === 'play'
        ? 'play'
        : reconfigure.reconfigureMode === 'create'
          ? 'create'
          : 'watch';

  const handleRealtimeRoomReconfigured = useCallback(
    (payload: WsRoomReconfiguredMessage) => {
      reconfigure.handleRemoteRoomReconfigured(payload);
      void resetPlaybackRef.current();
    },
    [reconfigure.handleRemoteRoomReconfigured],
  );

  const handleRealtimeRoomEnded = useCallback(() => {
    router.push('/rooms');
  }, [router]);

  const realtime = useRoomRealtime({
    roomId,
    joinedRole,
    appendDebug,
    setError,
    setInfo,
    refreshRoom,
    setJoinedRole,
    onRoomReconfigured: handleRealtimeRoomReconfigured,
    onRoomEnded: handleRealtimeRoomEnded,
  });

  const playback = useRoomPlayback({
    room,
    joinedRole,
    roomState: realtime.roomState,
    appendDebug,
    setError,
    setInfo,
  });

  useEffect(() => {
    resetPlaybackRef.current = playback.resetPlaybackState;
  }, [playback.resetPlaybackState]);

  useEffect(() => {
    if (realtime.roomState) {
      void playback.applyRemoteState(realtime.roomState);
    }
  }, [realtime.roomState, playback.applyRemoteState]);

  const activeCreateTool: 'text' | 'canvas' =
    realtime.createState?.active_tool === 'canvas' ? 'canvas' : 'text';
  const effectiveRoomMode = room?.room_mode ?? 'video';
  const memberRoleDisplay = nonAdminRoleLabel(effectiveRoomMode);
  const joinedRoleDisplay = joinedRole ? roleLabel(joinedRole, effectiveRoomMode) : '';

  const canPlayPause = useMemo(() => {
    if (!room || !joinedRole) return false;
    if (joinedRole === 'host') return true;
    return room.policy.allow_non_host_play_pause;
  }, [room, joinedRole]);

  const canSeek = useMemo(() => {
    if (!room || !joinedRole) return false;
    if (joinedRole === 'host') return true;
    return room.policy.allow_non_host_seek;
  }, [room, joinedRole]);

  const controlsEnabled = canPlayPause || canSeek || joinedRole === 'host';
  const qualityOptions = useMemo(
    () => filterPlaybackQualityOptions(playback.sourceVideoHeight),
    [playback.sourceVideoHeight],
  );

  const roomDurationSeconds = useMemo(() => {
    if (!room) return 0;
    const endTs = room.ended_ts ?? Math.floor(nowMs / 1000);
    return Math.max(0, endTs - room.created_ts);
  }, [room, nowMs]);
  const roomItemId = typeof room?.item_id === 'string' ? room.item_id : '';
  const roomLoadingArtworkUrl =
    roomItem?.thumb_url ?? roomItem?.poster_url ?? roomItem?.backdrop_url ?? null;
  const roomLoadingArtworkAlt = roomItem?.title?.trim() || 'Room media artwork';
  const activeMembers = useMemo(
    () => {
      const fallbackMembers = Array.isArray(room?.members)
        ? room.members.map((member) => ({
          user_id: member.user_id,
          username: member.username,
          role: member.role,
          connected: member.status === 'joined',
        }))
        : [];
      const realtimeMembers =
        realtime.roomState?.members ??
        realtime.audioState?.members ??
        realtime.webState?.members ??
        realtime.screenState?.members ??
        realtime.youtubeState?.members ??
        realtime.createState?.members ??
        realtime.playState?.members;
      const membersSource = Array.isArray(realtimeMembers) ? realtimeMembers : fallbackMembers;
      return membersSource
        .map((member) =>
          normalizeRoomPresenceMember(
            member as Partial<RoomPresenceSnapshot> | null | undefined,
          ),
        )
        .slice()
        .sort((left, right) => {
          if (left.connected !== right.connected) {
            return left.connected ? -1 : 1;
          }
          return left.username.localeCompare(right.username, undefined, { sensitivity: 'base' });
        });
    },
    [
      realtime.roomState?.members,
      realtime.audioState?.members,
      realtime.webState?.members,
      realtime.screenState?.members,
      realtime.youtubeState?.members,
      realtime.createState?.members,
      realtime.playState?.members,
      room?.members,
    ],
  );
  const connectedMemberCount = activeMembers.filter((member) => member.connected).length;

  const invitableUsers = useMemo(() => {
    if (!me) return [];
    const memberIds = new Set(activeMembers.map((member) => member.user_id));
    return allUsers.filter((user) => user.id !== me.id && !memberIds.has(user.id));
  }, [allUsers, activeMembers, me]);

  const sendWs = realtime.sendWs;

  useEffect(() => () => {
    clearInfoTimeout();
  }, [clearInfoTimeout]);

  useEffect(() => {
    setPortalMounted(true);
  }, []);

  useEffect(() => {
    if (!reconfigure.reconfigureModalOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        reconfigure.setReconfigureModalOpen(false);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [reconfigure.reconfigureModalOpen, reconfigure.setReconfigureModalOpen]);

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!me) return;
    void loadRoom();
  }, [me, loadRoom]);

  useEffect(() => {
    if (!me) return;
    listWatchPartyUsers().then(setAllUsers).catch(() => {});
  }, [me]);

  useEffect(() => {
    const normalizedRoomItemId = roomItemId.trim();
    if (!normalizedRoomItemId) {
      setRoomItem(null);
      return;
    }

    let cancelled = false;
    setRoomItem(null);

    apiJson<RoomItemSummary>(`/items/${normalizedRoomItemId}`)
      .then((item) => {
        if (!cancelled) {
          setRoomItem(item);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setRoomItem(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [roomItemId]);

  useEffect(() => {
    if (!room || room.status === 'ended') return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [room]);

  useEffect(() => {
    if (!joinedRole || !isVideoRoom || joinedRole !== 'host') {
      setMediaDrawerOpen(false);
      return;
    }
    if (!roomItemId || roomItemId.trim().length === 0) {
      setMediaDrawerOpen(true);
    }
  }, [isVideoRoom, joinedRole, roomItemId]);

  useEffect(() => {
    if (!joinedRole) return;
    if (!realtime.wsConnected) {
      void refreshRoom();
    }
    const intervalMs = realtime.wsConnected ? 30000 : 5000;
    const id = window.setInterval(() => {
      if (!realtime.wsConnected || document.visibilityState === 'visible') {
        void refreshRoom();
      }
    }, intervalMs);
    return () => window.clearInterval(id);
  }, [joinedRole, realtime.wsConnected, refreshRoom]);

  function handleSwitchCreateTool(target: 'text' | 'canvas') {
    if (!joinedRole || target === activeCreateTool) {
      return;
    }
    setError('');
    realtime.setCreateState((prev) =>
      prev
        ? {
            ...prev,
            active_tool: target,
          }
        : prev,
    );
    sendWs({
      type: 'create_set_tool',
      tool: target,
    });
  }

  const handleReconfigureRoom = reconfigure.handleReconfigureRoom;
  const handleSwitchWatchSource = reconfigure.handleSwitchWatchSource;
  const handleApplyLocalMedia = reconfigure.handleApplyLocalMedia;
  const handleConfigureAudioLibrary = reconfigure.handleConfigureAudioLibrary;
  const hostCanChooseLocalMedia =
    joinedRole === 'host' && isVideoRoom && reconfigure.reconfigureVideoLibraries.length > 0;

  const handleDownloadCurrentVideo = useCallback(async () => {
    if (!playback.descriptor?.file_id || !roomItemId) return;
    setDownloadingVideo(true);
    setError('');
    try {
      const search = new URLSearchParams();
      if (playback.hlsTargetHeight && playback.hlsTargetHeight > 0) {
        search.set('target_height', String(playback.hlsTargetHeight));
      }
      const suffix = search.toString();
      const path = `/playback/download/${playback.descriptor.file_id}${suffix ? `?${suffix}` : ''}`;
      const res = await apiFetch(path, { method: 'GET' });
      if (!res.ok) {
        throw new Error(clientErrorMessage(await res.text(), `Download failed: ${res.status}`));
      }
      const blob = await res.blob();
      const downloadUrl = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = downloadUrl;
      anchor.download = extractDownloadFilename(
        res.headers.get('content-disposition'),
        fallbackVideoDownloadName(roomItemId, playback.hlsTargetHeight),
      );
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(downloadUrl);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to download room media.'));
    } finally {
      setDownloadingVideo(false);
    }
  }, [playback.descriptor?.file_id, playback.hlsTargetHeight, roomItemId]);

  async function handleJoin() {
    setJoining(true);
    setError('');
    setInfo('');
    appendDebug('room join requested');
    try {
      const result = await joinWatchPartyRoom(roomId, joinPassword || undefined);
      setJoinedRole(result.role);
      appendDebug(`room join succeeded role=${result.role}`);
      await loadRoom();
    } catch (err: unknown) {
      const message = clientErrorMessage(err, 'Failed to join room');
      setError(message);
      appendDebug(`room join failed error=${message}`);
    } finally {
      setJoining(false);
    }
  }

  async function handleLeave() {
    setLeaving(true);
    setError('');
    try {
      await leaveWatchPartyRoom(roomId);
      router.push('/rooms');
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to leave room'));
    } finally {
      setLeaving(false);
    }
  }

  async function handleEndRoom() {
    setEnding(true);
    setError('');
    try {
      await endWatchPartyRoom(roomId);
      router.push('/rooms');
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to end room'));
      setEnding(false);
    }
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setInfo('Room link copied to clipboard.');
    } catch {
      setError('Failed to copy room link');
    }
  }

  async function handleSendInvites() {
    const payload = Object.entries(inviteSelections).map(([user_id, role]) => ({ user_id, role }));
    if (payload.length === 0) return;
    setSendingInvites(true);
    setError('');
    try {
      const response = await inviteToRoom(roomId, payload);
      setInviteSelections({});
      await refreshRoom();
      if (response.cooldown_blocked_users.length > 0) {
        const blocked = response.cooldown_blocked_users.join(', ');
        setInfo(
          `Invited ${response.invited} user${response.invited === 1 ? '' : 's'}. Cooldown active for: ${blocked}.`,
        );
      } else {
        setInfo(`Invited ${response.invited} user${response.invited === 1 ? '' : 's'}.`);
      }
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to send invites'));
    } finally {
      setSendingInvites(false);
    }
  }

  if (authLoading || loadingRoom) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading room...</p>
      </div>
    );
  }

  if (!me) {
    return null;
  }

  if (!room) {
    const normalized = (typeof error === 'string' ? error : '').toLowerCase();
    let hint = 'This room could not be opened for this account.';
    if (normalized.includes('invite-only')) {
      hint =
        'This room is invite-only for this account. Ask the host to send an invite. A password alone does not bypass invite-only access.';
    } else if (normalized.includes('library access denied')) {
      hint =
        'This account does not have access to the library containing this media. Ask an admin to grant library access.';
    } else if (normalized.includes('not found')) {
      hint = 'This room link is invalid or the room has already ended.';
    }

    return (
      <div className="animate-rise rf-flat-page">
        <section className="rf-flat-section space-y-3 border-t border-[var(--border-subtle)] pt-6">
          <span className="chip chip-accent">Watch Party Room</span>
          <h1 className="text-2xl font-semibold sm:text-3xl">Unable to open room</h1>
          <p className="text-sm muted">{error || 'Failed to load watch party room.'}</p>
          <p className="text-sm muted">{hint}</p>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              className="btn-secondary px-4 py-2 text-sm"
              onClick={() => void loadRoom()}
            >
              Retry
            </button>
            <button
              type="button"
              className="btn-primary px-4 py-2 text-sm"
              onClick={() => router.push('/rooms')}
            >
              Back to Watch Party
            </button>
          </div>
        </section>
      </div>
    );
  }

  return (
    <div className="space-y-6 animate-rise">
      <div className="rf-inline-meta justify-end">
        <span>Duration: <strong>{formatElapsedSeconds(roomDurationSeconds)}</strong></span>
        <button
          type="button"
          className="rf-text-action text-sm"
          onClick={() => reconfigure.setReconfigureModalOpen(true)}
          disabled={!joinedRole}
          title={!joinedRole ? 'Join the room to reconfigure' : undefined}
        >
          Reconfigure room
        </button>
        <button
          type="button"
          className="rf-text-action text-sm"
          onClick={() => void copyLink()}
        >
          Copy room link
        </button>
        <button
          type="button"
          className="rf-text-action text-sm"
          onClick={() => void handleLeave()}
          disabled={leaving}
        >
          {leaving ? 'Leaving…' : 'Leave room'}
        </button>
        {joinedRole === 'host' && (
          <button
            type="button"
            className="rf-text-action text-sm"
            onClick={() => setPendingEndRoomConfirm(true)}
            disabled={ending}
          >
            {ending ? 'Ending…' : 'End room'}
          </button>
        )}
      </div>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {info && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{info}</div>}

      {!joinedRole && (
        <section className="rf-flat-section space-y-4 border-t border-[var(--border-subtle)] pt-5">
          <h2 className="text-xl font-semibold">Join Room</h2>
          <p className="text-sm muted">
            You must join this room before {isAudioRoom ? 'listening together' : isYoutubeRoom ? 'watching YouTube together' : isWebRoom ? 'browsing together' : isScreenRoom ? 'sharing a screen together' : isCreateRoom ? 'creating together' : isPlayRoom ? 'playing together' : 'opening synchronized playback'}.
          </p>

          {room.password_required && (
            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Password</span>
              <input
                type="password"
                value={joinPassword}
                onChange={(e) => setJoinPassword(e.target.value)}
                className="input px-3 py-2"
                placeholder="Enter room password"
              />
            </label>
          )}

          <button
            type="button"
            className="btn-primary px-5 py-2.5 text-sm"
            onClick={handleJoin}
            disabled={joining}
          >
            {joining ? 'Joining…' : 'Join room'}
          </button>
        </section>
      )}

      {joinedRole && isAudioRoom && realtime.audioState && (
        <AudioPlayer
          audioState={realtime.audioState}
          onlineStatusEvents={realtime.onlineAudioStatusEvents}
          canControl={canPlayPause}
          canSeek={canSeek}
          roomId={roomId}
          sendWs={sendWs}
          musicLibraries={reconfigure.reconfigureMusicLibraries.map((library) => ({
            id: library.id,
            name: library.name,
          }))}
          currentAudioLibraryId={reconfigure.reconfigureAudioLibraryId}
          canConfigureLocalLibrary={joinedRole === 'host'}
          configuringLocalLibrary={reconfigure.reconfiguring}
          onConfigureLocalLibrary={(libraryId) => {
            void handleConfigureAudioLibrary(libraryId);
          }}
        />
      )}

      {joinedRole && isAudioRoom && !realtime.audioState && (
        <section className="rf-flat-section border-t border-[var(--border-subtle)] pt-5">
          <p className="text-sm muted">
            Connecting to music party…
          </p>
        </section>
      )}

      {joinedRole && isYoutubeRoom && (
        <section className={`rf-flat-section relative ${watchWindowShiftClass}`}>
          {isWatchRoom && (
            <WatchSourceTabsBar
              className={`absolute left-4 right-4 z-10 -translate-y-[62%] sm:left-6 sm:right-6 ${watchTabsCounterShiftClass}`}
              activeSource={activeWatchSource}
              onSwitchSource={handleSwitchWatchSource}
              switchingDisabled={reconfigure.reconfiguring || joinedRole !== 'host'}
              badges={[
                `Role: ${joinedRoleDisplay}`,
                `Controls: ${canPlayPause ? 'allowed' : 'host-only'}`,
              ]}
            />
          )}
          <YouTubePlayer
            roomId={roomId}
            ytState={realtime.youtubeState}
            canControl={canPlayPause}
            canQueue={!!joinedRole}
            wsConnected={realtime.wsConnected}
            sendWs={sendWs}
          />
        </section>
      )}

      {joinedRole && isWebRoom && (
        <section className={`rf-flat-section relative ${watchWindowShiftClass}`}>
          {isWatchRoom && (
            <WatchSourceTabsBar
              className={`absolute left-4 right-4 z-10 -translate-y-[62%] sm:left-6 sm:right-6 ${watchTabsCounterShiftClass}`}
              activeSource={activeWatchSource}
              onSwitchSource={handleSwitchWatchSource}
              switchingDisabled={reconfigure.reconfiguring || joinedRole !== 'host'}
              badges={[
                `Role: ${joinedRoleDisplay}`,
                `Navigation: ${canPlayPause ? 'allowed' : 'admin-only'}`,
              ]}
            />
          )}
          <WebPlayer
            roomId={roomId}
            webState={realtime.webState}
            canControl={canPlayPause}
            wsConnected={realtime.wsConnected}
            sendWs={sendWs}
          />
        </section>
      )}

      {joinedRole && isScreenRoom && (
        <section className={`rf-flat-section relative ${watchWindowShiftClass}`}>
          {isWatchRoom && (
            <WatchSourceTabsBar
              className={`absolute left-4 right-4 z-10 -translate-y-[62%] sm:left-6 sm:right-6 ${watchTabsCounterShiftClass}`}
              activeSource={activeWatchSource}
              onSwitchSource={handleSwitchWatchSource}
              switchingDisabled={reconfigure.reconfiguring || joinedRole !== 'host'}
              badges={[
                `Role: ${joinedRoleDisplay}`,
                `Share: ${joinedRole === 'host' || joinedRole === 'controller' ? 'allowed' : 'host-only'}`,
              ]}
            />
          )}
          <ScreenPlayer
            roomId={roomId}
            currentUserId={me.id}
            joinedRole={joinedRole}
            wsConnected={realtime.wsConnected}
            screenState={realtime.screenState}
            screenSignalEvent={realtime.screenSignalEvent}
            sendWs={sendWs}
            setError={setError}
          />
        </section>
      )}

      {joinedRole && isCreateRoom && (
        <section className={`rf-flat-section relative pt-[60px] sm:pt-[64px] ${createWindowShiftClass}`}>
          <CreateToolTabsBar
            className={`absolute left-4 right-4 z-10 -translate-y-[62%] sm:left-6 sm:right-6 ${createTabsCounterShiftClass}`}
            activeTool={activeCreateTool}
            onSwitchTool={handleSwitchCreateTool}
            switchingDisabled={false}
            badges={[
              `Role: ${joinedRoleDisplay}`,
              `Edit Access: ${canPlayPause ? 'allowed' : 'admin-only'}`,
            ]}
          />
          <CreateTogetherEditor
            createState={realtime.createState}
            canEdit={canPlayPause}
            sendWs={sendWs}
            activeToolOverride={activeCreateTool}
          />
        </section>
      )}

      {joinedRole && isPlayRoom && (
        <PlayTogetherChess
          playState={realtime.playState}
          members={activeMembers}
          currentUserId={me.id}
          canControl={canPlayPause}
          sendWs={sendWs}
        />
      )}

      {reconfigure.reconfigureModalOpen &&
        portalMounted &&
        createPortal(
          <div
            className="fixed inset-0 z-40 flex items-start justify-center bg-black/45 p-4 pt-[30vh] backdrop-blur-[2px]"
            onClick={() => reconfigure.setReconfigureModalOpen(false)}
          >
            <div
              role="dialog"
              aria-modal="true"
              aria-label="Reconfigure room"
              className="w-full max-w-5xl max-h-[68vh] space-y-4 overflow-y-auto rounded-2xl border border-[var(--border)] bg-[var(--surface)]/95 p-5 sm:p-6"
              onClick={(event) => event.stopPropagation()}
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="space-y-1">
                  <h2 className="text-xl font-semibold">Reconfigure Room</h2>
                  <p className="text-sm muted">
                    Pick a room type and use its tools with everyone in this room.
                  </p>
                </div>
                <button
                  type="button"
                  className="btn-secondary px-3 py-1.5 text-xs"
                  onClick={() => reconfigure.setReconfigureModalOpen(false)}
                >
                  Close
                </button>
              </div>

              {joinedRole === 'host' ? (
                <div className="space-y-4">
                  <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[var(--border-subtle)] p-3">
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        className={`px-4 py-2 text-sm rounded-lg ${
                          currentReconfigureCategory === 'watch'
                            ? 'cursor-not-allowed border border-white/20 bg-white/10 text-white/60'
                            : reconfigure.isWatchReconfigureMode
                              ? 'btn-primary'
                              : 'btn-secondary'
                        }`}
                        onClick={() => reconfigure.selectReconfigureMode('video')}
                        disabled={currentReconfigureCategory === 'watch'}
                      >
                        Watch Together
                      </button>
                      <button
                        type="button"
                        className={`px-4 py-2 text-sm rounded-lg ${
                          currentReconfigureCategory === 'audio'
                            ? 'cursor-not-allowed border border-white/20 bg-white/10 text-white/60'
                            : reconfigure.reconfigureMode === 'audio'
                              ? 'btn-primary'
                              : 'btn-secondary'
                        }`}
                        onClick={() => reconfigure.selectReconfigureMode('audio')}
                        disabled={currentReconfigureCategory === 'audio'}
                      >
                        Listen Together
                      </button>
                      <button
                        type="button"
                        className={`px-4 py-2 text-sm rounded-lg ${
                          currentReconfigureCategory === 'play'
                            ? 'cursor-not-allowed border border-white/20 bg-white/10 text-white/60'
                            : reconfigure.reconfigureMode === 'play'
                              ? 'btn-primary'
                              : 'btn-secondary'
                        }`}
                        onClick={() => reconfigure.selectReconfigureMode('play')}
                        disabled={currentReconfigureCategory === 'play'}
                      >
                        Play Together
                      </button>
                      <button
                        type="button"
                        className={`px-4 py-2 text-sm rounded-lg ${
                          currentReconfigureCategory === 'create'
                            ? 'cursor-not-allowed border border-white/20 bg-white/10 text-white/60'
                            : reconfigure.reconfigureMode === 'create'
                              ? 'btn-primary'
                              : 'btn-secondary'
                        }`}
                        onClick={() => reconfigure.selectReconfigureMode('create')}
                        disabled={currentReconfigureCategory === 'create'}
                      >
                        Create Together
                      </button>
                    </div>
                    <div className="flex w-full justify-end sm:w-auto">
                      <button
                        type="button"
                        className="btn-primary px-5 py-2.5 text-sm disabled:opacity-50"
                        onClick={handleReconfigureRoom}
                        disabled={
                          reconfigure.reconfiguring ||
                          currentReconfigureCategory === selectedReconfigureCategory
                        }
                      >
                        {reconfigure.reconfiguring ? 'Reconfiguring…' : 'Apply Room Mode'}
                      </button>
                    </div>
                  </div>

                  {reconfigure.reconfigureMode === 'audio' ? (
                    <div className="space-y-3">
                      <p className="text-xs uppercase tracking-wide muted">Listen Together</p>
                      <div className="rf-flat-empty text-sm muted">
                        Listen together with one shared queue. Search online tracks, browse local
                        library tracks, and control playback together.
                      </div>
                    </div>
                  ) : reconfigure.reconfigureMode === 'create' ? (
                    <div className="space-y-3">
                      <p className="text-xs uppercase tracking-wide muted">Create Together</p>
                      <div className="rf-flat-empty text-sm muted">
                        Collaborate in shared documents and a shared canvas in real time. Edit, draw,
                        and export your work directly from the room.
                      </div>
                    </div>
                  ) : reconfigure.reconfigureMode === 'play' ? (
                    <div className="space-y-3">
                      <p className="text-xs uppercase tracking-wide muted">Play Together</p>
                      <div className="rf-flat-empty text-sm muted">
                        Play shared games in real time with room members. Start with Chess, assign players,
                        and take turns on the same board.
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <p className="text-xs uppercase tracking-wide muted">Watch Together</p>
                      <div className="rf-flat-empty text-sm muted">
                        Watch together using Local Media, YouTube, Web, or Screen sources. Use shared
                        controls for playback, navigation, or live screen presentation depending on the source.
                      </div>
                    </div>
                  )}
                </div>
              ) : (
                <div className="rf-flat-empty text-sm muted">
                  Only room admins can reconfigure the room.
                </div>
              )}
            </div>
          </div>,
          document.body,
        )}

      {pendingEndRoomConfirm &&
        portalMounted &&
        createPortal(
          <ConfirmModal
            open
            title="End Room"
            description="End this room for everyone? All participants will be disconnected from this room."
            confirmLabel={ending ? 'Ending…' : 'End room'}
            confirmDisabled={ending}
            cancelDisabled={ending}
            destructive
            onCancel={() => setPendingEndRoomConfirm(false)}
            onConfirm={() => {
              setPendingEndRoomConfirm(false);
              void handleEndRoom();
            }}
          />,
          document.body,
        )}

      {joinedRole && isVideoRoom && (
        <>
          <section
            className={`rf-flat-section relative overflow-hidden ${watchWindowShiftClass}`}
          >
            {isWatchRoom && (
              <WatchSourceTabsBar
                className={`absolute left-4 right-4 z-10 -translate-y-[62%] sm:left-6 sm:right-6 ${watchTabsCounterShiftClass}`}
                activeSource={activeWatchSource}
                onSwitchSource={handleSwitchWatchSource}
                switchingDisabled={reconfigure.reconfiguring || joinedRole !== 'host'}
                badges={[
                  `Role: ${joinedRoleDisplay}`,
                  `Play/Pause: ${canPlayPause ? 'allowed' : 'host-only'}`,
                  `Seek: ${canSeek ? 'allowed' : 'host-only'}`,
                ]}
              />
            )}
            <div className="relative min-h-[min(78vh,48rem)]">
              {hostCanChooseLocalMedia && (
                <button
                  type="button"
                  className="btn-secondary absolute right-4 top-4 z-20 px-3 py-1.5 text-xs sm:right-6"
                  onClick={() => setMediaDrawerOpen((open) => !open)}
                >
                  {mediaDrawerOpen ? 'Close Media' : 'Choose Media'}
                </button>
              )}

              {!roomItemId || roomItemId.trim().length === 0 ? (
                <div className="flex min-h-[min(78vh,48rem)] items-center justify-center rounded-[2.25rem] bg-white/[0.02] px-6 py-8 text-center">
                  <div className="mx-auto max-w-xl space-y-4">
                    <h2 className="text-2xl font-semibold sm:text-3xl">Local Media</h2>
                    <p className="text-sm muted sm:text-base">
                      {joinedRole === 'host'
                        ? hostCanChooseLocalMedia
                          ? 'Open the media drawer to choose a library and load a movie or episode into this room.'
                          : 'No shared local video libraries are available for current room participants.'
                        : 'Waiting for a room admin to load local media.'}
                    </p>
                    {hostCanChooseLocalMedia && (
                      <button
                        type="button"
                        className="btn-primary px-5 py-2.5 text-sm"
                        onClick={() => setMediaDrawerOpen(true)}
                      >
                        Open Media Browser
                      </button>
                    )}
                  </div>
                </div>
              ) : (
                <div className="min-h-[min(78vh,48rem)] flex-1">
                  <VideoPlayerSurface
                    shellRef={roomVideoShellRef}
                    videoRef={playback.videoRef}
                    playbackKey={roomItemId}
                    artworkUrl={roomLoadingArtworkUrl}
                    artworkAlt={roomLoadingArtworkAlt}
                    surfaceStyle="immersive"
                    canStartPlayback={Boolean(playback.descriptor)}
                    knownDurationSecs={playback.knownDurationMs > 0 ? playback.knownDurationMs / 1000 : 0}
                    bufferedWindowEndSecs={
                      playback.hlsSessionStartOffsetSecs + playback.hlsAvailableWindowDurationSecs
                    }
                    sessionStartOffsetSecs={playback.hlsSessionStartOffsetSecs}
                    qualityValue={playback.hlsTargetHeight ?? 'auto'}
                    qualityOptions={qualityOptions}
                    qualityDisabled={playback.startingHls}
                    onQualityChange={(value) => {
                      const previousTargetHeight = playback.hlsTargetHeight;
                      const nextTargetHeight = value === 'auto' ? null : value;
                      const video = playback.videoRef.current;
                      const currentAbsoluteSeconds =
                        video && Number.isFinite(video.currentTime) && video.currentTime >= 0
                          ? playback.hlsSessionStartOffsetSecs + video.currentTime
                          : undefined;
                      playback.setHlsTargetHeight(nextTargetHeight);
                      void (async () => {
                        const started = await playback.startHls({
                          silent: false,
                          targetHeightOverride: nextTargetHeight,
                          seekTimeOverrideSecs:
                            currentAbsoluteSeconds !== undefined && currentAbsoluteSeconds > 0.25
                              ? currentAbsoluteSeconds
                              : undefined,
                          syncRoomStateOnReady: false,
                        });
                        if (!started) {
                          playback.setHlsTargetHeight(previousTargetHeight ?? null);
                        }
                      })();
                    }}
                    onSeekRequest={async (targetSeconds) => {
                      if (!canSeek) return;
                      playback.notePendingSeek(targetSeconds);

                      if (playback.applyingRemoteRef.current) return;
                      sendWs({
                        type: 'seek',
                        position_ms: Math.floor(targetSeconds * 1000),
                      });

                      await playback.handleSeek(targetSeconds);
                    }}
                    onDownload={handleDownloadCurrentVideo}
                    downloading={downloadingVideo}
                    downloadDisabled={!playback.descriptor || playback.startingHls}
                    playbackEnabled={canPlayPause}
                    seekEnabled={canSeek}
                    playbackDisabledReason={
                      !controlsEnabled ? 'Playback controls are host-only in this room.' : null
                    }
                    statusText={!controlsEnabled ? 'Playback controls are host-only in this room.' : null}
                    maxViewportHeightClassName="h-full max-h-full"
                    videoElementProps={{
                      preload: 'auto',
                      onPlay: (event) => {
                        if (playback.applyingRemoteRef.current || !canPlayPause) return;
                        sendWs({
                          type: 'play',
                          position_ms: Math.floor(
                            (playback.hlsSessionStartOffsetSecs + event.currentTarget.currentTime) * 1000,
                          ),
                        });
                      },
                      onPause: (event) => {
                        if (playback.applyingRemoteRef.current || !canPlayPause) return;
                        sendWs({
                          type: 'pause',
                          position_ms: Math.floor(
                            (playback.hlsSessionStartOffsetSecs + event.currentTarget.currentTime) * 1000,
                          ),
                        });
                      },
                      onError: () => {
                        setError('HLS playback failed. Refresh the room and retry.');
                      },
                    }}
                  />
                </div>
              )}

              {hostCanChooseLocalMedia && (
                <>
                  <div
                    className={`absolute inset-0 z-20 bg-black/30 transition-opacity duration-200 ${
                      mediaDrawerOpen ? 'pointer-events-auto opacity-100' : 'pointer-events-none opacity-0'
                    }`}
                    onClick={() => setMediaDrawerOpen(false)}
                  />
                  <aside
                    className={`absolute inset-y-0 right-0 z-30 w-full max-w-[25rem] border-l border-[var(--border-subtle)] bg-[var(--surface)]/96 px-4 py-5 backdrop-blur-md transition-transform duration-200 ease-out sm:px-5 ${
                      mediaDrawerOpen ? 'translate-x-0' : 'translate-x-full'
                    }`}
                  >
                    <div className="flex h-full min-h-0 flex-col gap-4">
                      <div className="flex items-start justify-between gap-3">
                        <div className="space-y-1">
                          <h3 className="text-lg font-semibold">Choose Local Media</h3>
                          <p className="text-sm muted">
                            Select a library, then load a movie or episode into this room.
                          </p>
                        </div>
                        <button
                          type="button"
                          className="rf-text-action text-sm"
                          onClick={() => setMediaDrawerOpen(false)}
                        >
                          Close
                        </button>
                      </div>

                      <div className="min-h-0 flex-1 overflow-y-auto pr-1">
                        <MediaPicker
                          libraries={reconfigure.allLibraries}
                          eligibleLibraryIds={reconfigure.eligibleLibraryIds}
                          selectedLibraryId={reconfigure.reconfigureVideoLibraryId}
                          selectedItem={reconfigure.reconfigureVideoItem}
                          layout="stacked"
                          surfaceClassName="space-y-4"
                          noShadow
                          showHeading={false}
                          librarySelectorMode="tabs"
                          applyActionLabel="Load Into Room"
                          applyActionPendingLabel="Loading…"
                          applyActionDisabled={!reconfigure.reconfigureVideoItem}
                          applyActionLoading={reconfigure.reconfiguring}
                          onApplyAction={() => {
                            void handleApplyLocalMedia();
                            setMediaDrawerOpen(false);
                          }}
                          onLibraryChange={reconfigure.setReconfigureVideoLibraryId}
                          onSelectItem={reconfigure.setReconfigureVideoItem}
                        />
                      </div>
                    </div>
                  </aside>
                </>
              )}
            </div>
          </section>
        </>
      )}

      {joinedRole && (
        <div className={`grid gap-5 md:grid-cols-2 ${createLowerPanelsShiftClass}`}>
          <section className="rf-flat-section flex h-[22rem] min-h-0 flex-col gap-3 border-t border-[var(--border-subtle)] pt-5">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-xl font-semibold">Who&apos;s in the room</h2>
              <span className="text-xs text-emerald-200">{connectedMemberCount} live</span>
            </div>
            <ul className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
              {activeMembers.map((member) => (
                <li
                  key={member.user_id}
                  className={`rf-flat-row room-panel-row transition ${
                    member.connected ? 'room-member-online' : 'room-member-offline'
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div>
                      <p className="text-sm font-medium">{member.username}</p>
                      <p className="text-xs muted">{roleLabel(member.role, effectiveRoomMode)}</p>
                    </div>
                    <span
                      className={
                        member.connected ? 'text-xs text-emerald-200' : 'text-xs text-white/60'
                      }
                    >
                      {member.connected ? 'Online' : 'Offline'}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className="rf-flat-section flex h-[22rem] min-h-0 flex-col gap-4 border-t border-[var(--border-subtle)] pt-5">
            <h2 className="text-xl font-semibold">Invite to Room</h2>
            {invitableUsers.length === 0 ? (
              <div className="text-sm muted">
                All eligible users are already in this room.
              </div>
            ) : (
              <>
                <div className="min-h-0 flex-1 overflow-y-auto pr-1">
                  <ul className="rf-flat-list">
                    {invitableUsers.map((user) => {
                      const checked = user.id in inviteSelections;
                      const role = inviteSelections[user.id] ?? 'viewer';
                      return (
                        <li
                          key={user.id}
                          className={`rf-flat-row room-panel-row room-invite-row ${
                            checked ? 'room-invite-row-active' : ''
                          }`}
                        >
                          <div className="flex items-center justify-between gap-3">
                            <div className="flex min-w-0 items-center gap-3">
                              <input
                                type="checkbox"
                                checked={checked}
                                onChange={() => {
                                  setInviteSelections((prev) => {
                                    const next = { ...prev };
                                    if (next[user.id] !== undefined) {
                                      delete next[user.id];
                                    } else {
                                      next[user.id] = 'viewer';
                                    }
                                    return next;
                                  });
                                }}
                                className="h-4 w-4 shrink-0"
                              />
                              <span className="w-[14ch] truncate text-sm font-medium" title={user.username}>
                                {truncateInviteName(user.username)}
                              </span>
                            </div>
                            <div className="w-[7.75rem] shrink-0">
                              <select
                                className="select w-full px-2 py-1.5 text-sm"
                                value={role}
                                onChange={(e) =>
                                  setInviteSelections((prev) => ({
                                    ...prev,
                                    [user.id]: e.target.value as 'viewer' | 'controller',
                                  }))
                                }
                              >
                                <option value="viewer">{memberRoleDisplay}</option>
                                <option value="controller">Admin</option>
                              </select>
                            </div>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                </div>
                <button
                  type="button"
                  className="btn-primary px-5 py-2.5 text-sm disabled:opacity-50"
                  onClick={handleSendInvites}
                  disabled={sendingInvites || Object.keys(inviteSelections).length === 0}
                >
                  {sendingInvites ? 'Sending…' : 'Send Invites'}
                </button>
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
