'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
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

interface TmdbConfig {
  configured: boolean;
  key_preview: string | null;
  source: 'database' | 'environment' | null;
}

export default function AdminPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [libraries, setLibraries] = useState<Library[]>([]);
  const [libraryEdits, setLibraryEdits] = useState<Record<string, LibraryEditState>>({});
  const [jobs, setJobs] = useState<Job[]>([]);
  const [users, setUsers] = useState<UserAccount[]>([]);
  const [userEdits, setUserEdits] = useState<Record<string, UserEditState>>({});

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

  useEffect(() => {
    if (!authLoading && (!me || me.role !== 'admin')) {
      router.replace('/libraries');
    }
  }, [authLoading, me, router]);

  const loadData = useCallback(async () => {
    try {
      const [libs, jobList, userList, tmdb] = await Promise.all([
        apiJson<Library[]>('/libraries'),
        apiJson<Job[]>('/jobs'),
        apiJson<UserAccount[]>('/users'),
        apiJson<TmdbConfig>('/system/tmdb'),
      ]);
      setLibraries(libs);
      const nextLibEdits: Record<string, LibraryEditState> = {};
      for (const lib of libs) {
        nextLibEdits[lib.id] = {
          name: lib.name,
          path: lib.paths[0]?.path || '',
          show_images: lib.settings?.show_images ?? true,
          prefer_local_artwork: lib.settings?.prefer_local_artwork ?? true,
          fetch_online_artwork: lib.settings?.fetch_online_artwork ?? true,
        };
      }
      setLibraryEdits(nextLibEdits);
      setJobs(jobList);
      setUsers(userList);

      const nextEdits: Record<string, UserEditState> = {};
      for (const user of userList) {
        nextEdits[user.id] = {
          role: user.role,
          library_ids: [...(user.library_ids || [])],
        };
      }
      setUserEdits(nextEdits);
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
    if (newUser.role === 'user' && newUser.library_ids.length === 0) {
      setErr('Simple users must have at least one library assigned');
      return;
    }
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
    if (edit.role === 'user' && edit.library_ids.length === 0) {
      setErr('Simple users must have at least one library assigned');
      return;
    }
    try {
      await apiJson(`/users/${userId}`, {
        method: 'PATCH',
        body: JSON.stringify({
          role: edit.role,
          library_ids: edit.role === 'user' ? edit.library_ids : [],
        }),
      });
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

  return (
    <>
      {authLoading && (
        <div className="panel-soft px-5 py-4"><p className="text-sm muted">Checking access…</p></div>
      )}
      {!authLoading && (!me || me.role !== 'admin') && (
        <div className="panel px-6 py-8"><p className="text-sm muted">Admin access required.</p></div>
      )}
      {!authLoading && me?.role === 'admin' && (
    <div className="space-y-8 animate-rise">
      <header className="space-y-2">
        <span className="chip">Server Controls</span>
        <h1 className="text-3xl font-semibold sm:text-4xl">Admin Dashboard</h1>
        <p className="text-sm muted sm:text-base">
          Configure media sources, manage user permissions, and monitor active jobs.
        </p>
      </header>

      {msg && (
        <p className={`${msgType === 'ok' ? 'notice-ok' : 'notice-error'} rounded-xl px-4 py-2 text-sm`}>
          {msg}
        </p>
      )}

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
            ? `configured (${tmdbConfig.source || 'unknown'}${tmdbConfig.key_preview ? `, ${tmdbConfig.key_preview}` : ''})`
            : 'not configured'}
        </p>
      </section>

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
            <button
              type="submit"
              className="btn-primary px-4 py-2 text-sm"
            >
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
              placeholder="Password (min 12 chars)"
              minLength={12}
              value={newUser.password}
              onChange={(e) => setNewUser({ ...newUser, password: e.target.value })}
              className="input px-3 py-2 text-sm"
              required
            />
            <select
              aria-label="New user role"
              value={newUser.role}
              onChange={(e) => setNewUser({
                ...newUser,
                role: e.target.value as 'admin' | 'user',
                library_ids: e.target.value === 'admin' ? [] : newUser.library_ids,
              })}
              className="select px-3 py-2 text-sm"
            >
              <option value="user">Simple User</option>
              <option value="admin">Admin</option>
            </select>
          </div>

          {newUser.role === 'user' && (
            <div className="space-y-2">
              <p className="text-sm font-medium">Allowed Libraries</p>
              {libraries.length === 0 ? (
                <p className="text-xs muted">Create at least one library before creating simple users.</p>
              ) : (
                <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
                  {libraries.map((lib) => (
                    <label key={lib.id} className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                      <input
                        type="checkbox"
                        checked={newUser.library_ids.includes(lib.id)}
                        onChange={() => setNewUser({
                          ...newUser,
                          library_ids: toggleIds(newUser.library_ids, lib.id),
                        })}
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

      <section className="space-y-3">
        <h2 className="text-xl font-semibold">User Permissions</h2>
        {users.length === 0 ? (
          <div className="panel-soft px-4 py-3">
            <p className="text-sm muted">No users found.</p>
          </div>
        ) : (
          users.map((user) => {
            const edit = userEdits[user.id] || { role: user.role, library_ids: user.library_ids || [] };
            return (
              <div key={user.id} className="tile space-y-4 p-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p className="font-medium">{user.username}</p>
                    <p className="text-xs muted">{new Date(user.created_ts * 1000).toLocaleString()}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <select
                      aria-label={`Role for ${user.username}`}
                      value={edit.role}
                      onChange={(e) => updateEditRole(user.id, e.target.value as 'admin' | 'user')}
                      className="select px-2 py-1.5 text-sm"
                    >
                      <option value="user">Simple User</option>
                      <option value="admin">Admin</option>
                    </select>
                    <button
                      onClick={() => saveUserPermissions(user.id)}
                      className="btn-secondary px-3 py-1.5 text-sm"
                    >
                      Save
                    </button>
                    <button
                      onClick={() => deleteUser(user.id)}
                      className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)] hover:text-[var(--danger)]"
                    >
                      Delete
                    </button>
                  </div>
                </div>

                {edit.role === 'user' && (
                  <div className="space-y-2">
                    <p className="text-xs uppercase tracking-[0.18em] muted">Allowed Libraries</p>
                    {libraries.length === 0 ? (
                      <p className="text-xs muted">No libraries available to assign.</p>
                    ) : (
                      <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
                        {libraries.map((lib) => (
                          <label key={lib.id} className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
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
                    )}
                  </div>
                )}
              </div>
            );
          })
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Libraries</h2>
        {libraries.length === 0 ? (
          <div className="panel-soft px-4 py-3">
            <p className="text-sm muted">No libraries configured.</p>
          </div>
        ) : (
          libraries.map((lib) => (
            <div key={lib.id} className="tile space-y-4 p-4">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[1.1fr_2fr_auto_auto_auto]">
                <input
                  aria-label={`Library name ${lib.name}`}
                  value={libraryEdits[lib.id]?.name ?? lib.name}
                  onChange={(e) => setLibraryEdit(lib.id, 'name', e.target.value)}
                  className="input px-3 py-2 text-sm"
                />
                <input
                  aria-label={`Library path ${lib.name}`}
                  value={libraryEdits[lib.id]?.path ?? lib.paths[0]?.path ?? ''}
                  onChange={(e) => setLibraryEdit(lib.id, 'path', e.target.value)}
                  className="input px-3 py-2 text-sm"
                />
                <button
                  onClick={() => browseExistingLibraryPath(lib.id)}
                  disabled={pickingPathForLibraryId === lib.id}
                  className="btn-secondary px-3 py-1.5 text-sm disabled:opacity-50"
                >
                  {pickingPathForLibraryId === lib.id ? 'Opening...' : 'Browse'}
                </button>
                <button
                  onClick={() => scanLibrary(lib.id)}
                  className="btn-secondary px-3 py-1.5 text-sm"
                >
                  Scan
                </button>
                <button
                  onClick={() => saveLibrary(lib.id)}
                  className="btn-primary px-3 py-1.5 text-sm"
                >
                  Save
                </button>
              </div>

              <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
                <label className="panel-soft flex items-center gap-2 px-3 py-2 text-sm">
                  <input
                    type="checkbox"
                    checked={libraryEdits[lib.id]?.show_images ?? lib.settings.show_images}
                    onChange={(e) => setLibraryEdit(lib.id, 'show_images', e.target.checked)}
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

              <div className="flex items-center justify-between gap-3">
                <p className="text-sm muted">{lib.kind} · {lib.item_count} items</p>
                <button
                  onClick={() => deleteLibrary(lib.id)}
                  className="btn-ghost px-3 py-1.5 text-sm text-[var(--danger)] hover:text-[var(--danger)]"
                >
                  Delete Library
                </button>
              </div>
            </div>
          ))
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-xl font-semibold">Jobs</h2>
        {jobs.length === 0 ? (
          <p className="text-sm muted">No active jobs</p>
        ) : (
          jobs.map((job) => (
            <div
              key={job.id}
              className="tile space-y-2 p-3"
            >
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-medium">{job.kind}</p>
                <span className="chip">{job.status}</span>
              </div>
              <p className="text-xs muted">{Math.round(job.progress * 100)}%</p>
              <div className="h-2 overflow-hidden rounded-full bg-white/8">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-[var(--orange)] to-[var(--purple)]"
                  style={{ width: `${Math.max(0, Math.min(100, Math.round(job.progress * 100)))}%` }}
                />
              </div>
            </div>
          ))
        )}
      </section>
    </div>
      )}
    </>
  );
}
