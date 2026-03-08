'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';

import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import {
  DiscoveryCandidate,
  MinecraftServerAction,
  MinecraftServer,
  MinecraftServerActionResponse,
  MinecraftServerLogsResponse,
  MinecraftServerEvent,
  MinecraftServerOperationResponse,
  ServerLogLine,
  createMinecraftServer,
  HostDirectoryListEntry,
  listMinecraftServerLogs,
  listMinecraftServerEvents,
  listBackendHostDirectories,
  listMinecraftServers,
  importMinecraftServer,
  provisionMinecraftServer,
  refreshMinecraftServerStatus,
  requestMinecraftServerAction,
  scanMinecraftDiscoveryCandidates,
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

function formatTsMs(ts?: number | null) {
  if (!ts) return 'Unknown';
  return new Date(ts).toLocaleString();
}

function titleCase(value: string) {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function getServerIndicator(server: MinecraftServer) {
  if (server.observed_state === 'running' && server.health_state === 'healthy') {
    return {
      label: 'Online',
      dotClass: 'bg-emerald-400 shadow-[0_0_14px_rgba(74,222,128,0.45)]',
      textClass: 'text-emerald-200',
    };
  }

  if (server.observed_state === 'starting' || server.observed_state === 'restarting') {
    return {
      label: titleCase(server.observed_state),
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
    };
  }

  if (server.observed_state === 'stopping') {
    return {
      label: 'Stopping',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
    };
  }

  if (server.health_state === 'error' || server.observed_state === 'error') {
    return {
      label: 'Error',
      dotClass: 'bg-rose-400 shadow-[0_0_14px_rgba(251,113,133,0.4)]',
      textClass: 'text-rose-200',
    };
  }

  return {
    label: 'Offline',
    dotClass: 'bg-rose-400 shadow-[0_0_14px_rgba(251,113,133,0.4)]',
    textClass: 'text-rose-200',
  };
}

export default function ServersPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const [servers, setServers] = useState<MinecraftServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const [managementServerId, setManagementServerId] = useState<string | null>(null);
  const [selectedEvents, setSelectedEvents] = useState<MinecraftServerEvent[]>([]);
  const [selectedLogs, setSelectedLogs] = useState<ServerLogLine[]>([]);
  const [loading, setLoading] = useState(true);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [logsLoading, setLogsLoading] = useState(false);
  const [statusRefreshing, setStatusRefreshing] = useState(false);
  const [actionLoading, setActionLoading] = useState<MinecraftServerAction | null>(null);
  const [provisioning, setProvisioning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
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
  const [discoveryRootPath, setDiscoveryRootPath] = useState('');
  const [discoveryRoots, setDiscoveryRoots] = useState<string[]>([]);
  const [discoveryCandidates, setDiscoveryCandidates] = useState<DiscoveryCandidate[]>([]);

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
          return null;
        });
        setManagementServerId((current) => {
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
      setSelectedLogs([]);
      return;
    }

    let cancelled = false;
    void (async () => {
      try {
        await Promise.all([
          refreshSelectedServerStatus(selectedServerId, true),
          loadSelectedServerEvents(selectedServerId, true),
          loadSelectedServerLogs(selectedServerId, true),
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
      void loadSelectedServerLogs(selectedServerId, false);
    }, 5000);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [me, selectedServerId]);

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
      return null;
    });
    setManagementServerId((current) => {
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

  async function loadSelectedServerLogs(serverId: string, showSpinner = true) {
    if (showSpinner) {
      setLogsLoading(true);
    }
    try {
      const response: MinecraftServerLogsResponse = await listMinecraftServerLogs(serverId, 80);
      setSelectedLogs(response.lines);
    } catch {
      setSelectedLogs([]);
    } finally {
      if (showSpinner) {
        setLogsLoading(false);
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

  async function handleDiscoveryScan(rootPath?: string) {
    setDiscoveryLoading(true);
    setError('');
    try {
      const response = await scanMinecraftDiscoveryCandidates(rootPath, 64);
      setDiscoveryRoots(response.roots);
      setDiscoveryCandidates(response.candidates);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to scan for existing Minecraft servers'));
      setDiscoveryCandidates([]);
    } finally {
      setDiscoveryLoading(false);
    }
  }

  async function handleRequestAction(server: MinecraftServer, action: MinecraftServerAction) {
    setSelectedServerId(server.id);
    setActionLoading(action);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerActionResponse = await requestMinecraftServerAction(server.id, action);
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
        loadSelectedServerLogs(response.instance.id, false),
      ]);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, `Failed to ${action} server`));
    } finally {
      setActionLoading(null);
    }
  }

  async function handleProvisionServer(server: MinecraftServer) {
    setSelectedServerId(server.id);
    setProvisioning(true);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerOperationResponse = await provisionMinecraftServer(server.id);
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
        loadSelectedServerLogs(response.instance.id, false),
      ]);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to provision Minecraft server'));
    } finally {
      setProvisioning(false);
    }
  }

  async function handleImportServer(server: MinecraftServer) {
    if (!importSourcePath.trim()) {
      setError('Import source path is required');
      return;
    }

    setSelectedServerId(server.id);
    setImporting(true);
    setError('');
    setSuccessMessage('');
    try {
      const response: MinecraftServerOperationResponse = await importMinecraftServer(
        server.id,
        importSourcePath.trim(),
      );
      upsertServer(response.instance);
      setSuccessMessage(response.message);
      await Promise.all([
        refreshSelectedServerStatus(response.instance.id, false),
        loadSelectedServerEvents(response.instance.id, false),
        loadSelectedServerLogs(response.instance.id, false),
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

  const managementServer = servers.find((server) => server.id === managementServerId) ?? null;

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
            Rustyfin now tracks Minecraft server records in PostgreSQL, uses a dedicated Rust
            servers agent for privileged Debian host operations, and exposes lifecycle control,
            discovery, import, status, and logs through one management surface.
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

      <section className="panel flex flex-col gap-4 p-5 sm:p-6">
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
          <div className="space-y-4">
            {servers.map((server) => {
              const expanded = selectedServerId === server.id;
              const indicator = getServerIndicator(server);
              const canStart =
                server.observed_state !== 'running' &&
                server.observed_state !== 'starting' &&
                server.observed_state !== 'restarting';
              const canRestart =
                server.observed_state === 'running' || server.health_state === 'healthy';
              const canStop =
                server.observed_state === 'running' ||
                server.observed_state === 'starting' ||
                server.observed_state === 'restarting';
              return (
                <div
                  key={server.id}
                  className={`panel-soft rounded-xl px-4 py-4 transition ${
                    expanded ? 'border-[var(--orange-soft)] bg-white/10' : ''
                  }`}
                >
                  <div className="flex flex-col gap-4">
                    <div className="flex flex-wrap items-start justify-between gap-4">
                      <div className="min-w-0 flex-1 space-y-2">
                        <div className="flex flex-wrap items-center gap-3">
                          <h3 className="text-lg font-semibold text-white">{server.display_name}</h3>
                          <span
                            className={`inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1 text-xs font-medium ${indicator.textClass}`}
                          >
                            <span className={`h-2.5 w-2.5 rounded-full ${indicator.dotClass}`} />
                            {indicator.label}
                          </span>
                          <span className="chip">Port {server.listen_port}</span>
                          <span className="chip">
                            {server.current_player_count}/{server.max_player_count ?? 0} players
                          </span>
                          <span className="chip">{server.server_distribution} {server.minecraft_version}</span>
                        </div>
                        {server.description ? (
                          <div className="max-w-3xl text-sm muted">{server.description}</div>
                        ) : null}
                        <div className="flex flex-wrap items-center gap-3 text-sm muted">
                          <span>{server.server_distribution} {server.minecraft_version}</span>
                          <span className="text-white/20">•</span>
                          <span>World {server.world_name}</span>
                          <span className="text-white/20">•</span>
                          <span>{titleCase(server.runtime_mode)}</span>
                        </div>
                      </div>

                      <div className="flex min-w-fit flex-col items-end gap-2 self-start">
                        <div className="flex flex-wrap justify-end gap-2">
                          <button
                            type="button"
                            className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                            disabled={statusRefreshing || actionLoading !== null}
                            onClick={() => void refreshSelectedServerStatus(server.id, true)}
                          >
                            {statusRefreshing && expanded ? 'Refreshing…' : 'Refresh Status'}
                          </button>
                          <button
                            type="button"
                            className="btn-primary px-3 py-2 text-xs disabled:opacity-50"
                            disabled={actionLoading !== null || !canStart}
                            onClick={() => void handleRequestAction(server, 'start')}
                          >
                            {actionLoading === 'start' && expanded ? 'Starting…' : 'Start'}
                          </button>
                          <button
                            type="button"
                            className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                            disabled={actionLoading !== null || !canRestart}
                            onClick={() => void handleRequestAction(server, 'restart')}
                          >
                            {actionLoading === 'restart' && expanded ? 'Restarting…' : 'Restart'}
                          </button>
                          <button
                            type="button"
                            className="btn-secondary px-3 py-2 text-xs disabled:opacity-50"
                            disabled={actionLoading !== null || !canStop}
                            onClick={() => void handleRequestAction(server, 'stop')}
                          >
                            {actionLoading === 'stop' && expanded ? 'Stopping…' : 'Stop'}
                          </button>
                        </div>
                        <button
                          type="button"
                          className="btn-ghost px-3 py-2 text-xs"
                          onClick={() =>
                            setSelectedServerId((current) => (current === server.id ? null : server.id))
                          }
                        >
                          {expanded ? 'Hide details' : 'Show details'}
                        </button>
                      </div>
                    </div>

                    {expanded ? (
                      <>
                        <div className="text-xs muted">
                          Lifecycle controls target the native Debian 12 systemd unit for this instance.
                          If the unit has not been provisioned or imported yet, status refresh will report that clearly.
                        </div>

                        <div className="grid gap-3 text-sm sm:grid-cols-2 xl:grid-cols-4">
                          <div>
                            <div className="muted">Owner</div>
                            <div>{server.owner_display_name}</div>
                          </div>
                          <div>
                            <div className="muted">Runtime</div>
                            <div>{titleCase(server.runtime_mode)}</div>
                          </div>
                          <div>
                            <div className="muted">Version</div>
                            <div>{server.server_distribution} {server.minecraft_version}</div>
                          </div>
                          <div>
                            <div className="muted">World</div>
                            <div>{server.world_name}</div>
                          </div>
                          <div>
                            <div className="muted">Gamemode</div>
                            <div>{titleCase(server.gamemode)}</div>
                          </div>
                          <div>
                            <div className="muted">Difficulty</div>
                            <div>{titleCase(server.difficulty)}</div>
                          </div>
                          <div>
                            <div className="muted">Port</div>
                            <div>{server.listen_host}:{server.listen_port}</div>
                          </div>
                          <div>
                            <div className="muted">Memory</div>
                            <div>{server.min_memory_mb}-{server.max_memory_mb} MB</div>
                          </div>
                          <div>
                            <div className="muted">Systemd unit</div>
                            <div className="break-all">{server.systemd_unit_name}</div>
                          </div>
                          <div>
                            <div className="muted">Health</div>
                            <div>{titleCase(server.health_state)}</div>
                          </div>
                          <div>
                            <div className="muted">Last started</div>
                            <div>{formatTs(server.last_started_ts)}</div>
                          </div>
                          <div>
                            <div className="muted">Last stopped</div>
                            <div>{formatTs(server.last_stopped_ts)}</div>
                          </div>
                          <div>
                            <div className="muted">Last ready</div>
                            <div>{formatTs(server.last_ready_ts)}</div>
                          </div>
                          <div>
                            <div className="muted">Updated</div>
                            <div>{formatTs(server.updated_ts)}</div>
                          </div>
                          <div>
                            <div className="muted">Advertised address</div>
                            <div>
                              {server.advertised_host
                                ? `${server.advertised_host}:${server.advertised_port ?? server.listen_port}`
                                : 'Not set'}
                            </div>
                          </div>
                          <div>
                            <div className="muted">Exit code</div>
                            <div>{server.last_exit_code ?? 'Unknown'}</div>
                          </div>
                          <div>
                            <div className="muted">MOTD</div>
                            <div>{server.motd || 'Not set'}</div>
                          </div>
                          <div>
                            <div className="muted">Autostart</div>
                            <div>{server.autostart ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Auto stop when empty</div>
                            <div>
                              {server.auto_stop_when_empty
                                ? `Enabled${server.auto_stop_idle_minutes ? ` (${server.auto_stop_idle_minutes} min idle)` : ''}`
                                : 'Disabled'}
                            </div>
                          </div>
                          <div>
                            <div className="muted">Online mode</div>
                            <div>{server.online_mode ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">PVP</div>
                            <div>{server.pvp ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Allow flight</div>
                            <div>{server.allow_flight ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Command blocks</div>
                            <div>{server.enable_command_block ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Whitelist</div>
                            <div>{server.white_list_enabled ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Hardcore</div>
                            <div>{server.hardcore ? 'Enabled' : 'Disabled'}</div>
                          </div>
                          <div>
                            <div className="muted">Work directory</div>
                            <div className="break-all">{server.server_work_dir}</div>
                          </div>
                          <div>
                            <div className="muted">Java path</div>
                            <div className="break-all">{server.java_path}</div>
                          </div>
                          <div className="sm:col-span-2 xl:col-span-4">
                            <div className="muted">Planned root</div>
                            <div className="break-all">{server.instance_root}</div>
                          </div>
                          <div className="sm:col-span-2 xl:col-span-4">
                            <div className="muted">Last runtime error</div>
                            <div>{server.last_error_summary || 'None'}</div>
                          </div>
                        </div>

                        <div className="grid gap-4 xl:grid-cols-2">
                          <div className="flex min-h-0 flex-col gap-3">
                            <div className="flex items-center justify-between gap-3">
                              <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                                Recent events
                              </h4>
                              {eventsLoading ? <span className="chip text-[11px]">Refreshing</span> : null}
                            </div>
                            {selectedEvents.length === 0 ? (
                              <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                                No lifecycle events recorded yet.
                              </div>
                            ) : (
                              <div className="max-h-[20rem] space-y-3 overflow-y-auto pr-1">
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

                          <div className="flex min-h-0 flex-col gap-3">
                            <div className="flex items-center justify-between gap-3">
                              <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                                Journald logs
                              </h4>
                              {logsLoading ? <span className="chip text-[11px]">Refreshing</span> : null}
                            </div>
                            {selectedLogs.length === 0 ? (
                              <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                                No host logs have been returned for this unit yet.
                              </div>
                            ) : (
                              <div className="max-h-[20rem] space-y-2 overflow-y-auto pr-1">
                                {selectedLogs.map((line, index) => (
                                  <div
                                    key={`${line.ts_ms ?? 'no-ts'}-${index}`}
                                    className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/55 px-4 py-3 text-sm"
                                  >
                                    <div className="text-white">{line.message}</div>
                                    <div className="mt-2 text-xs muted">
                                      {formatTsMs(line.ts_ms)}
                                      {line.priority ? ` · priority ${line.priority}` : ''}
                                    </div>
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        </div>
                      </>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <div className="grid gap-6 xl:grid-cols-2">
        <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
            <div>
              <h2 className="text-xl font-semibold">Create Minecraft server</h2>
              <p className="text-sm muted">
                Draft creation is the entry point for brand-new Minecraft servers. Create the record
                here, then use the management column to provision or import it on the Debian host.
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
                  Draft records appear in known servers immediately. Provisioning and import now live
                  in the dedicated management column beside this form.
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

        <section className="panel flex min-h-[34rem] flex-col gap-4 p-5 sm:p-6">
          <div>
            <h2 className="text-xl font-semibold">Server management</h2>
            <p className="text-sm muted">
              Provisioning and import act on a chosen server record. Discovery scan is host-wide and
              feeds import paths into that target.
            </p>
          </div>

          {me.role !== 'admin' ? (
            <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
              Only admins can provision, import, or discover server data.
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
              <label className="space-y-2">
                <span className="text-sm font-medium text-white">Target server</span>
                <select
                  className="select rounded-xl px-4 py-3"
                  value={managementServerId ?? ''}
                  onChange={(event) => setManagementServerId(event.target.value || null)}
                >
                  <option value="">Choose a server record</option>
                  {servers.map((server) => (
                    <option key={server.id} value={server.id}>
                      {server.display_name} · {server.server_distribution} {server.minecraft_version} · Port {server.listen_port}
                    </option>
                  ))}
                </select>
              </label>

              {managementServer ? (
                <div className="panel-soft rounded-xl px-4 py-3 text-sm">
                  <div className="font-medium text-white">{managementServer.display_name}</div>
                  <div className="mt-1 text-xs muted">
                    {managementServer.server_distribution} {managementServer.minecraft_version}
                    {' · '}
                    {managementServer.world_name}
                    {' · '}
                    Port {managementServer.listen_port}
                  </div>
                </div>
              ) : (
                <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                  Choose a server record before provisioning or importing.
                </div>
              )}

              <div className="panel rounded-xl px-4 py-4">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                    Managed Provision
                  </h4>
                  <p className="text-sm muted">
                    Download the configured Minecraft artifact, render `server.properties`, write
                    `eula.txt`, and install the native systemd unit for the target server.
                  </p>
                </div>
                <div className="mt-3 text-xs muted">
                  {managementServer
                    ? `Distribution: ${managementServer.server_distribution} ${managementServer.minecraft_version} · Java: ${managementServer.java_path}`
                    : 'Choose a target server first.'}
                </div>
                <button
                  type="button"
                  className="btn-primary mt-4 px-4 py-2 text-sm disabled:opacity-50"
                  disabled={!managementServer || provisioning || importing || actionLoading !== null}
                  onClick={() => managementServer && void handleProvisionServer(managementServer)}
                >
                  {provisioning ? 'Provisioning…' : 'Provision Managed Server'}
                </button>
              </div>

              <div className="panel rounded-xl px-4 py-4">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                    Import Existing Server
                  </h4>
                  <p className="text-sm muted">
                    Copy an existing Minecraft server directory from the host into the managed target
                    path, normalize the server jar, and install the native unit.
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
                    disabled={!managementServer || importing || provisioning || actionLoading !== null}
                    onClick={() => managementServer && void handleImportServer(managementServer)}
                  >
                    {importing ? 'Importing…' : 'Import Existing Server'}
                  </button>
                </div>
              </div>

              <div className="panel rounded-xl px-4 py-4">
                <div className="space-y-2">
                  <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                    Discovery Scan
                  </h4>
                  <p className="text-sm muted">
                    Scan configured import roots on the Debian host for existing Minecraft directories,
                    then push a discovered path into the import field for the chosen target server.
                  </p>
                </div>
                <label className="mt-4 block space-y-2">
                  <span className="text-sm font-medium text-white">Optional scan root</span>
                  <input
                    className="input rounded-xl px-4 py-3"
                    value={discoveryRootPath}
                    onChange={(event) => setDiscoveryRootPath(event.target.value)}
                    placeholder="/srv/minecraft"
                  />
                </label>
                {discoveryRoots.length > 0 ? (
                  <div className="mt-3 flex flex-wrap gap-2">
                    {discoveryRoots.map((root) => (
                      <button
                        key={root}
                        type="button"
                        className="btn-ghost px-2.5 py-1 text-xs"
                        onClick={() => {
                          setDiscoveryRootPath(root);
                          void handleDiscoveryScan(root);
                        }}
                      >
                        {root}
                      </button>
                    ))}
                  </div>
                ) : null}
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="btn-secondary px-4 py-2 text-sm disabled:opacity-50"
                    disabled={discoveryLoading}
                    onClick={() => void handleDiscoveryScan(discoveryRootPath || undefined)}
                  >
                    {discoveryLoading ? 'Scanning…' : 'Scan For Existing Servers'}
                  </button>
                  <button
                    type="button"
                    className="btn-ghost px-4 py-2 text-sm disabled:opacity-50"
                    disabled={discoveryLoading}
                    onClick={() => void handleDiscoveryScan(undefined)}
                  >
                    Scan All Roots
                  </button>
                </div>
                <div className="mt-4 max-h-[17rem] space-y-2 overflow-y-auto pr-1">
                  {discoveryCandidates.length === 0 ? (
                    <div className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/55 px-3 py-3 text-sm muted">
                      No discovery results yet.
                    </div>
                  ) : (
                    discoveryCandidates.map((candidate) => (
                      <div
                        key={candidate.path}
                        className="rounded-xl border border-[var(--border)] bg-[var(--panel)]/55 px-3 py-3 text-sm"
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="truncate font-medium text-white" title={candidate.path}>
                              {candidate.name}
                            </div>
                            <div className="truncate text-xs muted" title={candidate.path}>
                              {candidate.path}
                            </div>
                          </div>
                          <button
                            type="button"
                            className="btn-primary shrink-0 px-3 py-1.5 text-xs"
                            onClick={() => setImportSourcePath(candidate.path)}
                          >
                            Use Path
                          </button>
                        </div>
                        <div className="mt-2 flex flex-wrap gap-2 text-xs muted">
                          {candidate.world_name ? <span className="chip">{candidate.world_name}</span> : null}
                          {candidate.top_level_jars[0] ? <span className="chip">{candidate.top_level_jars[0]}</span> : null}
                          <span className="chip">
                            {candidate.server_properties_present ? 'server.properties' : 'jar only'}
                          </span>
                          {candidate.last_modified_ts ? (
                            <span className="chip">Updated {formatTs(candidate.last_modified_ts)}</span>
                          ) : null}
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
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
