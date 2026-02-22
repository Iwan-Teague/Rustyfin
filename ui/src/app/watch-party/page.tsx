'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { apiJson } from '@/lib/api';
import { useAuth } from '@/lib/auth';
import {
  WatchPartyInvite,
  WatchPartyPolicy,
  WatchPartyUser,
  createWatchPartyRoom,
  declineWatchPartyInvite,
  getEligibleLibraries,
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

type LibrarySummary = {
  id: string;
  name: string;
  kind: string;
};

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
  const [invites, setInvites] = useState<WatchPartyInvite[]>([]);

  const [selectedInvites, setSelectedInvites] = useState<Record<string, SelectedInvite>>({});
  const [eligibleLibraryIds, setEligibleLibraryIds] = useState<string[]>([]);
  const [selectedLibraryId, setSelectedLibraryId] = useState('');
  const [selectedItem, setSelectedItem] = useState<MediaItemNode | null>(null);
  const [policy, setPolicy] = useState<WatchPartyPolicy>(DEFAULT_POLICY);
  const [password, setPassword] = useState('');

  const [loading, setLoading] = useState(true);
  const [decliningRoomId, setDecliningRoomId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const selectedInviteIds = useMemo(() => Object.keys(selectedInvites), [selectedInvites]);

  const visibleLibraries = useMemo(
    () => libraries.filter((library) => eligibleLibraryIds.includes(library.id)),
    [libraries, eligibleLibraryIds],
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
        const [userList, libraryList, inviteList] = await Promise.all([
          listWatchPartyUsers(),
          apiJson<LibrarySummary[]>('/libraries'),
          listWatchPartyInvites(),
        ]);

        if (cancelled) return;

        setUsers(userList);
        setLibraries(libraryList);
        setInvites(inviteList);

        const initialEligible = await getEligibleLibraries([]);
        if (cancelled) return;

        setEligibleLibraryIds(initialEligible);

        const nextLibraryId =
          initialEligible.find((id) => libraryList.some((library) => library.id === id)) || '';
        setSelectedLibraryId(nextLibraryId);
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
          setSelectedLibraryId(eligible[0] || '');
          setSelectedItem(null);
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
  }, [selectedInviteIds, me, selectedLibraryId]);

  function setPolicyField<K extends keyof WatchPartyPolicy>(key: K, value: WatchPartyPolicy[K]) {
    setPolicy((prev) => ({ ...prev, [key]: value }));
  }

  function toggleInvite(userId: string) {
    setSelectedInvites((prev) => {
      const next = { ...prev };
      if (next[userId]) {
        delete next[userId];
      } else {
        next[userId] = { role: 'viewer' };
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
    if (!selectedItem) {
      setError('Select a movie or episode first.');
      return;
    }

    if (!eligibleLibraryIds.includes(selectedItem.library_id)) {
      setError('The selected media is not accessible to all selected invitees.');
      return;
    }

    setCreating(true);
    setError('');
    setMessage('');

    try {
      const invitesPayload = Object.entries(selectedInvites).map(([user_id, config]) => ({
        user_id,
        role: config.role,
      }));

      const payload = {
        item_id: selectedItem.id,
        invites: invitesPayload,
        password: password.trim() ? password.trim() : undefined,
        policy,
      };

      const created = await createWatchPartyRoom(payload);
      setMessage(`Room created: ${created.room_id}`);
      router.push(created.join_path);
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

  return (
    <div className="space-y-6 animate-rise">
      <header className="panel space-y-3 p-6 sm:p-7">
        <span className="chip chip-accent">Watch Party</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Create a shared playback room</h1>
        <p className="text-sm muted sm:text-base">
          Choose a playable item, invite users, set room controls, and share a room link.
        </p>
      </header>

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {message && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{message}</div>}

      <div className="grid gap-4 xl:grid-cols-2">
        <MediaPicker
          libraries={libraries}
          eligibleLibraryIds={eligibleLibraryIds}
          selectedLibraryId={selectedLibraryId}
          selectedItem={selectedItem}
          onLibraryChange={setSelectedLibraryId}
          onSelectItem={setSelectedItem}
        />

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
              disabled={creating || !selectedItem}
            >
              {creating ? 'Creating room…' : 'Create Watch Party'}
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

      {visibleLibraries.length === 0 && (
        <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
          No shared libraries are available for the current invite selection.
        </div>
      )}
    </div>
  );
}
