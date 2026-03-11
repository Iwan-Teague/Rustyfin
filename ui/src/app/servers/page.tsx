'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import ConfirmModal from '@/app/components/ConfirmModal';

import { useAuth } from '@/lib/auth';
import { clientErrorMessage } from '@/lib/errors';
import {
  deleteMinecraftServer,
  DiscoveryCandidate,
  MinecraftServerAction,
  MinecraftRuntimeCapabilities,
  MinecraftServer,
  MinecraftServerActionResponse,
  MinecraftServerOperationResponse,
  createMinecraftServer,
  getMinecraftRuntimeCapabilities,
  HostDirectoryListEntry,
  listBackendHostDirectories,
  listMinecraftServers,
  importMinecraftServer,
  provisionMinecraftServer,
  refreshMinecraftServerStatus,
  requestMinecraftServerAction,
  scanMinecraftDiscoveryCandidates,
  updateMinecraftServer,
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

type ServerSettingsFormState = {
  gamemode: 'survival' | 'creative' | 'adventure' | 'spectator';
  difficulty: 'peaceful' | 'easy' | 'normal' | 'hard';
  hardcore: boolean;
  motd: string;
  max_player_count: string;
  autostart: boolean;
  online_mode: boolean;
  pvp: boolean;
  allow_flight: boolean;
  enable_command_block: boolean;
  white_list_enabled: boolean;
};

type ServersGameTab = 'minecraft' | 'more-soon';

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

const SERVER_SETTINGS_TOGGLE_FIELDS: Array<{
  key: keyof Pick<
    ServerSettingsFormState,
    | 'online_mode'
    | 'pvp'
    | 'allow_flight'
    | 'enable_command_block'
    | 'white_list_enabled'
    | 'autostart'
    | 'hardcore'
  >;
  label: string;
}> = [
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

function serverToSettingsForm(server: MinecraftServer): ServerSettingsFormState {
  return {
    gamemode: server.gamemode as ServerSettingsFormState['gamemode'],
    difficulty: server.difficulty as ServerSettingsFormState['difficulty'],
    hardcore: server.hardcore,
    motd: server.motd,
    max_player_count: String(server.max_player_count ?? 20),
    autostart: server.autostart,
    online_mode: server.online_mode,
    pvp: server.pvp,
    allow_flight: server.allow_flight,
    enable_command_block: server.enable_command_block,
    white_list_enabled: server.white_list_enabled,
  };
}

function getServerIndicator(server: MinecraftServer) {
  if (server.observed_state === 'draft') {
    return {
      label: 'Draft',
      dotClass: 'bg-slate-300 shadow-[0_0_14px_rgba(226,232,240,0.22)]',
      textClass: 'text-slate-200',
    };
  }

  if (server.observed_state === 'unprovisioned') {
    return {
      label: 'Needs Provisioning',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
    };
  }

  if (server.observed_state === 'provisioning' || server.observed_state === 'importing') {
    return {
      label: titleCase(server.observed_state),
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
    };
  }

  if (server.observed_state === 'running' && server.health_state === 'pending') {
    return {
      label: 'Booting',
      dotClass: 'bg-amber-400 shadow-[0_0_14px_rgba(251,191,36,0.4)]',
      textClass: 'text-amber-200',
    };
  }

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

function getServerProgressMessage(server: MinecraftServer) {
  if (server.observed_state === 'draft') {
    return 'Created. Click Start to provision the server files and launch the Minecraft service.';
  }

  if (server.observed_state === 'unprovisioned') {
    return 'Ready to provision. Click Start to create the native service and first-time server files.';
  }

  if (server.observed_state === 'provisioning') {
    return 'Provisioning server files and native service. First boot can take a minute or two.';
  }

  if (server.observed_state === 'importing') {
    return 'Importing the existing server into Rustyfin management now.';
  }

  if (server.observed_state === 'starting') {
    return 'Launching the Minecraft service now.';
  }

  if (server.observed_state === 'restarting') {
    return 'Restarting the Minecraft service now.';
  }

  if (server.observed_state === 'running' && server.health_state === 'pending') {
    return 'The process is up. Waiting for Minecraft to finish booting and accept player connections.';
  }

  if (
    (server.health_state === 'error' || server.observed_state === 'failed' || server.observed_state === 'error') &&
    server.last_error_summary
  ) {
    return server.last_error_summary;
  }

  return null;
}

function shouldAutoRefreshServer(server: MinecraftServer) {
  return (
    server.observed_state === 'provisioning' ||
    server.observed_state === 'importing' ||
    server.observed_state === 'starting' ||
    server.observed_state === 'restarting' ||
    (server.observed_state === 'running' && server.health_state === 'pending')
  );
}

export default function ServersPage() {
  const router = useRouter();
  const { me, loading: authLoading } = useAuth();

  const activeGameTab: ServersGameTab = 'minecraft';
  const [servers, setServers] = useState<MinecraftServer[]>([]);
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const [managementServerId, setManagementServerId] = useState<string | null>(null);
  const [serverEdits, setServerEdits] = useState<Record<string, ServerSettingsFormState>>({});
  const [loading, setLoading] = useState(true);
  const [statusRefreshingServerId, setStatusRefreshingServerId] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState<MinecraftServerAction | null>(null);
  const [actionServerId, setActionServerId] = useState<string | null>(null);
  const [savingServerId, setSavingServerId] = useState<string | null>(null);
  const [provisioning, setProvisioning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deletingServerId, setDeletingServerId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [deleteConfirmServer, setDeleteConfirmServer] = useState<MinecraftServer | null>(null);
  const [runtimeCapabilities, setRuntimeCapabilities] = useState<MinecraftRuntimeCapabilities | null>(null);
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
        const [rows, capabilities] = await Promise.all([
          listMinecraftServers(),
          getMinecraftRuntimeCapabilities(),
        ]);
        if (cancelled) return;
        setServers(rows);
        setRuntimeCapabilities(capabilities);
        setServerEdits((prev) => {
          const next: Record<string, ServerSettingsFormState> = {};
          for (const row of rows) {
            next[row.id] = prev[row.id] ?? serverToSettingsForm(row);
          }
          return next;
        });
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
    if (!me || !(runtimeCapabilities?.status_supported ?? true)) {
      return;
    }

    const activeServerIds = servers
      .filter((server) => shouldAutoRefreshServer(server))
      .map((server) => server.id);

    if (activeServerIds.length === 0) {
      return;
    }

    let cancelled = false;
    let inFlight = false;

    const refreshActiveServers = async () => {
      if (cancelled || inFlight) return;
      inFlight = true;
      try {
        await Promise.all(
          activeServerIds.map((serverId) =>
            refreshSelectedServerStatus(serverId, false).catch(() => undefined),
          ),
        );
      } finally {
        inFlight = false;
      }
    };

    void refreshActiveServers();
    const interval = window.setInterval(() => {
      void refreshActiveServers();
    }, 3000);

    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [me, runtimeCapabilities?.status_supported, servers]);

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
    setServerEdits((prev) => {
      const next: Record<string, ServerSettingsFormState> = {};
      for (const row of rows) {
        next[row.id] = prev[row.id] ?? serverToSettingsForm(row);
      }
      return next;
    });
    setSelectedServerId((current) => {
      if (current && rows.some((row) => row.id === current)) return current;
      return null;
    });
    setManagementServerId((current) => {
      if (selectId && rows.some((row) => row.id === selectId)) return selectId;
      if (current && rows.some((row) => row.id === current)) return current;
      return rows[0]?.id ?? null;
    });
  }

  async function refreshSelectedServerStatus(serverId: string, showSpinner = true) {
    if (showSpinner) {
      setStatusRefreshingServerId(serverId);
    }
    try {
      const updated = await refreshMinecraftServerStatus(serverId);
      upsertServer(updated);
      return updated;
    } finally {
      if (showSpinner) {
        setStatusRefreshingServerId((current) => (current === serverId ? null : current));
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
    setActionServerId(server.id);
    setActionLoading(action);
    setError('');
    try {
      const response: MinecraftServerActionResponse = await requestMinecraftServerAction(server.id, action);
      upsertServer(response.instance);
      await refreshSelectedServerStatus(response.instance.id, false);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, `Failed to ${action} server`));
    } finally {
      setActionLoading(null);
      setActionServerId((current) => (current === server.id ? null : current));
    }
  }

  async function handleProvisionServer(server: MinecraftServer) {
    setProvisioning(true);
    setError('');
    try {
      const response: MinecraftServerOperationResponse = await provisionMinecraftServer(server.id);
      upsertServer(response.instance);
      await refreshSelectedServerStatus(response.instance.id, false);
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

    setImporting(true);
    setError('');
    try {
      const response: MinecraftServerOperationResponse = await importMinecraftServer(
        server.id,
        importSourcePath.trim(),
      );
      upsertServer(response.instance);
      await refreshSelectedServerStatus(response.instance.id, false);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to import Minecraft server'));
    } finally {
      setImporting(false);
    }
  }

  function setServerEdit<K extends keyof ServerSettingsFormState>(
    serverId: string,
    key: K,
    value: ServerSettingsFormState[K],
  ) {
    const server = servers.find((entry) => entry.id === serverId);
    if (!server) {
      return;
    }
    setServerEdits((prev) => ({
      ...prev,
      [serverId]: {
        ...(prev[serverId] ?? serverToSettingsForm(server)),
        [key]: value,
      },
    }));
  }

  async function handleSaveServerSettings(server: MinecraftServer) {
    const edit = serverEdits[server.id] ?? serverToSettingsForm(server);
    setSavingServerId(server.id);
    setError('');
    try {
      const updated = await updateMinecraftServer(server.id, {
        gamemode: edit.gamemode,
        difficulty: edit.difficulty,
        hardcore: edit.hardcore,
        motd: edit.motd.trim(),
        max_player_count: Number(edit.max_player_count),
        autostart: edit.autostart,
        online_mode: edit.online_mode,
        pvp: edit.pvp,
        allow_flight: edit.allow_flight,
        enable_command_block: edit.enable_command_block,
        white_list_enabled: edit.white_list_enabled,
      });
      upsertServer(updated);
      setServerEdits((prev) => ({
        ...prev,
        [server.id]: serverToSettingsForm(updated),
      }));
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to update Minecraft server settings'));
    } finally {
      setSavingServerId((current) => (current === server.id ? null : current));
    }
  }

  async function handleCreateServer() {
    setCreating(true);
    setError('');
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
      await refreshServers(created.id);
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to create Minecraft server'));
    } finally {
      setCreating(false);
    }
  }

  async function handleDeleteServer(server: MinecraftServer) {
    setDeletingServerId(server.id);
    setError('');
    try {
      await deleteMinecraftServer(server.id);
      setDeleteConfirmServer(null);
      await refreshServers();
    } catch (err: unknown) {
      setError(clientErrorMessage(err, 'Failed to delete Minecraft server'));
    } finally {
      setDeletingServerId(null);
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
      {error ? (
        <div className="panel-soft animate-rise border border-red-400/30 px-5 py-4 text-sm text-red-200">
          {error}
        </div>
      ) : null}

      {runtimeCapabilities?.reason ? (
        <div className="panel-soft animate-rise border border-amber-400/30 px-5 py-4 text-sm text-amber-100">
          {runtimeCapabilities.reason}
        </div>
      ) : null}

      <section className="panel relative mt-[17px] flex flex-col gap-4 p-5 pt-[20px] sm:p-6 sm:pt-[24px]">
        <div className="absolute left-4 right-4 top-[-16px] z-10 -translate-y-[62%] sm:left-6 sm:right-6">
          <div className="flex flex-wrap items-end gap-2">
            <button
              type="button"
              disabled
              className={`rounded-t-lg border border-b-0 px-5 py-2.5 text-sm font-medium transition-colors disabled:opacity-100 ${
                activeGameTab === 'minecraft'
                  ? 'border-[var(--border)] bg-[var(--surface)]'
                  : 'border-[var(--border)]/50 bg-[var(--surface)]/40 opacity-60'
              }`}
            >
              Minecraft
            </button>
            <button
              type="button"
              disabled
              className="rounded-t-lg border border-b-0 border-[var(--border)]/50 bg-[var(--surface)]/40 px-5 py-2.5 text-sm font-medium opacity-60"
            >
              More soon
            </button>
          </div>
        </div>

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
              const progressMessage = getServerProgressMessage(server);
              const needsProvisioning =
                server.observed_state === 'draft' ||
                server.observed_state === 'unprovisioned' ||
                server.observed_state === 'provisioning' ||
                server.observed_state === 'importing';
              const lifecycleSupported = runtimeCapabilities?.lifecycle_supported ?? true;
              const statusSupported = runtimeCapabilities?.status_supported ?? true;
              const deleteSupported = runtimeCapabilities?.delete_supported ?? true;
              const canDelete = me.role === 'admin';
              const canStart =
                lifecycleSupported &&
                server.observed_state !== 'provisioning' &&
                server.observed_state !== 'importing' &&
                server.observed_state !== 'running' &&
                server.observed_state !== 'starting' &&
                server.observed_state !== 'restarting';
              const canRestart =
                lifecycleSupported &&
                !needsProvisioning &&
                (server.observed_state === 'running' || server.health_state === 'healthy');
              const canStop =
                lifecycleSupported &&
                !needsProvisioning &&
                (server.observed_state === 'running' ||
                  server.observed_state === 'starting' ||
                  server.observed_state === 'restarting');
              return (
                <div
                  key={server.id}
                  className={`panel-soft rounded-xl px-4 py-4 transition ${
                    expanded ? 'border-[var(--orange-soft)] bg-white/10' : ''
                  }`}
                >
                  <div className="flex flex-col gap-4">
                    <div className="flex flex-wrap items-start justify-between gap-4">
                      <div className="min-w-0 flex-1">
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
                      </div>

                      <div className="grid w-full grid-cols-2 gap-2 sm:flex sm:w-auto sm:min-w-fit sm:flex-wrap sm:justify-end sm:self-start">
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={
                            statusRefreshingServerId !== null || actionLoading !== null || !statusSupported
                          }
                          onClick={() => void refreshSelectedServerStatus(server.id, true)}
                          title={!statusSupported ? runtimeCapabilities?.reason ?? 'Status refresh unavailable here' : undefined}
                        >
                          {statusRefreshingServerId === server.id ? 'Refreshing…' : 'Refresh Status'}
                        </button>
                        <button
                          type="button"
                          className="btn-primary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canStart}
                          onClick={() => void handleRequestAction(server, 'start')}
                        >
                          {actionLoading === 'start' && actionServerId === server.id ? 'Starting…' : 'Start'}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canRestart}
                          onClick={() => void handleRequestAction(server, 'restart')}
                          title={
                            needsProvisioning
                              ? 'Provision or import this Minecraft server before restarting it.'
                              : undefined
                          }
                        >
                          {actionLoading === 'restart' && actionServerId === server.id ? 'Restarting…' : 'Restart'}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary w-full px-3 py-2 text-center text-xs disabled:opacity-50 sm:w-auto"
                          disabled={actionLoading !== null || !canStop}
                          onClick={() => void handleRequestAction(server, 'stop')}
                          title={
                            needsProvisioning
                              ? 'Provision or import this Minecraft server before stopping it.'
                              : undefined
                          }
                        >
                          {actionLoading === 'stop' && actionServerId === server.id ? 'Stopping…' : 'Stop'}
                        </button>
                        {canDelete ? (
                          <button
                            type="button"
                            className="rounded-xl border border-red-400/30 bg-red-500/10 px-3 py-2 text-center text-xs font-medium text-red-100 transition hover:border-red-300/50 hover:bg-red-500/15 disabled:opacity-50 sm:w-auto"
                            disabled={deletingServerId !== null || actionLoading !== null || !deleteSupported}
                            onClick={() => setDeleteConfirmServer(server)}
                            title={!deleteSupported ? runtimeCapabilities?.reason ?? 'Delete unavailable here' : undefined}
                          >
                            Delete
                          </button>
                        ) : null}
                        <button
                          type="button"
                          className="btn-ghost col-span-2 w-full px-3 py-2 text-center text-xs sm:col-auto sm:min-w-[7.75rem] sm:w-auto"
                          onClick={() => {
                            setServerEdits((prev) => ({
                              ...prev,
                              [server.id]: prev[server.id] ?? serverToSettingsForm(server),
                            }));
                            setSelectedServerId((current) => (current === server.id ? null : server.id));
                          }}
                        >
                          {expanded ? 'Hide details' : 'Show details'}
                        </button>
                      </div>
                    </div>

                    <div className="w-full space-y-2">
                      {server.description ? (
                        <div className="w-full text-sm muted">{server.description}</div>
                      ) : null}
                      <div className="min-h-[3.5rem] w-full">
                        {progressMessage ? (
                          <div
                            className={`w-full text-sm leading-6 ${
                              server.health_state === 'error' ||
                              server.observed_state === 'failed' ||
                              server.observed_state === 'error'
                                ? 'text-rose-200'
                                : 'text-amber-100'
                            }`}
                          >
                            {progressMessage}
                          </div>
                        ) : null}
                      </div>
                    </div>

                    {expanded ? (
                      <>
                        <div className="grid gap-3 text-sm sm:grid-cols-2 xl:grid-cols-4">
                          <div>
                            <div className="muted">Owner</div>
                            <div>{server.owner_display_name}</div>
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
                            <div className="muted">Port</div>
                            <div>{server.listen_host}:{server.listen_port}</div>
                          </div>
                          <div>
                            <div className="muted">Memory</div>
                            <div>{server.min_memory_mb}-{server.max_memory_mb} MB</div>
                          </div>
                          <div>
                            <div className="muted">Players</div>
                            <div>{server.current_player_count} / {server.max_player_count ?? '—'}</div>
                          </div>
                          <div>
                            <div className="muted">Last started</div>
                            <div>{formatTs(server.last_started_ts)}</div>
                          </div>
                          <div>
                            <div className="muted">Last stopped</div>
                            <div>{formatTs(server.last_stopped_ts)}</div>
                          </div>
                        </div>

                        {me.role === 'admin' ? (
                          <div className="space-y-4 rounded-2xl border border-[var(--border)] bg-[var(--panel)]/45 p-4">
                            <div className="space-y-1">
                              <h4 className="text-sm font-semibold uppercase tracking-[0.18em] text-white/80">
                                Server settings
                              </h4>
                              <p className="text-sm muted">
                                Save writes the managed Minecraft configuration to the Debian host. If the server is already running, restart it to apply all runtime changes cleanly.
                              </p>
                            </div>

                            <div className="grid gap-4 sm:grid-cols-2">
                              <label className="space-y-2">
                                <span className="text-sm font-medium text-white">Gamemode</span>
                                <select
                                  className="select rounded-xl px-4 py-3"
                                  value={(serverEdits[server.id] ?? serverToSettingsForm(server)).gamemode}
                                  onChange={(event) =>
                                    setServerEdit(
                                      server.id,
                                      'gamemode',
                                      event.target.value as ServerSettingsFormState['gamemode'],
                                    )
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
                                  value={(serverEdits[server.id] ?? serverToSettingsForm(server)).difficulty}
                                  onChange={(event) =>
                                    setServerEdit(
                                      server.id,
                                      'difficulty',
                                      event.target.value as ServerSettingsFormState['difficulty'],
                                    )
                                  }
                                >
                                  <option value="peaceful">Peaceful</option>
                                  <option value="easy">Easy</option>
                                  <option value="normal">Normal</option>
                                  <option value="hard">Hard</option>
                                </select>
                              </label>

                              <label className="space-y-2">
                                <span className="text-sm font-medium text-white">Max players</span>
                                <input
                                  className="input rounded-xl px-4 py-3"
                                  type="number"
                                  min={1}
                                  max={500}
                                  value={(serverEdits[server.id] ?? serverToSettingsForm(server)).max_player_count}
                                  onChange={(event) => setServerEdit(server.id, 'max_player_count', event.target.value)}
                                />
                              </label>

                              <label className="space-y-2 sm:col-span-2">
                                <span className="text-sm font-medium text-white">Message of the day</span>
                                <input
                                  className="input rounded-xl px-4 py-3"
                                  value={(serverEdits[server.id] ?? serverToSettingsForm(server)).motd}
                                  onChange={(event) => setServerEdit(server.id, 'motd', event.target.value)}
                                />
                              </label>
                            </div>

                            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                              {SERVER_SETTINGS_TOGGLE_FIELDS.map(({ key, label }) => (
                                <label
                                  key={key}
                                  className="panel-soft flex items-center gap-3 rounded-xl px-4 py-3 text-sm"
                                >
                                  <input
                                    type="checkbox"
                                    checked={(serverEdits[server.id] ?? serverToSettingsForm(server))[key]}
                                    onChange={(event) => setServerEdit(server.id, key, event.target.checked)}
                                  />
                                  <span>{label}</span>
                                </label>
                              ))}
                            </div>

                            <div className="flex justify-end">
                              <button
                                type="button"
                                className="btn-primary px-4 py-2 text-sm disabled:opacity-50"
                                disabled={savingServerId === server.id}
                                onClick={() => void handleSaveServerSettings(server)}
                              >
                                {savingServerId === server.id ? 'Saving…' : 'Save settings'}
                              </button>
                            </div>
                          </div>
                        ) : (
                          <div className="panel-soft rounded-xl px-4 py-3 text-sm muted">
                            Server settings can be changed by admins. Runtime controls remain available from the server row.
                          </div>
                        )}
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
                Only admins can create server records. Users can still start, stop, and restart
                visible servers from the known servers list.
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
              Only admins can provision, import, discover, or delete server data. Users can still
              control server runtime from the known servers list.
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
                  disabled={
                    !managementServer ||
                    provisioning ||
                    importing ||
                    actionLoading !== null ||
                    !(runtimeCapabilities?.provision_supported ?? true)
                  }
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
                    disabled={
                      !managementServer ||
                      importing ||
                      provisioning ||
                      actionLoading !== null ||
                      !(runtimeCapabilities?.import_supported ?? true)
                    }
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

      <ConfirmModal
        open={Boolean(deleteConfirmServer)}
        title="Delete Server"
        description="Delete this Minecraft server record and remove its managed host files? This action is destructive and cannot be undone."
        confirmLabel={
          deleteConfirmServer && deletingServerId === deleteConfirmServer.id
            ? 'Deleting…'
            : 'Delete server'
        }
        confirmDisabled={Boolean(deleteConfirmServer && deletingServerId === deleteConfirmServer.id)}
        cancelDisabled={Boolean(deleteConfirmServer && deletingServerId === deleteConfirmServer.id)}
        destructive
        maxWidthClassName="max-w-md"
        zIndexClassName="z-[150]"
        onCancel={() => setDeleteConfirmServer(null)}
        onConfirm={() => {
          if (!deleteConfirmServer) return;
          void handleDeleteServer(deleteConfirmServer);
        }}
      >
        {deleteConfirmServer ? (
          <div className="panel-soft rounded-xl px-4 py-3 text-sm">
            <div className="font-medium text-white">{deleteConfirmServer.display_name}</div>
            <div className="mt-1 text-xs muted">
              {deleteConfirmServer.server_distribution} {deleteConfirmServer.minecraft_version}
              {' · '}
              {deleteConfirmServer.world_name}
              {' · '}
              Port {deleteConfirmServer.listen_port}
            </div>
          </div>
        ) : null}
      </ConfirmModal>

      {hostBrowser.open ? (
        <div className="fixed inset-0 z-[150] flex items-center justify-center overflow-y-auto bg-black/70 p-4 sm:p-6">
          <div className="panel my-auto flex max-h-[calc(100vh-2rem)] w-full max-w-3xl flex-col gap-4 overflow-hidden rounded-2xl p-5 sm:max-h-[calc(100vh-3rem)] sm:p-6">
            <div className="flex shrink-0 items-center justify-between gap-3">
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
              <div className="max-h-24 shrink-0 overflow-y-auto">
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
              </div>
            ) : null}

            <div className="panel-soft flex shrink-0 items-center gap-2 rounded-xl border border-[var(--border)] px-3 py-2">
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
              <p className="shrink-0 text-sm text-red-300">{hostBrowser.error}</p>
            ) : null}

            <div className="panel-soft min-h-[260px] flex-1 overflow-auto rounded-xl border border-[var(--border)] p-2">
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

            <div className="flex shrink-0 items-center justify-end gap-2">
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
