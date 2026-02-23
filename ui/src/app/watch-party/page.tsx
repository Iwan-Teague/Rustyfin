'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { apiJson } from '@/lib/api';
import { useAuth } from '@/lib/auth';
import {
  PublicRoom,
  WatchPartyInvite,
  WatchPartyPolicy,
  WatchPartyUser,
  createWatchPartyRoom,
  declineWatchPartyInvite,
  getEligibleLibraries,
  listPublicRooms,
  listWatchPartyInvites,
  listWatchPartyUsers,
} from '@/lib/watchPartyApi';
import MediaPicker, {
  MediaItemNode,
  MediaLibrary,
} from './components/MediaPicker';
import UserInvitePicker, { SelectedInvite } from './components/UserInvitePicker';
import RoomOptions from './components/RoomOptions';
import InvitesPanel from './components/InvitesPanel';
import { elapsedSinceSeconds, formatElapsedSeconds } from '@/lib/time';

type LibrarySummary = {
  id: string;
  name: string;
  kind: string;
};

type RoomMode = 'video' | 'audio' | 'youtube';

const DEFAULT_POLICY: WatchPartyPolicy = {
  allow_non_host_play_pause: true,
  allow_non_host_seek: false,
  default_join_role: 'viewer',
  invite_only: false,
};

export default function WatchPartyPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [users, setUsers] = useState<WatchPartyUser[]>([]);
  const [libraries, setLibraries] = useState<MediaLibrary[]>([]);
  const [allLibraries, setAllLibraries] = useState<LibrarySummary[]>([]);
  const [invites, setInvites] = useState<WatchPartyInvite[]>([]);

  const [roomMode, setRoomMode] = useState<RoomMode>('video');
  const [selectedInvites, setSelectedInvites] = useState<Record<string, SelectedInvite>>({});
  const [eligibleLibraryIds, setEligibleLibraryIds] = useState<string[]>([]);
  const [selectedLibraryId, setSelectedLibraryId] = useState('');
  const [selectedItem, setSelectedItem] = useState<MediaItemNode | null>(null);
  const [selectedAudioLibraryId, setSelectedAudioLibraryId] = useState('');
  const [policy, setPolicy] = useState<WatchPartyPolicy>(DEFAULT_POLICY);
  const [password, setPassword] = useState('');

  const [loading, setLoading] = useState(true);
  const [decliningRoomId, setDecliningRoomId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [publicRooms, setPublicRooms] = useState<PublicRoom[]>([]);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const selectedInviteIds = useMemo(() => Object.keys(selectedInvites), [selectedInvites]);

  const visibleLibraries = useMemo(
    () => libraries.filter((library) => eligibleLibraryIds.includes(library.id)),
    [libraries, eligibleLibraryIds],
  );

  const musicLibraries = useMemo(
    () =>
      allLibraries.filter(
        (lib) => lib.kind === 'music' && eligibleLibraryIds.includes(lib.id),
      ),
    [allLibraries, eligibleLibraryIds],
  );

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
        const [userList, libraryList, inviteList, publicRoomList] = await Promise.all([
          listWatchPartyUsers(),
          apiJson<LibrarySummary[]>('/libraries'),
          listWatchPartyInvites(),
          listPublicRooms(),
        ]);

        if (cancelled) return;

        setUsers(userList);
        setAllLibraries(libraryList);
        setLibraries(libraryList);
        setInvites(inviteList);
        setPublicRooms(publicRoomList);

        const initialEligible = await getEligibleLibraries([]);
        if (cancelled) return;

        setEligibleLibraryIds(initialEligible);

        const videoLib = libraryList.find(
          (lib) => lib.kind !== 'music' && initialEligible.includes(lib.id),
        );
        setSelectedLibraryId(videoLib?.id || '');

        const musicLib = libraryList.find(
          (lib) => lib.kind === 'music' && initialEligible.includes(lib.id),
        );
        setSelectedAudioLibraryId(musicLib?.id || '');
      } catch (err: any) {
        if (!cancelled) {
          setError(err?.message || 'Failed to load watch-party data');
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
    if (!me) return;

    let cancelled = false;
    setError('');

    (async () => {
      try {
        const eligible = await getEligibleLibraries(selectedInviteIds);
        if (cancelled) return;

        setEligibleLibraryIds(eligible);

        if (!eligible.includes(selectedLibraryId)) {
          const nextVideoLib = allLibraries.find(
            (lib) => lib.kind !== 'music' && eligible.includes(lib.id),
          );
          setSelectedLibraryId(nextVideoLib?.id || '');
          setSelectedItem(null);
        }

        if (!eligible.includes(selectedAudioLibraryId)) {
          const nextMusicLib = allLibraries.find(
            (lib) => lib.kind === 'music' && eligible.includes(lib.id),
          );
          setSelectedAudioLibraryId(nextMusicLib?.id || '');
        }
      } catch (err: any) {
        if (!cancelled) {
          setError(err?.message || 'Failed to update shared library intersection');
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [selectedInviteIds, me, selectedLibraryId, selectedAudioLibraryId, allLibraries]);

  useEffect(() => {
    if (publicRooms.length === 0) return;
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [publicRooms.length]);

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

  async function refreshInvites() {
    try {
      setInvites(await listWatchPartyInvites());
    } catch {
      // Non-fatal for main create flow.
    }
  }

  async function handleDeclineInvite(roomId: string) {
    setDecliningRoomId(roomId);
    try {
      await declineWatchPartyInvite(roomId);
      await refreshInvites();
    } catch (err: any) {
      setError(err?.message || 'Failed to decline invite');
    } finally {
      setDecliningRoomId(null);
    }
  }

  async function handleCreateRoom() {
    setCreating(true);
    setError('');
    setMessage('');

    try {
      const invitesPayload = Object.entries(selectedInvites).map(([user_id, config]) => ({
        user_id,
        role: config.role,
      }));

      if (roomMode === 'audio') {
        if (!selectedAudioLibraryId) {
          setError('Select a music library first.');
          return;
        }

        const payload = {
          audio_library_id: selectedAudioLibraryId,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      } else if (roomMode === 'youtube') {
        const payload = {
          room_mode: 'youtube' as const,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      } else {
        if (!selectedItem) {
          setError('Select a movie or episode first.');
          return;
        }

        if (!eligibleLibraryIds.includes(selectedItem.library_id)) {
          setError('The selected media is not accessible to all selected invitees.');
          return;
        }

        const payload = {
          item_id: selectedItem.id,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      }
    } catch (err: any) {
      setError(err?.message || 'Failed to create watch party room');
    } finally {
      setCreating(false);
    }
  }

  if (authLoading || loading) {
    return (
      <div className="panel-soft animate-rise px-5 py-4">
        <p className="text-sm muted">Loading watch-party workspace...</p>
      </div>
    );
  }

  if (!me) {
    return null;
  }

  const canCreate =
    roomMode === 'audio' ? !!selectedAudioLibraryId : roomMode === 'youtube' ? true : !!selectedItem;

  return (
    <div className="space-y-6 animate-rise">

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {message && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{message}</div>}

      {/* Public rooms currently running */}
      {publicRooms.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-xl font-semibold sm:text-2xl">Open Rooms</h2>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {publicRooms.map((room) => (
              <div key={room.room_id} className="tile p-4 flex items-center justify-between gap-3">
                <div className="min-w-0 space-y-0.5">
                  <p className="font-semibold truncate">{room.title}</p>
                  <p className="text-xs muted">
                    Hosted by {room.host_username}
                    {room.member_count > 0 && ` · ${room.member_count} watching`}
                    {' · '}
                    {formatElapsedSeconds(elapsedSinceSeconds(room.created_ts, nowMs))}
                  </p>
                  {room.password_required && (
                    <span className="chip text-xs">🔒 Password</span>
                  )}
                </div>
                <Link
                  href={`/watch-party/rooms/${room.room_id}`}
                  className="btn-primary shrink-0 px-4 py-2 text-sm"
                >
                  Join
                </Link>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Create room section */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Create a Room</h2>
      </section>

      <section className="panel p-5 sm:p-6">
        <div className="flex gap-2 flex-wrap">
          <button
            type="button"
            className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'video' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setRoomMode('video')}
          >
            Watch a Video
          </button>
          <button
            type="button"
            className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'audio' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setRoomMode('audio')}
          >
            Listen Together
          </button>
          <button
            type="button"
            className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'youtube' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setRoomMode('youtube')}
          >
            YouTube
          </button>
        </div>
      </section>

      <div className="grid gap-4 xl:grid-cols-2">
        {roomMode === 'video' ? (
          <MediaPicker
            libraries={libraries}
            eligibleLibraryIds={eligibleLibraryIds}
            selectedLibraryId={selectedLibraryId}
            selectedItem={selectedItem}
            onLibraryChange={setSelectedLibraryId}
            onSelectItem={setSelectedItem}
          />
        ) : roomMode === 'youtube' ? (
          <section className="panel space-y-4 p-5 sm:p-6">
            <div className="space-y-2">
              <h2 className="text-xl font-semibold">YouTube Party</h2>
              <p className="text-sm muted">
                Watch YouTube videos together in sync. Once in the room, paste any YouTube URL or
                video ID to load a video for everyone.
              </p>
            </div>
            <div className="notice-ok rounded-xl px-3 py-3 text-sm">
              No media library required — anyone you invite can join immediately.
            </div>
          </section>
        ) : (
          <section className="panel space-y-4 p-5 sm:p-6">
            <div className="space-y-2">
              <h2 className="text-xl font-semibold">Music Library</h2>
              <p className="text-sm muted">
                Pick a music library. All tracks will be shuffled into a shared queue.
              </p>
            </div>

            {musicLibraries.length === 0 ? (
              <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                No music libraries available. Create a music library and scan it first.
              </div>
            ) : (
              <div className="space-y-2">
                <label
                  htmlFor="audio-library-select"
                  className="block text-xs uppercase tracking-wide muted"
                >
                  Music Library
                </label>
                <select
                  id="audio-library-select"
                  value={selectedAudioLibraryId}
                  onChange={(e) => setSelectedAudioLibraryId(e.target.value)}
                  className="select px-3 py-2 text-sm"
                >
                  {musicLibraries.map((lib) => (
                    <option key={lib.id} value={lib.id}>
                      {lib.name}
                    </option>
                  ))}
                </select>
                {selectedAudioLibraryId && (
                  <div className="notice-ok rounded-xl px-3 py-2 text-xs">
                    Selected:{' '}
                    <strong>
                      {musicLibraries.find((l) => l.id === selectedAudioLibraryId)?.name}
                    </strong>
                  </div>
                )}
              </div>
            )}
          </section>
        )}

        <div className="space-y-4">
          <UserInvitePicker
            users={users}
            currentUserId={me.id}
            selected={selectedInvites}
            onToggle={toggleInvite}
            onRoleChange={setInviteRole}
          />

          <RoomOptions
            password={password}
            allowPlayPause={policy.allow_non_host_play_pause}
            allowSeek={policy.allow_non_host_seek}
            inviteOnly={policy.invite_only}
            defaultJoinRole={policy.default_join_role}
            onPasswordChange={setPassword}
            onAllowPlayPauseChange={(value) => setPolicyField('allow_non_host_play_pause', value)}
            onAllowSeekChange={(value) => setPolicyField('allow_non_host_seek', value)}
            onInviteOnlyChange={(value) => setPolicyField('invite_only', value)}
            onDefaultJoinRoleChange={(value) => setPolicyField('default_join_role', value)}
          />

          <section className="panel p-5 sm:p-6">
            <button
              type="button"
              className="btn-primary w-full px-5 py-3 text-sm disabled:opacity-50"
              onClick={handleCreateRoom}
              disabled={creating || !canCreate}
            >
              {creating
                ? 'Creating room…'
                : roomMode === 'audio'
                  ? 'Create Music Party'
                  : roomMode === 'youtube'
                    ? 'Create YouTube Party'
                    : 'Create Watch Party'}
            </button>
          </section>
        </div>
      </div>

      <InvitesPanel
        invites={invites}
        onJoin={(roomId) => router.push(`/watch-party/rooms/${roomId}`)}
        onDecline={handleDeclineInvite}
        decliningRoomId={decliningRoomId}
      />

      {roomMode !== 'youtube' && visibleLibraries.length === 0 && musicLibraries.length === 0 && (
        <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
          No shared libraries are available for the current invite selection.
        </div>
      )}
    </div>
  );
}
