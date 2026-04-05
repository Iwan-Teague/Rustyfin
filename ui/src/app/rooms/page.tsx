'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { useAuth } from '@/lib/auth';
import {
  PublicRoom,
  WatchPartyInvite,
  WatchPartyPolicy,
  WatchPartyUser,
  createWatchPartyRoom,
  declineWatchPartyInvite,
  listPublicRooms,
  listWatchPartyInvites,
  listWatchPartyUsers,
} from '@/lib/watchPartyApi';
import UserInvitePicker, { SelectedInvite } from './components/UserInvitePicker';
import RoomOptions from './components/RoomOptions';
import { elapsedSinceSeconds, formatElapsedSeconds } from '@/lib/time';
import { clientErrorMessage } from '@/lib/errors';
import { roleLabel } from '@/lib/watchPartyRoles';

type RoomMode = 'watch' | 'audio' | 'play' | 'create';
type RoomsSidebarTab = 'open_rooms' | 'inbox';
type CreateRoomPanelTab = 'options' | 'invites';

const DEFAULT_POLICY: WatchPartyPolicy = {
  allow_non_host_play_pause: true,
  allow_non_host_seek: false,
  default_join_role: 'viewer',
  invite_only: false,
};

const ROOM_MODE_OPTIONS: Array<{ value: RoomMode; label: string; description: string }> = [
  {
    value: 'watch',
    label: 'Watch',
    description: 'Synchronized movie and episode playback with shared controls.',
  },
  {
    value: 'audio',
    label: 'Listen',
    description: 'Low-latency shared listening sessions for music and audio.',
  },
  {
    value: 'play',
    label: 'Challenge',
    description: 'Party-style interactive rooms for gameplay sessions.',
  },
  {
    value: 'create',
    label: 'Create',
    description: 'Collaborative creation rooms for writing and planning.',
  },
];

export default function WatchPartyPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [users, setUsers] = useState<WatchPartyUser[]>([]);
  const [roomMode, setRoomMode] = useState<RoomMode>('watch');
  const [selectedInvites, setSelectedInvites] = useState<Record<string, SelectedInvite>>({});
  const [roomName, setRoomName] = useState('');
  const [policy, setPolicy] = useState<WatchPartyPolicy>(DEFAULT_POLICY);
  const [password, setPassword] = useState('');
  const [sidebarTab, setSidebarTab] = useState<RoomsSidebarTab>('open_rooms');
  const [createPanelTab, setCreatePanelTab] = useState<CreateRoomPanelTab>('options');

  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [createError, setCreateError] = useState('');
  const [publicRooms, setPublicRooms] = useState<PublicRoom[]>([]);
  const [invites, setInvites] = useState<WatchPartyInvite[]>([]);
  const [decliningInviteRoomId, setDecliningInviteRoomId] = useState<string | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [modeMotionCycle, setModeMotionCycle] = useState(0);

  const effectivePolicyRoomMode = roomMode === 'audio'
    ? 'audio'
    : roomMode === 'create'
      ? 'create'
      : 'video';

  useEffect(() => {
    if (!authLoading && !me) {
      router.replace('/login');
    }
  }, [authLoading, me, router]);

  useEffect(() => {
    if (!me) return;

    let cancelled = false;
    setLoading(true);
    setError('');

    (async () => {
      try {
        const [userList, publicRoomList, inviteList] = await Promise.all([
          listWatchPartyUsers(),
          listPublicRooms(),
          listWatchPartyInvites(),
        ]);

        if (cancelled) return;

        setUsers(userList);
        setPublicRooms(publicRoomList);
        setInvites(inviteList);

      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load watch-party data'));
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [me]);

  useEffect(() => {
    if (publicRooms.length === 0) return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [publicRooms.length]);

  useEffect(() => {
    if (!me) return;

    let cancelled = false;

    const refreshLivePanels = () => {
      if (document.visibilityState !== 'visible') return;

      void Promise.allSettled([listPublicRooms(), listWatchPartyInvites()]).then((results) => {
        if (cancelled) return;

        const [roomsResult, invitesResult] = results;
        if (roomsResult.status === 'fulfilled') {
          setPublicRooms(roomsResult.value);
        }
        if (invitesResult.status === 'fulfilled') {
          setInvites(invitesResult.value);
        }
      });
    };

    const handleVisibilityOrFocus = () => {
      refreshLivePanels();
    };

    const id = window.setInterval(refreshLivePanels, 3000);
    window.addEventListener('focus', handleVisibilityOrFocus);
    document.addEventListener('visibilitychange', handleVisibilityOrFocus);

    return () => {
      cancelled = true;
      window.clearInterval(id);
      window.removeEventListener('focus', handleVisibilityOrFocus);
      document.removeEventListener('visibilitychange', handleVisibilityOrFocus);
    };
  }, [me]);

  function setPolicyField<K extends keyof WatchPartyPolicy>(key: K, value: WatchPartyPolicy[K]) {
    setPolicy((prev) => ({ ...prev, [key]: value }));
  }

  function toggleInvite(userId: string, initialRole?: 'viewer' | 'controller') {
    setSelectedInvites((prev) => {
      const next = { ...prev };
      if (next[userId]) {
        delete next[userId];
      } else {
        next[userId] = { role: initialRole ?? 'viewer' };
      }
      return next;
    });
  }

  function setInviteRole(userId: string, role: 'viewer' | 'controller') {
    setSelectedInvites((prev) => ({
      ...prev,
      [userId]: {
        role,
      },
    }));
  }

  function handleRoomModeSelect(nextMode: RoomMode) {
    if (nextMode === roomMode) return;
    setRoomMode(nextMode);
    setModeMotionCycle((prev) => prev + 1);
  }

  function handleJoinInvite(roomId: string) {
    router.push(`/rooms/${roomId}`);
  }

  async function handleDeclineInvite(roomId: string) {
    setDecliningInviteRoomId(roomId);
    setError('');
    try {
      await declineWatchPartyInvite(roomId);
      setInvites((prev) => prev.filter((invite) => invite.room_id !== roomId));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to decline invite'));
    } finally {
      setDecliningInviteRoomId(null);
    }
  }

  async function handleCreateRoom() {
    setCreating(true);
    setCreateError('');

    try {
      const invitesPayload = Object.entries(selectedInvites).map(([user_id, config]) => ({
        user_id,
        role: config.role,
      }));
      const normalizedRoomName = roomName.trim();

      if (roomMode === 'create') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'create' as const,
          create_tool: 'text' as const,
          create_document_name: normalizedRoomName || undefined,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };
        const created = await createWatchPartyRoom(payload);
        router.push(created.join_path);
      } else if (roomMode === 'audio') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'audio' as const,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        router.push(created.join_path);
      } else if (roomMode === 'play') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'play' as const,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };
        const created = await createWatchPartyRoom(payload);
        router.push(created.join_path);
      } else if (roomMode === 'watch') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'video' as const,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        router.push(created.join_path);
      } else {
        setCreateError('Unsupported room mode.');
        return;
      }
    } catch (err: unknown) {
      setCreateError(clientErrorMessage(err, 'Failed to create watch party room'));
    } finally {
      setCreating(false);
    }
  }

  if (authLoading || loading) {
    return (
      <div className="rf-flat-empty animate-rise">
        <p className="text-sm muted">Loading watch-party workspace...</p>
      </div>
    );
  }

  if (!me) {
    return null;
  }

  const canCreate = true;
  const activeModeOption = ROOM_MODE_OPTIONS.find((option) => option.value === roomMode) ?? ROOM_MODE_OPTIONS[0];
  const modeMotionClass =
    modeMotionCycle === 0
      ? ''
      : modeMotionCycle % 2 === 0
        ? 'rf-room-mode-enter-a'
        : 'rf-room-mode-enter-b';
  const createPanelMotionClass = createPanelTab === 'options' ? 'rf-room-mode-enter-a' : 'rf-room-mode-enter-b';
  return (
    <div className="animate-rise rf-flat-page">

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <div className="grid gap-6 lg:[grid-template-columns:minmax(0,7fr)_minmax(18rem,3fr)]">
        <section className="rf-flat-section flex min-h-[44rem] flex-col gap-5">
          <div className="space-y-1">
            <h2 className="text-xl font-semibold sm:text-2xl">Create a Room</h2>
            <p className="text-sm muted">Choose the room type, configure access, invite people, and open it directly.</p>
          </div>

          {createError ? <div className="notice-error rounded-xl px-4 py-2 text-sm">{createError}</div> : null}

          <div className="space-y-3">
            <label className="block text-sm">
              <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Name</span>
              <input
                type="text"
                value={roomName}
                onChange={(e) => setRoomName(e.target.value)}
                className="rf-flat-input px-3 py-2 text-sm"
                placeholder="Optional room name"
                maxLength={120}
              />
            </label>

            <div className="flex flex-wrap gap-2">
              {ROOM_MODE_OPTIONS.map((option) => {
                const isActive = roomMode === option.value;
                return (
                  <button
                    key={option.value}
                    type="button"
                    data-active={isActive ? 'true' : 'false'}
                    className={`rf-room-mode-btn px-4 py-2 text-sm ${isActive ? 'btn-primary' : 'btn-secondary'}`}
                    onClick={() => handleRoomModeSelect(option.value)}
                    aria-pressed={isActive}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
            <p className={`rf-room-mode-feedback text-xs muted ${modeMotionClass}`}>{activeModeOption.description}</p>
          </div>

          <div className="rf-top-tabbar border-b border-[var(--border-subtle)] pb-0">
            <button
              type="button"
              className="rf-top-tab"
              data-active={createPanelTab === 'options'}
              onClick={() => setCreatePanelTab('options')}
            >
              Room Options
            </button>
            <button
              type="button"
              className="rf-top-tab"
              data-active={createPanelTab === 'invites'}
              onClick={() => setCreatePanelTab('invites')}
            >
              Invite Users
            </button>
          </div>

          <div className={`min-h-0 flex-1 ${createPanelMotionClass}`}>
            {createPanelTab === 'options' ? (
              <RoomOptions
                roomMode={effectivePolicyRoomMode}
                password={password}
                allowPlayPause={policy.allow_non_host_play_pause}
                allowSeek={policy.allow_non_host_seek}
                inviteOnly={policy.invite_only}
                defaultJoinRole={policy.default_join_role}
                embedded
                noShadow
                fillHeight
                onPasswordChange={setPassword}
                onAllowPlayPauseChange={(value) => setPolicyField('allow_non_host_play_pause', value)}
                onAllowSeekChange={(value) => setPolicyField('allow_non_host_seek', value)}
                onInviteOnlyChange={(value) => setPolicyField('invite_only', value)}
                onDefaultJoinRoleChange={(value) => setPolicyField('default_join_role', value)}
              />
            ) : (
              <UserInvitePicker
                users={users}
                currentUserId={me.id}
                roomMode={effectivePolicyRoomMode}
                selected={selectedInvites}
                embedded
                noShadow
                fillHeight
                onToggle={toggleInvite}
                onRoleChange={setInviteRole}
              />
            )}
          </div>

          <div className="border-t border-[var(--border-subtle)] pt-5">
            <button
              type="button"
              className="btn-primary w-full px-5 py-3 text-sm disabled:opacity-50"
              onClick={handleCreateRoom}
              disabled={creating || !canCreate}
            >
              {creating ? (
                <span className="rf-room-create-loading">
                  <span className="rf-room-create-spinner" aria-hidden="true" />
                  Creating room…
                </span>
              ) : (
                'Create Room'
              )}
            </button>
          </div>
        </section>

        <aside className="rf-flat-section flex min-h-[44rem] min-w-0 flex-col gap-5">
          <div className="rf-top-tabbar border-b border-[var(--border-subtle)] pb-0">
            <button
              type="button"
              className="rf-top-tab"
              data-active={sidebarTab === 'open_rooms'}
              onClick={() => setSidebarTab('open_rooms')}
            >
              Open Rooms
            </button>
            <button
              type="button"
              className="rf-top-tab"
              data-active={sidebarTab === 'inbox'}
              onClick={() => setSidebarTab('inbox')}
            >
              Inbox
            </button>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto pr-1">
            {sidebarTab === 'open_rooms' ? (
              publicRooms.length === 0 ? (
                <div className="rf-flat-empty text-sm muted">No open rooms right now.</div>
              ) : (
                <div className="rf-flat-list">
                  {publicRooms.map((room) => (
                    <Link
                      key={room.room_id}
                      href={`/rooms/${room.room_id}`}
                      className="rf-flat-row flex items-center justify-between gap-3 rounded-2xl px-3 transition hover:bg-white/[0.05]"
                    >
                      <div className="min-w-0 space-y-0.5">
                        <p className="truncate font-semibold">{room.title}</p>
                        <p className="text-xs muted">
                          Hosted by {room.host_username}
                          {` · ${room.member_count} in room now`}
                          {' · '}
                          {formatElapsedSeconds(elapsedSinceSeconds(room.created_ts, nowMs))}
                        </p>
                        {room.password_required ? (
                          <span className="chip text-xs">Password Protected</span>
                        ) : null}
                      </div>
                    </Link>
                  ))}
                </div>
              )
            ) : invites.length === 0 ? (
              <div className="rf-flat-empty text-sm muted">No pending invites.</div>
            ) : (
              <ul className="rf-flat-list">
                {invites.map((invite) => (
                  <li key={invite.room_id} className="rf-flat-row">
                    <div
                      className="flex w-full items-center justify-between gap-3 rounded-2xl px-3 py-2 text-left transition hover:bg-white/[0.05]"
                      role="button"
                      tabIndex={0}
                      onClick={() => handleJoinInvite(invite.room_id)}
                      onKeyDown={(event) => {
                        if (event.key === 'Enter' || event.key === ' ') {
                          event.preventDefault();
                          handleJoinInvite(invite.room_id);
                        }
                      }}
                    >
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium">{invite.item_title}</p>
                        <p className="text-xs muted">
                          Host: {invite.host_username} • Role: {roleLabel(invite.role, 'video')}
                          {invite.password_required ? ' • Password required' : ''}
                        </p>
                      </div>
                      <button
                        type="button"
                        className="rf-text-action text-xs"
                        onClick={(event) => {
                          event.stopPropagation();
                          handleDeclineInvite(invite.room_id);
                        }}
                        disabled={decliningInviteRoomId === invite.room_id}
                      >
                        {decliningInviteRoomId === invite.room_id ? 'Declining…' : 'Decline'}
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>
      </div>

    </div>
  );
}
