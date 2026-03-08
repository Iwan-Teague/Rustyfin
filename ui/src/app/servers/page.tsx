'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import {
  MinecraftServerAction,
  MinecraftServer,
  MinecraftServerActionResponse,
  MinecraftServerEvent,
  MinecraftServerOperationResponse,
  createMinecraftServer,
  HostDirectoryListEntry,
  listMinecraftServerEvents,
  listBackendHostDirectories,
  listMinecraftServers,
  importMinecraftServer,
  provisionMinecraftServer,
  refreshMinecraftServerStatus,
  requestMinecraftServerAction,
} from '@/lib/serversApi';

type CreateFormState = {
  display_name: string;
  description: string;
  server_distribution: 'vanilla' | 'paper';
  minecraft_version: string;
  world_name: string;
  listen_port: string;
  gamemode: 'survival' | 'creative' | 'adventure' | 'spectator';
  difficulty: 'peaceful' | 'easy' | 'normal' | 'hard';
  hardcore: boolean;
  motd: string;
  max_player_count: string;
  min_memory_mb: string;
  max_memory_mb: string;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
  autostart: boolean;
  eula_accepted: boolean;
};

type HostDirectoryBrowserState = {
  open: boolean;
  loading: boolean;
  error: string;
  currentPath: string;
  parentPath: string | null;
  roots: string[];
  directories: HostDirectoryListEntry[];
};

const DEFAULT_FORM: CreateFormState = {
  display_name: '',
  description: '',
  server_distribution: 'paper',
  minecraft_version: '1.21.1',
  world_name: '',
  listen_port: '25565',
  gamemode: 'survival',
  difficulty: 'normal',
  hardcore: false,
  motd: '',
  max_player_count: '20',
  min_memory_mb: '1024',
  max_memory_mb: '4096',
  online_mode: true,
  pvp: true,
  allow_flight: false,
  enable_command_block: false,
  white_list_enabled: false,
  autostart: false,
  eula_accepted: false,
};

const TOGGLE_FIELDS: Array<{ key: keyof Pick<
  CreateFormState,
  | 'online_mode'
  | 'pvp'
  | 'allow_flight'
  | 'enable_command_block'
  | 'white_list_enabled'
  | 'autostart'
  | 'hardcore'
>; label: string }> = [
  { key: 'online_mode', label: 'Online mode' },
  { key: 'pvp', label: 'PVP enabled' },
  { key: 'allow_flight', label: 'Allow flight' },
  { key: 'enable_command_block', label: 'Enable command blocks' },
  { key: 'white_list_enabled', label: 'Enable whitelist' },
  { key: 'autostart', label: 'Autostart on host boot' },
  { key: 'hardcore', label: 'Hardcore mode' },
];

function formatTs(ts?: number | null) {
  if (!ts) return 'Never';
  return new Date(ts * 1000).toLocaleString();
}

function titleCase(value: string) {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

export default function ServersPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [servers, setServers] = useState<MinecraftServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const [selectedEvents, setSelectedEvents] = useState<MinecraftServerEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [statusRefreshing, setStatusRefreshing] = useState(false);
  const [actionLoading, setActionLoading] = useState<MinecraftServerAction | null>(null);
  const [provisioning, setProvisioning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState('');
  const [successMessage, setSuccessMessage] = useState('');
  const [form, setForm] = useState<CreateFormState>(DEFAULT_FORM);
  const [importSourcePath, setImportSourcePath] = useState('');
  const [hostBrowser, setHostBrowser] = useState<HostDirectoryBrowserState>({
    open: false,
    loading: false,
    error: '',
    currentPath: '',
    parentPath: null,
    roots: [],
    directories: [],
  });

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
        const rows = await listMinecraftServers();
        if (cancelled) return;
        setServers(rows);
        setSelectedServerId((current) => {
          if (current && rows.some((row) => row.id === current)) {
            return current;
          }
          return rows[0]?.id ?? null;
        });
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to load servers'));
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
    if (!me || !selectedServerId) {
      setSelectedEvents([]);
      return;
    }

    let cancelled = false;
    void (async () => {
      try {
        await Promise.all([
          refreshSelectedServerStatus(selectedServerId, true),
          loadSelectedServerEvents(selectedServerId, true),
        ]);
      } catch (err: unknown) {
        if (!cancelled) {
          setError(clientErrorMessage(err, 'Failed to refresh selected server status'));
        }
      }
    })();

    const interval = window.setInterval(() => {
      if (cancelled) return;
      void refreshSelectedServerStatus(selectedServerId, false);
      void loadSelectedServerEvents(selectedServerId, false);
    }, 5000);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [me, selectedServerId]);

  const selectedServer = useMemo(
    () => servers.find((server) => server.id === selectedServerId) ?? null,
    [servers, selectedServerId],
  );

  function updateForm<K extends keyof CreateFormState>(key: K, value: CreateFormState[K]) {
    setForm((prev) => ({ ...prev, [key]: value }));
  }

  function upsertServer(updated: MinecraftServer) {
    setServers((prev) => {
      const existingIndex = prev.findIndex((server) => server.id === updated.id);
      if (existingIndex === -1) {
        return [updated, ...prev];
      }
      const next = [...prev];
      next[existingIndex] = updated;
      return next;
    });
  }

  function closeHostDirectoryBrowser() {
    setHostBrowser((prev) => ({
      ...prev,
      open: false,
      loading: false,
      error: '',
    }));
  }

  async function fetchHostDirectories(path?: string) {
    const listing = await listBackendHostDirectories(path);
    setHostBrowser((prev) => ({
      ...prev,
      loading: false,
      error: '',
      currentPath: listing.current_path,
      parentPath: listing.parent_path ?? null,
      roots: listing.roots,
      directories: listing.directories,
    }));
  }

  function openHostDirectoryBrowser(initialPath?: string) {
    setHostBrowser((prev) => ({
      ...prev,
      open: true,
      loading: true,
      error: '',
    }));
    void fetchHostDirectories(initialPath)
      .catch((err: unknown) => {
        setHostBrowser((prev) => ({
          ...prev,
          loading: false,
          error: clientErrorMessage(err, 'Failed to browse backend directories'),
        }));
      });
  }

  function navigateHostDirectory(path?: string | null) {
    const target = path?.trim();
    if (!target) return;
    setHostBrowser((prev) => ({ ...prev, loading: true, error: '' }));
    void fetchHostDirectories(target).catch((err: unknown) => {
      setHostBrowser((prev) => ({
        ...prev,
        loading: false,
        error: clientErrorMessage(err, 'Failed to browse backend directories'),
      }));
    });
  }

  async function refreshServers(selectId?: string) {
    const rows = await listMinecraftServers();
    setServers(rows);
    setSelectedServerId((current) => {
      if (selectId && rows.some((row) => row.id === selectId)) return selectId;
      if (current && rows.some((row) => row.id === current)) return current;
      return rows[0]?.id ?? null;
    });
  }

  async function loadSelectedServerEvents(serverId: string, showSpinner = true) {
    if (showSpinner) {
      setEventsLoading(true);
    }
    try {
      const rows = await listMinecraftServerEvents(serverId, 20);
      setSelectedEvents(rows);
    } catch {
      setSelectedEvents([]);
    } finally {
      if (showSpinner) {
        setEventsLoading(false);
      }
    }
  }

  async function refreshSelectedServerStatus(serverId: string, showSpinner = true) {
    if (showSpinner) {
      setStatusRefreshing(true);
    }
    try {
      const updated = await refreshMinecraftServerStatus(serverId);
      upsertServer(updated);
      return updated;
    } finally {
      if (showSpinner) {
        setStatusRefreshing(false);
      }
    }
  }

  async function handleRequestAction(action: MinecraftServerAction) {
    if (!selectedServer) return;
    setActionLoading(action);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerActionResponse = await requestMinecraftServerAction(
        selectedServer.id,
        action,
      );
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
      ]);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, `Failed to ${action} server`));
    } finally {
      setActionLoading(null);
    }
  }

  async function handleProvisionServer() {
    if (!selectedServer) return;
    setProvisioning(true);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerOperationResponse = await provisionMinecraftServer(selectedServer.id);
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
      ]);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to provision Minecraft server'));
    } finally {
      setProvisioning(false);
    }
  }

  async function handleImportServer() {
    if (!selectedServer) return;
    if (!importSourcePath.trim()) {
      setError('Import source path is required');
      return;
    }

    setImporting(true);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerOperationResponse = await importMinecraftServer(
        selectedServer.id,
        importSourcePath.trim(),
      );
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
      ]);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to import Minecraft server'));
    } finally {
      setImporting(false);
    }
  }

  async function handleCreateServer() {
    setCreating(true);
    setError('');
    setSuccessMessage('');
    try {
      const created = await createMinecraftServer({
        display_name: form.display_name,
        description: form.description,
        server_distribution: form.server_distribution,
        minecraft_version: form.minecraft_version,
        world_name: form.world_name,
        listen_port: Number(form.listen_port),
        gamemode: form.gamemode,
        difficulty: form.difficulty,
        hardcore: form.hardcore,
        motd: form.motd,
        max_player_count: Number(form.max_player_count),
        min_memory_mb: Number(form.min_memory_mb),
        max_memory_mb: Number(form.max_memory_mb),
        online_mode: form.online_mode,
        pvp: form.pvp,
        allow_flight: form.allow_flight,
        enable_command_block: form.enable_command_block,
        white_list_enabled: form.white_list_enabled,
        autostart: form.autostart,
        eula_accepted: form.eula_accepted,
      });
      setForm({
        ...DEFAULT_FORM,
        minecraft_version: form.minecraft_version,
        server_distribution: form.server_distribution,
      });
      setSuccessMessage('Draft Minecraft server created. Runtime provisioning comes next.');
      await refreshServers(created.id);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to create Minecraft server'));
    } finally {
      setCreating(false);
    }
  }

  if (authLoading || !me) {
    return (
      <main className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
        <div className="panel-soft animate-rise px-5 py-4 text-sm muted">Loading servers…</div>
      </main>
    );
  }

  return (
    <main className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <header className="panel animate-rise space-y-4 p-6 sm:p-8">
        <div className="flex flex-wrap items-center gap-3">
          <span className="chip chip-accent">Servers</span>
          <span className="chip">Minecraft</span>
          <span className="chip">Debian 12 Native</span>
        </div>
        <div className="space-y-2">
          <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">Game servers</h1>
          <p className="max-w-3xl text-sm muted sm:text-base">
            Rustyfin now tracks Minecraft server records in PostgreSQL, exposes native lifecycle controls
            through the Rust API, and polls live runtime state into the UI. Provisioning and import land next.
          </p>
        </div>
      </header>

      {error ? (
        <div className="panel-soft animate-rise border border-red-400/30 px-5 py-4 text-sm text-red-200">
          {error}
        </div>
      ) : null}

      {successMessage ? (
        <div className="panel-soft animate-rise border border-green-400/30 px-5 py-4 text-sm text-green-200">
          {successMessage}
        </div>
      ) : null}

      <div className="grid gap-6 xl:grid-cols-[1.1fr_1.25fr_1.1fr]">
        <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h2 className="text-xl font-semibold">Known servers</h2>
              <p className="text-sm muted">Visible instances for your account.</p>
            </div>
            <span className="chip">{servers.length}</span>
          </div>

          {loading ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">Loading server records…</div>
          ) : servers.length === 0 ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              No Minecraft servers exist yet.
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
              {servers.map((server) => {
                const selected = selectedServerId === server.id;
                return (
                  <button
                    key={server.id}
                    type="button"
                    onClick={() => setSelectedServerId(server.id)}
                    className={`panel-soft rounded-xl px-4 py-4 text-left transition ${
                      selected ? 'border-[var(--orange-soft)] bg-white/10' : ''
                    }`}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="space-y-1">
                        <div className="text-sm font-semibold text-white">{server.display_name}</div>
                        <div className="text-xs muted">
                          {server.server_distribution} {server.minecraft_version}
                        </div>
                      </div>
                      <span className="chip text-[11px]">{titleCase(server.observed_state)}</span>
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2 text-xs muted">
                      <span className="chip">Port {server.listen_port}</span>
                      <span className="chip">{server.world_name}</span>
                      <span className="chip">
                        {server.current_player_count}/{server.max_player_count ?? 0} players
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </section>

        <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-xl font-semibold">Server detail</h2>
              <p className="text-sm muted">Selected instance state and recent lifecycle events.</p>
            </div>
            {selectedServer ? <span className="chip chip-accent">{titleCase(selectedServer.health_state)}</span> : null}
          </div>

          {!selectedServer ? (
            <div className="panel-soft flex flex-1 items-center justify-center rounded-xl px-4 py-10 text-sm muted">
              Select a server on the left to inspect it.
            </div>
          ) : (
            <>
              <div className="panel-soft rounded-xl px-4 py-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-lg font-semibold text-white">{selectedServer.display_name}</h3>
                    <span className="chip">{titleCase(selectedServer.observed_state)}</span>
                    <span className="chip">{titleCase(selectedServer.install_mode)}</span>
                    <span className="chip">Desired {titleCase(selectedServer.desired_state)}</span>
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                      disabled={statusRefreshing || actionLoading !== null}
                      onClick={() => void refreshSelectedServerStatus(selectedServer.id, true)}
                    >
                      {statusRefreshing ? 'Refreshing…' : 'Refresh Status'}
                    </button>
                    <button
                      type="button"
                      className="btn-primary px-3 py-2 text-xs disabled:opacity-50"
                      disabled={actionLoading !== null}
                      onClick={() => void handleRequestAction('start')}
                    >
                      {actionLoading === 'start' ? 'Starting…' : 'Start'}
                    </button>
                    <button
                      type="button"
                      className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                      disabled={actionLoading !== null}
                      onClick={() => void handleRequestAction('restart')}
                    >
                      {actionLoading === 'restart' ? 'Restarting…' : 'Restart'}
                    </button>
                    <button
                      type="button"
                      className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                      disabled={actionLoading !== null}
                      onClick={() => void handleRequestAction('stop')}
                    >
                      {actionLoading === 'stop' ? 'Stopping…' : 'Stop'}
                    </button>
                  </div>
                </div>
                <div className="mt-3 text-xs muted">
                  Lifecycle controls target the native Debian 12 systemd unit for this instance. If the
                  unit has not been provisioned or imported yet, status refresh will report that clearly.
                </div>
                <div className="mt-4 grid gap-3 text-sm sm:grid-cols-2">
                  <div>
                    <div className="muted">Owner</div>
                    <div>{selectedServer.owner_display_name}</div>
                  </div>
                  <div>
                    <div className="muted">Runtime</div>
                    <div>{titleCase(selectedServer.runtime_mode)}</div>
                  </div>
                  <div>
                    <div className="muted">Version</div>
                    <div>
                      {selectedServer.server_distribution} {selectedServer.minecraft_version}
                    </div>
                  </div>
                  <div>
                    <div className="muted">World</div>
                    <div>{selectedServer.world_name}</div>
                  </div>
                  <div>
                    <div className="muted">Gamemode</div>
                    <div>{titleCase(selectedServer.gamemode)}</div>
                  </div>
                  <div>
                    <div className="muted">Difficulty</div>
                    <div>{titleCase(selectedServer.difficulty)}</div>
                  </div>
                  <div>
                    <div className="muted">Port</div>
                    <div>{selectedServer.listen_host}:{selectedServer.listen_port}</div>
                  </div>
                  <div>
                    <div className="muted">Memory</div>
                    <div>{selectedServer.max_memory_mb} MB</div>
                  </div>
                  <div>
                    <div className="muted">Systemd unit</div>
                    <div className="break-all">{selectedServer.systemd_unit_name}</div>
                  </div>
                  <div>
                    <div className="muted">Health</div>
                    <div>{titleCase(selectedServer.health_state)}</div>
                  </div>
                  <div className="sm:col-span-2">
                    <div className="muted">Planned root</div>
                    <div className="break-all">{selectedServer.instance_root}</div>
                  </div>
                  <div className="sm:col-span-2">
                    <div className="muted">Last runtime error</div>
                    <div>{selectedServer.last_error_summary || 'None'}</div>
                  </div>
                  <div>
                    <div className="muted">Last started</div>
                    <div>{formatTs(selectedServer.last_started_ts)}</div>
                  </div>
                  <div>
                    <div className="muted">Last stopped</div>
                    <div>{formatTs(selectedServer.last_stopped_ts)}</div>
                  </div>
                </div>
              </div>

              {me.role === 'admin' ? (
                <div className="grid gap-3 lg:grid-cols-2">
                  <div className="panel-soft rounded-xl px-4 py-4">
                    <div className="space-y-2">
                      <h3 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                        Managed Provision
                      </h3>
                      <p className="text-sm muted">
                        Download the selected Minecraft server artifact, render `server.properties`,
                        write `eula.txt`, and install a native systemd unit for this instance.
                      </p>
                    </div>
                    <div className="mt-3 text-xs muted">
                      Distribution: {selectedServer.server_distribution} {selectedServer.minecraft_version}
                      {' · '}Java: {selectedServer.java_path}
                    </div>
                    <button
                      type="button"
                      className="btn-primary mt-4 px-4 py-2 text-sm disabled:opacity-50"
                      disabled={provisioning || importing || actionLoading !== null}
                      onClick={() => void handleProvisionServer()}
                    >
                      {provisioning ? 'Provisioning…' : 'Provision Managed Server'}
                    </button>
                  </div>

                  <div className="panel-soft rounded-xl px-4 py-4">
                    <div className="space-y-2">
                      <h3 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                        Import Existing Server
                      </h3>
                      <p className="text-sm muted">
                        Copy an existing Minecraft server directory from the host into Rustyfin’s
                        managed instance path, normalize the server jar, and install the unit.
                      </p>
                    </div>
                    <label className="mt-4 block space-y-2">
                      <span className="text-sm font-medium text-white">Host source path</span>
                      <input
                        className="input rounded-xl px-4 py-3"
                        value={importSourcePath}
                        onChange={(event) => setImportSourcePath(event.target.value)}
                        placeholder="/srv/minecraft/existing-world"
                      />
                    </label>
                    <div className="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="btn-secondary px-4 py-2 text-sm"
                        onClick={() => openHostDirectoryBrowser(importSourcePath)}
                      >
                        Browse Host Directories
                      </button>
                      <button
                        type="button"
                        className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                        disabled={importing || provisioning || actionLoading !== null}
                        onClick={() => void handleImportServer()}
                      >
                        {importing ? 'Importing…' : 'Import Existing Server'}
                      </button>
                    </div>
                  </div>
                </div>
              ) : null}

              <div className="flex min-h-0 flex-1 flex-col gap-3">
                <div className="flex items-center justify-between gap-3">
                  <h3 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                    Recent events
                  </h3>
                  {eventsLoading ? <span className="chip text-[11px]">Refreshing</span> : null}
                </div>
                {selectedEvents.length === 0 ? (
                  <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                    No lifecycle events recorded yet.
                  </div>
                ) : (
                  <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
                    {selectedEvents.map((event) => (
                      <div key={event.id} className="panel-soft rounded-xl px-4 py-3 text-sm">
                        <div className="flex items-start justify-between gap-3">
                          <div className="font-medium text-white">{event.message}</div>
                          <span className="chip text-[11px]">{titleCase(event.level)}</span>
                        </div>
                        <div className="mt-2 text-xs muted">
                          {titleCase(event.event_kind)} · {formatTs(event.created_ts)}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </>
          )}
        </section>

        <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
          <div>
            <h2 className="text-xl font-semibold">Create Minecraft server</h2>
            <p className="text-sm muted">
              Draft creation is still the entry point for brand-new Minecraft servers. Native start, stop,
              restart, and status refresh now sit on top of the same record model.
            </p>
          </div>

          {me.role !== 'admin' ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              Only admins can create or manage server records.
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
              <label className="space-y-2">
                <span className="text-sm font-medium text-white">Display name</span>
                <input
                  className="input rounded-xl px-4 py-3"
                  value={form.display_name}
                  onChange={(event) => updateForm('display_name', event.target.value)}
                  placeholder="Example: Family SMP"
                />
              </label>

              <label className="space-y-2">
                <span className="text-sm font-medium text-white">Description</span>
                <textarea
                  className="input min-h-[5.5rem] rounded-xl px-4 py-3"
                  value={form.description}
                  onChange={(event) => updateForm('description', event.target.value)}
                  placeholder="Optional notes about this server."
                />
              </label>

              <div className="grid gap-4 sm:grid-cols-2">
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Distribution</span>
                  <select
                    className="select rounded-xl px-4 py-3"
                    value={form.server_distribution}
                    onChange={(event) =>
                      updateForm('server_distribution', event.target.value as CreateFormState['server_distribution'])
                    }
                  >
                    <option value="paper">Paper</option>
                    <option value="vanilla">Vanilla</option>
                  </select>
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Minecraft version</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    value={form.minecraft_version}
                    onChange={(event) => updateForm('minecraft_version', event.target.value)}
                  />
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">World name</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    value={form.world_name}
                    onChange={(event) => updateForm('world_name', event.target.value)}
                  />
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Port</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    type="number"
                    value={form.listen_port}
                    onChange={(event) => updateForm('listen_port', event.target.value)}
                  />
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Gamemode</span>
                  <select
                    className="select rounded-xl px-4 py-3"
                    value={form.gamemode}
                    onChange={(event) =>
                      updateForm('gamemode', event.target.value as CreateFormState['gamemode'])
                    }
                  >
                    <option value="survival">Survival</option>
                    <option value="creative">Creative</option>
                    <option value="adventure">Adventure</option>
                    <option value="spectator">Spectator</option>
                  </select>
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Difficulty</span>
                  <select
                    className="select rounded-xl px-4 py-3"
                    value={form.difficulty}
                    onChange={(event) =>
                      updateForm('difficulty', event.target.value as CreateFormState['difficulty'])
                    }
                  >
                    <option value="peaceful">Peaceful</option>
                    <option value="easy">Easy</option>
                    <option value="normal">Normal</option>
                    <option value="hard">Hard</option>
                  </select>
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Min memory (MB)</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    type="number"
                    value={form.min_memory_mb}
                    onChange={(event) => updateForm('min_memory_mb', event.target.value)}
                  />
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Max memory (MB)</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    type="number"
                    value={form.max_memory_mb}
                    onChange={(event) => updateForm('max_memory_mb', event.target.value)}
                  />
                </label>
                <label className="space-y-2">
                  <span className="text-sm font-medium text-white">Max players</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    type="number"
                    value={form.max_player_count}
                    onChange={(event) => updateForm('max_player_count', event.target.value)}
                  />
                </label>
                <label className="space-y-2 sm:col-span-2">
                  <span className="text-sm font-medium text-white">Message of the day</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    value={form.motd}
                    onChange={(event) => updateForm('motd', event.target.value)}
                    placeholder="Defaults to the display name if left blank."
                  />
                </label>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                {TOGGLE_FIELDS.map(({ key, label }) => (
                  <label key={key} className="panel-soft flex items-center gap-3 rounded-xl px-4 py-3 text-sm">
                    <input
                      type="checkbox"
                      checked={form[key]}
                      onChange={(event) => updateForm(key, event.target.checked)}
                    />
                    <span>{label}</span>
                  </label>
                ))}
                <label className="panel-soft flex items-center gap-3 rounded-xl border border-[var(--orange-soft)]/40 px-4 py-3 text-sm">
                  <input
                    type="checkbox"
                    checked={form.eula_accepted}
                    onChange={(event) => updateForm('eula_accepted', event.target.checked)}
                  />
                  <span>I confirm the Minecraft EULA has been accepted.</span>
                </label>
              </div>

              <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                Import existing servers, artifact download, server provisioning, and systemd unit rendering are
                the next implementation steps after this lifecycle-enabled draft flow.
              </div>

              <button
                type="button"
                className="btn-primary px-5 py-3 text-sm disabled:opacity-60"
                disabled={creating}
                onClick={() => void handleCreateServer()}
              >
                {creating ? 'Creating…' : 'Create Draft Server'}
              </button>
            </div>
          )}
        </section>
      </div>

      {hostBrowser.open ? (
        <div className="fixed inset-0 z-[150] flex items-center justify-center bg-black/70 px-4 py-6">
          <div className="panel flex w-full max-w-3xl flex-col gap-4 rounded-2xl p-5 sm:p-6">
            <div className="flex items-center justify-between gap-3">
              <div>
                <h2 className="text-xl font-semibold">Browse Backend Directories</h2>
                <p className="text-sm muted">
                  Choose the host directory that contains the existing Minecraft server you want to import.
                </p>
              </div>
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-3 py-2 text-sm"
              >
                Close
              </button>
            </div>

            {hostBrowser.roots.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {hostBrowser.roots.map((rootPath) => (
                  <button
                    key={rootPath}
                    type="button"
                    onClick={() => navigateHostDirectory(rootPath)}
                    className={`btn-ghost px-2.5 py-1 text-xs ${
                      hostBrowser.currentPath.startsWith(rootPath)
                        ? 'border-[var(--orange-soft)] text-[var(--orange-soft)]'
                        : ''
                    }`}
                  >
                    {rootPath}
                  </button>
                ))}
              </div>
            ) : null}

            <div className="panel-soft rounded-xl border border-[var(--border)] px-3 py-2 flex items-center gap-2">
              <button
                type="button"
                onClick={() => navigateHostDirectory(hostBrowser.parentPath)}
                disabled={!hostBrowser.parentPath || hostBrowser.loading}
                className="btn-secondary px-3 py-1 text-xs disabled:opacity-50"
              >
                Up
              </button>
              <div className="min-w-0">
                <p className="text-[11px] uppercase tracking-[0.12em] muted">Current Path</p>
                <p className="text-sm font-mono truncate" title={hostBrowser.currentPath}>
                  {hostBrowser.currentPath || '—'}
                </p>
              </div>
            </div>

            {hostBrowser.error ? (
              <p className="text-sm text-red-300">{hostBrowser.error}</p>
            ) : null}

            <div className="panel-soft min-h-[260px] overflow-auto rounded-xl border border-[var(--border)] p-2">
              {hostBrowser.loading ? (
                <p className="px-2 py-2 text-sm muted">Loading directories…</p>
              ) : hostBrowser.directories.length === 0 ? (
                <p className="px-2 py-2 text-sm muted">No child directories found.</p>
              ) : (
                <div className="space-y-1">
                  {hostBrowser.directories.map((entry) => (
                    <button
                      key={entry.path}
                      type="button"
                      onClick={() => navigateHostDirectory(entry.path)}
                      className="w-full rounded-lg border border-[var(--border)] bg-[var(--panel)]/65 px-3 py-2 text-left text-sm hover:border-[var(--orange-soft)]/55 hover:bg-[var(--panel)]"
                      title={entry.path}
                    >
                      <div className="font-medium">{entry.name}</div>
                      <div className="truncate text-xs muted">{entry.path}</div>
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={closeHostDirectoryBrowser}
                className="btn-ghost px-4 py-2 text-sm"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  setImportSourcePath(hostBrowser.currentPath);
                  closeHostDirectoryBrowser();
                }}
                disabled={hostBrowser.loading || !hostBrowser.currentPath}
                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
              >
                Use This Directory
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </main>
  );
}
