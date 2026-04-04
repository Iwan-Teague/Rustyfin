'use client';

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
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
import InvitesPanel from './components/InvitesPanel';
import UserInvitePicker, { SelectedInvite } from './components/UserInvitePicker';
import RoomOptions from './components/RoomOptions';
import { elapsedSinceSeconds, formatElapsedSeconds } from '@/lib/time';
import { clientErrorMessage } from '@/lib/errors';

type RoomMode = 'watch' | 'audio' | 'play' | 'create';

const DEFAULT_POLICY: WatchPartyPolicy = {
  allow_non_host_play_pause: true,
  allow_non_host_seek: false,
  default_join_role: 'viewer',
  invite_only: false,
};

const ROOM_MODE_OPTIONS: Array<{ value: RoomMode; label: string; description: string }> = [
  {
    value: 'watch',
    label: 'Watch Together',
    description: 'Synchronized movie and episode playback with shared controls.',
  },
  {
    value: 'audio',
    label: 'Listen Together',
    description: 'Low-latency shared listening sessions for music and audio.',
  },
  {
    value: 'play',
    label: 'Play Together',
    description: 'Party-style interactive rooms for gameplay sessions.',
  },
  {
    value: 'create',
    label: 'Create Together',
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
  const [createModalOpen, setCreateModalOpen] = useState(false);
  const [mounted, setMounted] = useState(false);

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
    setMounted(true);
    return () => setMounted(false);
  }, []);

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
        setCreateModalOpen(false);
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
        setCreateModalOpen(false);
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
        setCreateModalOpen(false);
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
        setCreateModalOpen(false);
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

  function openCreateRoomModal() {
    setCreateError('');
    setCreateModalOpen(true);
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
  const fixedTopPanelsStyle = { height: '15.5rem' };
  const fixedColumnHeightStyle = { height: '26rem' };
  return (
    <div className="animate-rise rf-flat-page">

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}

      <div className="grid gap-5 md:grid-cols-2 md:[grid-template-columns:minmax(0,1fr)_minmax(0,1fr)]">
        <section
          className="rf-flat-section flex h-full min-h-0 flex-col gap-4"
          style={fixedTopPanelsStyle}
        >
          <div className="space-y-2">
            <h2 className="text-xl font-semibold">Open Rooms</h2>
            <p className="text-sm muted">Public rooms you can join right now.</p>
          </div>

          {publicRooms.length === 0 ? (
            <div className="rf-flat-empty text-sm muted">No open rooms right now.</div>
          ) : (
            <div className="min-h-0 flex-1 overflow-y-auto pr-1">
              <div className="rf-flat-list">
                {publicRooms.map((room) => (
                  <div
                    key={room.room_id}
                    className="rf-flat-row flex items-center justify-between gap-3"
                  >
                    <div className="min-w-0 space-y-0.5">
                      <p className="font-semibold truncate">{room.title}</p>
                      <p className="text-xs muted">
                        Hosted by {room.host_username}
                        {` · ${room.member_count} in room now`}
                        {' · '}
                        {formatElapsedSeconds(elapsedSinceSeconds(room.created_ts, nowMs))}
                      </p>
                      {room.password_required && (
                        <span className="chip text-xs">Password Protected</span>
                      )}
                    </div>
                    <Link
                      href={`/rooms/${room.room_id}`}
                      className="btn-primary shrink-0 px-3 py-1.5 text-sm"
                    >
                      Join
                    </Link>
                  </div>
                ))}
              </div>
            </div>
          )}
        </section>

        <div style={fixedTopPanelsStyle}>
          <InvitesPanel
            invites={invites}
            onJoin={handleJoinInvite}
            onDecline={handleDeclineInvite}
            decliningRoomId={decliningInviteRoomId}
          />
        </div>
      </div>

      <section className="rf-flat-section">
        <div className="flex justify-end">
          <button
            type="button"
            className="btn-primary px-5 py-3 text-sm"
            onClick={openCreateRoomModal}
          >
            Create Room
          </button>
        </div>
      </section>

      {mounted && createModalOpen
        ? createPortal(
            <div
              className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-[2px]"
              role="presentation"
              onClick={() => {
                if (creating) return;
                setCreateModalOpen(false);
              }}
            >
              <div
                className="rf-preserve-surface w-full max-w-5xl space-y-5 rounded-2xl border border-[var(--border)] bg-[rgb(19,24,36)] p-6 shadow-[0_30px_60px_rgba(0,0,0,0.42)]"
                role="dialog"
                aria-modal="true"
                aria-label="Create room"
                onClick={(event) => event.stopPropagation()}
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="space-y-1">
                    <h2 className="text-xl font-semibold sm:text-2xl">Create a Room</h2>
                    <p className="text-sm muted">Choose the room type, invite people, and open it directly.</p>
                  </div>
                  <button
                    type="button"
                    className="rf-inline-icon-btn h-9 w-9 text-lg"
                    onClick={() => {
                      if (creating) return;
                      setCreateModalOpen(false);
                    }}
                    aria-label="Close create room dialog"
                  >
                    ×
                  </button>
                </div>

                {createError ? <div className="notice-error rounded-xl px-4 py-2 text-sm">{createError}</div> : null}

                <section className="space-y-3">
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
                </section>

                <div className="grid gap-5 md:grid-cols-2 md:[grid-template-columns:minmax(0,1fr)_minmax(0,1fr)]">
                  <section className={`space-y-4 ${modeMotionClass}`}>
                    <div style={fixedColumnHeightStyle}>
                      <RoomOptions
                        roomMode={effectivePolicyRoomMode}
                        password={password}
                        allowPlayPause={policy.allow_non_host_play_pause}
                        allowSeek={policy.allow_non_host_seek}
                        inviteOnly={policy.invite_only}
                        defaultJoinRole={policy.default_join_role}
                        noShadow
                        fillHeight
                        onPasswordChange={setPassword}
                        onAllowPlayPauseChange={(value) => setPolicyField('allow_non_host_play_pause', value)}
                        onAllowSeekChange={(value) => setPolicyField('allow_non_host_seek', value)}
                        onInviteOnlyChange={(value) => setPolicyField('invite_only', value)}
                        onDefaultJoinRoleChange={(value) => setPolicyField('default_join_role', value)}
                      />
                    </div>
                  </section>

                  <section className={`space-y-4 rf-room-mode-panel-late ${modeMotionClass}`}>
                    <div style={fixedColumnHeightStyle}>
                      <UserInvitePicker
                        users={users}
                        currentUserId={me.id}
                        roomMode={effectivePolicyRoomMode}
                        selected={selectedInvites}
                        noShadow
                        fillHeight
                        onToggle={toggleInvite}
                        onRoleChange={setInviteRole}
                      />
                    </div>
                  </section>
                </div>

                <div className="space-y-2 border-t border-[var(--border-subtle)] pt-5">
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
                  <button
                    type="button"
                    className="btn-ghost w-full px-4 py-2 text-sm"
                    onClick={() => setCreateModalOpen(false)}
                    disabled={creating}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            </div>,
            document.body,
          )
        : null}

    </div>
  );
}
