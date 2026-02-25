'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
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

type RoomMode = 'watch' | 'audio' | 'play' | 'create';
type WatchSource = 'local' | 'youtube' | 'web';
type AudioSource = 'local' | 'online';
type CreateTool = 'text' | 'canvas';
type RightPanelTab = 'invites' | 'options';

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

  const [roomMode, setRoomMode] = useState<RoomMode>('watch');
  const [watchSource, setWatchSource] = useState<WatchSource>('local');
  const [audioSource, setAudioSource] = useState<AudioSource>('local');
  const [createTool, setCreateTool] = useState<CreateTool>('text');
  const [webStartUrl, setWebStartUrl] = useState('');
  const [rightPanelTab, setRightPanelTab] = useState<RightPanelTab>('invites');
  const [selectedInvites, setSelectedInvites] = useState<Record<string, SelectedInvite>>({});
  const [eligibleLibraryIds, setEligibleLibraryIds] = useState<string[]>([]);
  const [selectedLibraryId, setSelectedLibraryId] = useState('');
  const [selectedItem, setSelectedItem] = useState<MediaItemNode | null>(null);
  const [selectedAudioLibraryId, setSelectedAudioLibraryId] = useState('');
  const [roomName, setRoomName] = useState('');
  const [policy, setPolicy] = useState<WatchPartyPolicy>(DEFAULT_POLICY);
  const [password, setPassword] = useState('');

  const [loading, setLoading] = useState(true);
  const [decliningRoomId, setDecliningRoomId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [publicRooms, setPublicRooms] = useState<PublicRoom[]>([]);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [fixedColumnHeightPx, setFixedColumnHeightPx] = useState<number | null>(null);
  const roomOptionsMeasureRef = useRef<HTMLDivElement | null>(null);

  const selectedInviteIds = useMemo(() => Object.keys(selectedInvites), [selectedInvites]);
  const effectivePolicyRoomMode = roomMode === 'audio'
    ? 'audio'
    : roomMode === 'create'
      ? 'create'
    : watchSource === 'youtube'
      ? 'youtube'
      : watchSource === 'web'
        ? 'web'
      : 'video';

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

  useEffect(() => {
    if (loading) return;
    const measureEl = roomOptionsMeasureRef.current;
    if (!measureEl) return;

    const updateHeight = () => {
      const measured = Math.ceil(measureEl.getBoundingClientRect().height);
      if (measured > 0) {
        setFixedColumnHeightPx(measured);
      }
    };

    updateHeight();

    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => updateHeight());
    observer.observe(measureEl);
    return () => observer.disconnect();
  }, [
    loading,
    effectivePolicyRoomMode,
    password,
    policy.allow_non_host_play_pause,
    policy.allow_non_host_seek,
    policy.invite_only,
    policy.default_join_role,
  ]);

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
      if (roomMode === 'play') {
        setError('Play Together rooms are coming soon.');
        return;
      }

      const invitesPayload = Object.entries(selectedInvites).map(([user_id, config]) => ({
        user_id,
        role: config.role,
      }));
      const normalizedRoomName = roomName.trim();

      if (roomMode === 'create') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'create' as const,
          create_tool: createTool,
          create_document_name: normalizedRoomName || undefined,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };
        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      } else if (roomMode === 'audio') {
        const payload =
          audioSource === 'online'
            ? {
                room_name: normalizedRoomName || undefined,
                room_mode: 'audio' as const,
                audio_source: 'online' as const,
                invites: invitesPayload,
                password: password.trim() ? password.trim() : undefined,
                policy,
              }
            : (() => {
                if (!selectedAudioLibraryId) {
                  throw new Error('Select a music library first.');
                }
                return {
                  room_name: normalizedRoomName || undefined,
                  room_mode: 'audio' as const,
                  audio_source: 'library' as const,
                  audio_library_id: selectedAudioLibraryId,
                  invites: invitesPayload,
                  password: password.trim() ? password.trim() : undefined,
                  policy,
                };
              })();

        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      } else if (watchSource === 'web') {
        const payload = {
          room_name: normalizedRoomName || undefined,
          room_mode: 'web' as const,
          web_url: webStartUrl.trim() || undefined,
          invites: invitesPayload,
          password: password.trim() ? password.trim() : undefined,
          policy,
        };

        const created = await createWatchPartyRoom(payload);
        setMessage(`Room created: ${created.room_id}`);
        router.push(created.join_path);
      } else if (watchSource === 'youtube') {
        const payload = {
          room_name: normalizedRoomName || undefined,
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
          room_name: normalizedRoomName || undefined,
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
    roomMode === 'play'
      ? false
      : roomMode === 'create'
        ? true
      : roomMode === 'audio'
        ? (audioSource === 'online' ? true : !!selectedAudioLibraryId)
        : watchSource === 'youtube' || watchSource === 'web'
          ? true
          : !!selectedItem;
  const fixedColumnHeightStyle = fixedColumnHeightPx
    ? { height: `${fixedColumnHeightPx}px` }
    : { minHeight: '30rem' };

  return (
    <div className="space-y-6 animate-rise">

      {error && <div className="notice-error rounded-xl px-4 py-2 text-sm">{error}</div>}
      {message && <div className="notice-ok rounded-xl px-4 py-2 text-sm">{message}</div>}

      <div className="grid gap-5 xl:grid-cols-2">
        <section className="panel space-y-4 p-5 sm:p-6">
          <div className="space-y-2">
            <h2 className="text-xl font-semibold">Open Rooms</h2>
            <p className="text-sm muted">Public rooms you can join right now.</p>
          </div>

          {publicRooms.length === 0 ? (
            <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">No open rooms right now.</div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2">
              {publicRooms.map((room) => (
                <div key={room.room_id} className="tile p-4 flex items-center justify-between gap-3">
                  <div className="min-w-0 space-y-0.5">
                    <p className="font-semibold truncate">{room.title}</p>
                    <p className="text-xs muted">
                      Hosted by {room.host_username}
                      {room.member_count > 0 && ` · ${room.member_count} joined`}
                      {' · '}
                      {formatElapsedSeconds(elapsedSinceSeconds(room.created_ts, nowMs))}
                    </p>
                    {room.password_required && (
                      <span className="chip text-xs">Password Protected</span>
                    )}
                  </div>
                  <Link
                    href={`/rooms/${room.room_id}`}
                    className="btn-primary shrink-0 px-4 py-2 text-sm"
                  >
                    Join
                  </Link>
                </div>
              ))}
            </div>
          )}
        </section>

        <InvitesPanel
          invites={invites}
          onJoin={(roomId) => router.push(`/rooms/${roomId}`)}
          onDecline={handleDeclineInvite}
          decliningRoomId={decliningRoomId}
        />
      </div>

      {/* Create room section */}
      <section className="space-y-3">
        <h2 className="text-xl font-semibold sm:text-2xl">Create a Room</h2>
      </section>

      <section className="panel p-5 sm:p-6">
        <div className="space-y-3">
          <label className="block text-sm">
            <span className="mb-1 block text-xs uppercase tracking-wide muted">Room Name</span>
            <input
              type="text"
              value={roomName}
              onChange={(e) => setRoomName(e.target.value)}
              className="input px-3 py-2 text-sm"
              placeholder="Optional room name"
              maxLength={120}
            />
          </label>

          <div className="flex gap-2 flex-wrap">
            <button
              type="button"
              className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'watch' ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setRoomMode('watch')}
            >
              Watch Together
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
              className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'play' ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setRoomMode('play')}
            >
              Play Together
            </button>
            <button
              type="button"
              className={`px-4 py-2 text-sm rounded-lg ${roomMode === 'create' ? 'btn-primary' : 'btn-secondary'}`}
              onClick={() => setRoomMode('create')}
            >
              Create Together
            </button>
          </div>
        </div>
      </section>

      <div className="mt-3 grid gap-5 xl:grid-cols-2">
        <section className="space-y-4">
          {roomMode === 'watch' && (
            <div className="flex gap-2 border-b border-[var(--border)] pb-0">
              {(['local', 'youtube', 'web'] as const).map((source) => (
                <button
                  key={source}
                  type="button"
                  onClick={() => setWatchSource(source)}
                  className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
                    watchSource === source
                      ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                      : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
                  }`}
                >
                  {source === 'local' ? 'Local Media' : source === 'youtube' ? 'YouTube' : 'Web'}
                </button>
              ))}
            </div>
          )}

          <div className="overflow-hidden" style={fixedColumnHeightStyle}>
            <div className="h-full overflow-y-auto pr-1">
              {roomMode === 'watch' ? (
                watchSource === 'local' ? (
                  <MediaPicker
                    libraries={libraries}
                    eligibleLibraryIds={eligibleLibraryIds}
                    selectedLibraryId={selectedLibraryId}
                    selectedItem={selectedItem}
                    noShadow
                    onLibraryChange={setSelectedLibraryId}
                    onSelectItem={setSelectedItem}
                  />
                ) : watchSource === 'web' ? (
                  <section className="panel space-y-4 p-5 sm:p-6" style={{ boxShadow: 'none' }}>
                    <div className="space-y-2">
                      <h2 className="text-xl font-semibold">Watch Together (Web)</h2>
                      <p className="text-sm muted">
                        Enter a URL to open for everyone in a shared web room.
                      </p>
                    </div>
                    <label className="block text-sm">
                      <span className="mb-1 block text-xs uppercase tracking-wide muted">Initial URL</span>
                      <input
                        type="text"
                        value={webStartUrl}
                        onChange={(e) => setWebStartUrl(e.target.value)}
                        className="input px-3 py-2 text-sm"
                        placeholder="https://www.mozilla.org/"
                        maxLength={2048}
                      />
                    </label>
                    <p className="text-xs muted">
                      Hosts/admins can change the URL live inside the room. Some websites block iframe embedding.
                    </p>
                  </section>
                ) : (
                  <section className="panel space-y-4 p-5 sm:p-6" style={{ boxShadow: 'none' }}>
                    <div className="space-y-2">
                      <h2 className="text-xl font-semibold">Watch Together (YouTube)</h2>
                      <p className="text-sm muted">
                        Create the room first, then use shared YouTube search in the lobby to choose and queue videos.
                      </p>
                    </div>
                    <div className="notice-ok rounded-xl px-3 py-3 text-sm">
                      No local media library is required for YouTube watch-together rooms.
                    </div>
                  </section>
                )
              ) : roomMode === 'audio' ? (
                <section className="panel space-y-4 p-5 sm:p-6">
                  <div className="space-y-3">
                    <h2 className="text-xl font-semibold">Listen Together</h2>
                    <div className="flex gap-2 border-b border-[var(--border)] pb-0">
                      {([
                        ['local', 'Local'],
                        ['online', 'Online'],
                      ] as const).map(([source, label]) => (
                        <button
                          key={source}
                          type="button"
                          onClick={() => setAudioSource(source)}
                          className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
                            audioSource === source
                              ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                              : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
                          }`}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                  </div>

                  {audioSource === 'online' ? (
                    <div className="space-y-3">
                      <p className="text-sm muted">
                        Online mode lets room members search YouTube tracks in-lobby. Tracks are downloaded to room-scoped MP3 files and removed when the room is deleted.
                      </p>
                      <div className="notice-ok rounded-xl px-3 py-3 text-sm">
                        Create the room, then search and queue tracks from inside the lobby.
                      </div>
                    </div>
                  ) : musicLibraries.length === 0 ? (
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
              ) : roomMode === 'create' ? (
                <section className="panel space-y-4 p-5 sm:p-6">
                  <div className="space-y-2">
                    <h2 className="text-xl font-semibold">Create Together</h2>
                    <p className="text-sm muted">
                      Work collaboratively on shared text/PDF documents or a paint-style shared canvas.
                    </p>
                  </div>

                  <div className="flex gap-2 border-b border-[var(--border)] pb-0">
                    {([
                      ['text', 'Shared Document'],
                      ['canvas', 'Shared Canvas'],
                    ] as const).map(([tool, label]) => (
                      <button
                        key={tool}
                        type="button"
                        onClick={() => setCreateTool(tool)}
                        className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
                          createTool === tool
                            ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                            : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
                        }`}
                      >
                        {label}
                      </button>
                    ))}
                  </div>

                  <div className="space-y-3">
                    {createTool === 'text' ? (
                      <>
                        <p className="text-sm muted">
                          Shared document mode supports live text editing, Markdown, PDF import (text extraction), and export to TXT/MD/PDF from inside the room.
                        </p>
                        <div className="notice-ok rounded-xl px-3 py-3 text-sm">
                          Members with edit access can collaboratively update one shared document in real time.
                        </div>
                      </>
                    ) : (
                      <>
                        <p className="text-sm muted">
                          Shared canvas mode provides an MS Paint style drawing board with color/brush controls and PNG download.
                        </p>
                        <div className="notice-ok rounded-xl px-3 py-3 text-sm">
                          Canvas edits sync to everyone in the room using realtime websocket updates.
                        </div>
                      </>
                    )}
                  </div>
                </section>
              ) : (
                <section className="panel space-y-4 p-5 sm:p-6">
                  <div className="space-y-2">
                    <h2 className="text-xl font-semibold">Play Together</h2>
                    <p className="text-sm muted">
                      Multiplayer room mode is being added. You can already use Watch Together and Listen Together.
                    </p>
                  </div>
                  <div className="panel-soft rounded-xl px-3 py-3 text-sm muted">
                    Play Together creation is not available yet.
                  </div>
                </section>
              )}
            </div>
          </div>
        </section>

        <section className="space-y-4 relative">
          <div className="flex gap-2 border-b border-[var(--border)] pb-0">
            {([
              ['invites', 'Invite Users'],
              ['options', 'Room Options'],
            ] as const).map(([tab, label]) => (
              <button
                key={tab}
                type="button"
                onClick={() => setRightPanelTab(tab)}
                className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
                  rightPanelTab === tab
                    ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                    : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
                }`}
              >
                {label}
              </button>
            ))}
          </div>

          <div
            aria-hidden="true"
            className="pointer-events-none absolute left-0 right-0 top-[calc(2.5rem+0.5rem)] -z-10 opacity-0"
          >
            <div ref={roomOptionsMeasureRef}>
              <RoomOptions
                roomMode={effectivePolicyRoomMode}
                password={password}
                allowPlayPause={policy.allow_non_host_play_pause}
                allowSeek={policy.allow_non_host_seek}
                inviteOnly={policy.invite_only}
                defaultJoinRole={policy.default_join_role}
                noShadow
                onPasswordChange={setPassword}
                onAllowPlayPauseChange={(value) => setPolicyField('allow_non_host_play_pause', value)}
                onAllowSeekChange={(value) => setPolicyField('allow_non_host_seek', value)}
                onInviteOnlyChange={(value) => setPolicyField('invite_only', value)}
                onDefaultJoinRoleChange={(value) => setPolicyField('default_join_role', value)}
              />
            </div>
          </div>

          <div className="overflow-hidden" style={fixedColumnHeightStyle}>
            {rightPanelTab === 'invites' ? (
              <div className="h-full overflow-y-auto pr-1">
                <UserInvitePicker
                  users={users}
                  currentUserId={me.id}
                  roomMode={effectivePolicyRoomMode}
                  selected={selectedInvites}
                  noShadow
                  onToggle={toggleInvite}
                  onRoleChange={setInviteRole}
                />
              </div>
            ) : (
              <div className="h-full overflow-y-auto pr-1">
                <RoomOptions
                  roomMode={effectivePolicyRoomMode}
                  password={password}
                  allowPlayPause={policy.allow_non_host_play_pause}
                  allowSeek={policy.allow_non_host_seek}
                  inviteOnly={policy.invite_only}
                  defaultJoinRole={policy.default_join_role}
                  noShadow
                  onPasswordChange={setPassword}
                  onAllowPlayPauseChange={(value) => setPolicyField('allow_non_host_play_pause', value)}
                  onAllowSeekChange={(value) => setPolicyField('allow_non_host_seek', value)}
                  onInviteOnlyChange={(value) => setPolicyField('invite_only', value)}
                  onDefaultJoinRoleChange={(value) => setPolicyField('default_join_role', value)}
                />
              </div>
            )}
          </div>
        </section>
      </div>

      <section className="panel p-5 sm:p-6">
        <button
          type="button"
          className="btn-primary w-full px-5 py-3 text-sm disabled:opacity-50"
          onClick={handleCreateRoom}
          disabled={creating || !canCreate}
        >
          {creating ? 'Creating room…' : 'Create Room'}
        </button>
      </section>

      {roomMode === 'audio' && audioSource === 'local' && musicLibraries.length === 0 && (
        <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
          No shared music libraries are available for the current invite selection.
        </div>
      )}
    </div>
  );
}
