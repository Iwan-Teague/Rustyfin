'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { apiJson, apiFetch } from '@/lib/api';
import { useAuth } from '@/lib/auth';

interface Library {
  id: string;
  name: string;
  kind: string;
  paths: { id: string; path: string; is_read_only: boolean }[];
  settings: {
    show_images: boolean;
    prefer_local_artwork: boolean;
    fetch_online_artwork: boolean;
  };
  item_count: number;
}

interface LibraryEditState {
  name: string;
  path: string;
  show_images: boolean;
  prefer_local_artwork: boolean;
  fetch_online_artwork: boolean;
}

interface Job {
  id: string;
  kind: string;
  status: string;
  progress: number;
  payload?: Record<string, unknown> | null;
  error?: string | null;
  created_ts: number;
  updated_ts: number;
}

interface UserAccount {
  id: string;
  username: string;
  role: 'admin' | 'user';
  created_ts: number;
  library_ids: string[];
}

interface UserEditState {
  role: 'admin' | 'user';
  library_ids: string[];
}

interface ChannelRecord {
  id: string;
  name: string;
  kind: 'text' | 'voice';
  position: number;
  is_private: boolean;
  created_by: string;
  created_ts: number;
}

interface ChannelEditState {
  name: string;
  is_private: boolean;
}

interface RoomRecord {
  room_id: string;
  room_name: string;
  title: string;
  host_user_id: string;
  host_username: string;
  item_id: string;
  status: string;
  room_mode: string;
  audio_library_name: string;
  web_url: string;
  password_required: boolean;
  invite_only: boolean;
  member_count: number;
  created_ts: number;
  updated_ts: number;
}

interface RoomEditState {
  room_name: string;
}

interface TmdbConfig {
  configured: boolean;
  key_preview: string | null;
  source: 'database' | 'environment' | null;
}

type AdminTab = 'users' | 'libraries' | 'channels' | 'rooms' | 'logs' | 'tmdb';

const ADMIN_TABS: { key: AdminTab; label: string }[] = [
  { key: 'users', label: 'Users' },
  { key: 'libraries', label: 'Libraries' },
  { key: 'channels', label: 'Channels' },
  { key: 'rooms', label: 'Rooms' },
  { key: 'logs', label: 'Logs' },
  { key: 'tmdb', label: 'TMDB Metadata' },
];

export default function AdminPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [activeTab, setActiveTab] = useState<AdminTab>('users');

  const [libraries, setLibraries] = useState<Library[]>([]);
  const [libraryEdits, setLibraryEdits] = useState<Record<string, LibraryEditState>>({});
  const [jobs, setJobs] = useState<Job[]>([]);
  const [users, setUsers] = useState<UserAccount[]>([]);
  const [userEdits, setUserEdits] = useState<Record<string, UserEditState>>({});
  const [channels, setChannels] = useState<ChannelRecord[]>([]);
  const [channelEdits, setChannelEdits] = useState<Record<string, ChannelEditState>>({});
  const [rooms, setRooms] = useState<RoomRecord[]>([]);
  const [roomEdits, setRoomEdits] = useState<Record<string, RoomEditState>>({});
  const [pendingDeleteRoom, setPendingDeleteRoom] = useState<RoomRecord | null>(null);

  const [newLib, setNewLib] = useState({
    name: '',
    kind: 'movies',
    path: '',
    show_images: true,
    prefer_local_artwork: true,
    fetch_online_artwork: true,
  });
  const [newUser, setNewUser] = useState({
    username: '',
    password: '',
    role: 'user' as 'admin' | 'user',
    library_ids: [] as string[],
  });
  const [newChannel, setNewChannel] = useState({
    name: '',
    kind: 'text' as 'text' | 'voice',
    is_private: false,
  });

  const [pickingPath, setPickingPath] = useState(false);
  const [pickingPathForLibraryId, setPickingPathForLibraryId] = useState<string | null>(null);
  const [tmdbConfig, setTmdbConfig] = useState<TmdbConfig>({
    configured: false,
    key_preview: null,
    source: null,
  });
  const [tmdbApiKey, setTmdbApiKey] = useState('');
  const [savingTmdb, setSavingTmdb] = useState(false);
  const [msg, setMsg] = useState('');
  const [msgType, setMsgType] = useState<'ok' | 'error'>('ok');

  const librariesRef = useRef<Library[]>([]);
  const usersRef = useRef<UserAccount[]>([]);
  const channelsRef = useRef<ChannelRecord[]>([]);
  const roomsRef = useRef<RoomRecord[]>([]);

  function sameLibraryIds(a: string[], b: string[]): boolean {
    if (a.length !== b.length) return false;
    const setA = new Set(a);
    if (setA.size !== b.length) return false;
    return b.every((id) => setA.has(id));
  }

  function toLibraryEditState(library: Library): LibraryEditState {
    return {
      name: library.name,
      path: library.paths[0]?.path || '',
      show_images: library.settings?.show_images ?? true,
      prefer_local_artwork: library.settings?.prefer_local_artwork ?? true,
      fetch_online_artwork: library.settings?.fetch_online_artwork ?? true,
    };
  }

  function sameLibraryEdit(a: LibraryEditState, b: LibraryEditState): boolean {
    return (
      a.name === b.name &&
      a.path === b.path &&
      a.show_images === b.show_images &&
      a.prefer_local_artwork === b.prefer_local_artwork &&
      a.fetch_online_artwork === b.fetch_online_artwork
    );
  }

  function toChannelEditState(channel: ChannelRecord): ChannelEditState {
    return {
      name: channel.name,
      is_private: channel.is_private,
    };
  }

  function sameChannelEdit(a: ChannelEditState, b: ChannelEditState): boolean {
    return a.name === b.name && a.is_private === b.is_private;
  }

  function toRoomEditState(room: RoomRecord): RoomEditState {
    return {
      room_name: room.room_name,
    };
  }

  function sameRoomEdit(a: RoomEditState, b: RoomEditState): boolean {
    return a.room_name === b.room_name;
  }

  useEffect(() => {
    if (!authLoading && (!me || me.role !== 'admin')) {
      router.replace('/libraries');
    }
  }, [authLoading, me, router]);

  const loadData = useCallback(async () => {
    try {
      const [libs, jobList, userList, tmdb, channelList, roomList] = await Promise.all([
        apiJson<Library[]>('/libraries'),
        apiJson<Job[]>('/jobs'),
        apiJson<UserAccount[]>('/users'),
        apiJson<TmdbConfig>('/system/tmdb'),
        apiJson<ChannelRecord[]>('/channels'),
        apiJson<RoomRecord[]>('/watch-party/admin/rooms'),
      ]);

      setLibraries(libs);
      setLibraryEdits((prev) => {
        const currentLibrariesById = new Map(
          librariesRef.current.map((library) => [library.id, library]),
        );
        const nextLibEdits: Record<string, LibraryEditState> = {};
        for (const lib of libs) {
          const serverEdit = toLibraryEditState(lib);
          const prevEdit = prev[lib.id];
          const currentLibrary = currentLibrariesById.get(lib.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentLibrary &&
            !sameLibraryEdit(prevEdit, toLibraryEditState(currentLibrary));
          nextLibEdits[lib.id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextLibEdits;
      });

      setJobs(jobList);
      setUsers(userList);
      setUserEdits((prev) => {
        const currentUsersById = new Map(usersRef.current.map((user) => [user.id, user]));
        const nextEdits: Record<string, UserEditState> = {};
        for (const user of userList) {
          const serverEdit: UserEditState = {
            role: user.role,
            library_ids: [...(user.library_ids || [])],
          };
          const prevEdit = prev[user.id];
          const currentUser = currentUsersById.get(user.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentUser &&
            (prevEdit.role !== currentUser.role ||
              !sameLibraryIds(prevEdit.library_ids, currentUser.library_ids || []));
          nextEdits[user.id] = hasUnsavedChanges
            ? {
                role: prevEdit.role,
                library_ids: [...prevEdit.library_ids],
              }
            : serverEdit;
        }
        return nextEdits;
      });

      setChannels(channelList);
      setChannelEdits((prev) => {
        const currentChannelsById = new Map(channelsRef.current.map((ch) => [ch.id, ch]));
        const nextEdits: Record<string, ChannelEditState> = {};
        for (const channel of channelList) {
          const serverEdit = toChannelEditState(channel);
          const prevEdit = prev[channel.id];
          const currentChannel = currentChannelsById.get(channel.id);
          const hasUnsavedChanges =
            !!prevEdit &&
            !!currentChannel &&
            !sameChannelEdit(prevEdit, toChannelEditState(currentChannel));
          nextEdits[channel.id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextEdits;
      });

      setRooms(roomList);
      setRoomEdits((prev) => {
        const currentRoomsById = new Map(roomsRef.current.map((room) => [room.room_id, room]));
        const nextEdits: Record<string, RoomEditState> = {};
        for (const room of roomList) {
          const serverEdit = toRoomEditState(room);
          const prevEdit = prev[room.room_id];
          const currentRoom = currentRoomsById.get(room.room_id);
          const hasUnsavedChanges =
            !!prevEdit && !!currentRoom && !sameRoomEdit(prevEdit, toRoomEditState(currentRoom));
          nextEdits[room.room_id] = hasUnsavedChanges ? { ...prevEdit } : serverEdit;
        }
        return nextEdits;
      });

      setTmdbConfig({
        configured: tmdb.configured,
        key_preview: tmdb.key_preview ?? null,
        source: tmdb.source ?? null,
      });
    } catch (err: any) {
      setMsgType('error');
      setMsg(err.message || 'Failed to load admin data');
    }
  }, []);

  useEffect(() => {
    if (me?.role === 'admin') {
      void loadData();
    }
  }, [me, loadData]);

  useEffect(() => {
    librariesRef.current = libraries;
  }, [libraries]);

  useEffect(() => {
    usersRef.current = users;
  }, [users]);

  useEffect(() => {
    channelsRef.current = channels;
  }, [channels]);

  useEffect(() => {
    roomsRef.current = rooms;
  }, [rooms]);

  const hasActiveJobs = useMemo(
    () => jobs.some((job) => job.status === 'queued' || job.status === 'running'),
    [jobs],
  );

  useEffect(() => {
    if (me?.role !== 'admin') return;
    const intervalMs = hasActiveJobs ? 1000 : 5000;
    const timer = setInterval(() => {
      void loadData();
    }, intervalMs);
    return () => clearInterval(timer);
  }, [me, hasActiveJobs, loadData]);

  const usersById = useMemo(() => {
    return new Map(users.map((u) => [u.id, u]));
  }, [users]);

  function setOk(message: string) {
    setMsgType('ok');
    setMsg(message);
  }

  function setErr(message: string) {
    setMsgType('error');
    setMsg(message);
  }

  async function createLibrary(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/libraries', {
        method: 'POST',
        body: JSON.stringify({
          name: newLib.name,
          kind: newLib.kind,
          paths: [newLib.path],
          settings: {
            show_images: newLib.show_images,
            prefer_local_artwork: newLib.prefer_local_artwork,
            fetch_online_artwork: newLib.fetch_online_artwork,
          },
        }),
      });
      setOk('Library created');
      setNewLib({
        name: '',
        kind: 'movies',
        path: '',
        show_images: true,
        prefer_local_artwork: true,
        fetch_online_artwork: true,
      });
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to create library');
    }
  }

  async function scanLibrary(libId: string) {
    try {
      await apiJson(`/libraries/${libId}/scan`, { method: 'POST' });
      setOk('Scan started');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to start scan');
    }
  }

  async function syncLibraryTmdb(libId: string) {
    try {
      await apiJson(`/libraries/${libId}/tmdb-sync`, { method: 'POST' });
      setOk('TMDB sync started');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to start TMDB sync');
    }
  }

  async function browseLibraryPath() {
    setPickingPath(true);
    try {
      const data = await apiJson<{ path: string }>('/system/pick-directory', {
        method: 'POST',
      });
      setNewLib((prev) => ({ ...prev, path: data.path }));
      setOk('Directory selected');
    } catch (err: any) {
      setErr(err.message || 'Failed to open directory picker');
    } finally {
      setPickingPath(false);
    }
  }

  function setLibraryEdit<K extends keyof LibraryEditState>(
    libraryId: string,
    key: K,
    value: LibraryEditState[K],
  ) {
    setLibraryEdits((prev) => ({
      ...prev,
      [libraryId]: {
        ...(prev[libraryId] || {
          name: '',
          path: '',
          show_images: true,
          prefer_local_artwork: true,
          fetch_online_artwork: true,
        }),
        [key]: value,
      },
    }));
  }

  async function browseExistingLibraryPath(libraryId: string) {
    setPickingPathForLibraryId(libraryId);
    try {
      const data = await apiJson<{ path: string }>('/system/pick-directory', {
        method: 'POST',
      });
      setLibraryEdit(libraryId, 'path', data.path);
      setOk('Directory selected');
    } catch (err: any) {
      setErr(err.message || 'Failed to open directory picker');
    } finally {
      setPickingPathForLibraryId(null);
    }
  }

  async function saveLibrary(libraryId: string) {
    const edit = libraryEdits[libraryId];
    if (!edit) return;
    if (!edit.path.trim()) {
      setErr('Library path is required');
      return;
    }
    try {
      await apiJson(`/libraries/${libraryId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: edit.name,
          paths: [edit.path],
          settings: {
            show_images: edit.show_images,
            prefer_local_artwork: edit.prefer_local_artwork,
            fetch_online_artwork: edit.fetch_online_artwork,
          },
        }),
      });
      const nextLibraries = libraries.map((library) => {
        if (library.id !== libraryId) return library;
        const nextPath = library.paths[0]
          ? [{ ...library.paths[0], path: edit.path }, ...library.paths.slice(1)]
          : [{ id: 'primary', path: edit.path, is_read_only: false }];
        return {
          ...library,
          name: edit.name,
          paths: nextPath,
          settings: {
            ...library.settings,
            show_images: edit.show_images,
            prefer_local_artwork: edit.prefer_local_artwork,
            fetch_online_artwork: edit.fetch_online_artwork,
          },
        };
      });
      librariesRef.current = nextLibraries;
      setLibraries(nextLibraries);
      setLibraryEdits((prev) => ({
        ...prev,
        [libraryId]: { ...edit },
      }));
      setOk('Library updated');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to update library');
    }
  }

  async function deleteLibrary(libId: string) {
    const target = libraries.find((l) => l.id === libId);
    const label = target ? `"${target.name}"` : 'this library';
    if (!window.confirm(`Delete ${label}? This removes all indexed items for it.`)) {
      return;
    }
    try {
      const res = await apiFetch(`/libraries/${libId}`, { method: 'DELETE' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || 'Delete failed');
      }
      setOk('Library deleted');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to delete library');
    }
  }

  function toggleIds(ids: string[], id: string): string[] {
    return ids.includes(id) ? ids.filter((v) => v !== id) : [...ids, id];
  }

  async function createUser(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/users', {
        method: 'POST',
        body: JSON.stringify({
          username: newUser.username,
          password: newUser.password,
          role: newUser.role,
          library_ids: newUser.role === 'user' ? newUser.library_ids : [],
        }),
      });
      setOk('User created');
      setNewUser({
        username: '',
        password: '',
        role: 'user',
        library_ids: [],
      });
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to create user');
    }
  }

  function updateEditRole(userId: string, role: 'admin' | 'user') {
    setUserEdits((prev) => ({
      ...prev,
      [userId]: {
        role,
        library_ids: role === 'admin' ? [] : prev[userId]?.library_ids || [],
      },
    }));
  }

  function toggleEditLibrary(userId: string, libraryId: string) {
    setUserEdits((prev) => {
      const current = prev[userId] || { role: 'user' as const, library_ids: [] };
      return {
        ...prev,
        [userId]: {
          ...current,
          library_ids: toggleIds(current.library_ids, libraryId),
        },
      };
    });
  }

  async function saveUserPermissions(userId: string) {
    const edit = userEdits[userId];
    if (!edit) return;
    try {
      await apiJson(`/users/${userId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          role: edit.role,
          library_ids: edit.role === 'user' ? edit.library_ids : [],
        }),
      });
      const savedRole = edit.role;
      const savedLibraryIds = edit.role === 'user' ? [...edit.library_ids] : [];
      const nextUsers = users.map((user) =>
        user.id === userId
          ? {
              ...user,
              role: savedRole,
              library_ids: [...savedLibraryIds],
            }
          : user,
      );
      usersRef.current = nextUsers;
      setUsers(nextUsers);
      setUserEdits((prev) => ({
        ...prev,
        [userId]: {
          role: savedRole,
          library_ids: [...savedLibraryIds],
        },
      }));
      setOk('User permissions updated');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to update permissions');
    }
  }

  async function deleteUser(userId: string) {
    try {
      const res = await apiFetch(`/users/${userId}`, { method: 'DELETE' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || 'Delete failed');
      }
      setOk('User deleted');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to delete user');
    }
  }

  async function createChannel(e: React.FormEvent) {
    e.preventDefault();
    try {
      await apiJson('/channels', {
        method: 'POST',
        body: JSON.stringify(newChannel),
      });
      setOk('Channel created');
      setNewChannel({
        name: '',
        kind: 'text',
        is_private: false,
      });
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to create channel');
    }
  }

  function setChannelEdit<K extends keyof ChannelEditState>(
    channelId: string,
    key: K,
    value: ChannelEditState[K],
  ) {
    setChannelEdits((prev) => ({
      ...prev,
      [channelId]: {
        ...(prev[channelId] || {
          name: '',
          is_private: false,
        }),
        [key]: value,
      },
    }));
  }

  async function saveChannel(channelId: string) {
    const edit = channelEdits[channelId];
    if (!edit) return;
    if (!edit.name.trim()) {
      setErr('Channel name is required');
      return;
    }
    try {
      await apiJson(`/channels/${channelId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: edit.name,
          is_private: edit.is_private,
        }),
      });
      const nextChannels = channels.map((channel) =>
        channel.id === channelId
          ? {
              ...channel,
              name: edit.name,
              is_private: edit.is_private,
            }
          : channel,
      );
      channelsRef.current = nextChannels;
      setChannels(nextChannels);
      setChannelEdits((prev) => ({
        ...prev,
        [channelId]: { ...edit },
      }));
      setOk('Channel updated');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to update channel');
    }
  }

  async function deleteChannel(channelId: string) {
    const target = channels.find((c) => c.id === channelId);
    const label = target ? `"${target.name}"` : 'this channel';
    if (!window.confirm(`Delete ${label}?`)) {
      return;
    }
    try {
      const res = await apiFetch(`/channels/${channelId}`, { method: 'DELETE' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || 'Delete failed');
      }
      setOk('Channel deleted');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to delete channel');
    }
  }

  function setRoomEdit<K extends keyof RoomEditState>(
    roomId: string,
    key: K,
    value: RoomEditState[K],
  ) {
    setRoomEdits((prev) => ({
      ...prev,
      [roomId]: {
        ...(prev[roomId] || {
          room_name: '',
        }),
        [key]: value,
      },
    }));
  }

  async function saveRoomName(roomId: string) {
    const edit = roomEdits[roomId];
    if (!edit) return;
    try {
      await apiJson(`/watch-party/admin/rooms/${roomId}/rename`, {
        method: 'PATCH',
        body: JSON.stringify({ room_name: edit.room_name }),
      });
      const nextRooms = rooms.map((room) =>
        room.room_id === roomId
          ? {
              ...room,
              room_name: edit.room_name,
            }
          : room,
      );
      roomsRef.current = nextRooms;
      setRooms(nextRooms);
      setRoomEdits((prev) => ({
        ...prev,
        [roomId]: { ...edit },
      }));
      setOk('Room renamed');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to rename room');
    }
  }

  async function endRoom(roomId: string) {
    const target = rooms.find((r) => r.room_id === roomId);
    const label = target?.title || roomId;
    if (!window.confirm(`End room "${label}"?`)) {
      return;
    }
    try {
      await apiJson(`/watch-party/admin/rooms/${roomId}/end`, {
        method: 'POST',
      });
      setOk('Room ended');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to end room');
    }
  }

  async function deleteRoom(roomId: string) {
    try {
      const res = await apiFetch(`/watch-party/admin/rooms/${roomId}`, {
        method: 'DELETE',
      });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || 'Delete failed');
      }
      setOk('Room deleted');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to delete room');
    }
  }

  async function saveTmdbKey(e: React.FormEvent) {
    e.preventDefault();
    setSavingTmdb(true);
    try {
      const updated = await apiJson<TmdbConfig>('/system/tmdb', {
        method: 'PUT',
        body: JSON.stringify({ api_key: tmdbApiKey }),
      });
      setTmdbConfig({
        configured: updated.configured,
        key_preview: updated.key_preview ?? null,
        source: updated.source ?? null,
      });
      setTmdbApiKey('');
      setOk(updated.configured ? 'TMDB key saved' : 'TMDB key cleared');
    } catch (err: any) {
      setErr(err.message || 'Failed to save TMDB key');
    } finally {
      setSavingTmdb(false);
    }
  }

  async function clearTmdbKey() {
    setSavingTmdb(true);
    try {
      const updated = await apiJson<TmdbConfig>('/system/tmdb', {
        method: 'PUT',
        body: JSON.stringify({ api_key: '' }),
      });
      setTmdbConfig({
        configured: updated.configured,
        key_preview: updated.key_preview ?? null,
        source: updated.source ?? null,
      });
      setTmdbApiKey('');
      setOk(updated.configured ? 'Using environment TMDB key' : 'TMDB key cleared');
    } catch (err: any) {
      setErr(err.message || 'Failed to clear TMDB key');
    } finally {
      setSavingTmdb(false);
    }
  }

  async function deleteJob(jobId: string) {
    try {
      const res = await apiFetch(`/jobs/${jobId}`, { method: 'DELETE' });
      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        throw new Error(body?.error?.message || 'Delete failed');
      }
      setOk('Log entry deleted');
      await loadData();
    } catch (err: any) {
      setErr(err.message || 'Failed to delete log entry');
    }
  }

  if (authLoading) {
    return (
      <div className="panel-soft px-5 py-4">
        <p className="text-sm muted">Checking access…</p>
      </div>
    );
  }

  if (!me || me.role !== 'admin') {
    return (
      <div className="panel px-6 py-8">
        <p className="text-sm muted">Admin access required.</p>
      </div>
    );
  }

  const adminUsers = users.filter((u) => u.role === 'admin');
  const regularUsers = users.filter((u) => u.role !== 'admin');

  return (
    <div className="space-y-8 animate-rise">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold sm:text-4xl">Admin Dashboard</h1>
      </header>

      {msg && (
        <p className={`${msgType === 'ok' ? 'notice-ok' : 'notice-error'} rounded-xl px-4 py-2 text-sm`}>
          {msg}
        </p>
      )}

      <div className="flex flex-wrap gap-2 border-b border-[var(--border)] pb-0">
        {ADMIN_TABS.map((tab) => (
          <button
            key={tab.key}
            type="button"
            onClick={() => setActiveTab(tab.key)}
            className={`px-5 py-2.5 text-sm font-medium rounded-t-lg transition-colors ${
              activeTab === tab.key
                ? 'bg-[var(--surface)] border border-b-0 border-[var(--border)]'
                : 'opacity-60 hover:opacity-100 hover:bg-[var(--surface)] hover:bg-opacity-50 hover:border hover:border-b-0 hover:border-[var(--border)] hover:border-opacity-50'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {activeTab === 'users' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create User</h2>
            <form onSubmit={createUser} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                <input
                  placeholder="Username"
                  value={newUser.username}
                  onChange={(e) => setNewUser({ ...newUser, username: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <input
                  type="password"
                  placeholder="Password (min 6 chars)"
                  minLength={6}
                  value={newUser.password}
                  onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="New user role"
                  value={newUser.role}
                  onChange={(e) =>
                    setNewUser({
                      ...newUser,
                      role: e.target.value as 'admin' | 'user',
                      library_ids: e.target.value === 'admin' ? [] : newUser.library_ids,
                    })
                  }
                  className="select px-3 py-2 text-sm"
                >
                  <option value="user">User</option>
                  <option value="admin">Admin</option>
                </select>
              </div>

              {newUser.role === 'user' && (
                <div className="space-y-2">
                  <p className="text-sm font-medium">Allowed Libraries</p>
                  {libraries.length === 0 ? (
                    <p className="text-xs muted">
                      No libraries configured yet. You can create this user now and assign access later.
                    </p>
                  ) : (
                    <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
                      {libraries.map((lib) => (
                        <label key={lib.id} className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <input
                            type="checkbox"
                            checked={newUser.library_ids.includes(lib.id)}
                            onChange={() =>
                              setNewUser({
                                ...newUser,
                                library_ids: toggleIds(newUser.library_ids, lib.id),
                              })
                            }
                            className="h-4 w-4 [accent-color:var(--purple)]"
                          />
                          <span>{lib.name}</span>
                        </label>
                      ))}
                    </div>
                  )}
                </div>
              )}

              <button type="submit" className="btn-primary px-4 py-2 text-sm">
                Create User
              </button>
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Users</h2>
            {users.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No users found.</p>
              </div>
            ) : (
              <div className="space-y-4">
                {adminUsers.length > 0 && (
                  <div className="space-y-3">
                    <p className="text-sm font-medium muted">Admin Accounts</p>
                    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                      {adminUsers.map((user) => {
                        const edit = userEdits[user.id] || {
                          role: user.role,
                          library_ids: user.library_ids || [],
                        };
                        return (
                          <div key={user.id} className="tile space-y-3 p-4">
                            <div>
                              <p className="font-medium">{user.username}</p>
                              <p className="text-xs muted">
                                {new Date(user.created_ts * 1000).toLocaleString()}
                              </p>
                            </div>
                            <select
                              aria-label={`Role for ${user.username}`}
                              value={edit.role}
                              onChange={(e) => updateEditRole(user.id, e.target.value as 'admin' | 'user')}
                              className="select w-full px-2 py-1.5 text-sm"
                            >
                              <option value="user">User</option>
                              <option value="admin">Admin</option>
                            </select>
                            <div className="flex gap-2 pt-1">
                              <button
                                onClick={() => saveUserPermissions(user.id)}
                                className="btn-primary flex-1 px-3 py-1.5 text-sm"
                              >
                                Save
                              </button>
                              <button
                                onClick={() => deleteUser(user.id)}
                                className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}

                {adminUsers.length > 0 && regularUsers.length > 0 && (
                  <div className="border-t border-[var(--border)]" />
                )}

                {regularUsers.length > 0 && (
                  <div className="space-y-3">
                    <p className="text-sm font-medium muted">User Accounts</p>
                    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                      {regularUsers.map((user) => {
                        const edit = userEdits[user.id] || {
                          role: user.role,
                          library_ids: user.library_ids || [],
                        };
                        return (
                          <div key={user.id} className="tile space-y-3 p-4">
                            <div>
                              <p className="font-medium">{user.username}</p>
                              <p className="text-xs muted">
                                {new Date(user.created_ts * 1000).toLocaleString()}
                              </p>
                            </div>
                            <select
                              aria-label={`Role for ${user.username}`}
                              value={edit.role}
                              onChange={(e) => updateEditRole(user.id, e.target.value as 'admin' | 'user')}
                              className="select w-full px-2 py-1.5 text-sm"
                            >
                              <option value="user">User</option>
                              <option value="admin">Admin</option>
                            </select>
                            {edit.role === 'user' && libraries.length > 0 && (
                              <div className="space-y-1.5">
                                <p className="text-xs uppercase tracking-[0.18em] muted">Libraries</p>
                                <div className="space-y-1">
                                  {libraries.map((lib) => (
                                    <label
                                      key={lib.id}
                                      className="panel-soft flex items-center gap-2 px-3 py-2 text-sm"
                                    >
                                      <input
                                        type="checkbox"
                                        checked={edit.library_ids.includes(lib.id)}
                                        onChange={() => toggleEditLibrary(user.id, lib.id)}
                                        className="h-4 w-4 [accent-color:var(--purple)]"
                                      />
                                      <span>{lib.name}</span>
                                    </label>
                                  ))}
                                </div>
                              </div>
                            )}
                            <div className="flex gap-2 pt-1">
                              <button
                                onClick={() => saveUserPermissions(user.id)}
                                className="btn-primary flex-1 px-3 py-1.5 text-sm"
                              >
                                Save
                              </button>
                              <button
                                onClick={() => deleteUser(user.id)}
                                className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'libraries' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create Library</h2>
            <form onSubmit={createLibrary} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.1fr_0.9fr_2fr_auto_auto]">
                <input
                  placeholder="Name"
                  value={newLib.name}
                  onChange={(e) => setNewLib({ ...newLib, name: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="Library type"
                  value={newLib.kind}
                  onChange={(e) => setNewLib({ ...newLib, kind: e.target.value })}
                  className="select px-3 py-2 text-sm"
                >
                  <option value="movies">Movies</option>
                  <option value="tv_shows">TV Shows</option>
                  <option value="music">Music</option>
                </select>
                <input
                  placeholder="/path/to/media"
                  value={newLib.path}
                  onChange={(e) => setNewLib({ ...newLib, path: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <button
                  type="button"
                  onClick={browseLibraryPath}
                  disabled={pickingPath}
                  className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
                >
                  {pickingPath ? 'Opening...' : 'Browse'}
                </button>
                <button type="submit" className="btn-primary px-4 py-2 text-sm">
                  Create
                </button>
              </div>

              <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
                <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                  <input
                    type="checkbox"
                    checked={newLib.show_images}
                    onChange={(e) => setNewLib({ ...newLib, show_images: e.target.checked })}
                    className="h-4 w-4 [accent-color:var(--purple)]"
                  />
                  <span>Enable artwork thumbnails</span>
                </label>
                <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                  <input
                    type="checkbox"
                    checked={newLib.prefer_local_artwork}
                    onChange={(e) =>
                      setNewLib({ ...newLib, prefer_local_artwork: e.target.checked })
                    }
                    className="h-4 w-4 [accent-color:var(--purple)]"
                  />
                  <span>Prefer local artwork files</span>
                </label>
                <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                  <input
                    type="checkbox"
                    checked={newLib.fetch_online_artwork}
                    onChange={(e) =>
                      setNewLib({ ...newLib, fetch_online_artwork: e.target.checked })
                    }
                    className="h-4 w-4 [accent-color:var(--purple)]"
                  />
                  <span>Fetch missing artwork online</span>
                </label>
              </div>
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Libraries</h2>
            {libraries.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No libraries configured.</p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
                {libraries.map((lib) => (
                  <div key={lib.id} className="tile space-y-3 p-4">
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        void saveLibrary(lib.id);
                      }}
                      className="space-y-2"
                    >
                      <input
                        aria-label={`Library name ${lib.name}`}
                        value={libraryEdits[lib.id]?.name ?? lib.name}
                        onChange={(e) => setLibraryEdit(lib.id, 'name', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                      />
                      <input
                        aria-label={`Library path ${lib.name}`}
                        value={libraryEdits[lib.id]?.path ?? lib.paths[0]?.path ?? ''}
                        onChange={(e) => setLibraryEdit(lib.id, 'path', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                      />
                      <div className="flex gap-2">
                        <button
                          type="button"
                          onClick={() => browseExistingLibraryPath(lib.id)}
                          disabled={pickingPathForLibraryId === lib.id}
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm disabled:opacity-50"
                        >
                          {pickingPathForLibraryId === lib.id ? 'Opening...' : 'Browse'}
                        </button>
                        <button
                          type="button"
                          onClick={() => scanLibrary(lib.id)}
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm"
                        >
                          Scan
                        </button>
                        <button
                          type="button"
                          onClick={() => syncLibraryTmdb(lib.id)}
                          className="btn-secondary flex-1 px-3 py-1.5 text-sm"
                        >
                          TMDB Sync
                        </button>
                        <button type="submit" className="btn-primary flex-1 px-3 py-1.5 text-sm">
                          Save
                        </button>
                      </div>
                      <div className="space-y-1">
                        <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <input
                            type="checkbox"
                            checked={libraryEdits[lib.id]?.show_images ?? lib.settings.show_images}
                            onChange={(e) =>
                              setLibraryEdit(lib.id, 'show_images', e.target.checked)
                            }
                            className="h-4 w-4 [accent-color:var(--purple)]"
                          />
                          <span>Enable artwork thumbnails</span>
                        </label>
                        <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <input
                            type="checkbox"
                            checked={
                              libraryEdits[lib.id]?.prefer_local_artwork ??
                              lib.settings.prefer_local_artwork
                            }
                            onChange={(e) =>
                              setLibraryEdit(lib.id, 'prefer_local_artwork', e.target.checked)
                            }
                            className="h-4 w-4 [accent-color:var(--purple)]"
                          />
                          <span>Prefer local artwork files</span>
                        </label>
                        <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <input
                            type="checkbox"
                            checked={
                              libraryEdits[lib.id]?.fetch_online_artwork ??
                              lib.settings.fetch_online_artwork
                            }
                            onChange={(e) =>
                              setLibraryEdit(lib.id, 'fetch_online_artwork', e.target.checked)
                            }
                            className="h-4 w-4 [accent-color:var(--purple)]"
                          />
                          <span>Fetch missing artwork online</span>
                        </label>
                      </div>
                    </form>
                    <div className="flex items-center justify-between gap-3 border-t border-[var(--border)] pt-3">
                      <p className="text-sm muted">
                        {lib.kind} · {lib.item_count} items
                      </p>
                      <button
                        onClick={() => deleteLibrary(lib.id)}
                        className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'channels' && (
        <div className="space-y-8">
          <section className="panel space-y-4 p-6">
            <h2 className="text-xl font-semibold">Create Channel</h2>
            <form onSubmit={createChannel} className="space-y-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.4fr_1fr_auto]">
                <input
                  placeholder="Channel name"
                  value={newChannel.name}
                  onChange={(e) => setNewChannel({ ...newChannel, name: e.target.value })}
                  className="input px-3 py-2 text-sm"
                  required
                />
                <select
                  aria-label="Channel kind"
                  value={newChannel.kind}
                  onChange={(e) =>
                    setNewChannel({ ...newChannel, kind: e.target.value as 'text' | 'voice' })
                  }
                  className="select px-3 py-2 text-sm"
                >
                  <option value="text">Text</option>
                  <option value="voice">Voice</option>
                </select>
                <button type="submit" className="btn-primary px-4 py-2 text-sm">
                  Create
                </button>
              </div>
              <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm max-w-sm">
                <input
                  type="checkbox"
                  checked={newChannel.is_private}
                  onChange={(e) => setNewChannel({ ...newChannel, is_private: e.target.checked })}
                  className="h-4 w-4 [accent-color:var(--purple)]"
                />
                <span>Private channel (admins only)</span>
              </label>
            </form>
          </section>

          <section className="space-y-4">
            <h2 className="text-xl font-semibold">Manage Channels</h2>
            {channels.length === 0 ? (
              <div className="panel-soft px-4 py-3">
                <p className="text-sm muted">No channels available.</p>
              </div>
            ) : (
              <div className="space-y-3">
                {channels.map((channel) => {
                  const edit = channelEdits[channel.id] || toChannelEditState(channel);
                  const creatorName = usersById.get(channel.created_by)?.username || channel.created_by;
                  return (
                    <div key={channel.id} className="tile space-y-3 p-4">
                      <div className="flex flex-wrap items-start justify-between gap-2">
                        <div className="flex items-center gap-2">
                          <span className="chip">{channel.kind}</span>
                          <span className="chip">#{channel.position}</span>
                          <span className="chip">{edit.is_private ? 'Admins only' : 'All users'}</span>
                        </div>
                        <p className="text-xs muted">
                          Created by {creatorName} · {new Date(channel.created_ts * 1000).toLocaleString()}
                        </p>
                      </div>
                      <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.4fr_auto_auto]">
                        <input
                          aria-label={`Channel name ${channel.name}`}
                          value={edit.name}
                          onChange={(e) => setChannelEdit(channel.id, 'name', e.target.value)}
                          className="input w-full px-3 py-2 text-sm"
                        />
                        <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                          <input
                            type="checkbox"
                            checked={edit.is_private}
                            onChange={(e) =>
                              setChannelEdit(channel.id, 'is_private', e.target.checked)
                            }
                            className="h-4 w-4 [accent-color:var(--purple)]"
                          />
                          <span>Private</span>
                        </label>
                        <div className="flex gap-2">
                          <button
                            onClick={() => saveChannel(channel.id)}
                            className="btn-primary flex-1 px-3 py-1.5 text-sm"
                          >
                            Save
                          </button>
                          <button
                            onClick={() => deleteChannel(channel.id)}
                            className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                          >
                            Delete
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        </div>
      )}

      {activeTab === 'rooms' && (
        <div className="space-y-4">
          <h2 className="text-xl font-semibold">Manage Rooms</h2>
          {rooms.length === 0 ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              No rooms available.
            </div>
          ) : (
            <div className="space-y-3">
              {rooms.map((room) => {
                const edit = roomEdits[room.room_id] || toRoomEditState(room);
                return (
                  <div key={room.room_id} className="tile space-y-3 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <p className="font-semibold">{room.title}</p>
                      <div className="flex items-center gap-2">
                        <span className="chip">{room.room_mode}</span>
                        <span className="chip">{room.status}</span>
                        <span className="chip">{room.invite_only ? 'Private' : 'Public'}</span>
                      </div>
                    </div>
                    <p className="text-xs muted">
                      Host: {room.host_username} · Members: {room.member_count}
                      {room.password_required ? ' · Password protected' : ''}
                    </p>
                    <p className="text-xs muted">
                      Created {new Date(room.created_ts * 1000).toLocaleString()}
                    </p>
                    <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_auto]">
                      <input
                        aria-label={`Room name ${room.room_id}`}
                        value={edit.room_name}
                        onChange={(e) => setRoomEdit(room.room_id, 'room_name', e.target.value)}
                        className="input w-full px-3 py-2 text-sm"
                        placeholder="Room name"
                      />
                      <div className="flex flex-wrap gap-2">
                        <button
                          onClick={() => saveRoomName(room.room_id)}
                          className="btn-primary px-3 py-1.5 text-sm"
                        >
                          Save Name
                        </button>
                        <button
                          onClick={() => endRoom(room.room_id)}
                          disabled={room.status === 'ended'}
                          className="btn-secondary px-3 py-1.5 text-sm disabled:opacity-50"
                        >
                          End
                        </button>
                        <button
                          onClick={() => setPendingDeleteRoom(room)}
                          className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)]"
                        >
                          Delete
                        </button>
                      </div>
                    </div>
                    <p className="text-xs muted break-all">Room ID: {room.room_id}</p>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {activeTab === 'logs' && (
        <section className="space-y-3">
          <h2 className="text-xl font-semibold">Logs</h2>
          {jobs.length === 0 ? (
            <p className="text-sm muted">No logs</p>
          ) : (
            jobs.map((job) => {
              const isTerminal = !['queued', 'running'].includes(job.status);
              return (
                <div key={job.id} className="tile space-y-2 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-sm font-medium">{job.kind}</p>
                      <p className="text-xs muted">
                        {new Date(job.created_ts * 1000).toLocaleString()}
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="chip">{job.status}</span>
                      {isTerminal && (
                        <button
                          onClick={() => deleteJob(job.id)}
                          className="btn-ghost px-2 py-1 text-xs text-[var(--danger)]"
                          title="Delete log"
                        >
                          Delete
                        </button>
                      )}
                    </div>
                  </div>
                  <p className="text-xs muted">{Math.round(job.progress * 100)}%</p>
                  <div className="h-2 overflow-hidden rounded-full bg-white/8">
                    <div
                      className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
                      style={{
                        width: `${Math.max(0, Math.min(100, Math.round(job.progress * 100)))}%`,
                      }}
                    />
                  </div>
                  {job.payload && (
                    <pre className="max-h-40 overflow-auto rounded-lg bg-black/20 px-2 py-1 text-xs muted">
                      {JSON.stringify(job.payload, null, 2)}
                    </pre>
                  )}
                  {job.error && (
                    <p className="text-xs text-red-300">{job.error}</p>
                  )}
                </div>
              );
            })
          )}
        </section>
      )}

      {activeTab === 'tmdb' && (
        <section className="panel space-y-4 p-6">
          <h2 className="text-xl font-semibold">TMDB Metadata</h2>
          <p className="text-sm muted">
            Set a TMDB API key so scans can fetch posters and metadata for detected movies/shows.
          </p>
          <form onSubmit={saveTmdbKey} className="space-y-3">
            <input
              type="password"
              value={tmdbApiKey}
              onChange={(e) => setTmdbApiKey(e.target.value)}
              placeholder="Enter TMDB API key (leave empty to clear)"
              className="input w-full px-3 py-2 text-sm"
            />
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="submit"
                disabled={savingTmdb}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                {savingTmdb ? 'Saving...' : 'Save TMDB Key'}
              </button>
              <button
                type="button"
                onClick={clearTmdbKey}
                disabled={savingTmdb}
                className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
              >
                Clear Stored Key
              </button>
            </div>
          </form>
          <p className="text-xs muted">
            Status:{' '}
            {tmdbConfig.configured
              ? `configured (${tmdbConfig.source || 'unknown'}${
                  tmdbConfig.key_preview ? `, ${tmdbConfig.key_preview}` : ''
                })`
              : 'not configured'}
          </p>
        </section>
      )}

      {pendingDeleteRoom && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 backdrop-blur-[2px] p-4">
          <div className="panel rounded-2xl p-6 w-full max-w-sm space-y-4 border border-[var(--border)]">
            <h2 className="font-semibold text-lg">Delete Room</h2>
            <p className="text-sm muted">
              Delete &ldquo;{pendingDeleteRoom.title}&rdquo;? This removes the room and its history and cannot be undone.
            </p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setPendingDeleteRoom(null)}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  void deleteRoom(pendingDeleteRoom.room_id);
                  setPendingDeleteRoom(null);
                }}
                className="btn-primary px-4 py-2 text-sm bg-red-500 hover:bg-red-600"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
